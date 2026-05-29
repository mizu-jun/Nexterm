//! Scrollback search — `SearchState` and the incremental search methods on `ClientState`
//!
//! Extracted from `state/mod.rs`:
//! - `SearchState` — state of the search query and the current match position
//! - `impl ClientState` — incremental search operations such as `start_search` /
//!   `push_search_char` / `pop_search_char` / `search_next` / `search_prev` / `end_search`

use super::ClientState;

/// Incremental search state
pub struct SearchState {
    pub query: String,
    pub is_active: bool,
    /// Currently highlighted row index (inside the scrollback)
    pub current_match: Option<usize>,
}

impl SearchState {
    pub(crate) fn new() -> Self {
        Self {
            query: String::new(),
            is_active: false,
            current_match: None,
        }
    }
}

impl ClientState {
    /// Start a scrollback search
    pub fn start_search(&mut self) {
        self.search.is_active = true;
        self.search.query.clear();
        self.search.current_match = None;
    }

    /// Append a char to the query and re-run the incremental search
    pub fn push_search_char(&mut self, ch: char) {
        self.search.query.push(ch);
        self.search_next_from(0);
    }

    /// Pop the last char off the search query
    pub fn pop_search_char(&mut self) {
        self.search.query.pop();
        self.search_next_from(0);
    }

    /// Move to the next match
    pub fn search_next(&mut self) {
        let from = self.search.current_match.map(|m| m + 1).unwrap_or(0);
        self.search_next_from(from);
    }

    /// Move to the previous match
    pub fn search_prev(&mut self) {
        let query = self.search.query.clone();
        let current = self.search.current_match.unwrap_or(0);
        let result = self
            .focused_pane_mut()
            .and_then(|pane| pane.scrollback.search_prev(&query, current));
        self.search.current_match = result;
        if let Some(row) = result
            && let Some(pane) = self.focused_pane_mut()
        {
            pane.scroll_offset = row;
        }
    }

    pub(super) fn search_next_from(&mut self, from: usize) {
        let query = self.search.query.clone();
        // Compute the result first so the borrow is released before we re-borrow
        let result = self
            .focused_pane_mut()
            .and_then(|pane| pane.scrollback.search_next(&query, from));
        self.search.current_match = result;
        if let Some(row) = result
            && let Some(pane) = self.focused_pane_mut()
        {
            pane.scroll_offset = row;
        }
    }

    /// End the search
    pub fn end_search(&mut self) {
        self.search.is_active = false;
        self.search.query.clear();
        self.search.current_match = None;
        if let Some(pane) = self.focused_pane_mut() {
            pane.scroll_offset = 0;
        }
    }
}
