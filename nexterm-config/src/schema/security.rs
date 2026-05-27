//! Security / consent-policy configuration (Sprint 4-1).
//!
//! Controls the user-consent flow for sensitive operations:
//! - Confirmation before opening an external URL.
//! - OSC 52 clipboard-write requests.
//! - OSC 9 / 777 desktop-notification requests.
//!
//! Each policy is one of `allow` / `deny` / `prompt`. The default is `prompt`
//! for all of them (asks the user for consent).

use serde::{Deserialize, Serialize};

/// Default behavior for a sensitive operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConsentPolicy {
    /// Allow without prompting.
    Allow,
    /// Deny without prompting.
    Deny,
    /// Show a confirmation modal and defer to the user (default).
    #[default]
    Prompt,
}

/// Security configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Behavior when opening an external URL via an OSC 8 hyperlink or
    /// Ctrl+click.
    #[serde(default)]
    pub external_url: ConsentPolicy,

    /// Behavior when an OSC 52 (clipboard-write) request is received.
    #[serde(default)]
    pub osc52_clipboard: ConsentPolicy,

    /// Behavior when an OSC 9 / 777 (desktop-notification) request is received.
    #[serde(default)]
    pub osc_notification: ConsentPolicy,

    /// Maximum size (bytes) of text that OSC 52 may write. Larger requests are
    /// rejected unconditionally.
    #[serde(default = "default_osc52_max_bytes")]
    pub osc52_max_bytes: usize,

    /// Maximum size (bytes) of text that OSC 9 / 777 may send. Excess content
    /// is truncated.
    #[serde(default = "default_notification_max_bytes")]
    pub notification_max_bytes: usize,
}

fn default_osc52_max_bytes() -> usize {
    // Sufficient for ordinary clipboard operations; blocks DoS attacks via
    // gigantic payloads.
    1024 * 1024 // 1 MiB
}

fn default_notification_max_bytes() -> usize {
    4096 // Notifications should stay short.
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
    fn default_is_prompt_everywhere() {
        let cfg = SecurityConfig::default();
        assert_eq!(cfg.external_url, ConsentPolicy::Prompt);
        assert_eq!(cfg.osc52_clipboard, ConsentPolicy::Prompt);
        assert_eq!(cfg.osc_notification, ConsentPolicy::Prompt);
    }

    #[test]
    fn default_size_limits() {
        let cfg = SecurityConfig::default();
        assert_eq!(cfg.osc52_max_bytes, 1024 * 1024);
        assert_eq!(cfg.notification_max_bytes, 4096);
    }

    #[test]
    fn allow_is_parsed_from_toml() {
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
    fn toml_roundtrip() {
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
