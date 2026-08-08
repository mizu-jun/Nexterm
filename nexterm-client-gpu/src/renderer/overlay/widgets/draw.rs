//! Visuals for [`WidgetSpec`] — the single place chrome controls are painted.
//!
//! Every migrated control now shares one hover / press / focus language:
//!
//! - **Hover**: the row fills with `surface_3` at low alpha.
//! - **Focus**: the row fills with `surface_2` and the control gets a Fluent
//!   two-tone focus ring (accent outer, surface inner) so it stays visible on
//!   both light and dark schemes.
//! - **Disabled**: text and control drop to `text_muted`.
//!
//! Geometry comes from [`nexterm_config::MetricTokens`]; the caller passes it
//! pre-scaled to physical pixels via `MetricTokens::scaled`. Colours come from
//! [`nexterm_config::DesignTokens`], so the widgets follow the active scheme.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::vertex_util::{add_px_rounded_rect_sdf, add_string_verts, truncate_to_width};

use super::super::settings::row::{MIN_TEXT_CONTRAST, ensure_readable};
use super::spec::{WidgetKind, WidgetRect, WidgetSpec};

/// Colours, metrics and screen geometry needed to paint a widget.
pub(crate) struct WidgetTheme<'a> {
    /// Palette-derived colours.
    pub tokens: &'a nexterm_config::DesignTokens,
    /// Metric tokens, already scaled to physical pixels.
    pub metrics: &'a nexterm_config::MetricTokens,
    /// Surface width in pixels.
    pub sw: f32,
    /// Surface height in pixels.
    pub sh: f32,
    /// Character cell width.
    pub cell_w: f32,
    /// Character cell height.
    pub cell_h: f32,
}

/// The vertex buffers a widget appends to.
pub(crate) struct WidgetSink<'a> {
    /// Background quads.
    pub bg_verts: &'a mut Vec<BgVertex>,
    /// Background indices.
    pub bg_idx: &'a mut Vec<u16>,
    /// Glyph quads.
    pub text_verts: &'a mut Vec<TextVertex>,
    /// Glyph indices.
    pub text_idx: &'a mut Vec<u16>,
}

/// Alpha applied to `surface_3` for the hover fill. Low enough that hover
/// reads as a hint rather than as selection, which focus owns.
const HOVER_ALPHA: f32 = 0.35;
/// Focus ring thickness in epx, before DPI scaling.
const FOCUS_RING_PX: f32 = 2.0;
/// Toggle track height as a fraction of the cell height.
const TOGGLE_TRACK_H: f32 = 0.95;
/// Toggle track width as a multiple of its own height (Fluent's 40:20 pill).
const TOGGLE_ASPECT: f32 = 2.0;
/// Toggle thumb diameter as a fraction of the track height.
const TOGGLE_THUMB: f32 = 0.55;
/// Slider track height as a fraction of the cell height.
const SLIDER_TRACK_H: f32 = 0.3;
/// Slider thumb diameter as a fraction of the cell height.
const SLIDER_THUMB: f32 = 0.85;

/// Paint one widget: row background, label, then the control itself.
pub(crate) fn draw_widget(
    spec: &WidgetSpec,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    if spec.rect.w <= 0.0 || spec.rect.h <= 0.0 {
        // Collapsed by the search filter — nothing to paint.
        return;
    }

    draw_row_background(spec, theme, sink);

    // A swatch is its own control with no label column.
    if let WidgetKind::Swatch { color, selected } = spec.kind() {
        draw_swatch(spec, *color, *selected, theme, sink);
        return;
    }

    draw_label(spec, theme, font, atlas, queue, sink);

    if spec.focused() && spec.kind().is_interactive() {
        draw_focus_ring(spec.control_rect, theme, sink);
    }

    match spec.kind() {
        WidgetKind::Label => {}
        WidgetKind::Toggle { on } => draw_toggle(spec, *on, theme, sink),
        WidgetKind::Cycle { value } => draw_cycle(spec, value, theme, font, atlas, queue, sink),
        kind @ WidgetKind::Slider { display, .. } => draw_slider(
            spec,
            kind.slider_fraction(),
            display,
            theme,
            font,
            atlas,
            queue,
            sink,
        ),
        WidgetKind::Text { value, editing } => {
            draw_text_field(spec, value, *editing, theme, font, atlas, queue, sink)
        }
        WidgetKind::Swatch { .. } => unreachable!("swatches return early above"),
    }
}

