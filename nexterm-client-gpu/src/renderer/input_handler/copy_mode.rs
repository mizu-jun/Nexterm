//! Copy-mode (tmux-compatible) key input handling
//!
//! Key handlers:
//! - `handle_copy_mode_key` — navigation, selection, and search in Normal/Visual/VisualLine Vi modes
//! - `handle_copy_mode_search_key` — search-query input opened with `/` (forward) or `?` (backward)
//! - Word-boundary navigation (w / b / e)
//! - Incremental search (/ ? n N)
//! - Yank (y / Y) — copy to the clipboard

use winit::keyboard::KeyCode as WKeyCode;

use crate::state::ViMode;
use super::EventHandler;

impl EventHandler {
    /// Handle key input in copy mode (true = consumed)
    pub(super) fn handle_copy_mode_key(&mut self, code: WKeyCode) -> bool {
        // While in search input, delegate to the dedicated handler
        if self.app.state.copy_mode.is_in_search_input() {
            return self.handle_copy_mode_search_key(code);
        }

        // Resolve gg two-key sequence before entering the main match.
        // If gg_pending is true and the user pressed `g` again (without shift), jump to top.
        let was_gg_pending = self.app.state.copy_mode.gg_pending;
        if was_gg_pending {
            self.app.state.copy_mode.gg_pending = false;
            if code == WKeyCode::KeyG && !self.modifiers.shift_key() {
                self.app.state.copy_mode.cursor_row = 0;
                self.app.state.copy_mode.cursor_col = 0;
                return true;
            }
            // Any other key: gg_pending cleared; fall through to handle normally
        }

        let cm = &mut self.app.state.copy_mode;
        let max_col = self.app.state.cols.saturating_sub(1);
        let max_row = self.app.state.rows.saturating_sub(1);

        match code {
            // q / Escape: exit copy mode
            WKeyCode::KeyQ | WKeyCode::Escape => {
                cm.exit();
            }
            // h / Left: move left
            WKeyCode::KeyH | WKeyCode::ArrowLeft => {
                cm.cursor_col = cm.cursor_col.saturating_sub(1);
            }
            // l / Right: move right
            WKeyCode::KeyL | WKeyCode::ArrowRight => {
                if cm.cursor_col < max_col {
                    cm.cursor_col += 1;
                }
            }
            // j / Down: move down
            WKeyCode::KeyJ | WKeyCode::ArrowDown => {
                if cm.cursor_row < max_row {
                    cm.cursor_row += 1;
                }
            }
            // k / Up: move up
            WKeyCode::KeyK | WKeyCode::ArrowUp => {
                cm.cursor_row = cm.cursor_row.saturating_sub(1);
            }
            // 0: beginning of line
            WKeyCode::Digit0 => {
                cm.cursor_col = 0;
            }
            // $ (Shift+4): end of line
            WKeyCode::Digit4 => {
                cm.cursor_col = max_col;
            }
            // ^ (Shift+6): first non-blank character of the line
            WKeyCode::Digit6 => {
                let row_idx = cm.cursor_row as usize;
                // Last use of cm here; NLL releases the borrow before the focused_pane() call
                let first_nonblank = self
                    .app
                    .state
                    .focused_pane()
                    .and_then(|pane| pane.grid.rows.get(row_idx))
                    .and_then(|row| {
                        row.iter()
                            .position(|c| !c.ch.is_whitespace())
                            .map(|i| i as u16)
                    })
                    .unwrap_or(0);
                self.app.state.copy_mode.cursor_col = first_nonblank;
            }
            // G (Shift+g): jump to last row; g alone: arm gg_pending for gg sequence
            WKeyCode::KeyG => {
                if self.modifiers.shift_key() {
                    cm.cursor_row = max_row;
                    cm.cursor_col = 0;
                } else {
                    cm.gg_pending = true;
                }
            }
            // Ctrl+U: scroll up half a page
            WKeyCode::KeyU if self.modifiers.control_key() => {
                let half = (max_row / 2).max(1);
                cm.cursor_row = cm.cursor_row.saturating_sub(half);
            }
            // Ctrl+D: scroll down half a page
            WKeyCode::KeyD if self.modifiers.control_key() => {
                let half = (max_row / 2).max(1);
                cm.cursor_row = (cm.cursor_row + half).min(max_row);
            }
            // w: start of the next word
            WKeyCode::KeyW => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_next_word_start(col, row, max_col, max_row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // b: start of the previous word
            WKeyCode::KeyB => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_prev_word_start(col, row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // e: end of the current word
            WKeyCode::KeyE => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_word_end(col, row, max_col, max_row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // v: character-wise visual selection; V (Shift+v): line-wise visual selection
            WKeyCode::KeyV => {
                if self.modifiers.shift_key() {
                    cm.toggle_visual_line();
                } else {
                    cm.toggle_selection();
                }
            }
            // y: yank selection; Y (Shift+y): yank entire current line
            WKeyCode::KeyY => {
                if self.modifiers.shift_key() {
                    self.yank_current_line();
                } else {
                    self.yank_selection();
                }
            }
            // ? (Shift+/): enter backward incremental search
            WKeyCode::Slash if self.modifiers.shift_key() => {
                self.app.state.copy_mode.search_input = Some(String::new());
                self.app.state.copy_mode.search_backward = true;
            }
            // /: enter forward incremental search
            WKeyCode::Slash => {
                self.app.state.copy_mode.search_input = Some(String::new());
                self.app.state.copy_mode.search_backward = false;
            }
            // n: repeat last search in its original direction
            // N (Shift+n): repeat last search in the reverse direction
            WKeyCode::KeyN => {
                let shift = self.modifiers.shift_key();
                let q = self.app.state.copy_mode.last_search_query.clone();
                let backward = if shift {
                    !self.app.state.copy_mode.search_backward
                } else {
                    self.app.state.copy_mode.search_backward
                };
                if !q.is_empty() {
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    let result = if backward {
                        self.search_prev(&q, col.saturating_sub(1), row, max_col, max_row)
                    } else {
                        self.search_forward(&q, col + 1, row, max_col, max_row)
                    };
                    if let Some((nc, nr)) = result {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// Handle keys while typing the search query (true = consumed)
    fn handle_copy_mode_search_key(&mut self, code: WKeyCode) -> bool {
        match code {
            // Escape: cancel the search and return to normal copy mode
            WKeyCode::Escape => {
                self.app.state.copy_mode.search_input = None;
            }
            // Enter: commit the search and jump to the first/last match
            WKeyCode::Enter => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_input
                    .clone()
                    .unwrap_or_default();
                let backward = self.app.state.copy_mode.search_backward;
                // Always clear search_input so nav keys route correctly after commit
                self.app.state.copy_mode.search_input = None;
                if !q.is_empty() {
                    let max_col = self.app.state.cols.saturating_sub(1);
                    let max_row = self.app.state.rows.saturating_sub(1);
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    let result = if backward {
                        self.search_prev(&q, col, row, max_col, max_row)
                    } else {
                        self.search_forward(&q, col, row, max_col, max_row)
                    };
                    if let Some((nc, nr)) = result {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                        // Store so n/N can repeat the search
                        self.app.state.copy_mode.last_search_query = q;
                    }
                }
            }
            // Backspace: remove the last character from the query
            WKeyCode::Backspace => {
                if let Some(ref mut q) = self.app.state.copy_mode.search_input {
                    q.pop();
                }
            }
            _ => return false,
        }
        true
    }

    /// Return the start position of the next word (None if already at the end)
    fn find_next_word_start(
        &self,
        col: u16,
        row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as usize;
        let mut r = row as usize;

        // Skip remaining non-whitespace on the current cell
        if let Some(cells) = pane.grid.rows.get(r) {
            while c < cells.len() && !cells[c].ch.is_whitespace() {
                c += 1;
            }
        }
        // Advance to the first non-whitespace of the next word
        loop {
            if let Some(cells) = pane.grid.rows.get(r) {
                while c < cells.len() {
                    if !cells[c].ch.is_whitespace() {
                        return Some((c as u16, r as u16));
                    }
                    c += 1;
                }
            }
            if r >= max_row as usize {
                break;
            }
            r += 1;
            c = 0;
        }
        Some((max_col, max_row))
    }

    /// Return the start position of the previous word (None if already at the top-left)
    fn find_prev_word_start(&self, col: u16, row: u16) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as isize - 1;
        let mut r = row as isize;

        // Skip whitespace immediately before the current position
        loop {
            if c < 0 {
                if r <= 0 {
                    return Some((0, 0));
                }
                r -= 1;
                c = pane
                    .grid
                    .rows
                    .get(r as usize)
                    .map(|row| row.len() as isize - 1)
                    .unwrap_or(0);
            }
            if let Some(cells) = pane.grid.rows.get(r as usize)
                && c < cells.len() as isize
                && !cells[c as usize].ch.is_whitespace()
            {
                break;
            }
            c -= 1;
        }
        // Walk back to the start of the word
        loop {
            if c <= 0 {
                return Some((0, r as u16));
            }
            if let Some(cells) = pane.grid.rows.get(r as usize) {
                if c - 1 < cells.len() as isize && cells[(c - 1) as usize].ch.is_whitespace() {
                    break;
                }
            } else {
                break;
            }
            c -= 1;
        }
        Some((c as u16, r as u16))
    }

    /// Return the end position of the current (or next) word
    fn find_word_end(
        &self,
        col: u16,
        row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as usize + 1;
        let mut r = row as usize;

        loop {
            if let Some(cells) = pane.grid.rows.get(r) {
                // Skip leading whitespace (handles the case where we stepped into a gap)
                while c < cells.len() && cells[c].ch.is_whitespace() {
                    c += 1;
                }
                // Walk to the last non-whitespace before a boundary
                while c < cells.len() {
                    let at_end =
                        c + 1 >= cells.len() || cells[c + 1].ch.is_whitespace();
                    if !cells[c].ch.is_whitespace() && at_end {
                        return Some((c as u16, r as u16));
                    }
                    c += 1;
                }
            }
            if r >= max_row as usize {
                break;
            }
            r += 1;
            c = 0;
        }
        Some((max_col, max_row))
    }

    /// Forward search: return the first (col, row) matching the query at or after (start_col, start_row)
    fn search_forward(
        &self,
        query: &str,
        start_col: u16,
        start_row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let rows_total = (max_row + 1) as usize;

        for dr in 0..rows_total {
            let r = ((start_row as usize) + dr) % rows_total;
            let cells = pane.grid.rows.get(r)?;
            let row_str: String = cells.iter().map(|c| c.ch).collect();
            let col_start = if dr == 0 { start_col as usize } else { 0 };
            let search_in = if col_start < row_str.len() {
                &row_str[col_start..]
            } else {
                continue;
            };
            if let Some(offset) = search_in.find(query) {
                let found_col = (col_start + offset).min(max_col as usize) as u16;
                return Some((found_col, r as u16));
            }
        }
        None
    }

    /// Backward search: return the last (col, row) matching the query at or before (start_col, start_row)
    fn search_prev(
        &self,
        query: &str,
        start_col: u16,
        start_row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let rows_total = (max_row + 1) as usize;

        for dr in 0..rows_total {
            let r = (start_row as usize + rows_total - dr) % rows_total;
            let cells = pane.grid.rows.get(r)?;
            let row_str: String = cells.iter().map(|c| c.ch).collect();
            let search_in = if dr == 0 {
                let end = (start_col as usize + query.len()).min(row_str.len());
                &row_str[..end]
            } else {
                &row_str[..]
            };
            if let Some(offset) = search_in.rfind(query) {
                return Some((offset.min(max_col as usize) as u16, r as u16));
            }
        }
        None
    }

    /// Yank the selected text to the clipboard and exit copy mode.
    /// Handles both character-wise (Visual) and line-wise (VisualLine) selections.
    fn yank_selection(&mut self) {
        let text = {
            let cm = &self.app.state.copy_mode;
            match cm.vi_mode {
                ViMode::VisualLine => {
                    if let Some((sr, er)) = cm.normalized_visual_line_range() {
                        self.app
                            .state
                            .focused_pane()
                            .map(|pane| {
                                (sr..=er)
                                    .filter_map(|row_idx| pane.grid.rows.get(row_idx as usize))
                                    .map(|row| row.iter().map(|c| c.ch).collect::<String>())
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    }
                }
                _ => {
                    if let Some(((sc, sr), (ec, er))) = cm.normalized_selection() {
                        self.app
                            .state
                            .focused_pane()
                            .map(|pane| {
                                let mut lines = Vec::new();
                                for row_idx in sr..=er {
                                    if let Some(row) = pane.grid.rows.get(row_idx as usize) {
                                        let col_start =
                                            if row_idx == sr { sc as usize } else { 0 };
                                        let col_end = if row_idx == er {
                                            (ec + 1) as usize
                                        } else {
                                            row.len()
                                        };
                                        let line: String = row
                                            [col_start.min(row.len())..col_end.min(row.len())]
                                            .iter()
                                            .map(|c| c.ch)
                                            .collect();
                                        lines.push(line);
                                    }
                                }
                                lines.join("\n")
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    }
                }
            }
        };

        if !text.is_empty()
            && let Ok(mut clipboard) = arboard::Clipboard::new()
        {
            let _ = clipboard.set_text(text);
        }
        self.app.state.copy_mode.exit();
    }

    /// Yank the entire cursor row to the clipboard and exit copy mode (Y key)
    fn yank_current_line(&mut self) {
        let row_idx = self.app.state.copy_mode.cursor_row as usize;
        let text = if let Some(pane) = self.app.state.focused_pane() {
            pane.grid
                .rows
                .get(row_idx)
                .map(|row| row.iter().map(|c| c.ch).collect::<String>())
                .unwrap_or_default()
        } else {
            String::new()
        };
        if !text.is_empty()
            && let Ok(mut clipboard) = arboard::Clipboard::new()
        {
            let _ = clipboard.set_text(text);
        }
        self.app.state.copy_mode.exit();
    }
}
