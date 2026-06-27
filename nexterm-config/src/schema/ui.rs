//! UI chrome appearance configuration (Sprint 5-15 / Phase 1).
//!
//! Centralises pixel-radius knobs used by the SDF rounded-rect background
//! pipeline. Setting any radius to `0.0` produces pixel-identical output to
//! pre-v2 builds (the shader takes its flat-rect early-return path).

use serde::{Deserialize, Serialize};

/// UI chrome rounding configuration.
///
/// ```toml
/// [ui]
/// corner_radius_chrome  = 6.0   # tab pills, focused-pane outline
/// corner_radius_overlay = 10.0  # command palette, settings panel, dialogs
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Corner radius in pixels for chrome surfaces (tab pills, focused-pane
    /// outline, banners). Default `6.0`. `0.0` disables rounding.
    pub corner_radius_chrome: f32,
    /// Corner radius in pixels for overlay panels (command palette, settings
    /// panel, dialogs). Default `10.0`. `0.0` disables rounding.
    pub corner_radius_overlay: f32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            corner_radius_chrome: 6.0,
            corner_radius_overlay: 10.0,
        }
    }
}

impl UiConfig {
    /// Clamp the chrome radius to non-negative.
    pub fn chrome_radius(&self) -> f32 {
        self.corner_radius_chrome.max(0.0)
    }

    /// Clamp the overlay radius to non-negative.
    pub fn overlay_radius(&self) -> f32 {
        self.corner_radius_overlay.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let cfg = UiConfig::default();
        assert!((cfg.corner_radius_chrome - 6.0).abs() < f32::EPSILON);
        assert!((cfg.corner_radius_overlay - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn negative_radius_clamps_to_zero() {
        let cfg = UiConfig {
            corner_radius_chrome: -3.0,
            corner_radius_overlay: -1.0,
        };
        assert_eq!(cfg.chrome_radius(), 0.0);
        assert_eq!(cfg.overlay_radius(), 0.0);
    }

    #[test]
    fn parses_from_toml() {
        let toml_str = r#"
[ui]
corner_radius_chrome = 8.0
corner_radius_overlay = 12.0
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        assert!((parsed.ui.corner_radius_chrome - 8.0).abs() < f32::EPSILON);
        assert!((parsed.ui.corner_radius_overlay - 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn missing_section_uses_defaults() {
        let toml_str = r#"
language = "ja"
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        assert!((parsed.ui.corner_radius_chrome - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn roundtrip_preserves_values() {
        let cfg = UiConfig {
            corner_radius_chrome: 4.5,
            corner_radius_overlay: 14.0,
        };
        let s = toml::to_string(&cfg).expect("UiConfig should be serializable");
        let parsed: UiConfig = toml::from_str(&s).expect("output should be deserializable");
        assert_eq!(parsed, cfg);
    }
}
