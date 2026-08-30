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
use crate::vertex_util::add_string_verts;

use super::super::widgets::draw::{WidgetSink, WidgetTheme, draw_widget};
use super::super::widgets::geometry::TabGeometry;
use super::super::widgets::settings_ssh::{
    build_ssh_widgets, ssh_fields_top, ssh_list_window, ssh_note_y,
};
use super::delete_dialog::{DeleteDialogView, draw_delete_dialog};
use super::layout::LIST_ROW_PITCH;
use super::row::draw_section_header;
use nexterm_config::SurfaceLevel;

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

    if sp.ssh_hosts.is_empty() {
        add_string_verts(
            &nexterm_i18n::fl!("settings-ssh-empty"),
            content_inner_x,
            content_top + cell_h * 1.8,
            tokens.text_on(SurfaceLevel::S2).muted,
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
            tokens.text_on(SurfaceLevel::S2).muted,
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
                tokens.text_on(SurfaceLevel::S2).muted,
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

        let note_text = if sp.ssh_field_editing.is_some() {
            nexterm_i18n::fl!("settings-ssh-note-editing")
        } else {
            nexterm_i18n::fl!("settings-ssh-note-idle")
        };
        add_string_verts(
            &note_text,
            content_inner_x,
            ssh_note_y(sp, content_top, cell_h),
            tokens.text_on(SurfaceLevel::S2).muted,
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
    let sel = sp.selected_host_index.min(sp.ssh_hosts.len() - 1);
    let target = if sp.ssh_hosts[sel].name.is_empty() {
        sp.ssh_hosts[sel].host.clone()
    } else {
        sp.ssh_hosts[sel].name.clone()
    };
    DeleteDialogView {
        title: nexterm_i18n::fl!("settings-ssh-delete-title"),
        target,
        confirm_label: nexterm_i18n::fl!("settings-ssh-delete-confirm"),
        hint: Some(nexterm_i18n::fl!("settings-ssh-delete-hint")),
        confirm_focused: sp.ssh_delete_dialog_confirm_focused,
    }
}
