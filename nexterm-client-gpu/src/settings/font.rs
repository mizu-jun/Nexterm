//! Font category: family/size/ligatures/fallback-list editing.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::{SettingsPanel, TextInputState};

impl SettingsPanel {
    /// Set the font size from a slider X coordinate (used by mouse clicks/drags).
    pub fn set_font_size_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        // Font size range: 8.0..=32.0 (a 24-wide range, snapped to 0.5 steps).
        let raw = 8.0 + ratio * 24.0;
        self.font_size = (raw * 2.0).round() / 2.0;
        self.dirty = true;
    }

    /// Append a character to the font-family input field.
    pub fn push_font_family_char(&mut self, ch: char) {
        if self.font_family_editing {
            self.font_family.push(ch);
            self.dirty = true;
        }
    }

    /// Pop the trailing character from the font-family input field.
    pub fn pop_font_family_char(&mut self) {
        if self.font_family_editing {
            self.font_family.pop();
            self.dirty = true;
        }
    }

    pub fn increase_font_size(&mut self) {
        self.font_size = (self.font_size + 0.5).min(32.0);
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn decrease_font_size(&mut self) {
        self.font_size = (self.font_size - 0.5).max(8.0);
        self.dirty = true;
    }

    /// Total number of fields in the Font category.
    pub const FONT_FIELD_COUNT: u16 = 4;

    /// Move focus to the next field (stops at the last one).
    pub fn next_font_field(&mut self) -> bool {
        if self.font_field_focus + 1 < Self::FONT_FIELD_COUNT {
            self.font_field_focus += 1;
            true
        } else {
            false
        }
    }

    /// Move focus to the previous field (stops at the first one).
    pub fn prev_font_field(&mut self) -> bool {
        if self.font_field_focus > 0 {
            self.font_field_focus -= 1;
            true
        } else {
            false
        }
    }

    /// Increment the focused field's value (Right arrow).
    pub fn font_field_increase(&mut self) {
        match self.font_field_focus {
            1 => self.increase_font_size(),
            2 => self.toggle_font_ligatures(),
            _ => {}
        }
    }

    /// Decrement the focused field's value (Left arrow).
    pub fn font_field_decrease(&mut self) {
        match self.font_field_focus {
            1 => self.decrease_font_size(),
            2 => self.toggle_font_ligatures(),
            _ => {}
        }
    }

    /// Toggle `[font].ligatures`.
    pub fn toggle_font_ligatures(&mut self) {
        self.font_ligatures = !self.font_ligatures;
        self.dirty = true;
    }

    /// Start editing `font_fallbacks_text` (focus must be on field 3).
    /// Returns `true` when edit mode actually started.
    pub fn begin_font_fallbacks_edit(&mut self) -> bool {
        if self.font_field_focus != 3 {
            return false;
        }
        self.font_fallbacks_editing = Some(TextInputState::new(self.font_fallbacks_text.clone()));
        true
    }

    /// Commit the in-flight `font_fallbacks_text` buffer and leave edit mode.
    /// Returns `true` when a write-back happened.
    pub fn commit_font_fallbacks_edit(&mut self) -> bool {
        let Some(state) = self.font_fallbacks_editing.take() else {
            return false;
        };
        self.font_fallbacks_text = state.buffer;
        self.dirty = true;
        true
    }

    /// Discard the in-flight `font_fallbacks_text` edit.
    /// Returns `true` if edit mode was active.
    pub fn cancel_font_fallbacks_edit(&mut self) -> bool {
        self.font_fallbacks_editing.take().is_some()
    }

    /// Insert one character at the cursor inside the in-flight buffer.
    pub fn font_fallbacks_insert_char(&mut self, ch: char) {
        if let Some(state) = self.font_fallbacks_editing.as_mut() {
            state.insert_char(ch);
        }
    }

    /// Delete the character immediately before the cursor (Backspace).
    pub fn font_fallbacks_backspace(&mut self) {
        if let Some(state) = self.font_fallbacks_editing.as_mut() {
            state.backspace();
        }
    }

    /// Delete the character immediately after the cursor (Delete).
    pub fn font_fallbacks_delete(&mut self) {
        if let Some(state) = self.font_fallbacks_editing.as_mut() {
            state.delete_forward();
        }
    }

    /// Move the in-flight cursor one character left.
    pub fn font_fallbacks_move_left(&mut self) {
        if let Some(state) = self.font_fallbacks_editing.as_mut() {
            state.move_left();
        }
    }

    /// Move the in-flight cursor one character right.
    pub fn font_fallbacks_move_right(&mut self) {
        if let Some(state) = self.font_fallbacks_editing.as_mut() {
            state.move_right();
        }
    }

    /// Move the in-flight cursor to the start of the buffer.
    pub fn font_fallbacks_move_home(&mut self) {
        if let Some(state) = self.font_fallbacks_editing.as_mut() {
            state.move_home();
        }
    }

    /// Move the in-flight cursor to the end of the buffer.
    pub fn font_fallbacks_move_end(&mut self) {
        if let Some(state) = self.font_fallbacks_editing.as_mut() {
            state.move_end();
        }
    }

    /// Parse `font_fallbacks_text` into a TOML-ready list: split on `,`, trim
    /// each entry, and drop empty entries. An all-empty/whitespace string
    /// yields an empty list.
    pub fn font_fallbacks_list(&self) -> Vec<String> {
        self.font_fallbacks_text
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Used by SR via `Action::SetValue(NumericValue)`: clamp the f64 value to
    /// `8.0..=32.0`, snap to 0.5 steps, and store it as the font size.
    ///
    /// The mouse-drag path (`set_font_size_from_slider`) takes a pixel X
    /// coordinate instead of a direct value, but the rounding and clamp ranges
    /// are identical.
    pub fn set_font_size_value(&mut self, v: f64) {
        let raw = (v as f32).clamp(8.0, 32.0);
        self.font_size = (raw * 2.0).round() / 2.0;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn font_size_clamped() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.font_size = 32.0;
        panel.increase_font_size();
        assert_eq!(
            panel.font_size, 32.0,
            "must not exceed the 32.0 upper bound"
        );

        panel.font_size = 8.0;
        panel.decrease_font_size();
        assert_eq!(
            panel.font_size, 8.0,
            "must not fall below the 8.0 lower bound"
        );

        panel.font_size = 14.0;
        panel.increase_font_size();
        assert!((panel.font_size - 14.5).abs() < f32::EPSILON);
        assert!(panel.dirty);
    }

    #[test]
    fn save_writes_font_ligatures() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.toggle_font_ligatures();
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains(&format!("ligatures = {}", panel.font_ligatures)));
    }

    #[test]
    fn save_writes_font_fallbacks_split_and_trimmed() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.font_fallbacks_text = " Fira Code ,JetBrains Mono ,, Noto Color Emoji".to_string();
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("font_fallbacks = ["));
        assert!(toml_str.contains("\"Fira Code\""));
        assert!(toml_str.contains("\"JetBrains Mono\""));
        assert!(toml_str.contains("\"Noto Color Emoji\""));
    }

    #[test]
    fn save_writes_empty_font_fallbacks_as_empty_array() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.font_fallbacks_text = "   ,  ,".to_string();
        assert!(panel.font_fallbacks_list().is_empty());
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("font_fallbacks = []"));
    }

    #[test]
    fn font_fallbacks_edit_round_trip() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.font_field_focus = 3;
        assert!(panel.begin_font_fallbacks_edit());
        panel.font_fallbacks_insert_char('x');
        assert!(panel.commit_font_fallbacks_edit());
        assert!(panel.font_fallbacks_text.ends_with('x'));
        assert!(panel.font_fallbacks_editing.is_none());
    }

    #[test]
    fn font_field_navigation_covers_all_four_fields() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        for expected in 1..SettingsPanel::FONT_FIELD_COUNT {
            assert!(panel.next_font_field());
            assert_eq!(panel.font_field_focus, expected);
        }
        assert!(!panel.next_font_field());
        for expected in (0..SettingsPanel::FONT_FIELD_COUNT - 1).rev() {
            assert!(panel.prev_font_field());
            assert_eq!(panel.font_field_focus, expected);
        }
        assert!(!panel.prev_font_field());
    }
}
