//! Continuous control: a track with a thumb and a numeric readout.

use crate::font::FontManager;
use crate::glyph_atlas::GlyphAtlas;
use crate::vertex_util::{add_px_rounded_rect_sdf, add_string_verts, truncate_to_width};

use super::super::super::settings::row::{MIN_TEXT_CONTRAST, ensure_readable};
use super::super::spec::{WidgetRect, WidgetSpec};
use super::{WidgetSink, WidgetTheme, text_baseline};

/// Slider track height as a fraction of the cell height.
const SLIDER_TRACK_H: f32 = 0.3;
/// Slider thumb diameter as a fraction of the cell height.
const SLIDER_THUMB: f32 = 0.85;
/// Width reserved at the right of a slider row for its numeric readout,
/// in cells. Shared by the readout placement and the track width.
const READOUT_CELLS: f32 = 8.0;

/// Track + filled portion + thumb, with the formatted value to the right.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_slider(
    spec: &WidgetSpec,
    fraction: f32,
    display: &str,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    let track_w = draw_slider_track(spec, fraction, theme, sink);
    let readout_w = theme.cell_w * READOUT_CELLS;
    let color = ensure_readable(
        theme.tokens.text_primary,
        theme.tokens.surface_2,
        MIN_TEXT_CONTRAST,
    );
    let text = truncate_to_width(display, readout_w, theme.cell_w);
    add_string_verts(
        &text,
        spec.control_rect.x + track_w + theme.cell_w,
        text_baseline(spec.rect, theme),
        color,
        spec.focused(),
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

/// Track rectangle of a slider, derived from its control rect.
///
/// Exposed because the mouse hit-test needs the same rectangle to start a
/// drag; deriving it in both places from one function is what keeps the
/// grab region on top of the drawn track.
pub(crate) fn slider_track_rect(control: WidgetRect, cell_w: f32, cell_h: f32) -> WidgetRect {
    let track_w = (control.w - cell_w * READOUT_CELLS).max(cell_w);
    let track_h = cell_h * SLIDER_TRACK_H;
    WidgetRect::new(
        control.x,
        control.y + (control.h - track_h) * 0.5,
        track_w,
        track_h,
    )
}

/// Draw the slider's rail, filled portion and thumb. Returns the track width
/// so the caller can place the readout after it.
///
/// Split out from [`draw_slider`] because it needs no font state, which keeps
/// the geometry unit-testable.
fn draw_slider_track(
    spec: &WidgetSpec,
    fraction: f32,
    theme: &WidgetTheme<'_>,
    sink: &mut WidgetSink<'_>,
) -> f32 {
    let f = fraction.clamp(0.0, 1.0);
    let track = slider_track_rect(spec.control_rect, theme.cell_w, theme.cell_h);
    let (track_x, track_y, track_w, track_h) = (track.x, track.y, track.w, track.h);
    let radius = track_h * 0.5;

    let (rail, fill, thumb) = if spec.enabled() {
        (
            theme.tokens.surface_3,
            theme.tokens.accent_primary,
            theme.tokens.text_primary,
        )
    } else {
        (
            theme.tokens.surface_3,
            theme.tokens.text_muted,
            theme.tokens.text_muted,
        )
    };

    add_px_rounded_rect_sdf(
        track_x,
        track_y,
        track_w,
        track_h,
        radius,
        rail,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
    if f > 0.0 {
        add_px_rounded_rect_sdf(
            track_x,
            track_y,
            track_w * f,
            track_h,
            radius,
            fill,
            theme.sw,
            theme.sh,
            sink.bg_verts,
            sink.bg_idx,
        );
    }

    let thumb_d = theme.cell_h * SLIDER_THUMB;
    let thumb_x =
        (track_x + track_w * f - thumb_d * 0.5).clamp(track_x, track_x + track_w - thumb_d);
    add_px_rounded_rect_sdf(
        thumb_x,
        spec.control_rect.y + (spec.control_rect.h - thumb_d) * 0.5,
        thumb_d,
        thumb_d,
        thumb_d * 0.5,
        thumb,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );

    track_w
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::renderer::overlay::widgets::spec::WidgetKind;

    fn slider_spec(value: f32) -> WidgetSpec {
        spec_at(WidgetKind::Slider {
            value,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            display: "x".into(),
        })
    }

    #[test]
    fn a_slider_at_zero_skips_the_fill_quad() {
        let spec = slider_spec(0.0);
        // rail + thumb, with no filled portion.
        assert_eq!(
            bg_quads(|t, s| {
                draw_slider_track(&spec, 0.0, t, s);
            }),
            2
        );
        // rail + fill + thumb.
        assert_eq!(
            bg_quads(|t, s| {
                draw_slider_track(&spec, 0.5, t, s);
            }),
            3
        );
    }

    #[test]
    fn slider_fraction_is_clamped() {
        let spec = slider_spec(5.0);
        // An out-of-range fraction must not panic or drop the thumb.
        assert_eq!(
            bg_quads(|t, s| {
                draw_slider_track(&spec, 5.0, t, s);
            }),
            3
        );
        assert_eq!(
            bg_quads(|t, s| {
                draw_slider_track(&spec, -1.0, t, s);
            }),
            2
        );
    }

    #[test]
    fn the_track_leaves_room_for_the_readout() {
        // The readout width is what the drag hit-region is measured against,
        // so the track must stop short of the control's right edge.
        let control = WidgetRect::new(100.0, 0.0, 200.0, 24.0);
        let track = slider_track_rect(control, 10.0, 20.0);
        assert_eq!(track.x, 100.0);
        assert_eq!(track.w, 200.0 - 10.0 * READOUT_CELLS);
    }

    #[test]
    fn a_track_narrower_than_its_readout_stays_positive() {
        // Otherwise a collapsed row would produce a zero-width grab region
        // and the thumb clamp would invert.
        let track = slider_track_rect(WidgetRect::new(0.0, 0.0, 10.0, 24.0), 10.0, 20.0);
        assert!(track.w > 0.0);
    }
}
