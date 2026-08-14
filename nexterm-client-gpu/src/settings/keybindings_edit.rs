//! In-flight edit-buffer manipulation for the Keybindings category:
//! the key-string text editor, key recording, and the leader-key editor.
//! List management (add/delete/select) lives in `keybindings.rs`.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::{KeyEditMode, SettingsPanel, TextInputState};

impl SettingsPanel {
    /// Start Text edit mode initialised with the current key string.
    /// Used to enter Text mode directly without going through Record first.
    /// Currently exercised by tests; the in-UI entry path is Enter → Record → Tab.
    #[allow(dead_code)]
    pub fn begin_key_text_edit(&mut self) -> bool {
        if self.keybindings.is_empty() {
            return false;
        }
        let Some(kb) = self.keybindings.get(self.selected_key_index) else {
            return false;
        };
        self.key_editing = Some(KeyEditMode::Text(TextInputState::new(kb.key.clone())));
        true
    }

    /// Commit the in-flight Text buffer back to the selected binding's key.
    /// Returns `true` when a write-back happened. Record mode is a no-op
    /// here (Record commits on capture).
    pub fn commit_key_edit(&mut self) -> bool {
        let Some(KeyEditMode::Text(state)) = self.key_editing.take() else {
            return false;
        };
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        kb.key = state.buffer;
        self.dirty = true;
        true
    }

    /// Discard any in-flight buffer and leave edit mode.
    /// Returns `true` if edit mode was active.
    pub fn cancel_key_edit(&mut self) -> bool {
        self.key_editing.take().is_some()
    }

