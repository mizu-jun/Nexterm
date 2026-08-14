//! Startup category: language selection, update-check toggle, and the
//! shell program/args editing fields.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::{SettingsPanel, TextInputState};

impl SettingsPanel {
    /// Used by SR via `Action::Click`: toggle the "check for updates at startup" box.
    pub fn toggle_auto_check_update(&mut self) {
        self.auto_check_update = !self.auto_check_update;
        self.dirty = true;
    }

    /// Return the currently selected language code.
    pub fn language_code(&self) -> &str {
        LANGUAGE_OPTIONS
            .get(self.language_index)
            .map(|(_, code)| *code)
            .unwrap_or("auto")
    }

    /// Switch to the next language.
    pub fn next_language(&mut self) {
        self.language_index = (self.language_index + 1) % LANGUAGE_OPTIONS.len();
        self.dirty = true;
    }

    /// Switch to the previous language.
    pub fn prev_language(&mut self) {
        let len = LANGUAGE_OPTIONS.len();
        self.language_index = (self.language_index + len - 1) % len;
        self.dirty = true;
    }

    /// Total number of fields in the Startup category.
    pub const STARTUP_FIELD_COUNT: u16 = 4;

    /// Move focus to the next Startup field (stops at the last one).
    pub fn next_startup_field(&mut self) -> bool {
        if self.focused_widget_index + 1 < Self::STARTUP_FIELD_COUNT {
            self.focused_widget_index += 1;
            true
        } else {
            false
        }
    }

    /// Move focus to the previous Startup field (stops at the first one).
    pub fn prev_startup_field(&mut self) -> bool {
        if self.focused_widget_index > 0 {
            self.focused_widget_index -= 1;
            true
        } else {
            false
        }
    }

    /// Start editing the focused shell field (`focused_widget_index` 2 or 3).
    /// Returns `true` when edit mode actually started.
    pub fn begin_shell_field_edit(&mut self) -> bool {
        let initial = match self.focused_widget_index {
            2 => self.shell_program.clone(),
            3 => self.shell_args.clone(),
            _ => return false,
        };
        self.shell_field_editing = Some(TextInputState::new(initial));
        true
    }

    /// Commit the in-flight shell-field buffer back into `shell_program` /
    /// `shell_args` and leave edit mode. Returns `true` when a write-back happened.
    pub fn commit_shell_field_edit(&mut self) -> bool {
        let Some(state) = self.shell_field_editing.take() else {
            return false;
        };
        match self.focused_widget_index {
            2 => self.shell_program = state.buffer,
            3 => self.shell_args = state.buffer,
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// Discard the in-flight shell-field edit. Returns `true` if edit mode was active.
    pub fn cancel_shell_field_edit(&mut self) -> bool {
        self.shell_field_editing.take().is_some()
    }

    /// Insert one character at the cursor inside the in-flight buffer.
    pub fn shell_field_insert_char(&mut self, ch: char) {
        if let Some(state) = self.shell_field_editing.as_mut() {
            state.insert_char(ch);
        }
    }

    /// Delete the character immediately before the cursor (Backspace).
    pub fn shell_field_backspace(&mut self) {
        if let Some(state) = self.shell_field_editing.as_mut() {
            state.backspace();
        }
    }

    /// Delete the character immediately after the cursor (Delete).
    pub fn shell_field_delete(&mut self) {
        if let Some(state) = self.shell_field_editing.as_mut() {
            state.delete_forward();
        }
    }

    /// Move the in-flight cursor one character left.
    pub fn shell_field_move_left(&mut self) {
        if let Some(state) = self.shell_field_editing.as_mut() {
            state.move_left();
        }
    }

    /// Move the in-flight cursor one character right.
    pub fn shell_field_move_right(&mut self) {
        if let Some(state) = self.shell_field_editing.as_mut() {
            state.move_right();
        }
    }

    /// Move the in-flight cursor to the start of the buffer.
    pub fn shell_field_move_home(&mut self) {
        if let Some(state) = self.shell_field_editing.as_mut() {
            state.move_home();
        }
    }

    /// Move the in-flight cursor to the end of the buffer.
    pub fn shell_field_move_end(&mut self) {
        if let Some(state) = self.shell_field_editing.as_mut() {
            state.move_end();
        }
    }
}

/// Language choices: (display name, language code).
///
/// The display names are intentionally written in each language's native script
/// so the picker shows them in their own form.
pub const LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("Auto (OS)", "auto"),
    ("English", "en"),
    ("日本語", "ja"),
    ("Français", "fr"),
    ("Deutsch", "de"),
    ("Español", "es"),
    ("Italiano", "it"),
    ("中文(简体)", "zh-CN"),
    ("한국어", "ko"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn startup_field_navigation_stops_at_bounds() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert_eq!(panel.focused_widget_index, 0);

        assert!(panel.next_startup_field());
        assert_eq!(panel.focused_widget_index, 1);
        assert!(panel.next_startup_field());
        assert_eq!(panel.focused_widget_index, 2);
        assert!(panel.next_startup_field());
        assert_eq!(panel.focused_widget_index, 3);
        assert!(
            !panel.next_startup_field(),
            "the last field must report no further movement"
        );
        assert_eq!(panel.focused_widget_index, 3);

        assert!(panel.prev_startup_field());
        assert_eq!(panel.focused_widget_index, 2);
    }

    #[test]
    fn shell_field_edit_lifecycle_program() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.focused_widget_index = 2; // shell program
        panel.shell_program = "/bin/bash".to_string();

        assert!(panel.begin_shell_field_edit());
        assert!(panel.shell_field_editing.is_some());

        // Clear and retype.
        for _ in 0.."/bin/bash".len() {
            panel.shell_field_backspace();
        }
        for ch in "/usr/bin/zsh".chars() {
            panel.shell_field_insert_char(ch);
        }

        assert!(panel.commit_shell_field_edit());
        assert_eq!(panel.shell_program, "/usr/bin/zsh");
        assert!(panel.shell_field_editing.is_none());
        assert!(panel.dirty);
    }

    #[test]
    fn shell_field_edit_lifecycle_args_and_cancel() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.focused_widget_index = 3; // shell args
        panel.shell_args = "-l".to_string();

        assert!(panel.begin_shell_field_edit());
        panel.shell_field_insert_char('X');
        // Cancel: the buffer edit must not be applied.
        assert!(panel.cancel_shell_field_edit());
        assert_eq!(panel.shell_args, "-l");
        assert!(panel.shell_field_editing.is_none());
    }

    #[test]
    fn begin_shell_field_edit_is_a_no_op_outside_fields_2_and_3() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.focused_widget_index = 0; // language field, not a shell field
        assert!(!panel.begin_shell_field_edit());
        assert!(panel.shell_field_editing.is_none());
    }

    #[test]
    fn save_writes_shell_program_and_args() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.shell_program = "/usr/bin/fish".to_string();
        panel.shell_args = "-l --login".to_string();
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("program = \"/usr/bin/fish\""));
        assert!(toml_str.contains("args = [\"-l\", \"--login\"]"));
    }

    #[test]
    fn save_omits_shell_program_when_empty() {
        // An empty program string must not overwrite a value already on disk
        // (mirrors the existing `font_family` guard).
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.shell_program = String::new();
        let existing = "[shell]\nprogram = \"/bin/bash\"\n";
        let toml_str = panel.apply_to_toml_string(existing);
        assert!(toml_str.contains("program = \"/bin/bash\""));
    }
}
