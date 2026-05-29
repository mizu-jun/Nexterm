//! Text selection — URL detection, mouse drag selection, copy mode
//!
//! Extracted from `state/mod.rs`:
//! - `DetectedUrl` + `detect_urls_in_row` — detect URLs in a grid row
//! - `MouseSelection` — text selection state driven by mouse drag
//! - `CopyModeState` — tmux-compatible Vim-style copy mode

/// URL on the grid with its range (used for underline rendering and click hit-testing)
#[derive(Debug, Clone)]
pub struct DetectedUrl {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub url: String,
}

impl DetectedUrl {
    /// Returns whether the given grid cell falls inside this URL's range
    pub fn contains(&self, col: u16, row: u16) -> bool {
        row == self.row && col >= self.col_start && col < self.col_end
    }
}

/// Detect URLs in the row text of a grid
pub fn detect_urls_in_row(row_idx: u16, cells: &[nexterm_proto::Cell]) -> Vec<DetectedUrl> {
    let text: String = cells.iter().map(|c| c.ch).collect();
    let mut urls = Vec::new();

    // Detect URLs starting with https:// or http://
    let prefixes = ["https://", "http://"];
    for prefix in prefixes {
        let mut search_from = 0;
        while let Some(start) = text[search_from..].find(prefix) {
            let abs_start = search_from + start;
            // The URL terminates at whitespace, control chars, or brackets
            let end = text[abs_start..]
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')'))
                .map(|i| abs_start + i)
                .unwrap_or(text.len());
            if end > abs_start {
                urls.push(DetectedUrl {
                    row: row_idx,
                    col_start: abs_start as u16,
                    col_end: end as u16,
                    url: text[abs_start..end].to_string(),
                });
            }
            search_from = abs_start + 1;
        }
    }
    urls
}

/// Mouse drag text selection state
pub struct MouseSelection {
    /// Whether a drag is in progress
    pub is_dragging: bool,
    /// Selection start cell (grid coordinates)
    pub start: (u16, u16),
    /// Selection end cell (grid coordinates, updated continuously while dragging)
    pub end: (u16, u16),
}

impl MouseSelection {
    pub fn new() -> Self {
        Self {
            is_dragging: false,
            start: (0, 0),
            end: (0, 0),
        }
    }

    /// Begin a drag
    pub fn begin(&mut self, col: u16, row: u16) {
        self.is_dragging = true;
        self.start = (col, row);
        self.end = (col, row);
    }

    /// Update the end point while dragging
    pub fn update(&mut self, col: u16, row: u16) {
        if self.is_dragging {
            self.end = (col, row);
        }
    }

    /// Finish the drag
    pub fn finish(&mut self) {
        self.is_dragging = false;
    }

    /// Returns the normalized selection range (guarantees start <= end).
    /// Returns None if nothing is selected (start == end).
    pub fn normalized(&self) -> Option<((u16, u16), (u16, u16))> {
        let (sc, sr) = self.start;
        let (ec, er) = self.end;
        if (sr, sc) == (er, ec) {
            return None;
        }
        if (sr, sc) <= (er, ec) {
            Some(((sc, sr), (ec, er)))
        } else {
            Some(((ec, er), (sc, sr)))
        }
    }

    /// Returns whether the given cell is inside the selection range
    pub fn contains(&self, col: u16, row: u16) -> bool {
        if let Some(((sc, sr), (ec, er))) = self.normalized() {
            if row < sr || row > er {
                return false;
            }
            if row == sr && row == er {
                return col >= sc && col <= ec;
            }
            if row == sr {
                return col >= sc;
            }
            if row == er {
                return col <= ec;
            }
            true
        } else {
            false
        }
    }
}

/// Copy mode (Vim-style text selection) state
pub struct CopyModeState {
    /// Whether copy mode is active
    pub is_active: bool,
    /// Cursor column (grid coordinates, 0-based)
    pub cursor_col: u16,
    /// Cursor row (grid coordinates, 0-based)
    pub cursor_row: u16,
    /// Selection start position (cursor position when `v` was pressed)
    pub selection_start: Option<(u16, u16)>,
    /// Incremental search query (active while `Some`)
    pub search_query: Option<String>,
}

impl CopyModeState {
    pub(crate) fn new() -> Self {
        Self {
            is_active: false,
            cursor_col: 0,
            cursor_row: 0,
            selection_start: None,
            search_query: None,
        }
    }

    /// Enter copy mode and align the cursor to the current pane cursor
    pub fn enter(&mut self, pane_cursor_col: u16, pane_cursor_row: u16) {
        self.is_active = true;
        self.cursor_col = pane_cursor_col;
        self.cursor_row = pane_cursor_row;
        self.selection_start = None;
    }

    /// Exit copy mode
    pub fn exit(&mut self) {
        self.is_active = false;
        self.selection_start = None;
        self.search_query = None;
    }

    /// Toggle the start/end of the selection (the `v` key)
    pub fn toggle_selection(&mut self) {
        if self.selection_start.is_some() {
            self.selection_start = None;
        } else {
            self.selection_start = Some((self.cursor_col, self.cursor_row));
        }
    }

    /// Returns the normalized selection range (guarantees start <= end)
    pub fn normalized_selection(&self) -> Option<((u16, u16), (u16, u16))> {
        let (sc, sr) = self.selection_start?;
        let (ec, er) = (self.cursor_col, self.cursor_row);
        if (sr, sc) <= (er, ec) {
            Some(((sc, sr), (ec, er)))
        } else {
            Some(((ec, er), (sc, sr)))
        }
    }
}
