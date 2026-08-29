//! Tooltip: the first component built on the widget layer.
//!
//! A tooltip is not a widget of its own — it is a transient surface attached
//! to whichever widget the pointer has been resting on. This module owns the
//! placement math ([`place_tooltip`], pure and unit-tested) and the drawing;
//! the dwell timing lives with the rest of the panel state in
//! [`crate::settings::HoverDwell`].
//!
//! Elevation and radius come from [`nexterm_config::MetricTokens`]: Tooltip
//! sits at elevation 16 with the control radius, rendered as a real soft
//! shadow through the shared `shadow_params` mapping (UI/UX v3 P2a), so its
//! weight stays ordered against the other overlay surfaces.

use crate::font::FontManager;
use crate::glyph_atlas::GlyphAtlas;
use crate::vertex_util::{
    add_px_rounded_rect_sdf, add_px_rounded_rect_sdf_with_acrylic, add_px_soft_shadow_sdf,
    add_run_verts, measure_run,
};

use super::super::util::shadow_params;
use super::draw::{WidgetSink, WidgetTheme};
use super::spec::WidgetRect;

/// Gap between the anchor widget and the tooltip, in character cells.
const ANCHOR_GAP_CELLS: f32 = 0.4;
/// Horizontal padding inside the tooltip, in character cells.
const PAD_X_CELLS: f32 = 0.6;
/// Vertical padding inside the tooltip, in character cells.
const PAD_Y_CELLS: f32 = 0.25;
/// Where a tooltip of the given text should be drawn.
///
/// Prefers directly below the anchor. Flips above when it would fall off the
/// bottom edge, and clamps horizontally so it never leaves the surface. When
/// the tooltip cannot fit in either direction the below-placement is kept and
/// clamped, which is still better than drawing off-screen.
///
/// UI/UX v3 P4b: the text width arrives **pre-measured** rather than being
/// derived from a cell count here. The tooltip sizes itself from its text, so
/// the box and the glyphs have to come from one measurement — and that
/// measurement now needs a `FontManager`, which this function deliberately
/// does not take so it can stay pure and unit-testable. `line_h` is the ramp
/// step's line height, for the same reason.
pub(crate) fn place_tooltip(
    anchor: WidgetRect,
    text_w: f32,
    line_h: f32,
    cell_w: f32,
    cell_h: f32,
    surface_w: f32,
    surface_h: f32,
) -> WidgetRect {
    let w = text_w + PAD_X_CELLS * 2.0 * cell_w;
    let h = line_h + PAD_Y_CELLS * 2.0 * cell_h;
    let gap = ANCHOR_GAP_CELLS * cell_h;

    let below_y = anchor.y + anchor.h + gap;
    let above_y = anchor.y - gap - h;
    let y = if below_y + h <= surface_h {
        below_y
    } else if above_y >= 0.0 {
        above_y
    } else {
        below_y.min((surface_h - h).max(0.0))
    };

    // Centre on the anchor, then pull back inside the surface.
    let (anchor_cx, _) = anchor.center();
    let x = (anchor_cx - w * 0.5).clamp(0.0, (surface_w - w).max(0.0));

    WidgetRect::new(x, y, w, h)
}

