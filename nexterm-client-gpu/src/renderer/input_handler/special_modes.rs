//! Key input for Quick Select mode and consent dialogs
//!
//! Extracted from `input_handler.rs`:
//! - `handle_quick_select_key` — Quick Select label input
//! - `handle_consent_dialog_key` — Sprint 4-1 consent dialog key operations

use winit::keyboard::KeyCode as WKeyCode;

use super::EventHandler;
use crate::key_map::winit_code_to_char;

impl EventHandler {
    /// Handle key input in Quick Select mode (true = consumed)
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

        // Accept alphabetic keys as label input
        if let Some(ch) = winit_code_to_char(code) {
            self.app.state.quick_select.typed_label.push(ch);

            // On a confirmed match, copy to the clipboard and exit
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

    /// Keyboard handling for the consent dialog (Sprint 4-1)
    ///
    /// Key bindings:
    /// - Y / Enter: allow once
    /// - N / Esc:   deny once
    /// - A:         always allow for this session
    /// - D:         always deny for this session
    /// - Arrows / Tab: move the selected button
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
                // Consume other keys without doing anything (prevent accidental actions)
            }
        }
        true
    }

    /// Keyboard handling for the Window-close confirmation dialog (Sprint 5-9 Phase 4-6)
    ///
    /// Key bindings:
    /// - Enter / Y: confirm the current selection (selected_button = 0 → Kill, 1 → Cancel)
    /// - Esc / N:   cancel (writes 0xFF to selected_button so the poll consumes it)
    /// - ←:         focus the Kill button (selected_button = 0)
    /// - → / Tab:   focus the Cancel button (selected_button = 1)
    ///
    /// Confirmation signal values:
    /// - `0xFE` = Kill confirmed (`poll_pending_close_request` runs KillSession + exit on the next frame)
    /// - `0xFF` = Cancel confirmed (`poll_pending_close_request` clears the pending state on the next frame)
    pub(super) fn handle_close_window_dialog_key(&mut self, code: WKeyCode) -> bool {
        let Some(dialog) = self.app.state.close_window_dialog.as_mut() else {
            return false;
        };
        match code {
            WKeyCode::Enter | WKeyCode::KeyY => {
                // Confirm or cancel based on the currently selected button
                dialog.selected_button = if dialog.selected_button == 0 {
                    0xFE // Kill confirmed
                } else {
                    0xFF // Cancel confirmed
                };
            }
            WKeyCode::Escape | WKeyCode::KeyN => {
                // Force cancel (safe default)
                dialog.selected_button = 0xFF;
            }
            WKeyCode::ArrowLeft => {
                dialog.selected_button = 0; // Focus Kill
            }
            WKeyCode::ArrowRight | WKeyCode::Tab => {
                dialog.selected_button = 1; // Focus Cancel
            }
            _ => {
                // Consume other keys without doing anything (avoid closing accidentally)
            }
        }
        // Trigger a redraw
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        true
    }
}
