//! モーダル系オーバーレイの頂点ビルダー。
//!
//! パスワード入力モーダル / コンテキストメニュー / 同意ダイアログを担当する。

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::{ClientState, ContextMenu};
use crate::vertex_util::{add_px_rect, add_string_verts, visual_width};

use super::super::WgpuState;
use super::util::{pane_id_for, preview_text, wrap_text};

impl WgpuState {
    /// パスワード入力モーダルの頂点を構築する
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_password_modal_verts(
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
        let Some(modal) = &state.host_manager.password_modal else {
            return;
        };

        let pw = 44.0 * cell_w;
        let ph = 6.0 * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // 背景（濃い紺）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.06, 0.10, 0.20, 0.97],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上端アクセント線（オレンジ）
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            [0.9, 0.5, 0.1, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // タイトル
        let title = format!(
            "Password: {}@{}:{}",
            modal.host.username, modal.host.host, modal.host.port
        );
        add_string_verts(
            &title,
            px + cell_w,
            py + cell_h * 0.15,
            [0.9, 0.6, 0.2, 1.0],
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

        // パスワード入力欄（マスク表示）
        // HIGH H-6: input は private な Zeroizing<String> なので input_len() 経由で文字数のみ取得
        let masked = "*".repeat(modal.input_len());
        let prompt = format!("> {}_", masked);
        add_string_verts(
            &prompt,
            px + cell_w,
            py + cell_h * 1.3,
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

        // エラーメッセージ
        if let Some(err) = &modal.error {
            add_string_verts(
                err,
                px + cell_w,
                py + cell_h * 2.5,
                [1.0, 0.3, 0.3, 1.0],
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

        // remember 状態（OS キーチェーン保存トグル）
        let remember_label = if modal.remember {
            "[X] OS キーチェーンに保存する (Tab で切替)"
        } else {
            "[ ] OS キーチェーンに保存する (Tab で切替)"
        };
        let remember_color = if modal.remember {
            [0.4, 0.9, 0.5, 1.0]
        } else {
            [0.6, 0.6, 0.6, 1.0]
        };
        add_string_verts(
            remember_label,
            px + cell_w,
            py + cell_h * 3.2,
            remember_color,
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
        if modal.prefilled {
            add_string_verts(
                "(キーチェーンから自動入力済み)",
                px + cell_w,
                py + cell_h * 2.0,
                [0.5, 0.7, 1.0, 1.0],
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

        // ヒント
        add_string_verts(
            "Enter=接続  Tab=保存切替  Esc=キャンセル",
            px + cell_w,
            py + cell_h * 4.1,
            [0.45, 0.50, 0.48, 1.0],
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

    /// コンテキストメニュー頂点を構築する（右クリック時のポップアップ）
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_context_menu_verts(
        &self,
        menu: &ContextMenu,
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
        // ラベルとヒントの最大表示幅からメニュー幅を動的に計算する
        let max_label_w = menu
            .items
            .iter()
            .map(|item| visual_width(&item.label))
            .max()
            .unwrap_or(8);
        let max_hint_w = menu
            .items
            .iter()
            .map(|item| visual_width(&item.hint))
            .max()
            .unwrap_or(0);
        // 左パディング(0.9) + ラベル + ギャップ(2) + ヒント + 右パディング(1.5)
        let min_cells = max_label_w + max_hint_w + 5;
        let menu_w = (min_cells as f32).max(16.0) * cell_w;
        let menu_h = menu.items.len() as f32 * cell_h;
        let mx = menu.x;
        let my = menu.y;

        // ドロップシャドウ（3px オフセット）
        add_px_rect(
            mx + 3.0,
            my + 3.0,
            menu_w,
            menu_h,
            [0.02, 0.02, 0.04, 0.80],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // 枠線（外側 1px、アクセントカラー薄め）
        add_px_rect(
            mx - 1.0,
            my - 1.0,
            menu_w + 2.0,
            menu_h + 2.0,
            [0.478, 0.635, 0.969, 0.15],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // メニュー全体の背景（完全不透明: ターミナル透過設定に関わらず常に不透明）
        add_px_rect(
            mx,
            my,
            menu_w,
            menu_h,
            [0.10, 0.11, 0.18, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // 上端のアクセント線（3px 太め）
        add_px_rect(
            mx,
            my,
            menu_w,
            3.0,
            [0.478, 0.635, 0.969, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        for (i, item) in menu.items.iter().enumerate() {
            use crate::state::ContextMenuAction;
            let item_y = my + i as f32 * cell_h;

            if matches!(item.action, ContextMenuAction::Separator) {
                // セパレーター: 中央に水平線を描く
                let sep_y = item_y + cell_h * 0.45;
                add_px_rect(
                    mx + cell_w * 0.5,
                    sep_y,
                    menu_w - cell_w,
                    1.0,
                    [0.28, 0.32, 0.45, 0.70],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                continue;
            }

            // ホバーハイライト背景（セパレーター以外）
            if menu.hovered == Some(i) {
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    menu_w - 4.0,
                    cell_h - 2.0,
                    [0.149, 0.200, 0.320, 0.90],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // ホバー時の左アクセント線（3px）
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    3.0,
                    cell_h - 2.0,
                    [0.478, 0.635, 0.969, 0.90],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // ラベルテキスト（左パディング 0.9セル分）
            let text_color = if menu.hovered == Some(i) {
                [0.95, 0.96, 1.0, 1.0] // ホバー時は少し明るく
            } else {
                [0.75, 0.78, 0.88, 1.0] // 通常は少し抑えた色
            };
            add_string_verts(
                &item.label,
                mx + cell_w * 0.9,
                item_y + cell_h * 0.1,
                text_color,
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

            // キーヒントテキスト（右寄せ、グレー）
            if !item.hint.is_empty() {
                let hint_visual_w = visual_width(&item.hint) as f32;
                let hint_x = mx + menu_w - (hint_visual_w * cell_w + cell_w * 0.5);
                add_string_verts(
                    &item.hint,
                    hint_x,
                    item_y + cell_h * 0.1,
                    [0.45, 0.48, 0.60, 0.80],
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

    /// 同意ダイアログ（Sprint 4-1: 機密操作確認モーダル）の頂点を構築する
    ///
    /// 中央フローティング。種別に応じてタイトル・プレビュー・ボタンを描画する。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_consent_dialog_verts(
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
        use crate::state::ConsentKind;

        let Some(dialog) = &state.pending_consent else {
            return;
        };

        // 背景半透明オーバーレイ（画面全体）
        add_px_rect(
            0.0,
            0.0,
            sw,
            sh,
            [0.0, 0.0, 0.0, 0.55],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // ダイアログ寸法（横 60 セル、縦 12 セル）
        let pw = (60.0 * cell_w).min(sw - cell_w * 4.0);
        let ph = 12.0 * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // 背景（濃い紺）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.08, 0.12, 0.20, 0.97],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上端アクセント線（黄色 = 警告色）
        add_px_rect(
            px,
            py,
            pw,
            3.0,
            [0.95, 0.75, 0.15, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // タイトル
        let title_key = match dialog.kind {
            ConsentKind::OpenUrl(_) => "consent-title-open-url",
            ConsentKind::ClipboardWrite { .. } => "consent-title-clipboard-write",
            ConsentKind::Notification { .. } => "consent-title-notification",
        };
        let title = nexterm_i18n::t(title_key);
        add_string_verts(
            &title,
            px + cell_w,
            py + cell_h * 0.4,
            [0.95, 0.75, 0.15, 1.0],
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

        // 要求元ペイン情報
        let mut content_y = py + cell_h * 1.8;
        if let Some(pane_id) = pane_id_for(&dialog.kind) {
            let label = nexterm_i18n::fl!("consent-source-pane", pane_id = pane_id);
            add_string_verts(
                &label,
                px + cell_w,
                content_y,
                [0.7, 0.7, 0.8, 1.0],
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
            content_y += cell_h * 1.3;
        }

        // ペイロードプレビュー（最大 2 行・各行 56 文字）
        let preview = preview_text(&dialog.kind);
        for (i, line) in wrap_text(&preview, 56).iter().take(2).enumerate() {
            add_string_verts(
                line,
                px + cell_w,
                content_y + i as f32 * cell_h,
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
        }

        // ボタン行（4 ボタン、選択中はハイライト）
        let buttons = [
            nexterm_i18n::t("consent-allow"),
            nexterm_i18n::t("consent-deny"),
            nexterm_i18n::t("consent-always-allow"),
            nexterm_i18n::t("consent-always-deny"),
        ];
        let btn_y = py + ph - cell_h * 2.6;
        let total_w: f32 = buttons
            .iter()
            .map(|b| visual_width(b) as f32 * cell_w + cell_w * 2.0)
            .sum();
        let mut bx = px + (pw - total_w) / 2.0;
        for (i, btn) in buttons.iter().enumerate() {
            let bw = visual_width(btn) as f32 * cell_w + cell_w * 1.5;
            let is_selected = dialog.selected == i;
            let bg = if is_selected {
                [0.95, 0.75, 0.15, 1.0]
            } else {
                [0.16, 0.20, 0.30, 1.0]
            };
            add_px_rect(bx, btn_y, bw, cell_h * 1.4, bg, sw, sh, bg_verts, bg_idx);
            let fg = if is_selected {
                [0.10, 0.10, 0.10, 1.0]
            } else {
                [0.95, 0.95, 0.95, 1.0]
            };
            add_string_verts(
                btn,
                bx + cell_w * 0.75,
                btn_y + cell_h * 0.2,
                fg,
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
            bx += bw + cell_w * 0.5;
        }

        // 操作ヒント（最下行）
        let hint = nexterm_i18n::t("consent-hint");
        add_string_verts(
            &hint,
            px + cell_w,
            py + ph - cell_h * 1.0,
            [0.6, 0.6, 0.7, 1.0],
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
