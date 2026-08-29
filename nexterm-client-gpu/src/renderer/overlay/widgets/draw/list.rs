//! List entries: full-width rows whose label *is* their value.

use crate::font::FontManager;
use crate::glyph_atlas::GlyphAtlas;
use crate::vertex_util::add_px_rounded_rect_sdf;

use super::super::super::settings::row::{MIN_TEXT_CONTRAST, ensure_readable};
use super::super::spec::{WidgetRect, WidgetSpec};
use super::{FOCUS_RING_PX, WidgetSink, WidgetTheme, draw_focus_ring, draw_row_run, row_style};
use nexterm_config::SurfaceLevel;

/// Width of the selection bar, in cells.
const SELECTION_BAR_W: f32 = 0.25;
/// Fraction of the row height the selection bar spans.
const SELECTION_BAR_H: f32 = 0.7;
/// Gap between the leading edge and the label, in cells.
const LABEL_INSET: f32 = 0.8;

/// One list entry.
///
/// Selection and focus are separate states here, unlike in the row-shaped
/// controls: the keyboard can rest on an entry that is not the current
/// selection. Both use `surface_2` for their fill, so selection is told apart
/// by an accent bar on the leading edge rather than by the fill alone.
pub(super) fn draw_list_item(
    spec: &WidgetSpec,
    selected: bool,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    if spec.focused() {
        draw_focus_ring(focus_rect(spec.rect), theme, sink);
    }
    if selected {
        draw_selection_bar(spec.rect, theme, sink);
    }

    let base = if !spec.enabled() {
        theme.tokens.text_on(SurfaceLevel::S3).muted
    } else if spec.desc.search_match {
        theme.tokens.accent_primary
    } else if selected || spec.focused() {
        theme.tokens.text_on(SurfaceLevel::S3).primary
    } else {
        theme.tokens.text_on(SurfaceLevel::S3).secondary
    };
    let color = ensure_readable(base, theme.tokens.surface_2, MIN_TEXT_CONTRAST);

    let inset = theme.cell_w * LABEL_INSET;
    let label_w = (spec.rect.w - inset).max(0.0);
    let style = row_style(theme, selected);
    draw_row_run(
        &spec.desc.label,
        &style,
        spec.rect.x + inset,
        label_w,
        spec.rect,
        color,
        theme,
        font,
        atlas,
        queue,
        sink,
    );
}

/// Rectangle to hand [`draw_focus_ring`] so the ring lands *on* the row.
///
/// The ring is painted outside the rectangle it is given, and list rows sit
/// close together — passing the row itself would let the ring bleed over its
/// neighbours, so it is inset by the ring's own thickness first.
fn focus_rect(row: WidgetRect) -> WidgetRect {
    let inset = FOCUS_RING_PX * 2.0;
    WidgetRect::new(
        row.x + inset,
        row.y + inset,
        (row.w - inset * 2.0).max(0.0),
        (row.h - inset * 2.0).max(0.0),
    )
}

/// Accent bar marking the selected entry.
fn draw_selection_bar(row: WidgetRect, theme: &WidgetTheme<'_>, sink: &mut WidgetSink<'_>) {
    let w = theme.cell_w * SELECTION_BAR_W;
    let h = row.h * SELECTION_BAR_H;
    add_px_rounded_rect_sdf(
        row.x,
        row.y + (row.h - h) * 0.5,
        w,
        h,
        w * 0.5,
        theme.tokens.accent_primary,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::renderer::overlay::widgets::spec::WidgetKind;

    #[test]
    fn only_the_selected_entry_gets_a_bar() {
        assert_eq!(
            bg_quads(|t, s| draw_selection_bar(WidgetRect::new(0.0, 0.0, 400.0, 24.0), t, s)),
            1
        );
    }

    #[test]
    fn the_focus_ring_stays_inside_the_row() {
        // Otherwise the ring would overlap the neighbouring entries, which sit
        // only a fraction of a cell apart.
        let row = WidgetRect::new(10.0, 100.0, 400.0, 24.0);
        let inner = focus_rect(row);
        // `draw_focus_ring` expands by 2× the ring thickness on each side.
        let outer_x = inner.x - FOCUS_RING_PX * 2.0;
        let outer_y = inner.y - FOCUS_RING_PX * 2.0;
        assert_eq!(outer_x, row.x);
        assert_eq!(outer_y, row.y);
        assert_eq!(inner.w + FOCUS_RING_PX * 4.0, row.w);
        assert_eq!(inner.h + FOCUS_RING_PX * 4.0, row.h);
    }

    #[test]
    fn a_row_smaller_than_the_ring_does_not_invert() {
        let inner = focus_rect(WidgetRect::new(0.0, 0.0, 2.0, 2.0));
        assert_eq!(inner.w, 0.0);
        assert_eq!(inner.h, 0.0);
    }

    #[test]
    fn a_selected_entry_is_still_hit_testable() {
        // Guards the invariant `hit_test` relies on: list entries are
        // interactive regardless of selection state.
        assert!(WidgetKind::ListItem { selected: true }.is_interactive());
        assert!(WidgetKind::ListItem { selected: false }.is_interactive());
    }
}
