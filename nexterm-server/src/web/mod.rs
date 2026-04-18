//! Web ターミナル — axum WebSocket + xterm.js + TOTP / OAuth2 認証 + HTTPS/TLS
//!
//! # 設定例（nexterm.toml）
//! ```toml
//! [web]
//! enabled = true
//! port = 7681
//! force_https = true   # HTTP アクセスを HTTPS へリダイレクト
//! max_sessions = 10    # 同時セッション数上限
//!
//! [web.auth]
//! session_timeout_secs = 86400  # セッション有効期限（秒）
//!
//! # ── TOTP 認証 ──────────────────────────────────────────
//! totp_enabled = true
//!
//! # ── OAuth2 認証 ────────────────────────────────────────
//! [web.auth.oauth]
//! enabled = true
//! provider = "github"        # "github" | "google" | "azure" | "oidc"
//! client_id = "xxx"
//! # client_secret は環境変数 NEXTERM_OAUTH_CLIENT_SECRET を推奨
//! allowed_emails = ["admin@example.com"]
//! # allowed_orgs = ["my-org"]  # GitHub のみ
//!
//! # ── アクセスログ ────────────────────────────────────────
//! [web.access_log]
//! enabled = true
//! file = "/var/log/nexterm/access.csv"  # 省略時はサーバーログへ出力
//!
//! [web.tls]
//! enabled = true
//! # cert_file / key_file を省略すると自己署名証明書を自動生成
//! ```

mod access_log;
mod auth;
mod oauth;
mod otp;
mod tls;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Form, Json, Router,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use nexterm_config::WebConfig;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::{info, warn};

use crate::session::SessionManager;

// ── 静的ファイル埋め込み ─────────────────────────────────────────────────────

#[derive(Embed)]
#[folder = "static/"]
struct Assets;

// ── 共有状態 ─────────────────────────────────────────────────────────────────

/// セットアップ未完了時の一時シークレット
struct PendingSetup {
    secret: String,
    totp: otp::TotpManager,
}

#[derive(Clone)]
struct AppState {
    manager: Arc<SessionManager>,
    /// 後方互換: URL クエリパラメータによるトークン認証
    legacy_token: Option<String>,
    /// アクティブな TOTP マネージャー（セットアップ完了後に設定）
    totp: Arc<tokio::sync::RwLock<Option<otp::TotpManager>>>,
    /// セッション管理（TTL・同時接続数管理）
    auth_mgr: Arc<auth::AuthManager>,
    /// 初回セットアップ待ちシークレット（未設定時のみ Some）
    pending_setup: Arc<Mutex<Option<PendingSetup>>>,
    totp_enabled: bool,
    /// OAuth2 マネージャー（OAuth 有効時のみ Some）
    oauth_mgr: Option<Arc<oauth::OAuthManager>>,
    tls_enabled: bool,
    force_https: bool,
    issuer: String,
    /// アクセスログライター
    access_logger: Arc<access_log::AccessLogger>,
}

// ── エントリポイント ──────────────────────────────────────────────────────────

