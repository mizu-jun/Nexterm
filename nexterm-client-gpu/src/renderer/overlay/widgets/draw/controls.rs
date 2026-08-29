//! Discrete controls: pill toggle, chevron cycler, colour swatch, push button.

use crate::font::FontManager;
use crate::glyph_atlas::GlyphAtlas;
use crate::vertex_util::{add_icon_verts, add_px_rounded_rect_sdf, icon_size_for_slot};

use super::super::super::settings::row::{MIN_TEXT_CONTRAST, ensure_readable};
use super::super::spec::WidgetSpec;
use super::{
    FOCUS_RING_PX, WidgetSink, WidgetTheme, draw_focus_ring, draw_row_run, draw_row_run_centred,
    row_style, text_baseline,
};

/// Toggle track height as a fraction of the cell height.
const TOGGLE_TRACK_H: f32 = 0.95;
/// Toggle track width as a multiple of its own height (Fluent's 40:20 pill).
const TOGGLE_ASPECT: f32 = 2.0;
/// Toggle thumb diameter as a fraction of the track height.
const TOGGLE_THUMB: f32 = 0.55;

/// Fluent pill switch: a rounded track with a circular thumb that sits left
/// when off and right when on.
pub(super) fn draw_toggle(
    spec: &WidgetSpec,
    on: bool,
    theme: &WidgetTheme<'_>,
    sink: &mut WidgetSink<'_>,
) {
    let track_h = theme.cell_h * TOGGLE_TRACK_H;
    let track_w = track_h * TOGGLE_ASPECT;
    let track_x = spec.control_rect.x;
    let track_y = spec.control_rect.y + (spec.control_rect.h - track_h) * 0.5;
    let radius = track_h * 0.5;

    let (track_color, thumb_color) = if !spec.enabled() {
        (theme.tokens.surface_3, theme.tokens.text_muted)
    } else if on {
        (theme.tokens.accent_primary, theme.tokens.text_on_accent)
    } else {
        (theme.tokens.surface_3, theme.tokens.text_secondary)
    };

    // Off state gets a visible outline so an empty track never disappears
    // into the panel surface.
    if !on {
        let b = theme.tokens.border_default;
        add_px_rounded_rect_sdf(
            track_x - 1.0,
            track_y - 1.0,
            track_w + 2.0,
            track_h + 2.0,
            radius + 1.0,
            [b[0], b[1], b[2], 1.0],
            theme.sw,
            theme.sh,
            sink.bg_verts,
            sink.bg_idx,
        );
    }
    add_px_rounded_rect_sdf(
        track_x,
        track_y,
        track_w,
        track_h,
        radius,
        track_color,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );

    let thumb_d = track_h * TOGGLE_THUMB;
    let inset = (track_h - thumb_d) * 0.5;
    let thumb_x = if on {
        track_x + track_w - thumb_d - inset
    } else {
        track_x + inset
    };
    add_px_rounded_rect_sdf(
        thumb_x,
        track_y + inset,
        thumb_d,
        thumb_d,
        thumb_d * 0.5,
        thumb_color,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
}

/// Chevron cycler: `‹ value ›`. The chevrons brighten with focus so the
/// ←/→ affordance is discoverable without a hint string in the value.
pub(super) fn draw_cycle(
    spec: &WidgetSpec,
    value: &str,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    let chevron_color = if !spec.enabled() {
        theme.tokens.text_muted
    } else if spec.focused() {
        theme.tokens.accent_primary
    } else {
        theme.tokens.text_muted
    };
    let chevron_color = ensure_readable(chevron_color, theme.tokens.surface_2, MIN_TEXT_CONTRAST);
    let value_color = ensure_readable(
        if !spec.enabled() {
            theme.tokens.text_muted
        } else if spec.desc.invalid {
            theme.tokens.semantic_error
        } else {
            theme.tokens.text_primary
        },
        theme.tokens.surface_2,
        MIN_TEXT_CONTRAST,
    );
    let y = text_baseline(spec.rect, theme);
    let right_x = spec.control_rect.x + spec.control_rect.w - theme.cell_w;

    // UI/UX v3 P4a: the two chevrons come from the bundled icon font, each
    // centred in the one-cell slot the `‹` / `›` glyphs occupied. The slots
    // themselves are unchanged, so `hit_test` still resolves the same columns.
    //
    // The value sits between the two chevrons, each one cell wide plus a gap.
    // It is drawn first so `put` — which borrows `font` and `atlas` — is done
    // with them before the icons need the same borrows.
    let value_x = spec.control_rect.x + theme.cell_w * 2.0;
    let value_w = (right_x - value_x - theme.cell_w).max(0.0);
    let style = row_style(theme, spec.focused());
    draw_row_run(
        value,
        &style,
        value_x,
        value_w,
        spec.rect,
        value_color,
        theme,
        font,
        atlas,
        queue,
        sink,
    );

    let chevron_size = icon_size_for_slot(font.icon_px(16.0), theme.cell_w, theme.cell_h, 0.1);
    for (icon, x) in [
        (crate::icons::CHEVRON_LEFT, spec.control_rect.x),
        (crate::icons::CHEVRON_RIGHT, right_x),
    ] {
        add_icon_verts(
            icon,
            x,
            y,
            theme.cell_w,
            theme.cell_h,
            chevron_size,
            chevron_color,
            theme.sw,
            theme.sh,
            font,
            atlas,
            queue,
            sink.text_verts,
            sink.text_idx,
        );
    }
}

/// A colour chip, ringed in the accent colour when it is the active choice
/// and in the focus colours when the keyboard is on it.
pub(super) fn draw_swatch(
    spec: &WidgetSpec,
    color: [f32; 4],
    selected: bool,
    theme: &WidgetTheme<'_>,
    sink: &mut WidgetSink<'_>,
) {
    let r = theme.metrics.radius.control;
    if spec.focused() {
        draw_focus_ring(spec.control_rect, theme, sink);
    } else if selected {
        let t = FOCUS_RING_PX;
        add_px_rounded_rect_sdf(
            spec.control_rect.x - t,
            spec.control_rect.y - t,
            spec.control_rect.w + t * 2.0,
            spec.control_rect.h + t * 2.0,
            r + t,
            theme.tokens.accent_primary,
            theme.sw,
            theme.sh,
            sink.bg_verts,
            sink.bg_idx,
        );
    }
    add_px_rounded_rect_sdf(
        spec.control_rect.x,
        spec.control_rect.y,
        spec.control_rect.w,
        spec.control_rect.h,
        r,
        color,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
}

/// Push button: a filled rounded rect with its label centred inside.
///
/// The label lives on the button rather than in the row's label column, so
/// this is one of the kinds `draw_widget` hands the whole row to.
///
/// A destructive button (Delete) is outlined and lettered in `semantic_error`
/// rather than filled with it: a solid red block next to a neutral Add button
/// reads as an error state rather than as an available action.
pub(super) fn draw_button(
    spec: &WidgetSpec,
    destructive: bool,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    draw_button_face(spec, destructive, theme, sink);

    let color = ensure_readable(
        if !spec.enabled() {
            theme.tokens.text_muted
        } else if destructive {
            theme.tokens.semantic_error
        } else {
            theme.tokens.text_primary
        },
        theme.tokens.surface_3,
        MIN_TEXT_CONTRAST,
    );
    let style = row_style(theme, spec.focused());
    draw_row_run_centred(
        &spec.desc.label,
        &style,
        spec.control_rect,
        color,
        theme,
        font,
        atlas,
        queue,
        sink,
    );
}

/// Focus ring, border and face of a button — everything but its text.
///
/// Split out from [`draw_button`] for the same reason as `draw_slider_track`:
/// it needs no font state, which keeps the geometry unit-testable.
fn draw_button_face(
    spec: &WidgetSpec,
    destructive: bool,
    theme: &WidgetTheme<'_>,
    sink: &mut WidgetSink<'_>,
) {
    if spec.focused() && spec.enabled() {
        draw_focus_ring(spec.control_rect, theme, sink);
    }

    let r = theme.metrics.radius.control;
    let border = if !spec.enabled() {
        theme.tokens.border_subtle
    } else if destructive {
        theme.tokens.semantic_error
    } else {
        theme.tokens.border_default
    };
    add_px_rounded_rect_sdf(
        spec.control_rect.x - 1.0,
        spec.control_rect.y - 1.0,
        spec.control_rect.w + 2.0,
        spec.control_rect.h + 2.0,
        r + 1.0,
        [border[0], border[1], border[2], 1.0],
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
    add_px_rounded_rect_sdf(
        spec.control_rect.x,
        spec.control_rect.y,
        spec.control_rect.w,
        spec.control_rect.h,
        r,
        theme.tokens.surface_3,
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
    fn toggle_off_adds_an_outline_that_on_does_not_need() {
        let off = spec_at(WidgetKind::Toggle { on: false });
        let on = spec_at(WidgetKind::Toggle { on: true });
        // off: outline + track + thumb, on: track + thumb.
        assert_eq!(bg_quads(|t, s| draw_toggle(&off, false, t, s)), 3);
        assert_eq!(bg_quads(|t, s| draw_toggle(&on, true, t, s)), 2);
    }

    #[test]
    fn a_selected_swatch_gains_a_ring_and_a_focused_one_gains_two() {
        let plain = spec_at(WidgetKind::Swatch {
            color: [1.0; 4],
            selected: false,
        });
        assert_eq!(
            bg_quads(|t, s| draw_swatch(&plain, [1.0; 4], false, t, s)),
            1
        );
        assert_eq!(
            bg_quads(|t, s| draw_swatch(&plain, [1.0; 4], true, t, s)),
            2
        );
        let focused = focused(plain.clone());
        assert_eq!(
            bg_quads(|t, s| draw_swatch(&focused, [1.0; 4], true, t, s)),
            3,
            "focus ring (2) + chip (1)"
        );
    }

    #[test]
    fn a_button_is_a_border_plus_a_face() {
        let spec = spec_at(WidgetKind::Button { destructive: false });
        assert_eq!(
            bg_quads(|t, s| {
                draw_button_face(&spec, false, t, s);
            }),
            2
        );
    }

    #[test]
    fn a_focused_button_adds_the_two_ring_quads() {
        let spec = focused(spec_at(WidgetKind::Button { destructive: true }));
        assert_eq!(
            bg_quads(|t, s| {
                draw_button_face(&spec, true, t, s);
            }),
            4,
            "focus ring (2) + border + face"
        );
    }

    #[test]
    fn a_disabled_button_paints_no_focus_ring() {
        // A Delete button with nothing to delete must not look actionable
        // even while the keyboard is parked on it.
        let mut spec = focused(spec_at(WidgetKind::Button { destructive: true }));
        spec.desc.enabled = false;
        assert_eq!(
            bg_quads(|t, s| {
                draw_button_face(&spec, true, t, s);
            }),
            2
        );
    }
}
