//! Design tokens derived algorithmically from the active color-scheme palette.
//!
//! Every UI chrome color (tab bar, borders, overlays, status bar) is computed
//! from the terminal's own `SchemePalette` instead of hard-coded Tokyo Night
//! RGBA values.  This makes Nexterm look correct with any built-in or custom
//! color scheme.
//!
//! # How tokens are derived
//!
//! 1. **Luminance**: ITU-R BT.709 luminance of `bg` determines dark vs light.
//!    A background with luminance < 0.35 is treated as dark.
//! 2. **Surfaces**: The background is lightened (dark) or darkened (light) in
//!    four steps to produce layered chrome surfaces.
//! 3. **Accent**: `ansi[12]` (bright blue) is preferred; `ansi[4]` (blue) is
//!    the fallback when bright blue is too dim.
//! 4. **Text**: Derived from `fg` at three opacity levels.
//!
//! All colors are stored as `[f32; 4]` RGBA in linear sRGB space (0.0–1.0)
//! ready to be passed directly to wgpu vertex builders.

use super::SchemePalette;

/// A full set of design tokens derived from a `SchemePalette`.
///
/// Obtain via [`DesignTokens::from_palette`].
#[derive(Debug, Clone)]
pub struct DesignTokens {
    // ── Surfaces ─────────────────────────────────────────────────────────────
    /// Terminal background (pass-through from palette).
    pub surface_0: [f32; 4],
    /// Tab bar / status bar background (slight lift from surface_0).
    pub surface_1: [f32; 4],
    /// Overlay / active-tab background (moderate lift).
    pub surface_2: [f32; 4],
    /// Hover / selected-item background (strong lift).
    pub surface_3: [f32; 4],

    // ── Borders ──────────────────────────────────────────────────────────────
    /// Pane dividers and subtle separators (fully opaque).
    pub border_subtle: [f32; 4],
    /// Overlay / dialog borders (slightly more visible).
    pub border_default: [f32; 4],
    /// Focused-pane border (= accent_primary, fully opaque).
    pub border_focus: [f32; 4],

    // ── Accent ───────────────────────────────────────────────────────────────
    /// Primary accent color (derived from ANSI bright-blue / blue).
    pub accent_primary: [f32; 4],
    /// Accent at ~0.22 alpha – used for focus halos.
    pub accent_muted: [f32; 4],
    /// Activity-indicator tab background (darkened warm hue from ANSI yellow).
    pub accent_activity: [f32; 4],

    // ── Text ─────────────────────────────────────────────────────────────────
    /// Full-brightness text (fg at 1.00 alpha).
    pub text_primary: [f32; 4],
    /// Secondary text (fg at 0.78 alpha).
    pub text_secondary: [f32; 4],
    /// Muted / placeholder text (fg at 0.48 alpha).
    pub text_muted: [f32; 4],
    /// Text on top of a solid accent surface (black or white for readability).
    pub text_on_accent: [f32; 4],

    // ── Semantic ─────────────────────────────────────────────────────────────
    /// Success / green (ANSI 2 or 10).
    pub semantic_success: [f32; 4],
    /// Warning / yellow (ANSI 3 or 11).
    pub semantic_warning: [f32; 4],
    /// Error / red (ANSI 1 or 9).
    pub semantic_error: [f32; 4],
    /// Info (= accent_primary).
    pub semantic_info: [f32; 4],

    // ── Tab-bar shorthands ────────────────────────────────────────────────────
    /// Active-tab background (= surface_2).
    pub tab_active_bg: [f32; 4],
    /// Inactive-tab background (= surface_1).
    pub tab_inactive_bg: [f32; 4],
    /// Activity-tab background (= accent_activity).
    pub tab_activity_bg: [f32; 4],
}

// ─────────────────────────────────────────────────────────────────────────────
// Derivation helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Convert an 8-bit sRGB channel to a linear `f32`.
#[inline]
fn u8_to_f32(v: u8) -> f32 {
    v as f32 / 255.0
}

/// ITU-R BT.709 relative luminance (inputs are linear 0–1).
#[inline]
fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Lighten `v` toward 1.0 by `amount`.
#[inline]
fn lighten(v: f32, amount: f32) -> f32 {
    (v + amount).min(1.0)
}

/// Darken `v` toward 0.0 by `amount`.
#[inline]
fn darken(v: f32, amount: f32) -> f32 {
    (v - amount).max(0.0)
}

