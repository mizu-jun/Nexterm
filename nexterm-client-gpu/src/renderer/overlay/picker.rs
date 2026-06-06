//! Vertex builders for picker-style overlays.
//!
//! Handles list-style UI for the command palette / SFTP file transfer /
//! macro picker / host manager.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::super::WgpuState;
use super::util::draw_overlay_panel;

impl WgpuState {
    /// Build vertices for the command palette (center floating)
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_palette_verts(
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
        let palette = &state.palette;
        let items = palette.filtered();
        let palette_cols: f32 = 40.0;
        let palette_rows = (items.len() + 2).min(12) as f32; // query row + up to 10 items + margin

        let pw = palette_cols * cell_w;
        let ph = palette_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome: drop-shadow + border ring + rounded background.
        draw_overlay_panel(px, py, pw, ph, tokens, 5.0, 6.0, sw, sh, bg_verts, bg_idx);
        // Top accent stripe (accent_primary)
        let ap = tokens.accent_primary;
        add_px_rect(px, py, pw, 2.0, ap, sw, sh, bg_verts, bg_idx);

        // Query row
        let query_text = format!("> {}", palette.query);
        add_string_verts(
            &query_text,
            px + cell_w,
            py + cell_h * 0.1,
            [1.0, 1.0, 1.0, 1.0],
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

        // Action list
        for (i, action) in items.iter().enumerate().take(palette_rows as usize - 1) {
            let item_py = py + cell_h * (i as f32 + 1.2);
            if i == palette.selected {
                // Highlight the selected row
                add_px_rect(
                    px + 2.0,
                    item_py,
                    pw - 4.0,
                    cell_h,
                    tokens.surface_2,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let prefix = if i == palette.selected { "> " } else { "  " };
            let label = format!("{}{}", prefix, action.label);
            let fg = if i == palette.selected {
                tokens.text_primary
            } else {
                tokens.text_muted
            };
            add_string_verts(
                &label,
                px + cell_w,
                item_py,
                fg,
                i == palette.selected,
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

    /// Build vertices for the SFTP file-transfer dialog
    ///
    /// Three fields: host name / local path / remote path.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_file_transfer_verts(
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
        let ft = &state.file_transfer;
        let panel_cols: f32 = 56.0;
        let panel_rows: f32 = 7.0; // title + host + local + remote + hint

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome: drop-shadow + border ring + rounded background.
        draw_overlay_panel(px, py, pw, ph, tokens, 5.0, 6.0, sw, sh, bg_verts, bg_idx);
        // Top accent stripe — upload: accent_primary (cyan), download: semantic_success (green).
        let accent = if ft.mode == "upload" {
            tokens.accent_primary
        } else {
            tokens.semantic_success
        };
        add_px_rect(px, py, pw, 2.0, accent, sw, sh, bg_verts, bg_idx);

        // Title
        let title = if ft.mode == "upload" {
            "SFTP Upload  (Tab=next, Enter=send, Esc=cancel)"
        } else {
            "SFTP Download  (Tab=next, Enter=send, Esc=cancel)"
        };
        add_string_verts(
            title,
            px + cell_w,
            py + cell_h * 0.1,
            accent,
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

        let field_labels = ["Host:", "Local:", "Remote:"];
        let field_values = [&ft.host_name, &ft.local_path, &ft.remote_path];

        for (i, (label, value)) in field_labels.iter().zip(field_values.iter()).enumerate() {
            let row_y = py + cell_h * (i as f32 * 1.5 + 1.3);
            let is_active = i == ft.field;

            // Field background: surface_2 when active (highlighted), surface_1 otherwise.
            let field_bg = if is_active {
                tokens.surface_2
            } else {
                tokens.surface_1
            };
            add_px_rect(
                px + cell_w * 8.0,
                row_y,
                pw - cell_w * 9.0,
                cell_h,
                field_bg,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );

            // Label
            add_string_verts(
                label,
                px + cell_w,
                row_y,
                if is_active {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                },
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

            // Input value + cursor
            let display = if is_active {
                format!("{}_", value)
            } else {
                value.to_string()
            };
            add_string_verts(
                &display,
                px + cell_w * 8.5,
                row_y,
                if is_active {
                    tokens.text_primary
                } else {
                    tokens.text_secondary
                },
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

    /// Build vertices for the Lua macro picker (center floating list)
    ///
    /// Lists defined macros; Enter runs the selected one.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_macro_picker_verts(
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
        let mp = &state.macro_picker;
        let items = mp.filtered();
        let panel_cols: f32 = 50.0;
        let panel_rows = (items.len() + 3).min(14) as f32;

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome: shared drop-shadow + border + rounded background.
        draw_overlay_panel(px, py, pw, ph, tokens, 5.0, 6.0, sw, sh, bg_verts, bg_idx);
        // Top accent line — intentional purple branding, kept as-is.
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            [0.7, 0.3, 1.0, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Title row
        add_string_verts(
            "Lua Macros",
            px + cell_w,
            py + cell_h * 0.1,
            [0.8, 0.5, 1.0, 1.0],
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

        // Query row
        let query_text = format!("> {}", mp.query);
        add_string_verts(
            &query_text,
            px + cell_w,
            py + cell_h * 1.1,
            [1.0, 1.0, 1.0, 1.0],
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

        // Macro list
        for (i, mac) in items.iter().enumerate().take(panel_rows as usize - 2) {
            let item_py = py + cell_h * (i as f32 + 2.2);
            let is_selected = i == mp.selected;
            if is_selected {
                add_px_rect(
                    px + 2.0,
                    item_py,
                    pw - 4.0,
                    cell_h,
                    [0.35, 0.15, 0.50, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let prefix = if is_selected { "> " } else { "  " };
            let desc = if mac.description.is_empty() {
                &mac.lua_fn
            } else {
                &mac.description
            };
            let label = format!("{}{:<22} {}", prefix, mac.name, desc);
            let fg = if is_selected {
                [0.95, 0.8, 1.0, 1.0]
            } else {
                [0.70, 0.60, 0.78, 1.0]
            };
            add_string_verts(
                &label,
                px + cell_w,
                item_py,
                fg,
                is_selected,
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

        // Hint when no macros are present
        if items.is_empty() {
            add_string_verts(
                "  (no macros in config)",
                px + cell_w,
                py + cell_h * 2.2,
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

    /// Build vertices for the host manager (center floating list)
    ///
    /// Lists SSH hosts using the same layout as the command palette.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_host_manager_verts(
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
        let hm = &state.host_manager;
        let items = hm.filtered();
        let panel_cols: f32 = 52.0;
        let panel_rows = (items.len() + 3).min(14) as f32; // title + query + up to 12 items

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // Panel chrome: shared drop-shadow + border + rounded background.
        draw_overlay_panel(px, py, pw, ph, tokens, 5.0, 6.0, sw, sh, bg_verts, bg_idx);
        // Top accent line — intentional green SSH branding, kept as-is.
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            [0.2, 0.8, 0.5, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Title row
        add_string_verts(
            "SSH Hosts",
            px + cell_w,
            py + cell_h * 0.1,
            [0.2, 0.9, 0.6, 1.0],
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

        // Query row
        let query_text = format!("> {}", hm.query);
        add_string_verts(
            &query_text,
            px + cell_w,
            py + cell_h * 1.1,
            [1.0, 1.0, 1.0, 1.0],
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

        // Host list (offset by 2 rows for title + query)
        for (i, host) in items.iter().enumerate().take(panel_rows as usize - 2) {
            let item_py = py + cell_h * (i as f32 + 2.2);
            let is_selected = i == hm.selected;
            if is_selected {
                add_px_rect(
                    px + 2.0,
                    item_py,
                    pw - 4.0,
                    cell_h,
                    [0.15, 0.45, 0.30, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            // Display format: "> name  user@host:port"
            let prefix = if is_selected { "> " } else { "  " };
            let label = format!(
                "{}{:<20} {}@{}:{}",
                prefix, host.name, host.username, host.host, host.port
            );
            let fg = if is_selected {
                [0.9, 1.0, 0.9, 1.0]
            } else {
                [0.70, 0.75, 0.72, 1.0]
            };
            add_string_verts(
                &label,
                px + cell_w,
                item_py,
                fg,
                is_selected,
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

        // Hint when no hosts are present
        if items.is_empty() {
            add_string_verts(
                "  (no hosts in config)",
                px + cell_w,
                py + cell_h * 2.2,
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
}
