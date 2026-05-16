//! Sprint 2-1 Phase A: ボーダー・タブバー・ステータス UI 頂点ビルダー
//!
//! `renderer.rs` から抽出した UI 系頂点ビルダー 6 メソッド。

use crate::color_util::hex_to_rgba;
use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::WgpuState;

impl WgpuState {
    /// ペイン境界線を描画する
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_border_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        tab_bar_h: f32,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
    ) {
        if state.pane_layouts.len() <= 1 {
            return;
        }
        // Tokyo Night: セパレーター色 #415068、フォーカス枠 #7AA2F7
        let border_color = [0.255, 0.286, 0.408, 1.0];
        let focused_border = [0.478, 0.635, 0.969, 1.0];
        // フォーカスペインの枠線ハイライト（薄い青、アルファ 0.25）
        let focused_highlight = [0.478, 0.635, 0.969, 0.25];
        // 境界線は 2px で視認性を向上
        let border_w = 2.0_f32;

        for layout in state.pane_layouts.values() {
            let px = layout.col_offset as f32 * cell_w;
            let py = layout.row_offset as f32 * cell_h + tab_bar_h;
            let pw = layout.cols as f32 * cell_w;
            let ph = layout.rows as f32 * cell_h;
            let is_focused = state.focused_pane_id == Some(layout.pane_id);

            // フォーカスペインに薄いハイライト枠（2px）を描画する
            if is_focused && state.pane_layouts.len() > 1 {
                // 上辺
                add_px_rect(px, py, pw, 2.0, focused_highlight, sw, sh, bg_verts, bg_idx);
                // 下辺
                add_px_rect(
                    px,
                    py + ph - 2.0,
                    pw,
                    2.0,
                    focused_highlight,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // 左辺
                add_px_rect(px, py, 2.0, ph, focused_highlight, sw, sh, bg_verts, bg_idx);
                // 右辺
                add_px_rect(
                    px + pw - 2.0,
                    py,
                    2.0,
                    ph,
                    focused_highlight,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // 右隣にペインがあれば 1px の垂直境界線を描画する
            let right_col = layout.col_offset + layout.cols + 1;
            let color = if is_focused {
                focused_border
            } else {
                border_color
            };
            if state
                .pane_layouts
                .values()
                .any(|o| o.pane_id != layout.pane_id && o.col_offset == right_col)
            {
                add_px_rect(px + pw, py, border_w, ph, color, sw, sh, bg_verts, bg_idx);
            }

            // 下隣にペインがあれば 1px の水平境界線を描画する
            let bottom_row = layout.row_offset + layout.rows + 1;
            if state
                .pane_layouts
                .values()
                .any(|o| o.pane_id != layout.pane_id && o.row_offset == bottom_row)
            {
                add_px_rect(px, py + ph, pw, border_w, color, sw, sh, bg_verts, bg_idx);
            }
        }
    }

    /// タブバー頂点を構築する（ウィンドウ最上行、WezTerm スタイル）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_tab_bar_verts(
        &mut self,
        state: &mut ClientState,
        cfg: &nexterm_config::TabBarConfig,
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
        let bar_h = cfg.height as f32;
        let bar_y = 0.0_f32;
        // アクティブタブのアクセントライン高さ（3px でより視認性を高める）
        let accent_h = 3.0_f32;

        // タブバー全体の背景（非アクティブ色）
        let inactive_bg = hex_to_rgba(&cfg.inactive_tab_bg, 1.0);
        add_px_rect(0.0, bar_y, sw, bar_h, inactive_bg, sw, sh, bg_verts, bg_idx);
        // タブバー下端の区切り線（境界色と統一）
        add_px_rect(
            0.0,
            bar_y + bar_h - 1.0,
            sw,
            1.0,
            [0.255, 0.286, 0.408, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // フォーカスペインの ID で「アクティブタブ」を表示する
        let focused_id = state.focused_pane_id.unwrap_or(0);
        let active_bg = hex_to_rgba(&cfg.active_tab_bg, 1.0);
        let activity_bg = hex_to_rgba(&cfg.activity_tab_bg, 1.0);
        let accent_color = hex_to_rgba(&cfg.active_accent_color, 1.0);
        // テキスト色: アクティブは白に近い、非アクティブは設定値でミュート化（Sprint 5-7 / UI-1-1）
        let text_fg = [0.97, 0.97, 0.97, 1.0];
        let dim = cfg.inactive_text_brightness.clamp(0.2, 1.0);
        let inactive_fg = [dim, dim, dim, 1.0];

        let padding = cell_w;
        let sep = cfg.separator.clone();

        // 右端の設定ボタン幅を先に確保する（固定幅で絵文字の幅計算ズレを防ぐ）
        let settings_label = " * Settings ";
        let settings_w = 12.0 * cell_w;
        let tab_area_w = sw - settings_w;

        // ペイン ID 順にタブを並べる
        let mut pane_ids: Vec<u32> = state.pane_layouts.keys().copied().collect();
        pane_ids.sort();

        // クリック判定テーブルを毎フレーム更新する
        state.tab_hit_rects.clear();

        let mut x_offset = 0.0_f32;
        let text_y = bar_y + (bar_h - cell_h) / 2.0;

        for (i, &pane_id) in pane_ids.iter().enumerate() {
            let is_active = pane_id == focused_id;
            let is_hovered = state.hovered_tab_id == Some(pane_id);
            // アクティビティフラグ・タイトルを取得する
            let (has_activity, raw_title) = state
                .panes
                .get(&pane_id)
                .map(|p| (p.has_activity, p.title.clone()))
                .unwrap_or((false, String::new()));

            // タブラベル: OSC タイトルがあれば表示、なければペイン番号
            let base_label = if raw_title.is_empty() {
                format!("pane:{}", pane_id)
            } else {
                // 長すぎるタイトルは末尾を省略する（最大 24 文字）
                let truncated: String = raw_title.chars().take(24).collect();
                if raw_title.chars().count() > 24 {
                    format!("{}…", truncated)
                } else {
                    truncated
                }
            };
            // タブ番号プレフィックス（Windows Terminal 風）: 設定 ON で [N] を前置
            let numbered = if cfg.show_tab_number {
                format!("[{}] {}", i + 1, base_label)
            } else {
                base_label
            };
            let label = if has_activity && !is_active {
                format!(" {} ● ", numbered)
            } else {
                format!(" {} ", numbered)
            };
            let label_w =
                (label.chars().count() as f32 * cell_w + padding * 2.0).min(tab_area_w - x_offset); // タブエリアをはみ出さない

            if label_w < cell_w * 2.0 {
                break; // これ以上タブを描画するスペースがない
            }

            // タブ背景色を決定する:
            //   1. アクティブ → active_bg
            //   2. アクティビティあり非アクティブ → activity_bg（設定）
            //   3. ホバー中 → inactive_bg を明るく
            //   4. 通常 → inactive_bg
            let tab_bg = if is_active {
                active_bg
            } else if has_activity {
                activity_bg
            } else if is_hovered && cfg.hover_highlight {
                [
                    (inactive_bg[0] + 0.06).min(1.0),
                    (inactive_bg[1] + 0.06).min(1.0),
                    (inactive_bg[2] + 0.08).min(1.0),
                    inactive_bg[3],
                ]
            } else {
                inactive_bg
            };

            // タブ背景
            add_px_rect(
                x_offset, bar_y, label_w, bar_h, tab_bg, sw, sh, bg_verts, bg_idx,
            );
            // アクティブタブの下部にアクセントライン（設定色）を描画する
            if is_active {
                add_px_rect(
                    x_offset,
                    bar_y + bar_h - accent_h,
                    label_w,
                    accent_h,
                    accent_color,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // タブラベル（垂直中央揃え）
            let fg = if is_active { text_fg } else { inactive_fg };
            add_string_verts(
                &label,
                x_offset + padding,
                text_y,
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

            // クリック判定範囲を記録する
            state
                .tab_hit_rects
                .insert(pane_id, (x_offset, x_offset + label_w));

            x_offset += label_w;

            // タブ間の縦区切り線（1px、薄いアクセント色）
            if i + 1 < pane_ids.len() {
                // アクティブタブの隣は区切り線を非表示にする（アクセント線で十分）
                if !is_active && pane_ids[i + 1] != focused_id {
                    let line_h = bar_h * 0.6; // タブバー高さの60%
                    let line_y = bar_y + (bar_h - line_h) / 2.0;
                    add_px_rect(
                        x_offset,
                        line_y,
                        1.0,
                        line_h,
                        [0.25, 0.28, 0.38, 0.50],
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                // セパレーター文字列が設定されている場合は互換のために残す（空文字列がデフォルト）
                if !sep.trim().is_empty() {
                    let sep_w = cell_w;
                    let sep_bg = if is_active { active_bg } else { inactive_bg };
                    add_px_rect(
                        x_offset, bar_y, sep_w, bar_h, sep_bg, sw, sh, bg_verts, bg_idx,
                    );
                    add_string_verts(
                        &sep,
                        x_offset,
                        text_y,
                        inactive_fg,
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
                    x_offset += sep_w;
                }
            }
        }

        // 右端: 設定ボタン
        let settings_x = sw - settings_w;
        let settings_open = state.settings_panel.is_open;
        let settings_bg = if settings_open {
            active_bg
        } else {
            // 少し明るい非アクティブ色で識別しやすくする
            [
                inactive_bg[0] + 0.05,
                inactive_bg[1] + 0.05,
                inactive_bg[2] + 0.08,
                1.0,
            ]
        };
        add_px_rect(
            settings_x,
            bar_y,
            settings_w,
            bar_h,
            settings_bg,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        let settings_fg = if settings_open {
            text_fg
        } else {
            [0.80, 0.80, 0.80, 1.0]
        };
        add_string_verts(
            settings_label,
            settings_x,
            text_y,
            settings_fg,
            settings_open,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
        // 設定ボタンのクリック範囲を記録する
        state.settings_tab_rect = Some((settings_x, sw));

        // タブ名変更中の場合: 対象タブの位置にインライン編集フィールドを表示する
        if let Some(rename_id) = state.settings_panel.tab_rename_editing
            && let Some(&(tx0, tx1)) = state.tab_hit_rects.get(&rename_id)
        {
            let edit_w = (tx1 - tx0).min(tab_area_w - tx0);
            // 編集フィールド背景（濃いアクセント色）
            add_px_rect(
                tx0,
                bar_y,
                edit_w,
                bar_h,
                [0.231, 0.259, 0.384, 1.0],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            // 下部アクセントラインは太くして編集状態を示す
            add_px_rect(
                tx0,
                bar_y + bar_h - accent_h * 2.0,
                edit_w,
                accent_h * 2.0,
                [0.478, 0.635, 0.969, 1.0],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            // テキスト + カーソル（末尾に | を表示）
            let edit_text = format!(" {}|", state.settings_panel.tab_rename_text);
            add_string_verts(
                &edit_text,
                tx0 + padding,
                text_y,
                [1.0, 1.0, 1.0, 1.0],
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
        }
    }

    /// ステータスライン頂点を構築する（ウィンドウ最下行）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_status_verts(
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
        let py = sh - cell_h;
        // ステータスライン背景（Tokyo Night: #1E2030）
        add_px_rect(
            0.0,
            py,
            sw,
            cell_h,
            [0.118, 0.125, 0.188, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // ステータスライン上部に 1px の区切り線（#2D3149）
        add_px_rect(
            0.0,
            py,
            sw,
            1.0,
            [0.176, 0.192, 0.286, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // テキスト: N アイコン + セッション名 + ペイン情報
        let pane_id = state.focused_pane_id.unwrap_or(0);
        let activity_ids = state.active_pane_ids();
        let pane_count = state.pane_layouts.len();
        let status = if activity_ids.is_empty() {
            format!(" N  nexterm | pane:{}/{}", pane_id, pane_count)
        } else {
            let ids: Vec<String> = activity_ids.iter().map(|id| id.to_string()).collect();
            format!(
                " N  nexterm | pane:{}/{} | ●{}",
                pane_id,
                pane_count,
                ids.join(",")
            )
        };

        // Tokyo Night テキスト色 #A9B1D6
        add_string_verts(
            &status,
            0.0,
            py,
            [0.663, 0.694, 0.839, 1.0],
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

        // 右側ウィジェット（status_bar_right_text または旧 status_bar_text）を右端に表示する
        let right_widget_src = if !state.status_bar_right_text.is_empty() {
            &state.status_bar_right_text
        } else {
            &state.status_bar_text
        };
        let mut right_offset = 0.0f32;
        if !right_widget_src.is_empty() {
            let widget_text = format!(" {} ", right_widget_src);
            let text_w = widget_text.chars().count() as f32 * cell_w;
            right_offset = text_w;
            let right_px = sw - text_w;
            add_string_verts(
                &widget_text,
                right_px,
                py,
                [0.4, 0.9, 0.6, 1.0],
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

        // 左側ウィジェット（status_bar_text）が別途設定されていれば表示する
        // （right_widgets と独立して左寄せ表示）
        if !state.status_bar_right_text.is_empty() && !state.status_bar_text.is_empty() {
            let left_text = format!(" {} ", state.status_bar_text);
            let left_end = left_text.chars().count() as f32 * cell_w;
            // 左側ウィジェットは nexterm | pane: テキストの右に表示する
            let base_left = {
                let pane_id = state.focused_pane_id.unwrap_or(0);
                let activity_ids = state.active_pane_ids();
                let status = if activity_ids.is_empty() {
                    format!(" nexterm | pane:{}", pane_id)
                } else {
                    let ids: Vec<String> = activity_ids.iter().map(|id| id.to_string()).collect();
                    format!(" nexterm | pane:{} | activity:{}", pane_id, ids.join(","))
                };
                status.chars().count() as f32 * cell_w
            };
            let _ = left_end;
            let _ = base_left;
            // TODO: 左ウィジェットのオフセット計算は将来拡張
        }

        // 右端インジケーター群（右から左へ積み上げる）

        // ズームインジケーター（[Z] ラベルを黄色で表示）
        if state.is_zoomed {
            let zoom_text = " [Z] ";
            right_offset += zoom_text.chars().count() as f32 * cell_w;
            let right_px = sw - right_offset;
            add_string_verts(
                zoom_text,
                right_px,
                py,
                [1.0, 0.85, 0.2, 1.0],
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
        }

        // スクロールバック中はインジケーターをウィジェットの左に表示する
        if let Some(pane) = state.focused_pane()
            && pane.scroll_offset > 0
        {
            let scroll_text = format!(" ↑{} ", pane.scroll_offset);
            let right_px = sw - scroll_text.chars().count() as f32 * cell_w - right_offset;
            add_string_verts(
                &scroll_text,
                right_px,
                py,
                [1.0, 0.85, 0.2, 1.0],
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
        }
    }

    /// 検索バー頂点を構築する（ウィンドウ下部のオーバーレイ）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_search_verts(
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
        // ステータスラインの 1 行上に検索バーを表示する
        let py = sh - cell_h * 2.0;
        add_px_rect(
            0.0,
            py,
            sw,
            cell_h,
            [0.08, 0.10, 0.15, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上辺に細いアクセントラインを引く
        add_px_rect(
            0.0,
            py,
            sw,
            2.0,
            [0.3, 0.7, 1.0, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // 検索クエリとカーソル（点滅の代わりに常時 `|` を表示）
        let query_with_cursor = format!("{}|", state.search.query);
        let match_text = if let Some(idx) = state.search.current_match {
            format!("  ↑↓:{}", idx)
        } else if !state.search.query.is_empty() {
            "  (no match)".to_string()
        } else {
            String::new()
        };
        let label = format!(" / {}{}", query_with_cursor, match_text);
        add_string_verts(
            &label,
            0.0,
            py,
            [0.3, 1.0, 0.5, 1.0],
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

        // 右端にキー操作ヒントを表示する
        let hint = "Enter/↑ next  Shift+Enter/↑ prev  Esc close ";
        let hint_x = sw - hint.chars().count() as f32 * cell_w;
        add_string_verts(
            hint,
            hint_x.max(0.0),
            py,
            [0.55, 0.55, 0.55, 1.0],
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

    /// 更新通知バナー頂点を構築する（画面上部の 1 行バー）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_update_banner_verts(
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
        let Some(ref version) = state.update_banner else {
            return;
        };

        // バナーは画面幅全体・1 行分の高さ
        let bar_h = cell_h * 1.4;
        let bar_y = 0.0;

        // 背景（深緑）
        add_px_rect(
            0.0,
            bar_y,
            sw,
            bar_h,
            [0.05, 0.28, 0.18, 0.97],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 左端アクセントライン（明緑）
        add_px_rect(
            0.0,
            bar_y,
            4.0,
            bar_h,
            [0.15, 0.85, 0.45, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // 通知テキスト（i18n キー "update-available" を使用、{version} を置換）
        let raw = nexterm_i18n::fl!("update-available");
        let msg = raw.replace("{version}", version);
        add_string_verts(
            &msg,
            cell_w * 1.2,
            bar_y + (bar_h - cell_h) * 0.5,
            [0.88, 1.0, 0.88, 1.0],
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

        // 右側ヒント（Esc で閉じる）
        let hint = "  [Esc]";
        let hint_x = sw - hint.len() as f32 * cell_w - cell_w;
        add_string_verts(
            hint,
            hint_x,
            bar_y + (bar_h - cell_h) * 0.5,
            [0.55, 0.80, 0.55, 1.0],
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

    /// Quick Select オーバーレイ頂点を構築する
    ///
    /// 各マッチ位置にラベル（a, b, ..., aa, ...）を黄色背景で描画する。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_quick_select_verts(
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
        let qs = &state.quick_select;
        if !qs.is_active {
            return;
        }

        // フォーカスペインのオフセットを取得する
        let (pane_x, pane_y) = if let Some(pid) = state.focused_pane_id {
            if let Some(layout) = state.pane_layouts.get(&pid) {
                (
                    layout.col_offset as f32 * cell_w,
                    layout.row_offset as f32 * cell_h,
                )
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };

        for m in &qs.matches {
            let lx = pane_x + m.col_start as f32 * cell_w;
            let ly = pane_y + m.row as f32 * cell_h;
            let label_w = m.label.len() as f32 * cell_w;

            // マッチ全体をセミ透明ハイライト
            let match_w = (m.col_end - m.col_start) as f32 * cell_w;
            add_px_rect(
                lx,
                ly,
                match_w,
                cell_h,
                [0.9, 0.85, 0.2, 0.25],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );

            // ラベル背景（黄色）
            let is_partial_match =
                !qs.typed_label.is_empty() && m.label.starts_with(&qs.typed_label);
            let bg_color = if is_partial_match {
                [1.0, 0.6, 0.0, 0.95]
            } else {
                [0.9, 0.85, 0.1, 0.92]
            };
            add_px_rect(lx, ly, label_w, cell_h, bg_color, sw, sh, bg_verts, bg_idx);

            // ラベルテキスト（黒）
            add_string_verts(
                &m.label,
                lx,
                ly,
                [0.05, 0.05, 0.05, 1.0],
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
        }

        // 入力中ラベルを画面上部に表示する
        let typed = format!("Quick Select: {}_", qs.typed_label);
        add_px_rect(
            0.0,
            0.0,
            typed.len() as f32 * cell_w + cell_w,
            cell_h,
            [0.15, 0.15, 0.18, 0.92],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_string_verts(
            &typed,
            cell_w * 0.5,
            0.0,
            [1.0, 0.85, 0.2, 1.0],
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
    }
}