/// Shift a color channel toward white (dark scheme) or toward black (light).
#[inline]
fn shift(v: f32, amount: f32, is_dark: bool) -> f32 {
    if is_dark {
        lighten(v, amount)
    } else {
        darken(v, amount)
    }
}

/// Build an opaque `[f32; 4]` from three `f32` channels.
#[inline]
fn rgba(r: f32, g: f32, b: f32, a: f32) -> [f32; 4] {
    [r, g, b, a]
}

/// Parse a `#RRGGBB` hex string into `[f32; 4]`.
/// Returns `None` on any parse error.
pub fn parse_hex_color(hex: &str) -> Option<[f32; 4]> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some([u8_to_f32(r), u8_to_f32(g), u8_to_f32(b), 1.0])
}

/// Resolve a user-supplied `Option<&str>` hex override against a token fallback.
///
/// * `Some(hex)` – parse and use the explicit color; fall through to `fallback`
///   on parse error.
/// * `None` – use `fallback` directly.
pub fn resolve(user: Option<&str>, fallback: [f32; 4]) -> [f32; 4] {
    match user {
        Some(hex) => parse_hex_color(hex).unwrap_or(fallback),
        None => fallback,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Main derivation
// ─────────────────────────────────────────────────────────────────────────────

impl DesignTokens {
    /// Derive the full token set from `palette`.
    ///
    /// Pass the `SchemePalette` returned by `BuiltinScheme::palette()` or
    /// constructed for a `CustomPalette`.
    pub fn from_palette(palette: &SchemePalette) -> Self {
        // ── Base colors as f32 ────────────────────────────────────────────────
        let [br, bg_g, bb] = palette.bg.map(u8_to_f32);
        let [fr, fg_g, fb] = palette.fg.map(u8_to_f32);

        let is_dark = luminance(br, bg_g, bb) < 0.35;

        // Surface steps: 0.045 / 0.10 / 0.16 / 0.22
        let s1 = 0.045_f32;
        let s2 = 0.10_f32;
        let s3 = 0.16_f32;
        let s4 = 0.22_f32;

        let surface_0 = rgba(br, bg_g, bb, 1.0);
        let surface_1 = rgba(
            shift(br, s1, is_dark),
            shift(bg_g, s1, is_dark),
            shift(bb, s1, is_dark),
            1.0,
        );
        let surface_2 = rgba(
            shift(br, s2, is_dark),
            shift(bg_g, s2, is_dark),
            shift(bb, s2, is_dark),
            1.0,
        );
        let surface_3 = rgba(
            shift(br, s3, is_dark),
            shift(bg_g, s3, is_dark),
            shift(bb, s3, is_dark),
            1.0,
        );

        // ── Borders ───────────────────────────────────────────────────────────
        let border_subtle = rgba(
            shift(br, s3, is_dark),
            shift(bg_g, s3, is_dark),
            shift(bb, s3, is_dark),
            1.0,
        );
        let border_default = rgba(
            shift(br, s4, is_dark),
            shift(bg_g, s4, is_dark),
            shift(bb, s4, is_dark),
            1.0,
        );

        // ── Accent: prefer ANSI bright-blue (index 12), fallback to blue (4) ──
        let [ab12r, ab12g, ab12b] = palette.ansi[12].map(u8_to_f32);
        let [ab4r, ab4g, ab4b] = palette.ansi[4].map(u8_to_f32);

        // Use bright blue if it's reasonably luminous, otherwise fall back.
        let (ar, ag_c, ab_c) = if luminance(ab12r, ab12g, ab12b) > 0.05 {
            (ab12r, ab12g, ab12b)
        } else {
            (ab4r, ab4g, ab4b)
        };

        let accent_primary = rgba(ar, ag_c, ab_c, 1.0);
        let accent_muted = rgba(ar, ag_c, ab_c, 0.22);
        let border_focus = accent_primary;

        // Activity: darkened warm yellow from ANSI 3 (dark yellow / olive).
        let [ay3r, ay3g, ay3b] = palette.ansi[3].map(u8_to_f32);
        let act_shift = if is_dark { 0.08 } else { 0.12 };
        let accent_activity = rgba(
            darken(ay3r, act_shift),
            darken(ay3g, act_shift),
            darken(ay3b, act_shift),
            1.0,
        );

        // ── Text ─────────────────────────────────────────────────────────────
        let text_primary = rgba(fr, fg_g, fb, 1.00);
        let text_secondary = rgba(fr, fg_g, fb, 0.78);
        let text_muted = rgba(fr, fg_g, fb, 0.48);

        // Text on accent: choose black or white based on accent luminance.
        let text_on_accent = if luminance(ar, ag_c, ab_c) > 0.35 {
            rgba(0.05, 0.05, 0.05, 1.0) // dark text on light accent
        } else {
            rgba(0.97, 0.97, 0.97, 1.0) // light text on dark accent
        };

        // ── Semantic ─────────────────────────────────────────────────────────
        let semantic_success = {
            let [r, g, b] = palette.ansi[10].map(u8_to_f32); // bright green
            rgba(r, g, b, 1.0)
        };
        let semantic_warning = {
            let [r, g, b] = palette.ansi[11].map(u8_to_f32); // bright yellow
            rgba(r, g, b, 1.0)
        };
        let semantic_error = {
            let [r, g, b] = palette.ansi[9].map(u8_to_f32); // bright red
            rgba(r, g, b, 1.0)
        };
        let semantic_info = accent_primary;

        Self {
            surface_0,
            surface_1,
            surface_2,
            surface_3,
            border_subtle,
            border_default,
            border_focus,
            accent_primary,
            accent_muted,
            accent_activity,
            text_primary,
            text_secondary,
            text_muted,
            text_on_accent,
            semantic_success,
            semantic_warning,
            semantic_error,
            semantic_info,
            tab_active_bg: surface_2,
            tab_inactive_bg: surface_1,
            tab_activity_bg: accent_activity,
        }
    }
}

impl Default for DesignTokens {
    fn default() -> Self {
        // Tokyo Night palette as a sensible fallback when no scheme is active.
        let palette = SchemePalette {
            fg: [0xC0, 0xCA, 0xF5],
            bg: [0x1A, 0x1B, 0x2E],
            ansi: [
                [0x15, 0x16, 0x2E],
                [0xF7, 0x76, 0x8E],
                [0x9E, 0xCE, 0x6A],
                [0xE0, 0xAF, 0x68],
                [0x7A, 0xA2, 0xF7],
                [0xBB, 0x9A, 0xF7],
                [0x7D, 0xCF, 0xFF],
                [0xA9, 0xB1, 0xD6],
                [0x41, 0x4B, 0x67],
                [0xFF, 0x89, 0x9D],
                [0xB9, 0xF2, 0x7C],
                [0xFF, 0xD5, 0x73],
                [0x73, 0xDA, 0xCA],
                [0xC0, 0xB0, 0xF8],
                [0xB4, 0xF9, 0xF8],
                [0xD5, 0xD6, 0xDB],
            ],
        };
        Self::from_palette(&palette)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tokyo_night_palette() -> SchemePalette {
        SchemePalette {
            fg: [0xC0, 0xCA, 0xF5],
            bg: [0x1A, 0x1B, 0x2E],
            ansi: [
                [0x15, 0x16, 0x2E],
                [0xF7, 0x76, 0x8E],
                [0x9E, 0xCE, 0x6A],
                [0xE0, 0xAF, 0x68],
                [0x7A, 0xA2, 0xF7],
                [0xBB, 0x9A, 0xF7],
                [0x7D, 0xCF, 0xFF],
                [0xA9, 0xB1, 0xD6],
                [0x41, 0x4B, 0x67],
                [0xFF, 0x89, 0x9D],
                [0xB9, 0xF2, 0x7C],
                [0xFF, 0xD5, 0x73],
                [0x73, 0xDA, 0xCA],
                [0xC0, 0xB0, 0xF8],
                [0xB4, 0xF9, 0xF8],
                [0xD5, 0xD6, 0xDB],
            ],
        }
    }

    fn gruvbox_light_palette() -> SchemePalette {
        // Gruvbox Light – bg is a warm ivory, not dark.
        SchemePalette {
            fg: [0x3C, 0x38, 0x36],
            bg: [0xFB, 0xF1, 0xC7],
            ansi: [
                [0xFB, 0xF1, 0xC7],
                [0xCC, 0x24, 0x1D],
                [0x98, 0x97, 0x1A],
                [0xD7, 0x99, 0x21],
                [0x45, 0x85, 0x88],
                [0xB1, 0x62, 0x86],
                [0x68, 0x9D, 0x6A],
                [0x7C, 0x6F, 0x64],
                [0x92, 0x83, 0x74],
                [0x9D, 0x00, 0x06],
                [0x79, 0x74, 0x0E],
                [0xB5, 0x76, 0x14],
                [0x07, 0x66, 0x78],
                [0x8F, 0x3F, 0x71],
                [0x42, 0x7B, 0x58],
                [0x3C, 0x38, 0x36],
            ],
        }
    }

    #[test]
    fn dark_palette_is_detected_as_dark() {
        let p = tokyo_night_palette();
        let [r, g, b] = p.bg.map(u8_to_f32);
        assert!(luminance(r, g, b) < 0.35, "Tokyo Night bg should be dark");
    }

    #[test]
    fn light_palette_is_detected_as_light() {
        let p = gruvbox_light_palette();
        let [r, g, b] = p.bg.map(u8_to_f32);
        assert!(
            luminance(r, g, b) > 0.35,
            "Gruvbox Light bg should be light"
        );
    }

    #[test]
    fn dark_scheme_surfaces_lighten() {
        let tokens = DesignTokens::from_palette(&tokyo_night_palette());
        // Each surface level must be brighter than the one below.
        let lum = |c: [f32; 4]| luminance(c[0], c[1], c[2]);
        assert!(lum(tokens.surface_1) > lum(tokens.surface_0));
        assert!(lum(tokens.surface_2) > lum(tokens.surface_1));
        assert!(lum(tokens.surface_3) > lum(tokens.surface_2));
    }

    #[test]
    fn light_scheme_surfaces_darken() {
        let tokens = DesignTokens::from_palette(&gruvbox_light_palette());
        let lum = |c: [f32; 4]| luminance(c[0], c[1], c[2]);
        assert!(lum(tokens.surface_1) < lum(tokens.surface_0));
        assert!(lum(tokens.surface_2) < lum(tokens.surface_1));
        assert!(lum(tokens.surface_3) < lum(tokens.surface_2));
    }

    #[test]
    fn tab_shorthands_match_surfaces() {
        let tokens = DesignTokens::from_palette(&tokyo_night_palette());
        assert_eq!(tokens.tab_active_bg, tokens.surface_2);
        assert_eq!(tokens.tab_inactive_bg, tokens.surface_1);
        assert_eq!(tokens.tab_activity_bg, tokens.accent_activity);
    }

    #[test]
    fn border_focus_equals_accent_primary() {
        let tokens = DesignTokens::from_palette(&tokyo_night_palette());
        assert_eq!(tokens.border_focus, tokens.accent_primary);
    }

    #[test]
    fn parse_hex_color_valid() {
        let c = parse_hex_color("#7AA2F7").unwrap();
        assert!((c[0] - 0.478).abs() < 0.002);
        assert!((c[1] - 0.635).abs() < 0.002);
        assert!((c[2] - 0.969).abs() < 0.002);
        assert_eq!(c[3], 1.0);
    }

    #[test]
    fn parse_hex_color_without_hash() {
        assert!(parse_hex_color("7AA2F7").is_some());
    }

    #[test]
    fn parse_hex_color_invalid_returns_none() {
        assert!(parse_hex_color("ZZZZZZ").is_none());
        assert!(parse_hex_color("short").is_none());
        assert!(parse_hex_color("").is_none());
    }

    #[test]
    fn resolve_none_returns_fallback() {
        let fallback = [1.0, 0.0, 0.0, 1.0];
        assert_eq!(resolve(None, fallback), fallback);
    }

    #[test]
    fn resolve_some_valid_overrides_fallback() {
        let fallback = [1.0, 0.0, 0.0, 1.0];
        let result = resolve(Some("#000000"), fallback);
        assert_eq!(result, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn resolve_some_invalid_falls_through_to_fallback() {
        let fallback = [1.0, 0.0, 0.0, 1.0];
        assert_eq!(resolve(Some("not-a-color"), fallback), fallback);
    }

    #[test]
    fn default_tokens_are_stable() {
        // Smoke test: default() must not panic and produce non-zero surfaces.
        let t = DesignTokens::default();
        assert!(t.surface_0[0] > 0.0 || t.surface_0[1] > 0.0 || t.surface_0[2] > 0.0);
    }
}
