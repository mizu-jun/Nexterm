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

#[cfg(test)]
mod tests {
    use super::*;

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
}
