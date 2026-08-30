//! Security category, migrated onto the shared widget layer (UI/UX v3 P1c).
//!
//! Four consent-policy cyclers followed by three byte-cap fields, plus a
//! footer note. Geometry lives in `overlay/widgets/settings_security.rs`.
use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_security::{build_security_widgets, note_y};
use super::row::draw_description_rows;
use nexterm_config::SurfaceLevel;

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_security_tab(
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
    for spec in &build_security_widgets(sp, &geometry) {
        draw_widget(spec, &theme, font, atlas, queue, &mut sink);
    }

    // Footer note: the plugin_read policy has no synchronous prompt path
    // yet, so `prompt` currently behaves as `deny`.
    draw_description_rows(
        &nexterm_i18n::fl!("settings-security-note"),
        content_inner_x,
        note_y(&geometry),
        cell_h,
        content_w,
        tokens.text_on(SurfaceLevel::S2).muted,
        sw,
        sh,
        font,
        atlas,
        queue,
        sink.text_verts,
        sink.text_idx,
    );
}
