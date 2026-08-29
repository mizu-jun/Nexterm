//! Vertex builders for picker-style overlays.
//!
//! Handles list-style UI for the command palette / SFTP file transfer /
//! macro picker / host manager.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_run_verts, add_string_verts, measure_run};

use super::super::WgpuState;
use super::util::draw_overlay_panel;
use nexterm_config::SurfaceLevel;

/// The selection marker drawn to the left of a picker row.
///
/// Drawn as its own run rather than prefixed onto the label: `> ` and `  ` are
/// the same width only in a monospace font, and once the rows draw at a ramp
/// step (UI/UX v3 P4e) a prefixed marker would shift every selected row's text
/// sideways.
const ROW_MARKER: &str = "> ";

/// Width of the name column in the host and macro lists.
///
/// Both lists used to align their two columns with `{:<20}` / `{:<22}` — a
/// count of *characters*, which only lines up because the chrome borrowed the
/// terminal's monospace font. It also broke for any name past the pad width
/// and for CJK names, whose cells are twice as wide. The column is now the
/// widest measured name plus one gap, clamped so the detail column keeps at
/// least `min_detail_w` — a long name shortens nothing but its own column.
///
/// Pure, so the clamp is testable without a font.
fn name_column_width(name_widths: &[f32], gap: f32, panel_w: f32, min_detail_w: f32) -> f32 {
    let widest = name_widths.iter().copied().fold(0.0_f32, f32::max);
    let ceiling = (panel_w - min_detail_w).max(0.0);
    (widest + gap).min(ceiling).max(0.0)
}

impl WgpuState {
    /// Draw one chrome run inside a picker row, truncated to `max_w` and
    /// vertically centred in a row of `row_h` (UI/UX v3 P4e).
    ///
    /// The three pickers each drew their rows with `add_string_verts` at the
    /// terminal cell size and none of them truncated, so a long label ran past
    /// the panel edge. This is their equivalent of the widget layer's
    /// `draw_row_run`: one measurement, shared by the truncation and the draw,
    /// and it returns the width it used so a caller can place a second column.
    #[allow(clippy::too_many_arguments)]
    fn draw_picker_run(
        &self,
        text: &str,
        style: &nexterm_config::TypeStyle,
        x: f32,
        row_y: f32,
        row_h: f32,
        max_w: f32,
        color: [f32; 4],
        sw: f32,
        sh: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) -> f32 {
        let (_size, line_h, _bold) = font.chrome_metrics(style);
        let shown = crate::vertex_util::truncate_run_to_width(text, style, max_w, font);
        add_run_verts(
            &shown,
            style,
            x,
            row_y + (row_h - line_h) * 0.5,
            color,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        )
    }

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
        acrylic_mix: f32,
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
        let elevation = nexterm_config::ElevationScale::default().flyout;
        draw_overlay_panel(
            px,
            py,
            pw,
            ph,
            tokens,
            elevation,
            6.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );
        // Top accent stripe (accent_primary)
        let ap = tokens.accent_primary;
        add_px_rect(px, py, pw, 2.0, ap, sw, sh, bg_verts, bg_idx);

        // UI/UX v3 P4e: the rows draw at the chrome ramp. Geometry is
        // unchanged — the panel is still `palette_cols` cells wide and rows
        // are still `cell_h` apart — so this moves no row and no hit region;
        // only the size the glyphs are rasterised at, plus the truncation the
        // cell path never had.
        let metrics = nexterm_config::MetricTokens::default();
        let body = metrics.type_ramp.body;
        let body_strong = metrics.type_ramp.body_strong;
        let text_x = px + cell_w;
        let text_max_w = (pw - cell_w * 2.0).max(0.0);
        let marker_w = measure_run(ROW_MARKER, &body_strong, font);

        // Query row
        let query_text = format!("> {}", palette.query);
        self.draw_picker_run(
            &query_text,
            &body,
            text_x,
            py + cell_h * 0.1,
            cell_h,
            text_max_w,
            [1.0, 1.0, 1.0, 1.0],
            sw,
            sh,
            font,
            atlas,
            text_verts,
            text_idx,
        );

