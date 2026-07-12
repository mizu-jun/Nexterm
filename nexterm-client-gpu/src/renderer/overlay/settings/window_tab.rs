//! Window category: opacity / cursor style / padding x/y / present mode.
//!
//! 5 fields, one per row. The focused field (via `sp.window_field_focus`)
//! renders with a highlight rect spanning the row and a brighter label.
//! Controls: ↑/↓ move between fields, ←/→ change the focused value
//! (handled in the input handler, unchanged by this layout pass).
//!
//! Row-Y geometry (`labels_top`, `row_h`) and the slider track geometry
//! (`bar_y` / `bar_w` / `content_inner_x`) mirror
//! `settings_panel_hit.rs::hit_test_settings_panel` exactly — keep both in
//! sync when changing either file.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{add_px_rect, add_string_verts, truncate_to_width};

use super::layout::compute_row_layout;
use super::row::{MIN_TEXT_CONTRAST, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_window_tab(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
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
    let layout = compute_row_layout(content_w, cell_w);
    let focus = sp.window_field_focus;
    let bar_w = content_w - cell_w * 3.0;
    let row_h = cell_h * 3.2;
    let labels_top = content_top + cell_h * 0.6;

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

    let draw_row_highlight =
        |row_y: f32, is_focused: bool, bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>| {
            if is_focused {
                add_px_rect(
                    content_inner_x - cell_w * 0.3,
                    row_y - cell_h * 0.1,
                    content_w - cell_w * 0.7,
                    cell_h * 3.0,
                    tokens.surface_2,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
        };

    // ===== Row 0: opacity (slider) =====
    let row0_y = labels_top;
    draw_row_highlight(row0_y, focus == 0, bg_verts, bg_idx);
    let opacity_color = if focus == 0 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let opacity_line = nexterm_i18n::fl!(
        "settings-window-opacity",
        value = format!("{:.0}", sp.opacity * 100.0)
    );
    let opacity_line = truncate_to_width(&opacity_line, layout.label_w + layout.control_w, cell_w);
    add_string_verts(
        &opacity_line,
        content_inner_x,
        row0_y,
        opacity_color,
        focus == 0,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let bar_y = row0_y + cell_h * 1.4;
    add_px_rect(
        content_inner_x,
        bar_y,
        bar_w,
        cell_h * 0.35,
        tokens.surface_1,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    add_px_rect(
        content_inner_x,
        bar_y,
        bar_w * sp.opacity,
        cell_h * 0.35,
        tokens.accent_primary,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // ===== Row 1: cursor style (cycle) =====
    let row1_y = labels_top + row_h;
    draw_row_highlight(row1_y, focus == 1, bg_verts, bg_idx);
    let cs_label_color = if focus == 1 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let cs_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-window-cursor-style"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &cs_label,
        content_inner_x,
        row1_y,
        cs_label_color,
        focus == 1,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let cs_value = format!("< {} >", sp.cursor_style_label());
    let cs_value = truncate_to_width(&cs_value, layout.control_w, cell_w);
    add_string_verts(
        &cs_value,
        content_inner_x + layout.control_x_off,
        row1_y,
        cs_label_color,
        focus == 1,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    // ===== Row 2: horizontal padding =====
    let row2_y = labels_top + row_h * 2.0;
    draw_row_highlight(row2_y, focus == 2, bg_verts, bg_idx);
    let px_color = if focus == 2 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let px_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-window-horizontal-padding", value = sp.padding_x),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &px_label,
        content_inner_x,
        row2_y,
        px_color,
        focus == 2,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    // Mini slider (0 - 32). Track geometry mirrors the hit-test exactly.
    let px_bar_y = row2_y + cell_h * 1.4;
    let px_bar_w = bar_w * 0.6;
    add_px_rect(
        content_inner_x,
        px_bar_y,
        px_bar_w,
        cell_h * 0.25,
        tokens.surface_1,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    add_px_rect(
        content_inner_x,
        px_bar_y,
        px_bar_w * (sp.padding_x as f32 / 32.0),
        cell_h * 0.25,
        tokens.accent_primary,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // ===== Row 3: vertical padding =====
    let row3_y = labels_top + row_h * 3.0;
    draw_row_highlight(row3_y, focus == 3, bg_verts, bg_idx);
    let py_color = if focus == 3 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let py_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-window-vertical-padding", value = sp.padding_y),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &py_label,
        content_inner_x,
        row3_y,
        py_color,
        focus == 3,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let py_bar_y = row3_y + cell_h * 1.4;
    let py_bar_w = bar_w * 0.6;
    add_px_rect(
        content_inner_x,
        py_bar_y,
        py_bar_w,
        cell_h * 0.25,
        tokens.surface_1,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    add_px_rect(
        content_inner_x,
        py_bar_y,
        py_bar_w * (sp.padding_y as f32 / 32.0),
        cell_h * 0.25,
        tokens.accent_primary,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // ===== Row 4: present mode (cycle) =====
    let row4_y = labels_top + row_h * 4.0;
    draw_row_highlight(row4_y, focus == 4, bg_verts, bg_idx);
    let pm_color = if focus == 4 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let pm_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-window-present-mode"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &pm_label,
        content_inner_x,
        row4_y,
        pm_color,
        focus == 4,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let pm_value = format!("< {} >", sp.present_mode_label());
    let pm_value = truncate_to_width(&pm_value, layout.control_w, cell_w);
    add_string_verts(
        &pm_value,
        content_inner_x + layout.control_x_off,
        row4_y,
        pm_color,
        focus == 4,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    // ===== Phase B4: rows 5-10 — cursor blink / scrollback / tab-bar toggles /
    // animation toggle+intensity. All are simple label+value rows sharing the
    // same row_h spacing as rows 1/4 above (no slider track needed).
    let toggle_str = |v: bool| if v { "[ON ]" } else { "[OFF]" };

    let row5_y = labels_top + row_h * 5.0;
    draw_row_highlight(row5_y, focus == 5, bg_verts, bg_idx);
    let row5_color = if focus == 5 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row5_label = truncate_to_width(
        &nexterm_i18n::fl!(
            "settings-window-cursor-blink",
            value = toggle_str(sp.cursor_blink_enabled)
        ),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &row5_label,
        content_inner_x,
        row5_y,
        row5_color,
        focus == 5,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let row6_y = labels_top + row_h * 6.0;
    draw_row_highlight(row6_y, focus == 6, bg_verts, bg_idx);
    let row6_color = if focus == 6 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row6_label = truncate_to_width(
        &nexterm_i18n::fl!(
            "settings-window-scrollback-lines",
            value = sp.scrollback_lines
        ),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &row6_label,
        content_inner_x,
        row6_y,
        row6_color,
        focus == 6,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let row7_y = labels_top + row_h * 7.0;
    draw_row_highlight(row7_y, focus == 7, bg_verts, bg_idx);
    let row7_color = if focus == 7 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row7_label = truncate_to_width(
        &nexterm_i18n::fl!(
            "settings-window-show-tab-number",
            value = toggle_str(sp.tab_show_tab_number)
        ),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &row7_label,
        content_inner_x,
        row7_y,
        row7_color,
        focus == 7,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let row8_y = labels_top + row_h * 8.0;
    draw_row_highlight(row8_y, focus == 8, bg_verts, bg_idx);
    let row8_color = if focus == 8 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row8_label = truncate_to_width(
        &nexterm_i18n::fl!(
            "settings-window-show-new-tab-button",
            value = toggle_str(sp.tab_show_new_tab_button)
        ),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &row8_label,
        content_inner_x,
        row8_y,
        row8_color,
        focus == 8,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let row9_y = labels_top + row_h * 9.0;
    draw_row_highlight(row9_y, focus == 9, bg_verts, bg_idx);
    let row9_color = if focus == 9 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row9_label = truncate_to_width(
        &nexterm_i18n::fl!(
            "settings-window-animations-enabled",
            value = toggle_str(sp.animations_enabled)
        ),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &row9_label,
        content_inner_x,
        row9_y,
        row9_color,
        focus == 9,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let row10_y = labels_top + row_h * 10.0;
    draw_row_highlight(row10_y, focus == 10, bg_verts, bg_idx);
    let row10_color = if focus == 10 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row10_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-window-animation-intensity"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &row10_label,
        content_inner_x,
        row10_y,
        row10_color,
        focus == 10,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let row10_value = format!("< {} >", sp.animations_intensity_label());
    let row10_value = truncate_to_width(&row10_value, layout.control_w, cell_w);
    add_string_verts(
        &row10_value,
        content_inner_x + layout.control_x_off,
        row10_y,
        row10_color,
        focus == 10,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    // ===== Phase B4-P2: rows 11-13 — window decorations / close action /
    // GPU fps limit. Rows 11/12 are enum cycles (same shape as rows 1/4
    // above); row 13 is a numeric field (same shape as row 6).
    let row11_y = labels_top + row_h * 11.0;
    draw_row_highlight(row11_y, focus == 11, bg_verts, bg_idx);
    let row11_color = if focus == 11 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row11_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-window-decorations"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &row11_label,
        content_inner_x,
        row11_y,
        row11_color,
        focus == 11,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let row11_value = format!("< {} >", sp.window_decorations_label());
    let row11_value = truncate_to_width(&row11_value, layout.control_w, cell_w);
    add_string_verts(
        &row11_value,
        content_inner_x + layout.control_x_off,
        row11_y,
        row11_color,
        focus == 11,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let row12_y = labels_top + row_h * 12.0;
    draw_row_highlight(row12_y, focus == 12, bg_verts, bg_idx);
    let row12_color = if focus == 12 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row12_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-window-close-action"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &row12_label,
        content_inner_x,
        row12_y,
        row12_color,
        focus == 12,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let row12_value = format!("< {} >", sp.window_close_action_label());
    let row12_value = truncate_to_width(&row12_value, layout.control_w, cell_w);
    add_string_verts(
        &row12_value,
        content_inner_x + layout.control_x_off,
        row12_y,
        row12_color,
        focus == 12,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let row13_y = labels_top + row_h * 13.0;
    draw_row_highlight(row13_y, focus == 13, bg_verts, bg_idx);
    let row13_color = if focus == 13 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let row13_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-window-fps-limit", value = sp.fps_limit_label()),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &row13_label,
        content_inner_x,
        row13_y,
        row13_color,
        focus == 13,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
}
