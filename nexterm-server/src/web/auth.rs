//! Session management — cookie-based session tokens.
//!
//! - Configurable TTL (default: 24 hours).
//! - Concurrent session count limit.
//! - Records the session creator identifier (auth method + user ID).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rand::Rng;

/// Session entry.
struct SessionEntry {
    /// Session expiry timestamp.
    expiry: Instant,
    /// Authentication method (e.g. "totp", "oauth:github", "token").
    auth_method: String,
    /// Authenticated user identifier (OAuth username, etc.; empty string when anonymous).
    user_id: String,
}

/// Authentication manager (held inside the router's `AppState`).
#[derive(Clone)]
pub struct AuthManager {
    /// token -> session entry.
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
    /// Session lifetime (seconds).
    ttl: Duration,
    /// Concurrent session limit (0 = unlimited).
    max_sessions: usize,
}

impl AuthManager {
    /// Construct a new `AuthManager`.
    pub fn new(session_timeout_secs: u64, max_sessions: usize) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            ttl: Duration::from_secs(session_timeout_secs),
            max_sessions,
        }
    }

    /// Issue a new session token after a successful authentication.
    ///
    /// When `max_sessions` is exceeded, the oldest session is evicted.
    pub fn create_session(&self, auth_method: &str, user_id: &str) -> Option<String> {
        let token: String = rand::rng()
            .sample_iter(rand::distr::Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();

        let expiry = Instant::now() + self.ttl;
        let mut sessions = self.sessions.lock().expect("session store mutex poisoned");

        // Purge expired sessions up front.
        sessions.retain(|_, v| Instant::now() < v.expiry);

        // Enforce the limit.
        if self.max_sessions > 0 && sessions.len() >= self.max_sessions {
            // Evict the session with the earliest expiry.
            if let Some(oldest_key) = sessions
                .iter()
                .min_by_key(|(_, v)| v.expiry)
                .map(|(k, _)| k.clone())
            {
                sessions.remove(&oldest_key);
            }
        }

        sessions.insert(
            token.clone(),
            SessionEntry {
                expiry,
                auth_method: auth_method.to_string(),
                user_id: user_id.to_string(),
            },
        );
        Some(token)
    }

    /// Check whether a session token is still valid.
    pub fn is_valid(&self, token: &str) -> bool {
        let sessions = self.sessions.lock().expect("session store mutex poisoned");
        sessions
            .get(token)
            .map(|entry| Instant::now() < entry.expiry)
            .unwrap_or(false)
    }

    /// Return session metadata (for the access log).
    pub fn session_info(&self, token: &str) -> Option<(String, String)> {
        let sessions = self.sessions.lock().expect("session store mutex poisoned");
        sessions.get(token).and_then(|entry| {
            if Instant::now() < entry.expiry {
                Some((entry.auth_method.clone(), entry.user_id.clone()))
            } else {
                None
            }
        })
    }

    /// Explicitly remove a session (used for logout).
    pub fn revoke_session(&self, token: &str) {
        self.sessions
            .lock()
            .expect("session store mutex poisoned")
            .remove(token);
    }

    /// Return the number of active sessions (excluding expired ones).
    #[allow(dead_code)]
    pub fn active_count(&self) -> usize {
        let sessions = self.sessions.lock().expect("session store mutex poisoned");
        sessions
            .values()
            .filter(|v| Instant::now() < v.expiry)
            .count()
    }
}

/// Extract the `nexterm_session` cookie from request headers.
pub fn extract_session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_str = headers.get("cookie")?.to_str().ok()?;
    cookie_str.split(';').find_map(|part| {
        part.trim()
            .strip_prefix("nexterm_session=")
            .map(|v| v.to_string())
    })
}

/// Build a Set-Cookie header value (adds the `Secure` flag when HTTPS is in use).
pub fn make_session_cookie(token: &str, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "nexterm_session={}; HttpOnly; Path=/; SameSite=Strict{}",
        token, secure_flag
    )
}

/// Build the Set-Cookie header value used to delete the session cookie.
pub fn make_logout_cookie() -> String {
    "nexterm_session=; HttpOnly; Path=/; SameSite=Strict; Max-Age=0".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn auth_manager_new_creates_empty_manager() {
        let manager = AuthManager::new(3600, 100);
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn create_session_returns_token() {
        let manager = AuthManager::new(3600, 100);
        let token = manager.create_session("totp", "user123");
        assert!(token.is_some());
        assert_eq!(token.as_ref().unwrap().len(), 48);
    }

    #[test]
    fn create_session_increases_count() {
        let manager = AuthManager::new(3600, 100);
        let initial = manager.active_count();
        manager.create_session("oauth", "github_user");
        assert_eq!(manager.active_count(), initial + 1);
    }

    #[test]
    fn is_valid_returns_true_for_valid_session() {
        let manager = AuthManager::new(3600, 100);
        let token = manager.create_session("totp", "user123").unwrap();
        assert!(manager.is_valid(&token));
    }

    #[test]
    fn is_valid_returns_false_for_invalid_token() {
        let manager = AuthManager::new(3600, 100);
        assert!(!manager.is_valid("invalid_token"));
    }

    #[test]
    fn session_info_returns_auth_method_and_user_id() {
        let manager = AuthManager::new(3600, 100);
        let token = manager.create_session("oauth:github", "octocat").unwrap();
        let (auth_method, user_id) = manager.session_info(&token).unwrap();
        assert_eq!(auth_method, "oauth:github");
        assert_eq!(user_id, "octocat");
    }

    #[test]
    fn revoke_session_makes_token_invalid() {
        let manager = AuthManager::new(3600, 100);
        let token = manager.create_session("totp", "user123").unwrap();
        assert!(manager.is_valid(&token));
        manager.revoke_session(&token);
        assert!(!manager.is_valid(&token));
    }

    #[test]
    fn max_sessions_limits_active_sessions() {
        let manager = AuthManager::new(3600, 2);
        let _token1 = manager.create_session("totp", "user1").unwrap();
        let _token2 = manager.create_session("totp", "user2").unwrap();
        // Creating a third session evicts the oldest one.
        let _token3 = manager.create_session("totp", "user3").unwrap();
        assert_eq!(manager.active_count(), 2);
        // The first token may have been evicted.
    }

    #[test]
    fn make_session_cookie_includes_token() {
        let cookie = make_session_cookie("abc123", false);
        assert!(cookie.contains("abc123"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn make_session_cookie_secure_flag() {
        let cookie = make_session_cookie("abc123", true);
        assert!(cookie.contains("Secure"));
    }

    #[test]
    fn make_logout_cookie_expires_immediately() {
        let cookie = make_logout_cookie();
        assert!(cookie.contains("Max-Age=0"));
        assert!(cookie.contains("nexterm_session=;"));
    }

    #[test]
    fn extract_session_cookie_parses_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            "other=value; nexterm_session=abc123; another=test"
                .parse()
                .unwrap(),
        );
        let session = extract_session_cookie(&headers);
        assert_eq!(session, Some("abc123".to_string()));
    }

    #[test]
    fn extract_session_cookie_returns_none_if_missing() {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", "other=value".parse().unwrap());
        let session = extract_session_cookie(&headers);
        assert_eq!(session, None);
    }

    #[test]
    fn extract_session_cookie_returns_none_if_no_cookie_header() {
        let headers = HeaderMap::new();
        let session = extract_session_cookie(&headers);
        assert_eq!(session, None);
    }
}