/// Draw a tooltip: shadow, border ring, surface, then the text.
///
/// `acrylic_mix` reaches the surface fill only — the shadow and border ring
/// stay opaque, matching how `draw_overlay_panel`'s three-layer chrome treats
/// acrylic (UI/UX v3 P2b).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_tooltip(
    rect: WidgetRect,
    text: &str,
    theme: &WidgetTheme<'_>,
    acrylic_mix: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let r = theme.metrics.radius.control;
    let shadow = shadow_params(theme.metrics.elevation.tooltip);

    add_px_soft_shadow_sdf(
        rect.x + shadow.offset,
        rect.y + shadow.offset,
        rect.w,
        rect.h,
        r,
        [0.0, 0.0, 0.0, shadow.alpha],
        shadow.softness,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
    let b = theme.tokens.border_default;
    add_px_rounded_rect_sdf(
        rect.x - 1.0,
        rect.y - 1.0,
        rect.w + 2.0,
        rect.h + 2.0,
        r + 1.0,
        [b[0], b[1], b[2], 1.0],
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
    add_px_rounded_rect_sdf_with_acrylic(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        r,
        theme.tokens.surface_3,
        theme.sw,
        theme.sh,
        acrylic_mix,
        sink.bg_verts,
        sink.bg_idx,
    );

    // Caption: a tooltip is secondary metadata about the control it hangs off,
    // which is exactly what the ramp's smallest step is for.
    let style = theme.metrics.type_ramp.caption;
    add_run_verts(
        text,
        &style,
        rect.x + PAD_X_CELLS * theme.cell_w,
        rect.y + PAD_Y_CELLS * theme.cell_h,
        theme.tokens.text_primary,
        theme.sw,
        theme.sh,
        font,
        atlas,
        queue,
        sink.text_verts,
        sink.text_idx,
    );
}

/// Measure a tooltip's text at the step [`draw_tooltip`] draws it in.
///
/// The one place callers should get `text_w` / `line_h` for [`place_tooltip`];
/// going through here is what keeps the box and the glyphs agreeing.
pub(crate) fn measure_tooltip(
    text: &str,
    metrics: &nexterm_config::MetricTokens,
    font: &mut FontManager,
) -> (f32, f32) {
    let style = metrics.type_ramp.caption;
    let (_size, line_h, _bold) = font.chrome_metrics(&style);
    (measure_run(text, &style, font), line_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_W: f32 = 10.0;
    const CELL_H: f32 = 20.0;
    const SURFACE_W: f32 = 800.0;
    const SURFACE_H: f32 = 600.0;

    /// Stand-in for the measured text width. The tests care about placement,
    /// not about typography, so they feed the width a `FontManager` would
    /// have measured — one cell per column, which is what the pre-P4b
    /// implementation computed internally.
    fn place(anchor: WidgetRect, text: &str) -> WidgetRect {
        let text_w = crate::vertex_util::visual_width(text) as f32 * CELL_W;
        place_tooltip(anchor, text_w, CELL_H, CELL_W, CELL_H, SURFACE_W, SURFACE_H)
    }

    #[test]
    fn tooltip_sits_below_the_anchor_by_default() {
        let anchor = WidgetRect::new(100.0, 100.0, 200.0, 24.0);
        let t = place(anchor, "hint");
        assert!(t.y > anchor.y + anchor.h);
    }

    #[test]
    fn tooltip_flips_above_when_it_would_overflow_the_bottom() {
        let anchor = WidgetRect::new(100.0, SURFACE_H - 30.0, 200.0, 24.0);
        let t = place(anchor, "hint");
        assert!(t.y + t.h <= anchor.y, "expected a flip above the anchor");
        assert!(t.y >= 0.0);
    }

    #[test]
    fn tooltip_is_centred_on_the_anchor() {
        let anchor = WidgetRect::new(300.0, 100.0, 200.0, 24.0);
        let t = place(anchor, "hint");
        let (ax, _) = anchor.center();
        let (tx, _) = t.center();
        assert!((ax - tx).abs() < 0.01);
    }

    #[test]
    fn tooltip_is_clamped_to_the_left_and_right_edges() {
        let left = place(WidgetRect::new(0.0, 100.0, 20.0, 24.0), "a long hint text");
        assert!(left.x >= 0.0);
        let right = place(
            WidgetRect::new(SURFACE_W - 20.0, 100.0, 20.0, 24.0),
            "a long hint text",
        );
        assert!(right.x + right.w <= SURFACE_W + 0.01);
    }

    #[test]
    fn tooltip_width_follows_the_text() {
        let short = place(WidgetRect::new(300.0, 100.0, 100.0, 24.0), "a");
        let long = place(WidgetRect::new(300.0, 100.0, 100.0, 24.0), "aaaaaaaaaa");
        assert!(long.w > short.w);
    }

    #[test]
    fn tooltip_width_accounts_for_full_width_characters() {
        // CJK glyphs are two cells wide; the box must grow accordingly.
        let ascii = place(WidgetRect::new(300.0, 100.0, 100.0, 24.0), "ab");
        let cjk = place(WidgetRect::new(300.0, 100.0, 100.0, 24.0), "あい");
        assert!(cjk.w > ascii.w);
    }

    #[test]
    fn a_tooltip_taller_than_the_surface_stays_on_screen() {
        let tiny_surface_h = 10.0;
        let t = place_tooltip(
            WidgetRect::new(100.0, 0.0, 100.0, 8.0),
            4.0 * CELL_W,
            CELL_H,
            CELL_W,
            CELL_H,
            SURFACE_W,
            tiny_surface_h,
        );
        assert!(t.y >= 0.0);
    }
}
