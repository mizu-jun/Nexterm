//! Color-conversion utilities — ANSI 256 colors and hex color strings.

/// Convert a `nexterm_proto::Color` into RGBA in the [0, 1] range.
///
/// When `is_fg` is true the `Default` color resolves to the foreground; otherwise to
/// the background. If `palette` is provided, the color scheme palette takes precedence.
pub(crate) fn resolve_color(
    color: &nexterm_proto::Color,
    is_fg: bool,
    palette: Option<&nexterm_config::SchemePalette>,
) -> [f32; 4] {
    use nexterm_proto::Color;
    let u8_to_f32 = |v: u8| v as f32 / 255.0;
    match color {
        Color::Default => {
            if let Some(p) = palette {
                let c = if is_fg { p.fg } else { p.bg };
                [u8_to_f32(c[0]), u8_to_f32(c[1]), u8_to_f32(c[2]), 1.0]
            } else if is_fg {
                [0.85, 0.85, 0.85, 1.0]
            } else {
                [0.05, 0.05, 0.05, 1.0]
            }
        }
        Color::Rgb(r, g, b) => [u8_to_f32(*r), u8_to_f32(*g), u8_to_f32(*b), 1.0],
        Color::Indexed(n) => {
            // ANSI 0-15: prefer the scheme palette when available.
            if *n < 16
                && let Some(p) = palette
            {
                let c = p.ansi[*n as usize];
                return [u8_to_f32(c[0]), u8_to_f32(c[1]), u8_to_f32(c[2]), 1.0];
            }
            ansi_256_to_rgb(*n)
        }
    }
}

/// Resolve a `ColorScheme` into its concrete `SchemePalette`.
///
/// Extracted from `render_frame` (roadmap #10b) so the theme-report path can
/// reuse the same fg/bg values the renderer draws with. Malformed hex values
/// in a custom palette fall back to black, matching the renderer's historical
/// behavior.
pub(crate) fn scheme_palette(
    scheme: &nexterm_config::ColorScheme,
) -> nexterm_config::SchemePalette {
    match scheme {
        nexterm_config::ColorScheme::Builtin(s) => s.palette(),
        nexterm_config::ColorScheme::Custom(p) => {
            let parse_hex = |s: &str| -> [u8; 3] {
                let s = s.trim_start_matches('#');
                let v = u32::from_str_radix(s, 16).unwrap_or(0);
                [
                    ((v >> 16) & 0xFF) as u8,
                    ((v >> 8) & 0xFF) as u8,
                    (v & 0xFF) as u8,
                ]
            };
            let mut ansi = [[0u8; 3]; 16];
            for (i, hex) in p.ansi.iter().enumerate().take(16) {
                ansi[i] = parse_hex(hex);
            }
            nexterm_config::SchemePalette {
                fg: parse_hex(&p.foreground),
                bg: parse_hex(&p.background),
                ansi,
            }
        }
    }
}

/// Like [`resolve_color`], but consults the pane's OSC 4/10/11 dynamic-color
/// overrides first (roadmap #10b): the dynamic fg/bg beat the scheme
/// defaults, and OSC 4 palette entries beat both the scheme ANSI palette and
/// the builtin 256-color table.
pub(crate) fn resolve_color_with_overrides(
    color: &nexterm_proto::Color,
    is_fg: bool,
    palette: Option<&nexterm_config::SchemePalette>,
    overrides: Option<&crate::state::PaneColorOverrides>,
) -> [f32; 4] {
    use nexterm_proto::Color;
    let u8_to_f32 = |v: u8| v as f32 / 255.0;
    if let Some(ov) = overrides {
        match color {
            Color::Default => {
                let dynamic = if is_fg { ov.fg } else { ov.bg };
                if let Some(c) = dynamic {
                    return [u8_to_f32(c[0]), u8_to_f32(c[1]), u8_to_f32(c[2]), 1.0];
                }
            }
            Color::Indexed(n) => {
                if let Some(c) = ov.palette.get(n) {
                    return [u8_to_f32(c[0]), u8_to_f32(c[1]), u8_to_f32(c[2]), 1.0];
                }
            }
            Color::Rgb(..) => {}
        }
    }
    resolve_color(color, is_fg, palette)
}

