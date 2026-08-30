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
use crate::vertex_util::add_run_verts;

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_keybindings::{
    build_keybindings_widgets, key_fields_top, key_leader_y, key_list_window,
};
use super::delete_dialog::{DeleteDialogView, draw_delete_dialog};
use super::layout::LIST_ROW_PITCH;
use super::row::draw_section_header;
use nexterm_config::SurfaceLevel;

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
    now: std::time::Instant,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    // UI/UX v3 N-4e: the prose this file still owns — the empty
    // state, the list range-indicator and the edit-hint note. All of
    // it is left-aligned at a fixed x and bounds no hit region, so
    // this is typography with no geometry attached. `caption` is the
    // ramp step for secondary metadata, which is what these are.
    let prose_style = nexterm_config::MetricTokens::default().type_ramp.caption;
    draw_section_header(
        &nexterm_i18n::fl!("settings-keybindings-header"),
        content_inner_x,
        content_top + cell_h * 0.5,
        content_w,
        tokens.text_on(SurfaceLevel::S2).secondary,
        sw,
        sh,
        metrics,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    if sp.keybindings.is_empty() {
        add_run_verts(
            &nexterm_i18n::fl!("settings-keybindings-empty"),
            &prose_style,
            content_inner_x,
            content_top + cell_h * 1.8,
            tokens.text_on(SurfaceLevel::S2).muted,
            sw,
            sh,
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
            add_run_verts(
                &nexterm_i18n::fl!(
                    "settings-list-window",
                    from = w.first + 1,
                    to = w.first + w.visible,
                    total = sp.keybindings.len()
                ),
                &prose_style,
                content_inner_x,
                indicator_y,
                tokens.text_on(SurfaceLevel::S2).muted,
                sw,
                sh,
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
            tokens.text_on(SurfaceLevel::S2).secondary,
            sw,
            sh,
            metrics,
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
    for spec in &build_keybindings_widgets(sp, &geometry) {
        draw_widget(spec, &theme, font, atlas, queue, &mut sink);
    }

    // Hint + warning lines hang off the bottom of the leader-key row.
    let leader_y = key_leader_y(sp, content_top, cell_h);
    let muted = tokens.text_on(SurfaceLevel::S2).muted;
    let hint = if sp.leader_key_editing.is_some() {
        nexterm_i18n::fl!("settings-hint-confirm-cancel")
    } else {
        nexterm_i18n::fl!("settings-hint-edit-idle")
    };
    add_run_verts(
        &hint,
        &prose_style,
        content_inner_x,
        leader_y + cell_h * 1.3,
        muted,
        sw,
        sh,
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
        add_run_verts(
            &warn,
            &prose_style,
            content_inner_x,
            leader_y + cell_h * 2.5,
            tokens.text_on(SurfaceLevel::S2).warning,
            sw,
            sh,
            font,
            atlas,
            queue,
            sink.text_verts,
            sink.text_idx,
        );
    }

    if sp.key_delete_dialog_open && !sp.keybindings.is_empty() {
        draw_delete_dialog(
            &delete_dialog_view(sp),
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

/// Build the view the shared modal draws (UI/UX v3 N-4d).
///
/// The modal itself lives in `delete_dialog.rs`; this names only what is
/// specific to this tab. The two copies it replaced had drifted in four
/// places — see that module's header.
fn delete_dialog_view(sp: &SettingsPanel) -> DeleteDialogView {
    let sel = sp.selected_key_index.min(sp.keybindings.len() - 1);
    let target = sp.keybindings[sel].label();
    DeleteDialogView {
        title: nexterm_i18n::fl!("settings-keybindings-delete-title"),
        target,
        confirm_label: nexterm_i18n::fl!("settings-keybindings-delete-confirm"),
        hint: None,
        confirm_focused: sp.key_delete_dialog_confirm_focused,
    }
}