/// Web サーバーを起動する
pub async fn start_web_server(config: WebConfig, manager: Arc<SessionManager>) {
    let totp_enabled = config.auth.totp_enabled;
    let tls_enabled = config.tls.enabled;
    let force_https = config.force_https;
    let issuer = config.auth.issuer.clone();

    // TOTP マネージャーの初期化
    let (active_totp, pending_setup) = if totp_enabled {
        match &config.auth.totp_secret {
            Some(secret) => match otp::TotpManager::from_secret(secret, &issuer) {
                Ok(mgr) => {
                    info!("TOTP 認証が有効です");
                    (Some(mgr), None)
                }
                Err(e) => {
                    warn!("TOTP シークレットが不正です: {}。TOTP 認証を無効化します。", e);
                    (None, None)
                }
            },
            None => {
                let secret = otp::TotpManager::generate_secret();
                info!(
                    "TOTP 認証が有効ですが、シークレットが未設定です。\
                    ブラウザで http(s)://localhost:{}/setup を開いてセットアップしてください。",
                    config.port
                );
                match otp::TotpManager::from_secret(&secret, &issuer) {
                    Ok(setup_totp) => {
                        let pending = PendingSetup { secret, totp: setup_totp };
                        (None, Some(pending))
                    }
                    Err(e) => {
                        warn!("セットアップ用 TOTP の生成に失敗: {}。TOTP 認証を無効化します。", e);
                        (None, None)
                    }
                }
            }
        }
    } else {
        (None, None)
    };

    // OAuth2 マネージャーの初期化
    let oauth_mgr = if config.auth.oauth.enabled {
        let scheme = if tls_enabled { "https" } else { "http" };
        let redirect_base = config
            .auth
            .oauth
            .redirect_url
            .clone()
            .unwrap_or_else(|| format!("{}://localhost:{}", scheme, config.port));
        let redirect_base = if redirect_base.contains("/auth/callback") {
            // redirect_url が完全な callback URL の場合はベースを抽出する
            redirect_base
                .trim_end_matches("/auth/callback")
                .to_string()
        } else {
            redirect_base
        };

        info!(
            "OAuth2 認証が有効です（プロバイダー: {}）",
            config.auth.oauth.provider
        );
        Some(Arc::new(oauth::OAuthManager::new(
            config.auth.oauth.clone(),
            redirect_base,
        )))
    } else {
        None
    };

    // アクセスログライターの初期化
    let access_logger = Arc::new(access_log::AccessLogger::new(
        config.access_log.enabled,
        config.access_log.file.as_deref(),
    ));

    let state = AppState {
        manager,
        legacy_token: config.token,
        totp: Arc::new(tokio::sync::RwLock::new(active_totp)),
        auth_mgr: Arc::new(auth::AuthManager::new(
            config.auth.session_timeout_secs,
            config.max_sessions,
        )),
        pending_setup: Arc::new(Mutex::new(pending_setup)),
        totp_enabled,
        oauth_mgr,
        tls_enabled,
        force_https,
        issuer,
        access_logger,
    };

    let app = build_router(state);
    let addr_str = format!("0.0.0.0:{}", config.port);
    let addr: SocketAddr = addr_str.parse().expect("invalid bind address");

    if tls_enabled {
        match tls::load_or_generate(
            config.tls.cert_file.as_deref(),
            config.tls.key_file.as_deref(),
        ) {
            Ok((cert_pem, key_pem)) => {
                info!("Web ターミナルを起動します (HTTPS): https://localhost:{}", config.port);
                start_tls_server(addr, app, cert_pem, key_pem).await;
            }
            Err(e) => {
                warn!("証明書の読み込みに失敗: {}。HTTP にフォールバックします。", e);
                start_plain_http(addr, app).await;
            }
        }
    } else {
        info!("Web ターミナルを起動します: http://localhost:{}", config.port);
        start_plain_http(addr, app).await;
    }
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/login", get(serve_login))
        .route("/setup", get(serve_setup))
        .route("/auth/login", post(handle_login))
        .route("/auth/oauth", get(handle_oauth_redirect))
        .route("/auth/callback", get(handle_oauth_callback))
        .route("/auth/logout", post(handle_logout))
        .route("/auth/setup-url", get(handle_setup_url))
        .route("/setup/verify", post(handle_setup_verify))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn start_plain_http(addr: SocketAddr, app: Router) {
    match TcpListener::bind(&addr).await {
        Ok(listener) => {
            if let Err(e) = axum::serve(listener, app).await {
                warn!("Web サーバーエラー: {}", e);
            }
        }
        Err(e) => {
            warn!("Web サーバーのバインドに失敗: {}: {}", addr, e);
        }
    }
}

async fn start_tls_server(addr: SocketAddr, app: Router, cert_pem: Vec<u8>, key_pem: Vec<u8>) {
    use std::sync::Arc;
    use hyper_util::{
        rt::{TokioExecutor, TokioIo},
        server::conn::auto::Builder,
        service::TowerToHyperService,
    };

    // PEM 証明書を解析する
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = {
        let mut reader = std::io::BufReader::new(cert_pem.as_slice());
        rustls_pemfile::certs(&mut reader)
            .filter_map(|r| r.ok())
            .collect()
    };
    let private_key = {
        let mut reader = std::io::BufReader::new(key_pem.as_slice());
        match rustls_pemfile::private_key(&mut reader) {
            Ok(Some(k)) => k,
            _ => {
                warn!("TLS: 秘密鍵の解析に失敗しました。HTTP にフォールバックします。");
                start_plain_http(addr, app).await;
                return;
            }
        }
    };

    let tls_config = match rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, private_key)
    {
        Ok(c) => Arc::new(c),
        Err(e) => {
            warn!("TLS 設定エラー: {}。HTTP にフォールバックします。", e);
            start_plain_http(addr, app).await;
            return;
        }
    };

    let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            warn!("TLS バインドに失敗: {}: {}", addr, e);
            return;
        }
    };

    loop {
        let (tcp_stream, _remote_addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                warn!("TCP accept エラー: {}", e);
                continue;
            }
        };

        let acceptor = acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(tcp_stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TLS ハンドシェイクエラー: {}", e);
                    return;
                }
            };
            let io = TokioIo::new(tls_stream);
            let service = TowerToHyperService::new(app);
            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(io, service)
                .await
            {
                tracing::debug!("HTTP 接続エラー: {}", e);
            }
        });
    }
}

