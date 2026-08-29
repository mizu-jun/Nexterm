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
    /// Text on top of a solid accent surface (black or white for readability).
    ///
    /// Unlike the surface-relative text roles this is *fill*-relative — it is
    /// chosen from `accent_primary`'s own luminance — so it is not part of
    /// [`TextTokens`] and stays here.
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

    // ── Per-surface text (UI/UX v3 P5a) ──────────────────────────────────────
    /// Text-role colours corrected per surface level. Read via
    /// [`DesignTokens::text_on`]; the array order is [`SurfaceLevel::ALL`].
    on_surface: [TextTokens; 4],
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
// WCAG contrast (UI/UX v3 P5a)
// ─────────────────────────────────────────────────────────────────────────────
//
// Note the deliberate split from `luminance` above. That one is a plain BT.709
// dot product on the raw channels and decides *dark vs light* for the surface
// ramp; changing it would move every scheme's chrome. The functions here apply
// the sRGB transfer function first, which is what WCAG 2.x specifies and what
// the ratios in this module's tests are quoted against.

/// WCAG 2.x contrast floor for text.
pub const MIN_TEXT_CONTRAST: f32 = 4.5;

/// The background luminance at which black and white are equally legible.
///
/// The best ratio obtainable against a background of relative luminance `Y` is
/// `max(1.05 / (Y + 0.05), (Y + 0.05) / 0.05)`. The two arms meet here, at a
/// ceiling of ≈ 4.58:1 — so 4.5:1 is reachable against *any* background, and
/// 7:1 is not. [`contrast_correct`] uses this to pick a direction.
pub const NEUTRAL_LUMINANCE: f32 = 0.179_13;

/// Bisection steps used by [`contrast_correct`]. Twelve halvings resolve the
/// search parameter to ~1/4096, well below one 8-bit channel step.
const BISECT_STEPS: u32 = 12;

