//! Animation configuration (Sprint 5-7 / Phase 3-2).
//!
//! Controls every UI animation, including tab switching, pane insertion, and
//! the cursor blink. Setting `enabled = false` or `intensity = "off"` disables
//! them entirely, which lets the application respect a reduced-motion
//! accessibility preference.

use serde::{Deserialize, Serialize};

/// Animation intensity (provides the factor by which the duration is scaled).
///
/// Levels:
/// - `Off`     — apply instantly (0 ms).
/// - `Subtle`  — restrained (duration × 0.5).
/// - `Normal`  — standard (duration × 1.0, the default).
/// - `Energetic` — pronounced (duration × 1.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AnimationIntensity {
    /// Disabled (duration = 0).
    Off,
    /// Subtle (× 0.5).
    Subtle,
    /// Standard (× 1.0).
    #[default]
    Normal,
    /// Energetic (× 1.5).
    Energetic,
}

impl AnimationIntensity {
    /// Returns the multiplier applied to the base duration (in milliseconds).
    pub fn multiplier(&self) -> f32 {
        match self {
            AnimationIntensity::Off => 0.0,
            AnimationIntensity::Subtle => 0.5,
            AnimationIntensity::Normal => 1.0,
            AnimationIntensity::Energetic => 1.5,
        }
    }
}

/// Top-level animation configuration.
///
/// ```toml
/// [animations]
/// enabled = true
/// intensity = "normal"  # off / subtle / normal / energetic
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationsConfig {
    /// Master switch. When `false`, every animation is applied instantly
    /// regardless of `intensity`.
    #[serde(default = "default_animations_enabled")]
    pub enabled: bool,
    /// Animation intensity (off / subtle / normal / energetic).
    #[serde(default)]
    pub intensity: AnimationIntensity,
}

fn default_animations_enabled() -> bool {
    // Enabled by default. Users who prefer reduced motion can disable
    // animations with `enabled = false`.
    true
}

impl Default for AnimationsConfig {
    fn default() -> Self {
        Self {
            enabled: default_animations_enabled(),
            intensity: AnimationIntensity::default(),
        }
    }
}

impl AnimationsConfig {
    /// Returns the effective multiplier (0 when `enabled = false` or
    /// `intensity = Off`).
    pub fn effective_multiplier(&self) -> f32 {
        if self.enabled {
            self.intensity.multiplier()
        } else {
            0.0
        }
    }

    /// Returns the effective duration (the base milliseconds scaled by the
    /// multiplier). A return value of `0` means "no animation; apply instantly".
    pub fn scaled_duration_ms(&self, base_ms: u32) -> u32 {
        let mult = self.effective_multiplier();
        if mult <= 0.0 {
            return 0;
        }
        (base_ms as f32 * mult).round() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_enabled_and_normal() {
        let cfg = AnimationsConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.intensity, AnimationIntensity::Normal);
        assert!((cfg.effective_multiplier() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn intensity_multipliers_are_correct() {
        assert!((AnimationIntensity::Off.multiplier() - 0.0).abs() < f32::EPSILON);
        assert!((AnimationIntensity::Subtle.multiplier() - 0.5).abs() < f32::EPSILON);
        assert!((AnimationIntensity::Normal.multiplier() - 1.0).abs() < f32::EPSILON);
        assert!((AnimationIntensity::Energetic.multiplier() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn enabled_false_yields_zero() {
        let cfg = AnimationsConfig {
            enabled: false,
            intensity: AnimationIntensity::Energetic,
        };
        assert_eq!(cfg.effective_multiplier(), 0.0);
        assert_eq!(cfg.scaled_duration_ms(200), 0);
    }

    #[test]
    fn off_yields_zero() {
        let cfg = AnimationsConfig {
            enabled: true,
            intensity: AnimationIntensity::Off,
        };
        assert_eq!(cfg.effective_multiplier(), 0.0);
        assert_eq!(cfg.scaled_duration_ms(200), 0);
    }

    #[test]
    fn scaled_duration_ms_honors_the_multiplier() {
        let cfg = AnimationsConfig {
            enabled: true,
            intensity: AnimationIntensity::Subtle,
        };
        assert_eq!(cfg.scaled_duration_ms(200), 100); // 200 × 0.5
        let cfg = AnimationsConfig {
            enabled: true,
            intensity: AnimationIntensity::Energetic,
        };
        assert_eq!(cfg.scaled_duration_ms(200), 300); // 200 × 1.5
    }

    #[test]
    fn parses_from_toml() {
        let toml_str = r#"
[animations]
enabled = true
intensity = "subtle"
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        assert!(parsed.animations.enabled);
        assert_eq!(parsed.animations.intensity, AnimationIntensity::Subtle);
    }

    #[test]
    fn default_struct_toml_roundtrip() {
        let cfg = AnimationsConfig::default();
        let s = toml::to_string(&cfg).unwrap();
        let parsed: AnimationsConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, parsed);
    }
}
