//! In-flight text-edit state shared by the settings panel's text fields
//! (SSH host name/host/username, shell program/args, leader key, ...).
//!
//! Moved out of `settings_panel.rs` verbatim (Phase B6 mechanical split).

/// Phase 5-11-8 Step 8-3 (Sub-phase A): inline text-input state.
///
/// Holds the in-flight edit state for `TextInput` fields inside the settings
/// panel. Used to edit the SSH host name / host / username fields.
/// IME preedit text (Sub-phase B) is stored in the `preedit` field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInputState {
    /// Edit buffer.
    pub buffer: String,
    /// Cursor position (byte index inside `buffer`).
    /// Invariant: `buffer.is_char_boundary(cursor) == true`.
    pub cursor: usize,
    /// IME preedit text (used in Sub-phase B). `None` means no preedit in flight.
    pub preedit: Option<String>,
}

impl TextInputState {
    /// Build a `TextInputState` from an initial string; cursor goes to the end.
    pub fn new(initial: String) -> Self {
        let cursor = initial.len();
        Self {
            buffer: initial,
            cursor,
            preedit: None,
        }
    }

    /// Insert a single character at the cursor and advance past it.
    pub fn insert_char(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// Insert a string at the cursor and advance past it.
    /// Also used to commit multiple characters at once via the IME `Commit` path.
    pub fn insert_str(&mut self, s: &str) {
        self.buffer.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Delete the character immediately before the cursor (Backspace).
    /// Honours multibyte boundaries by doing a manual `floor_char_boundary`-style scan.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Find the character boundary immediately before the cursor.
        let mut prev = self.cursor - 1;
        while prev > 0 && !self.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        self.buffer.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    /// Delete the character immediately after the cursor (Delete).
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut next = self.cursor + 1;
        while next < self.buffer.len() && !self.buffer.is_char_boundary(next) {
            next += 1;
        }
        self.buffer.replace_range(self.cursor..next, "");
    }

    /// Move the cursor one character left.
    pub fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut prev = self.cursor - 1;
        while prev > 0 && !self.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        self.cursor = prev;
    }

    /// Move the cursor one character right.
    pub fn move_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut next = self.cursor + 1;
        while next < self.buffer.len() && !self.buffer.is_char_boundary(next) {
            next += 1;
        }
        self.cursor = next;
    }

    /// Move the cursor to the start of the buffer.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end of the buffer.
    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    /// Return the display string. With `preedit == None`, returns the buffer
    /// as-is; with `Some(pe)`, returns the string with the preedit inserted at
    /// the cursor.
    pub fn display_string(&self) -> String {
        match &self.preedit {
            None => self.buffer.clone(),
            Some(pe) => {
                let mut s = self.buffer.clone();
                s.insert_str(self.cursor, pe);
                s
            }
        }
    }

    /// Return the cursor position (in bytes) inside the display string.
    /// When a preedit is present, points to the end of the preedit (matches
    /// the visual cursor before IME commit).
    pub fn display_cursor(&self) -> usize {
        match &self.preedit {
            None => self.cursor,
            Some(pe) => self.cursor + pe.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_state_new_cursor_at_end() {
        let s = TextInputState::new("hello".to_string());
        assert_eq!(s.buffer, "hello");
        assert_eq!(s.cursor, 5);
        assert!(s.preedit.is_none());

        let empty = TextInputState::new(String::new());
        assert_eq!(empty.cursor, 0);
    }

    #[test]
    fn text_input_state_insert_char_advances_cursor_ascii() {
        let mut s = TextInputState::new(String::new());
        s.insert_char('a');
        s.insert_char('b');
        s.insert_char('c');
        assert_eq!(s.buffer, "abc");
        assert_eq!(s.cursor, 3);
    }

    #[test]
    fn text_input_state_insert_char_advances_cursor_cjk() {
        // One Japanese character = 3 bytes in UTF-8; the cursor must advance
        // in bytes too.
        let mut s = TextInputState::new(String::new());
        s.insert_char('あ');
        assert_eq!(s.buffer, "あ");
        assert_eq!(s.cursor, 3);
        s.insert_char('い');
        assert_eq!(s.buffer, "あい");
        assert_eq!(s.cursor, 6);
    }

    #[test]
    fn text_input_state_backspace_respects_utf8_boundary() {
        // Backspace on "あい" yields "あ" with the cursor at byte 3 (boundary).
        let mut s = TextInputState::new("あい".to_string());
        assert_eq!(s.cursor, 6);
        s.backspace();
        assert_eq!(s.buffer, "あ");
        assert_eq!(s.cursor, 3);
        s.backspace();
        assert_eq!(s.buffer, "");
        assert_eq!(s.cursor, 0);
        // Backspace on an empty buffer is a no-op.
        s.backspace();
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn text_input_state_move_left_right_clamps_and_respects_boundary() {
        let mut s = TextInputState::new("aあb".to_string());
        // Tail (5 = 1 + 3 + 1).
        assert_eq!(s.cursor, 5);
        s.move_left();
        assert_eq!(s.cursor, 4, "right before 'b'");
        s.move_left();
        assert_eq!(
            s.cursor, 1,
            "right before 'あ' (honours the UTF-8 boundary)"
        );
        s.move_left();
        assert_eq!(s.cursor, 0);
        // Moving further left at the head is a no-op.
        s.move_left();
        assert_eq!(s.cursor, 0);

        s.move_right();
        assert_eq!(s.cursor, 1);
        s.move_right();
        assert_eq!(s.cursor, 4, "steps past 'あ'");
        s.move_right();
        assert_eq!(s.cursor, 5);
        // Moving further right at the tail is a no-op.
        s.move_right();
        assert_eq!(s.cursor, 5);
    }

    #[test]
    fn text_input_state_display_string_with_preedit() {
        let mut s = TextInputState::new("ab".to_string());
        s.move_left(); // cursor to 1
        assert_eq!(s.cursor, 1);
        s.preedit = Some("X".to_string());

        // The display string inserts the preedit at the cursor.
        assert_eq!(s.display_string(), "aXb");
        assert_eq!(s.display_cursor(), 2, "points to the end of the preedit");

        // Clearing the preedit restores the original.
        s.preedit = None;
        assert_eq!(s.display_string(), "ab");
        assert_eq!(s.display_cursor(), 1);
    }
}
