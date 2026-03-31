//! セッション管理 — Cookie ベースのセッショントークン

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rand::Rng;

/// セッション有効期限（24 時間）
const SESSION_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// 認証マネージャー（Router の AppState に保持）
#[derive(Clone)]
pub struct AuthManager {
    /// token → 有効期限
    sessions: Arc<Mutex<HashMap<String, Instant>>>,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// OTP 検証成功後に新しいセッショントークンを生成する
    pub fn create_session(&self) -> String {
        let token: String = rand::thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();

        let expiry = Instant::now() + SESSION_TTL;
        self.sessions.lock().unwrap().insert(token.clone(), expiry);
        token
    }

    /// セッショントークンが有効か（存在 + 未期限切れ）を確認する
    pub fn is_valid(&self, token: &str) -> bool {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .get(token)
            .map(|expiry| Instant::now() < *expiry)
            .unwrap_or(false)
    }
}

/// リクエストヘッダーから `nexterm_session` Cookie を抽出する
pub fn extract_session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_str = headers.get("cookie")?.to_str().ok()?;
    cookie_str
        .split(';')
        .find_map(|part| {
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
