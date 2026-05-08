//! セッション管理 — Cookie ベースのセッショントークン
//!
//! - 設定可能な TTL（デフォルト 24 時間）
//! - 同時セッション数の上限制限
//! - セッション作成者の識別子（認証方式・ユーザー ID）を記録

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rand::Rng;

/// セッションエントリ
struct SessionEntry {
    /// セッション有効期限
    expiry: Instant,
    /// 認証方式（例: "totp", "oauth:github", "token"）
    auth_method: String,
    /// 認証したユーザー識別子（OAuth ユーザー名等。匿名は空文字列）
    user_id: String,
}

/// 認証マネージャー（Router の AppState に保持）
#[derive(Clone)]
pub struct AuthManager {
    /// token → セッションエントリ
    sessions: Arc<Mutex<HashMap<String, SessionEntry>>>,
    /// セッション有効期限（秒）
    ttl: Duration,
    /// 同時セッション数の上限（0 = 無制限）
    max_sessions: usize,
}

impl AuthManager {
    /// 新しい AuthManager を生成する
    pub fn new(session_timeout_secs: u64, max_sessions: usize) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            ttl: Duration::from_secs(session_timeout_secs),
            max_sessions,
        }
    }

    /// 認証成功後に新しいセッショントークンを生成する
    ///
    /// `max_sessions` を超える場合は最も古いセッションを削除する。
    pub fn create_session(&self, auth_method: &str, user_id: &str) -> Option<String> {
        let token: String = rand::rng()
            .sample_iter(rand::distr::Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();

        let expiry = Instant::now() + self.ttl;
        let mut sessions = self.sessions.lock().expect("session store mutex poisoned");

        // 期限切れセッションを事前に掃除する
        sessions.retain(|_, v| Instant::now() < v.expiry);

        // 上限チェック
        if self.max_sessions > 0 && sessions.len() >= self.max_sessions {
            // 最も早く期限切れになるセッションを削除する
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

    /// セッショントークンが有効か確認する
    pub fn is_valid(&self, token: &str) -> bool {
        let sessions = self.sessions.lock().expect("session store mutex poisoned");
        sessions
            .get(token)
            .map(|entry| Instant::now() < entry.expiry)
            .unwrap_or(false)
    }

    /// セッションのメタ情報を取得する（アクセスログ用）
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

    /// 明示的にセッションを削除する（ログアウト用）
    pub fn revoke_session(&self, token: &str) {
        self.sessions
            .lock()
            .expect("session store mutex poisoned")
            .remove(token);
    }

    /// アクティブなセッション数を返す（期限切れを除く）
    #[allow(dead_code)]
    pub fn active_count(&self) -> usize {
        let sessions = self.sessions.lock().expect("session store mutex poisoned");
        sessions
            .values()
            .filter(|v| Instant::now() < v.expiry)
            .count()
    }
}

/// リクエストヘッダーから `nexterm_session` Cookie を抽出する
pub fn extract_session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_str = headers.get("cookie")?.to_str().ok()?;
    cookie_str.split(';').find_map(|part| {
        part.trim()
            .strip_prefix("nexterm_session=")
            .map(|v| v.to_string())
    })
}

/// Set-Cookie ヘッダー値を生成する（HTTPS 時は Secure フラグを追加）
pub fn make_session_cookie(token: &str, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "nexterm_session={}; HttpOnly; Path=/; SameSite=Strict{}",
        token, secure_flag
    )
}

/// セッション削除用の Set-Cookie ヘッダー値を生成する
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
        // 3つ目を作成すると最も古いものが削除される
        let _token3 = manager.create_session("totp", "user3").unwrap();
        assert_eq!(manager.active_count(), 2);
        // 最初のトークンは削除されている可能性がある
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
