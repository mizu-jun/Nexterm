//! Copy-mode (tmux-compatible) key input handling
//!
//! Extracted from `input_handler.rs`:
//! - `handle_copy_mode_key` — key input in the normal mode
//! - `handle_copy_mode_search_key` — search input mode opened with `/`
//! - Word-boundary navigation (w / b)
//! - Incremental search (n / Enter to commit)
//! - Yank (y / Y) — copy to the clipboard

use winit::keyboard::KeyCode as WKeyCode;

use super::EventHandler;

impl EventHandler {
    /// Handle key input in copy mode (true = consumed)
    pub(super) fn handle_copy_mode_key(&mut self, code: WKeyCode) -> bool {
        // While in search input, delegate to the dedicated handler
        if self.app.state.copy_mode.search_query.is_some() {
            return self.handle_copy_mode_search_key(code);
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
            // 0: jump to start of line
            WKeyCode::Digit0 => {
                cm.cursor_col = 0;
            }
            // $: jump to end of line
            WKeyCode::Digit4 => {
                // Treat Shift+4 as '$' (WKeyCode has no Dollar variant)
                cm.cursor_col = max_col;
            }
            // w: move to the start of the next word
            WKeyCode::KeyW => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_next_word_start(col, row, max_col, max_row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // b: move to the start of the previous word
            WKeyCode::KeyB => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_prev_word_start(col, row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // v: toggle selection start/end
            WKeyCode::KeyV => {
                cm.toggle_selection();
            }
            // y / Y: y = yank selection; Y = yank the entire line
            WKeyCode::KeyY => {
                if self.modifiers.shift_key() {
                    self.yank_current_line();
                } else {
                    self.yank_selection();
                }
            }
            // /: enter incremental search mode
            WKeyCode::Slash => {
                self.app.state.copy_mode.search_query = Some(String::new());
            }
            // n: jump to the next search match
            WKeyCode::KeyN => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_query
                    .clone()
                    .unwrap_or_default();
                if !q.is_empty() {
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    if let Some((nc, nr)) = self.search_forward(&q, col + 1, row, max_col, max_row)
                    {
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
                self.app.state.copy_mode.search_query = None;
            }
            // Enter: commit the search and jump to the first match
            WKeyCode::Enter => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_query
                    .clone()
                    .unwrap_or_default();
                self.app.state.copy_mode.search_query = None;
                if !q.is_empty() {
                    let max_col = self.app.state.cols.saturating_sub(1);
                    let max_row = self.app.state.rows.saturating_sub(1);
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    if let Some((nc, nr)) = self.search_forward(&q, col, row, max_col, max_row) {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                        // Save the last search query so the `n` key can reuse it
                        self.app.state.copy_mode.search_query = Some(q);
                    }
                }
            }
            // Backspace: pop the last char off the query
            WKeyCode::Backspace => {
                if let Some(ref mut q) = self.app.state.copy_mode.search_query {
                    q.pop();
                }
            }
            _ => return false,
        }
        true
    }

    /// Return the start position of the next word (None if not found)
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

        // If we're on a word char, skip to the end of the word
        if let Some(cells) = pane.grid.rows.get(r) {
            while c < cells.len() && !cells[c].ch.is_whitespace() {
                c += 1;
            }
        }
        // Start of the next word (skip whitespace)
        loop {
            if let Some(cells) = pane.grid.rows.get(r) {
                while c < cells.len() {
                    if !cells[c].ch.is_whitespace() {
                        return Some((c as u16, r as u16));
                    }
                    c += 1;
                }
            }
            // Next row
            if r >= max_row as usize {
                break;
            }
            r += 1;
            c = 0;
        }
        Some((max_col, max_row))
    }

    /// Return the start position of the previous word (None if not found)
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
        // Skip back to the start of the word
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

    /// Forward search: return the first (col, row) that matches the query
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

    /// Copy the selected text to the clipboard and exit copy mode
    fn yank_selection(&mut self) {
        let cm = &self.app.state.copy_mode;
        if let Some(((sc, sr), (ec, er))) = cm.normalized_selection() {
            // Extract the selected text from the grid
            let text = if let Some(pane) = self.app.state.focused_pane() {
                let mut lines = Vec::new();
                for row_idx in sr..=er {
                    if let Some(row) = pane.grid.rows.get(row_idx as usize) {
                        let col_start = if row_idx == sr { sc as usize } else { 0 };
                        let col_end = if row_idx == er {
                            (ec + 1) as usize
                        } else {
                            row.len()
                        };
                        let line: String = row[col_start.min(row.len())..col_end.min(row.len())]
                            .iter()
                            .map(|c| c.ch)
                            .collect();
                        lines.push(line);
                    }
                }
                lines.join("\n")
            } else {
                String::new()
            };

            if !text.is_empty()
                && let Ok(mut clipboard) = arboard::Clipboard::new()
            {
                let _ = clipboard.set_text(text);
            }
        }
        self.app.state.copy_mode.exit();
    }

    /// Copy the entire cursor row to the clipboard and exit copy mode (Y key)
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
