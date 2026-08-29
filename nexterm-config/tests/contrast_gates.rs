//! Contrast gates for the design-token set (UI/UX v3 P5a).
//!
//! These are the phase's acceptance criteria, not illustrations. Before P5a,
//! `text_muted` failed the 4.5:1 floor on all nine built-in schemes at every
//! surface level, and the Light scheme's semantic green and yellow reached as
//! low as 1.18:1. The gates below fail if any of that comes back — including
//! for a user's own palette, which is what `custom_palettes_*` is for.
//!
//! Scope note: these cover the **text** role. The fill role (`semantic_*` and
//! `accent_primary` drawn as banner grounds, stripes, focus rings and borders)
//! has a different floor — WCAG's 3:1 for non-text — and the right assertion
//! depends on whether a given site is a UI-component boundary or a large filled
//! region. That classification is per-call-site work and lands with P5b, which
//! is the PR that touches those sites.

use nexterm_config::{
    BuiltinScheme, DesignTokens, MIN_TEXT_CONTRAST, SchemePalette, SurfaceLevel, TextTokens,
    composite_over, wcag_contrast,
};

/// The ground each level is drawn on, as an opaque RGB triple.
fn ground(tokens: &DesignTokens, level: SurfaceLevel) -> [f32; 3] {
    let s = match level {
        SurfaceLevel::S0 => tokens.surface_0,
        SurfaceLevel::S1 => tokens.surface_1,
        SurfaceLevel::S2 => tokens.surface_2,
        SurfaceLevel::S3 => tokens.surface_3,
    };
    [s[0], s[1], s[2]]
}

/// Every field of [`TextTokens`], paired with its name for failure messages.
fn text_fields(t: &TextTokens) -> [(&'static str, [f32; 4]); 8] {
    [
        ("primary", t.primary),
        ("secondary", t.secondary),
        ("muted", t.muted),
        ("accent", t.accent),
        ("success", t.success),
        ("warning", t.warning),
        ("error", t.error),
        ("info", t.info),
    ]
}

/// The scheme's foreground exactly as the palette states it — the input the
/// correction is judged against. P5b removed the flat `text_primary` field so
/// no call site can pick a colour without naming its ground; these tests want
/// the uncorrected value, so they reconstruct it.
fn raw_foreground(palette: &SchemePalette) -> [f32; 4] {
    let c = palette.fg.map(|v| v as f32 / 255.0);
    [c[0], c[1], c[2], 1.0]
}

/// Ratio of one text token against its own surface, alpha composited.
fn ratio_on(tokens: &DesignTokens, level: SurfaceLevel, color: [f32; 4]) -> f32 {
    let bg = ground(tokens, level);
    wcag_contrast(composite_over(color, bg), bg)
}

/// Collect every `(label, ratio)` below the floor for one token set.
fn failures(tokens: &DesignTokens) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    for level in SurfaceLevel::ALL {
        for (name, color) in text_fields(tokens.text_on(level)) {
            let r = ratio_on(tokens, level, color);
            if r < MIN_TEXT_CONTRAST {
                out.push((format!("{level:?}/{name}"), r));
            }
        }
    }
    out
}

/// **G-text.** Every built-in scheme × every surface level × every text-role
/// token clears 4.5:1.
#[test]
fn every_builtin_scheme_meets_the_text_floor_on_every_surface() {
    let mut report = Vec::new();
    for scheme in BuiltinScheme::all() {
        let tokens = DesignTokens::from_palette(&scheme.palette());
        for (label, ratio) in failures(&tokens) {
            report.push(format!("  {} {label} = {ratio:.2}", scheme.display_name()));
        }
    }
    assert!(
        report.is_empty(),
        "text tokens below {MIN_TEXT_CONTRAST}:1:\n{}",
        report.join("\n")
    );
}

/// The correction must not be a blunt "paint everything black or white": a
/// scheme whose text already reads must keep the colour the user chose.
///
/// Tokyo Night's body text clears the floor on all four surfaces before P5a
/// (10.59 / 9.23 / 7.58 / 5.99), so any change to it would be gratuitous.
#[test]
fn correction_is_a_no_op_where_the_scheme_already_reads() {
    let palette = BuiltinScheme::TokyoNight.palette();
    let tokens = DesignTokens::from_palette(&palette);
    for level in SurfaceLevel::ALL {
        assert_eq!(
            tokens.text_on(level).primary,
            raw_foreground(&palette),
            "{level:?}: Tokyo Night body text already clears the floor and must be left alone"
        );
    }
}

