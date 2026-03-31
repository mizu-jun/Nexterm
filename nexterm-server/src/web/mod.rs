//! Web ターミナル — axum WebSocket + xterm.js + TOTP 認証 + HTTPS/TLS
//!
//! # 設定例（nexterm.toml）
//! ```toml
//! [web]
//! enabled = true
//! port = 7681
//!
//! [web.auth]
//! totp_enabled = true
//! # totp_secret は初回セットアップ後に自動設定される
//!
//! [web.tls]
//! enabled = true
//! # cert_file / key_file を省略すると自己署名証明書を自動生成
//! ```

mod auth;
mod otp;
mod tls;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Form, Json, Router,
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
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
    /// セッション管理
    auth_mgr: Arc<auth::AuthManager>,
    /// 初回セットアップ待ちシークレット（未設定時のみ Some）
    pending_setup: Arc<Mutex<Option<PendingSetup>>>,
    totp_enabled: bool,
    tls_enabled: bool,
    issuer: String,
}

// ── エントリポイント ──────────────────────────────────────────────────────────

/// Web サーバーを起動する
pub async fn start_web_server(config: WebConfig, manager: Arc<SessionManager>) {
    let totp_enabled = config.auth.totp_enabled;
    let tls_enabled = config.tls.enabled;
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
                // 初回: シークレットを生成してブラウザでセットアップ
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

    let state = AppState {
        manager,
        legacy_token: config.token,
        totp: Arc::new(tokio::sync::RwLock::new(active_totp)),
        auth_mgr: Arc::new(auth::AuthManager::new()),
        pending_setup: Arc::new(Mutex::new(pending_setup)),
        totp_enabled,
        tls_enabled,
        issuer,
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

/// TOTP 認証が必要な状況でセッションが有効かを確認する
fn has_valid_session(state: &AppState, headers: &HeaderMap) -> bool {
    if !state.totp_enabled {
        return true; // TOTP 無効 → 認証不要
    }
    auth::extract_session_cookie(headers)
        .map(|token| state.auth_mgr.is_valid(&token))
        .unwrap_or(false)
}

// ── ページハンドラ ────────────────────────────────────────────────────────────

/// GET / — メイン画面（TOTP 有効時は未認証をログインページへリダイレクト）
async fn serve_index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if state.totp_enabled && !has_valid_session(&state, &headers) {
        return redirect("/login");
    }
    serve_asset("index.html")
}

/// GET /login — ログインページ
async fn serve_login() -> Response {
    serve_asset("login.html")
}

/// GET /setup — 初回 TOTP セットアップページ
async fn serve_setup(State(state): State<AppState>) -> Response {
    // セットアップが不要な場合はトップへリダイレクト
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
    Form(form): Form<LoginForm>,
) -> Response {
    let totp_guard = state.totp.read().await;
    let totp = match totp_guard.as_ref() {
        Some(t) => t,
        None => return redirect("/login?error=not_configured"),
    };

    if !totp.verify(&form.code) {
        warn!("TOTP ログイン失敗: 無効なコード");
        return redirect("/login?error=invalid_code");
    }

    let token = state.auth_mgr.create_session();
    let cookie = auth::make_session_cookie(&token, state.tls_enabled);
    Response::builder()
        .status(302)
        .header("Location", "/")
        .header("Set-Cookie", cookie)
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
    Form(form): Form<LoginForm>,
) -> Response {
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

    // シークレットを設定ファイルに保存する
    if let Err(e) = otp::save_secret_to_config(&secret_clone) {
        warn!("TOTP シークレットの保存に失敗: {}。インメモリのみで動作します。", e);
    }

    // アクティブな TOTP マネージャーに昇格させる
    match otp::TotpManager::from_secret(&secret_clone, &state.issuer) {
        Ok(mgr) => {
            *state.totp.write().await = Some(mgr);
            *state.pending_setup.lock().unwrap() = None;
            info!("TOTP セットアップが完了しました");
        }
        Err(e) => {
            warn!("TOTP マネージャーの作成に失敗: {}", e);
            return redirect("/setup?error=internal");
        }
    }

    // セットアップ完了後はセッションを発行してトップへ
    let token = state.auth_mgr.create_session();
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
    // TOTP セッション確認
    if state.totp_enabled && !has_valid_session(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // 後方互換: クエリパラメータのトークン確認
    if let Some(ref expected) = state.legacy_token {
        if query.token != *expected {
            warn!("WebSocket 認証失敗: 無効なトークン");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

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
                        if let Some(text) = pty_message_to_text(&msg) {
                            if socket.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
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
