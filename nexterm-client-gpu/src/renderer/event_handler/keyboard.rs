//! winit `WindowEvent::KeyboardInput` のハンドラ
//!
//! `event_handler.rs` から抽出した:
//! - `on_keyboard_input` — 検索モード入力 / ローカル消費判定 / サーバー転送

use winit::{
    event::KeyEvent,
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode as WKeyCode, PhysicalKey},
};

use super::EventHandler;

impl EventHandler {
    /// `WindowEvent::KeyboardInput`
    pub(super) fn on_keyboard_input(&mut self, key_event: KeyEvent, event_loop: &ActiveEventLoop) {
        let KeyEvent {
            physical_key, text, ..
        } = key_event;

        // 検索モードの文字入力を処理する（PTY には転送しない）
        if self.app.state.search.is_active {
            if matches!(physical_key, PhysicalKey::Code(WKeyCode::Backspace)) {
                self.app.state.pop_search_char();
            } else if let Some(ref t) = text
                && !self.modifiers.control_key()
            {
                for ch in t.chars() {
                    self.app.state.push_search_char(ch);
                }
            }
            // Escape / Enter は handle_key で処理する
            if let PhysicalKey::Code(code) = physical_key
                && matches!(code, WKeyCode::Escape | WKeyCode::Enter)
            {
                self.handle_key(code, event_loop);
            }
            return;
        }

        // ローカル操作（パレット・検索開始など）をチェックする
        let consumed = if let PhysicalKey::Code(code) = physical_key {
            self.handle_key(code, event_loop)
        } else {
            false
        };

        // 設定パネルのフォントファミリー入力中は文字をフィールドに追加する
        if !consumed
            && self.app.state.settings_panel.is_open
            && self.app.state.settings_panel.font_family_editing
        {
            if let Some(ref t) = text
                && !self.modifiers.control_key()
                && !self.modifiers.alt_key()
            {
                for ch in t.chars() {
                    self.app.state.settings_panel.push_font_family_char(ch);
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                return;
            }
            // テキストがない場合（矢印キー等）もサーバーへは転送しない
            return;
        }

        // ローカルで消費されなかった場合はサーバーへ転送する
        if !consumed {
            self.forward_key_to_server(physical_key, text.as_deref());
        }
    }
}
