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
//!
//! This module owns the dispatch and the chrome every control shares — row
//! fill, label column, focus ring, text baseline — and one submodule owns each
//! control family. The split is by family rather than by tab so that adding a
//! control kind touches one file.

mod controls;
mod list;
mod slider;
mod text;

pub(crate) use slider::slider_track_rect;

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::vertex_util::{
    add_px_rounded_rect_sdf, add_px_stroke_sdf, add_string_verts, truncate_to_width,
};

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
pub(super) const FOCUS_RING_PX: f32 = 2.0;

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

    // Kinds that own their whole row: they place their own text rather than
    // sitting in a control column beside a label, so they return early instead
    // of going through `draw_label`.
    match spec.kind() {
        WidgetKind::Swatch { color, selected } => {
            controls::draw_swatch(spec, *color, *selected, theme, sink);
            return;
        }
        WidgetKind::ListItem { selected } => {
            list::draw_list_item(spec, *selected, theme, font, atlas, queue, sink);
            return;
        }
        WidgetKind::Button { destructive } => {
            controls::draw_button(spec, *destructive, theme, font, atlas, queue, sink);
            return;
        }
        _ => {}
    }

    draw_label(spec, theme, font, atlas, queue, sink);

    if spec.focused() && spec.kind().is_interactive() {
        draw_focus_ring(spec.control_rect, theme, sink);
    }

    match spec.kind() {
        WidgetKind::Label => {}
        WidgetKind::Toggle { on } => controls::draw_toggle(spec, *on, theme, sink),
        WidgetKind::Cycle { value } => {
            controls::draw_cycle(spec, value, theme, font, atlas, queue, sink)
        }
        // A spin button shares the cycler's `< value >` visual language; the
        // difference is purely semantic (numeric role for readers).
        WidgetKind::SpinButton { display, .. } => {
            controls::draw_cycle(spec, display, theme, font, atlas, queue, sink)
        }
        kind @ WidgetKind::Slider { display, .. } => slider::draw_slider(
            spec,
            kind.slider_fraction(),
            display,
            theme,
            font,
            atlas,
            queue,
            sink,
        ),
        WidgetKind::Text {
            value,
            editing,
            caret,
        } => text::draw_text_field(
            spec, value, *editing, *caret, theme, font, atlas, queue, sink,
        ),
        WidgetKind::KeyCapture { value, recording } => {
            text::draw_key_capture(spec, value, *recording, theme, font, atlas, queue, sink)
        }
        WidgetKind::Swatch { .. } | WidgetKind::ListItem { .. } | WidgetKind::Button { .. } => {
            unreachable!("full-row kinds return early above")
        }
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
    } else if spec.desc.invalid {
        // A failed validation outranks the search accent: the row has to read
        // as broken even while a filter is highlighting it.
        theme.tokens.semantic_error
    } else if spec.desc.search_match {
        // While a search is active, matching rows pop out in the accent
        // colour so the eye lands on them first.
        theme.tokens.accent_primary
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
///
/// The ring is painted *outside* `rect`, so a caller that must keep it within
/// a row's bounds (a full-row control) passes an inset rectangle.
///
/// Both bands are real outlines (UI/UX v3 P2a's `stroke_width` attribute).
/// They used to be two stacked *fills*, where the inner rect repainted the
/// whole control area only to be covered again by the row fill and then by the
/// control — invisible while every surface is opaque, but a double blend the
/// moment one is not (which is where P2b's acrylic is heading). The two bands
/// now meet with a shared half-pixel of anti-aliasing instead of an opaque
/// butt joint, which is the one visible difference.
pub(super) fn draw_focus_ring(
    rect: WidgetRect,
    theme: &WidgetTheme<'_>,
    sink: &mut WidgetSink<'_>,
) {
    let t = FOCUS_RING_PX;
    let r = theme.metrics.radius.control;
    add_px_stroke_sdf(
        rect.x - t * 2.0,
        rect.y - t * 2.0,
        rect.w + t * 4.0,
        rect.h + t * 4.0,
        r + t * 2.0,
        theme.tokens.accent_primary,
        t,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
    add_px_stroke_sdf(
        rect.x - t,
        rect.y - t,
        rect.w + t * 2.0,
        rect.h + t * 2.0,
        r + t,
        theme.tokens.surface_2,
        t,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
}

/// Y position that vertically centres one line of text inside `rect`.
pub(super) fn text_baseline(rect: WidgetRect, theme: &WidgetTheme<'_>) -> f32 {
    rect.y + (rect.h - theme.cell_h) * 0.5
}

/// Fixtures shared by this module's tests and those of every submodule.
///
/// Only background quads are counted: they are produced without touching the
/// GPU-backed glyph atlas, so they can be exercised without a device.
#[cfg(test)]
pub(super) mod test_support {
    use super::*;
    use crate::renderer::overlay::widgets::spec::{WidgetDesc, WidgetId};

    pub(in crate::renderer::overlay::widgets::draw) fn spec_at(kind: WidgetKind) -> WidgetSpec {
        WidgetDesc::new(WidgetId::new(1, 0), kind, "label").place(
            WidgetRect::new(0.0, 0.0, 400.0, 24.0),
            WidgetRect::new(200.0, 0.0, 200.0, 24.0),
        )
    }

    /// Focus is part of the semantics, so it is set on the desc.
    pub(in crate::renderer::overlay::widgets::draw) fn focused(spec: WidgetSpec) -> WidgetSpec {
        WidgetSpec {
            desc: spec.desc.focused(true),
            ..spec
        }
    }

    /// Count the background quads a drawing closure emits.
    pub(in crate::renderer::overlay::widgets::draw) fn bg_quads(
        f: impl FnOnce(&WidgetTheme<'_>, &mut WidgetSink<'_>),
    ) -> usize {
        bg_vertices(f).len() / 4
    }

    /// Collect the background vertices a drawing closure emits, for the tests
    /// that assert on the SDF metadata rather than on the quad count alone.
    pub(in crate::renderer::overlay::widgets::draw) fn bg_vertices(
        f: impl FnOnce(&WidgetTheme<'_>, &mut WidgetSink<'_>),
    ) -> Vec<BgVertex> {
        let tokens = nexterm_config::DesignTokens::default();
        let metrics = nexterm_config::MetricTokens::default();
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
        bv
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

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
    fn the_focus_ring_is_two_outline_quads() {
        // UI/UX v3 P2a follow-up: the ring used to be two *filled* rects, the
        // inner one repainting the whole control area just to be covered again
        // by the row fill and the control. Both bands are now real outlines, so
        // nothing but the ring itself is painted.
        let rect = WidgetRect::new(10.0, 10.0, 100.0, 20.0);
        let verts = bg_vertices(|t, s| draw_focus_ring(rect, t, s));
        assert_eq!(verts.len() / 4, 2);
        for v in &verts {
            assert_eq!(v.stroke_width, FOCUS_RING_PX);
            // Strokes and shadows are separate shader branches; a ring must
            // not accidentally ask for a penumbra as well.
            assert_eq!(v.shadow_softness, 0.0);
        }
    }

    #[test]
    fn the_focus_ring_keeps_its_pre_stroke_geometry() {
        // The outer band still starts two ring widths outside the rect, which
        // is the amount `list::focus_rect` insets a row by; moving it would let
        // the ring bleed onto the neighbouring list entries.
        let rect = WidgetRect::new(100.0, 100.0, 200.0, 24.0);
        let verts = bg_vertices(|t, s| draw_focus_ring(rect, t, s));
        let t = FOCUS_RING_PX;
        assert_eq!(verts[0].rect_center, [200.0, 112.0]);
        assert_eq!(verts[0].rect_half_size, [100.0 + t * 2.0, 12.0 + t * 2.0]);
        assert_eq!(verts[4].rect_center, [200.0, 112.0]);
        assert_eq!(verts[4].rect_half_size, [100.0 + t, 12.0 + t]);
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
        let tokens = nexterm_config::DesignTokens::default();
        let metrics = nexterm_config::MetricTokens::default();
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