/// Solarized is the scheme the roadmap named. Its defect is a mid-luminance
/// *foreground* over a dark ground, so the fix is to lighten — and stage 2 does
/// that with hue and saturation intact. Pinning "still recognisably the same
/// hue" is what stops a future change from solving contrast by flooding the
/// scheme with white.
#[test]
fn solarized_body_text_is_lightened_without_losing_its_hue() {
    let palette = BuiltinScheme::Solarized.palette();
    let tokens = DesignTokens::from_palette(&palette);
    let raw = raw_foreground(&palette);
    let fixed = tokens.text_on(SurfaceLevel::S3).primary;

    assert!(
        ratio_on(&tokens, SurfaceLevel::S3, fixed) >= MIN_TEXT_CONTRAST,
        "the whole point of the correction"
    );
    assert!(
        fixed[0] > raw[0] && fixed[1] > raw[1] && fixed[2] > raw[2],
        "expected a lift on every channel, got {raw:?} -> {fixed:?}"
    );
    assert!(
        fixed[0] < 1.0 || fixed[1] < 1.0 || fixed[2] < 1.0,
        "stage 2 has headroom here; the result must not be flat white: {fixed:?}"
    );
    // A uniform `v` ramp holds channel ratios exactly. Allowing 2 % covers the
    // clamp at the top of the ramp without admitting a tint toward white.
    let k = fixed[2] / raw[2];
    for c in 0..3 {
        assert!(
            (fixed[c] / raw[c] - k).abs() < 0.02,
            "channel {c} moved off the hue: {raw:?} -> {fixed:?}"
        );
    }
}

/// **G-custom.** The built-ins are nine fixed points; a user's `CustomPalette`
/// is the general case, and it is the one no reviewer will ever eyeball. A
/// deterministic sweep stands in for a property test without adding a
/// dependency: the seed is fixed, so a failure is always reproducible.
#[test]
fn custom_palettes_meet_the_text_floor() {
    // xorshift32 — deterministic, and adding `rand` for a test would be a
    // supply-chain cost for no benefit.
    let mut state: u32 = 0x5eed_1337;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let mut byte = || (next() & 0xff) as u8;

    let mut report = Vec::new();
    for case in 0..512 {
        let palette = SchemePalette {
            fg: [byte(), byte(), byte()],
            bg: [byte(), byte(), byte()],
            ansi: std::array::from_fn(|_| [byte(), byte(), byte()]),
        };
        let tokens = DesignTokens::from_palette(&palette);
        for (label, ratio) in failures(&tokens) {
            report.push(format!(
                "  case {case} fg={:?} bg={:?} {label} = {ratio:.2}",
                palette.fg, palette.bg
            ));
        }
    }
    assert!(
        report.is_empty(),
        "generated palettes below {MIN_TEXT_CONTRAST}:1:\n{}",
        report.join("\n")
    );
}

/// The pathological case the sweep above is unlikely to hit exactly: a ground
/// sitting at `NEUTRAL_LUMINANCE`, where the ceiling is ≈ 4.58:1 and the floor
/// has almost no margin. If a future change loosens the search, this is the
/// test that notices.
#[test]
fn a_mid_tone_ground_still_clears_the_floor() {
    // sRGB 0x77 has relative luminance ≈ 0.1845, just above the neutral point.
    let palette = SchemePalette {
        fg: [0x78, 0x78, 0x78],
        bg: [0x77, 0x77, 0x77],
        ansi: std::array::from_fn(|_| [0x77, 0x77, 0x77]),
    };
    let tokens = DesignTokens::from_palette(&palette);
    let bad = failures(&tokens);
    assert!(bad.is_empty(), "mid-tone ground: {bad:?}");
}

/// The property P5b's call-site migration leans on.
///
/// Several surfaces change ground with state — a settings row is the panel's
/// `surface_2` at rest and a `surface_3` blend when hovered — and giving one
/// row two text colours would mean animating the colour on hover. Those sites
/// take the level of the *worst* ground they can appear over instead. That is
/// only sound if a colour corrected for the bottom of the ramp still reads
/// everywhere above it, which holds because the surfaces are a monotone
/// luminance sequence and the correction pushes text the other way. This
/// pins it rather than leaving it as an argument.
#[test]
fn text_corrected_for_the_deepest_surface_reads_on_every_shallower_one() {
    let mut report = Vec::new();
    for scheme in BuiltinScheme::all() {
        let tokens = DesignTokens::from_palette(&scheme.palette());
        for (name, color) in text_fields(tokens.text_on(SurfaceLevel::S3)) {
            for level in [SurfaceLevel::S0, SurfaceLevel::S1, SurfaceLevel::S2] {
                let r = ratio_on(&tokens, level, color);
                if r < MIN_TEXT_CONTRAST {
                    report.push(format!(
                        "  {} S3/{name} on {level:?} = {r:.2}",
                        scheme.display_name()
                    ));
                }
            }
        }
    }
    assert!(
        report.is_empty(),
        "S3-corrected text fails on a shallower surface:\n{}",
        report.join("\n")
    );
}