// ── 認証ヘルパー ──────────────────────────────────────────────────────────────

/// 認証が必要な状況でセッションが有効かを確認する
///
/// TOTP と OAuth の両方が無効の場合は認証不要として true を返す。
fn has_valid_session(state: &AppState, headers: &HeaderMap) -> bool {
    let auth_required = state.totp_enabled || state.oauth_mgr.is_some();
    if !auth_required {
        return true;
    }
    auth::extract_session_cookie(headers)
        .map(|token| state.auth_mgr.is_valid(&token))
        .unwrap_or(false)
}

/// リクエストヘッダーからクライアント IP を取得する
///
/// X-Forwarded-For → X-Real-IP → "unknown" の順で試みる。
fn client_ip(headers: &HeaderMap) -> String {
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        // X-Forwarded-For は "IP1, IP2, ..." の形式なので最初のエントリを使う
        return v.split(',').next().unwrap_or(v).trim().to_string();
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        return v.to_string();
    }
    "unknown".to_string()
}

// ── force_https リダイレクト ──────────────────────────────────────────────────

/// HTTP リクエストを HTTPS へリダイレクトするレスポンスを返す
fn https_redirect(headers: &HeaderMap, port: u16) -> Response {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    // ポート部分を除去して HTTPS ポートに置き換える
    let host_no_port = host.split(':').next().unwrap_or(host);
    let location = format!("https://{}:{}/", host_no_port, port);
    Response::builder()
        .status(301)
        .header("Location", location)
        .body(axum::body::Body::empty())
        .unwrap()
}

// ── ページハンドラ ────────────────────────────────────────────────────────────

/// GET / — メイン画面（未認証はログインページへリダイレクト）
async fn serve_index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // force_https: TLS 無効または既に HTTPS の場合は無視する
    // ここでは簡易チェック（X-Forwarded-Proto ヘッダーを確認）
    if state.force_https && !state.tls_enabled {
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");
        if proto != "https" {
            return https_redirect(&headers, 7681); // デフォルトポート
        }
    }

    if !has_valid_session(&state, &headers) {
        return redirect("/login");
    }
    serve_asset("index.html")
}

/// GET /login — ログインページ
async fn serve_login(State(state): State<AppState>) -> Response {
    serve_login_html(&state)
}

