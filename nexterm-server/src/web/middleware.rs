//! Web ターミナルの認証ヘルパー / クライアント IP 抽出 / HTTPS リダイレクト。

use axum::{
    http::HeaderMap,
    response::{IntoResponse, Response},
};

use super::AppState;
use super::auth;

/// 認証が必要な状況でセッションが有効かを確認する
///
/// TOTP と OAuth の両方が無効の場合は認証不要として true を返す。
pub(in crate::web) fn has_valid_session(state: &AppState, headers: &HeaderMap) -> bool {
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
pub(in crate::web) fn client_ip(headers: &HeaderMap) -> String {
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        // X-Forwarded-For は "IP1, IP2, ..." の形式なので最初のエントリを使う
        return v.split(',').next().unwrap_or(v).trim().to_string();
    }
    if let Some(v) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        return v.to_string();
    }
    "unknown".to_string()
}

/// HTTP リクエストを HTTPS へリダイレクトするレスポンスを返す
pub(in crate::web) fn https_redirect(headers: &HeaderMap, port: u16) -> Response {
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
        .expect("Response::builder への無効なヘッダー値")
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    // ---- client_ip テスト ----

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
        // X-Forwarded-For が優先される
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

    // ---- https_redirect テスト ----

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
        // 元のポートが削除され、HTTPSポートに置き換えられる
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
}
