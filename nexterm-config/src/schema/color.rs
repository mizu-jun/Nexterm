//! Color schemes (built-in palettes plus custom palettes).

use serde::{Deserialize, Serialize};

/// A color-scheme palette (foreground, background, and the ANSI 16-color set).
#[derive(Debug, Clone)]
pub struct SchemePalette {
    /// Default foreground color, as `[R, G, B]`.
    pub fg: [u8; 3],
    /// Default background color, as `[R, G, B]`.
    pub bg: [u8; 3],
    /// ANSI 16-color palette (0 = black … 15 = bright white).
    pub ansi: [[u8; 3]; 16],
}

/// Built-in color scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinScheme {
    /// Dark theme.
    Dark,
    /// Light theme.
    Light,
    /// Tokyo Night theme.
    TokyoNight,
    /// Solarized theme.
    Solarized,
    /// Gruvbox theme.
    Gruvbox,
    /// Catppuccin theme.
    Catppuccin,
    /// Dracula theme.
    Dracula,
    /// Nord theme.
    Nord,
    #[serde(rename = "onedark")]
    /// One Dark theme.
    OneDark,
}

impl BuiltinScheme {
    /// Returns the human-readable display name of the scheme.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::TokyoNight => "Tokyo Night",
            Self::Solarized => "Solarized",
            Self::Gruvbox => "Gruvbox",
            Self::Catppuccin => "Catppuccin",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::OneDark => "One Dark",
        }
    }

    /// Returns the TOML identifier (lower-case) of the scheme.
    pub fn toml_name(&self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::TokyoNight => "tokyonight",
            Self::Solarized => "solarized",
            Self::Gruvbox => "gruvbox",
            Self::Catppuccin => "catppuccin",
            Self::Dracula => "dracula",
            Self::Nord => "nord",
            Self::OneDark => "onedark",
        }
    }

    /// Returns every built-in scheme as a slice (Sprint 5-4 / D4: theme gallery).
    pub fn all() -> &'static [Self] {
        &[
            Self::Dark,
            Self::Light,
            Self::TokyoNight,
            Self::Solarized,
            Self::Gruvbox,
            Self::Catppuccin,
            Self::Dracula,
            Self::Nord,
            Self::OneDark,
        ]
    }

    /// Returns the built-in scheme matching the given TOML identifier.
    ///
    /// Matching is case-insensitive. Unknown names return `None` (the legacy
    /// `parse_builtin_scheme` fell back to `Dark`; this method is for strict
    /// validation).
    pub fn from_toml_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "tokyonight" => Some(Self::TokyoNight),
            "solarized" => Some(Self::Solarized),
            "gruvbox" => Some(Self::Gruvbox),
            "catppuccin" => Some(Self::Catppuccin),
            "dracula" => Some(Self::Dracula),
            "nord" => Some(Self::Nord),
            "onedark" => Some(Self::OneDark),
            _ => None,
        }
    }

    /// Returns the color palette of the scheme.
    pub fn palette(&self) -> SchemePalette {
        match self {
            Self::Dark => SchemePalette {
                fg: [0xD8, 0xD8, 0xD8],
                bg: [0x0D, 0x0D, 0x0D],
                ansi: [
                    [0x00, 0x00, 0x00],
                    [0x80, 0x00, 0x00],
                    [0x00, 0x80, 0x00],
                    [0x80, 0x80, 0x00],
                    [0x00, 0x00, 0x80],
                    [0x80, 0x00, 0x80],
                    [0x00, 0x80, 0x80],
                    [0xC0, 0xC0, 0xC0],
                    [0x80, 0x80, 0x80],
                    [0xFF, 0x00, 0x00],
                    [0x00, 0xFF, 0x00],
                    [0xFF, 0xFF, 0x00],
                    [0x00, 0x00, 0xFF],
                    [0xFF, 0x00, 0xFF],
                    [0x00, 0xFF, 0xFF],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::Light => SchemePalette {
                fg: [0x2C, 0x2C, 0x2C],
                bg: [0xF2, 0xF2, 0xF2],
                ansi: [
                    [0x00, 0x00, 0x00],
                    [0xC0, 0x00, 0x00],
                    [0x00, 0x80, 0x00],
                    [0x80, 0x80, 0x00],
                    [0x00, 0x00, 0xC0],
                    [0xC0, 0x00, 0xC0],
                    [0x00, 0x80, 0x80],
                    [0xC0, 0xC0, 0xC0],
                    [0x60, 0x60, 0x60],
                    [0xFF, 0x40, 0x40],
                    [0x00, 0xC0, 0x00],
                    [0xC0, 0xC0, 0x00],
                    [0x40, 0x40, 0xFF],
                    [0xFF, 0x40, 0xFF],
                    [0x00, 0xC0, 0xC0],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::TokyoNight => SchemePalette {
                fg: [0xC0, 0xCA, 0xF5],
                bg: [0x1A, 0x1B, 0x26],
                ansi: [
                    [0x15, 0x16, 0x20],
                    [0xF7, 0x76, 0x8E],
                    [0x9E, 0xCE, 0x6A],
                    [0xE0, 0xAF, 0x68],
                    [0x7A, 0xA2, 0xF7],
                    [0xBB, 0x9A, 0xF7],
                    [0x7D, 0xCF, 0xFF],
                    [0xA9, 0xB1, 0xD6],
                    [0x41, 0x4B, 0x67],
                    [0xF7, 0x76, 0x8E],
                    [0x9E, 0xCE, 0x6A],
                    [0xE0, 0xAF, 0x68],
                    [0x7A, 0xA2, 0xF7],
                    [0xBB, 0x9A, 0xF7],
                    [0x7D, 0xCF, 0xFF],
                    [0xC0, 0xCA, 0xF5],
                ],
            },
            Self::Solarized => SchemePalette {
                fg: [0x83, 0x94, 0x96],
                bg: [0x00, 0x2B, 0x36],
                ansi: [
                    [0x07, 0x36, 0x42],
                    [0xDC, 0x32, 0x2F],
                    [0x85, 0x99, 0x00],
                    [0xB5, 0x89, 0x00],
                    [0x26, 0x8B, 0xD2],
                    [0xD3, 0x36, 0x82],
                    [0x2A, 0xA1, 0x98],
                    [0xEE, 0xE8, 0xD5],
                    [0x00, 0x2B, 0x36],
                    [0xCB, 0x4B, 0x16],
                    [0x58, 0x6E, 0x75],
                    [0x65, 0x7B, 0x83],
                    [0x83, 0x94, 0x96],
                    [0x6C, 0x71, 0xC4],
                    [0x93, 0xA1, 0xA1],
                    [0xFD, 0xF6, 0xE3],
                ],
            },
            Self::Gruvbox => SchemePalette {
                fg: [0xEB, 0xDB, 0xB2],
                bg: [0x28, 0x28, 0x28],
                ansi: [
                    [0x28, 0x28, 0x28],
                    [0xCC, 0x24, 0x1D],
                    [0x98, 0x97, 0x1A],
                    [0xD7, 0x99, 0x21],
                    [0x45, 0x85, 0x88],
                    [0xB1, 0x62, 0x86],
                    [0x68, 0x9D, 0x6A],
                    [0xA8, 0x99, 0x84],
                    [0x92, 0x83, 0x74],
                    [0xFB, 0x49, 0x34],
                    [0xB8, 0xBB, 0x26],
                    [0xFA, 0xBD, 0x2F],
                    [0x83, 0xA5, 0x98],
                    [0xD3, 0x86, 0x9B],
                    [0x8E, 0xC0, 0x7C],
                    [0xEB, 0xDB, 0xB2],
                ],
            },
            Self::Catppuccin => SchemePalette {
                // Catppuccin Mocha
                fg: [0xCD, 0xD6, 0xF4],
                bg: [0x1E, 0x1E, 0x2E],
                ansi: [
                    [0x45, 0x47, 0x5A],
                    [0xF3, 0x8B, 0xA8],
                    [0xA6, 0xE3, 0xA1],
                    [0xF9, 0xE2, 0xAF],
                    [0x89, 0xB4, 0xFA],
                    [0xF5, 0xC2, 0xE7],
                    [0x94, 0xE2, 0xD5],
                    [0xBA, 0xC2, 0xDE],
                    [0x58, 0x5B, 0x70],
                    [0xF3, 0x8B, 0xA8],
                    [0xA6, 0xE3, 0xA1],
                    [0xF9, 0xE2, 0xAF],
                    [0x89, 0xB4, 0xFA],
                    [0xF5, 0xC2, 0xE7],
                    [0x94, 0xE2, 0xD5],
                    [0xA6, 0xAD, 0xC8],
                ],
            },
            Self::Dracula => SchemePalette {
                fg: [0xF8, 0xF8, 0xF2],
                bg: [0x28, 0x2A, 0x36],
                ansi: [
                    [0x21, 0x22, 0x2C],
                    [0xFF, 0x55, 0x55],
                    [0x50, 0xFA, 0x7B],
                    [0xF1, 0xFA, 0x8C],
                    [0xBD, 0x93, 0xF9],
                    [0xFF, 0x79, 0xC6],
                    [0x8B, 0xE9, 0xFD],
                    [0xF8, 0xF8, 0xF2],
                    [0x6B, 0x72, 0x89],
                    [0xFF, 0x6E, 0x6E],
                    [0x69, 0xFF, 0x94],
                    [0xFF, 0xFF, 0xA5],
                    [0xD6, 0xAC, 0xFF],
                    [0xFF, 0x92, 0xDF],
                    [0xA4, 0xFF, 0xFF],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::Nord => SchemePalette {
                fg: [0xD8, 0xDE, 0xE9],
                bg: [0x2E, 0x34, 0x40],
                ansi: [
                    [0x3B, 0x42, 0x52],
                    [0xBF, 0x61, 0x6A],
                    [0xA3, 0xBE, 0x8C],
                    [0xEB, 0xCB, 0x8B],
                    [0x81, 0xA1, 0xC1],
                    [0xB4, 0x8E, 0xAD],
                    [0x88, 0xC0, 0xD0],
                    [0xE5, 0xE9, 0xF0],
                    [0x4C, 0x56, 0x6A],
                    [0xBF, 0x61, 0x6A],
                    [0xA3, 0xBE, 0x8C],
                    [0xEB, 0xCB, 0x8B],
                    [0x81, 0xA1, 0xC1],
                    [0xB4, 0x8E, 0xAD],
                    [0x8F, 0xBD, 0xBB],
                    [0xEC, 0xEF, 0xF4],
                ],
            },
            Self::OneDark => SchemePalette {
                fg: [0xAB, 0xB2, 0xBF],
                bg: [0x28, 0x2C, 0x34],
                ansi: [
                    [0x28, 0x2C, 0x34],
                    [0xE0, 0x6C, 0x75],
                    [0x98, 0xC3, 0x79],
                    [0xE5, 0xC0, 0x7B],
                    [0x61, 0xAF, 0xEF],
                    [0xC6, 0x78, 0xDD],
                    [0x56, 0xB6, 0xC2],
                    [0xAB, 0xB2, 0xBF],
                    [0x5C, 0x63, 0x70],
                    [0xE0, 0x6C, 0x75],
                    [0x98, 0xC3, 0x79],
                    [0xE5, 0xC0, 0x7B],
                    [0x61, 0xAF, 0xEF],
                    [0xC6, 0x78, 0xDD],
                    [0x56, 0xB6, 0xC2],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
        }
    }
}

/// Custom color palette (defined in TOML).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomPalette {
    /// Foreground color (`#RRGGBB`).
    pub foreground: String,
    /// Background color (`#RRGGBB`).
    pub background: String,
    /// Cursor color (`#RRGGBB`).
    pub cursor: String,
    /// ANSI 16-color palette (`#RRGGBB × 16`: black, red, green, yellow, blue,
    /// magenta, cyan, white, and the eight bright variants).
    pub ansi: Vec<String>,
}

/// Color-scheme configuration.
///
/// TOML accepts three forms (handled by a custom deserializer):
/// 1. `colors = "tokyonight"` — string referring to a built-in scheme.
/// 2. `[colors] scheme = "tokyonight"` — table form (documented form).
/// 3. `[colors] foreground = "#..." background = "#..." ...` — full custom palette.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ColorScheme {
    /// Built-in scheme name.
    Builtin(BuiltinScheme),
    /// Custom palette.
    Custom(CustomPalette),
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::Builtin(BuiltinScheme::TokyoNight)
    }
}

impl<'de> Deserialize<'de> for ColorScheme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct ColorSchemeVisitor;

        impl<'de> Visitor<'de> for ColorSchemeVisitor {
            type Value = ColorScheme;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(
                    "a string (the name of a built-in scheme) or a table (`[colors] scheme = \"...\"` or a custom palette)",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(ColorScheme::Builtin(parse_builtin_scheme(value)))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(ColorScheme::Builtin(parse_builtin_scheme(&value)))
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                // Collect every key into a HashMap first.
                let mut entries: std::collections::HashMap<String, toml::Value> =
                    std::collections::HashMap::new();
                while let Some((key, value)) = map.next_entry::<String, toml::Value>()? {
                    entries.insert(key, value);
                }

                // Pattern 2: `[colors] scheme = "tokyonight"`.
                if let Some(scheme) = entries.get("scheme")
                    && let Some(name) = scheme.as_str()
                {
                    return Ok(ColorScheme::Builtin(parse_builtin_scheme(name)));
                }

                // Pattern 3: full custom palette (foreground / background /
                // cursor / ansi).
                let palette: CustomPalette = toml::Value::Table(entries.into_iter().collect())
                    .try_into()
                    .map_err(|e| {
                        de::Error::custom(format!("failed to parse the custom palette: {}", e))
                    })?;
                Ok(ColorScheme::Custom(palette))
            }
        }

        deserializer.deserialize_any(ColorSchemeVisitor)
    }
}

/// Parses a built-in scheme name into a `BuiltinScheme`. Unknown values fall
/// back to `Dark`.
///
/// Sprint 5-4 / D4: the previous implementation could only parse 5 of the
/// 9 schemes, so `catppuccin` / `dracula` / `nord` / `onedark` were silently
/// downgraded to `Dark`. Delegating to `BuiltinScheme::from_toml_name` fixes
/// the issue and covers all 9 entries.
fn parse_builtin_scheme(s: &str) -> BuiltinScheme {
    BuiltinScheme::from_toml_name(s).unwrap_or(BuiltinScheme::Dark)
}

// =============================================================================
// Phase 6 (UI/UX v2): inactive-pane HSB transform.
// =============================================================================

/// WezTerm-style HSB transform for inactive panes. Replaces the v1 flat-black
/// dim overlay with a configurable brightness / saturation knob.
///
/// The renderer keeps the existing spring-animated alpha for the transition
/// (see [`crate::renderer::animations`] in `nexterm-client-gpu`). Each frame
/// the overlay colour and alpha are derived from these values via
/// [`InactivePaneHsbConfig::overlay_rgba`].
///
/// ```toml
/// [inactive_pane_hsb]
/// hue = 1.0
/// saturation = 0.6
/// brightness = 0.85
/// ```
///
/// ## Approximation notes
///
/// A true HSB transform requires a post-process shader pass that converts
/// each pane's pixels from RGB → HSB, scales H/S/B by the configured values,
/// and converts back. Nexterm's current background pipeline emits flat alpha
/// overlays only, so this v1 implementation approximates the effect:
///
/// - `brightness < 1.0` → black overlay of alpha `1.0 - brightness` (drops the
///   pane's perceived luminosity by roughly the same factor).
/// - `saturation < 1.0` → mixes the overlay toward neutral grey (50% grey),
///   which desaturates the underlying colour.
/// - `hue != 1.0` → **ignored** in v1. A real hue shift needs the shader pass
///   mentioned above and lands in Phase 6b.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct InactivePaneHsbConfig {
    /// Hue multiplier. `1.0` = no shift. Currently a no-op (see struct docs).
    #[serde(default = "default_hsb_hue")]
    pub hue: f32,
    /// Saturation multiplier in `[0.0, 1.0]`. `1.0` = full saturation
    /// (no overlay tint), `0.0` = grayscale (overlay tints toward 50% grey).
    #[serde(default = "default_hsb_saturation")]
    pub saturation: f32,
    /// Brightness multiplier in `[0.0, 1.0]`. `1.0` = no dim, `0.0` = black.
    #[serde(default = "default_hsb_brightness")]
    pub brightness: f32,
}

fn default_hsb_hue() -> f32 {
    1.0
}

fn default_hsb_saturation() -> f32 {
    0.6
}

fn default_hsb_brightness() -> f32 {
    0.85
}

impl Default for InactivePaneHsbConfig {
    fn default() -> Self {
        Self {
            hue: default_hsb_hue(),
            saturation: default_hsb_saturation(),
            brightness: default_hsb_brightness(),
        }
    }
}

impl InactivePaneHsbConfig {
    /// Convert the HSB config into the RGBA overlay colour the renderer
    /// should paint over an inactive pane. Pure function for unit-testing.
    ///
    /// `animation_t` is the renderer's spring-animated dim progress in
    /// `[0.0, 1.0]` (0 = fully focused, 1 = fully inactive). Used to fade
    /// the overlay in/out so the existing transition is preserved.
    ///
    /// Returns `[r, g, b, a]` in linear `[0.0, 1.0]` space, suitable for
    /// `add_px_rect` straight away.
    pub fn overlay_rgba(&self, animation_t: f32) -> [f32; 4] {
        let brightness = self.brightness.clamp(0.0, 1.0);
        let saturation = self.saturation.clamp(0.0, 1.0);
        let t = animation_t.clamp(0.0, 1.0);

        // Saturation in [0,1]: 1.0 → overlay is pure black (no desat tint),
        // 0.0 → overlay is 50% grey (maximum desaturation).
        let desat_mix = 1.0 - saturation;
        let grey = 0.5 * desat_mix;

        // Brightness → alpha: brightness=1.0 means no dim (alpha 0), 0.0
        // means full black (alpha 1.0). Scaled by `t` so the spring still
        // controls the fade-in.
        let alpha = (1.0 - brightness) * t;

        [grey, grey, grey, alpha]
    }

    /// Whether the transform produces any visible effect at all. When
    /// `false`, the renderer can skip the overlay drawcall entirely.
    pub fn is_active(&self) -> bool {
        self.brightness < 0.999 || self.saturation < 0.999
    }
}

#[cfg(test)]
mod inactive_pane_hsb_tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn defaults_match_plan_spec() {
        let c = InactivePaneHsbConfig::default();
        assert!(approx(c.hue, 1.0));
        assert!(approx(c.saturation, 0.6));
        assert!(approx(c.brightness, 0.85));
    }

    #[test]
    fn default_overlay_is_partial_grey_at_full_t() {
        let c = InactivePaneHsbConfig::default();
        let [r, g, b, a] = c.overlay_rgba(1.0);
        // saturation=0.6 → desat_mix=0.4 → grey=0.2.
        assert!(approx(r, 0.2));
        assert!(approx(g, 0.2));
        assert!(approx(b, 0.2));
        // brightness=0.85 → alpha 0.15 at t=1.
        assert!(approx(a, 0.15));
    }

    #[test]
    fn animation_t_zero_produces_invisible_overlay() {
        let c = InactivePaneHsbConfig::default();
        let [_, _, _, a] = c.overlay_rgba(0.0);
        assert!(approx(a, 0.0));
    }

    #[test]
    fn brightness_one_disables_dim_at_any_t() {
        let c = InactivePaneHsbConfig {
            hue: 1.0,
            saturation: 0.5,
            brightness: 1.0,
        };
        for t in [0.0_f32, 0.25, 0.5, 1.0] {
            let [_, _, _, a] = c.overlay_rgba(t);
            assert!(
                approx(a, 0.0),
                "brightness=1.0 must yield alpha 0 at t={}",
                t
            );
        }
    }

    #[test]
    fn saturation_one_produces_pure_black_overlay() {
        let c = InactivePaneHsbConfig {
            hue: 1.0,
            saturation: 1.0,
            brightness: 0.5,
        };
        let [r, g, b, a] = c.overlay_rgba(1.0);
        assert!(approx(r, 0.0));
        assert!(approx(g, 0.0));
        assert!(approx(b, 0.0));
        assert!(approx(a, 0.5));
    }

    #[test]
    fn out_of_range_inputs_are_clamped() {
        let c = InactivePaneHsbConfig {
            hue: 0.0,
            saturation: -1.0,
            brightness: 2.0,
        };
        let [r, g, b, a] = c.overlay_rgba(5.0);
        // saturation clamps to 0 → grey 0.5. brightness clamps to 1 → alpha 0.
        // t clamps to 1.
        assert!(approx(r, 0.5));
        assert!(approx(g, 0.5));
        assert!(approx(b, 0.5));
        assert!(approx(a, 0.0));
    }

    #[test]
    fn is_active_only_when_visibly_different() {
        let neutral = InactivePaneHsbConfig {
            hue: 1.0,
            saturation: 1.0,
            brightness: 1.0,
        };
        assert!(!neutral.is_active());
        assert!(InactivePaneHsbConfig::default().is_active());
    }

    #[test]
    fn round_trips_through_toml() {
        let toml_str = r#"
[inactive_pane_hsb]
hue = 1.0
saturation = 0.5
brightness = 0.75
"#;
        let parsed: crate::schema::Config = toml::from_str(toml_str).unwrap();
        let c = parsed.inactive_pane_hsb;
        assert!(approx(c.hue, 1.0));
        assert!(approx(c.saturation, 0.5));
        assert!(approx(c.brightness, 0.75));
    }
}