/// ログインページ HTML を動的に生成する（TOTP / OAuth ボタンの表示制御）
fn serve_login_html(state: &AppState) -> Response {
    let oauth_button = if let Some(ref oauth_mgr) = state.oauth_mgr {
        // OAuth ボタンを表示するために認証 URL を生成する
        match oauth_mgr.authorization_url() {
            Ok(url) => {
                let provider_label = "OAuth でログイン";
                format!(
                    r#"<div class="oauth-section">
  <div class="or-divider"><span>または</span></div>
  <a href="{}" class="oauth-btn">{}</a>
</div>"#,
                    url, provider_label
                )
            }
            Err(e) => {
                warn!("OAuth URL 生成エラー: {}", e);
                String::new()
            }
        }
    } else {
        String::new()
    };

    // login.html テンプレートに OAuth セクションを埋め込む
    let base_html = match Assets::get("login.html") {
        Some(file) => String::from_utf8_lossy(file.data.as_ref()).into_owned(),
        None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let html = base_html.replace("<!-- OAUTH_SECTION -->", &oauth_button);
    Html(html).into_response()
}

/// GET /setup — 初回 TOTP セットアップページ
async fn serve_setup(State(state): State<AppState>) -> Response {
    if state.pending_setup.lock().unwrap().is_none() {
        return redirect("/");
    }
    serve_asset("setup.html")
}

// ── 認証 API ─────────────────────────────────────────────────────────────────

/// ログインフォームのフィールド
#[derive(Deserialize)]
struct LoginForm {
    code: String,
}

/// POST /auth/login — TOTP コードを検証してセッションを発行する
async fn handle_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let addr = client_ip(&headers);
    let totp_guard = state.totp.read().await;
    let totp = match totp_guard.as_ref() {
        Some(t) => t,
        None => {
            state.access_logger.log(&access_log::AccessLogEntry {
                remote_addr: addr.clone(),
                method: "POST".to_string(),
                path: "/auth/login".to_string(),
                status: 302,
                auth_method: "totp".to_string(),
                user_id: String::new(),
            });
            return redirect("/login?error=not_configured");
        }
    };

    if !totp.verify(&form.code) {
        warn!("TOTP ログイン失敗: 無効なコード（{}）", addr);
        state.access_logger.log(&access_log::AccessLogEntry {
            remote_addr: addr.clone(),
            method: "POST".to_string(),
            path: "/auth/login".to_string(),
            status: 401,
            auth_method: "totp".to_string(),
            user_id: String::new(),
        });
        return redirect("/login?error=invalid_code");
    }

    let token = match state.auth_mgr.create_session("totp", "") {
        Some(t) => t,
        None => return redirect("/login?error=session_limit"),
    };
    let cookie = auth::make_session_cookie(&token, state.tls_enabled);

    info!("TOTP ログイン成功（{}）", addr);
    state.access_logger.log(&access_log::AccessLogEntry {
        remote_addr: addr,
        method: "POST".to_string(),
        path: "/auth/login".to_string(),
        status: 302,
        auth_method: "totp".to_string(),
        user_id: String::new(),
    });

    Response::builder()
        .status(302)
        .header("Location", "/")
        .header("Set-Cookie", cookie)
        .body(axum::body::Body::empty())
        .unwrap()
}

/// GET /auth/oauth — OAuth プロバイダーの認証ページへリダイレクト
async fn handle_oauth_redirect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let addr = client_ip(&headers);
    let oauth_mgr = match state.oauth_mgr.as_ref() {
        Some(m) => m,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    match oauth_mgr.authorization_url() {
        Ok(url) => {
            info!("OAuth 認証開始（{}）", addr);
            redirect(&url)
        }
        Err(e) => {
            warn!("OAuth URL 生成エラー: {}", e);
            redirect("/login?error=oauth_config")
        }
    }
}

