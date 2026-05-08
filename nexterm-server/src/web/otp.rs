//! TOTP (Time-based One-Time Password) — RFC 6238 に準拠した OTP 認証
//!
//! シークレットは nexterm.toml の `[web.auth]` セクションに Base32 形式で保存する。
//! 初回起動時にシークレットが未設定の場合はブラウザ上でセットアップを行う。
//!
//! # リプレイ防御（CRITICAL #6 対策）
//!
//! 同一 OTP コードは ±1 ウィンドウ（最大 90 秒）の間に複数回受け付けない。
//! ネットワーク盗聴・画面録画・ショルダーサーフィンで OTP コードを取得した
//! 攻撃者が、同一コードで複数回ログインすることを防ぐ。

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use subtle::ConstantTimeEq;
use totp_rs::{Algorithm, Secret, TOTP};

/// TOTP のステップ（秒）。RFC 6238 標準値の 30 秒。
const TOTP_STEP: u64 = 30;

/// 使用済み OTP を保持する最大ウィンドウ範囲（現在 ±2 ウィンドウ = 過去 90 秒）。
///
/// `±1` で OTP を許容するため、`±2` まで保持しておけば古い分は安全に GC できる。
const REPLAY_WINDOW_RANGE: i64 = 2;

/// TOTP マネージャー — シークレットと TOTP インスタンスを保持する
pub struct TotpManager {
    totp: TOTP,
    /// Base32 エンコードされたシークレット（表示・保存用）
    secret_b32: String,
    /// 使用済み (window_id, code) ペア。リプレイ攻撃を防止する。
    used_codes: Mutex<HashSet<(u64, String)>>,
}

impl TotpManager {
    /// 設定ファイルの Base32 シークレットから生成する
    pub fn from_secret(secret_b32: &str, issuer: &str) -> anyhow::Result<Self> {
        let secret = Secret::Encoded(secret_b32.to_uppercase());
        let totp = TOTP::new(
            Algorithm::SHA1,
            6,
            1,
            30,
            secret
                .to_bytes()
                .map_err(|e| anyhow::anyhow!("TOTP シークレットが不正です: {}", e))?,
            Some(issuer.to_string()),
            "web-terminal".to_string(),
        )
        .map_err(|e| anyhow::anyhow!("TOTP 初期化エラー: {}", e))?;

        Ok(Self {
            totp,
            secret_b32: secret_b32.to_uppercase(),
            used_codes: Mutex::new(HashSet::new()),
        })
    }

    /// 暗号論的に安全なランダムシークレットを生成する（Base32 文字列）
    pub fn generate_secret() -> String {
        Secret::generate_secret().to_string()
    }

    /// QR コード生成に使う otpauth:// URL を返す
    pub fn get_url(&self) -> String {
        self.totp.get_url()
    }

    /// 保存・表示用の Base32 シークレット文字列を返す
    pub fn secret_b32(&self) -> &str {
        &self.secret_b32
    }

    /// 6 桁の OTP コードを現在の時刻ウィンドウで検証する。
    ///
    /// 検証成功時は使用済みセットに記録し、同一 (window, code) の再利用を拒否する。
    /// ±1 ウィンドウ（最大 90 秒）を許容し、その間のリプレイを検出する。
    pub fn verify(&self, code: &str) -> bool {
        let code = code.trim();
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }

        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs(),
            Err(_) => return false,
        };
        let current_window = (now / TOTP_STEP) as i64;

        // ±1 ウィンドウを試す（RFC 6238 では時刻ドリフト許容）
        for offset in [-1_i64, 0, 1] {
            let window = current_window + offset;
            if window < 0 {
                continue;
            }
            let window = window as u64;
            let unix_time = window * TOTP_STEP;

            let expected = match self.totp.generate(unix_time) {
                s if s.len() == code.len() => s,
                _ => continue,
            };

            // 定数時間比較（タイミング攻撃対策）
            if expected.as_bytes().ct_eq(code.as_bytes()).unwrap_u8() == 1 {
                return self.try_record_use(window, expected, current_window);
            }
        }
        false
    }

    /// 使用済みセットに `(window, code)` を記録する。既に記録されていれば拒否（リプレイ）。
    /// 古いウィンドウのエントリは GC する。
    fn try_record_use(&self, window: u64, code: String, current_window: i64) -> bool {
        let mut used = match self.used_codes.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!("TOTP used_codes mutex がポイズン状態。回復して継続します");
                poisoned.into_inner()
            }
        };

        if !used.insert((window, code)) {
            tracing::warn!(
                "TOTP リプレイ攻撃を検出: window={} のコードが再利用されました",
                window
            );
            return false;
        }

        // GC: 現在ウィンドウから REPLAY_WINDOW_RANGE 以上古いエントリを削除
        let cutoff = current_window.saturating_sub(REPLAY_WINDOW_RANGE);
        used.retain(|(w, _)| (*w as i64) >= cutoff);

        true
    }
}

