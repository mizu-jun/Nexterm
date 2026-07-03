//! Audit round 2 / G4: axum-level integration tests for the Web Terminal
//! authentication stack (TOTP login, session cookies, logout, rate limiting,
//! setup endpoints, and OAuth entry points).
//!
//! Prior to this module the auth flow was only covered by per-module unit
//! tests; these tests drive real HTTP requests through the full router
//! (`build_router`) with `tower::ServiceExt::oneshot`, so routing, extractors,
//! middleware, and handlers are exercised together. No sockets are bound and
//! no external network is touched (the OAuth tests stop at CSRF validation).

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::{AppState, PendingSetup, access_log, auth, otp, rate_limit, router};
use crate::session::SessionManager;

/// A fixed Base32 TOTP secret for tests (any valid Base32 works).
const TEST_SECRET: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";

/// Computes the currently valid 6-digit code for [`TEST_SECRET`] using the
/// same RFC 6238 parameters as `TotpManager` (SHA1 / 6 digits / 30 s step).
fn current_totp_code() -> String {
    let secret = totp_rs::Secret::Encoded(TEST_SECRET.to_string())
        .to_bytes()
        .expect("valid test secret");
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret,
        Some("test".to_string()),
        "web-terminal".to_string(),
    )
    .expect("TOTP construction");
    totp.generate_current().expect("system clock before epoch")
}

/// Builds an `AppState` with TOTP enabled and the given knobs. The access log
/// is disabled and no OS resources are touched.
fn test_state(with_totp: bool, with_pending_setup: bool) -> AppState {
    let web_config = nexterm_config::WebConfig::default();
    let totp = if with_totp {
        Some(otp::TotpManager::from_secret(TEST_SECRET, "test").expect("test TOTP manager"))
    } else {
        None
    };
    let pending_setup = if with_pending_setup {
        Some(PendingSetup {
            secret: TEST_SECRET.to_string(),
            totp: otp::TotpManager::from_secret(TEST_SECRET, "test").expect("setup TOTP manager"),
        })
    } else {
        None
    };
    AppState {
        manager: Arc::new(SessionManager::new(nexterm_config::ShellConfig::default())),
        legacy_token: None,
        totp: Arc::new(tokio::sync::RwLock::new(totp)),
        auth_mgr: Arc::new(auth::AuthManager::new(3600, 10)),
        pending_setup: Arc::new(Mutex::new(pending_setup)),
        totp_enabled: with_totp,
        oauth_mgr: None,
        tls_enabled: false,
        force_https: false,
        issuer: "test".to_string(),
        access_logger: Arc::new(access_log::AccessLogger::new(&web_config.access_log)),
        totp_rate_limiter: Arc::new(rate_limit::RateLimiter::new(
            rate_limit::RateLimitConfig::totp_default(),
        )),
    }
}

/// POSTs the login form with the given code and returns the response.
async fn post_login(app: &axum::Router, code: &str) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::post("/auth/login")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("code={code}")))
                .expect("build login request"),
        )
        .await
        .expect("router must respond")
}

/// GETs `/` with an optional session cookie.
async fn get_index(app: &axum::Router, cookie: Option<&str>) -> axum::http::Response<Body> {
    let mut req = Request::get("/");
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).expect("build index request"))
        .await
        .expect("router must respond")
}

