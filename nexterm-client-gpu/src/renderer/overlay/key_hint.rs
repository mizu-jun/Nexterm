//! キーヒントオーバーレイの頂点ビルダー（Sprint 5-7 / UI-1-4）。
//!
//! WezTerm の `show_active_key_table` 相当。Leader 単独押下後 2 秒間、
//! 画面下部に半透明オーバーレイで `<leader> ...` または `ctrl+b ...` 形式の
//! バインド一覧を表示する。

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::super::WgpuState;

impl WgpuState {
    /// キーヒントオーバーレイの頂点を構築する。
    ///
    /// `state.key_hint_visible_until` が `Some(時刻)` かつ現在時刻より未来のときに描画する。
    /// 画面下部に半透明バナーを置き、config.keys から prefix 系（leader_key で始まる、
    /// または `<leader>` を含む）バインドの末尾キーと action 名を一覧表示する。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_key_hint_verts(
        &self,
        state: &ClientState,
        cfg: &nexterm_config::Config,
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
        // 表示判定: 期限が Some かつ現在時刻より未来
        let visible = match state.key_hint_visible_until {
            Some(t) => std::time::Instant::now() < t,
            None => false,
        };
        if !visible {
            return;
        }

        // 表示対象のバインドを抽出する:
        //   1. `<leader> ...` で始まる（明示的に leader を使う）
        //   2. `<leader_key> ...`（例: `ctrl+b ...`）で始まる（後方互換 tmux 互換）
        let leader = &cfg.leader_key;
        let mut hints: Vec<(String, String)> = Vec::new();
        for binding in &cfg.keys {
            let key = &binding.key;
            // スペース区切りで 2 トークン以上のもののみ（prefix 系）
            let tokens: Vec<&str> = key.split_whitespace().collect();
            if tokens.len() < 2 {
                continue;
            }
            let first = tokens[0];
            let is_leader_prefix =
                first == "<leader>" || first.eq_ignore_ascii_case(leader.as_str());
            if !is_leader_prefix {
                continue;
            }
            // 後半キー（先頭以外を全部結合）
            let rest = tokens[1..].join(" ");
            hints.push((rest, binding.action.clone()));
        }
        // 重複を除いた表示用に最大 12 件
        hints.dedup_by(|a, b| a.0 == b.0);
        hints.truncate(12);

        if hints.is_empty() {
            return;
        }

        // バナー高さ: ヘッダ 1 行 + 各エントリ 1 行
        let lines = (hints.len() as f32) + 1.0;
        let pad = cell_w * 0.6;
        let banner_h = lines * cell_h + pad * 2.0;
        let banner_w = sw.min(80.0 * cell_w);
        let bx = (sw - banner_w) / 2.0;
        let by = sh - banner_h - pad;

        // 背景（半透明濃紺）
        add_px_rect(
            bx,
            by,
            banner_w,
            banner_h,
            [0.06, 0.10, 0.20, 0.92],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上端アクセント線（leader 表示色: Tokyo Night blue）
        add_px_rect(
            bx,
            by,
            banner_w,
            2.0,
            [0.478, 0.635, 0.969, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // ヘッダ
        let header = format!(" Leader: {} — 次のキーを押してください ", leader);
        add_string_verts(
            &header,
            bx + pad,
            by + pad,
            [0.95, 0.95, 0.95, 1.0],
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

        // 各エントリ
        let key_fg = [0.85, 0.90, 1.0, 1.0];
        let action_fg = [0.75, 0.75, 0.75, 1.0];
        for (i, (key, action)) in hints.iter().enumerate() {
            let row_y = by + pad + cell_h + (i as f32 * cell_h);
            let key_x = bx + pad;
            add_string_verts(
                &format!(" {:<8}", key),
                key_x,
                row_y,
                key_fg,
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
                &format!(" → {}", action),
                key_x + 10.0 * cell_w,
                row_y,
                action_fg,
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
