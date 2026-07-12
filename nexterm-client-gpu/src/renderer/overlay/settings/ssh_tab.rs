//! Ssh category: host list + field-edit panel + Add/Delete + delete dialog.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{add_px_rect, add_string_verts, truncate_to_width};

use super::layout::compute_row_layout;
use super::row::{MIN_TEXT_CONTRAST, draw_section_header, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_ssh_tab(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
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
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    let layout = compute_row_layout(content_w, cell_w);
    let list_label_max_w = content_w - cell_w * 0.7;

    draw_section_header(
        &nexterm_i18n::fl!("settings-ssh-header"),
        content_inner_x,
        content_top + cell_h * 0.5,
        content_w,
        tokens.text_secondary,
        sw,
        sh,
        cell_w,
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
        for (i, host) in sp.ssh_hosts.iter().enumerate() {
            let item_y = content_top + cell_h * (1.5 + i as f32 * 1.2);
            let is_sel = sp.selected_host_index == i;
            if is_sel {
                add_px_rect(
                    content_inner_x - cell_w * 0.3,
                    item_y - cell_h * 0.1,
                    content_w - cell_w * 0.7,
                    cell_h,
                    tokens.surface_2,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let label = truncate_to_width(&host.label(), list_label_max_w, cell_w);
            let fg = if is_sel {
                tokens.text_secondary
            } else {
                ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST)
            };
            add_string_verts(
                &label,
                content_inner_x,
                item_y,
                fg,
                is_sel,
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

        // ===== Field-edit panel for the selected host =====
        let sel = sp.selected_host_index.min(sp.ssh_hosts.len() - 1);
        let host = &sp.ssh_hosts[sel];
        let fields_top = content_top + cell_h * (1.5 + sp.ssh_hosts.len() as f32 * 1.2 + 0.6);

        draw_section_header(
            &nexterm_i18n::fl!("settings-ssh-edit-header"),
            content_inner_x,
            fields_top,
            content_w,
            tokens.text_secondary,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            queue,
            text_verts,
            text_idx,
        );

        // Labels + current values for the 5 fields, two-column (label |
        // value). port/auth_type behave like SpinButton/ComboBox
        // (`< value >`, changeable via ←/→ without an edit mode); the
        // others enter a text-edit buffer on Enter.
        let editing_focus = sp.ssh_field_editing.as_ref().map(|_| sp.ssh_field_focus);
        let field_labels: [(String, String, u8); 5] = [
            (
                nexterm_i18n::fl!("settings-ssh-field-name"),
                host.name.clone(),
                1,
            ),
            (
                nexterm_i18n::fl!("settings-ssh-field-host"),
                host.host.clone(),
                2,
            ),
            (
                nexterm_i18n::fl!("settings-ssh-field-port"),
                host.port.to_string(),
                3,
            ),
            (
                nexterm_i18n::fl!("settings-ssh-field-username"),
                host.username.clone(),
                4,
            ),
            (
                nexterm_i18n::fl!("settings-ssh-field-auth-type"),
                host.auth_type.clone(),
                5,
            ),
        ];
        for (i, (label, raw_value, field_id)) in field_labels.iter().enumerate() {
            let row_y = fields_top + cell_h * (1.3 + i as f32 * 1.1);
            let is_focused = sp.ssh_field_focus == *field_id;
            let is_editing = editing_focus == Some(*field_id);
            let is_spin_or_combo = matches!(*field_id, 3 | 5);

            if is_focused {
                let bg_color = if is_editing {
                    tokens.surface_3
                } else {
                    tokens.surface_2
                };
                add_px_rect(
                    content_inner_x - cell_w * 0.3,
                    row_y - cell_h * 0.1,
                    content_w - cell_w * 0.7,
                    cell_h,
                    bg_color,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            let fg = if is_focused {
                tokens.text_secondary
            } else {
                ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST)
            };

            let display_value = if is_editing {
                sp.ssh_field_editing
                    .as_ref()
                    .map(|s| s.display_string())
                    .unwrap_or_else(|| raw_value.clone())
            } else if is_spin_or_combo {
                format!("< {} >", raw_value)
            } else {
                raw_value.clone()
            };

            let label_text = truncate_to_width(label, layout.label_w, cell_w);
            add_string_verts(
                &label_text,
                content_inner_x,
                row_y,
                fg,
                is_focused,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                queue,
                text_verts,
                text_idx,
            );
            let value_text = truncate_to_width(&display_value, layout.control_w, cell_w);
            add_string_verts(
                &value_text,
                content_inner_x + layout.control_x_off,
                row_y,
                fg,
                is_focused,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                queue,
                text_verts,
                text_idx,
            );

            // Cursor bar overlay while editing: positioned at the control
            // column offset + the cursor's column within the (untruncated)
            // display string.
            if is_editing && let Some(state) = sp.ssh_field_editing.as_ref() {
                let cursor_byte = state.display_cursor();
                let display = state.display_string();
                let cursor_col = display
                    .get(..cursor_byte.min(display.len()))
                    .map(|s| s.chars().count() as f32)
                    .unwrap_or(0.0);
                let cursor_x = content_inner_x + layout.control_x_off + cell_w * cursor_col;
                add_px_rect(
                    cursor_x,
                    row_y - cell_h * 0.05,
                    2.0,
                    cell_h * 1.1,
                    tokens.text_primary,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
        }

        // Footnote
        let note_y = fields_top + cell_h * (1.3 + 5.0 * 1.1 + 0.4);
        let note_text = if sp.ssh_field_editing.is_some() {
            nexterm_i18n::fl!("settings-ssh-note-editing")
        } else {
            nexterm_i18n::fl!("settings-ssh-note-idle")
        };
        add_string_verts(
            &note_text,
            content_inner_x,
            note_y,
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

    draw_add_delete_buttons(
        sp,
        tokens,
        content_top,
        content_inner_x,
        content_w,
        sw,
        sh,
        cell_w,
        cell_h,
        font,
        atlas,
        queue,
        bg_verts,
        bg_idx,
        text_verts,
        text_idx,
    );

    if sp.ssh_delete_dialog_open && !sp.ssh_hosts.is_empty() {
        draw_delete_dialog(
            sp, tokens, px, py, panel_w, panel_h, sw, sh, cell_w, cell_h, font, atlas, queue,
            bg_verts, bg_idx, text_verts, text_idx,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_add_delete_buttons(
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
    let _ = content_w;
    let buttons_y = if sp.ssh_hosts.is_empty() {
        content_top + cell_h * 4.0
    } else {
        let fields_top = content_top + cell_h * (1.5 + sp.ssh_hosts.len() as f32 * 1.2 + 0.6);
        let note_y = fields_top + cell_h * (1.3 + 5.0 * 1.1 + 0.4);
        note_y + cell_h * 1.5
    };
    let add_focused = sp.ssh_field_focus == 6;
    let delete_focused = sp.ssh_field_focus == 7;
    let delete_disabled = sp.ssh_hosts.is_empty();
    let btn_w = cell_w * 24.0;
    let btn_h = cell_h * 1.4;
    let btn_gap = cell_w * 2.0;

    let add_x = content_inner_x;
    let add_bg = if add_focused {
        tokens.surface_2
    } else {
        tokens.surface_1
    };
    add_px_rect(
        add_x - cell_w * 0.3,
        buttons_y - cell_h * 0.15,
        btn_w,
        btn_h,
        add_bg,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    let add_fg = if add_focused {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    add_string_verts(
        &nexterm_i18n::fl!("settings-ssh-add"),
        add_x,
        buttons_y,
        add_fg,
        add_focused,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let del_x = add_x + btn_w + btn_gap;
    let del_bg = if delete_focused && !delete_disabled {
        [0.298, 0.149, 0.149, 1.0]
    } else {
        tokens.surface_1
    };
    add_px_rect(
        del_x - cell_w * 0.3,
        buttons_y - cell_h * 0.15,
        btn_w,
        btn_h,
        del_bg,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    let del_fg = if delete_disabled {
        ensure_readable(tokens.text_muted, tokens.surface_1, MIN_TEXT_CONTRAST)
    } else if delete_focused {
        [0.984, 0.808, 0.808, 1.0]
    } else {
        [0.776, 0.553, 0.553, 1.0]
    };
    let del_label = if delete_disabled {
        nexterm_i18n::fl!("settings-ssh-delete-disabled")
    } else {
        nexterm_i18n::fl!("settings-ssh-delete")
    };
    add_string_verts(
        &del_label,
        del_x,
        buttons_y,
        del_fg,
        delete_focused && !delete_disabled,
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
        [0.0, 0.0, 0.0, 0.55],
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
        [0.984, 0.808, 0.808, 1.0],
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

    let confirm_bg = if confirm_focused {
        [0.498, 0.196, 0.196, 1.0]
    } else {
        [0.235, 0.118, 0.118, 1.0]
    };
    let confirm_x = dlg_btns_x + dlg_btn_w + dlg_btn_gap;
    add_px_rect(
        confirm_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, confirm_bg, sw, sh, bg_verts, bg_idx,
    );
    add_string_verts(
        &nexterm_i18n::fl!("settings-ssh-delete-confirm"),
        confirm_x + cell_w * 0.5,
        dlg_btns_y + cell_h * 0.2,
        [0.984, 0.808, 0.808, 1.0],
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
