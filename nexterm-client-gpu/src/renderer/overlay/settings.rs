//! Vertex builder for the settings panel (Ctrl+,).

use super::util::draw_overlay_panel;
use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::super::WgpuState;

impl WgpuState {
    /// Build vertices for the settings panel (opens with Ctrl+,)
    ///
    /// Displays the panel for tab 0=Font, 1=Colors, 2=Window.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_settings_panel_verts(
        &self,
        state: &ClientState,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        use crate::settings_panel::SettingsCategory;

        let sp = &state.settings_panel;
        if !sp.is_open {
            return;
        }

        // Open/close animation: smoothly via ease-out cubic
        let eased = sp.eased_progress();

        // Panel size (with left sidebar)
        let panel_w = (sw * 0.72).min(sw - cell_w * 4.0);
        let panel_h = (sh * 0.75).min(sh - cell_h * 4.0);
        let px = (sw - panel_w) / 2.0;
        // Slide-up: start 16px below and ease into the resting position
        let slide_offset = (1.0 - eased) * 16.0;
        let py = (sh - panel_h) / 2.0 + slide_offset;

        // Sidebar width / content area (reserve 18 cells to fit Japanese category names)
        let sidebar_w = cell_w * 18.0;
        let content_x = px + sidebar_w;
        let content_w = panel_w - sidebar_w;

        // Panel chrome: drop-shadow + border ring + rounded background via shared helper.
        draw_overlay_panel(
            px, py, panel_w, panel_h, tokens, 4.0, 6.0, sw, sh, bg_verts, bg_idx,
        );