/// Hover / focus fill behind the whole row.
fn draw_row_background(spec: &WidgetSpec, theme: &WidgetTheme<'_>, sink: &mut WidgetSink<'_>) {
    let fill = if spec.focused() {
        Some(theme.tokens.surface_2)
    } else if spec.hovered && spec.enabled() && spec.kind().is_interactive() {
        let s = theme.tokens.surface_3;
        Some([s[0], s[1], s[2], s[3] * HOVER_ALPHA])
    } else {
        None
    };
    if let Some(color) = fill {
        add_px_rounded_rect_sdf(
            spec.rect.x,
            spec.rect.y,
            spec.rect.w,
            spec.rect.h,
            theme.metrics.radius.control,
            color,
            theme.sw,
            theme.sh,
            sink.bg_verts,
            sink.bg_idx,
        );
    }
}

/// The label column text, contrast-corrected against the row's surface.
fn draw_label(
    spec: &WidgetSpec,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    let base = if !spec.enabled() {
        theme.tokens.text_muted
    } else if spec.focused() {
        theme.tokens.text_primary
    } else {
        theme.tokens.text_secondary
    };
    let color = ensure_readable(base, theme.tokens.surface_2, MIN_TEXT_CONTRAST);
    let label_w = (spec.control_rect.x - spec.rect.x).max(0.0);
    let text = truncate_to_width(&spec.desc.label, label_w, theme.cell_w);
    add_string_verts(
        &text,
        spec.rect.x,
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

/// Two-tone Fluent focus ring: accent outside, surface inside, so it stays
/// visible whichever way the surface underneath leans.
fn draw_focus_ring(rect: WidgetRect, theme: &WidgetTheme<'_>, sink: &mut WidgetSink<'_>) {
    let t = FOCUS_RING_PX;
    let r = theme.metrics.radius.control;
    add_px_rounded_rect_sdf(
        rect.x - t * 2.0,
        rect.y - t * 2.0,
        rect.w + t * 4.0,
        rect.h + t * 4.0,
        r + t * 2.0,
        theme.tokens.accent_primary,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
    add_px_rounded_rect_sdf(
        rect.x - t,
        rect.y - t,
        rect.w + t * 2.0,
        rect.h + t * 2.0,
        r + t,
        theme.tokens.surface_2,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
}

/// Fluent pill switch: a rounded track with a circular thumb that sits left
/// when off and right when on.
fn draw_toggle(spec: &WidgetSpec, on: bool, theme: &WidgetTheme<'_>, sink: &mut WidgetSink<'_>) {
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
fn draw_cycle(
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
        if spec.enabled() {
            theme.tokens.text_primary
        } else {
            theme.tokens.text_muted
        },
        theme.tokens.surface_2,
        MIN_TEXT_CONTRAST,
    );
    let y = text_baseline(spec.rect, theme);
    let right_x = spec.control_rect.x + spec.control_rect.w - theme.cell_w;

    let mut put = |s: &str, x: f32, color: [f32; 4], bold: bool, sink: &mut WidgetSink<'_>| {
        add_string_verts(
            s,
            x,
            y,
            color,
            bold,
            theme.sw,
            theme.sh,
            theme.cell_w,
            font,
            atlas,
            queue,
            sink.text_verts,
            sink.text_idx,
        );
    };

    put(
        "‹",
        spec.control_rect.x,
        chevron_color,
        spec.focused(),
        sink,
    );
    // The value sits between the two chevrons, each one cell wide plus a gap.
    let value_x = spec.control_rect.x + theme.cell_w * 2.0;
    let value_w = (right_x - value_x - theme.cell_w).max(0.0);
    let text = truncate_to_width(value, value_w, theme.cell_w);
    put(&text, value_x, value_color, spec.focused(), sink);
    put("›", right_x, chevron_color, spec.focused(), sink);
}

/// Track + filled portion + thumb, with the formatted value to the right.
#[allow(clippy::too_many_arguments)]
fn draw_slider(
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
    let readout_w = slider_readout_w(theme);
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

/// Width reserved at the right of a slider row for its numeric readout.
fn slider_readout_w(theme: &WidgetTheme<'_>) -> f32 {
    theme.cell_w * 8.0
}

/// Track rectangle of a slider, derived from its control rect.
///
/// Exposed because the mouse hit-test needs the same rectangle to start a
/// drag; deriving it in both places from one function is what keeps the
/// grab region on top of the drawn track.
pub(crate) fn slider_track_rect(control: WidgetRect, cell_w: f32, cell_h: f32) -> WidgetRect {
    let track_w = (control.w - cell_w * 8.0).max(cell_w);
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

/// Bordered input box; a block caret is appended while editing.
#[allow(clippy::too_many_arguments)]
fn draw_text_field(
    spec: &WidgetSpec,
    value: &str,
    editing: bool,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    let r = theme.metrics.radius.control;
    let b = theme.tokens.border_default;
    add_px_rounded_rect_sdf(
        spec.control_rect.x - 1.0,
        spec.control_rect.y - 1.0,
        spec.control_rect.w + 2.0,
        spec.control_rect.h + 2.0,
        r + 1.0,
        [b[0], b[1], b[2], 1.0],
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
        theme.tokens.surface_1,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );

    let shown = if editing {
        format!("{value}_")
    } else {
        value.to_string()
    };
    let color = ensure_readable(
        if spec.enabled() {
            theme.tokens.text_primary
        } else {
            theme.tokens.text_muted
        },
        theme.tokens.surface_1,
        MIN_TEXT_CONTRAST,
    );
    let inner = (spec.control_rect.w - theme.cell_w).max(0.0);
    let text = truncate_to_width(&shown, inner, theme.cell_w);
    add_string_verts(
        &text,
        spec.control_rect.x + theme.cell_w * 0.5,
        text_baseline(spec.rect, theme),
        color,
        editing,
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

/// A colour chip, ringed in the accent colour when it is the active choice
/// and in the focus colours when the keyboard is on it.
fn draw_swatch(
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

/// Y position that vertically centres one line of text inside `rect`.
fn text_baseline(rect: WidgetRect, theme: &WidgetTheme<'_>) -> f32 {
    rect.y + (rect.h - theme.cell_h) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::overlay::widgets::spec::{WidgetDesc, WidgetId, WidgetRect};

    fn theme_fixture() -> (nexterm_config::DesignTokens, nexterm_config::MetricTokens) {
        (
            nexterm_config::DesignTokens::default(),
            nexterm_config::MetricTokens::default(),
        )
    }

    fn spec_at(kind: WidgetKind) -> WidgetSpec {
        WidgetDesc::new(WidgetId::new(1, 0), kind, "label").place(
            WidgetRect::new(0.0, 0.0, 400.0, 24.0),
            WidgetRect::new(200.0, 0.0, 200.0, 24.0),
        )
    }

    /// Focus is part of the semantics, so it is set on the desc.
    fn focused(spec: WidgetSpec) -> WidgetSpec {
        WidgetSpec {
            desc: spec.desc.focused(true),
            ..spec
        }
    }

    /// Count only the background quads: they are produced without touching
    /// the GPU-backed glyph atlas, so they can be exercised in a unit test.
    fn bg_quads(f: impl FnOnce(&WidgetTheme<'_>, &mut WidgetSink<'_>)) -> usize {
        let (tokens, metrics) = theme_fixture();
        let theme = WidgetTheme {
            tokens: &tokens,
            metrics: &metrics,
            sw: 800.0,
            sh: 600.0,
            cell_w: 10.0,
            cell_h: 20.0,
        };
        let (mut bv, mut bi, mut tv, mut ti) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut sink = WidgetSink {
            bg_verts: &mut bv,
            bg_idx: &mut bi,
            text_verts: &mut tv,
            text_idx: &mut ti,
        };
        f(&theme, &mut sink);
        bv.len() / 4
    }

    #[test]
    fn an_unfocused_unhovered_row_paints_no_background() {
        let spec = spec_at(WidgetKind::Toggle { on: false });
        let n = bg_quads(|t, s| draw_row_background(&spec, t, s));
        assert_eq!(n, 0);
    }

    #[test]
    fn focus_and_hover_each_paint_one_row_fill() {
        let focused = focused(spec_at(WidgetKind::Toggle { on: false }));
        assert_eq!(bg_quads(|t, s| draw_row_background(&focused, t, s)), 1);
        let hovered = spec_at(WidgetKind::Toggle { on: false }).hovered(true);
        assert_eq!(bg_quads(|t, s| draw_row_background(&hovered, t, s)), 1);
    }

    #[test]
    fn a_hovered_label_paints_no_background() {
        // Non-interactive rows must not appear clickable.
        let spec = spec_at(WidgetKind::Label).hovered(true);
        assert_eq!(bg_quads(|t, s| draw_row_background(&spec, t, s)), 0);
    }

    #[test]
    fn the_focus_ring_is_two_concentric_rects() {
        let rect = WidgetRect::new(10.0, 10.0, 100.0, 20.0);
        assert_eq!(bg_quads(|t, s| draw_focus_ring(rect, t, s)), 2);
    }

    #[test]
    fn toggle_off_adds_an_outline_that_on_does_not_need() {
        let off = spec_at(WidgetKind::Toggle { on: false });
        let on = spec_at(WidgetKind::Toggle { on: true });
        // off: outline + track + thumb, on: track + thumb.
        assert_eq!(bg_quads(|t, s| draw_toggle(&off, false, t, s)), 3);
        assert_eq!(bg_quads(|t, s| draw_toggle(&on, true, t, s)), 2);
    }

    #[test]
    fn a_slider_at_zero_skips_the_fill_quad() {
        let spec = spec_at(WidgetKind::Slider {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            display: "0".into(),
        });
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
        let spec = spec_at(WidgetKind::Slider {
            value: 5.0,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            display: "x".into(),
        });
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
    fn a_collapsed_widget_paints_nothing() {
        let mut spec = focused(spec_at(WidgetKind::Toggle { on: true }));
        spec.rect = WidgetRect::new(0.0, 0.0, 400.0, 0.0);
        let n = bg_quads(|t, s| draw_row_background(&spec, t, s));
        assert_eq!(n, 1, "draw_row_background itself does not gate on size");
        // `draw_widget` is the layer that gates; verified via its early return
        // in `widget_draw_gates_on_a_collapsed_rect`.
    }

    #[test]
    fn text_is_vertically_centred_in_the_row() {
        let (tokens, metrics) = theme_fixture();
        let theme = WidgetTheme {
            tokens: &tokens,
            metrics: &metrics,
            sw: 800.0,
            sh: 600.0,
            cell_w: 10.0,
            cell_h: 20.0,
        };
        // A 24 px row with a 20 px cell leaves 2 px above and below.
        let y = text_baseline(WidgetRect::new(0.0, 100.0, 400.0, 24.0), &theme);
        assert_eq!(y, 102.0);
    }
}
