//! Window category, migrated onto the shared widget layer (UI/UX v3 P1c).
//!
//! The geometry lives in `overlay/widgets/settings_window.rs` and is shared
//! with the mouse hit-test and the AccessKit tree, so the "keep both in sync"
//! contract the old hand-rolled version carried is gone.
//!
//! The only thing drawn here beyond the widgets is the navigation hint line
//! above the rows, which is prose rather than a control.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::add_string_verts;

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_window::build_window_widgets;
use super::row::{MIN_TEXT_CONTRAST, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_window_tab(
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
    // UI/UX v3 P3c: what the OS last reported, so the animations-enabled
    // row's `auto` state can say which way it currently resolves.
    animations_os_reduced: bool,
) {
    add_string_verts(
        &nexterm_i18n::fl!("settings-window-hint-nav"),
        content_inner_x,
        content_top,
        ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST),
        false,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

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
    for spec in &build_window_widgets(sp, &geometry, animations_os_reduced) {
        draw_widget(spec, &theme, font, atlas, queue, &mut sink);
    }
}
