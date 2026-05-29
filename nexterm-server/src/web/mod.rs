//! Web terminal — axum WebSocket + xterm.js + TOTP / OAuth2 authentication + HTTPS/TLS.
//!
//! # Configuration example (nexterm.toml)
//! ```toml
//! [web]
//! enabled = true
//! port = 7681
//! force_https = true   # redirect HTTP requests to HTTPS
//! max_sessions = 10    # max simultaneous sessions
//!
//! [web.auth]
//! session_timeout_secs = 86400  # session lifetime (seconds)
//!
//! # ── TOTP authentication ──────────────────────────────────────
//! totp_enabled = true
//!
//! # ── OAuth2 authentication ────────────────────────────────────
//! [web.auth.oauth]
//! enabled = true
//! provider = "github"        # "github" | "google" | "azure" | "oidc"
//! client_id = "xxx"
//! # client_secret: prefer the NEXTERM_OAUTH_CLIENT_SECRET environment variable.
//! allowed_emails = ["admin@example.com"]
//! # allowed_orgs = ["my-org"]  # GitHub only
//!
//! # ── Access log ───────────────────────────────────────────────
//! [web.access_log]
//! enabled = true
//! file = "/var/log/nexterm/access.csv"  # falls back to server log when omitted
//!
//! [web.tls]
//! enabled = true
//! # Omitting cert_file / key_file auto-generates a self-signed certificate.
//! ```
//!
//! # Internal layout (Sprint 5-4 / A3)
//!
//! Splits the old `web/mod.rs` (1,088 lines) into:
//! - [`router`] — router build + HTTP/TLS server startup.
//! - [`middleware`] — auth check + client IP + HTTPS redirect.
//! - [`handlers`] — HTTP / WebSocket handlers (page / login / oauth / ws / assets).

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

// ── Embedded static files ────────────────────────────────────────────────────

#[derive(Embed)]
#[folder = "static/"]
pub(in crate::web) struct Assets;

// ── Shared state ─────────────────────────────────────────────────────────────

/// Temporary secret used while setup is pending.
pub(in crate::web) struct PendingSetup {
    pub(in crate::web) secret: String,
    pub(in crate::web) totp: otp::TotpManager,
}

#[derive(Clone)]
pub(in crate::web) struct AppState {
    pub(in crate::web) manager: Arc<SessionManager>,
    /// Backward compatibility: token-based authentication via URL query parameter.
    pub(in crate::web) legacy_token: Option<String>,
    /// Active TOTP manager (set after setup completes).
    pub(in crate::web) totp: Arc<tokio::sync::RwLock<Option<otp::TotpManager>>>,
    /// Session management (TTL, concurrent connection limit).
    pub(in crate::web) auth_mgr: Arc<auth::AuthManager>,
    /// Pending initial-setup secret (`Some` only when not yet configured).
    pub(in crate::web) pending_setup: Arc<Mutex<Option<PendingSetup>>>,
    pub(in crate::web) totp_enabled: bool,
    /// OAuth2 manager (`Some` only when OAuth is enabled).
    pub(in crate::web) oauth_mgr: Option<Arc<oauth::OAuthManager>>,
    pub(in crate::web) tls_enabled: bool,
    pub(in crate::web) force_https: bool,
    pub(in crate::web) issuer: String,
    /// Access log writer.
    pub(in crate::web) access_logger: Arc<access_log::AccessLogger>,
    /// Rate limit on TOTP login (IP-based, 5 attempts/min).
    pub(in crate::web) totp_rate_limiter: Arc<rate_limit::RateLimiter>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Start the web server.
pub async fn start_web_server(config: WebConfig, manager: Arc<SessionManager>) {
    let totp_enabled = config.auth.totp_enabled;
    let tls_enabled = config.tls.enabled;
    let force_https = config.force_https;
    let issuer = config.auth.issuer.clone();

    // Initialize the TOTP manager.
    let (active_totp, pending_setup) = if totp_enabled {
        match &config.auth.totp_secret {
            Some(secret) => match otp::TotpManager::from_secret(secret, &issuer) {
                Ok(mgr) => {
                    info!("TOTP authentication is enabled");
                    (Some(mgr), None)
                }
                Err(e) => {
                    warn!("invalid TOTP secret: {}; disabling TOTP authentication.", e);
                    (None, None)
                }
            },
            None => {
                let secret = otp::TotpManager::generate_secret();
                info!(
                    "TOTP authentication is enabled but no secret is configured. \
                    Open http(s)://localhost:{}/setup in a browser to complete setup.",
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
                            "failed to generate setup TOTP: {}; disabling TOTP authentication.",
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

    // Initialize the OAuth2 manager.
    let oauth_mgr = if config.auth.oauth.enabled {
        let scheme = if tls_enabled { "https" } else { "http" };
        let redirect_base = config
            .auth
            .oauth
            .redirect_url
            .clone()
            .unwrap_or_else(|| format!("{}://localhost:{}", scheme, config.port));
        let redirect_base = if redirect_base.contains("/auth/callback") {
            // When redirect_url is a full callback URL, extract the base.
            redirect_base.trim_end_matches("/auth/callback").to_string()
        } else {
            redirect_base
        };

        info!(
            "OAuth2 authentication is enabled (provider: {})",
            config.auth.oauth.provider
        );
        Some(Arc::new(oauth::OAuthManager::new(
            config.auth.oauth.clone(),
            redirect_base,
        )))
    } else {
        None
    };

    // Initialize the access log writer.
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
                    "starting web terminal (HTTPS): https://localhost:{}",
                    config.port
                );
                router::start_tls_server(addr, app, cert_pem, key_pem).await;
            }
            Err(e) => {
                // CRITICAL #3: falling back to plain HTTP on TLS failure risks leaking session
                // tokens, TOTP codes, passwords, and other secrets. Abort startup unless the
                // operator explicitly opts in.
                if config.allow_http_fallback {
                    warn!(
                        "failed to load certificate: {}; falling back to HTTP because allow_http_fallback=true (not recommended).",
                        e
                    );
                    router::start_plain_http(addr, app).await;
                } else {
                    tracing::error!(
                        "failed to load certificate: {}; aborting web server startup.\n\
                         To allow HTTP fallback set [web] allow_http_fallback = true (recommended only for testing/development).",
                        e
                    );
                    // Abort startup: this function runs inside a spawned task, so returning here
                    // leaves the web server unstarted (the main IPC continues to run).
                }
            }
        }
    } else {
        info!("starting web terminal: http://localhost:{}", config.port);
        router::start_plain_http(addr, app).await;
    }
}
