//! Startup category, migrated onto the shared widget layer (UI/UX v3 P1c).
//!
//! Language picker, update-check toggle and the two shell fields. Geometry
//! lives in `overlay/widgets/settings_startup.rs`; only the explanatory note
//! is drawn here.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::add_string_verts;

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_startup::{build_startup_widgets, language_note_y};
use super::row::{MIN_TEXT_CONTRAST, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_startup_tab(
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
    for spec in &build_startup_widgets(sp, &geometry) {
        draw_widget(spec, &theme, font, atlas, queue, &mut sink);
    }

    add_string_verts(
        &nexterm_i18n::fl!("settings-startup-language-note"),
        content_inner_x,
        language_note_y(&geometry),
        ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST),
        false,
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