        // Action list
        for (i, action) in items.iter().enumerate().take(palette_rows as usize - 1) {
            let item_py = py + cell_h * (i as f32 + 1.2);
            let is_selected = i == palette.selected;
            if is_selected {
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
            let fg = if is_selected {
                tokens.text_on(SurfaceLevel::S2).primary
            } else {
                tokens.text_on(SurfaceLevel::S2).muted
            };
            // The marker is its own run so every label starts at the same x,
            // selected or not.
            if is_selected {
                self.draw_picker_run(
                    ROW_MARKER,
                    &body_strong,
                    text_x,
                    item_py,
                    cell_h,
                    marker_w,
                    fg,
                    sw,
                    sh,
                    font,
                    atlas,
                    text_verts,
                    text_idx,
                );
            }
            let style = if is_selected { body_strong } else { body };
            self.draw_picker_run(
                &action.label,
                &style,
                text_x + marker_w,
                item_py,
                cell_h,
                (text_max_w - marker_w).max(0.0),
                fg,
                sw,
                sh,
                font,
                atlas,
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
        acrylic_mix: f32,
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
        let elevation = nexterm_config::ElevationScale::default().flyout;
        draw_overlay_panel(
            px,
            py,
            pw,
            ph,
            tokens,
            elevation,
            6.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );
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
                    tokens.text_on(SurfaceLevel::S2).primary
                } else {
                    tokens.text_on(SurfaceLevel::S2).secondary
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
                    tokens.text_on(SurfaceLevel::S2).primary
                } else {
                    tokens.text_on(SurfaceLevel::S2).secondary
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
        acrylic_mix: f32,
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
        let elevation = nexterm_config::ElevationScale::default().flyout;
        draw_overlay_panel(
            px,
            py,
            pw,
            ph,
            tokens,
            elevation,
            6.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );
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

        // UI/UX v3 P4e: ramp + measured two-column rows. The colours here are
        // hard-coded and stay that way in this PR — they are G11 residue, and
        // changing hue and type size together would make a visual regression
        // impossible to attribute.
        let metrics = nexterm_config::MetricTokens::default();
        let body = metrics.type_ramp.body;
        let body_strong = metrics.type_ramp.body_strong;
        let text_x = px + cell_w;
        let text_max_w = (pw - cell_w * 2.0).max(0.0);
        let marker_w = measure_run(ROW_MARKER, &body_strong, font);

        // Title row
        self.draw_picker_run(
            "Lua Macros",
            &body_strong,
            text_x,
            py + cell_h * 0.1,
            cell_h,
            text_max_w,
            [0.8, 0.5, 1.0, 1.0],
            sw,
            sh,
            font,
            atlas,
            text_verts,
            text_idx,
        );

        // Query row
        let query_text = format!("> {}", mp.query);
        self.draw_picker_run(
            &query_text,
            &body,
            text_x,
            py + cell_h * 1.1,
            cell_h,
            text_max_w,
            [1.0, 1.0, 1.0, 1.0],
            sw,
            sh,
            font,
            atlas,
            text_verts,
            text_idx,
        );

        // Macro list. The name column is measured rather than padded to 22
        // characters, so a long or CJK name no longer pushes its description
        // out of alignment with the rows around it.
        let visible = items.len().min(panel_rows as usize - 2);
        let name_widths: Vec<f32> = items[..visible]
            .iter()
            .map(|mac| measure_run(&mac.name, &body, font))
            .collect();
        let name_col_w =
            name_column_width(&name_widths, cell_w, text_max_w - marker_w, cell_w * 8.0);

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
            let desc = if mac.description.is_empty() {
                &mac.lua_fn
            } else {
                &mac.description
            };
            let fg = if is_selected {
                [0.95, 0.8, 1.0, 1.0]
            } else {
                [0.70, 0.60, 0.78, 1.0]
            };
            let style = if is_selected { body_strong } else { body };
            if is_selected {
                self.draw_picker_run(
                    ROW_MARKER,
                    &body_strong,
                    text_x,
                    item_py,
                    cell_h,
                    marker_w,
                    fg,
                    sw,
                    sh,
                    font,
                    atlas,
                    text_verts,
                    text_idx,
                );
            }
            self.draw_picker_run(
                &mac.name,
                &style,
                text_x + marker_w,
                item_py,
                cell_h,
                name_col_w,
                fg,
                sw,
                sh,
                font,
                atlas,
                text_verts,
                text_idx,
            );
            self.draw_picker_run(
                desc,
                &style,
                text_x + marker_w + name_col_w,
                item_py,
                cell_h,
                (text_max_w - marker_w - name_col_w).max(0.0),
                fg,
                sw,
                sh,
                font,
                atlas,
                text_verts,
                text_idx,
            );
        }

        // Hint when no macros are present
        if items.is_empty() {
            self.draw_picker_run(
                "(no macros in config)",
                &body,
                text_x + marker_w,
                py + cell_h * 2.2,
                cell_h,
                text_max_w,
                tokens.text_on(SurfaceLevel::S2).muted,
                sw,
                sh,
                font,
                atlas,
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
        acrylic_mix: f32,
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
        let elevation = nexterm_config::ElevationScale::default().flyout;
        draw_overlay_panel(
            px,
            py,
            pw,
            ph,
            tokens,
            elevation,
            6.0,
            sw,
            sh,
            acrylic_mix,
            bg_verts,
            bg_idx,
        );
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

        // UI/UX v3 P4e: same treatment as the macro picker — ramp, measured
        // name column, truncation. The green branding and the row fills stay
        // hard-coded here, as they were before this change.
        let metrics = nexterm_config::MetricTokens::default();
        let body = metrics.type_ramp.body;
        let body_strong = metrics.type_ramp.body_strong;
        let text_x = px + cell_w;
        let text_max_w = (pw - cell_w * 2.0).max(0.0);
        let marker_w = measure_run(ROW_MARKER, &body_strong, font);

        // Title row
        self.draw_picker_run(
            "SSH Hosts",
            &body_strong,
            text_x,
            py + cell_h * 0.1,
            cell_h,
            text_max_w,
            [0.2, 0.9, 0.6, 1.0],
            sw,
            sh,
            font,
            atlas,
            text_verts,
            text_idx,
        );

        // Query row
        let query_text = format!("> {}", hm.query);
        self.draw_picker_run(
            &query_text,
            &body,
            text_x,
            py + cell_h * 1.1,
            cell_h,
            text_max_w,
            [1.0, 1.0, 1.0, 1.0],
            sw,
            sh,
            font,
            atlas,
            text_verts,
            text_idx,
        );

        // Host list (offset by 2 rows for title + query)
        let visible = items.len().min(panel_rows as usize - 2);
        let name_widths: Vec<f32> = items[..visible]
            .iter()
            .map(|host| measure_run(&host.name, &body, font))
            .collect();
        let name_col_w =
            name_column_width(&name_widths, cell_w, text_max_w - marker_w, cell_w * 12.0);

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
            // Display format: "> name  user@host:port", the two parts now
            // being two runs rather than one padded string.
            let target = format!("{}@{}:{}", host.username, host.host, host.port);
            let fg = if is_selected {
                [0.9, 1.0, 0.9, 1.0]
            } else {
                [0.70, 0.75, 0.72, 1.0]
            };
            let style = if is_selected { body_strong } else { body };
            if is_selected {
                self.draw_picker_run(
                    ROW_MARKER,
                    &body_strong,
                    text_x,
                    item_py,
                    cell_h,
                    marker_w,
                    fg,
                    sw,
                    sh,
                    font,
                    atlas,
                    text_verts,
                    text_idx,
                );
            }
            self.draw_picker_run(
                &host.name,
                &style,
                text_x + marker_w,
                item_py,
                cell_h,
                name_col_w,
                fg,
                sw,
                sh,
                font,
                atlas,
                text_verts,
                text_idx,
            );
            self.draw_picker_run(
                &target,
                &style,
                text_x + marker_w + name_col_w,
                item_py,
                cell_h,
                (text_max_w - marker_w - name_col_w).max(0.0),
                fg,
                sw,
                sh,
                font,
                atlas,
                text_verts,
                text_idx,
            );
        }

        // Hint when no hosts are present
        if items.is_empty() {
            self.draw_picker_run(
                "(no hosts in config)",
                &body,
                text_x + marker_w,
                py + cell_h * 2.2,
                cell_h,
                text_max_w,
                tokens.text_on(SurfaceLevel::S2).muted,
                sw,
                sh,
                font,
                atlas,
                text_verts,
                text_idx,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAP: f32 = 10.0;
    const PANEL_W: f32 = 500.0;
    const MIN_DETAIL: f32 = 80.0;

    #[test]
    fn the_name_column_is_the_widest_name_plus_one_gap() {
        let w = name_column_width(&[40.0, 120.0, 90.0], GAP, PANEL_W, MIN_DETAIL);
        assert_eq!(w, 130.0);
    }

    /// A name long enough to eat the row cannot squeeze the detail column out
    /// of existence — the `{:<20}` padding it replaces had no such guard, and
    /// a 30-character host name simply pushed `user@host:port` off the panel.
    #[test]
    fn a_very_long_name_cannot_starve_the_detail_column() {
        let w = name_column_width(&[10_000.0], GAP, PANEL_W, MIN_DETAIL);
        assert_eq!(w, PANEL_W - MIN_DETAIL);
    }

    /// An empty list still has to answer, and a degenerate panel must not
    /// produce a negative column that would place the second run left of the
    /// first.
    #[test]
    fn the_column_is_never_negative() {
        assert_eq!(name_column_width(&[], GAP, PANEL_W, MIN_DETAIL), GAP);
        assert_eq!(name_column_width(&[50.0], GAP, 40.0, MIN_DETAIL), 0.0);
    }

    /// The three pickers must not go back to aligning columns by character
    /// count: a left-pad format spec lines up only in a monospace font, which
    /// is exactly the assumption the ramp removes.
    #[test]
    fn no_picker_row_pads_its_columns_with_characters_again() {
        // Comments (including the ones above, which quote the old spec) and
        // this test module itself are stripped before the search.
        let src = include_str!("picker.rs");
        let production = src
            .split("#[cfg(test)]")
            .next()
            .expect("the file has a production half");
        let code: String = production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        let pad_spec = ["{", ":", "<"].concat();
        assert!(
            !code.contains(&pad_spec),
            "picker.rs pads a column to a character count again; \
             name_column_width owns that alignment"
        );
    }
}
