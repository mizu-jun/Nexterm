//! Keybindings category: key-binding list + field-edit panel + Add/Delete +
//! leader-key row + delete dialog.
//!
//! Migrated onto the shared widget layer in UI/UX v3 phase P1c. The windowed
//! binding list, the key/action pair, the Add/Delete buttons and the
//! leader-key row live in `overlay/widgets/settings_keybindings.rs`, shared
//! with the mouse hit-test and the AccessKit tree; this file paints what that
//! module describes, plus the prose it still owns (section header, the empty
//! state, the list range-indicator, the mode-dependent edit header) and the
//! delete-confirmation modal, which is deliberately not a settings row.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::{KeyEditMode, SettingsPanel};
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_keybindings::{
    build_keybindings_widgets, key_fields_top, key_leader_y, key_list_window,
};
use super::layout::LIST_ROW_PITCH;
use super::row::{MIN_TEXT_CONTRAST, draw_section_header, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_keybindings_tab(
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
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
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
        // Range indicator: which slice of the full list the window shows.
        let w = key_list_window(sp);
        if w.clipped {
            let indicator_y = content_top + cell_h * (1.5 + w.visible as f32 * LIST_ROW_PITCH);
            add_string_verts(
                &nexterm_i18n::fl!(
                    "settings-list-window",
                    from = w.first + 1,
                    to = w.first + w.visible,
                    total = sp.keybindings.len()
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

        // Mode-dependent edit header above the key/action pair.
        let header = match &sp.key_editing {
            Some(KeyEditMode::Record) => nexterm_i18n::fl!("settings-keybindings-edit-record"),
            Some(KeyEditMode::Text(_)) => nexterm_i18n::fl!("settings-keybindings-edit-text"),
            None => match sp.focused_widget_index {
                1 => nexterm_i18n::fl!("settings-keybindings-edit-key-focus"),
                2 => nexterm_i18n::fl!("settings-keybindings-edit-action-focus"),
                _ => nexterm_i18n::fl!("settings-keybindings-edit-default"),
            },
        };
        draw_section_header(
            &header,
            content_inner_x,
            key_fields_top(sp, content_top, cell_h),
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
    };
    let mut sink = WidgetSink {
        bg_verts,
        bg_idx,
        text_verts,
        text_idx,
    };
    for spec in &build_keybindings_widgets(sp, &geometry) {
        draw_widget(spec, &theme, font, atlas, queue, &mut sink);
    }

    // Hint + warning lines hang off the bottom of the leader-key row.
    let leader_y = key_leader_y(sp, content_top, cell_h);
    let muted = ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST);
    let hint = if sp.leader_key_editing.is_some() {
        nexterm_i18n::fl!("settings-hint-confirm-cancel")
    } else {
        nexterm_i18n::fl!("settings-hint-edit-idle")
    };
    add_string_verts(
        &hint,
        content_inner_x,
        leader_y + cell_h * 1.3,
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

    // P3 (WT-like UX): duplicate-chord warning. When the selected binding's
    // key is also assigned to another binding, surface it right under the hint
    // line so a Record-mode capture gets immediate feedback. Warn-only by
    // design — duplicates stay saveable (the first match wins at dispatch
    // time), matching Windows Terminal's non-blocking warning.
    if let Some(other) = sp.selected_key_conflict() {
        let warn = format!(
            "⚠ {}",
            nexterm_i18n::fl!("settings-key-conflict", action = other.action.clone())
        );
        add_string_verts(
            &warn,
            content_inner_x,
            leader_y + cell_h * 2.5,
            ensure_readable(tokens.semantic_warning, tokens.surface_2, MIN_TEXT_CONTRAST),
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

    if sp.key_delete_dialog_open && !sp.keybindings.is_empty() {
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
