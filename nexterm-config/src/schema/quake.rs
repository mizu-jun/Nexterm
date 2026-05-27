//! Quake-mode configuration (Sprint 5-7 / Phase 2-2).
//!
//! Quake mode is a toggle that slides the terminal window in from the top edge
//! of the screen (or the bottom, left, or right edge) via a global hotkey.
//! Equivalent to the "Hotkey Window" feature in Tilix, Guake, and iTerm2.
//!
//! Configure it under the `[quake_mode]` section of `config.toml`:
//!
//! ```toml
//! [quake_mode]
//! enabled = true
//! hotkey = "ctrl+`"
//! edge = "top"
//! height_pct = 45
//! width_pct = 100
//! animation_ms = 150
//! ```
//!
//! Platform notes:
//! - On Linux/Wayland, global hotkeys can only be implemented through the
//!   compositor by spec. This implementation relies on the `global-hotkey`
//!   crate and therefore works on Windows / macOS / Linux X11 but not on
//!   Wayland. Wayland users should fall back to invoking
//!   `nexterm-ctl quake toggle` from the compositor's `bindsym` (see the README).

use serde::{Deserialize, Serialize};

/// Anchor edge for the Quake window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum QuakeEdge {
    /// Top edge (default).
    #[default]
    Top,
    /// Bottom edge.
    Bottom,
    /// Left edge.
    Left,
    /// Right edge.
    Right,
}

/// Quake-mode configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuakeModeConfig {
    /// Whether Quake mode is enabled.
    /// When `false`, no hotkey is registered and the window decorations are
    /// left as-is.
    pub enabled: bool,
    /// Global hotkey string (e.g. `"ctrl+`"`, `"alt+space"`).
    /// Modifiers are joined with `+`: `ctrl` / `alt` / `shift` / `super`
    /// (or `meta` / `cmd` / `win`). The last token is the primary key.
    /// See the `global-hotkey` crate for full details.
    pub hotkey: String,
    /// Anchor position (`top` / `bottom` / `left` / `right`).
    pub edge: QuakeEdge,
    /// Percentage of the screen height to occupy (when `edge` is
    /// top/bottom; 1..=100).
    pub height_pct: u8,
    /// Percentage of the screen width to occupy (when `edge` is left/right —
    /// or top/bottom if you want to narrow the window; 1..=100).
    pub width_pct: u8,
    /// Slide-animation duration in milliseconds (0 disables the animation).
    pub animation_ms: u32,
    /// Keep the window topmost while it is visible.
    pub always_on_top: bool,
    /// Whether to minimize the window when hiding (otherwise just
    /// `set_visible(false)` is used).
    /// On macOS, minimizing maps to `Hide`, which removes the window from the
    /// Dock as well; `false` is recommended for UX reasons.
    pub minimize_on_hide: bool,
}

fn default_hotkey() -> String {
    // Ctrl + backtick (tilde key). Matches the defaults used by Guake / Tilix.
    "ctrl+`".to_string()
}

fn default_height_pct() -> u8 {
    45
}

fn default_width_pct() -> u8 {
    100
}

fn default_animation_ms() -> u32 {
    150
}

impl Default for QuakeModeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: default_hotkey(),
            edge: QuakeEdge::default(),
            height_pct: default_height_pct(),
            width_pct: default_width_pct(),
            animation_ms: default_animation_ms(),
            always_on_top: true,
            minimize_on_hide: false,
        }
    }
}

impl QuakeModeConfig {
    /// Returns `height_pct` clamped to the range 1..=100.
    pub fn clamped_height_pct(&self) -> u8 {
        self.height_pct.clamp(1, 100)
    }

    /// Returns `width_pct` clamped to the range 1..=100.
    pub fn clamped_width_pct(&self) -> u8 {
        self.width_pct.clamp(1, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quake_is_disabled_by_default() {
        let cfg = QuakeModeConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.hotkey, "ctrl+`");
        assert_eq!(cfg.edge, QuakeEdge::Top);
        assert_eq!(cfg.height_pct, 45);
        assert_eq!(cfg.width_pct, 100);
        assert_eq!(cfg.animation_ms, 150);
        assert!(cfg.always_on_top);
        assert!(!cfg.minimize_on_hide);
    }

    #[test]
    fn quake_clamped_pct_stays_in_range() {
        let cfg_low = QuakeModeConfig {
            height_pct: 0,
            width_pct: 0,
            ..QuakeModeConfig::default()
        };
        assert_eq!(cfg_low.clamped_height_pct(), 1);
        assert_eq!(cfg_low.clamped_width_pct(), 1);

        let cfg_high = QuakeModeConfig {
            height_pct: 200,
            width_pct: 200,
            ..QuakeModeConfig::default()
        };
        assert_eq!(cfg_high.clamped_height_pct(), 100);
        assert_eq!(cfg_high.clamped_width_pct(), 100);
    }

    #[test]
    fn quake_toml_roundtrip() {
        let cfg = QuakeModeConfig {
            enabled: true,
            hotkey: "alt+space".to_string(),
            edge: QuakeEdge::Bottom,
            height_pct: 60,
            width_pct: 80,
            animation_ms: 200,
            always_on_top: false,
            minimize_on_hide: true,
        };
        let s = toml::to_string(&cfg).unwrap();
        let parsed: QuakeModeConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn quake_partial_toml_fills_defaults() {
        let toml_str = r#"
enabled = true
hotkey = "ctrl+space"
"#;
        let parsed: QuakeModeConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.hotkey, "ctrl+space");
        // The rest stays at the defaults.
        assert_eq!(parsed.edge, QuakeEdge::Top);
        assert_eq!(parsed.height_pct, 45);
    }
}
