//! Font category, migrated onto the shared widget layer (UI/UX v3 P1c).
//!
//! Four rows with hint lines between them. Geometry lives in
//! `overlay/widgets/settings_font.rs` and is shared with the mouse hit-test
//! and the AccessKit tree; only the hints are drawn here, being prose rather
//! than controls.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::add_string_verts;

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_font::{build_font_widgets, hint_y, row};

use nexterm_config::SurfaceLevel;

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_font_tab(
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
    for spec in &build_font_widgets(sp, &geometry) {
        draw_widget(spec, &theme, font, atlas, queue, &mut sink);
    }

    let muted = tokens.text_on(SurfaceLevel::S2).muted;
    let hints = [
        (
            row::FAMILY,
            if sp.font_family_editing {
                nexterm_i18n::fl!("settings-hint-confirm-cancel")
            } else {
                nexterm_i18n::fl!("settings-font-hint-press-f")
            },
        ),
        (row::SIZE, nexterm_i18n::fl!("settings-font-hint-slider")),
        (
            row::FALLBACKS,
            if sp.font_fallbacks_editing.is_some() {
                nexterm_i18n::fl!("settings-font-fallbacks-hint-editing")
            } else {
                nexterm_i18n::fl!("settings-font-fallbacks-hint-idle")
            },
        ),
    ];
    for (index, text) in hints {
        add_string_verts(
            &text,
            content_inner_x,
            hint_y(&geometry, index),
            muted,
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
}
