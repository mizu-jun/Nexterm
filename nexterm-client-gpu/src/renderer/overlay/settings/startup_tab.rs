//! Startup category: language picker + update-check toggle.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::{LANGUAGE_OPTIONS, SettingsPanel};
use crate::vertex_util::{add_px_rect, add_string_verts, truncate_to_width};

use super::layout::compute_row_layout;
use super::row::{MIN_TEXT_CONTRAST, draw_label_control_row, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_startup_tab(
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

    // Language selection label
    add_string_verts(
        &nexterm_i18n::fl!("settings-startup-language"),
        content_inner_x,
        content_top + cell_h * 0.5,
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

    // Selection bar background
    let sel_y = content_top + cell_h * 1.6;
    let sel_w = content_w - cell_w * 2.0;
    add_px_rect(
        content_inner_x,
        sel_y,
        sel_w,
        cell_h,
        tokens.surface_2,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // Current language name display
    let lang_label = LANGUAGE_OPTIONS
        .get(sp.language_index)
        .map(|(name, _)| *name)
        .unwrap_or("Auto");
    let lang_text = truncate_to_width(&format!("< {} >", lang_label), sel_w, cell_w);
    add_string_verts(
        &lang_text,
        content_inner_x + cell_w * 0.5,
        sel_y + cell_h * 0.1,
        tokens.text_primary,
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

    // Update check toggle (label + value, two-column)
    let check_row_y = content_top + cell_h * 3.0;
    let check_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-startup-check-updates"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &check_label,
        content_inner_x,
        check_row_y,
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
    let toggle_str = if sp.auto_check_update {
        "[ON ]"
    } else {
        "[OFF]"
    };
    let toggle_color = if sp.auto_check_update {
        tokens.semantic_success
    } else {
        ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST)
    };
    add_string_verts(
        toggle_str,
        content_inner_x + layout.control_x_off,
        check_row_y,
        toggle_color,
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

    // Note that the change takes effect at next startup
    add_string_verts(
        &nexterm_i18n::fl!("settings-startup-language-note"),
        content_inner_x,
        content_top + cell_h * 4.4,
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

    // ===== Phase B4: shell program / args (rows 2/3, startup_field_focus) =====
    let focus = sp.startup_field_focus;

    let shell_program_display = if let (2, Some(state)) = (focus, sp.shell_field_editing.as_ref()) {
        state.display_string()
    } else {
        sp.shell_program.clone()
    };
    let program_row_y = content_top + cell_h * 5.8;
    draw_label_control_row(
        sp,
        tokens,
        content_inner_x,
        program_row_y,
        cell_h * 1.2,
        &layout,
        &nexterm_i18n::fl!("settings-startup-shell-program"),
        &shell_program_display,
        focus == 2,
        tokens.surface_2,
        if focus == 2 {
            tokens.text_primary
        } else {
            tokens.text_secondary
        },
        tokens.text_primary,
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

    let shell_args_display = if let (3, Some(state)) = (focus, sp.shell_field_editing.as_ref()) {
        state.display_string()
    } else {
        sp.shell_args.clone()
    };
    let args_row_y = content_top + cell_h * 7.2;
    draw_label_control_row(
        sp,
        tokens,
        content_inner_x,
        args_row_y,
        cell_h * 1.2,
        &layout,
        &nexterm_i18n::fl!("settings-startup-shell-args"),
        &shell_args_display,
        focus == 3,
        tokens.surface_2,
        if focus == 3 {
            tokens.text_primary
        } else {
            tokens.text_secondary
        },
        tokens.text_primary,
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

    add_string_verts(
        &nexterm_i18n::fl!("settings-hint-edit-commit-cancel"),
        content_inner_x,
        content_top + cell_h * 8.6,
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