/// TOTP シークレットを nexterm.toml に書き込む
///
/// `[web]` / `[web.auth]` テーブルが未存在の場合は自動的に生成する。
pub fn save_secret_to_config(secret_b32: &str) -> anyhow::Result<()> {
    use toml_edit::DocumentMut;

    let path = nexterm_config::toml_path();

    let content = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };

    let mut doc = content
        .parse::<DocumentMut>()
        .unwrap_or_else(|_| DocumentMut::new());

    // [web] が存在しなければ作成する
    if !doc.contains_table("web") {
        doc["web"] = toml_edit::table();
    }

    // [web.auth] が存在しなければ作成する
    {
        let web = doc["web"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("[web] がテーブルではありません"))?;
        if !web.contains_key("auth") {
            web["auth"] = toml_edit::table();
        }
        web["auth"]["totp_secret"] = toml_edit::value(secret_b32);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, doc.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_generation_is_unique() {
        let secret1 = TotpManager::generate_secret();
        let secret2 = TotpManager::generate_secret();
        assert_ne!(secret1, secret2);
    }

    #[test]
    fn generate_secret_returns_non_empty() {
        let secret = TotpManager::generate_secret();
        assert!(!secret.is_empty());
    }

    #[test]
    fn from_secret_invalid_base32_fails() {
        let invalid_secret = "INVALID!@#$";
        let result = TotpManager::from_secret(invalid_secret, "TestIssuer");
        assert!(result.is_err());
    }

    /// テスト用に有効な Base32 シークレットから TotpManager を生成する
    fn test_manager() -> TotpManager {
        // 32 文字 = 160 ビットの Base32 シークレット（RFC 6238 で 128 ビット以上推奨）
        TotpManager::from_secret("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP", "TestIssuer")
            .expect("有効なシークレット")
    }

    #[test]
    fn 不正な形式のコードは拒否される() {
        let mgr = test_manager();
        assert!(!mgr.verify(""));
        assert!(!mgr.verify("12345")); // 5 桁
        assert!(!mgr.verify("1234567")); // 7 桁
        assert!(!mgr.verify("12345a")); // 数字以外
        assert!(!mgr.verify("abcdef")); // 全部文字
    }

    #[test]
    fn 正しいコードは初回受け付けられる() {
        let mgr = test_manager();
        // 現在ウィンドウのコードを生成
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let window = now / TOTP_STEP;
        let code = mgr.totp.generate(window * TOTP_STEP);

        assert!(mgr.verify(&code), "現在ウィンドウのコードは受け付けるべき");
    }

    #[test]
    fn リプレイ攻撃は拒否される() {
        // CRITICAL #6: 同一 OTP コードを複数回使用しようとした場合に拒否
        let mgr = test_manager();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let window = now / TOTP_STEP;
        let code = mgr.totp.generate(window * TOTP_STEP);

        // 1 回目: 受け付け
        assert!(mgr.verify(&code), "初回は受け付けるべき");

        // 2 回目（リプレイ）: 拒否
        assert!(
            !mgr.verify(&code),
            "同じコードの 2 回目使用はリプレイとして拒否されるべき"
        );

        // 3 回目（さらにリプレイ）: 拒否
        assert!(
            !mgr.verify(&code),
            "同じコードの 3 回目使用も拒否されるべき"
        );
    }

    #[test]
    fn 異なるウィンドウのコードは独立に検証される() {
        let mgr = test_manager();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_window = now / TOTP_STEP;

        // 現在ウィンドウのコードを使用
        let code_now = mgr.totp.generate(current_window * TOTP_STEP);
        assert!(mgr.verify(&code_now));

        // 1 つ前のウィンドウのコード（許容範囲内）
        // ※ now が 30 秒未満経過なら前ウィンドウは存在する
        if current_window > 0 {
            let code_prev = mgr.totp.generate((current_window - 1) * TOTP_STEP);
            // 同一コードでない限り受付（前ウィンドウは独立扱い）
            // ただし TOTP コードは時刻に依存するため通常は別の値
            if code_prev != code_now {
                let prev_accepted = mgr.verify(&code_prev);
                // ±1 ウィンドウ内の検証なので前ウィンドウのコードも受付（時刻ドリフト許容）
                assert!(
                    prev_accepted,
                    "前ウィンドウのコードは時刻ドリフト許容で受付"
                );
                // 同コードの 2 回目は拒否
                assert!(!mgr.verify(&code_prev));
            }
        }
    }

    #[test]
    fn 古いウィンドウのエントリは_gc_される() {
        let mgr = test_manager();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_window = now / TOTP_STEP;

        // 古い偽データを直接挿入（現実には起きないが GC ロジックの確認）
        {
            let mut used = mgr.used_codes.lock().unwrap();
            used.insert((current_window.saturating_sub(100), "999999".to_string()));
            used.insert((current_window.saturating_sub(50), "888888".to_string()));
        }

        // 現在ウィンドウのコードを検証 → GC が走る
        let code = mgr.totp.generate(current_window * TOTP_STEP);
        mgr.verify(&code);

        // GC により古いエントリは削除されているはず
        let used = mgr.used_codes.lock().unwrap();
        assert!(
            used.iter()
                .all(|(w, _)| (*w as i64) >= current_window as i64 - REPLAY_WINDOW_RANGE),
            "GC 範囲外のエントリが残存している: {:?}",
            *used
        );
    }
}
