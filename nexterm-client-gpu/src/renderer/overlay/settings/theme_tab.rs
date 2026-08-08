//! Theme category: color-scheme cycler, preview swatches, follow-system toggle.
//!
//! Migrated onto the shared widget layer in UI/UX v3 phase P1b. The geometry
//! now lives in `overlay/widgets/settings_theme.rs` and is shared with the
//! mouse hit-test; this file only paints what that module describes, plus the
//! scheme name captions under the swatch strip (plain text, not a control).

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{add_string_verts, truncate_to_cols};

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::settings_theme::{
    THEME_CATEGORY, TabGeometry, build_theme_widgets, swatch_gap, swatch_index_of, swatch_names,
    swatch_y,
};
use super::super::widgets::tooltip::{draw_tooltip, place_tooltip};
use super::row::{MIN_TEXT_CONTRAST, ensure_readable};

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
    let max_cols = ((gap / cell_w).floor() as usize).max(1);
    for (i, name) in swatch_names().iter().enumerate() {
        let is_sel = sp.scheme_index == i;
        let color = if is_sel {
            tokens.text_secondary
        } else {
            ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST)
        };
        let x = specs
            .iter()
            .find(|s| swatch_index_of(s.id()) == Some(i))
            .map(|s| s.rect.x)
            .unwrap_or(content_inner_x);
        add_string_verts(
            &truncate_to_cols(name, max_cols),
            x,
            name_y,
            color,
            is_sel,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            queue,
            sink.text_verts,
            sink.text_idx,
        );
    }
}

/// Draw the tooltip for whichever Theme widget the pointer has been resting
/// on, if the dwell has elapsed.
///
/// Called after the scrollable content has been merged into the outer vertex
/// buffers, so the tooltip is never clipped by the content scissor. Pass
/// `content_top` already shifted by the scroll offset so the anchor follows
/// the row on screen.
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
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    let Some(dwell) = sp.hover_widget else {
        return;
    };
    if dwell.category != THEME_CATEGORY || !dwell.is_ready(std::time::Instant::now()) {
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
    let Some(spec) = specs.iter().find(|s| s.id().index == dwell.index) else {
        return;
    };
    let Some(text) = spec.desc.tooltip.as_deref() else {
        return;
    };

    let rect = place_tooltip(spec.rect, text, cell_w, cell_h, sw, sh);
    let theme = WidgetTheme {
        tokens,
        metrics,
        sw,
        sh,
        cell_w,
        cell_h,
    };
    let mut sink = WidgetSink {
        bg_verts,
        bg_idx,
        text_verts,
        text_idx,
    };
    draw_tooltip(rect, text, &theme, font, atlas, queue, &mut sink);
}
