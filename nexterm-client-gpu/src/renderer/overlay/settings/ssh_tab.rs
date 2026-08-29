//! Ssh category: host list + field-edit panel + Add/Delete + delete dialog.
//!
//! Migrated onto the shared widget layer in UI/UX v3 phase P1c. The windowed
//! host list, the five fields and the Add/Delete buttons live in
//! `overlay/widgets/settings_ssh.rs`, shared with the mouse hit-test and the
//! AccessKit tree; this file paints what that module describes, plus the
//! prose it still owns (section headers, the empty state, the list
//! range-indicator, the edit-hint note) and the delete-confirmation modal,
//! which is deliberately not a settings row.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::super::util::{SCRIM_ALPHA_FLOOR, scrim_color};
use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_ssh::{
    build_ssh_widgets, ssh_fields_top, ssh_list_window, ssh_note_y,
};
use super::layout::LIST_ROW_PITCH;
use super::row::{MIN_TEXT_CONTRAST, danger_button_colors, draw_section_header, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_ssh_tab(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    metrics: &nexterm_config::MetricTokens,
    px: f32,
    py: f32,
    panel_w: f32,
    panel_h: f32,
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
    draw_section_header(
        &nexterm_i18n::fl!("settings-ssh-header"),
        content_inner_x,
        content_top + cell_h * 0.5,
        content_w,
        tokens.text_secondary,
        sw,
        sh,
        metrics,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    if sp.ssh_hosts.is_empty() {
        add_string_verts(
            &nexterm_i18n::fl!("settings-ssh-empty"),
            content_inner_x,
            content_top + cell_h * 1.8,
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
        add_string_verts(
            &nexterm_i18n::fl!("settings-ssh-empty-hint"),
            content_inner_x,
            content_top + cell_h * 2.7,
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
    } else {
        // Range indicator: which slice of the full list the window shows.
        let w = ssh_list_window(sp);
        if w.clipped {
            let indicator_y = content_top + cell_h * (1.5 + w.visible as f32 * LIST_ROW_PITCH);
            add_string_verts(
                &nexterm_i18n::fl!(
                    "settings-list-window",
                    from = w.first + 1,
                    to = w.first + w.visible,
                    total = sp.ssh_hosts.len()
                ),
                content_inner_x,
                indicator_y,
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
        }

        draw_section_header(
            &nexterm_i18n::fl!("settings-ssh-edit-header"),
            content_inner_x,
            ssh_fields_top(sp, content_top, cell_h),
            content_w,
            tokens.text_secondary,
            sw,
            sh,
            metrics,
            font,
            atlas,
            queue,
            text_verts,
            text_idx,
        );

        let note_text = if sp.ssh_field_editing.is_some() {
            nexterm_i18n::fl!("settings-ssh-note-editing")
        } else {
            nexterm_i18n::fl!("settings-ssh-note-idle")
        };
        add_string_verts(
            &note_text,
            content_inner_x,
            ssh_note_y(sp, content_top, cell_h),
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
    }

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
    for spec in &build_ssh_widgets(sp, &geometry) {
        draw_widget(spec, &theme, font, atlas, queue, &mut sink);
    }

    if sp.ssh_delete_dialog_open && !sp.ssh_hosts.is_empty() {
        draw_delete_dialog(
            sp,
            tokens,
            px,
            py,
            panel_w,
            panel_h,
            sw,
            sh,
            cell_w,
            cell_h,
            font,
            atlas,
            queue,
            sink.bg_verts,
            sink.bg_idx,
            sink.text_verts,
            sink.text_idx,
        );
    }
}

/// Delete-confirmation modal, drawn centered over the whole panel.
#[allow(clippy::too_many_arguments)]
fn draw_delete_dialog(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    px: f32,
    py: f32,
    panel_w: f32,
    panel_h: f32,
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
    let sel = sp.selected_host_index.min(sp.ssh_hosts.len() - 1);
    let target_name = if sp.ssh_hosts[sel].name.is_empty() {
        sp.ssh_hosts[sel].host.clone()
    } else {
        sp.ssh_hosts[sel].name.clone()
    };

    add_px_rect(
        px,
        py,
        panel_w,
        panel_h,
        scrim_color(tokens, SCRIM_ALPHA_FLOOR),
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    let dialog_w = panel_w * 0.55;
    let dialog_h = cell_h * 8.5;
    let dialog_x = px + (panel_w - dialog_w) / 2.0;
    let dialog_y = py + (panel_h - dialog_h) / 2.0;

    add_px_rect(
        dialog_x - 2.0,
        dialog_y - 2.0,
        dialog_w + 4.0,
        dialog_h + 4.0,
        {
            let [r, g, b, _] = tokens.semantic_error;
            [r, g, b, 0.80]
        },
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    add_px_rect(
        dialog_x,
        dialog_y,
        dialog_w,
        dialog_h,
        tokens.surface_0,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    add_string_verts(
        &nexterm_i18n::fl!("settings-ssh-delete-title"),
        dialog_x + cell_w,
        dialog_y + cell_h * 0.6,
        ensure_readable(tokens.semantic_error, tokens.surface_0, MIN_TEXT_CONTRAST),
        true,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let msg = nexterm_i18n::fl!("settings-delete-confirm-message", target = target_name);
    add_string_verts(
        &msg,
        dialog_x + cell_w,
        dialog_y + cell_h * 2.2,
        tokens.text_secondary,
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

    let dlg_btn_w = cell_w * 14.0;
    let dlg_btn_h = cell_h * 1.4;
    let dlg_btn_gap = cell_w * 2.0;
    let dlg_btns_total_w = dlg_btn_w * 2.0 + dlg_btn_gap;
    let dlg_btns_x = dialog_x + (dialog_w - dlg_btns_total_w) / 2.0;
    let dlg_btns_y = dialog_y + dialog_h - cell_h * 2.5;
    let confirm_focused = sp.ssh_delete_dialog_confirm_focused;

    let cancel_bg = if !confirm_focused {
        tokens.surface_3
    } else {
        tokens.surface_1
    };
    add_px_rect(
        dlg_btns_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, cancel_bg, sw, sh, bg_verts, bg_idx,
    );
    add_string_verts(
        &nexterm_i18n::fl!("settings-dialog-cancel-plain"),
        dlg_btns_x + cell_w * 0.5,
        dlg_btns_y + cell_h * 0.2,
        tokens.text_primary,
        !confirm_focused,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let (confirm_bg, confirm_fg) = danger_button_colors(tokens, confirm_focused);
    let confirm_x = dlg_btns_x + dlg_btn_w + dlg_btn_gap;
    add_px_rect(
        confirm_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, confirm_bg, sw, sh, bg_verts, bg_idx,
    );
    add_string_verts(
        &nexterm_i18n::fl!("settings-ssh-delete-confirm"),
        confirm_x + cell_w * 0.5,
        dlg_btns_y + cell_h * 0.2,
        confirm_fg,
        confirm_focused,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    add_string_verts(
        &nexterm_i18n::fl!("settings-ssh-delete-hint"),
        dialog_x + cell_w,
        dialog_y + dialog_h - cell_h * 0.9,
        ensure_readable(tokens.text_muted, tokens.surface_0, MIN_TEXT_CONTRAST),
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
}