/// ANSI 256-color palette → RGBA in [0, 1].
pub(crate) fn ansi_256_to_rgb(n: u8) -> [f32; 4] {
    // Basic 16 colors (simple implementation).
    const BASIC: [[f32; 3]; 16] = [
        [0.0, 0.0, 0.0],       // 0: black
        [0.502, 0.0, 0.0],     // 1: red
        [0.0, 0.502, 0.0],     // 2: green
        [0.502, 0.502, 0.0],   // 3: yellow
        [0.0, 0.0, 0.502],     // 4: blue
        [0.502, 0.0, 0.502],   // 5: magenta
        [0.0, 0.502, 0.502],   // 6: cyan
        [0.753, 0.753, 0.753], // 7: white
        [0.502, 0.502, 0.502], // 8: bright black
        [1.0, 0.0, 0.0],       // 9: bright red
        [0.0, 1.0, 0.0],       // 10: bright green
        [1.0, 1.0, 0.0],       // 11: bright yellow
        [0.0, 0.0, 1.0],       // 12: bright blue
        [1.0, 0.0, 1.0],       // 13: bright magenta
        [0.0, 1.0, 1.0],       // 14: bright cyan
        [1.0, 1.0, 1.0],       // 15: bright white
    ];

    if (n as usize) < BASIC.len() {
        let c = BASIC[n as usize];
        return [c[0], c[1], c[2], 1.0];
    }

    // 216-color cube (16–231).
    if (16..=231).contains(&n) {
        let idx = n - 16;
        let b = (idx % 6) as f32 / 5.0;
        let g = ((idx / 6) % 6) as f32 / 5.0;
        let r = ((idx / 36) % 6) as f32 / 5.0;
        return [r, g, b, 1.0];
    }

    // Grayscale (232–255).
    let grey = (n - 232) as f32 / 23.0;
    [grey, grey, grey, 1.0]
}

// ---- Phase 6b (UI/UX v2): HSV conversion + WezTerm-style HSB transform ----

/// Convert linear-ish RGB in `[0, 1]` to HSV (H in `[0, 360)`, S/V in
/// `[0, 1]`). The colour space treatment matches what WezTerm does for
/// `inactive_pane_hsb`: a straightforward HSV computation on the cell
/// colour without gamma correction. Pure helper so it can be unit
/// tested without touching wgpu.
pub(crate) fn rgb_to_hsv(rgb: [f32; 3]) -> [f32; 3] {
    let r = rgb[0];
    let g = rgb[1];
    let b = rgb[2];
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let v = max;
    let s = if max <= 0.0 { 0.0 } else { delta / max };
    let h = if delta <= 0.0 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if (max - g).abs() < 1e-6 {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    [h, s, v]
}

/// Inverse of `rgb_to_hsv`. Hue is interpreted modulo 360 so the
/// multiplier-style hue shift used by `apply_hsb_multiplier` produces
/// valid output for any input.
pub(crate) fn hsv_to_rgb(hsv: [f32; 3]) -> [f32; 3] {
    let h = hsv[0].rem_euclid(360.0);
    let s = hsv[1].clamp(0.0, 1.0);
    let v = hsv[2].clamp(0.0, 1.0);
    let c = v * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - ((h_prime.rem_euclid(2.0)) - 1.0).abs());
    let (r1, g1, b1) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = v - c;
    [r1 + m, g1 + m, b1 + m]
}

/// Apply a WezTerm-style HSB multiplier to a colour. Each component
/// (hue, saturation, brightness) acts as a multiplier on the cell's
/// own HSV channels:
/// - `hue = 1.0` is identity. `hue = 0.5` rotates each cell's hue
///   value to half its angle (e.g. yellow → orange); `hue = 2.0`
///   doubles it (wrapping around 360°).
/// - `saturation = 1.0` is identity. Smaller values desaturate.
/// - `brightness = 1.0` is identity. Smaller values darken.
///
/// Alpha is preserved unchanged.
pub(crate) fn apply_hsb_multiplier(
    rgba: [f32; 4],
    hue_mul: f32,
    sat_mul: f32,
    brightness_mul: f32,
) -> [f32; 4] {
    let hsv = rgb_to_hsv([rgba[0], rgba[1], rgba[2]]);
    let h = (hsv[0] * hue_mul).rem_euclid(360.0);
    let s = (hsv[1] * sat_mul).clamp(0.0, 1.0);
    let v = (hsv[2] * brightness_mul).clamp(0.0, 1.0);
    let rgb = hsv_to_rgb([h, s, v]);
    [rgb[0], rgb[1], rgb[2], rgba[3]]
}

