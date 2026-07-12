//! Keybindings category: key-binding list + field-edit panel + Add/Delete +
//! delete dialog.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::{KEYBINDING_ACTIONS, KeyEditMode, SettingsPanel};
use crate::vertex_util::{add_px_rect, add_string_verts, truncate_to_width};

use super::layout::compute_row_layout;
use super::row::{MIN_TEXT_CONTRAST, draw_section_header, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_keybindings_tab(
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
        &nexterm_i18n::fl!("settings-keybindings-header"),
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
    if sp.keybindings.is_empty() {
        add_string_verts(
            &nexterm_i18n::fl!("settings-keybindings-empty"),
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
    } else {
        let row_count = sp.keybindings.len();
        for (i, kb) in sp.keybindings.iter().enumerate() {
            let item_y = content_top + cell_h * (1.5 + i as f32 * 1.2);
            let is_sel = sp.selected_key_index == i;
            if is_sel && sp.key_field_focus == 0 {
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
            let label = truncate_to_width(&kb.label(), list_label_max_w, cell_w);
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

        let sel = sp.selected_key_index.min(sp.keybindings.len() - 1);
        let kb = &sp.keybindings[sel];
        let visible_rows = row_count as f32;
        let fields_top = content_top + cell_h * (1.5 + visible_rows * 1.2 + 1.4);
        let header = match &sp.key_editing {
            Some(KeyEditMode::Record) => nexterm_i18n::fl!("settings-keybindings-edit-record"),
            Some(KeyEditMode::Text(_)) => nexterm_i18n::fl!("settings-keybindings-edit-text"),
            None => match sp.key_field_focus {
                1 => nexterm_i18n::fl!("settings-keybindings-edit-key-focus"),
                2 => nexterm_i18n::fl!("settings-keybindings-edit-action-focus"),
                _ => nexterm_i18n::fl!("settings-keybindings-edit-default"),
            },
        };
        draw_section_header(
            &header,
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

        // Show the in-flight Text buffer when editing; otherwise the stored value.
        let key_display: String = match &sp.key_editing {
            Some(KeyEditMode::Record) => {
                nexterm_i18n::fl!("settings-keybindings-recording-placeholder")
            }
            Some(KeyEditMode::Text(state)) => {
                let mut s = state.buffer.clone();
                if let Some(pre) = state.preedit.as_ref() {
                    s.push_str(pre);
                }
                s
            }
            None => kb.key.clone(),
        };
        // Render the action together with its position in
        // `KEYBINDING_ACTIONS` (or `(unknown)` when the configured value is
        // not in the fixed list).
        let action_display: String = {
            let actions = KEYBINDING_ACTIONS;
            match actions.iter().position(|&a| a == kb.action) {
                Some(i) => format!("{} ({}/{})", kb.action, i + 1, actions.len()),
                None => {
                    nexterm_i18n::fl!("settings-keybindings-action-unknown", action = kb.action)
                }
            }
        };
        let field_labels: [(String, &str, u8); 2] = [
            (
                nexterm_i18n::fl!("settings-keybindings-field-key"),
                key_display.as_str(),
                1,
            ),
            (
                nexterm_i18n::fl!("settings-keybindings-field-action"),
                action_display.as_str(),
                2,
            ),
        ];
        for (i, (label, raw_value, field_id)) in field_labels.iter().enumerate() {
            let row_y = fields_top + cell_h * (1.3 + i as f32 * 1.1);
            let is_focused = sp.key_field_focus == *field_id;
            if is_focused {
                add_px_rect(
                    content_inner_x - cell_w * 0.3,
                    row_y - cell_h * 0.1,
                    content_w - cell_w * 0.7,
                    cell_h,
                    tokens.surface_2,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let display = if raw_value.is_empty() {
                nexterm_i18n::fl!("settings-field-empty")
            } else {
                (*raw_value).to_string()
            };
            // An unknown/typo'd action is highlighted in red regardless of
            // focus state, so the validation hit is visible even without
            // inspecting the header hint.
            let action_invalid = *field_id == 2 && !sp.selected_key_action_is_valid();
            let fg = if action_invalid {
                tokens.semantic_error
            } else if is_focused {
                tokens.text_secondary
            } else {
                ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST)
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
            let value_text = truncate_to_width(&display, layout.control_w, cell_w);
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
        }
    }

    draw_add_delete_buttons(
        sp,
        tokens,
        content_top,
        content_inner_x,
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

    draw_leader_key_row(
        sp,
        tokens,
        content_top,
        content_inner_x,
        &layout,
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

    if sp.key_delete_dialog_open && !sp.keybindings.is_empty() {
        draw_delete_dialog(
            sp, tokens, px, py, panel_w, panel_h, sw, sh, cell_w, cell_h, font, atlas, queue,
            bg_verts, bg_idx, text_verts, text_idx,
        );
    }
}

/// Y position of the Add/Delete button row. Shared with
/// [`draw_leader_key_row`] so the leader-key row can stack directly beneath
/// it regardless of the (variable-length) binding list above.
fn key_buttons_y(sp: &SettingsPanel, content_top: f32, cell_h: f32) -> f32 {
    if sp.keybindings.is_empty() {
        content_top + cell_h * 4.0
    } else {
        let visible_rows = sp.keybindings.len() as f32;
        let fields_top = content_top + cell_h * (1.5 + visible_rows * 1.2 + 1.4);
        let last_field_y = fields_top + cell_h * (1.3 + 1.0 * 1.1);
        last_field_y + cell_h * 2.0
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_add_delete_buttons(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    content_top: f32,
    content_inner_x: f32,
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
    let key_buttons_y = key_buttons_y(sp, content_top, cell_h);
    let key_add_focused = sp.key_field_focus == 3;
    let key_delete_focused = sp.key_field_focus == 4;
    let key_delete_disabled = sp.keybindings.is_empty();
    let key_btn_w = cell_w * 26.0;
    let key_btn_h = cell_h * 1.4;
    let key_btn_gap = cell_w * 2.0;

    let key_add_x = content_inner_x;
    let key_add_bg = if key_add_focused {
        tokens.surface_2
    } else {
        tokens.surface_1
    };
    add_px_rect(
        key_add_x - cell_w * 0.3,
        key_buttons_y - cell_h * 0.15,
        key_btn_w,
        key_btn_h,
        key_add_bg,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    let key_add_fg = if key_add_focused {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    add_string_verts(
        &nexterm_i18n::fl!("settings-keybindings-add"),
        key_add_x,
        key_buttons_y,
        key_add_fg,
        key_add_focused,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let key_del_x = key_add_x + key_btn_w + key_btn_gap;
    let key_del_bg = if key_delete_focused && !key_delete_disabled {
        [0.298, 0.149, 0.149, 1.0]
    } else {
        tokens.surface_1
    };
    add_px_rect(
        key_del_x - cell_w * 0.3,
        key_buttons_y - cell_h * 0.15,
        key_btn_w,
        key_btn_h,
        key_del_bg,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    let key_del_fg = if key_delete_disabled {
        ensure_readable(tokens.text_muted, tokens.surface_1, MIN_TEXT_CONTRAST)
    } else if key_delete_focused {
        [0.984, 0.808, 0.808, 1.0]
    } else {
        [0.776, 0.553, 0.553, 1.0]
    };
    let key_del_label = if key_delete_disabled {
        nexterm_i18n::fl!("settings-keybindings-delete-disabled")
    } else {
        nexterm_i18n::fl!("settings-keybindings-delete")
    };
    add_string_verts(
        &key_del_label,
        key_del_x,
        key_buttons_y,
        key_del_fg,
        key_delete_focused && !key_delete_disabled,
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

/// Phase B4-P2: `leader_key` field, stacked below the Add/Delete buttons.
/// Always present (`key_field_focus == 5`), regardless of whether
/// `keybindings` is empty.
#[allow(clippy::too_many_arguments)]
fn draw_leader_key_row(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    content_top: f32,
    content_inner_x: f32,
    layout: &super::layout::RowLayout,
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
    let row_y = key_buttons_y(sp, content_top, cell_h) + cell_h * 3.0;
    let is_focused = sp.key_field_focus == 5;
    let editing = sp.leader_key_editing.is_some();
    if is_focused || editing {
        add_px_rect(
            content_inner_x - cell_w * 0.3,
            row_y - cell_h * 0.1,
            layout.control_x_off + layout.control_w + cell_w * 0.6,
            cell_h * 1.2,
            tokens.surface_2,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
    }
    let display: String = match &sp.leader_key_editing {
        Some(state) => {
            let mut s = state.buffer.clone();
            if let Some(pre) = state.preedit.as_ref() {
                s.push_str(pre);
            }
            format!("{s}|")
        }
        None => sp.leader_key.clone(),
    };
    let fg = if is_focused || editing {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let label = truncate_to_width(
        &nexterm_i18n::fl!("settings-keybindings-leader-key"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &label,
        content_inner_x,
        row_y,
        fg,
        is_focused || editing,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let value = truncate_to_width(&display, layout.control_w, cell_w);
    add_string_verts(
        &value,
        content_inner_x + layout.control_x_off,
        row_y,
        fg,
        is_focused || editing,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let hint = if editing {
        nexterm_i18n::fl!("settings-hint-confirm-cancel")
    } else {
        nexterm_i18n::fl!("settings-hint-edit-idle")
    };
    add_string_verts(
        &hint,
        content_inner_x,
        row_y + cell_h * 1.3,
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
    let sel = sp.selected_key_index.min(sp.keybindings.len() - 1);
    let target = sp.keybindings[sel].label();

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
        &nexterm_i18n::fl!("settings-keybindings-delete-title"),
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
    let msg = nexterm_i18n::fl!("settings-delete-confirm-message", target = target);
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
    let confirm_focused = sp.key_delete_dialog_confirm_focused;

    let cancel_bg = if !confirm_focused {
        tokens.surface_3
    } else {
        tokens.surface_1
    };
    add_px_rect(
        dlg_btns_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, cancel_bg, sw, sh, bg_verts, bg_idx,
    );
    let cancel_fg = if !confirm_focused {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    add_string_verts(
        &nexterm_i18n::fl!("settings-dialog-cancel-bracketed"),
        dlg_btns_x + cell_w,
        dlg_btns_y + cell_h * 0.2,
        cancel_fg,
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

    let confirm_x = dlg_btns_x + dlg_btn_w + dlg_btn_gap;
    let confirm_bg = if confirm_focused {
        [0.486, 0.180, 0.180, 1.0]
    } else {
        tokens.surface_1
    };
    add_px_rect(
        confirm_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, confirm_bg, sw, sh, bg_verts, bg_idx,
    );
    let confirm_fg = if confirm_focused {
        [0.984, 0.808, 0.808, 1.0]
    } else {
        [0.776, 0.553, 0.553, 1.0]
    };
    add_string_verts(
        &nexterm_i18n::fl!("settings-keybindings-delete-confirm"),
        confirm_x + cell_w,
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
}
