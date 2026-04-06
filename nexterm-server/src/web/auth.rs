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
        let token: String = rand::thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();

        let expiry = Instant::now() + self.ttl;
        let mut sessions = self.sessions.lock().unwrap();

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
        let sessions = self.sessions.lock().unwrap();
        sessions
            .get(token)
            .map(|entry| Instant::now() < entry.expiry)
            .unwrap_or(false)
    }

    /// セッションのメタ情報を取得する（アクセスログ用）
    pub fn session_info(&self, token: &str) -> Option<(String, String)> {
        let sessions = self.sessions.lock().unwrap();
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
        self.sessions.lock().unwrap().remove(token);
    }

    /// アクティブなセッション数を返す（期限切れを除く）
    pub fn active_count(&self) -> usize {
        let sessions = self.sessions.lock().unwrap();
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