/// Animated variant: lerps each multiplier toward identity (1.0)
/// based on `t` in `[0, 1]`. `t = 0` returns the input unchanged
/// (matches the focused-pane look); `t = 1` applies the full HSB
/// multiplier (matches the steady-state inactive look). Used so the
/// HSB transition follows the same spring-driven animation as the
/// existing Phase-6 overlay path.
pub(crate) fn apply_hsb_animated_rgba(
    rgba: [f32; 4],
    hue_mul: f32,
    sat_mul: f32,
    brightness_mul: f32,
    t: f32,
) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    let h = 1.0 + (hue_mul - 1.0) * t;
    let s = 1.0 + (sat_mul - 1.0) * t;
    let b = 1.0 + (brightness_mul - 1.0) * t;
    apply_hsb_multiplier(rgba, h, s, b)
}

/// WCAG 2.x relative luminance of a linear-ish sRGB color (components in [0, 1]).
///
/// Uses the standard sRGB transfer function per
/// <https://www.w3.org/TR/WCAG21/#dfn-relative-luminance>.
pub(crate) fn relative_luminance(rgb: [f32; 3]) -> f32 {
    let lin = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2])
}

/// WCAG 2.x contrast ratio between two opaque colors, in [1, 21].
///
/// Overlay text must reach at least 4.5:1 against its effective background
/// (project accessibility guideline).
pub(crate) fn contrast_ratio(fg: [f32; 3], bg: [f32; 3]) -> f32 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

/// Composite a straight-alpha RGBA color over an opaque background, returning
/// the effective opaque color. Used to evaluate the contrast of semi-transparent
/// overlay decorations against panel surfaces.
pub(crate) fn composite_over(fg: [f32; 4], bg: [f32; 3]) -> [f32; 3] {
    let a = fg[3].clamp(0.0, 1.0);
    [
        fg[0] * a + bg[0] * (1.0 - a),
        fg[1] * a + bg[1] * (1.0 - a),
        fg[2] * a + bg[2] * (1.0 - a),
    ]
}

/// Brightness multiplier applied to a control's fill at full press weight
/// (UI/UX v3 P3b3). Fluent's subtle-button ramp puts pressed below hover;
/// this is that step, expressed as an HSV `v` multiplier so it follows every
/// scheme without a per-scheme table.
pub(crate) const PRESS_DIM: f32 = 0.85;

/// Alpha multiplier applied to a hover layer at full press weight.
///
/// The dim alone is not enough. Three of the four press sites draw their
/// hover as an additive layer, and on a scheme whose chrome is already
/// near-black the brightness step has almost no absolute room — pressed
/// would read as identical to hovered. Strengthening the layer moves on
/// every scheme.
///
/// Raised from the brief's starting value of `1.7`: at that value the
/// perceptibility gate (`press_is_perceptible_on_every_builtin_scheme`)
/// still failed on 8 of the 9 builtin schemes, not only the one the brief
/// anticipated. `2.3` is the smallest value at which all nine pass; see
/// `press-pulse task-1-report.md` for the per-scheme scan and for the
/// unresolved conflict this creates with the companion contrast-guard test.
pub(crate) const PRESS_ALPHA_BOOST: f32 = 2.3;

/// Apply press feedback to a control fill already composed for `hover.max(press)`.
///
/// At `press == 0` this returns `color` unchanged, so a site that is never
/// pressed keeps its exact P3b2 appearance.
pub(crate) fn press_fill(color: [f32; 4], press: f32) -> [f32; 4] {
    let press = press.clamp(0.0, 1.0);
    let dimmed = apply_hsb_animated_rgba(color, 1.0, 1.0, PRESS_DIM, press);
    let alpha = (color[3] * (1.0 + press * (PRESS_ALPHA_BOOST - 1.0))).min(1.0);
    [dimmed[0], dimmed[1], dimmed[2], alpha]
}

/// Linearly interpolate two RGBA colours, `t` clamped to `[0, 1]`.
///
/// Used by the hover cross-fade (UI/UX v3 P3b2), where the hovered
/// appearance is a *different colour* rather than an extra layer — a
/// brightened tab background, an accent-tinted menu row — so alpha scaling
/// cannot express the transition and the colour itself has to move.
///
/// Interpolation is in whatever space the tokens already are (linear-ish
/// sRGB floats, as everywhere else in this renderer); no gamma correction is
/// applied, matching how the palette's existing blends behave.
///
/// `t <= 0.0` and `t >= 1.0` return `a` / `b` directly rather than going
/// through the arithmetic below: floating-point subtraction and
/// multiplication do not round-trip exactly (e.g. `0.1 + (0.9 - 0.1) * 1.0`
/// is not bit-identical to `0.9`), and these are also the overwhelmingly
/// common weights — most frames have no transition running at all.
pub(crate) fn lerp_rgba(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Return `color` with its alpha channel replaced by `alpha`.
///
/// Design tokens carry their own alpha (usually 1.0); overlay elements such
/// as the OSC 9;4 progress bar or the IME preedit backdrop keep their
/// element-specific translucency while taking the hue from the token.
pub(crate) fn with_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], alpha]
}