fn location_of(res: &axum::http::Response<Body>) -> &str {
    res.headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

fn set_cookie_of(res: &axum::http::Response<Body>) -> Option<String> {
    res.headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

#[tokio::test]
async fn index_without_a_session_redirects_to_login() {
    let app = router::build_router(test_state(true, false));
    let res = get_index(&app, None).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(location_of(&res), "/login");
}

#[tokio::test]
async fn index_with_a_garbage_cookie_redirects_to_login() {
    let app = router::build_router(test_state(true, false));
    let res = get_index(&app, Some("nexterm_session=bogus-token")).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(location_of(&res), "/login");
}

#[tokio::test]
async fn totp_login_with_a_valid_code_sets_a_session_cookie_and_grants_access() {
    let app = router::build_router(test_state(true, false));

    let res = post_login(&app, &current_totp_code()).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(location_of(&res), "/");
    let cookie = set_cookie_of(&res).expect("login must set a session cookie");
    assert!(cookie.starts_with("nexterm_session="));
    assert!(cookie.contains("HttpOnly"), "cookie must be HttpOnly");
    assert!(
        cookie.contains("SameSite=Strict"),
        "cookie must be SameSite=Strict"
    );

    // The issued cookie unlocks the index page.
    let session_pair = cookie.split(';').next().expect("cookie key=value pair");
    let res = get_index(&app, Some(session_pair)).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn totp_login_with_an_invalid_code_is_rejected_without_a_cookie() {
    let app = router::build_router(test_state(true, false));
    let res = post_login(&app, "000000").await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(location_of(&res), "/login?error=invalid_code");
    assert!(set_cookie_of(&res).is_none(), "no cookie on failed login");
}

#[tokio::test]
async fn totp_login_replay_of_the_same_code_is_rejected() {
    // CRITICAL #6 regression guard: the same (window, code) pair must not be
    // accepted twice.
    let app = router::build_router(test_state(true, false));
    let code = current_totp_code();
    let first = post_login(&app, &code).await;
    assert_eq!(location_of(&first), "/");
    let second = post_login(&app, &code).await;
    assert_eq!(location_of(&second), "/login?error=invalid_code");
}

#[tokio::test]
async fn totp_login_is_rate_limited_after_five_attempts() {
    // CRITICAL #2 regression guard: 5 attempts/min per IP.
    let app = router::build_router(test_state(true, false));
    for _ in 0..5 {
        let res = post_login(&app, "000000").await;
        assert_eq!(res.status(), StatusCode::FOUND);
    }
    let res = post_login(&app, "000000").await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        res.headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok()),
        Some("60")
    );
}

#[tokio::test]
async fn login_when_totp_is_not_configured_redirects_with_an_error() {
    let app = router::build_router(test_state(false, false));
    let res = post_login(&app, "123456").await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(location_of(&res), "/login?error=not_configured");
}

#[tokio::test]
async fn logout_revokes_the_session() {
    let app = router::build_router(test_state(true, false));
    let login = post_login(&app, &current_totp_code()).await;
    let cookie = set_cookie_of(&login).expect("session cookie");
    let session_pair = cookie.split(';').next().expect("cookie pair").to_string();

    let res = app
        .clone()
        .oneshot(
            Request::post("/auth/logout")
                .header(header::COOKIE, &session_pair)
                .body(Body::empty())
                .expect("build logout request"),
        )
        .await
        .expect("router must respond");
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(location_of(&res), "/login");
    let logout_cookie = set_cookie_of(&res).expect("logout must clear the cookie");
    assert!(logout_cookie.contains("Max-Age=0"));

    // The revoked cookie no longer grants access.
    let res = get_index(&app, Some(&session_pair)).await;
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(location_of(&res), "/login");
}

#[tokio::test]
async fn setup_url_is_available_only_while_setup_is_pending() {
    // Pending: the endpoint exposes the otpauth URL + secret.
    let app = router::build_router(test_state(true, true));
    let res = app
        .clone()
        .oneshot(
            Request::get("/auth/setup-url")
                .body(Body::empty())
                .expect("build setup-url request"),
        )
        .await
        .expect("router must respond");
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 64 * 1024)
        .await
        .expect("read body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("JSON body");
    assert_eq!(json["secret"], TEST_SECRET);
    assert!(
        json["url"]
            .as_str()
            .expect("url string")
            .starts_with("otpauth://"),
    );

    // Not pending: 404 (the secret must not be re-exposed after setup).
    let app = router::build_router(test_state(true, false));
    let res = app
        .oneshot(
            Request::get("/auth/setup-url")
                .body(Body::empty())
                .expect("build setup-url request"),
        )
        .await
        .expect("router must respond");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn setup_page_redirects_home_when_no_setup_is_pending() {
    let app = router::build_router(test_state(true, false));
    let res = app
        .oneshot(
            Request::get("/setup")
                .body(Body::empty())
                .expect("build setup request"),
        )
        .await
        .expect("router must respond");
    assert_eq!(res.status(), StatusCode::FOUND);
    assert_eq!(location_of(&res), "/");
}

#[tokio::test]
async fn oauth_endpoints_reject_requests_when_oauth_is_disabled() {
    let app = router::build_router(test_state(true, false));

    let res = app
        .clone()
        .oneshot(
            Request::get("/auth/oauth")
                .body(Body::empty())
                .expect("build oauth request"),
        )
        .await
        .expect("router must respond");
    // Disabled OAuth answers 404 (the endpoints do not exist for clients).
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let res = app
        .clone()
        .oneshot(
            Request::get("/auth/callback?code=x&state=y")
                .body(Body::empty())
                .expect("build callback request"),
        )
        .await
        .expect("router must respond");
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn security_headers_are_applied_to_the_login_page() {
    // G8 regression guard at the integration level: the middleware must fire
    // on real routed responses, not just in its unit tests.
    let app = router::build_router(test_state(true, false));
    let res = app
        .oneshot(
            Request::get("/login")
                .body(Body::empty())
                .expect("build login-page request"),
        )
        .await
        .expect("router must respond");
    assert_eq!(res.status(), StatusCode::OK);
    for header_name in [
        "content-security-policy",
        "x-content-type-options",
        "referrer-policy",
    ] {
        assert!(
            res.headers().contains_key(header_name),
            "missing security header: {header_name}"
        );
    }
}