/// WCAG 2.x relative luminance of an sRGB color (components in `[0, 1]`).
///
/// <https://www.w3.org/TR/WCAG21/#dfn-relative-luminance>
pub fn wcag_luminance(rgb: [f32; 3]) -> f32 {
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

/// WCAG 2.x contrast ratio between two opaque colors, in `[1, 21]`.
pub fn wcag_contrast(fg: [f32; 3], bg: [f32; 3]) -> f32 {
    let l1 = wcag_luminance(fg);
    let l2 = wcag_luminance(bg);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

/// Composite a straight-alpha RGBA color over an opaque background.
pub fn composite_over(fg: [f32; 4], bg: [f32; 3]) -> [f32; 3] {
    let a = fg[3].clamp(0.0, 1.0);
    [
        fg[0] * a + bg[0] * (1.0 - a),
        fg[1] * a + bg[1] * (1.0 - a),
        fg[2] * a + bg[2] * (1.0 - a),
    ]
}

/// Smallest `t` in `[lo, hi]` for which `pred` holds.
///
/// Every caller below feeds a predicate whose true-set is an upper interval:
/// each search path moves luminance monotonically, so contrast against a fixed
/// background may dip once (where the path crosses the background's own
/// luminance) and then rises without turning back. `pred(lo)` is always false
/// by construction — the caller has already rejected that point — so returning
/// `hi` is correct whether or not `pred(hi)` holds, and an unreachable
/// `min_ratio` degrades to "the most extreme point on this path".
fn bisect(mut lo: f32, mut hi: f32, pred: impl Fn(f32) -> bool) -> f32 {
    for _ in 0..BISECT_STEPS {
        let mid = 0.5 * (lo + hi);
        if pred(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// Scale every channel by `k`, clamped to the unit cube. Uniform scaling is an
/// HSV `v` ramp: hue and saturation are untouched.
#[inline]
fn scale(rgb: [f32; 3], k: f32) -> [f32; 3] {
    [
        (rgb[0] * k).clamp(0.0, 1.0),
        (rgb[1] * k).clamp(0.0, 1.0),
        (rgb[2] * k).clamp(0.0, 1.0),
    ]
}

#[inline]
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Darken until the floor is met. Black is always reachable at `k = 0`, so
/// this arm never needs to touch saturation.
fn darken_to(rgb: [f32; 3], bg: [f32; 3], min_ratio: f32) -> [f32; 3] {
    let t = bisect(0.0, 1.0, |t| {
        wcag_contrast(scale(rgb, 1.0 - t), bg) >= min_ratio
    });
    scale(rgb, 1.0 - t)
}

/// Lighten until the floor is met, in two stages.
fn lighten_to(rgb: [f32; 3], bg: [f32; 3], min_ratio: f32) -> [f32; 3] {
    let white = [1.0, 1.0, 1.0];
    let max_c = rgb[0].max(rgb[1]).max(rgb[2]);

    // Stage 2 — raise `v` until the largest channel saturates. Pure black has
    // no `v` to ramp (scaling leaves it black), so it skips straight to the tint.
    if max_c > 1e-4 {
        let headroom = 1.0 / max_c;
        let peak = scale(rgb, headroom);
        if wcag_contrast(peak, bg) >= min_ratio {
            let k = bisect(1.0, headroom, |k| {
                wcag_contrast(scale(rgb, k), bg) >= min_ratio
            });
            return scale(rgb, k);
        }
        // Stage 3 — the hue is as bright as this hue gets (a saturated blue
        // peaks at Y = 0.0722), so the only way up costs saturation.
        let t = bisect(0.0, 1.0, |t| {
            wcag_contrast(lerp3(peak, white, t), bg) >= min_ratio
        });
        return lerp3(peak, white, t);
    }

    let t = bisect(0.0, 1.0, |t| {
        wcag_contrast(lerp3(rgb, white, t), bg) >= min_ratio
    });
    lerp3(rgb, white, t)
}

/// Adjust `color` until it reaches `min_ratio` WCAG contrast against the opaque
/// background `bg`, in three stages, stopping at the first that succeeds.
///
/// 1. **Alpha.** A token like `text_muted` carries a fixed alpha tuned against
///    an opaque UI in general; over one specific surface the composited result
///    can land under the floor. Raising alpha fixes that without touching hue.
/// 2. **Value.** If the opaque color still falls short the problem is the hue's
///    own luminance, so ramp HSV `v` toward whichever extreme has more room
///    against `bg` (see [`NEUTRAL_LUMINANCE`]). Hue and saturation survive.
/// 3. **Saturation.** Only on the lighten path, and only once `v` has
///    saturated: tint toward white.
///
/// Returns `color` untouched when it already clears the bar. When `min_ratio`
/// is unreachable against `bg` — 7:1 against a mid-tone ground, say — the
/// result is the best achievable color rather than an error; callers that need
/// a guarantee assert it against the derived token set instead.
pub fn contrast_correct(color: [f32; 4], bg: [f32; 3], min_ratio: f32) -> [f32; 4] {
    if wcag_contrast(composite_over(color, bg), bg) >= min_ratio {
        return color;
    }

    // Stage 1. Compositing walks a straight line from `bg` (alpha 0) to the
    // opaque color (alpha 1), so contrast is monotone in alpha here — no dip.
    let rgb = [color[0], color[1], color[2]];
    if wcag_contrast(rgb, bg) >= min_ratio {
        let a = bisect(color[3], 1.0, |a| {
            wcag_contrast(composite_over([rgb[0], rgb[1], rgb[2], a], bg), bg) >= min_ratio
        });
        return [rgb[0], rgb[1], rgb[2], a];
    }

    // Stages 2/3.
    let out = if wcag_luminance(bg) > NEUTRAL_LUMINANCE {
        darken_to(rgb, bg, min_ratio)
    } else {
        lighten_to(rgb, bg, min_ratio)
    };
    [out[0], out[1], out[2], 1.0]
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-surface text tokens (UI/UX v3 P5a)
// ─────────────────────────────────────────────────────────────────────────────

/// Which layered chrome surface a run of text is drawn on.
///
/// Contrast decays monotonically as the ramp lifts the ground toward the
/// foreground, so a text colour is only meaningful together with its ground.
/// Naming the level is how a call site says which ground that is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceLevel {
    /// Terminal background — `surface_0`.
    S0,
    /// Tab bar / status bar — `surface_1`.
    S1,
    /// Overlay / active tab — `surface_2`.
    S2,
    /// Hover / selected item — `surface_3`.
    S3,
}

impl SurfaceLevel {
    /// Every level, for exhaustive iteration in the contrast gates.
    pub const ALL: [Self; 4] = [Self::S0, Self::S1, Self::S2, Self::S3];

    #[inline]
    const fn index(self) -> usize {
        match self {
            Self::S0 => 0,
            Self::S1 => 1,
            Self::S2 => 2,
            Self::S3 => 3,
        }
    }
}

/// Text-role colours corrected for one surface level.
///
/// Every field clears [`MIN_TEXT_CONTRAST`] against that surface — pinned by
/// the gates in `tests/contrast_gates.rs` for the built-in schemes and for
/// generated custom palettes alike.
///
/// These are the *text* role only. The fill-role tokens on [`DesignTokens`]
/// (`semantic_*`, `accent_primary`, the surfaces and borders) keep their raw
/// hue: measured across the client, `semantic_*` serves roughly four times as
/// many fills — banner backgrounds, the SFTP accent stripe, danger fills,
/// error borders — as it does text runs, and darkening the raw token to satisfy
/// a text floor would wreck the dominant use.
#[derive(Debug, Clone)]
pub struct TextTokens {
    /// Body text.
    pub primary: [f32; 4],
    /// Secondary text.
    pub secondary: [f32; 4],
    /// Muted / placeholder text.
    pub muted: [f32; 4],
    /// Accent-coloured text (links, emphasis) — *not* the accent fill.
    pub accent: [f32; 4],
    /// Success text.
    pub success: [f32; 4],
    /// Warning text.
    pub warning: [f32; 4],
    /// Error text.
    pub error: [f32; 4],
    /// Info text.
    pub info: [f32; 4],
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

        // ── Per-surface text (UI/UX v3 P5a) ──────────────────────────────────
        // Derived last, because every entry needs the surface it will be drawn
        // on. The correction is a no-op for the pairs that already clear the
        // floor, which on a well-behaved scheme is most of them.
        let on_surface = [surface_0, surface_1, surface_2, surface_3].map(|s| {
            let bg = [s[0], s[1], s[2]];
            let fix = |c: [f32; 4]| contrast_correct(c, bg, MIN_TEXT_CONTRAST);
            TextTokens {
                primary: fix(text_primary),
                secondary: fix(text_secondary),
                muted: fix(text_muted),
                accent: fix(accent_primary),
                success: fix(semantic_success),
                warning: fix(semantic_warning),
                error: fix(semantic_error),
                info: fix(semantic_info),
            }
        });

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
            text_on_accent,
            semantic_success,
            semantic_warning,
            semantic_error,
            semantic_info,
            tab_active_bg: surface_2,
            tab_inactive_bg: surface_1,
            tab_activity_bg: accent_activity,
            on_surface,
        }
    }

    /// Text colours guaranteed legible on `level`.
    ///
    /// The flat `text_*` / `semantic_*` fields are the *uncorrected* palette
    /// values and are being retired in P5b; new text call sites go through
    /// here, naming the surface they draw on.
    pub fn text_on(&self, level: SurfaceLevel) -> &TextTokens {
        &self.on_surface[level.index()]
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

    // ── contrast_correct (UI/UX v3 P5a) ──────────────────────────────────────

    const DARK_GROUND: [f32; 3] = [0.1, 0.1, 0.12];
    const LIGHT_GROUND: [f32; 3] = [0.95, 0.95, 0.93];

    fn ratio(color: [f32; 4], bg: [f32; 3]) -> f32 {
        wcag_contrast(composite_over(color, bg), bg)
    }

    /// The constant that decides the correction direction is not a taste
    /// value: it is where the two ceilings cross. If someone "rounds" it, this
    /// test says why they should not.
    #[test]
    fn neutral_luminance_is_where_black_and_white_tie() {
        let y = NEUTRAL_LUMINANCE;
        let via_white = 1.05 / (y + 0.05);
        let via_black = (y + 0.05) / 0.05;
        assert!(
            (via_white - via_black).abs() < 0.001,
            "ceilings disagree: white {via_white}, black {via_black}"
        );
        assert!(
            via_white > MIN_TEXT_CONTRAST && via_white < 4.6,
            "the worst-case ceiling should sit just above the floor, got {via_white}"
        );
    }

    #[test]
    fn a_colour_that_already_reads_is_returned_untouched() {
        let white = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(
            contrast_correct(white, DARK_GROUND, MIN_TEXT_CONTRAST),
            white
        );
    }

    /// Stage 1 exists so a translucency problem is not "solved" by mangling the
    /// hue. A fully opaque white clears the floor here, so only alpha may move.
    #[test]
    fn a_translucent_colour_is_fixed_by_alpha_alone() {
        let faint = [1.0, 1.0, 1.0, 0.2];
        let fixed = contrast_correct(faint, DARK_GROUND, MIN_TEXT_CONTRAST);
        assert_eq!([fixed[0], fixed[1], fixed[2]], [1.0, 1.0, 1.0]);
        assert!(fixed[3] > faint[3] && fixed[3] <= 1.0, "alpha {}", fixed[3]);
        assert!(ratio(fixed, DARK_GROUND) >= MIN_TEXT_CONTRAST);
    }

    /// …and it must stop as soon as the floor is met rather than slamming to
    /// opaque, or every muted token would silently become body text.
    #[test]
    fn alpha_stops_at_the_floor_instead_of_going_opaque() {
        let faint = [1.0, 1.0, 1.0, 0.2];
        let fixed = contrast_correct(faint, DARK_GROUND, MIN_TEXT_CONTRAST);
        assert!(
            fixed[3] < 0.95,
            "white on a dark ground needs far less than full alpha, got {}",
            fixed[3]
        );
    }

    /// Stage 2 on the dark-ground side: a uniform `v` ramp, so the channel
    /// ratios that define the hue survive exactly.
    #[test]
    fn a_dark_hue_on_a_dark_ground_is_lightened_along_its_own_hue() {
        // A slate with enough headroom that stage 2 alone clears the floor —
        // this is the Solarized shape, and the case the ramp is for. A more
        // saturated hue (`[0.05, 0.08, 0.20]`, say) peaks at Y = 0.178 even at
        // v = 1 and has to fall through to the tint; that is
        // `a_saturated_hue_that_cannot_brighten_enough_is_tinted` below.
        let slate = [0.10, 0.12, 0.18, 1.0];
        let fixed = contrast_correct(slate, DARK_GROUND, MIN_TEXT_CONTRAST);
        assert!(ratio(fixed, DARK_GROUND) >= MIN_TEXT_CONTRAST);
        let k = fixed[2] / slate[2];
        assert!(k > 1.0, "expected a lift, got {k}");
        assert!((fixed[0] / slate[0] - k).abs() < 0.01, "{fixed:?}");
        assert!((fixed[1] / slate[1] - k).abs() < 0.01, "{fixed:?}");
    }

    /// The mirror case: against a light ground the direction flips, and
    /// darkening never needs stage 3 because black is always reachable.
    #[test]
    fn a_light_hue_on_a_light_ground_is_darkened_along_its_own_hue() {
        let cream = [1.0, 0.95, 0.75, 1.0];
        let fixed = contrast_correct(cream, LIGHT_GROUND, MIN_TEXT_CONTRAST);
        assert!(ratio(fixed, LIGHT_GROUND) >= MIN_TEXT_CONTRAST);
        let k = fixed[1] / cream[1];
        assert!(k < 1.0, "expected a drop, got {k}");
        assert!((fixed[2] / cream[2] - k).abs() < 0.01);
    }

    /// Stage 3's reason to exist: a saturated blue is dark even at `v = 1`
    /// (`Y = 0.0722`), so on a light ground… it darkens. On a *dark* ground
    /// with a high floor it must tint, losing saturation, because the hue
    /// alone cannot get bright enough.
    #[test]
    fn a_saturated_hue_that_cannot_brighten_enough_is_tinted() {
        let blue = [0.0, 0.0, 1.0, 1.0];
        let ground = [0.35, 0.35, 0.35];
        let fixed = contrast_correct(blue, ground, MIN_TEXT_CONTRAST);
        assert!(ratio(fixed, ground) >= MIN_TEXT_CONTRAST);
        assert!(
            fixed[0] > 0.05 && fixed[1] > 0.05,
            "a tint must raise the other channels: {fixed:?}"
        );
    }

    /// An unreachable ratio is not an error. 7:1 against a mid-tone ground is
    /// impossible (§ [`NEUTRAL_LUMINANCE`]); the contract is "best effort", and
    /// best effort here is an extreme that still clears the 4.5:1 floor.
    #[test]
    fn an_unreachable_ratio_degrades_to_the_best_available_colour() {
        let ground = [0.46, 0.46, 0.46]; // Y ≈ 0.179
        let grey = [0.5, 0.5, 0.5, 1.0];
        let fixed = contrast_correct(grey, ground, 7.0);
        let got = ratio(fixed, ground);
        assert!(
            got < 7.0,
            "the test's premise is that 7:1 is unreachable here, got {got}"
        );
        assert!(
            got >= MIN_TEXT_CONTRAST,
            "best effort must still clear the text floor, got {got}"
        );
    }

    /// `text_on` must not hand out a colour derived against a different ground.
    #[test]
    fn text_on_returns_a_distinct_set_per_surface() {
        let tokens = DesignTokens::from_palette(&tokyo_night_palette());
        let muted: Vec<[f32; 4]> = SurfaceLevel::ALL
            .iter()
            .map(|l| tokens.text_on(*l).muted)
            .collect();
        assert!(
            muted.windows(2).any(|w| w[0] != w[1]),
            "contrast decays across the ramp, so the corrections cannot all be equal: {muted:?}"
        );
    }
}
