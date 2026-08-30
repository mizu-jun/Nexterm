//! Theme category: color-scheme cycler, preview swatches, follow-system toggle.
//!
//! Migrated onto the shared widget layer in UI/UX v3 phase P1b. The geometry
//! now lives in `overlay/widgets/settings_theme.rs` and is shared with the
//! mouse hit-test; this file only paints what that module describes, plus the
//! scheme name captions under the swatch strip (plain text, not a control).

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{add_run_verts, truncate_run_to_width};

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_theme::{
    THEME_CATEGORY, build_theme_widgets, swatch_gap, swatch_index_of, swatch_names, swatch_y,
};
use super::super::widgets::tooltip::{draw_tooltip, measure_tooltip, place_tooltip};

use nexterm_config::SurfaceLevel;

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_theme_tab(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    metrics: &nexterm_config::MetricTokens,
    content_top: f32,
    content_inner_x: f32,
    content_w: f32,
    sw: f32,
    sh: f32,
    cell_w: f32,
    cell_h: f32,
    now: std::time::Instant,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    let geometry = TabGeometry {
        content_top,
        content_inner_x,
        content_w,
        cell_w,
        cell_h,
    };
    let specs = build_theme_widgets(sp, &geometry);

    let theme = WidgetTheme {
        tokens,
        metrics,
        sw,
        sh,
        cell_w,
        cell_h,
        hover: &sp.hover_transition,
        press: &sp.press_pulse,
        now,
    };
    let mut sink = WidgetSink {
        bg_verts,
        bg_idx,
        text_verts,
        text_idx,
    };
    for spec in &specs {
        draw_widget(spec, &theme, font, atlas, queue, &mut sink);
    }

    // Scheme name captions under the swatches. The swatch slot bounds the
    // caption width, so a long scheme name never bleeds into the next chip.
    let gap = swatch_gap(&geometry);
    let name_y = swatch_y(&geometry) + cell_h * 1.3;
    // UI/UX v3 N-6c: the caption is bounded by the swatch slot in *pixels*.
    // `truncate_to_cols` converted that gap into a column count and then cut
    // by display width, which only bounds the drawn caption if it is drawn at
    // the cell — it is not. A scheme name is also the one string here that is
    // not translated, so the count was never going to be checked by a locale.
    let ramp = nexterm_config::MetricTokens::default().type_ramp;
    for (i, name) in swatch_names().iter().enumerate() {
        let is_sel = sp.scheme_index == i;
        let color = if is_sel {
            tokens.text_on(SurfaceLevel::S2).secondary
        } else {
            tokens.text_on(SurfaceLevel::S2).muted
        };
        // The selected caption was drawn bold on the cell path; `body_strong`
        // is the ramp's name for that (P4b D-2 maps its 600 to the bold flag).
        let style = if is_sel { ramp.body_strong } else { ramp.body };
        let x = specs
            .iter()
            .find(|s| swatch_index_of(s.id()) == Some(i))
            .map(|s| s.rect.x)
            .unwrap_or(content_inner_x);
        add_run_verts(
            &truncate_run_to_width(name, &style, gap, font),
            &style,
            x,
            name_y,
            color,
            sw,
            sh,
            font,
            atlas,
            queue,
            sink.text_verts,
            sink.text_idx,
        );
    }
}

/// Draw the tooltip for whichever Theme widget last had the pointer resting
/// on it, if `fade` says it should still be on screen.
///
/// Called after the scrollable content has been merged into the outer vertex
/// buffers, so the tooltip is never clipped by the content scissor. Pass
/// `content_top` already shifted by the scroll offset so the anchor follows
/// the row on screen.
///
/// The anchor and text come from `sp.tooltip_snapshot`, not from a fresh
/// `hover_widget` lookup: while the tooltip fades out (UI/UX v3 P3b),
/// `hover_widget` may already be `None`, and the snapshot is what lets the
/// exit still be drawn. `fade` (from `sp.tooltip_motion.progress`) is the
/// sole authority on whether to draw at all.
#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_theme_tooltip(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    metrics: &nexterm_config::MetricTokens,
    content_top: f32,
    content_inner_x: f32,
    content_w: f32,
    sw: f32,
    sh: f32,
    cell_w: f32,
    cell_h: f32,
    acrylic_mix: f32,
    now: std::time::Instant,
    fade: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    if fade <= 0.0 {
        return;
    }
    let Some((category, index)) = sp.tooltip_snapshot else {
        return;
    };
    if category != THEME_CATEGORY {
        return;
    }

    let geometry = TabGeometry {
        content_top,
        content_inner_x,
        content_w,
        cell_w,
        cell_h,
    };
    let specs = build_theme_widgets(sp, &geometry);
    let Some(spec) = specs.iter().find(|s| s.id().index == index) else {
        return;
    };
    let Some(text) = spec.desc.tooltip.as_deref() else {
        return;
    };

    // Measured at the step the tooltip draws in, so the box fits the glyphs
    // (UI/UX v3 P4b).
    let (text_w, line_h) = measure_tooltip(text, metrics, font);
    let rect = place_tooltip(spec.rect, text_w, line_h, cell_w, cell_h, sw, sh);
    let theme = WidgetTheme {
        tokens,
        metrics,
        sw,
        sh,
        cell_w,
        cell_h,
        hover: &sp.hover_transition,
        press: &sp.press_pulse,
        now,
    };
    let bg_start = bg_verts.len();
    let text_start = text_verts.len();
    let mut sink = WidgetSink {
        bg_verts,
        bg_idx,
        text_verts,
        text_idx,
    };
    draw_tooltip(
        rect,
        text,
        &theme,
        acrylic_mix,
        font,
        atlas,
        queue,
        &mut sink,
    );
    super::super::fade::apply_surface_fade(
        &mut bg_verts[bg_start..],
        &mut text_verts[text_start..],
        fade,
    );
}