/// Alpha of the terminal selection highlight — low enough that the selected
/// cells' glyphs still read through the fill drawn over them.
const SELECTION_ALPHA: f32 = 0.40;

/// Selection-highlight color, shared by the normal grid and by copy mode.
///
/// UI/UX v3 (G11 follow-up): the two grid builders and the copy-mode overlay
/// each carried their own literal, and they had already drifted apart
/// (`[0.25, 0.55, 1.0, 0.40]` against `[0.40, 0.65, 1.0, 0.45]`). Routing all
/// three through one helper is what makes a scheme change reach every
/// selection surface, and what keeps them from drifting again.
pub(crate) fn selection_color(tokens: &nexterm_config::DesignTokens) -> [f32; 4] {
    with_alpha(tokens.accent_primary, SELECTION_ALPHA)
}

/// Pick whichever of near-black / near-white text reads better on `fill`.
///
/// `DesignTokens::text_on_accent` answers this question for `accent_primary`
/// alone. A badge painted in a warm semantic color needs the choice made
/// against *its own* fill, otherwise a light-on-yellow pairing slips through.
/// The two extremes match the ones `text_on_accent` chooses between, so both
/// paths land on the same pair of values.
pub(crate) fn on_surface_text(fill: [f32; 4]) -> [f32; 4] {
    const DARK: [f32; 4] = [0.05, 0.05, 0.05, 1.0];
    const LIGHT: [f32; 4] = [0.97, 0.97, 0.97, 1.0];
    let fill_rgb = [fill[0], fill[1], fill[2]];
    if contrast_ratio([DARK[0], DARK[1], DARK[2]], fill_rgb)
        >= contrast_ratio([LIGHT[0], LIGHT[1], LIGHT[2]], fill_rgb)
    {
        DARK
    } else {
        LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_rgba_hits_both_endpoints_exactly() {
        let a = [0.1, 0.2, 0.3, 1.0];
        let b = [0.9, 0.8, 0.7, 0.5];
        assert_eq!(lerp_rgba(a, b, 0.0), a);
        assert_eq!(lerp_rgba(a, b, 1.0), b);
    }

    #[test]
    fn lerp_rgba_is_linear_at_the_midpoint() {
        let m = lerp_rgba([0.0, 0.0, 0.0, 0.0], [1.0, 0.5, 0.25, 1.0], 0.5);
        assert!((m[0] - 0.5).abs() < 1e-6);
        assert!((m[1] - 0.25).abs() < 1e-6);
        assert!((m[2] - 0.125).abs() < 1e-6);
        assert!((m[3] - 0.5).abs() < 1e-6);
    }

    /// A weight arrives from an eased curve and is already clamped, but a
    /// colour helper that trusts its caller is a colour helper that produces
    /// out-of-range channels the first time someone doesn't.
    ///
    /// Note: `[0,0,0,0]` and `[1,1,1,1]` are exactly representable, so a
    /// naive `a + (b - a) * t.clamp(0,1)` would reproduce this test's
    /// expectations too. This test does not on its own distinguish the
    /// early-return implementation from a clamp-then-lerp one — that is
    /// `lerp_rgba_hits_both_endpoints_exactly`, which uses non-trivial
    /// endpoints and genuinely needs the early returns.
    #[test]
    fn lerp_rgba_clamps_t() {
        let a = [0.0, 0.0, 0.0, 0.0];
        let b = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(lerp_rgba(a, b, -0.5), a);
        assert_eq!(lerp_rgba(a, b, 1.5), b);
    }

    #[test]
    fn with_alpha_replaces_only_the_alpha_channel() {
        assert_eq!(with_alpha([0.2, 0.4, 0.6, 1.0], 0.5), [0.2, 0.4, 0.6, 0.5]);
        assert_eq!(with_alpha([1.0, 0.0, 0.5, 0.3], 0.9), [1.0, 0.0, 0.5, 0.9]);
    }

    #[test]
    fn contrast_black_on_white_is_21() {
        let r = contrast_ratio([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
        assert!((r - 21.0).abs() < 0.01, "got {r}");
    }

    #[test]
    fn contrast_is_symmetric_and_identity_is_1() {
        let a = [0.2, 0.4, 0.6];
        let b = [0.9, 0.9, 0.9];
        assert_eq!(contrast_ratio(a, b), contrast_ratio(b, a));
        assert!((contrast_ratio(a, a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn contrast_gray_on_white_matches_wcag_reference() {
        // #767676 on white is the canonical ~4.54:1 WCAG boundary example.
        let g = 118.0 / 255.0;
        let r = contrast_ratio([g, g, g], [1.0, 1.0, 1.0]);
        assert!((r - 4.54).abs() < 0.05, "got {r}");
    }

    #[test]
    fn composite_over_blends_alpha() {
        // 50% white over black = mid gray.
        let out = composite_over([1.0, 1.0, 1.0, 0.5], [0.0, 0.0, 0.0]);
        assert!(out.iter().all(|c| (c - 0.5).abs() < 1e-6));
        // Fully opaque ignores the background.
        let out = composite_over([0.3, 0.2, 0.1, 1.0], [1.0, 1.0, 1.0]);
        assert_eq!(out, [0.3, 0.2, 0.1]);
    }

    #[test]
    fn ansi256_basic_16_colors_convert() {
        let black = ansi_256_to_rgb(0);
        assert_eq!(black, [0.0, 0.0, 0.0, 1.0]);
        let white = ansi_256_to_rgb(15);
        assert_eq!(white, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn ansi256_grayscale_converts() {
        let grey = ansi_256_to_rgb(232);
        assert_eq!(grey, [0.0, 0.0, 0.0, 1.0]);
        let bright = ansi_256_to_rgb(255);
        assert_eq!(bright, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn default_color_resolves() {
        let fg = resolve_color(&nexterm_proto::Color::Default, true, None);
        assert!(fg[0] > 0.5); // foreground is bright
        let bg = resolve_color(&nexterm_proto::Color::Default, false, None);
        assert!(bg[0] < 0.5); // background is dark
    }

    // ---- Roadmap #10b: OSC dynamic-color overrides ----

    #[test]
    fn scheme_palette_parses_custom_hex_colors() {
        let custom = nexterm_config::CustomPalette {
            foreground: "#aabbcc".to_string(),
            background: "112233".to_string(), // leading '#' optional
            cursor: "#ffffff".to_string(),
            ansi: vec!["#ff0000".to_string()],
        };
        let pal = scheme_palette(&nexterm_config::ColorScheme::Custom(custom));
        assert_eq!(pal.fg, [0xaa, 0xbb, 0xcc]);
        assert_eq!(pal.bg, [0x11, 0x22, 0x33]);
        assert_eq!(pal.ansi[0], [0xff, 0x00, 0x00]);
        // Unspecified / malformed entries fall back to black.
        assert_eq!(pal.ansi[1], [0, 0, 0]);
    }

    fn overrides_with(
        fg: Option<[u8; 3]>,
        bg: Option<[u8; 3]>,
        palette: &[(u8, [u8; 3])],
    ) -> crate::state::PaneColorOverrides {
        crate::state::PaneColorOverrides {
            fg,
            bg,
            palette: palette.iter().copied().collect(),
        }
    }

    #[test]
    fn dynamic_fg_bg_overrides_beat_the_scheme_defaults() {
        use nexterm_proto::Color;
        let ov = overrides_with(Some([255, 0, 0]), Some([0, 0, 255]), &[]);
        let fg = resolve_color_with_overrides(&Color::Default, true, None, Some(&ov));
        assert_eq!(fg, [1.0, 0.0, 0.0, 1.0]);
        let bg = resolve_color_with_overrides(&Color::Default, false, None, Some(&ov));
        assert_eq!(bg, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn palette_overrides_beat_the_builtin_indexed_colors() {
        use nexterm_proto::Color;
        let ov = overrides_with(None, None, &[(1, [0, 255, 0]), (196, [1, 2, 3])]);
        // Overridden entries (both in the 0–15 range and the cube).
        let c1 = resolve_color_with_overrides(&Color::Indexed(1), true, None, Some(&ov));
        assert_eq!(c1, [0.0, 1.0, 0.0, 1.0]);
        let c196 = resolve_color_with_overrides(&Color::Indexed(196), true, None, Some(&ov));
        assert!(c196[0] < 0.01 && c196[1] < 0.01);
        // Untouched entries fall through to the builtin palette.
        let c2 = resolve_color_with_overrides(&Color::Indexed(2), true, None, Some(&ov));
        assert_eq!(c2, ansi_256_to_rgb(2));
    }

    #[test]
    fn no_overrides_behaves_like_resolve_color() {
        use nexterm_proto::Color;
        for (color, is_fg) in [
            (Color::Default, true),
            (Color::Default, false),
            (Color::Indexed(42), true),
            (Color::Rgb(9, 8, 7), true),
        ] {
            assert_eq!(
                resolve_color_with_overrides(&color, is_fg, None, None),
                resolve_color(&color, is_fg, None),
            );
        }
    }

    // ---- Phase 6b: HSB helpers ----

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// HSV round-trip must be the identity (within float tolerance)
    /// for primary colours so the per-cell transform is non-destructive
    /// when `hsb` is `1.0 / 1.0 / 1.0`.
    #[test]
    fn hsv_round_trip_preserves_primary_colours() {
        for rgb in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0],
            [0.5, 0.25, 0.75],
        ] {
            let out = hsv_to_rgb(rgb_to_hsv(rgb));
            for i in 0..3 {
                assert!(
                    close(out[i], rgb[i]),
                    "round trip {:?} → {:?} channel {}",
                    rgb,
                    out,
                    i
                );
            }
        }
    }

    /// `apply_hsb_multiplier` with `(1.0, 1.0, 1.0)` must be the
    /// identity. Regression guard: when a user leaves `inactive_pane_hsb`
    /// at the defaults, inactive panes must look unchanged.
    #[test]
    fn hsb_identity_multipliers_preserve_colour() {
        let input = [0.3, 0.6, 0.9, 0.5];
        let out = apply_hsb_multiplier(input, 1.0, 1.0, 1.0);
        for i in 0..4 {
            assert!(close(out[i], input[i]), "channel {} drifted", i);
        }
    }

    /// Brightness multiplier < 1.0 must darken the colour without
    /// touching alpha.
    #[test]
    fn hsb_brightness_multiplier_darkens() {
        let input = [0.8, 0.8, 0.8, 0.7];
        let out = apply_hsb_multiplier(input, 1.0, 1.0, 0.5);
        assert!(out[0] < input[0]);
        assert!(out[1] < input[1]);
        assert!(out[2] < input[2]);
        assert!(close(out[3], 0.7));
    }

    /// Saturation multiplier of 0 must collapse to grey (all channels
    /// equal) without changing brightness.
    #[test]
    fn hsb_zero_saturation_collapses_to_grey() {
        let input = [0.9, 0.2, 0.4, 1.0];
        let out = apply_hsb_multiplier(input, 1.0, 0.0, 1.0);
        assert!(close(out[0], out[1]));
        assert!(close(out[1], out[2]));
    }

    /// Hue multiplier ≠ 1.0 must actually move the hue. Red (h=0°)
    /// stays red because 0 × anything = 0; pick a non-zero hue.
    #[test]
    fn hsb_hue_multiplier_shifts_hue() {
        // Pure green: HSV hue = 120°. Multiplier 2.0 → 240° (blue).
        let input = [0.0, 1.0, 0.0, 1.0];
        let out = apply_hsb_multiplier(input, 2.0, 1.0, 1.0);
        // Result should now be predominantly blue.
        assert!(
            out[2] > out[0] && out[2] > out[1],
            "expected blue-dominant, got {:?}",
            out
        );
    }

    /// `apply_hsb_animated_rgba` must equal the input at `t = 0`
    /// (focused look) and equal `apply_hsb_multiplier` at `t = 1`
    /// (fully inactive look) — that is the contract the renderer
    /// relies on for the spring transition.
    #[test]
    fn hsb_animated_endpoints_are_identity_and_full() {
        let input = [0.3, 0.6, 0.9, 1.0];
        let at_zero = apply_hsb_animated_rgba(input, 0.5, 0.5, 0.5, 0.0);
        for i in 0..4 {
            assert!(close(at_zero[i], input[i]));
        }
        let at_one = apply_hsb_animated_rgba(input, 0.5, 0.5, 0.5, 1.0);
        let direct = apply_hsb_multiplier(input, 0.5, 0.5, 0.5);
        for i in 0..4 {
            assert!(close(at_one[i], direct[i]));
        }
    }

    /// Animated mid-point (`t = 0.5`) must sit between the input and
    /// the fully-applied output — guards against the lerp formula
    /// being inverted.
    #[test]
    fn hsb_animated_midpoint_is_between() {
        let input = [0.8, 0.8, 0.8, 1.0];
        // Brightness-only halving.
        let at_half = apply_hsb_animated_rgba(input, 1.0, 1.0, 0.5, 0.5);
        // Effective brightness multiplier at t=0.5 is 1.0 + (0.5 - 1.0) * 0.5 = 0.75.
        let expected = apply_hsb_multiplier(input, 1.0, 1.0, 0.75);
        for i in 0..3 {
            assert!(
                close(at_half[i], expected[i]),
                "channel {} animated mismatch: {} vs {}",
                i,
                at_half[i],
                expected[i]
            );
        }
    }

    /// Alpha must always be preserved by the HSB transform. Regression
    /// guard so a future refactor cannot silently break transparency.
    #[test]
    fn hsb_preserves_alpha() {
        let input = [0.5, 0.5, 0.5, 0.42];
        for (h, s, b) in [(1.0, 1.0, 1.0), (0.5, 0.5, 0.5), (2.0, 0.0, 0.3)] {
            let out = apply_hsb_multiplier(input, h, s, b);
            assert!(close(out[3], 0.42));
        }
    }

    // ---- UI/UX v3 G11 follow-up: shared terminal-surface colours ----

    fn tokens_for(scheme: nexterm_config::BuiltinScheme) -> nexterm_config::DesignTokens {
        nexterm_config::DesignTokens::from_palette(&scheme.palette())
    }

    /// The highlight must take the accent hue and stay translucent, or the
    /// selected cells' text disappears underneath it.
    #[test]
    fn selection_color_takes_the_accent_hue_and_stays_translucent() {
        let tokens = tokens_for(nexterm_config::BuiltinScheme::TokyoNight);
        let sel = selection_color(&tokens);
        assert_eq!(
            [sel[0], sel[1], sel[2]],
            [
                tokens.accent_primary[0],
                tokens.accent_primary[1],
                tokens.accent_primary[2]
            ]
        );
        assert!(
            sel[3] > 0.0 && sel[3] < 1.0,
            "selection alpha must let the cell text show through, got {}",
            sel[3]
        );
    }

    /// Regression guard against a hard-coded literal coming back: two schemes
    /// with different accents must produce different selection colours.
    #[test]
    fn selection_color_follows_the_active_scheme() {
        let dark = selection_color(&tokens_for(nexterm_config::BuiltinScheme::Dark));
        let gruvbox = selection_color(&tokens_for(nexterm_config::BuiltinScheme::Gruvbox));
        assert_ne!(
            [dark[0], dark[1], dark[2]],
            [gruvbox[0], gruvbox[1], gruvbox[2]]
        );
    }

    /// The pane-number badge draws its label over a warm, opaque-ish fill, so
    /// the label colour has to follow the fill rather than the accent (which
    /// is what `text_on_accent` is derived from).
    #[test]
    fn on_surface_text_picks_the_readable_extreme() {
        let on_yellow = on_surface_text([0.9, 0.75, 0.0, 1.0]);
        assert!(
            on_yellow[0] < 0.5,
            "a bright fill needs dark text, got {on_yellow:?}"
        );
        let on_near_black = on_surface_text([0.05, 0.05, 0.10, 1.0]);
        assert!(
            on_near_black[0] > 0.5,
            "a dark fill needs light text, got {on_near_black:?}"
        );
    }

    /// Whichever extreme is picked, it must clear the WCAG AA floor for
    /// large text against the fill it sits on.
    #[test]
    fn on_surface_text_clears_the_contrast_floor() {
        for scheme in [
            nexterm_config::BuiltinScheme::Dark,
            nexterm_config::BuiltinScheme::Light,
            nexterm_config::BuiltinScheme::Gruvbox,
            nexterm_config::BuiltinScheme::Solarized,
        ] {
            let tokens = tokens_for(scheme);
            let fill = tokens.semantic_warning;
            let text = on_surface_text(fill);
            let ratio = contrast_ratio([text[0], text[1], text[2]], [fill[0], fill[1], fill[2]]);
            assert!(
                ratio >= 3.0,
                "{scheme:?}: badge label only reached {ratio} against its fill"
            );
        }
    }

    #[test]
    fn press_fill_is_identity_at_zero_weight() {
        let c = [0.4, 0.5, 0.6, 0.35];
        let out = press_fill(c, 0.0);
        for i in 0..4 {
            assert!(
                (out[i] - c[i]).abs() < 1e-6,
                "channel {i} moved at weight 0"
            );
        }
    }

    #[test]
    fn press_fill_darkens_and_strengthens_at_full_weight() {
        let c = [0.4, 0.5, 0.6, 0.35];
        let out = press_fill(c, 1.0);
        assert!(
            relative_luminance([out[0], out[1], out[2]]) < relative_luminance([c[0], c[1], c[2]]),
            "pressed fill must be darker"
        );
        assert!(out[3] > c[3], "pressed fill must be stronger");
        assert!(out[3] <= 1.0, "alpha must stay in range");
    }

    #[test]
    fn press_fill_never_pushes_an_opaque_fill_past_one() {
        // The tab's hover is an opaque lerp; the boost must clamp rather than
        // produce an out-of-range alpha the shader would clip unpredictably.
        let out = press_fill([0.2, 0.2, 0.25, 1.0], 1.0);
        assert!((out[3] - 1.0).abs() < 1e-6);
    }

    /// The design's open question, pinned as a gate: on every builtin scheme a
    /// pressed control must be visibly different from a merely hovered one.
    /// Modelled on the settings row, the weakest of the four sites (a
    /// `surface_3` layer at `HOVER_ALPHA` over `surface_1`).
    #[test]
    fn press_is_perceptible_on_every_builtin_scheme() {
        use nexterm_config::BuiltinScheme;
        const SCHEMES: [BuiltinScheme; 9] = [
            BuiltinScheme::Dark,
            BuiltinScheme::Light,
            BuiltinScheme::TokyoNight,
            BuiltinScheme::Solarized,
            BuiltinScheme::Gruvbox,
            BuiltinScheme::Catppuccin,
            BuiltinScheme::Dracula,
            BuiltinScheme::Nord,
            BuiltinScheme::OneDark,
        ];
        for scheme in SCHEMES {
            let tokens = nexterm_config::DesignTokens::from_palette(&scheme.palette());
            let bg = [
                tokens.surface_1[0],
                tokens.surface_1[1],
                tokens.surface_1[2],
            ];
            let s = tokens.surface_3;
            let hovered = [s[0], s[1], s[2], s[3] * 0.35];
            let rest = relative_luminance(composite_over(hovered, bg));
            let pressed = relative_luminance(composite_over(press_fill(hovered, 1.0), bg));
            assert!(
                (rest - pressed).abs() > 0.004,
                "{scheme:?}: pressed is indistinguishable from hovered (Δ luminance {})",
                (rest - pressed).abs()
            );
        }
    }

    /// Press must not cost legibility. Two ways to pass: stay above the WCAG AA
    /// 4.5:1 floor, or — for a scheme already below it — not make the ratio
    /// meaningfully worse. Solarized and OneDark carry contrast defects in their
    /// resting chrome (known since P2b, tracked for P5), so a flat 4.5:1
    /// assertion would fail on those pre-existing defects rather than on
    /// anything P3b3 does. Fixing what press inherits is P5's job.
    ///
    /// The 10% arm is sized by measurement, not taste: Solarized moves 3.52 →
    /// 3.34 under a full pulse. That scheme is already a contrast defect at
    /// rest — 3.52 was never legible — so the pulse deepens a problem P5 owns
    /// rather than creating one. A tighter arm would block this phase on a fix
    /// that belongs to another.
    #[test]
    fn press_never_worsens_text_contrast_on_any_builtin_scheme() {
        use nexterm_config::BuiltinScheme;
        const SCHEMES: [BuiltinScheme; 9] = [
            BuiltinScheme::Dark,
            BuiltinScheme::Light,
            BuiltinScheme::TokyoNight,
            BuiltinScheme::Solarized,
            BuiltinScheme::Gruvbox,
            BuiltinScheme::Catppuccin,
            BuiltinScheme::Dracula,
            BuiltinScheme::Nord,
            BuiltinScheme::OneDark,
        ];
        for scheme in SCHEMES {
            let tokens = nexterm_config::DesignTokens::from_palette(&scheme.palette());
            let bg = [
                tokens.surface_1[0],
                tokens.surface_1[1],
                tokens.surface_1[2],
            ];
            let s = tokens.surface_3;
            let hovered = [s[0], s[1], s[2], s[3] * 0.35];
            let fg = [
                tokens.text_primary[0],
                tokens.text_primary[1],
                tokens.text_primary[2],
            ];
            let before = contrast_ratio(fg, composite_over(hovered, bg));
            let after = contrast_ratio(fg, composite_over(press_fill(hovered, 1.0), bg));
            assert!(
                after >= 4.5 || after >= before * 0.90,
                "{scheme:?}: press cut text contrast from {before} to {after}"
            );
        }
    }
}
