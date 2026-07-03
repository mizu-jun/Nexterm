//! Scrolling configuration (wheel multiplier + touchpad momentum).
//!
//! Example:
//! ```toml
//! [scrolling]
//! multiplier = 3.0
//! momentum = true
//! ```

use serde::{Deserialize, Serialize};

/// Scrolling configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrollingConfig {
    /// Rows scrolled per discrete wheel notch (`LineDelta`). Clamped to
    /// `1.0..=20.0` by the client.
    #[serde(default = "default_multiplier")]
    pub multiplier: f32,

    /// Continue touchpad scrolls with simulated inertia after the fingers
    /// lift (kitty-style momentum). Off by default: Windows precision
    /// touchpads and macOS already synthesize inertial events at the OS
    /// level, so this mainly benefits Linux/X11. Applies to pixel-precision
    /// (touchpad) scrolling only — discrete wheels are never given inertia.
    #[serde(default = "default_momentum")]
    pub momentum: bool,
}

fn default_multiplier() -> f32 {
    3.0
}

fn default_momentum() -> bool {
    false
}

impl Default for ScrollingConfig {
    fn default() -> Self {
        Self {
            multiplier: default_multiplier(),
            momentum: default_momentum(),
        }
    }
}

impl ScrollingConfig {
    /// Wheel multiplier clamped to a sane range.
    pub fn effective_multiplier(&self) -> f32 {
        self.multiplier.clamp(1.0, 20.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_historical_behavior() {
        let cfg = ScrollingConfig::default();
        assert_eq!(cfg.multiplier, 3.0);
        assert!(!cfg.momentum);
    }

    #[test]
    fn deserialise_from_empty_table_uses_defaults() {
        let cfg: ScrollingConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, ScrollingConfig::default());
    }

    #[test]
    fn multiplier_is_clamped() {
        let cfg = ScrollingConfig {
            multiplier: 0.0,
            ..Default::default()
        };
        assert_eq!(cfg.effective_multiplier(), 1.0);
        let cfg = ScrollingConfig {
            multiplier: 100.0,
            ..Default::default()
        };
        assert_eq!(cfg.effective_multiplier(), 20.0);
    }
}
