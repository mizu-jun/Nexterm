//! Fading a whole overlay surface (UI/UX v3 P3b).
//!
//! An overlay's entrance and exit are applied *after* its builder has run,
//! by scaling the alpha of the vertices that builder appended. Both overlay
//! shaders take straight alpha in the vertex color and premultiply in the
//! fragment stage (`shaders.rs`: `return vec4(c.rgb * c.a, c.a)`), so
//! scaling `color[3]` is the correct and only edit — and it is correct for
//! acrylic panel fills too, since `acrylic_mix` blends `rgb` only.
//!
//! Doing it here rather than inside each builder is what lets ten surfaces
//! gain motion without ten independently written layout diffs.

use crate::glyph_atlas::{BgVertex, TextVertex};

/// Scale the alpha of `bg` and `text` by `alpha`, clamped to `[0, 1]`.
///
/// Pass the sub-slices a single surface appended, e.g.
///
/// ```ignore
/// let (bg_start, text_start) = (bg_verts.len(), text_verts.len());
/// self.build_macro_picker_verts(/* ... */);
/// apply_surface_fade(
///     &mut bg_verts[bg_start..],
///     &mut text_verts[text_start..],
///     progress,
/// );
/// ```
pub(in crate::renderer) fn apply_surface_fade(
    bg: &mut [BgVertex],
    text: &mut [TextVertex],
    alpha: f32,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha >= 1.0 {
        return;
    }
    for v in bg {
        v.color[3] *= alpha;
    }
    for v in text {
        v.color[3] *= alpha;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bg(alpha: f32) -> BgVertex {
        BgVertex {
            position: [0.0, 0.0],
            color: [0.2, 0.4, 0.6, alpha],
            rect_center: [0.0, 0.0],
            rect_half_size: [0.0, 0.0],
            corner_radius: 0.0,
            shadow_softness: 0.0,
            stroke_width: 0.0,
            acrylic_mix: 0.0,
        }
    }

    fn text(alpha: f32) -> TextVertex {
        TextVertex {
            position: [0.0, 0.0],
            uv: [0.0, 0.0],
            color: [1.0, 1.0, 1.0, alpha],
        }
    }

    #[test]
    fn a_half_fade_halves_every_alpha_and_leaves_rgb_alone() {
        let mut b = [bg(1.0), bg(0.5)];
        let mut t = [text(1.0)];
        apply_surface_fade(&mut b, &mut t, 0.5);
        assert!((b[0].color[3] - 0.5).abs() < 1e-6);
        assert!((b[1].color[3] - 0.25).abs() < 1e-6);
        assert!((t[0].color[3] - 0.5).abs() < 1e-6);
        assert!((b[0].color[0] - 0.2).abs() < 1e-6, "rgb must not change");
    }

    #[test]
    fn a_full_fade_changes_nothing() {
        let mut b = [bg(0.75)];
        let mut t = [text(0.75)];
        apply_surface_fade(&mut b, &mut t, 1.0);
        assert!((b[0].color[3] - 0.75).abs() < 1e-6);
        assert!((t[0].color[3] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_alphas_are_clamped() {
        let mut b = [bg(1.0)];
        let mut t = [];
        apply_surface_fade(&mut b, &mut t, 1.7);
        assert!((b[0].color[3] - 1.0).abs() < 1e-6);
        apply_surface_fade(&mut b, &mut t, -0.3);
        assert!(b[0].color[3].abs() < 1e-6);
    }

    #[test]
    fn empty_ranges_are_fine() {
        let mut b: [BgVertex; 0] = [];
        let mut t: [TextVertex; 0] = [];
        apply_surface_fade(&mut b, &mut t, 0.5);
    }
}
