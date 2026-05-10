//! セキュリティ・同意ポリシー設定（Sprint 4-1）
//!
//! 機密操作に対するユーザー同意フローを制御する:
//! - 外部 URL クリック前の確認
//! - OSC 52 クリップボード書き込み要求
//! - OSC 9 / 777 デスクトップ通知要求
//!
//! 各ポリシーは `allow` / `deny` / `prompt` のいずれか。
//! デフォルトはすべて `prompt`（ユーザーに同意を求める）。

use serde::{Deserialize, Serialize};

/// 機密操作のデフォルト動作
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConsentPolicy {
    /// 確認なしで許可する
    Allow,
    /// 確認なしで拒否する
    Deny,
    /// 確認モーダルを表示してユーザー判断を仰ぐ（デフォルト）
    #[default]
    Prompt,
}

/// セキュリティ設定
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// OSC 8 ハイパーリンクや Ctrl+クリックによる外部 URL オープン時の動作
    #[serde(default)]
    pub external_url: ConsentPolicy,

    /// OSC 52（クリップボード書き込み要求）受信時の動作
    #[serde(default)]
    pub osc52_clipboard: ConsentPolicy,

    /// OSC 9 / 777（デスクトップ通知要求）受信時の動作
    #[serde(default)]
    pub osc_notification: ConsentPolicy,

    /// OSC 52 で書き込めるテキストの最大長（バイト単位）。これを超える要求は無条件で拒否する
    #[serde(default = "default_osc52_max_bytes")]
    pub osc52_max_bytes: usize,

    /// OSC 9 / 777 で通知できるテキストの最大長（バイト単位）。超過分は切り詰める
    #[serde(default = "default_notification_max_bytes")]
    pub notification_max_bytes: usize,
}

fn default_osc52_max_bytes() -> usize {
    // 一般的なクリップボード操作で十分な上限。攻撃者による巨大ペイロード DoS を防ぐ
    1024 * 1024 // 1 MiB
}

fn default_notification_max_bytes() -> usize {
    4096 // 通知は短く保つ
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            external_url: ConsentPolicy::default(),
            osc52_clipboard: ConsentPolicy::default(),
            osc_notification: ConsentPolicy::default(),
            osc52_max_bytes: default_osc52_max_bytes(),
            notification_max_bytes: default_notification_max_bytes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn デフォルトはすべて_prompt() {
        let cfg = SecurityConfig::default();
        assert_eq!(cfg.external_url, ConsentPolicy::Prompt);
        assert_eq!(cfg.osc52_clipboard, ConsentPolicy::Prompt);
        assert_eq!(cfg.osc_notification, ConsentPolicy::Prompt);
    }

    #[test]
    fn デフォルトサイズ上限() {
        let cfg = SecurityConfig::default();
        assert_eq!(cfg.osc52_max_bytes, 1024 * 1024);
        assert_eq!(cfg.notification_max_bytes, 4096);
    }

    #[test]
    fn toml_から_allow_を読める() {
        let toml_str = r#"
external_url = "allow"
osc52_clipboard = "deny"
osc_notification = "prompt"
"#;
        let cfg: SecurityConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.external_url, ConsentPolicy::Allow);
        assert_eq!(cfg.osc52_clipboard, ConsentPolicy::Deny);
        assert_eq!(cfg.osc_notification, ConsentPolicy::Prompt);
    }

    #[test]
    fn toml_往復() {
        let cfg = SecurityConfig {
            external_url: ConsentPolicy::Deny,
            osc52_clipboard: ConsentPolicy::Allow,
            osc_notification: ConsentPolicy::Prompt,
            osc52_max_bytes: 2048,
            notification_max_bytes: 1024,
        };
        let s = toml::to_string(&cfg).unwrap();
        let parsed: SecurityConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, parsed);
    }
}
