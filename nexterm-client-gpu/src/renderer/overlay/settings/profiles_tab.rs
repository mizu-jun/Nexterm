//! Profiles category: read-only list of `[[profiles]]` entries from
//! `nexterm.toml`.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{add_px_rect, add_string_verts, truncate_to_width};

use super::row::{MIN_TEXT_CONTRAST, draw_section_header, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_profiles_tab(
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
    draw_section_header(
        &nexterm_i18n::fl!("settings-profiles-header"),
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

    // Phase B4: active-profile selector (←/→ cycles `0..=profiles.len()`,
    // where 0 means "no active profile"). Rendered even when the list is
    // empty is unnecessary (there is nothing to activate), so it is skipped
    // in that branch below.
    if !sp.profiles.is_empty() {
        let active_row_y = content_top + cell_h * 1.7;
        let active_label = truncate_to_width(
            &nexterm_i18n::fl!("settings-profiles-active"),
            content_w * 0.4,
            cell_w,
        );
        add_string_verts(
            &active_label,
            content_inner_x,
            active_row_y,
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
        let none_label = nexterm_i18n::fl!("settings-profiles-none");
        let active_value = format!("< {} >", sp.active_profile_name().unwrap_or(&none_label));
        let active_value = truncate_to_width(&active_value, content_w * 0.5, cell_w);
        add_string_verts(
            &active_value,
            content_inner_x + content_w * 0.42,
            active_row_y,
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
    }

    if sp.profiles.is_empty() {
        add_string_verts(
            &nexterm_i18n::fl!("settings-profiles-empty"),
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
            &nexterm_i18n::fl!("settings-profiles-empty-hint"),
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
        return;
    }

    // Phase B4: the active-profile selector row above takes up ~1.2 lines
    // when the list is non-empty, so the list itself starts lower to avoid overlap.
    let list_top = 3.0;
    let label_max_w = content_w - cell_w * 0.7;
    for (i, prof) in sp.profiles.iter().enumerate() {
        let item_y = content_top + cell_h * (list_top + i as f32 * 1.2);
        let is_sel = sp.selected_profile == i;
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
        let label = format!("{} {}", prof.icon, prof.name);
        let label = truncate_to_width(&label, label_max_w, cell_w);
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
}
