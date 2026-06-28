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

    // -------------------------------------------------------------------
    // Adversarial tests (QA persona: 悪意ある操作者)
    //
    // These exercise the deny paths of the auth surface:
    // - Tampered / forged tokens must never be valid.
    // - Expired tokens must be rejected once the TTL elapses.
    // - Concurrent issuance must respect `max_sessions`.
    // - Cookie parsing must not be fooled by suffix collisions.
    // -------------------------------------------------------------------

    #[test]
    fn is_valid_rejects_random_unknown_token() {
        let manager = AuthManager::new(3600, 100);
        // No session has been issued — any guess must be rejected.
        assert!(!manager.is_valid("not-a-real-token"));
        assert!(!manager.is_valid(""));
        assert!(!manager.is_valid(&"A".repeat(48)));
    }

    #[test]
    fn is_valid_rejects_tampered_token() {
        let manager = AuthManager::new(3600, 100);
        let token = manager.create_session("password", "alice").expect("token");
        // Flip one character — must no longer match.
        let mut tampered: Vec<char> = token.chars().collect();
        tampered[0] = if tampered[0] == 'A' { 'B' } else { 'A' };
        let tampered: String = tampered.into_iter().collect();
        assert!(
            !manager.is_valid(&tampered),
            "tampered token must be rejected"
        );
        // Original is still valid.
        assert!(manager.is_valid(&token));
    }

    #[test]
    fn expired_session_is_rejected_after_ttl() {
        // 0-second TTL: every fresh token is considered expired on the next
        // tick. Using ttl=0 here makes the check deterministic without sleeps.
        let manager = AuthManager::new(0, 100);
        let token = manager.create_session("password", "u").expect("token");
        // The expiry is already in the past — `is_valid` must say so.
        // (Instant::now() advances by at least a nanosecond between calls.)
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(
            !manager.is_valid(&token),
            "0-TTL tokens must expire immediately"
        );
        assert!(manager.session_info(&token).is_none());
    }

    #[test]
    fn max_sessions_evicts_oldest_on_overflow() {
        let manager = AuthManager::new(3600, 2);
        let t1 = manager.create_session("password", "u1").expect("t1");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let t2 = manager.create_session("password", "u2").expect("t2");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let t3 = manager.create_session("password", "u3").expect("t3");
        // The oldest (t1) must have been evicted.
        assert!(!manager.is_valid(&t1), "oldest session must be evicted");
        assert!(manager.is_valid(&t2));
        assert!(manager.is_valid(&t3));
        assert_eq!(manager.active_count(), 2);
    }

    #[test]
    fn revoked_token_cannot_be_resurrected() {
        let manager = AuthManager::new(3600, 100);
        let token = manager.create_session("password", "u").expect("token");
        manager.revoke_session(&token);
        assert!(!manager.is_valid(&token));
        // Revoking again is a noop, not a panic.
        manager.revoke_session(&token);
        manager.revoke_session("never-existed");
        assert!(!manager.is_valid(&token));
    }

    #[test]
    fn cookie_parser_is_not_confused_by_suffix_collision() {
        // `not_nexterm_session=foo` must not be parsed as our cookie even
        // though it ends with the same substring.
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            "not_nexterm_session=tricked; other=ok".parse().unwrap(),
        );
        assert_eq!(
            extract_session_cookie(&headers),
            None,
            "suffix collision must not be accepted as the session cookie"
        );
    }

    #[test]
    fn each_session_token_is_unique() {
        // Two consecutive sessions must produce different tokens (random 48-char alnum).
        let manager = AuthManager::new(3600, 100);
        let a = manager.create_session("password", "u").expect("a");
        let b = manager.create_session("password", "u").expect("b");
        assert_ne!(a, b, "session tokens must not collide");
        // Both 48 chars, alphanumeric only.
        for tok in [&a, &b] {
            assert_eq!(tok.len(), 48);
            assert!(tok.chars().all(|c| c.is_ascii_alphanumeric()));
        }
    }
}
