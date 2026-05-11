//! ピッカー系オーバーレイの頂点ビルダー。
//!
//! コマンドパレット / SFTP ファイル転送 / マクロピッカー / ホストマネージャ
//! のリスト系 UI を担当する。

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::super::WgpuState;

impl WgpuState {
    /// コマンドパレット頂点を構築する（中央フローティング）
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_palette_verts(
        &self,
        state: &ClientState,
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
        let palette_rows = (items.len() + 2).min(12) as f32; // クエリ行 + 最大10アイテム + マージン

        let pw = palette_cols * cell_w;
        let ph = palette_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パレット背景（ダークグレー）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.15, 0.15, 0.18, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 外枠（やや明るい）
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            [0.4, 0.6, 1.0, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // クエリ行
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

        // アクション一覧
        for (i, action) in items.iter().enumerate().take(palette_rows as usize - 1) {
            let item_py = py + cell_h * (i as f32 + 1.2);
            if i == palette.selected {
                // 選択行ハイライト
                add_px_rect(
                    px + 2.0,
                    item_py,
                    pw - 4.0,
                    cell_h,
                    [0.25, 0.40, 0.65, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let prefix = if i == palette.selected { "> " } else { "  " };
            let label = format!("{}{}", prefix, action.label);
            let fg = if i == palette.selected {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.75, 0.75, 0.78, 1.0]
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

    /// SFTP ファイル転送ダイアログ頂点を構築する
    ///
    /// ホスト名・ローカルパス・リモートパスの 3 フィールドを入力する。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_file_transfer_verts(
        &self,
        state: &ClientState,
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
        let panel_rows: f32 = 7.0; // タイトル + ホスト + ローカル + リモート + ヒント

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景（深青緑）
        let bg_color = if ft.mode == "upload" {
            [0.05, 0.15, 0.20, 0.96]
        } else {
            [0.05, 0.20, 0.12, 0.96]
        };
        add_px_rect(px, py, pw, ph, bg_color, sw, sh, bg_verts, bg_idx);
        let accent = if ft.mode == "upload" {
            [0.2, 0.8, 1.0, 1.0]
        } else {
            [0.2, 1.0, 0.6, 1.0]
        };
        add_px_rect(px, py, pw, 2.0, accent, sw, sh, bg_verts, bg_idx);

        // タイトル
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

            // フィールド背景（アクティブは明るく）
            let field_bg = if is_active {
                [0.15, 0.25, 0.35, 1.0]
            } else {
                [0.10, 0.15, 0.20, 1.0]
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

            // ラベル
            add_string_verts(
                label,
                px + cell_w,
                row_y,
                if is_active {
                    [0.9, 0.95, 1.0, 1.0]
                } else {
                    [0.6, 0.65, 0.7, 1.0]
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

            // 入力値 + カーソル
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
                    [1.0, 1.0, 0.8, 1.0]
                } else {
                    [0.8, 0.85, 0.8, 1.0]
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

    /// Lua マクロピッカー頂点を構築する（中央フローティングリスト）
    ///
    /// 定義済みマクロを一覧表示し、Enter で実行する。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_macro_picker_verts(
        &self,
        state: &ClientState,
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

        // パネル背景（深紫系）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.12, 0.08, 0.20, 0.96],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上端アクセント線（紫/ピンク）
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

        // タイトル行
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

        // クエリ行
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

        // マクロ一覧
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

        // 空マクロ時のヒント
        if items.is_empty() {
            add_string_verts(
                "  (no macros in config)",
                px + cell_w,
                py + cell_h * 2.2,
                [0.5, 0.5, 0.5, 1.0],
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

    /// ホストマネージャ頂点を構築する（中央フローティングリスト）
    ///
    /// コマンドパレットと同様のレイアウトで SSH ホスト一覧を表示する。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_host_manager_verts(
        &self,
        state: &ClientState,
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
        let panel_rows = (items.len() + 3).min(14) as f32; // タイトル + クエリ + 最大12項目

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景（深めの紺）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.08, 0.12, 0.22, 0.96],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上端アクセント線（緑系）
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

        // タイトル行
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

        // クエリ行
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

        // ホスト一覧（タイトル+クエリ = 2行分オフセット）
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
            // 表示フォーマット: "> name  user@host:port"
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

        // 空ホスト時のヒント
        if items.is_empty() {
            add_string_verts(
                "  (no hosts in config)",
                px + cell_w,
                py + cell_h * 2.2,
                [0.5, 0.5, 0.5, 1.0],
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
