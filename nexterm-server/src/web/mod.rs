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
//!
//! # 内部構成（Sprint 5-4 / A3）
//!
//! 旧 `web/mod.rs` (1,088 行) を以下に再分割した:
//! - [`router`] — ルーター構築 + HTTP/TLS サーバー起動
//! - [`middleware`] — 認証確認 + クライアント IP + HTTPS リダイレクト
//! - [`handlers`] — HTTP / WebSocket ハンドラ群（page / login / oauth / ws / assets）

mod access_log;
mod auth;
mod handlers;
mod middleware;
mod oauth;
mod otp;
mod rate_limit;
mod router;
mod tls;

use std::sync::{Arc, Mutex};

use nexterm_config::WebConfig;
use rust_embed::Embed;
use tracing::{info, warn};

use crate::session::SessionManager;

// ── 静的ファイル埋め込み ─────────────────────────────────────────────────────

#[derive(Embed)]
#[folder = "static/"]
pub(in crate::web) struct Assets;

// ── 共有状態 ─────────────────────────────────────────────────────────────────

/// セットアップ未完了時の一時シークレット
pub(in crate::web) struct PendingSetup {
    pub(in crate::web) secret: String,
    pub(in crate::web) totp: otp::TotpManager,
}

#[derive(Clone)]
pub(in crate::web) struct AppState {
    pub(in crate::web) manager: Arc<SessionManager>,
    /// 後方互換: URL クエリパラメータによるトークン認証
    pub(in crate::web) legacy_token: Option<String>,
    /// アクティブな TOTP マネージャー（セットアップ完了後に設定）
    pub(in crate::web) totp: Arc<tokio::sync::RwLock<Option<otp::TotpManager>>>,
    /// セッション管理（TTL・同時接続数管理）
    pub(in crate::web) auth_mgr: Arc<auth::AuthManager>,
    /// 初回セットアップ待ちシークレット（未設定時のみ Some）
    pub(in crate::web) pending_setup: Arc<Mutex<Option<PendingSetup>>>,
    pub(in crate::web) totp_enabled: bool,
    /// OAuth2 マネージャー（OAuth 有効時のみ Some）
    pub(in crate::web) oauth_mgr: Option<Arc<oauth::OAuthManager>>,
    pub(in crate::web) tls_enabled: bool,
    pub(in crate::web) force_https: bool,
    pub(in crate::web) issuer: String,
    /// アクセスログライター
    pub(in crate::web) access_logger: Arc<access_log::AccessLogger>,
    /// TOTP ログインのレート制限（IP ベース、5 試行/分）
    pub(in crate::web) totp_rate_limiter: Arc<rate_limit::RateLimiter>,
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
                    warn!(
                        "TOTP シークレットが不正です: {}。TOTP 認証を無効化します。",
                        e
                    );
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
                        let pending = PendingSetup {
                            secret,
                            totp: setup_totp,
                        };
                        (None, Some(pending))
                    }
                    Err(e) => {
                        warn!(
                            "セットアップ用 TOTP の生成に失敗: {}。TOTP 認証を無効化します。",
                            e
                        );
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
            redirect_base.trim_end_matches("/auth/callback").to_string()
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
    let access_logger = Arc::new(access_log::AccessLogger::new(&config.access_log));

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
        totp_rate_limiter: Arc::new(rate_limit::RateLimiter::new(
            rate_limit::RateLimitConfig::totp_default(),
        )),
    };

    let app = router::build_router(state);
    let addr_str = format!("0.0.0.0:{}", config.port);
    let addr: std::net::SocketAddr = addr_str.parse().expect("invalid bind address");

    if tls_enabled {
        match tls::load_or_generate(
            config.tls.cert_file.as_deref(),
            config.tls.key_file.as_deref(),
        ) {
            Ok((cert_pem, key_pem)) => {
                info!(
                    "Web ターミナルを起動します (HTTPS): https://localhost:{}",
                    config.port
                );
                router::start_tls_server(addr, app, cert_pem, key_pem).await;
            }
            Err(e) => {
                // CRITICAL #3: TLS 失敗時の平文 HTTP フォールバックは
                // セッショントークン・TOTP コード・パスワード等の漏洩リスクが
                // あるため、明示的なオプトインがない限り起動を中止する。
                if config.allow_http_fallback {
                    warn!(
                        "証明書の読み込みに失敗: {}。allow_http_fallback=true のため HTTP にフォールバックします（推奨されない）。",
                        e
                    );
                    router::start_plain_http(addr, app).await;
                } else {
                    tracing::error!(
                        "証明書の読み込みに失敗: {}。Web サーバーの起動を中止します。\n\
                         HTTP フォールバックを許可するには [web] allow_http_fallback = true を設定してください（テスト・開発時のみ推奨）。",
                        e
                    );
                    // 起動中止: caller (start_web_server) は spawn された task のため
                    // ここで関数から抜ければ Web サーバーは起動しない（メインの IPC は継続）
                }
            }
        }
    } else {
        info!(
            "Web ターミナルを起動します: http://localhost:{}",
            config.port
        );
        router::start_plain_http(addr, app).await;
    }
}