/// OAuth2 コールバッククエリパラメータ
#[derive(Deserialize)]
struct OAuthCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// GET /auth/callback — OAuth2 コールバック処理
async fn handle_oauth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallback>,
) -> Response {
    let addr = client_ip(&headers);
    let oauth_mgr = match state.oauth_mgr.as_ref() {
        Some(m) => m.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // プロバイダーからのエラーを処理する
    if let Some(err) = query.error {
        warn!(
            "OAuth エラー: {} — {} （{}）",
            err,
            query.error_description.as_deref().unwrap_or(""),
            addr
        );
        return redirect("/login?error=oauth_denied");
    }

    let code = match query.code {
        Some(c) => c,
        None => return redirect("/login?error=oauth_no_code"),
    };
    let oauth_state = match query.state {
        Some(s) => s,
        None => return redirect("/login?error=oauth_no_state"),
    };

    // コードを access_token に交換してユーザー情報を取得する
    let user = match oauth_mgr.exchange_code(code, oauth_state).await {
        Ok(u) => u,
        Err(e) => {
            warn!("OAuth コード交換失敗: {} （{}）", e, addr);
            state.access_logger.log(&access_log::AccessLogEntry {
                remote_addr: addr.clone(),
                method: "GET".to_string(),
                path: "/auth/callback".to_string(),
                status: 401,
                auth_method: "oauth".to_string(),
                user_id: String::new(),
            });
            return redirect("/login?error=oauth_exchange");
        }
    };

    // アクセス許可チェック
    if !oauth_mgr.is_user_allowed(&user).await {
        warn!(
            "OAuth アクセス拒否: user_id={} （{}）",
            user.user_id, addr
        );
        state.access_logger.log(&access_log::AccessLogEntry {
            remote_addr: addr.clone(),
            method: "GET".to_string(),
            path: "/auth/callback".to_string(),
            status: 403,
            auth_method: format!("oauth:{}", user.provider),
            user_id: user.user_id.clone(),
        });
        return redirect("/login?error=oauth_forbidden");
    }

    let auth_method = format!("oauth:{}", user.provider);
    let user_id = user.login.as_deref().unwrap_or(&user.user_id).to_string();

    let token = match state.auth_mgr.create_session(&auth_method, &user_id) {
        Some(t) => t,
        None => return redirect("/login?error=session_limit"),
    };
    let cookie = auth::make_session_cookie(&token, state.tls_enabled);

    info!(
        "OAuth ログイン成功: {} ({}) （{}）",
        user_id, user.provider, addr
    );
    state.access_logger.log(&access_log::AccessLogEntry {
        remote_addr: addr,
        method: "GET".to_string(),
        path: "/auth/callback".to_string(),
        status: 302,
        auth_method,
        user_id,
    });

    Response::builder()
        .status(302)
        .header("Location", "/")
        .header("Set-Cookie", cookie)
        .body(axum::body::Body::empty())
        .unwrap()
}

/// POST /auth/logout — セッションを破棄してログインページへリダイレクト
async fn handle_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(token) = auth::extract_session_cookie(&headers) {
        state.auth_mgr.revoke_session(&token);
    }
    Response::builder()
        .status(302)
        .header("Location", "/login")
        .header("Set-Cookie", auth::make_logout_cookie())
        .body(axum::body::Body::empty())
        .unwrap()
}

/// セットアップ URL レスポンス
#[derive(Serialize)]
struct SetupUrlResponse {
    url: String,
    secret: String,
}

/// GET /auth/setup-url — セットアップ用の otpauth:// URL とシークレットを返す
async fn handle_setup_url(State(state): State<AppState>) -> Response {
    let guard = state.pending_setup.lock().unwrap();
    match guard.as_ref() {
        Some(ps) => Json(SetupUrlResponse {
            url: ps.totp.get_url(),
            secret: ps.totp.secret_b32().to_string(),
        })
        .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// POST /setup/verify — 初回 TOTP コードを検証してシークレットを保存する
async fn handle_setup_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let addr = client_ip(&headers);
    let (secret_clone, is_valid) = {
        let guard = state.pending_setup.lock().unwrap();
        match guard.as_ref() {
            Some(ps) => (ps.secret.clone(), ps.totp.verify(&form.code)),
            None => return redirect("/?setup=done"),
        }
    };

    if !is_valid {
        return redirect("/setup?error=invalid_code");
    }

    if let Err(e) = otp::save_secret_to_config(&secret_clone) {
        warn!("TOTP シークレットの保存に失敗: {}。インメモリのみで動作します。", e);
    }

    match otp::TotpManager::from_secret(&secret_clone, &state.issuer) {
        Ok(mgr) => {
            *state.totp.write().await = Some(mgr);
            *state.pending_setup.lock().unwrap() = None;
            info!("TOTP セットアップが完了しました（{}）", addr);
        }
        Err(e) => {
            warn!("TOTP マネージャーの作成に失敗: {}", e);
            return redirect("/setup?error=internal");
        }
    }

    let token = match state.auth_mgr.create_session("totp", "") {
        Some(t) => t,
        None => return redirect("/login?error=session_limit"),
    };
    let cookie = auth::make_session_cookie(&token, state.tls_enabled);
    Response::builder()
        .status(302)
        .header("Location", "/")
        .header("Set-Cookie", cookie)
        .body(axum::body::Body::empty())
        .unwrap()
}

// ── WebSocket ─────────────────────────────────────────────────────────────────

/// WebSocket クエリパラメータ
#[derive(Deserialize)]
struct WsQuery {
    #[serde(default = "default_session_name")]
    session: String,
    #[serde(default)]
    token: String,
}

fn default_session_name() -> String {
    "main".to_string()
}

/// GET /ws — WebSocket ハンドラ（PTY セッションへのブリッジ）
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let addr = client_ip(&headers);

    // セッション確認
    if !has_valid_session(&state, &headers) {
        state.access_logger.log(&access_log::AccessLogEntry {
            remote_addr: addr.clone(),
            method: "GET".to_string(),
            path: "/ws".to_string(),
            status: 401,
            auth_method: String::new(),
            user_id: String::new(),
        });
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // 後方互換: クエリパラメータのトークン確認
    if let Some(ref expected) = state.legacy_token
        && query.token != *expected {
            warn!("WebSocket 認証失敗: 無効なトークン（{}）", addr);
            return StatusCode::UNAUTHORIZED.into_response();
        }

    // アクセスログに WebSocket 接続を記録する
    let (auth_method, user_id) = auth::extract_session_cookie(&headers)
        .and_then(|t| state.auth_mgr.session_info(&t))
        .unwrap_or_default();

    state.access_logger.log(&access_log::AccessLogEntry {
        remote_addr: addr,
        method: "GET".to_string(),
        path: "/ws".to_string(),
        status: 101,
        auth_method,
        user_id,
    });

    let session_name = query.session.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, state.manager, session_name))
}

