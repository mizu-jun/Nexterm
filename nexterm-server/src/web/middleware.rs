//! Web terminal auth helpers / client IP extraction / HTTPS redirect / security headers.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::AppState;
use super::auth;

/// Check whether the request session is valid when authentication is required.
///
/// Returns `true` (no auth required) when both TOTP and OAuth are disabled.
pub(in crate::web) fn has_valid_session(state: &AppState, headers: &HeaderMap) -> bool {
    let auth_required = state.totp_enabled || state.oauth_mgr.is_some();
    if !auth_required {
        return true;
    }
    auth::extract_session_cookie(headers)
        .map(|token| state.auth_mgr.is_valid(&token))
        .unwrap_or(false)
}

/// Extract the client IP from request headers.
///
/// Tries X-Forwarded-For, then X-Real-IP, then `"unknown"`.
pub(in crate::web) fn client_ip(headers: &HeaderMap) -> String {
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        // X-Forwarded-For has the form "IP1, IP2, ..."; use the first entry.
        return v.split(',').next().unwrap_or(v).trim().to_string();
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        return v.to_string();
    }
    "unknown".to_string()
}

/// Return a response that redirects an HTTP request to HTTPS.
pub(in crate::web) fn https_redirect(headers: &HeaderMap, port: u16) -> Response {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    // Strip the port portion and replace it with the HTTPS port.
    let host_no_port = host.split(':').next().unwrap_or(host);
    let location = format!("https://{}:{}/", host_no_port, port);
    Response::builder()
        .status(301)
        .header("Location", location)
        .body(axum::body::Body::empty())
        .expect("invalid header value for Response::builder")
        .into_response()
}

// ── Security headers ─────────────────────────────────────────────────────────

/// Returns the (name, value) pairs for OWASP-recommended response headers.
///
/// When `tls_enabled` is `true`, `Strict-Transport-Security` is included
/// (RFC 6797 §8.1 — only meaningful over HTTPS).
fn security_header_pairs(tls_enabled: bool) -> Vec<(&'static str, &'static str)> {
    let mut headers: Vec<(&str, &str)> = vec![
        // Prevent MIME-type sniffing.
        ("x-content-type-options", "nosniff"),
        // Prevent clickjacking (belt-and-suspenders with CSP frame-ancestors).
        ("x-frame-options", "DENY"),
        // Limit referrer information sent to third parties.
        ("referrer-policy", "strict-origin-when-cross-origin"),
        // Prevent cross-origin window opener attacks.
        ("cross-origin-opener-policy", "same-origin"),
        // Prevent cross-origin resource loading.
        ("cross-origin-resource-policy", "same-origin"),
        // Disable browser features not needed by the web terminal.
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=(), payment=()",
        ),
        // CSP tailored for xterm.js: same-origin scripts/styles, WebSocket
        // connections, and data-URI images/fonts; deny all framing.
        (
            "content-security-policy",
            "default-src 'none'; \
             script-src 'self'; \
             style-src 'self' 'unsafe-inline'; \
             connect-src 'self' ws: wss:; \
             img-src 'self' data:; \
             font-src 'self' data:; \
             frame-ancestors 'none'",
        ),
    ];
    if tls_enabled {
        headers.push((
            "strict-transport-security",
            "max-age=63072000; includeSubDomains",
        ));
    }
    headers
}

/// Axum middleware that adds OWASP security headers to every response.
///
/// `Strict-Transport-Security` is included only when `AppState.tls_enabled`
/// is `true`, matching RFC 6797 §8.1.
pub(in crate::web) async fn security_headers(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    for (name, value) in security_header_pairs(state.tls_enabled) {
        h.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    // ---- client_ip tests ----

    #[test]
    fn client_ip_from_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "192.168.1.1, 10.0.0.1".parse().unwrap());
        assert_eq!(client_ip(&headers), "192.168.1.1");
    }

    #[test]
    fn client_ip_from_x_real_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "192.168.1.2".parse().unwrap());
        assert_eq!(client_ip(&headers), "192.168.1.2");
    }

    #[test]
    fn client_ip_prefers_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "10.0.0.1".parse().unwrap());
        headers.insert("x-real-ip", "192.168.1.1".parse().unwrap());
        // X-Forwarded-For wins.
        assert_eq!(client_ip(&headers), "10.0.0.1");
    }

    #[test]
    fn client_ip_returns_unknown_when_no_header() {
        let headers = HeaderMap::new();
        assert_eq!(client_ip(&headers), "unknown");
    }

    #[test]
    fn client_ip_trims_whitespace() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "  192.168.1.1  ".parse().unwrap());
        assert_eq!(client_ip(&headers), "192.168.1.1");
    }

    // ---- https_redirect tests ----

    #[test]
    fn https_redirect_uses_host_header() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "example.com".parse().unwrap());
        let response = https_redirect(&headers, 8443);
        assert_eq!(response.status(), 301);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.contains("example.com:8443"));
        assert!(location.starts_with("https://"));
    }

    #[test]
    fn https_redirect_removes_port_from_host() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "example.com:8080".parse().unwrap());
        let response = https_redirect(&headers, 8443);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        // The original port is removed and replaced with the HTTPS port.
        assert!(!location.contains(":8080"));
        assert!(location.contains(":8443"));
    }

    #[test]
    fn https_redirect_uses_localhost_fallback() {
        let headers = HeaderMap::new();
        let response = https_redirect(&headers, 8443);
        let location = response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(location.contains("localhost:8443"));
    }

    // ---- security_header_pairs tests ----

    #[test]
    fn security_headers_http_excludes_hsts() {
        let pairs = security_header_pairs(false);
        assert!(
            !pairs.iter().any(|(k, _)| *k == "strict-transport-security"),
            "HSTS must not be set over plain HTTP"
        );
    }

    #[test]
    fn security_headers_https_includes_hsts() {
        let pairs = security_header_pairs(true);
        let hsts = pairs
            .iter()
            .find(|(k, _)| *k == "strict-transport-security")
            .map(|(_, v)| *v);
        assert_eq!(
            hsts,
            Some("max-age=63072000; includeSubDomains"),
            "HSTS value must include a 2-year max-age and includeSubDomains"
        );
    }

    #[test]
    fn security_headers_always_present() {
        for tls in [false, true] {
            let pairs = security_header_pairs(tls);
            let names: Vec<&str> = pairs.iter().map(|(k, _)| *k).collect();
            for required in &[
                "x-content-type-options",
                "x-frame-options",
                "referrer-policy",
                "cross-origin-opener-policy",
                "cross-origin-resource-policy",
                "permissions-policy",
                "content-security-policy",
            ] {
                assert!(
                    names.contains(required),
                    "missing required header: {required} (tls_enabled={tls})"
                );
            }
        }
    }

    #[test]
    fn security_headers_csp_denies_framing() {
        let pairs = security_header_pairs(false);
        let csp = pairs
            .iter()
            .find(|(k, _)| *k == "content-security-policy")
            .map(|(_, v)| *v)
            .expect("CSP must be present");
        assert!(
            csp.contains("frame-ancestors 'none'"),
            "CSP must deny all framing"
        );
    }
}
