//! TOTP login / logout / initial TOTP setup handlers.

use axum::{
    Form, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::assets::redirect;
use crate::web::AppState;
use crate::web::access_log;
use crate::web::auth;
use crate::web::middleware::client_ip;
use crate::web::otp;

/// Fields of the login form.
#[derive(Deserialize)]
pub(in crate::web) struct LoginForm {
    code: String,
}

/// POST /auth/login — verify a TOTP code and issue a session.
pub(in crate::web) async fn handle_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let addr = client_ip(&headers);

    // Rate limit: allow only 5 attempts per 60 seconds from the same IP
    // (TOTP brute-force defense, CRITICAL #2).
    if !state.totp_rate_limiter.check_and_record(&addr) {
        warn!("TOTP login rate limit exceeded ({})", addr);
        state.access_logger.log(&access_log::AccessLogEntry {
            remote_addr: addr.clone(),
            method: "POST".to_string(),
            path: "/auth/login".to_string(),
            status: 429,
            auth_method: "totp".to_string(),
            user_id: String::new(),
        });
        return Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("Retry-After", "60")
            .body(axum::body::Body::from(
                "Too many failed attempts. Try again in 60 seconds.",
            ))
            .unwrap_or_else(|_| redirect("/login?error=rate_limited"));
    }

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
        warn!("TOTP login failure: invalid code ({})", addr);
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

    // Authentication succeeded: reset the rate-limit counter (free legitimate users from penalties).
    state.totp_rate_limiter.reset(&addr);

    let token = match state.auth_mgr.create_session("totp", "") {
        Some(t) => t,
        None => return redirect("/login?error=session_limit"),
    };
    let cookie = auth::make_session_cookie(&token, state.tls_enabled);

    info!("TOTP login succeeded ({})", addr);
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
        .expect("invalid header value for Response::builder")
}

/// POST /auth/logout — revoke the session and redirect to the login page.
pub(in crate::web) async fn handle_logout(
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
        .expect("failed to build logout redirect response")
}

/// Setup URL response payload.
#[derive(Serialize)]
struct SetupUrlResponse {
    url: String,
    secret: String,
}

/// GET /auth/setup-url — return the otpauth:// URL and the secret for setup.
pub(in crate::web) async fn handle_setup_url(State(state): State<AppState>) -> Response {
    let guard = state
        .pending_setup
        .lock()
        .expect("pending_setup mutex poisoned");
    match guard.as_ref() {
        Some(ps) => Json(SetupUrlResponse {
            url: ps.totp.get_url(),
            secret: ps.totp.secret_b32().to_string(),
        })
        .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// POST /setup/verify — verify the first TOTP code and persist the secret.
pub(in crate::web) async fn handle_setup_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let addr = client_ip(&headers);
    let (secret_clone, is_valid) = {
        let guard = state
            .pending_setup
            .lock()
            .expect("pending_setup mutex poisoned");
        match guard.as_ref() {
            Some(ps) => (ps.secret.clone(), ps.totp.verify(&form.code)),
            None => return redirect("/?setup=done"),
        }
    };

    if !is_valid {
        return redirect("/setup?error=invalid_code");
    }

    if let Err(e) = otp::save_secret_to_config(&secret_clone) {
        warn!(
            "failed to save TOTP secret: {}. continuing with in-memory state only.",
            e
        );
    }

    match otp::TotpManager::from_secret(&secret_clone, &state.issuer) {
        Ok(mgr) => {
            *state.totp.write().await = Some(mgr);
            *state
                .pending_setup
                .lock()
                .expect("pending_setup mutex poisoned") = None;
            info!("TOTP setup completed ({})", addr);
        }
        Err(e) => {
            warn!("failed to create TOTP manager: {}", e);
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
        .expect("invalid header value for Response::builder")
}
