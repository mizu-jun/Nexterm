//! Lua macro picker UI — Ctrl+Shift+M opens a floating list.
//!
//! Lists every `[[macros]]` entry from the config file; pressing Enter executes
//! the selected macro (sends `RunMacro` to the server).

use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use nexterm_config::MacroConfig;

/// Display / interaction state of the macro picker.
pub struct MacroPicker {
    /// Registered macros (loaded from the config file).
    macros: Vec<MacroConfig>,
    /// Current search query.
    pub query: String,
    /// Whether the panel is open.
    pub is_open: bool,
    /// Selected index (relative to the filtered list).
    pub selected: usize,
    /// Fuzzy matcher.
    matcher: SkimMatcherV2,
}

impl MacroPicker {
    /// Build a picker from a list of macro configs.
    pub fn new(macros: Vec<MacroConfig>) -> Self {
        Self {
            macros,
            query: String::new(),
            is_open: false,
            selected: 0,
            matcher: SkimMatcherV2::default(),
        }
    }

    /// Open the panel and reset the query / selection.
    pub fn open(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.is_open = true;
    }

    /// Close the panel.
    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
    }

    /// Append a character to the search query.
    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
        self.selected = 0;
    }

    /// Pop the last character from the search query.
    pub fn pop_char(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// Move the selection down (wraps around).
    pub fn select_next(&mut self) {
        let count = self.filtered().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    /// Move the selection up (wraps around).
    pub fn select_prev(&mut self) {
        let count = self.filtered().len();
        if count > 0 {
            self.selected = if self.selected == 0 {
                count - 1
            } else {
                self.selected - 1
            };
        }
    }

    /// Return the currently selected macro config.
    pub fn selected_macro(&self) -> Option<&MacroConfig> {
        self.filtered().into_iter().nth(self.selected)
    }

    /// Return macros matching the query, sorted by descending fuzzy score.
    pub fn filtered(&self) -> Vec<&MacroConfig> {
        if self.query.is_empty() {
            return self.macros.iter().collect();
        }

        let mut scored: Vec<(i64, &MacroConfig)> = self
            .macros
            .iter()
            .filter_map(|m| {
                let haystack = format!("{} {}", m.name, m.description);
                self.matcher
                    .fuzzy_match(&haystack, &self.query)
                    .map(|score| (score, m))
            })
            .collect();

        scored.sort_by_key(|b| std::cmp::Reverse(b.0));
        scored.into_iter().map(|(_, m)| m).collect()
    }

    /// Replace the macro list (used when the config file is reloaded).
    pub fn reload(&mut self, macros: Vec<MacroConfig>) {
        self.macros = macros;
        self.selected = 0;
    }
}
