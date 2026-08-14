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
    add_px_rounded_rect_sdf, add_px_soft_shadow_sdf, add_string_verts, visual_width,
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
pub(crate) fn place_tooltip(
    anchor: WidgetRect,
    text: &str,
    cell_w: f32,
    cell_h: f32,
    surface_w: f32,
    surface_h: f32,
) -> WidgetRect {
    let text_w = visual_width(text) as f32 * cell_w;
    let w = text_w + PAD_X_CELLS * 2.0 * cell_w;
    let h = cell_h + PAD_Y_CELLS * 2.0 * cell_h;
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
pub(crate) fn draw_tooltip(
    rect: WidgetRect,
    text: &str,
    theme: &WidgetTheme<'_>,
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
    add_px_rounded_rect_sdf(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        r,
        theme.tokens.surface_3,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );

    add_string_verts(
        text,
        rect.x + PAD_X_CELLS * theme.cell_w,
        rect.y + PAD_Y_CELLS * theme.cell_h,
        theme.tokens.text_primary,
        false,
        theme.sw,
        theme.sh,
        theme.cell_w,
        font,
        atlas,
        queue,
        sink.text_verts,
        sink.text_idx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_W: f32 = 10.0;
    const CELL_H: f32 = 20.0;
    const SURFACE_W: f32 = 800.0;
    const SURFACE_H: f32 = 600.0;

    fn place(anchor: WidgetRect, text: &str) -> WidgetRect {
        place_tooltip(anchor, text, CELL_W, CELL_H, SURFACE_W, SURFACE_H)
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
            "hint",
            CELL_W,
            CELL_H,
            SURFACE_W,
            tiny_surface_h,
        );
        assert!(t.y >= 0.0);
    }
}
