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

/// Whether animations run (UI/UX v3 P3c).
///
/// Tri-state rather than a bool because there are three distinct user
/// intents: "do what my OS asks" (the default), "animate regardless", and
/// "never animate". The middle one is the escape hatch — an OS-wide reduced
/// motion preference is not always what someone wants inside a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimationsEnabled {
    /// Follow the OS accessibility preference; animate where there is none.
    #[default]
    Auto,
    /// Always animate, even when the OS asks for reduced motion.
    Yes,
    /// Never animate.
    No,
}

impl<'de> Deserialize<'de> for AnimationsEnabled {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Str(String),
        }
        // Accepts the pre-P3c booleans unchanged; `"auto"` is the new spelling.
        match Raw::deserialize(d)? {
            Raw::Bool(true) => Ok(Self::Yes),
            Raw::Bool(false) => Ok(Self::No),
            Raw::Str(s) if s.eq_ignore_ascii_case("auto") => Ok(Self::Auto),
            Raw::Str(s) if s.eq_ignore_ascii_case("true") => Ok(Self::Yes),
            Raw::Str(s) if s.eq_ignore_ascii_case("false") => Ok(Self::No),
            Raw::Str(s) => Err(serde::de::Error::custom(format!(
                "animations.enabled must be true, false or \"auto\" (got {s:?})"
            ))),
        }
    }
}

impl Serialize for AnimationsEnabled {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str("auto"),
            Self::Yes => s.serialize_bool(true),
            Self::No => s.serialize_bool(false),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AnimationsConfig {
    /// Master switch: `auto` (follow the OS), `true`, or `false`.
    #[serde(default)]
    pub enabled: AnimationsEnabled,
    /// Animation intensity (off / subtle / normal / energetic).
    #[serde(default)]
    pub intensity: AnimationIntensity,
    /// What the OS accessibility preference last reported, written by the
    /// client's platform layer. Never read from or written to `config.toml`:
    /// it is not the user's setting, and persisting it would bake a machine's
    /// state into a file the user edits and syncs.
    #[serde(skip)]
    os_reduced_motion: bool,
}

impl AnimationsConfig {
    /// Record what the OS accessibility preference reports. Called by the
    /// client at startup and whenever the window regains focus.
    pub fn set_os_reduced_motion(&mut self, reduced: bool) {
        self.os_reduced_motion = reduced;
    }

    /// What the OS last reported. The settings panel shows this so the
    /// `auto` row can say which way it currently resolves.
    pub fn os_reduced_motion(&self) -> bool {
        self.os_reduced_motion
    }

    /// Returns the effective multiplier (0 when motion is off).
    pub fn effective_multiplier(&self) -> f32 {
        match self.enabled {
            AnimationsEnabled::No => 0.0,
            AnimationsEnabled::Auto if self.os_reduced_motion => 0.0,
            AnimationsEnabled::Auto | AnimationsEnabled::Yes => self.intensity.multiplier(),
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
        assert_eq!(cfg.enabled, AnimationsEnabled::Auto);
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
            enabled: AnimationsEnabled::No,
            intensity: AnimationIntensity::Energetic,
            ..AnimationsConfig::default()
        };
        assert_eq!(cfg.effective_multiplier(), 0.0);
        assert_eq!(cfg.scaled_duration_ms(200), 0);
    }

    #[test]
    fn off_yields_zero() {
        let cfg = AnimationsConfig {
            enabled: AnimationsEnabled::Yes,
            intensity: AnimationIntensity::Off,
            ..AnimationsConfig::default()
        };
        assert_eq!(cfg.effective_multiplier(), 0.0);
        assert_eq!(cfg.scaled_duration_ms(200), 0);
    }

    #[test]
    fn scaled_duration_ms_honors_the_multiplier() {
        let cfg = AnimationsConfig {
            enabled: AnimationsEnabled::Yes,
            intensity: AnimationIntensity::Subtle,
            ..AnimationsConfig::default()
        };
        assert_eq!(cfg.scaled_duration_ms(200), 100); // 200 × 0.5
        let cfg = AnimationsConfig {
            enabled: AnimationsEnabled::Yes,
            intensity: AnimationIntensity::Energetic,
            ..AnimationsConfig::default()
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
        assert_eq!(parsed.animations.enabled, AnimationsEnabled::Yes);
        assert_eq!(parsed.animations.intensity, AnimationIntensity::Subtle);
    }

    #[test]
    fn default_struct_toml_roundtrip() {
        let cfg = AnimationsConfig::default();
        let s = toml::to_string(&cfg).unwrap();
        let parsed: AnimationsConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn auto_follows_the_os_when_it_asks_for_reduced_motion() {
        let mut cfg = AnimationsConfig::default();
        assert_eq!(cfg.enabled, AnimationsEnabled::Auto, "auto is the default");
        assert!(cfg.effective_multiplier() > 0.0);
        cfg.set_os_reduced_motion(true);
        assert_eq!(cfg.effective_multiplier(), 0.0);
        assert_eq!(cfg.scaled_duration_ms(200), 0);
    }

    /// The escape hatch: an explicit `true` outranks the OS. Without this the
    /// setting would be unusable for a user whose OS-wide preference is not
    /// what they want inside a terminal.
    #[test]
    fn an_explicit_yes_overrules_the_os() {
        let mut cfg = AnimationsConfig {
            enabled: AnimationsEnabled::Yes,
            intensity: AnimationIntensity::Normal,
            ..AnimationsConfig::default()
        };
        cfg.set_os_reduced_motion(true);
        assert_eq!(cfg.effective_multiplier(), 1.0);
        assert_eq!(cfg.scaled_duration_ms(200), 200);
    }

    /// Detection only ever disables. An OS that is *not* asking for reduced
    /// motion must never revive animations the user turned off.
    #[test]
    fn the_os_can_never_enable_motion() {
        let mut cfg = AnimationsConfig {
            enabled: AnimationsEnabled::No,
            intensity: AnimationIntensity::Energetic,
            ..AnimationsConfig::default()
        };
        cfg.set_os_reduced_motion(false);
        assert_eq!(cfg.effective_multiplier(), 0.0);
    }

    #[test]
    fn the_pre_p3c_boolean_spellings_still_parse() {
        let on: AnimationsConfig = toml::from_str("enabled = true").expect("bool true parses");
        assert_eq!(on.enabled, AnimationsEnabled::Yes);
        let off: AnimationsConfig = toml::from_str("enabled = false").expect("bool false parses");
        assert_eq!(off.enabled, AnimationsEnabled::No);
        let auto: AnimationsConfig = toml::from_str(r#"enabled = "auto""#).expect("auto parses");
        assert_eq!(auto.enabled, AnimationsEnabled::Auto);
        let omitted: AnimationsConfig = toml::from_str("").expect("empty parses");
        assert_eq!(omitted.enabled, AnimationsEnabled::Auto);
    }

    /// The write-back boundary. The settings panel serializes this struct's
    /// neighbours through `toml_edit`; an OS-derived value that leaked into
    /// the document would persist a setting the user never chose.
    #[test]
    fn the_os_flag_is_never_serialized() {
        let mut cfg = AnimationsConfig::default();
        cfg.set_os_reduced_motion(true);
        let text = toml::to_string(&cfg).expect("serializes");
        assert!(
            !text.contains("os_reduced_motion"),
            "OS state leaked into the document: {text}"
        );
        assert!(
            text.contains("auto"),
            "auto round-trips as a string: {text}"
        );
    }
}
