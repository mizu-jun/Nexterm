//! Font category: family (text) + size (slider).

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{add_px_rect, add_string_verts};

use crate::vertex_util::truncate_to_width;

use super::layout::compute_row_layout;
use super::row::{MIN_TEXT_CONTRAST, draw_label_control_row, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_font_tab(
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
    let focus = sp.font_field_focus;

    // Family (label + value; an inline text box replaces the value column
    // while editing). Phase B4-P2: also highlighted when `font_field_focus
    // == 0` (the default, so pre-existing behavior is visually unchanged
    // when the panel first opens on this category).
    let family_row_y = content_top + cell_h * 1.0;
    let family_focused = sp.font_family_editing || focus == 0;
    let family_cursor = if sp.font_family_editing { "|" } else { "" };
    let family_value = format!("{}{}", sp.font_family, family_cursor);
    if sp.font_family_editing {
        add_px_rect(
            content_inner_x + layout.control_x_off,
            family_row_y,
            layout.control_w,
            cell_h,
            tokens.surface_2,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
    }
    // The editing box above already covers the highlight, so the label and
    // value are drawn directly (not via `draw_label_control_row`, which
    // would also paint a full-row highlight and double up on it).
    let family_color = if family_focused {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let family_label = truncate_to_width(
        &nexterm_i18n::fl!("settings-font-family"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &family_label,
        content_inner_x,
        family_row_y,
        family_color,
        sp.font_family_editing,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let family_value_text = truncate_to_width(&family_value, layout.control_w, cell_w);
    add_string_verts(
        &family_value_text,
        content_inner_x + layout.control_x_off,
        family_row_y,
        family_color,
        sp.font_family_editing,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
    let hint = if sp.font_family_editing {
        nexterm_i18n::fl!("settings-hint-confirm-cancel")
    } else {
        nexterm_i18n::fl!("settings-font-hint-press-f")
    };
    add_string_verts(
        &hint,
        content_inner_x,
        content_top + cell_h * 1.9,
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

    // Size (label + value). Phase B4-P2: highlighted when `font_field_focus == 1`.
    let size_row_y = content_top + cell_h * 3.0;
    let size_value = format!("{:.1}pt", sp.font_size);
    let size_focused = focus == 1;
    draw_label_control_row(
        sp,
        tokens,
        content_inner_x,
        size_row_y,
        cell_h * 1.2,
        &layout,
        &nexterm_i18n::fl!("settings-font-size"),
        &size_value,
        size_focused,
        tokens.surface_2,
        tokens.text_secondary,
        tokens.text_secondary,
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

    // Size slider (8 - 32pt). Geometry mirrors
    // `settings_panel_hit.rs::hit_test_settings_panel` exactly — do not
    // change `bar_y` / `bar_w` / `content_inner_x` here without updating it.
    let bar_w = content_w - cell_w * 3.0;
    let bar_y = content_top + cell_h * 4.2;
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
    let fill = ((sp.font_size - 8.0) / 24.0).clamp(0.0, 1.0);
    add_px_rect(
        content_inner_x,
        bar_y,
        bar_w * fill,
        cell_h * 0.35,
        tokens.accent_primary,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    add_string_verts(
        &nexterm_i18n::fl!("settings-font-hint-slider"),
        content_inner_x,
        content_top + cell_h * 4.8,
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

    // ===== Phase B4-P2: ligatures toggle (font_field_focus == 2) =====
    let lig_row_y = content_top + cell_h * 6.0;
    let lig_value = if sp.font_ligatures { "[ON ]" } else { "[OFF]" };
    draw_label_control_row(
        sp,
        tokens,
        content_inner_x,
        lig_row_y,
        cell_h * 1.2,
        &layout,
        &nexterm_i18n::fl!("settings-font-ligatures"),
        lig_value,
        focus == 2,
        tokens.surface_2,
        tokens.text_secondary,
        tokens.text_secondary,
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

    // ===== Phase B4-P2: font_fallbacks text field (font_field_focus == 3) =====
    let fb_row_y = content_top + cell_h * 7.4;
    let fb_editing = sp.font_fallbacks_editing.is_some();
    let fb_display: String = match &sp.font_fallbacks_editing {
        Some(state) => {
            let mut s = state.buffer.clone();
            if let Some(pre) = state.preedit.as_ref() {
                s.push_str(pre);
            }
            format!("{s}|")
        }
        None => sp.font_fallbacks_text.clone(),
    };
    draw_label_control_row(
        sp,
        tokens,
        content_inner_x,
        fb_row_y,
        cell_h * 1.2,
        &layout,
        &nexterm_i18n::fl!("settings-font-fallbacks"),
        &fb_display,
        focus == 3 || fb_editing,
        tokens.surface_2,
        tokens.text_secondary,
        tokens.text_secondary,
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
    let fb_hint = if fb_editing {
        nexterm_i18n::fl!("settings-font-fallbacks-hint-editing")
    } else {
        nexterm_i18n::fl!("settings-font-fallbacks-hint-idle")
    };
    add_string_verts(
        &fb_hint,
        content_inner_x,
        content_top + cell_h * 8.3,
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
