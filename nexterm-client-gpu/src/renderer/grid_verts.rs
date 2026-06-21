//! Sprint 2-1 Phase A: vertex builders for the grid and scrollback.
//!
//! Extracted from `renderer.rs`: the four `build_grid_verts`-family methods.
//! All take `&self` and only access `self.queue`.

use unicode_width::UnicodeWidthChar;

use crate::color_util::resolve_color;
use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, GlyphKey, LigatureKey, TextVertex};
use crate::vertex_util::{add_px_rect, draw_cursor};

use super::WgpuState;

impl WgpuState {
    /// Build the vertices for the grid contents.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_grid_verts(
        &self,
        pane: &crate::state::PaneState,
        mouse_sel: &crate::state::MouseSelection,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        y_offset: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        palette: Option<&nexterm_config::SchemePalette>,
        cursor_style: &nexterm_config::CursorStyle,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // Selection highlight color (semi-transparent blue)
        const SEL_COLOR: [f32; 4] = [0.25, 0.55, 1.0, 0.40];

        let grid = &pane.grid;
        for row in 0..grid.height as usize {
            let py = row as f32 * cell_h + y_offset;

            // Draw background colors and selection highlights first (regardless of ligature use)
            for col in 0..grid.width as usize {
                let Some(cell) = grid.get(col as u16, row as u16) else {
                    continue;
                };
                let px = col as f32 * cell_w;
                let bg = resolve_color(&cell.bg, false, palette);
                add_px_rect(px, py, cell_w, cell_h, bg, sw, sh, bg_verts, bg_idx);
                if mouse_sel.contains(col as u16, row as u16) {
                    add_px_rect(px, py, cell_w, cell_h, SEL_COLOR, sw, sh, bg_verts, bg_idx);
                }
            }

            // When ligatures are enabled: draw the whole row using line-level shaping.
            // Cells rendered this way are tracked in `ligature_rendered`, which skips
            // the per-cell fallback path.
            let mut ligature_rendered = std::collections::HashSet::new();
            if font.ligatures {
                // Collect non-blank cells in this row as (col, char, bold, italic, fg_u8)
                let row_chars: Vec<(usize, char, bool, bool, [u8; 4])> = (0..grid.width as usize)
                    .filter_map(|col| {
                        let cell = grid.get(col as u16, row as u16)?;
                        if cell.ch == ' ' {
                            return None;
                        }
                        let fg = resolve_color(&cell.fg, true, palette);
                        let fg_u8 = [
                            (fg[0] * 255.0) as u8,
                            (fg[1] * 255.0) as u8,
                            (fg[2] * 255.0) as u8,
                            (fg[3] * 255.0) as u8,
                        ];
                        Some((
                            col,
                            cell.ch,
                            cell.attrs.is_bold(),
                            cell.attrs.is_italic(),
                            fg_u8,
                        ))
                    })
                    .collect();

                if !row_chars.is_empty() {
                    // Build the row text for use as a cache key
                    let row_text: String = row_chars.iter().map(|(_, ch, _, _, _)| *ch).collect();

                    let rendered = font.rasterize_line_segment(&row_chars);
                    for glyph in rendered {
                        if glyph.width == 0 || glyph.pixels.is_empty() {
                            continue;
                        }
                        let col = glyph.col;
                        let Some(cell) = grid.get(col as u16, row as u16) else {
                            continue;
                        };
                        let fg = resolve_color(&cell.fg, true, palette);
                        let fg_u8 = [
                            (fg[0] * 255.0) as u8,
                            (fg[1] * 255.0) as u8,
                            (fg[2] * 255.0) as u8,
                            255,
                        ];
                        let fg_packed = u32::from_le_bytes(fg_u8);
                        let lig_key = LigatureKey {
                            col,
                            text: row_text.clone(),
                            bold: cell.attrs.is_bold(),
                            italic: cell.attrs.is_italic(),
                            fg_packed,
                        };
                        let rect = atlas.get_or_insert_ligature(
                            lig_key,
                            &glyph.pixels,
                            glyph.width,
                            glyph.height,
                            &self.queue,
                        );
                        let px = col as f32 * cell_w;
                        let tx0 = px / sw * 2.0 - 1.0;
                        let ty0 = 1.0 - py / sh * 2.0;
                        let tx1 = (px + glyph.width as f32) / sw * 2.0 - 1.0;
                        let ty1 = 1.0 - (py + glyph.height as f32) / sh * 2.0;
                        let base = text_verts.len() as u16;
                        text_verts.extend_from_slice(&[
                            TextVertex {
                                position: [tx0, ty0],
                                uv: rect.uv_min,
                                color: fg,
                            },
                            TextVertex {
                                position: [tx1, ty0],
                                uv: [rect.uv_max[0], rect.uv_min[1]],
                                color: fg,
                            },
                            TextVertex {
                                position: [tx1, ty1],
                                uv: rect.uv_max,
                                color: fg,
                            },
                            TextVertex {
                                position: [tx0, ty1],
                                uv: [rect.uv_min[0], rect.uv_max[1]],
                                color: fg,
                            },
                        ]);
                        text_idx.extend_from_slice(&[
                            base,
                            base + 1,
                            base + 2,
                            base,
                            base + 2,
                            base + 3,
                        ]);
                        ligature_rendered.insert(col);
                    }
                }
            }

            // Fall back to per-cell rendering for cells not yet drawn by ligature shaping
            for col in 0..grid.width as usize {
                if ligature_rendered.contains(&col) {
                    continue;
                }
                let Some(cell) = grid.get(col as u16, row as u16) else {
                    continue;
                };
                if cell.ch == ' ' {
                    continue;
                }
                let px = col as f32 * cell_w;
                let fg = resolve_color(&cell.fg, true, palette);
                let fg_u8 = [
                    (fg[0] * 255.0) as u8,
                    (fg[1] * 255.0) as u8,
                    (fg[2] * 255.0) as u8,
                    (fg[3] * 255.0) as u8,
                ];
                let is_wide = UnicodeWidthChar::width(cell.ch).unwrap_or(1) >= 2;
                let key = GlyphKey {
                    ch: cell.ch,
                    bold: cell.attrs.is_bold(),
                    italic: cell.attrs.is_italic(),
                    wide: is_wide,
                };
                let (gw, gh, pixels) = font.rasterize_char(
                    cell.ch,
                    cell.attrs.is_bold(),
                    cell.attrs.is_italic(),
                    fg_u8,
                    is_wide,
                );
                if gw == 0 || gh == 0 || pixels.is_empty() {
                    continue;
                }
                let rect = atlas.get_or_insert(key, &pixels, gw, gh, &self.queue);
                let tx0 = px / sw * 2.0 - 1.0;
                let ty0 = 1.0 - py / sh * 2.0;
                let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
                let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
                let base = text_verts.len() as u16;
                text_verts.extend_from_slice(&[
                    TextVertex {
                        position: [tx0, ty0],
                        uv: rect.uv_min,
                        color: fg,
                    },
                    TextVertex {
                        position: [tx1, ty0],
                        uv: [rect.uv_max[0], rect.uv_min[1]],
                        color: fg,
                    },
                    TextVertex {
                        position: [tx1, ty1],
                        uv: rect.uv_max,
                        color: fg,
                    },
                    TextVertex {
                        position: [tx0, ty1],
                        uv: [rect.uv_min[0], rect.uv_max[1]],
                        color: fg,
                    },
                ]);
                text_idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }

        // Cursor rectangle (drawn in the configured shape)
        let cx = pane.cursor_col as f32 * cell_w;
        let cy = pane.cursor_row as f32 * cell_h + y_offset;
        draw_cursor(
            cursor_style,
            cx,
            cy,
            cell_w,
            cell_h,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
    }

    /// Build the vertices for the scrollback contents.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_scrollback_verts(
        &self,
        pane: &crate::state::PaneState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        y_offset: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        palette: Option<&nexterm_config::SchemePalette>,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // Effective number of display rows, also excluding the status bar (bottom 1 cell)
        let visible_rows = ((sh - y_offset - cell_h) / cell_h).max(0.0) as usize;
        let offset = pane.scroll_offset;

        for visual_row in 0..visible_rows {
            let sb_row = offset + visual_row;
            let Some(line) = pane.scrollback.get(sb_row) else {
                continue;
            };
            let py = visual_row as f32 * cell_h + y_offset;
            for (col, cell) in line.iter().enumerate() {
                let px = col as f32 * cell_w;
                // Slightly darken the background for scrollback rows
                let bg = resolve_color(&cell.bg, false, palette);
                let dim_bg = [bg[0] * 0.75, bg[1] * 0.75, bg[2] * 0.75, 1.0];
                add_px_rect(px, py, cell_w, cell_h, dim_bg, sw, sh, bg_verts, bg_idx);
                if cell.ch == ' ' {
                    continue;
                }
                let fg = resolve_color(&cell.fg, true, palette);
                let fg_u8 = [
                    (fg[0] * 255.0) as u8,
                    (fg[1] * 255.0) as u8,
                    (fg[2] * 255.0) as u8,
                    (fg[3] * 255.0) as u8,
                ];
                let is_wide = UnicodeWidthChar::width(cell.ch).unwrap_or(1) >= 2;
                let key = GlyphKey {
                    ch: cell.ch,
                    bold: cell.attrs.is_bold(),
                    italic: false,
                    wide: is_wide,
                };
                let (gw, gh, pixels) =
                    font.rasterize_char(cell.ch, cell.attrs.is_bold(), false, fg_u8, is_wide);
                if gw == 0 || gh == 0 || pixels.is_empty() {
                    continue;
                }
                let rect = atlas.get_or_insert(key, &pixels, gw, gh, &self.queue);
                let tx0 = px / sw * 2.0 - 1.0;
                let ty0 = 1.0 - py / sh * 2.0;
                let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
                let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
                let base = text_verts.len() as u16;
                text_verts.extend_from_slice(&[
                    TextVertex {
                        position: [tx0, ty0],
                        uv: rect.uv_min,
                        color: fg,
                    },
                    TextVertex {
                        position: [tx1, ty0],
                        uv: [rect.uv_max[0], rect.uv_min[1]],
                        color: fg,
                    },
                    TextVertex {
                        position: [tx1, ty1],
                        uv: rect.uv_max,
                        color: fg,
                    },
                    TextVertex {
                        position: [tx0, ty1],
                        uv: [rect.uv_min[0], rect.uv_max[1]],
                        color: fg,
                    },
                ]);
                text_idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }

    /// Phase 2c-1 (command-blocks): paint left-border + selection tint for
    /// each block visible inside the current scrollback view.
    ///
    /// Scope of this initial implementation:
    /// - **Only invoked while the pane is in scrollback mode**
    ///   (`pane.scroll_offset > 0`). The normal grid display uses block ids
    ///   that have not yet been pushed to scrollback, which would require
    ///   a different coordinate translation; that case lands in a later
    ///   iteration once on-device verification is available.
    /// - Draws only the left N-pixel border (where N comes from
    ///   `BlocksConfig.effective_border_width_px()`) plus a faint full-row
    ///   tint for the selected block. Status badges (`✓` / `✗` / `●`) are
    ///   deferred because their precise glyph placement needs visual
    ///   calibration.
    /// - Gated by `BlocksConfig.enabled`; returns immediately when false.
    ///
    /// Colour mapping:
    /// - `BlockStatus::Success` → green
    /// - `BlockStatus::Failure` → red
    /// - `BlockStatus::Running` → grey
    ///
    /// The selected block additionally receives a low-alpha row-wide tint
    /// using the same hue so a glance reveals both *what* is highlighted
    /// and *why*.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_block_overlay_verts(
        &self,
        pane: &crate::state::PaneState,
        selected_block: Option<crate::command_blocks::BlockId>,
        config: &nexterm_config::BlocksConfig,
        sw: f32,
        sh: f32,
        _cell_w: f32,
        cell_h: f32,
        y_offset: f32,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
    ) {
        if !config.enabled || pane.blocks.is_empty() {
            return;
        }
        // Only the scrollback display path is supported in this first cut.
        if pane.scroll_offset == 0 {
            return;
        }

        let visible_rows = ((sh - y_offset - cell_h) / cell_h).max(0.0) as u16;
        if visible_rows == 0 {
            return;
        }
        let visible_top = pane.scroll_offset;

        let lines = crate::command_blocks::compute_block_overlay_lines(
            &pane.blocks,
            selected_block,
            visible_top,
            visible_rows,
        );
        if lines.is_empty() {
            return;
        }

        let border_w = config.effective_border_width_px() as f32;
        let grid_w = sw; // tint spans the entire grid

        for line in lines {
            // Clip start / end to the viewport in row units.
            let start_row = line.visual_row_start.max(0) as f32;
            let end_row_excl = ((line.visual_row_end + 1).max(0) as f32).min(visible_rows as f32);
            if end_row_excl <= start_row {
                continue;
            }
            let py = start_row * cell_h + y_offset;
            let h = (end_row_excl - start_row) * cell_h;

            let (mut r, mut g, mut b) = match line.status {
                crate::command_blocks::BlockStatus::Success => (0.20, 0.75, 0.30),
                crate::command_blocks::BlockStatus::Failure => (0.85, 0.25, 0.25),
                crate::command_blocks::BlockStatus::Running => (0.55, 0.55, 0.55),
            };
            // Subtly brighten the border for the selected block.
            if line.selected {
                r = (r * 1.2_f32).min(1.0);
                g = (g * 1.2_f32).min(1.0);
                b = (b * 1.2_f32).min(1.0);
            }
            let border_color = [r, g, b, 0.95];
            add_px_rect(0.0, py, border_w, h, border_color, sw, sh, bg_verts, bg_idx);

            // Full-row tint for the selected block (low alpha so the text
            // underneath stays readable).
            if line.selected {
                let tint = [r, g, b, 0.10];
                add_px_rect(0.0, py, grid_w, h, tint, sw, sh, bg_verts, bg_idx);
            }
        }
    }

    /// Multi-pane variant: draw the grid inside the given layout rectangle.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_grid_verts_in_rect(
        &self,
        pane: &crate::state::PaneState,
        layout: &nexterm_proto::PaneLayout,
        is_focused: bool,
        mouse_sel: &crate::state::MouseSelection,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        tab_bar_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        palette: Option<&nexterm_config::SchemePalette>,
        cursor_style: &nexterm_config::CursorStyle,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // Selection highlight color (semi-transparent blue)
        const SEL_COLOR: [f32; 4] = [0.25, 0.55, 1.0, 0.40];

        // `tab_bar_h` already includes `padding_y` (the caller passes `grid_offset_y`)
        let off_x = layout.col_offset as f32 * cell_w;
        let off_y = layout.row_offset as f32 * cell_h + tab_bar_h;
        // Dim non-focused panes slightly
        let dim = if is_focused { 1.0f32 } else { 0.70f32 };
        let grid = &pane.grid;

        for row in 0..layout.rows.min(grid.height) as usize {
            for col in 0..layout.cols.min(grid.width) as usize {
                let Some(cell) = grid.get(col as u16, row as u16) else {
                    continue;
                };
                let px = off_x + col as f32 * cell_w;
                let py = off_y + row as f32 * cell_h;
                let bg = resolve_color(&cell.bg, false, palette);
                let bg = [bg[0] * dim, bg[1] * dim, bg[2] * dim, 1.0];
                add_px_rect(px, py, cell_w, cell_h, bg, sw, sh, bg_verts, bg_idx);
                // Selection highlight overlay (focused pane only)
                if is_focused && mouse_sel.contains(col as u16, row as u16) {
                    add_px_rect(px, py, cell_w, cell_h, SEL_COLOR, sw, sh, bg_verts, bg_idx);
                }
                if cell.ch == ' ' {
                    continue;
                }
                let fg = resolve_color(&cell.fg, true, palette);
                let fg = [fg[0] * dim, fg[1] * dim, fg[2] * dim, fg[3]];
                let fg_u8 = [
                    (fg[0] * 255.0) as u8,
                    (fg[1] * 255.0) as u8,
                    (fg[2] * 255.0) as u8,
                    (fg[3] * 255.0) as u8,
                ];
                // Full-width characters (CJK etc., Unicode width = 2) are rendered across two cells
                let is_wide = UnicodeWidthChar::width(cell.ch).unwrap_or(1) >= 2;
                let key = GlyphKey {
                    ch: cell.ch,
                    bold: cell.attrs.is_bold(),
                    italic: cell.attrs.is_italic(),
                    wide: is_wide,
                };
                let (gw, gh, pixels) = font.rasterize_char(
                    cell.ch,
                    cell.attrs.is_bold(),
                    cell.attrs.is_italic(),
                    fg_u8,
                    is_wide,
                );
                if gw == 0 || gh == 0 || pixels.is_empty() {
                    continue;
                }
                let rect = atlas.get_or_insert(key, &pixels, gw, gh, &self.queue);
                let tx0 = px / sw * 2.0 - 1.0;
                let ty0 = 1.0 - py / sh * 2.0;
                let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
                let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
                let base = text_verts.len() as u16;
                text_verts.extend_from_slice(&[
                    TextVertex {
                        position: [tx0, ty0],
                        uv: rect.uv_min,
                        color: fg,
                    },
                    TextVertex {
                        position: [tx1, ty0],
                        uv: [rect.uv_max[0], rect.uv_min[1]],
                        color: fg,
                    },
                    TextVertex {
                        position: [tx1, ty1],
                        uv: rect.uv_max,
                        color: fg,
                    },
                    TextVertex {
                        position: [tx0, ty1],
                        uv: [rect.uv_min[0], rect.uv_max[1]],
                        color: fg,
                    },
                ]);
                text_idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }

        // Cursor (focused pane only)
        if is_focused {
            let cx = off_x + pane.cursor_col as f32 * cell_w;
            let cy = off_y + pane.cursor_row as f32 * cell_h;
            draw_cursor(
                cursor_style,
                cx,
                cy,
                cell_w,
                cell_h,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }
    }

    /// Multi-pane variant: draw the scrollback inside the given layout rectangle.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_scrollback_verts_in_rect(
        &self,
        pane: &crate::state::PaneState,
        layout: &nexterm_proto::PaneLayout,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        tab_bar_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        palette: Option<&nexterm_config::SchemePalette>,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let off_x = layout.col_offset as f32 * cell_w;
        let off_y = layout.row_offset as f32 * cell_h + tab_bar_h;
        let offset = pane.scroll_offset;

        for visual_row in 0..layout.rows as usize {
            let sb_row = offset + visual_row;
            let Some(line) = pane.scrollback.get(sb_row) else {
                continue;
            };
            let py = off_y + visual_row as f32 * cell_h;
            for (col, cell) in line.iter().enumerate().take(layout.cols as usize) {
                let px = off_x + col as f32 * cell_w;
                let bg = resolve_color(&cell.bg, false, palette);
                let dim_bg = [bg[0] * 0.75, bg[1] * 0.75, bg[2] * 0.75, 1.0];
                add_px_rect(px, py, cell_w, cell_h, dim_bg, sw, sh, bg_verts, bg_idx);
                if cell.ch == ' ' {
                    continue;
                }
                let fg = resolve_color(&cell.fg, true, palette);
                let fg_u8 = [
                    (fg[0] * 255.0) as u8,
                    (fg[1] * 255.0) as u8,
                    (fg[2] * 255.0) as u8,
                    (fg[3] * 255.0) as u8,
                ];
                let is_wide = UnicodeWidthChar::width(cell.ch).unwrap_or(1) >= 2;
                let key = GlyphKey {
                    ch: cell.ch,
                    bold: cell.attrs.is_bold(),
                    italic: false,
                    wide: is_wide,
                };
                let (gw, gh, pixels) =
                    font.rasterize_char(cell.ch, cell.attrs.is_bold(), false, fg_u8, is_wide);
                if gw == 0 || gh == 0 || pixels.is_empty() {
                    continue;
                }
                let rect = atlas.get_or_insert(key, &pixels, gw, gh, &self.queue);
                let tx0 = px / sw * 2.0 - 1.0;
                let ty0 = 1.0 - py / sh * 2.0;
                let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
                let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
                let base = text_verts.len() as u16;
                text_verts.extend_from_slice(&[
                    TextVertex {
                        position: [tx0, ty0],
                        uv: rect.uv_min,
                        color: fg,
                    },
                    TextVertex {
                        position: [tx1, ty0],
                        uv: [rect.uv_max[0], rect.uv_min[1]],
                        color: fg,
                    },
                    TextVertex {
                        position: [tx1, ty1],
                        uv: rect.uv_max,
                        color: fg,
                    },
                    TextVertex {
                        position: [tx0, ty1],
                        uv: [rect.uv_min[0], rect.uv_max[1]],
                        color: fg,
                    },
                ]);
                text_idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
}
