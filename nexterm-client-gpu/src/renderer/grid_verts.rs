//! Sprint 2-1 Phase A: グリッド・スクロールバック頂点ビルダー
//!
//! `renderer.rs` から抽出した `build_grid_verts` 系 4 メソッド。
//! いずれも `&self` レシーバで `self.queue` のみアクセスする。

use unicode_width::UnicodeWidthChar;

use crate::color_util::resolve_color;
use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, GlyphKey, LigatureKey, TextVertex};
use crate::vertex_util::{add_px_rect, draw_cursor};

use super::WgpuState;

impl WgpuState {
    /// グリッドコンテンツの頂点を構築する
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
        // 選択ハイライト色（半透明の青）
        const SEL_COLOR: [f32; 4] = [0.25, 0.55, 1.0, 0.40];

        let grid = &pane.grid;
        for row in 0..grid.height as usize {
            let py = row as f32 * cell_h + y_offset;

            // 背景色・選択ハイライトを先に描画する（リガチャ有無に関わらず）
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

            // リガチャが有効な場合: 行全体を行単位シェーピングで描画する
            // 成功したセルは `ligature_rendered` に記録してフォールバックをスキップする
            let mut ligature_rendered = std::collections::HashSet::new();
            if font.ligatures {
                // 行の非空白セルを (col, char, bold, italic, fg_u8) にまとめる
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
                    // 行テキストをキャッシュキー用に生成する
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

            // リガチャで描画済みでないセルを1文字単位でフォールバック描画する
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

        // カーソル矩形（スタイルに応じた形状で描画）
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

    /// スクロールバックコンテンツの頂点を構築する
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
        // ステータスバー（下部1セル）も除外した有効表示行数
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
                // スクロールバック行は背景を少し暗くする
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

    /// マルチペイン用: レイアウト矩形内にグリッドを描画する
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
        // 選択ハイライト色（半透明の青）
        const SEL_COLOR: [f32; 4] = [0.25, 0.55, 1.0, 0.40];

        // tab_bar_h には padding_y が含まれる（呼び出し元で grid_offset_y を渡している）
        let off_x = layout.col_offset as f32 * cell_w;
        let off_y = layout.row_offset as f32 * cell_h + tab_bar_h;
        // 非フォーカスペインを少し暗く表示する
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
                // 選択ハイライトオーバーレイ（フォーカスペインのみ）
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
                // 全角文字（CJK 等、Unicode width = 2）は 2 セル幅でレンダリングする
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

        // カーソル（フォーカスペインのみ）
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

    /// マルチペイン用: レイアウト矩形内にスクロールバックを描画する
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
