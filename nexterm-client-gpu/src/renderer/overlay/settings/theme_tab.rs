//! Theme category: color-scheme picker (label + value) and preview dots.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{add_px_rect, add_string_verts, truncate_to_cols};

use super::layout::compute_row_layout;
use super::row::{MIN_TEXT_CONTRAST, ensure_readable, search_label_color};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_theme_tab(
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
    let focus = sp.theme_field_focus;

    // Color scheme (label + value). Phase B4-P2: highlighted when
    // `theme_field_focus == 0` (the default, so pre-existing behavior is
    // visually unchanged when the panel first opens on this category).
    let scheme_row_y = content_top + cell_h * 1.0;
    let scheme_color = if focus == 0 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    if focus == 0 {
        add_px_rect(
            content_inner_x - cell_w * 0.3,
            scheme_row_y - cell_h * 0.1,
            content_w - cell_w * 0.7,
            cell_h * 1.2,
            tokens.surface_2,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
    }
    let label = crate::vertex_util::truncate_to_width(
        &nexterm_i18n::fl!("settings-theme-label"),
        layout.label_w,
        cell_w,
    );
    add_string_verts(
        &label,
        content_inner_x,
        scheme_row_y,
        search_label_color(sp, &label, scheme_color, tokens),
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
    let value = format!("{}  (←/→)", sp.scheme_name());
    let value = crate::vertex_util::truncate_to_width(&value, layout.control_w, cell_w);
    add_string_verts(
        &value,
        content_inner_x + layout.control_x_off,
        scheme_row_y,
        scheme_color,
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

    // Scheme preview dots (9 entries). Geometry (`dot_y` / `dot_gap` /
    // `dot_size` / `content_inner_x`) mirrors
    // `settings_panel_hit.rs::hit_test_settings_panel` exactly — keep both
    // in sync.
    let dot_y = content_top + cell_h * 2.5;
    let scheme_names = [
        "dark",
        "light",
        "tokyonight",
        "solarized",
        "gruvbox",
        "catppuccin",
        "dracula",
        "nord",
        "onedark",
    ];
    let schemes_colors: [[f32; 4]; 9] = [
        [0.15, 0.15, 0.18, 1.0],
        [0.95, 0.95, 0.92, 1.0],
        [0.10, 0.10, 0.20, 1.0],
        [0.00, 0.17, 0.21, 1.0],
        [0.28, 0.26, 0.22, 1.0],
        [0.19, 0.17, 0.23, 1.0],
        [0.16, 0.13, 0.23, 1.0],
        [0.18, 0.20, 0.25, 1.0],
        [0.16, 0.18, 0.22, 1.0],
    ];
    let dot_size = cell_w * 1.2;
    let dot_gap = (content_w - cell_w * 2.0) / 9.0;
    for (i, (&col, name)) in schemes_colors.iter().zip(scheme_names.iter()).enumerate() {
        let dot_x = content_inner_x + i as f32 * dot_gap;
        let is_sel = sp.scheme_index == i;
        if is_sel {
            add_px_rect(
                dot_x - 2.0,
                dot_y - 2.0,
                dot_size + 4.0,
                cell_h + 4.0,
                tokens.accent_primary,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }
        add_px_rect(
            dot_x, dot_y, dot_size, cell_h, col, sw, sh, bg_verts, bg_idx,
        );
        let name_y = dot_y + cell_h * 1.3;
        // The dot slot itself bounds the label width (dot_gap, minus a hair
        // of padding), so a long scheme name never bleeds into the next dot.
        let short = truncate_to_cols(name, ((dot_gap / cell_w).floor() as usize).max(1));
        add_string_verts(
            &short,
            dot_x,
            name_y,
            if is_sel {
                tokens.text_secondary
            } else {
                ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST)
            },
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

    // ===== Phase B4-P2: colors_follow_system toggle (theme_field_focus == 1) =====
    let follow_row_y = dot_y + cell_h * 2.8;
    let follow_color = if focus == 1 {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    if focus == 1 {
        add_px_rect(
            content_inner_x - cell_w * 0.3,
            follow_row_y - cell_h * 0.1,
            content_w - cell_w * 0.7,
            cell_h * 1.2,
            tokens.surface_2,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
    }
    let follow_toggle = if sp.colors_follow_system {
        "[ON ]"
    } else {
        "[OFF]"
    };
    let follow_label = crate::vertex_util::truncate_to_width(
        &nexterm_i18n::fl!("settings-theme-follow-system", toggle = follow_toggle),
        layout.label_w + layout.control_w,
        cell_w,
    );
    add_string_verts(
        &follow_label,
        content_inner_x,
        follow_row_y,
        search_label_color(sp, &follow_label, follow_color, tokens),
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
}