    /// Insert a single character into the in-flight Text buffer.
    /// No-op in Record mode or when not editing.
    pub fn key_field_insert_char(&mut self, ch: char) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.insert_char(ch);
        }
    }

    /// Insert a string into the in-flight Text buffer (IME Commit path).
    pub fn key_field_insert_str(&mut self, s: &str) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.insert_str(s);
        }
    }

    /// Backspace in the in-flight Text buffer.
    pub fn key_field_backspace(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.backspace();
        }
    }

    /// Forward-delete in the in-flight Text buffer.
    pub fn key_field_delete(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.delete_forward();
        }
    }

    /// Move cursor left by one character in the in-flight Text buffer.
    pub fn key_field_move_left(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.move_left();
        }
    }

    /// Move cursor right by one character in the in-flight Text buffer.
    pub fn key_field_move_right(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.move_right();
        }
    }

    /// Move cursor to the start of the in-flight Text buffer.
    pub fn key_field_move_home(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.move_home();
        }
    }

    /// Move cursor to the end of the in-flight Text buffer.
    pub fn key_field_move_end(&mut self) {
        if let Some(KeyEditMode::Text(state)) = self.key_editing.as_mut() {
            state.move_end();
        }
    }

    /// Start editing `leader_key` (focus must be on field 5).
    /// Returns `true` when edit mode actually started.
    pub fn begin_leader_key_edit(&mut self) -> bool {
        if self.focused_widget_index != 5 {
            return false;
        }
        self.leader_key_editing = Some(TextInputState::new(self.leader_key.clone()));
        true
    }

    /// Commit the in-flight `leader_key` buffer and leave edit mode.
    /// Returns `true` when a write-back happened.
    pub fn commit_leader_key_edit(&mut self) -> bool {
        let Some(state) = self.leader_key_editing.take() else {
            return false;
        };
        self.leader_key = state.buffer;
        self.dirty = true;
        true
    }

    /// Discard the in-flight `leader_key` edit. Returns `true` if edit mode was active.
    pub fn cancel_leader_key_edit(&mut self) -> bool {
        self.leader_key_editing.take().is_some()
    }

    /// Insert one character at the cursor inside the in-flight buffer.
    pub fn leader_key_insert_char(&mut self, ch: char) {
        if let Some(state) = self.leader_key_editing.as_mut() {
            state.insert_char(ch);
        }
    }

    /// Delete the character immediately before the cursor (Backspace).
    pub fn leader_key_backspace(&mut self) {
        if let Some(state) = self.leader_key_editing.as_mut() {
            state.backspace();
        }
    }

    /// Delete the character immediately after the cursor (Delete).
    pub fn leader_key_delete(&mut self) {
        if let Some(state) = self.leader_key_editing.as_mut() {
            state.delete_forward();
        }
    }

    /// Move the in-flight cursor one character left.
    pub fn leader_key_move_left(&mut self) {
        if let Some(state) = self.leader_key_editing.as_mut() {
            state.move_left();
        }
    }

    /// Move the in-flight cursor one character right.
    pub fn leader_key_move_right(&mut self) {
        if let Some(state) = self.leader_key_editing.as_mut() {
            state.move_right();
        }
    }

    /// Move the in-flight cursor to the start of the buffer.
    pub fn leader_key_move_home(&mut self) {
        if let Some(state) = self.leader_key_editing.as_mut() {
            state.move_home();
        }
    }

    /// Move the in-flight cursor to the end of the buffer.
    pub fn leader_key_move_end(&mut self) {
        if let Some(state) = self.leader_key_editing.as_mut() {
            state.move_end();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::KeyBindingEntry;
    use nexterm_config::Config;

    /// Duplicated from `keybindings.rs`'s own `tests` module: both test
    /// modules need a panel pre-seeded with one binding, and they are
    /// separate `mod tests` (one per split file), so the helper cannot be
    /// shared via `use super::*`.
    fn panel_with_one_binding() -> SettingsPanel {
        SettingsPanel {
            keybindings: vec![KeyBindingEntry {
                key: "ctrl+shift+p".to_string(),
                action: "CommandPalette".to_string(),
            }],
            selected_key_index: 0,
            ..Default::default()
        }
    }

    #[test]
    fn toggle_key_edit_mode_record_to_text_preserves_value() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_record();
        panel.toggle_key_edit_mode();
        assert!(panel.is_key_text_editing());
        // Buffer is seeded with the current binding's key value.
        if let Some(KeyEditMode::Text(state)) = &panel.key_editing {
            assert_eq!(state.buffer, "ctrl+shift+p");
        } else {
            panic!("expected Text mode after toggle");
        }
    }

    #[test]
    fn toggle_key_edit_mode_text_to_record_discards_buffer() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_text_edit();
        // Type a character into the buffer.
        panel.key_field_insert_char('a');
        panel.toggle_key_edit_mode();
        assert!(panel.is_key_recording());
        // Original binding key untouched.
        assert_eq!(panel.keybindings[0].key, "ctrl+shift+p");
    }

    #[test]
    fn commit_key_edit_writes_text_buffer() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_text_edit();
        // Replace the buffer with a prefix binding.
        if let Some(KeyEditMode::Text(state)) = panel.key_editing.as_mut() {
            state.buffer = "ctrl+b d".to_string();
            state.cursor = state.buffer.len();
        }
        assert!(panel.commit_key_edit());
        assert_eq!(panel.keybindings[0].key, "ctrl+b d");
        assert!(panel.key_editing.is_none());
        assert!(panel.dirty);
    }

    #[test]
    fn cancel_key_edit_discards_buffer() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_text_edit();
        panel.key_field_insert_char('x');
        assert!(panel.cancel_key_edit());
        // Original binding survives, edit state cleared.
        assert_eq!(panel.keybindings[0].key, "ctrl+shift+p");
        assert!(panel.key_editing.is_none());
    }

    #[test]
    fn key_field_text_edit_methods_proxy_to_state() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_text_edit();
        panel.key_field_move_home();
        panel.key_field_insert_str("abc");
        if let Some(KeyEditMode::Text(state)) = &panel.key_editing {
            assert_eq!(state.buffer, "abcctrl+shift+p");
            assert_eq!(state.cursor, 3);
        }
        panel.key_field_move_end();
        panel.key_field_backspace();
        if let Some(KeyEditMode::Text(state)) = &panel.key_editing {
            // Last char ('p') was removed.
            assert_eq!(state.buffer, "abcctrl+shift+");
        }
    }

    #[test]
    fn close_panel_resets_key_editing() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_record();
        panel.close();
        assert!(panel.key_editing.is_none());
    }

    #[test]
    fn save_writes_leader_key() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.leader_key = "ctrl+q".to_string();
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("leader_key = \"ctrl+q\""));
    }

    #[test]
    fn leader_key_edit_round_trip() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.focused_widget_index = 5;
        assert!(panel.begin_leader_key_edit());
        panel.leader_key_backspace();
        panel.leader_key_insert_char('x');
        assert!(panel.commit_leader_key_edit());
        assert!(panel.leader_key.ends_with('x'));
        assert!(panel.leader_key_editing.is_none());
    }
}
