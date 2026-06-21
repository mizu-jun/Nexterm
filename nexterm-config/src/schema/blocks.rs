//! Command-blocks UI configuration.
//!
//! Drives the Warp-style block overlay in the GPU renderer (left border
//! coloured by exit status, status badge, selection highlight). The feature is
//! tied to OSC 133 shell integration: when the shell never emits prompt
//! markers no blocks exist and the overlay draws nothing regardless of these
//! settings.
//!
//! Example:
//! ```toml
//! [blocks]
//! enabled = true
//! border_width_px = 2
//! show_exit_code_badge = true
//! ```

use serde::{Deserialize, Serialize};

/// Command-blocks UI configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlocksConfig {
    /// Master switch. When `false` the overlay pass is skipped entirely and
    /// the renderer behaves as it did before the blocks feature.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Width in pixels of the left border drawn beside each block. Clamped to
    /// `1..=8` by the renderer.
    #[serde(default = "default_border_width_px")]
    pub border_width_px: u8,

    /// Whether to show a small status badge (✓ / ✗ / ●) in the right margin
    /// next to each block's prompt row.
    #[serde(default = "default_show_exit_code_badge")]
    pub show_exit_code_badge: bool,
}

fn default_enabled() -> bool {
    true
}

fn default_border_width_px() -> u8 {
    2
}

fn default_show_exit_code_badge() -> bool {
    true
}

impl Default for BlocksConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            border_width_px: default_border_width_px(),
            show_exit_code_badge: default_show_exit_code_badge(),
        }
    }
}

impl BlocksConfig {
    /// Effective border width clamped to a safe range.
    pub fn effective_border_width_px(&self) -> u8 {
        self.border_width_px.clamp(1, 8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_enabled_with_2px_border_and_badge() {
        let cfg = BlocksConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.border_width_px, 2);
        assert!(cfg.show_exit_code_badge);
    }

    #[test]
    fn deserialise_from_empty_table_uses_defaults() {
        let cfg: BlocksConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, BlocksConfig::default());
    }

    #[test]
    fn deserialise_honours_user_values() {
        let cfg: BlocksConfig = toml::from_str(
            r#"
            enabled = false
            border_width_px = 4
            show_exit_code_badge = false
            "#,
        )
        .unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.border_width_px, 4);
        assert!(!cfg.show_exit_code_badge);
    }

    #[test]
    fn border_width_is_clamped() {
        let cfg = BlocksConfig {
            border_width_px: 0,
            ..BlocksConfig::default()
        };
        assert_eq!(cfg.effective_border_width_px(), 1);
        let cfg = BlocksConfig {
            border_width_px: 32,
            ..BlocksConfig::default()
        };
        assert_eq!(cfg.effective_border_width_px(), 8);
    }
}
