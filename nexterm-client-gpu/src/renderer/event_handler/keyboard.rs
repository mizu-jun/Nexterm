//! Handler for winit `WindowEvent::KeyboardInput`.
//!
//! Extracted from `event_handler.rs`:
//! - `on_keyboard_input` — search-mode input / local-consumption check / server forwarding

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

        // Handle character input in search mode (do not forward to PTY).
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
            // Escape / Enter are handled by handle_key.
            if let PhysicalKey::Code(code) = physical_key
                && matches!(code, WKeyCode::Escape | WKeyCode::Enter)
            {
                self.handle_key(code, event_loop);
            }
            return;
        }

        // Check for local actions (palette, start search, etc.).
        let consumed = if let PhysicalKey::Code(code) = physical_key {
            self.handle_key(code, event_loop)
        } else {
            false
        };

        // While editing the settings-panel font-family field, append the character to the field.
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
            // Even when there is no text (arrow keys, etc.), do not forward to the server.
            return;
        }

        // Phase 5-11-8 Step 8-3 (Sub-phase A): character input while editing an SSH field.
        // Backspace / Delete / arrows / Enter / Esc are already handled in handle_key,
        // so here we only insert printable characters into TextInputState.
        if !consumed
            && self.app.state.settings_panel.is_open
            && self.app.state.settings_panel.ssh_field_editing.is_some()
        {
            if let Some(ref t) = text
                && !self.modifiers.control_key()
                && !self.modifiers.alt_key()
            {
                // Exclude control characters (Backspace=\x08, Tab=\x09, etc.) and
                // insert only printable characters.
                for ch in t.chars() {
                    if !ch.is_control() {
                        self.app.state.settings_panel.ssh_field_insert_char(ch);
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                return;
            }
            // Text-less events such as arrow keys are also not forwarded to the server.
            return;
        }

        // Phase 5-11-9 Sub-phase B: character input while editing a key
        // binding in Text mode. Record mode is handled by `handle_key` and
        // never reaches here (the key is consumed before character lookup).
        if !consumed
            && self.app.state.settings_panel.is_open
            && self.app.state.settings_panel.is_key_text_editing()
        {
            if let Some(ref t) = text
                && !self.modifiers.control_key()
                && !self.modifiers.alt_key()
            {
                for ch in t.chars() {
                    if !ch.is_control() {
                        self.app.state.settings_panel.key_field_insert_char(ch);
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                return;
            }
            return;
        }

        // If not consumed locally, forward to the server.
        if !consumed {
            self.forward_key_to_server(physical_key, text.as_deref());
        }
    }
}
