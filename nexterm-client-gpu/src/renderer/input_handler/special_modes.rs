//! Quick Select モードと同意ダイアログのキー入力
//!
//! `input_handler.rs` から抽出した:
//! - `handle_quick_select_key` — Quick Select ラベル入力
//! - `handle_consent_dialog_key` — Sprint 4-1 同意ダイアログのキー操作

use winit::keyboard::KeyCode as WKeyCode;

use super::EventHandler;
use crate::key_map::winit_code_to_char;

impl EventHandler {
    /// Quick Select モードのキー入力を処理する（true = 消費済み）
    pub(super) fn handle_quick_select_key(&mut self, code: WKeyCode) -> bool {
        match code {
            WKeyCode::Escape => {
                self.app.state.quick_select.exit();
                return true;
            }
            WKeyCode::Backspace => {
                self.app.state.quick_select.typed_label.pop();
                return true;
            }
            _ => {}
        }

        // アルファベットキーをラベル入力として受け取る
        if let Some(ch) = winit_code_to_char(code) {
            self.app.state.quick_select.typed_label.push(ch);

            // マッチが確定したらクリップボードにコピーして終了
            if let Some(m) = self.app.state.quick_select.accept() {
                let text = m.text.clone();
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
                self.app.state.quick_select.exit();
            }
        }

        true
    }

    /// 同意ダイアログのキーボード処理（Sprint 4-1）
    ///
    /// キー割当:
    /// - Y / Enter: 1 度許可
    /// - N / Esc:   1 度拒否
    /// - A:         セッション中常に許可
    /// - D:         セッション中常に拒否
    /// - 矢印 / Tab: 選択ボタン移動
    pub(super) fn handle_consent_dialog_key(&mut self, code: WKeyCode) -> bool {
        match code {
            WKeyCode::KeyY | WKeyCode::Enter => {
                self.resolve_pending_consent(Some(true), false);
            }
            WKeyCode::KeyN | WKeyCode::Escape => {
                self.resolve_pending_consent(Some(false), false);
            }
            WKeyCode::KeyA => {
                self.resolve_pending_consent(Some(true), true);
            }
            WKeyCode::KeyD => {
                self.resolve_pending_consent(Some(false), true);
            }
            WKeyCode::ArrowLeft | WKeyCode::ArrowRight | WKeyCode::Tab => {
                if let Some(dialog) = self.app.state.pending_consent.as_mut() {
                    let dir = if code == WKeyCode::ArrowLeft { 3 } else { 1 };
                    dialog.selected = (dialog.selected + dir) % 4;
                }
            }
            _ => {
                // 他のキーは消費するが何もしない（誤入力で予期せぬ操作を防ぐ）
            }
        }
        true
    }
}