        // Title bar (tokens.surface_3, opaque)
        let title_h = cell_h * 1.4;
        add_px_rect(
            px,
            py,
            panel_w,
            title_h,
            tokens.surface_3,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Top accent line of the title bar (3px, accent_primary)
        let ap = tokens.accent_primary;
        add_px_rect(px, py, panel_w, 3.0, ap, sw, sh, bg_verts, bg_idx);
        // Inner 1px faint glow
        add_px_rect(
            px,
            py + 3.0,
            panel_w,
            1.0,
            [ap[0], ap[1], ap[2], 0.25],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Title
        add_string_verts(
            " * Nexterm Settings",
            px + cell_w * 0.5,
            py + cell_h * 0.2,
            tokens.text_secondary,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
        // Close-button hint
        let close_text = "Esc";
        let close_x = px + panel_w - close_text.len() as f32 * cell_w - cell_w;
        add_string_verts(
            close_text,
            close_x,
            py + cell_h * 0.2,
            tokens.accent_primary,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Sidebar background (tokens.surface_1, slightly darker than the panel)
        let sidebar_top = py + title_h;
        let sidebar_h = panel_h - title_h - cell_h * 1.5;
        add_px_rect(
            px,
            sidebar_top,
            sidebar_w,
            sidebar_h,
            tokens.surface_1,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Sidebar separator (faint accent color)
        let ap = tokens.accent_primary;
        add_px_rect(
            px + sidebar_w,
            sidebar_top,
            1.0,
            sidebar_h,
            [ap[0], ap[1], ap[2], 0.30],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Sidebar category list
        let cat_item_h = cell_h * 1.3;
        for (i, cat) in SettingsCategory::ALL.iter().enumerate() {
            let item_y = sidebar_top + i as f32 * cat_item_h + cell_h * 0.3;
            let is_active = &sp.category == cat;
            if is_active {
                // Active item: token-driven selection background
                add_px_rect(
                    px,
                    item_y - cell_h * 0.15,
                    sidebar_w,
                    cat_item_h,
                    tokens.tab_active_bg,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // Left-edge indicator (3px + faint inner 1px)
                let ap = tokens.accent_primary;
                add_px_rect(
                    px,
                    item_y - cell_h * 0.15,
                    3.0,
                    cat_item_h,
                    ap,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                add_px_rect(
                    px + 3.0,
                    item_y - cell_h * 0.15,
                    1.0,
                    cat_item_h,
                    [ap[0], ap[1], ap[2], 0.35],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let label = format!("  {} {}", cat.icon(), cat.label());
            let fg = if is_active {
                tokens.text_primary
            } else {
                tokens.text_secondary
            };
            add_string_verts(
                &label,
                px + cell_w * 0.5,
                item_y,
                fg,
                is_active,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // Content area
        let content_top = py + title_h + cell_h * 0.5;
        let content_inner_x = content_x + cell_w;

        match &sp.category {
            SettingsCategory::Font => {
                // Font family
                let family_cursor = if sp.font_family_editing { "|" } else { "" };
                let family_line = format!("Family:  {}{}", sp.font_family, family_cursor);
                if sp.font_family_editing {
                    let field_w = content_w - cell_w * 2.0;
                    add_px_rect(
                        content_inner_x,
                        content_top + cell_h * 1.0,
                        field_w,
                        cell_h,
                        tokens.surface_2,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                add_string_verts(
                    &family_line,
                    content_inner_x,
                    content_top + cell_h * 1.0,
                    tokens.text_secondary,
                    sp.font_family_editing,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let hint = if sp.font_family_editing {
                    "(Enter=confirm  Esc=cancel)"
                } else {
                    "(press F to edit)"
                };
                add_string_verts(
                    hint,
                    content_inner_x,
                    content_top + cell_h * 1.9,
                    tokens.text_muted,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // Font size
                let size_line = format!("Size:    {:.1}pt", sp.font_size);
                add_string_verts(
                    &size_line,
                    content_inner_x,
                    content_top + cell_h * 3.0,
                    tokens.text_secondary,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // Size bar (8 - 32pt)
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
                    "(↑/↓ to change)",
                    content_inner_x,
                    content_top + cell_h * 4.8,
                    tokens.text_muted,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
            SettingsCategory::Theme => {
                // Color scheme
                let scheme_line = format!("Theme:  {}  (←/→)", sp.scheme_name());
                add_string_verts(
                    &scheme_line,
                    content_inner_x,
                    content_top + cell_h * 1.0,
                    tokens.text_secondary,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // Scheme preview dots (9 entries)
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
                for (i, (&col, name)) in schemes_colors.iter().zip(scheme_names.iter()).enumerate()
                {
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
                    let short = &name[..3.min(name.len())];
                    add_string_verts(
                        short,
                        dot_x,
                        name_y,
                        if is_sel {
                            tokens.text_secondary
                        } else {
                            tokens.text_muted
                        },
                        is_sel,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                }
            }
            SettingsCategory::Window => {
                // Phase 5-11-6 #6: the Window category has 5 fields:
                //   row 0=opacity / row 1=cursor_style / row 2=padding_x / row 3=padding_y / row 4=present_mode
                // The focused field renders with a highlight rect and brighter value label.
                // Controls:
                //   ↑/↓ = move between fields (input_handler)
                //   ←/→ = change value (input_handler)

                let focus = sp.window_field_focus;
                let bar_w = content_w - cell_w * 3.0;
                // Vertical position per row (label + control offset together)
                let row_h = cell_h * 3.2; // height of one row (label + control)
                let labels_top = content_top + cell_h * 0.6;

                // ===== Help text (at the top) =====
                add_string_verts(
                    "↑/↓ to select field, ←/→ to change value",
                    content_inner_x,
                    content_top,
                    tokens.text_muted,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // ===== Row 0: opacity (slider) =====
                let row0_y = labels_top + row_h * 0.0;
                if focus == 0 {
                    add_px_rect(
                        content_inner_x - cell_w * 0.3,
                        row0_y - cell_h * 0.1,
                        content_w - cell_w * 0.7,
                        cell_h * 3.0,
                        tokens.surface_2,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                let opacity_color = if focus == 0 {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                };
                let opacity_line = format!("Opacity:  {:.0}%", sp.opacity * 100.0);
                add_string_verts(
                    &opacity_line,
                    content_inner_x,
                    row0_y,
                    opacity_color,
                    focus == 0,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let bar_y = row0_y + cell_h * 1.4;
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
                add_px_rect(
                    content_inner_x,
                    bar_y,
                    bar_w * sp.opacity,
                    cell_h * 0.35,
                    tokens.accent_primary,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );

                // ===== Row 1: cursor style (cycle) =====
                let row1_y = labels_top + row_h * 1.0;
                if focus == 1 {
                    add_px_rect(
                        content_inner_x - cell_w * 0.3,
                        row1_y - cell_h * 0.1,
                        content_w - cell_w * 0.7,
                        cell_h * 3.0,
                        tokens.surface_2,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                let cs_label_color = if focus == 1 {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                };
                add_string_verts(
                    "Cursor style:",
                    content_inner_x,
                    row1_y,
                    cs_label_color,
                    focus == 1,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let cs_value = format!("< {} >", sp.cursor_style_label());
                add_string_verts(
                    &cs_value,
                    content_inner_x + cell_w * 16.0,
                    row1_y,
                    cs_label_color,
                    focus == 1,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // ===== Row 2: horizontal padding =====
                let row2_y = labels_top + row_h * 2.0;
                if focus == 2 {
                    add_px_rect(
                        content_inner_x - cell_w * 0.3,
                        row2_y - cell_h * 0.1,
                        content_w - cell_w * 0.7,
                        cell_h * 3.0,
                        tokens.surface_2,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                let px_color = if focus == 2 {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                };
                add_string_verts(
                    &format!("Horizontal padding:  {} px", sp.padding_x),
                    content_inner_x,
                    row2_y,
                    px_color,
                    focus == 2,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // Mini slider (0 - 32)
                let px_bar_y = row2_y + cell_h * 1.4;
                let px_bar_w = bar_w * 0.6;
                add_px_rect(
                    content_inner_x,
                    px_bar_y,
                    px_bar_w,
                    cell_h * 0.25,
                    tokens.surface_1,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                add_px_rect(
                    content_inner_x,
                    px_bar_y,
                    px_bar_w * (sp.padding_x as f32 / 32.0),
                    cell_h * 0.25,
                    tokens.accent_primary,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );

                // ===== Row 3: vertical padding =====
                let row3_y = labels_top + row_h * 3.0;
                if focus == 3 {
                    add_px_rect(
                        content_inner_x - cell_w * 0.3,
                        row3_y - cell_h * 0.1,
                        content_w - cell_w * 0.7,
                        cell_h * 3.0,
                        tokens.surface_2,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                let py_color = if focus == 3 {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                };
                add_string_verts(
                    &format!("Vertical padding:  {} px", sp.padding_y),
                    content_inner_x,
                    row3_y,
                    py_color,
                    focus == 3,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let py_bar_y = row3_y + cell_h * 1.4;
                let py_bar_w = bar_w * 0.6;
                add_px_rect(
                    content_inner_x,
                    py_bar_y,
                    py_bar_w,
                    cell_h * 0.25,
                    tokens.surface_1,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                add_px_rect(
                    content_inner_x,
                    py_bar_y,
                    py_bar_w * (sp.padding_y as f32 / 32.0),
                    cell_h * 0.25,
                    tokens.accent_primary,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );

                // ===== Row 4: present mode (cycle) =====
                let row4_y = labels_top + row_h * 4.0;
                if focus == 4 {
                    add_px_rect(
                        content_inner_x - cell_w * 0.3,
                        row4_y - cell_h * 0.1,
                        content_w - cell_w * 0.7,
                        cell_h * 3.0,
                        tokens.surface_2,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                let pm_color = if focus == 4 {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                };
                add_string_verts(
                    "Present mode:",
                    content_inner_x,
                    row4_y,
                    pm_color,
                    focus == 4,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let pm_value = format!("< {} >", sp.present_mode_label());
                add_string_verts(
                    &pm_value,
                    content_inner_x + cell_w * 16.0,
                    row4_y,
                    pm_color,
                    focus == 4,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
            SettingsCategory::Profiles => {
                add_string_verts(
                    "Profiles:",
                    content_inner_x,
                    content_top + cell_h * 0.5,
                    tokens.text_secondary,
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                if sp.profiles.is_empty() {
                    add_string_verts(
                        "No profiles configured",
                        content_inner_x,
                        content_top + cell_h * 1.8,
                        tokens.text_muted,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                    add_string_verts(
                        "Add [[profiles]] to nexterm.toml",
                        content_inner_x,
                        content_top + cell_h * 2.7,
                        tokens.text_muted,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                } else {
                    for (i, prof) in sp.profiles.iter().enumerate() {
                        let item_y = content_top + cell_h * (1.5 + i as f32 * 1.2);
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
                        let fg = if is_sel {
                            tokens.text_secondary
                        } else {
                            tokens.text_muted
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
                            &self.queue,
                            text_verts,
                            text_idx,
                        );
                    }
                }
            }
            SettingsCategory::Startup => {
                use crate::settings_panel::LANGUAGE_OPTIONS;

                // Language selection label
                add_string_verts(
                    "Language",
                    content_inner_x,
                    content_top + cell_h * 0.5,
                    tokens.text_secondary,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
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
                let lang_text = format!("< {} >", lang_label);
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
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // Update check toggle
                let check_label = "Check for updates on startup";
                let check_y = content_top + cell_h * 3.0;
                add_string_verts(
                    check_label,
                    content_inner_x,
                    check_y,
                    tokens.text_secondary,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
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
                    tokens.text_muted
                };
                add_string_verts(
                    toggle_str,
                    content_inner_x + check_label.len() as f32 * cell_w * 0.6 + cell_w,
                    check_y,
                    toggle_color,
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // Note that the change takes effect at next startup
                add_string_verts(
                    "* Language change takes effect at next startup",
                    content_inner_x,
                    content_top + cell_h * 4.4,
                    tokens.text_muted,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
            SettingsCategory::Ssh => {
                // Phase 5-11-8 Step 8-1: Render the SSH host list as a ListBox (read-only)
                add_string_verts(
                    "SSH hosts:",
                    content_inner_x,
                    content_top + cell_h * 0.5,
                    tokens.text_secondary,
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                if sp.ssh_hosts.is_empty() {
                    add_string_verts(
                        "No SSH hosts configured",
                        content_inner_x,
                        content_top + cell_h * 1.8,
                        tokens.text_muted,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                    add_string_verts(
                        "Add an entry under the [[hosts]] section in nexterm.toml",
                        content_inner_x,
                        content_top + cell_h * 2.7,
                        tokens.text_muted,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
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
                        let label = host.label();
                        let fg = if is_sel {
                            tokens.text_secondary
                        } else {
                            tokens.text_muted
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
                            &self.queue,
                            text_verts,
                            text_idx,
                        );
                    }
                    // ===== Phase 5-11-8 Step 8-2: Field-edit UI for the selected host =====
                    let sel = sp.selected_host_index.min(sp.ssh_hosts.len() - 1);
                    let host = &sp.ssh_hosts[sel];
                    let fields_top =
                        content_top + cell_h * (1.5 + sp.ssh_hosts.len() as f32 * 1.2 + 0.6);

                    // Section title
                    add_string_verts(
                        "Edit selected host (screen readers can use SetValue):",
                        content_inner_x,
                        fields_top,
                        tokens.text_secondary,
                        true,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );

                    // Labels and current values for the 5 fields.
                    // Phase 5-11-8 Step 8-3 (Sub-phase A): while editing name/host/username,
                    // show the buffer contents and overlay a cursor bar.
                    // Phase 5-11-8 Step 8-3 (Sub-phase C): port is rendered as a SpinButton-style
                    // `< {value} >`, auth_type as a ComboBox-style `< {value} >`.
                    // Both can be changed at any time (no edit mode required; use ←/→ to
                    // increment/decrement or cycle).
                    let editing_focus = sp.ssh_field_editing.as_ref().map(|_| sp.ssh_field_focus);
                    let field_labels: [(&str, String, u8); 5] = [
                        ("name      :", host.name.clone(), 1),
                        ("host      :", host.host.clone(), 2),
                        ("port      :", host.port.to_string(), 3),
                        ("username  :", host.username.clone(), 4),
                        ("auth_type :", host.auth_type.clone(), 5),
                    ];
                    for (i, (label, raw_value, field_id)) in field_labels.iter().enumerate() {
                        let row_y = fields_top + cell_h * (1.3 + i as f32 * 1.1);
                        let is_focused = sp.ssh_field_focus == *field_id;
                        let is_editing = editing_focus == Some(*field_id);
                        // Sub-phase C: port (3) / auth_type (5) behave like SpinButton / ComboBox
                        let is_spin_or_combo = matches!(*field_id, 3 | 5);

                        // Highlight for the focused row (uses a different tint while editing)
                        if is_focused {
                            let bg_color = if is_editing {
                                // While editing: darker bluish highlight
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
                            tokens.text_muted
                        };

                        // While editing, show the buffer plus IME preedit.
                        // port/auth_type are rendered as SpinButton/ComboBox-style `< value >`.
                        // Other fields show the host's current value as-is.
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

                        let line = format!("  {} {}", label, display_value);
                        add_string_verts(
                            &line,
                            content_inner_x,
                            row_y,
                            fg,
                            is_focused,
                            sw,
                            sh,
                            cell_w,
                            font,
                            atlas,
                            &self.queue,
                            text_verts,
                            text_idx,
                        );

                        // Overlay a cursor bar while editing.
                        // Prefix: "  " (2) + label (11) + " " (1) = 14 character cells wide.
                        // Cursor position: derived from display_cursor() in character units
                        // (CJK widths will be improved later via unicode-width).
                        if is_editing && let Some(state) = sp.ssh_field_editing.as_ref() {
                            const PREFIX_COLS: f32 = 14.0;
                            let cursor_byte = state.display_cursor();
                            let display = state.display_string();
                            let cursor_col = display
                                .get(..cursor_byte.min(display.len()))
                                .map(|s| s.chars().count() as f32)
                                .unwrap_or(0.0);
                            let cursor_x = content_inner_x + cell_w * (PREFIX_COLS + cursor_col);
                            // Thin vertical bar (2px wide)
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
                        "Editing: Enter to confirm / Esc to cancel / ← → to move cursor"
                    } else {
                        "Enter to edit (name/host/username) / ← → to adjust port ±1 / cycle auth_type"
                    };
                    add_string_verts(
                        note_text,
                        content_inner_x,
                        note_y,
                        tokens.text_muted,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                }

                // ===== Phase 5-11-8 Step 8-3 (Sub-phase D): Add / Delete buttons =====
                // For an empty list, place at content_top + 4.0 rows;
                // otherwise, at note_y + 1.5 rows.
                let buttons_y = if sp.ssh_hosts.is_empty() {
                    content_top + cell_h * 4.0
                } else {
                    let sel = sp.selected_host_index.min(sp.ssh_hosts.len() - 1);
                    let _ = sel; // The actual calculation matches fields_top
                    let fields_top =
                        content_top + cell_h * (1.5 + sp.ssh_hosts.len() as f32 * 1.2 + 0.6);
                    let note_y = fields_top + cell_h * (1.3 + 5.0 * 1.1 + 0.4);
                    note_y + cell_h * 1.5
                };
                let add_focused = sp.ssh_field_focus == 6;
                let delete_focused = sp.ssh_field_focus == 7;
                let delete_disabled = sp.ssh_hosts.is_empty();
                let btn_w = cell_w * 24.0;
                let btn_h = cell_h * 1.4;
                let btn_gap = cell_w * 2.0;

                // Add button
                let add_x = content_inner_x;
                if add_focused {
                    add_px_rect(
                        add_x - cell_w * 0.3,
                        buttons_y - cell_h * 0.15,
                        btn_w,
                        btn_h,
                        tokens.surface_2,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                } else {
                    add_px_rect(
                        add_x - cell_w * 0.3,
                        buttons_y - cell_h * 0.15,
                        btn_w,
                        btn_h,
                        tokens.surface_1,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                let add_fg = if add_focused {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                };
                add_string_verts(
                    "[ + ] Add new host",
                    add_x,
                    buttons_y,
                    add_fg,
                    add_focused,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // Delete button (disabled when the list is empty)
                let del_x = add_x + btn_w + btn_gap;
                if delete_focused && !delete_disabled {
                    add_px_rect(
                        del_x - cell_w * 0.3,
                        buttons_y - cell_h * 0.15,
                        btn_w,
                        btn_h,
                        [0.298, 0.149, 0.149, 1.0],
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                } else {
                    add_px_rect(
                        del_x - cell_w * 0.3,
                        buttons_y - cell_h * 0.15,
                        btn_w,
                        btn_h,
                        tokens.surface_1,
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                let del_fg = if delete_disabled {
                    // disabled: light gray
                    tokens.text_muted
                } else if delete_focused {
                    [0.984, 0.808, 0.808, 1.0]
                } else {
                    [0.776, 0.553, 0.553, 1.0]
                };
                let del_label = if delete_disabled {
                    "[ x ] Delete selected host (disabled)"
                } else {
                    "[ x ] Delete selected host"
                };
                add_string_verts(
                    del_label,
                    del_x,
                    buttons_y,
                    del_fg,
                    delete_focused && !delete_disabled,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // ===== Phase 5-11-8 Step 8-3 (Sub-phase D): delete confirmation dialog =====
                // When ssh_delete_dialog_open=true, draw a modal dialog at the center of
                // the panel. Render order (z-order): panel body -> Add/Delete buttons ->
                // dialog -> fade overlay (at the end of settings_panel).
                if sp.ssh_delete_dialog_open && !sp.ssh_hosts.is_empty() {
                    let sel = sp.selected_host_index.min(sp.ssh_hosts.len() - 1);
                    let target_name = if sp.ssh_hosts[sel].name.is_empty() {
                        sp.ssh_hosts[sel].host.clone()
                    } else {
                        sp.ssh_hosts[sel].name.clone()
                    };

                    // Semi-transparent overlay covering the entire panel
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

                    // Dialog body (centered within the panel)
                    let dialog_w = panel_w * 0.55;
                    let dialog_h = cell_h * 8.5;
                    let dialog_x = px + (panel_w - dialog_w) / 2.0;
                    let dialog_y = py + (panel_h - dialog_h) / 2.0;

                    // Dialog background (opaque, with a warning-color accent)
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

                    // Title
                    add_string_verts(
                        " ! Delete this host?",
                        dialog_x + cell_w * 1.0,
                        dialog_y + cell_h * 0.6,
                        [0.984, 0.808, 0.808, 1.0],
                        true,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );

                    // Message
                    let msg = format!(
                        "\"{}\" will be deleted. This operation cannot be undone.",
                        target_name
                    );
                    add_string_verts(
                        &msg,
                        dialog_x + cell_w * 1.0,
                        dialog_y + cell_h * 2.2,
                        tokens.text_secondary,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );

                    // Cancel / Confirm buttons (side by side; Cancel is on the left and has default focus)
                    let dlg_btn_w = cell_w * 14.0;
                    let dlg_btn_h = cell_h * 1.4;
                    let dlg_btn_gap = cell_w * 2.0;
                    let dlg_btns_total_w = dlg_btn_w * 2.0 + dlg_btn_gap;
                    let dlg_btns_x = dialog_x + (dialog_w - dlg_btns_total_w) / 2.0;
                    let dlg_btns_y = dialog_y + dialog_h - cell_h * 2.5;
                    let confirm_focused = sp.ssh_delete_dialog_confirm_focused;

                    // Cancel button
                    let cancel_bg = if !confirm_focused {
                        tokens.surface_3
                    } else {
                        tokens.surface_1
                    };
                    add_px_rect(
                        dlg_btns_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, cancel_bg, sw, sh, bg_verts,
                        bg_idx,
                    );
                    add_string_verts(
                        "  Cancel (Esc)",
                        dlg_btns_x + cell_w * 0.5,
                        dlg_btns_y + cell_h * 0.2,
                        tokens.text_primary,
                        !confirm_focused,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );

                    // Confirm button
                    let confirm_bg = if confirm_focused {
                        [0.498, 0.196, 0.196, 1.0]
                    } else {
                        [0.235, 0.118, 0.118, 1.0]
                    };
                    let confirm_x = dlg_btns_x + dlg_btn_w + dlg_btn_gap;
                    add_px_rect(
                        confirm_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, confirm_bg, sw, sh, bg_verts,
                        bg_idx,
                    );
                    add_string_verts(
                        "  Delete",
                        confirm_x + cell_w * 0.5,
                        dlg_btns_y + cell_h * 0.2,
                        [0.984, 0.808, 0.808, 1.0],
                        confirm_focused,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );

                    // Operation hint
                    add_string_verts(
                        "  Use <- -> / Tab to switch buttons / Enter to confirm / Esc to cancel",
                        dialog_x + cell_w * 1.0,
                        dialog_y + dialog_h - cell_h * 0.9,
                        tokens.text_muted,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                }
            }
            SettingsCategory::Keybindings => {
                // Phase 5-11-9 Sub-phase A: render the key binding list as a ListBox.
                // Sub-phase B/C/D add Record-mode capture, Action ComboBox, and Add/Delete.
                add_string_verts(
                    "Key bindings:",
                    content_inner_x,
                    content_top + cell_h * 0.5,
                    tokens.text_secondary,
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                if sp.keybindings.is_empty() {
                    add_string_verts(
                        "No key bindings configured",
                        content_inner_x,
                        content_top + cell_h * 1.8,
                        tokens.text_muted,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                } else {
                    // ListBox: one row per binding. Sub-phase A is display-only;
                    // Sub-phase B+ adds inline edit.
                    let max_rows = sp.keybindings.len().min(12); // cap rows for layout
                    for (i, kb) in sp.keybindings.iter().take(max_rows).enumerate() {
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
                        let label = kb.label();
                        let fg = if is_sel {
                            tokens.text_secondary
                        } else {
                            tokens.text_muted
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
                            &self.queue,
                            text_verts,
                            text_idx,
                        );
                    }
                    if sp.keybindings.len() > max_rows {
                        let more_y = content_top + cell_h * (1.5 + max_rows as f32 * 1.2);
                        add_string_verts(
                            &format!("... ({} more)", sp.keybindings.len() - max_rows),
                            content_inner_x,
                            more_y,
                            tokens.text_muted,
                            false,
                            sw,
                            sh,
                            cell_w,
                            font,
                            atlas,
                            &self.queue,
                            text_verts,
                            text_idx,
                        );
                    }

                    // Edit fields for the selected binding.
                    // Phase 5-11-9 Sub-phase B enables editing the key field
                    // via Record / Text modes. Action editing arrives in
                    // Sub-phase C (ComboBox).
                    use crate::settings_panel::KeyEditMode;
                    let sel = sp.selected_key_index.min(sp.keybindings.len() - 1);
                    let kb = &sp.keybindings[sel];
                    let visible_rows = sp.keybindings.len().min(max_rows) as f32;
                    let fields_top = content_top + cell_h * (1.5 + visible_rows * 1.2 + 1.4);
                    let header = match &sp.key_editing {
                        Some(KeyEditMode::Record) => {
                            "Edit selected binding (Recording: press any key, Tab = Text, Esc = cancel):"
                        }
                        Some(KeyEditMode::Text(_)) => {
                            "Edit selected binding (Text: Enter = commit, Tab = Record, Esc = cancel):"
                        }
                        None => {
                            // Phase 5-11-9 Sub-phase C: tailor the hint to the focused field.
                            match sp.key_field_focus {
                                1 => {
                                    "Edit selected binding (Enter on key = record; ↑/↓ to switch field):"
                                }
                                2 => {
                                    "Edit selected binding (←/→ to cycle action; ↑/↓ to switch field):"
                                }
                                _ => {
                                    "Edit selected binding (↑/↓ to focus key or action; Enter on key = record):"
                                }
                            }
                        }
                    };
                    add_string_verts(
                        header,
                        content_inner_x,
                        fields_top,
                        tokens.text_secondary,
                        true,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                    // Show the in-flight Text buffer when editing; otherwise the stored value.
                    let key_display: String = match &sp.key_editing {
                        Some(KeyEditMode::Record) => "<press a key>".to_string(),
                        Some(KeyEditMode::Text(state)) => {
                            // Concatenate buffer + preedit so IME composition is visible.
                            let mut s = state.buffer.clone();
                            if let Some(pre) = state.preedit.as_ref() {
                                s.push_str(pre);
                            }
                            s
                        }
                        None => kb.key.clone(),
                    };
                    // Phase 5-11-9 Sub-phase C: render the action together with its
                    // position in `KEYBINDING_ACTIONS` (or `(unknown)` when the
                    // configured value is not in the fixed list).
                    use crate::settings_panel::KEYBINDING_ACTIONS;
                    let action_display: String = {
                        let actions = KEYBINDING_ACTIONS;
                        match actions.iter().position(|&a| a == kb.action) {
                            Some(i) => format!("{} ({}/{})", kb.action, i + 1, actions.len()),
                            None => format!("{} (unknown)", kb.action),
                        }
                    };
                    let field_labels: [(&str, &str, u8); 2] = [
                        ("key    :", key_display.as_str(), 1),
                        ("action :", action_display.as_str(), 2),
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
                            "(empty)".to_string()
                        } else {
                            (*raw_value).to_string()
                        };
                        let line = format!("{}  {}", label, display);
                        // Phase 5-11-9 Sub-phase C: highlight an unknown / typo'd
                        // action in red regardless of focus state, so the user
                        // sees the validation hit even without inspecting the
                        // hint header.
                        let action_invalid = *field_id == 2 && !sp.selected_key_action_is_valid();
                        let fg = if action_invalid {
                            tokens.semantic_error
                        } else if is_focused {
                            tokens.text_secondary
                        } else {
                            tokens.text_muted
                        };
                        add_string_verts(
                            &line,
                            content_inner_x,
                            row_y,
                            fg,
                            is_focused,
                            sw,
                            sh,
                            cell_w,
                            font,
                            atlas,
                            &self.queue,
                            text_verts,
                            text_idx,
                        );
                    }
                }

                // ===== Phase 5-11-9 Sub-phase D: Add / Delete buttons =====
                // Anchor position: when the list is empty, place buttons near
                // the top of the content area (Empty hint already occupies
                // row 1.8). Otherwise, anchor below the edit fields.
                let key_buttons_y = if sp.keybindings.is_empty() {
                    content_top + cell_h * 4.0
                } else {
                    let max_rows = sp.keybindings.len().min(12);
                    let visible_rows = sp.keybindings.len().min(max_rows) as f32;
                    let fields_top = content_top + cell_h * (1.5 + visible_rows * 1.2 + 1.4);
                    // 2 edit field rows (key + action) below fields_top header.
                    let last_field_y = fields_top + cell_h * (1.3 + 1.0 * 1.1);
                    last_field_y + cell_h * 2.0
                };
                let key_add_focused = sp.key_field_focus == 3;
                let key_delete_focused = sp.key_field_focus == 4;
                let key_delete_disabled = sp.keybindings.is_empty();
                let key_btn_w = cell_w * 26.0;
                let key_btn_h = cell_h * 1.4;
                let key_btn_gap = cell_w * 2.0;

                // Add button
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
                    "[ + ] Add new key binding",
                    key_add_x,
                    key_buttons_y,
                    key_add_fg,
                    key_add_focused,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // Delete button (disabled when the list is empty)
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
                    tokens.text_muted
                } else if key_delete_focused {
                    [0.984, 0.808, 0.808, 1.0]
                } else {
                    [0.776, 0.553, 0.553, 1.0]
                };
                let key_del_label = if key_delete_disabled {
                    "[ x ] Delete selected binding (disabled)"
                } else {
                    "[ x ] Delete selected binding"
                };
                add_string_verts(
                    key_del_label,
                    key_del_x,
                    key_buttons_y,
                    key_del_fg,
                    key_delete_focused && !key_delete_disabled,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // ===== Phase 5-11-9 Sub-phase D: delete-confirmation dialog =====
                if sp.key_delete_dialog_open && !sp.keybindings.is_empty() {
                    let sel = sp.selected_key_index.min(sp.keybindings.len() - 1);
                    let target = sp.keybindings[sel].label();

                    // Backdrop
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
                        " ! Delete this key binding?",
                        dialog_x + cell_w,
                        dialog_y + cell_h * 0.6,
                        [0.984, 0.808, 0.808, 1.0],
                        true,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                    let msg = format!(
                        "\"{}\" will be deleted. This operation cannot be undone.",
                        target
                    );
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
                        &self.queue,
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

                    // Cancel button (left, default focus)
                    let cancel_bg = if !confirm_focused {
                        tokens.surface_3
                    } else {
                        tokens.surface_1
                    };
                    add_px_rect(
                        dlg_btns_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, cancel_bg, sw, sh, bg_verts,
                        bg_idx,
                    );
                    let cancel_fg = if !confirm_focused {
                        tokens.text_primary
                    } else {
                        tokens.text_secondary
                    };
                    add_string_verts(
                        "[ Cancel (Esc) ]",
                        dlg_btns_x + cell_w * 1.0,
                        dlg_btns_y + cell_h * 0.2,
                        cancel_fg,
                        !confirm_focused,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );

                    // Confirm button (right)
                    let confirm_x = dlg_btns_x + dlg_btn_w + dlg_btn_gap;
                    let confirm_bg = if confirm_focused {
                        [0.486, 0.180, 0.180, 1.0]
                    } else {
                        tokens.surface_1
                    };
                    add_px_rect(
                        confirm_x, dlg_btns_y, dlg_btn_w, dlg_btn_h, confirm_bg, sw, sh, bg_verts,
                        bg_idx,
                    );
                    let confirm_fg = if confirm_focused {
                        [0.984, 0.808, 0.808, 1.0]
                    } else {
                        [0.776, 0.553, 0.553, 1.0]
                    };
                    add_string_verts(
                        "[ Delete (Enter) ]",
                        confirm_x + cell_w * 1.0,
                        dlg_btns_y + cell_h * 0.2,
                        confirm_fg,
                        confirm_focused,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                }
            }
        }

        // Bottom bar (Save / Cancel)
        let bottom_y = py + panel_h - cell_h * 1.5;
        add_px_rect(
            px,
            bottom_y,
            panel_w,
            1.0,
            tokens.surface_1,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_px_rect(
            px,
            bottom_y + 1.0,
            panel_w,
            cell_h * 1.5 - 1.0,
            tokens.surface_0,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_string_verts(
            "  Enter=Save  Esc=Cancel  Tab=Next category",
            px + cell_w * 0.5,
            bottom_y + cell_h * 0.3,
            tokens.text_muted,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Fade-in overlay: same color as the panel; becomes transparent as open_progress advances.
        // When eased=1.0, no overlay is drawn (fully visible).
        let fade_alpha = (1.0 - eased) * 0.95;
        if fade_alpha > 0.01 {
            add_px_rect(
                px - 1.0,
                py - 1.0,
                panel_w + 2.0,
                panel_h + 2.0,
                {
                    let [r, g, b, _] = tokens.surface_0;
                    [r, g, b, fade_alpha]
                },
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }
    }
}
