//! OAuth2 authentication handlers: redirect start + callback handling.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{info, warn};

use super::assets::redirect;
use crate::web::AppState;
use crate::web::access_log;
use crate::web::auth;
use crate::web::middleware::client_ip;

/// GET /auth/oauth — redirect to the OAuth provider's authorization page.
pub(in crate::web) async fn handle_oauth_redirect(
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
            info!("OAuth authorization start ({})", addr);
            redirect(&url)
        }
        Err(e) => {
            warn!("OAuth URL generation error: {}", e);
            redirect("/login?error=oauth_config")
        }
    }
}

/// OAuth2 callback query parameters.
#[derive(Deserialize)]
pub(in crate::web) struct OAuthCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// GET /auth/callback — OAuth2 callback handler.
pub(in crate::web) async fn handle_oauth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallback>,
) -> Response {
    let addr = client_ip(&headers);
    let oauth_mgr = match state.oauth_mgr.as_ref() {
        Some(m) => m.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // Handle errors from the provider.
    if let Some(err) = query.error {
        warn!(
            "OAuth error: {} — {} ({})",
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

    // Exchange the code for an access token and fetch user info.
    // The access token is also required for GitHub org membership verification.
    let (user, access_token) = match oauth_mgr.exchange_code(code, oauth_state).await {
        Ok(pair) => pair,
        Err(e) => {
            warn!("OAuth code exchange failed: {} ({})", e, addr);
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

    // Access permission check (access_token is used by the org-membership API).
    if !oauth_mgr.is_user_allowed(&user, &access_token).await {
        warn!("OAuth access denied: user_id={} ({})", user.user_id, addr);
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
        "OAuth login succeeded: {} ({}) ({})",
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
        .expect("invalid header value for Response::builder")
}
