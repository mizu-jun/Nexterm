//! TOTP (Time-based One-Time Password) — RFC 6238 に準拠した OTP 認証
//!
//! シークレットは nexterm.toml の `[web.auth]` セクションに Base32 形式で保存する。
//! 初回起動時にシークレットが未設定の場合はブラウザ上でセットアップを行う。

use totp_rs::{Algorithm, Secret, TOTP};

/// TOTP マネージャー — シークレットと TOTP インスタンスを保持する
pub struct TotpManager {
    totp: TOTP,
    /// Base32 エンコードされたシークレット（表示・保存用）
    secret_b32: String,
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

    /// 6 桁の OTP コードを現在の時刻ウィンドウで検証する
    pub fn verify(&self, code: &str) -> bool {
        self.totp.check_current(code.trim()).unwrap_or(false)
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
}