/// WebSocket 接続ごとの処理 — PTY 出力をブラウザに転送し、キー入力を PTY に転送する
async fn handle_socket(mut socket: WebSocket, manager: Arc<SessionManager>, session_name: String) {
    let _ = manager.get_or_create_and_attach(&session_name, 80, 24).await;

    let sessions_arc = manager.sessions();
    let mut rx = {
        let sessions = sessions_arc.lock().await;
        if let Some(session) = sessions.get(&session_name) {
            session.attach()
        } else {
            warn!("WebSocket: セッション '{}' が見つかりません", session_name);
            return;
        }
    };

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Some(text) = pty_message_to_text(&msg)
                            && socket.send(Message::Text(text)).await.is_err() {
                                break;
                            }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        let sessions = sessions_arc.lock().await;
                        if let Some(session) = sessions.get(&session_name) {
                            let _ = session.write_to_focused(text.as_bytes());
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        let sessions = sessions_arc.lock().await;
                        if let Some(session) = sessions.get(&session_name) {
                            let _ = session.write_to_focused(&data);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    info!("WebSocket 切断: セッション '{}'", session_name);
}

/// ServerToClient メッセージからテキスト出力を抽出する
fn pty_message_to_text(msg: &nexterm_proto::ServerToClient) -> Option<String> {
    use nexterm_proto::ServerToClient;
    match msg {
        ServerToClient::GridDiff { dirty_rows, .. } => {
            let text: String = dirty_rows
                .iter()
                .map(|row| {
                    let line: String = row.cells.iter().map(|c| c.ch).collect();
                    format!("\r{}\r\n", line)
                })
                .collect();
            if text.is_empty() { None } else { Some(text) }
        }
        ServerToClient::FullRefresh { grid, .. } => {
            let text: String = grid
                .rows
                .iter()
                .map(|row| {
                    let line: String = row.iter().map(|c| c.ch).collect();
                    format!("{}\r\n", line)
                })
                .collect();
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}

// ── ユーティリティ ────────────────────────────────────────────────────────────

fn serve_asset(name: &str) -> Response {
    match Assets::get(name) {
        Some(file) => Html(String::from_utf8_lossy(file.data.as_ref()).into_owned()).into_response(),
        None => Response::builder()
            .status(404)
            .body(axum::body::Body::from(format!("{} not found", name)))
            .unwrap(),
    }
}

fn redirect(location: &str) -> Response {
    Response::builder()
        .status(302)
        .header("Location", location)
        .body(axum::body::Body::empty())
        .unwrap()
}
