//! Command palette — Ctrl+Shift+P shows a floating UI.
//!
//! Sprint 5-7 / Phase 3-3: adds history persistence (`palette_history.json`)
//! and "most-recently-used actions float to the top" ranking.
//!
//! - Empty query: sort by history (`last_used` desc → `use_count` desc → original registration order).
//! - Non-empty query: fuzzy score + history bonus (up to +200), sorted descending.
//! - History is persisted at `~/.local/state/nexterm/palette_history.json` on Unix
//!   (`%APPDATA%\nexterm\palette_history.json` on Windows) with atomic-write + mode 0600.

use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use nexterm_i18n::fl;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

/// A single action that can be registered in the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteAction {
    /// Display label (already translated for the current locale).
    pub label: String,
    /// Action identifier (used for dispatch).
    pub action: String,
}

/// A single history entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteHistoryEntry {
    /// Most recent use time (UNIX seconds).
    pub last_used: u64,
    /// Cumulative use count.
    pub use_count: u32,
}

/// Action history (`action identifier` → entry).
pub type PaletteHistory = HashMap<String, PaletteHistoryEntry>;

/// Command-palette state.
pub struct CommandPalette {
    /// Registered actions.
    actions: Vec<PaletteAction>,
    /// Current search query.
    pub query: String,
    /// Whether the palette is open.
    pub is_open: bool,
    /// Selected index (within the filtered list).
    pub selected: usize,
    /// Fuzzy matcher.
    matcher: SkimMatcherV2,
    /// Action history (persisted to disk).
    history: PaletteHistory,
    /// Phase 2c-F: named-block search mode. When the query starts with `@`,
    /// the palette switches to ranking these synthetic actions (built by the
    /// host from `NamedBlockStore` + the focused pane's block list) instead
    /// of the static action list. Action ids are `"BlockSelect:<u64>"`.
    named_block_actions: Vec<PaletteAction>,
}

impl CommandPalette {
    /// Build a palette with the default actions (translated for the current locale).
    pub fn new() -> Self {
        let actions = default_actions();
        Self {
            actions,
            query: String::new(),
            is_open: false,
            selected: 0,
            matcher: SkimMatcherV2::default(),
            history: PaletteHistory::new(),
            named_block_actions: Vec::new(),
        }
    }

    /// Build a palette and merge the persisted history.
    ///
    /// Starts with an empty history if the file is missing or unparseable
    /// (the constructor never crashes).
    pub fn new_with_history() -> Self {
        let mut palette = Self::new();
        palette.history = load_history();
        palette
    }

    /// Open the palette.
    pub fn open(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.is_open = true;
    }

    /// Close the palette.
    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
    }

    /// Append a character to the query.
    #[allow(dead_code)]
    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
        self.selected = 0;
    }

    /// Pop the last character from the query.
    #[allow(dead_code)]
    pub fn pop_char(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// Move the selection down (wraps).
    pub fn select_next(&mut self) {
        let count = self.filtered().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    /// Move the selection up (wraps).
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

    /// Return the currently selected action.
    pub fn selected_action(&self) -> Option<&PaletteAction> {
        self.filtered().into_iter().nth(self.selected)
    }

    /// Return actions matching the query, sorted by descending score.
    ///
    /// - **`@`-prefix mode** (Phase 2c-F): when the query starts with `@`, the
    ///   palette ranks the named-block list against the rest of the query
    ///   (`@deploy` → fuzzy-match "deploy" against block names). History
    ///   ranking does not apply — blocks are session-scoped, not
    ///   action-scoped, and re-using the same `last_used_unix` heuristic
    ///   that lives in `NamedBlockStore` itself happens at refresh time.
    /// - **Empty query**: history order (`last_used` desc → `use_count` desc)
    ///   → original registration order.
    /// - **Non-empty query**: fuzzy score + history bonus, sorted descending.
    pub fn filtered(&self) -> Vec<&PaletteAction> {
        if let Some(sub) = self.query.strip_prefix('@') {
            // No history bonus inside @-mode: pass an empty map so ranking is
            // pure fuzzy match. An empty `sub` returns the full named-block
            // list in registration order (mirrors the empty-query branch
            // above, but for blocks).
            let empty_history = PaletteHistory::new();
            rank_actions(
                &self.named_block_actions,
                sub,
                &empty_history,
                &self.matcher,
            )
        } else {
            rank_actions(&self.actions, &self.query, &self.history, &self.matcher)
        }
    }

    /// Phase 2c-F: replace the synthetic named-block actions used by the
    /// `@`-prefix search. Callers (the input handler / event handler) refresh
    /// this list whenever the palette opens, so closed-then-reopened palette
    /// instances reflect newly-named blocks.
    pub fn set_named_block_actions(&mut self, actions: Vec<PaletteAction>) {
        self.named_block_actions = actions;
    }

    /// Record a "used" event and persist the history (Sprint 5-7 / Phase 3-3).
    pub fn record_use(&mut self, action: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = self
            .history
            .entry(action.to_string())
            .or_insert(PaletteHistoryEntry {
                last_used: now,
                use_count: 0,
            });
        entry.last_used = now;
        entry.use_count = entry.use_count.saturating_add(1);
        save_history(&self.history);
    }

    /// Register a custom action.
    #[allow(dead_code)]
    pub fn register(&mut self, action: PaletteAction) {
        self.actions.push(action);
    }
}

/// Phase 2c-F: build the synthetic palette actions used by the `@`-prefix
/// search. Each input pair `(BlockId, name)` becomes a `PaletteAction` whose
/// `label` is the human name and whose `action` field encodes the block ID as
/// `"BlockSelect:<u64>"` (the execute_action dispatcher parses that prefix).
///
/// Pure — no I/O. Callers should sort the input deterministically if they
/// care about the order when the query is empty (the palette itself does not
/// re-sort named-block actions).
pub fn build_named_block_actions<I>(blocks: I) -> Vec<PaletteAction>
where
    I: IntoIterator<Item = (u64, String)>,
{
    blocks
        .into_iter()
        .map(|(id, name)| PaletteAction {
            label: name,
            action: format!("BlockSelect:{}", id),
        })
        .collect()
}

/// Return the default action list (already translated via i18n).
///
/// Sprint 5-7 / Phase 3-3: covers every action wired into `execute_action`
/// (added ClosePane / NewWindow / QuickSelect / SetBroadcastOn/Off / Quit).
fn default_actions() -> Vec<PaletteAction> {
    vec![
        PaletteAction {
            label: fl!("palette-split-vertical"),
            action: "SplitVertical".to_string(),
        },
        PaletteAction {
            label: fl!("palette-split-horizontal"),
            action: "SplitHorizontal".to_string(),
        },
        PaletteAction {
            label: fl!("palette-focus-next"),
            action: "FocusNextPane".to_string(),
        },
        PaletteAction {
            label: fl!("palette-focus-prev"),
            action: "FocusPrevPane".to_string(),
        },
        // Sprint 5-7 / Phase 3-3: added entries (already wired into execute_action
        // but missing from the palette).
        PaletteAction {
            label: fl!("palette-close-pane"),
            action: "ClosePane".to_string(),
        },
        PaletteAction {
            label: fl!("palette-new-window"),
            action: "NewWindow".to_string(),
        },
        PaletteAction {
            label: fl!("palette-detach"),
            action: "Detach".to_string(),
        },
        PaletteAction {
            label: fl!("palette-search-scrollback"),
            action: "SearchScrollback".to_string(),
        },
        PaletteAction {
            label: fl!("palette-display-panes"),
            action: "DisplayPanes".to_string(),
        },
        PaletteAction {
            label: fl!("palette-toggle-zoom"),
            action: "ToggleZoom".to_string(),
        },
        PaletteAction {
            label: fl!("palette-quick-select"),
            action: "QuickSelect".to_string(),
        },
        PaletteAction {
            label: fl!("palette-swap-pane-next"),
            action: "SwapPaneNext".to_string(),
        },
        PaletteAction {
            label: fl!("palette-swap-pane-prev"),
            action: "SwapPanePrev".to_string(),
        },
        PaletteAction {
            label: fl!("palette-break-pane"),
            action: "BreakPane".to_string(),
        },
        PaletteAction {
            label: fl!("palette-set-broadcast-on"),
            action: "SetBroadcastOn".to_string(),
        },
        PaletteAction {
            label: fl!("palette-set-broadcast-off"),
            action: "SetBroadcastOff".to_string(),
        },
        PaletteAction {
            label: fl!("palette-connect-serial"),
            action: "ConnectSerialPrompt".to_string(),
        },
        PaletteAction {
            label: fl!("palette-show-host-manager"),
            action: "ShowHostManager".to_string(),
        },
        PaletteAction {
            label: fl!("palette-show-macro-picker"),
            action: "ShowMacroPicker".to_string(),
        },
        PaletteAction {
            label: fl!("palette-sftp-upload"),
            action: "SftpUploadDialog".to_string(),
        },
        PaletteAction {
            label: fl!("palette-sftp-download"),
            action: "SftpDownloadDialog".to_string(),
        },
        PaletteAction {
            label: fl!("palette-show-settings"),
            action: "ShowSettings".to_string(),
        },
        // Sprint 5-2 / B1: prompt jumps via OSC 133 semantic marks.
        PaletteAction {
            label: fl!("palette-jump-prev-prompt"),
            action: "JumpPrevPrompt".to_string(),
        },
        PaletteAction {
            label: fl!("palette-jump-next-prompt"),
            action: "JumpNextPrompt".to_string(),
        },
        // Sprint 5-8 / Phase 4-5: tab tearing (includes the Wayland-alternative UX).
        PaletteAction {
            label: fl!("palette-detach-to-new-window"),
            action: "DetachToNewWindow".to_string(),
        },
        PaletteAction {
            label: fl!("palette-close-os-window"),
            action: "CloseOsWindow".to_string(),
        },
        PaletteAction {
            label: fl!("palette-quit"),
            action: "Quit".to_string(),
        },
    ]
}

/// Palette ranking extracted as a pure function (easier to test).
///
/// - Empty query: history order (`last_used` desc → `use_count` desc); actions
///   without history retain their registration order at the tail.
/// - Non-empty query: fuzzy score + `history_bonus`, sorted descending.
///   - `history_bonus` = `use_count * 10 + (recency bonus up to +200)`.
pub fn rank_actions<'a>(
    actions: &'a [PaletteAction],
    query: &str,
    history: &PaletteHistory,
    matcher: &SkimMatcherV2,
) -> Vec<&'a PaletteAction> {
    if query.is_empty() {
        // Empty query: history order → original order for un-recorded actions.
        let mut indexed: Vec<(usize, &PaletteAction)> = actions.iter().enumerate().collect();
        indexed.sort_by(|a, b| {
            let ha = history.get(&a.1.action);
            let hb = history.get(&b.1.action);
            match (ha, hb) {
                (Some(ea), Some(eb)) => eb
                    .last_used
                    .cmp(&ea.last_used)
                    .then_with(|| eb.use_count.cmp(&ea.use_count))
                    .then_with(|| a.0.cmp(&b.0)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.0.cmp(&b.0),
            }
        });
        return indexed.into_iter().map(|(_, a)| a).collect();
    }

    let mut scored: Vec<(i64, usize, &PaletteAction)> = actions
        .iter()
        .enumerate()
        .filter_map(|(idx, a)| {
            matcher.fuzzy_match(&a.label, query).map(|score| {
                let bonus = history_bonus(history.get(&a.action));
                (score + bonus, idx, a)
            })
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, a)| a).collect()
}

/// Compute the bonus to add to the fuzzy score from a history entry.
///
/// - Use-count bonus: `use_count * 10` (capped at 100).
/// - Recency bonus: +100 if last used within 1 day; +50 within 1 week; 0 otherwise.
fn history_bonus(entry: Option<&PaletteHistoryEntry>) -> i64 {
    let Some(e) = entry else { return 0 };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let use_bonus = (e.use_count as i64 * 10).min(100);
    let age_secs = now.saturating_sub(e.last_used);
    let recency_bonus = if age_secs < 86_400 {
        100
    } else if age_secs < 86_400 * 7 {
        50
    } else {
        0
    };
    use_bonus + recency_bonus
}

// ---- History persistence ----

/// Return the history file path.
///
/// Unix: `~/.local/state/nexterm/palette_history.json`
/// Windows: `%APPDATA%\nexterm\palette_history.json`
fn history_path() -> PathBuf {
    if let Ok(test_path) = std::env::var("__NEXTERM_TEST_PALETTE_HISTORY_PATH__") {
        return PathBuf::from(test_path);
    }

    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("nexterm").join("palette_history.json")
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            return PathBuf::from(xdg)
                .join("nexterm")
                .join("palette_history.json");
        }
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        home.join(".local")
            .join("state")
            .join("nexterm")
            .join("palette_history.json")
    }
}

/// Load the history from the JSON file (empty map if absent).
fn load_history() -> PaletteHistory {
    let path = history_path();
    if !path.exists() {
        return PaletteHistory::new();
    }
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => {
            warn!("failed to read command-palette history: {}", e);
            return PaletteHistory::new();
        }
    };
    match serde_json::from_str(&json) {
        Ok(map) => map,
        Err(e) => {
            warn!("failed to parse command-palette history: {}", e);
            PaletteHistory::new()
        }
    }
}

/// Save the history to the JSON file (atomic write; mode 0600 on Unix).
fn save_history(history: &PaletteHistory) {
    let path = history_path();
    let json = match serde_json::to_string_pretty(history) {
        Ok(j) => j,
        Err(e) => {
            warn!("failed to serialise command-palette history: {}", e);
            return;
        }
    };
    if let Err(e) = write_atomic_secure(&path, json.as_bytes()) {
        warn!("failed to save command-palette history: {}", e);
    }
}

/// Write a file atomically; enforce mode 0600 on Unix.
fn write_atomic_secure(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("could not obtain parent directory: {:?}", path),
        )
    })?;
    std::fs::create_dir_all(parent)?;

    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("nexterm"),
        std::process::id()
    );
    let tmp_path = parent.join(tmp_name);

    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Mutex used by locale-changing tests to avoid racing the global locale.
    static LOCALE_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn default_actions_exist() {
        let palette = CommandPalette::new();
        assert!(!palette.actions.is_empty());
    }

    #[test]
    fn default_actions_include_recently_added_entries() {
        // All six actions added in Sprint 5-7 / Phase 3-3 must be registered.
        let palette = CommandPalette::new();
        let ids: Vec<&str> = palette.actions.iter().map(|a| a.action.as_str()).collect();
        for expected in [
            "ClosePane",
            "NewWindow",
            "QuickSelect",
            "SetBroadcastOn",
            "SetBroadcastOff",
            "Quit",
        ] {
            assert!(
                ids.contains(&expected),
                "action {} should be registered in the palette",
                expected
            );
        }
    }

    #[test]
    fn no_query_returns_all_actions() {
        let palette = CommandPalette::new();
        assert_eq!(palette.filtered().len(), palette.actions.len());
    }

    #[test]
    fn fuzzy_match_works() {
        // "split" matches both Split Vertical and Split Horizontal (en locale).
        let _guard = LOCALE_MUTEX.lock().unwrap();
        nexterm_i18n::set_locale("en");
        let mut p = CommandPalette::new();
        p.query = "split".to_string();
        let results = p.filtered();
        assert!(results.len() >= 2);
        assert!(results.iter().any(|a| a.action == "SplitVertical"));
        assert!(results.iter().any(|a| a.action == "SplitHorizontal"));
    }

    #[test]
    fn fuzzy_match_works_with_japanese_locale() {
        // Verify that the Japanese word for "split" matches in the ja locale.
        let _guard = LOCALE_MUTEX.lock().unwrap();
        nexterm_i18n::set_locale("ja");
        let mut p = CommandPalette::new();
        p.query = "分割".to_string();
        let results = p.filtered();
        nexterm_i18n::set_locale("en"); // Reset after the test.
        assert!(results.len() >= 2);
        assert!(results.iter().any(|a| a.action == "SplitVertical"));
        assert!(results.iter().any(|a| a.action == "SplitHorizontal"));
    }

    #[test]
    fn selection_wraps_around() {
        let mut p = CommandPalette::new();
        let total = p.filtered().len();
        // From the tail, "next" wraps to the head.
        p.selected = total - 1;
        p.select_next();
        assert_eq!(p.selected, 0);
        // From the head, "previous" wraps to the tail.
        p.select_prev();
        assert_eq!(p.selected, total - 1);
    }

    #[test]
    fn register_custom_action() {
        let mut p = CommandPalette::new();
        let before = p.actions.len();
        p.register(PaletteAction {
            label: "Custom".to_string(),
            action: "Custom".to_string(),
        });
        assert_eq!(p.actions.len(), before + 1);
    }

    // ---- Sprint 5-7 / Phase 3-3: history-ranking tests ----

    fn dummy_actions() -> Vec<PaletteAction> {
        vec![
            PaletteAction {
                label: "Alpha".to_string(),
                action: "Alpha".to_string(),
            },
            PaletteAction {
                label: "Beta".to_string(),
                action: "Beta".to_string(),
            },
            PaletteAction {
                label: "Gamma".to_string(),
                action: "Gamma".to_string(),
            },
        ]
    }

    #[test]
    fn rank_actions_empty_query_no_history_uses_registration_order() {
        let actions = dummy_actions();
        let history = PaletteHistory::new();
        let matcher = SkimMatcherV2::default();
        let ranked = rank_actions(&actions, "", &history, &matcher);
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].action, "Alpha");
        assert_eq!(ranked[1].action, "Beta");
        assert_eq!(ranked[2].action, "Gamma");
    }

    #[test]
    fn rank_actions_empty_query_with_history_uses_history_order() {
        let actions = dummy_actions();
        let mut history = PaletteHistory::new();
        // Pretend Beta was used recently (once) and Gamma was used five times long ago.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        history.insert(
            "Beta".to_string(),
            PaletteHistoryEntry {
                last_used: now,
                use_count: 1,
            },
        );
        history.insert(
            "Gamma".to_string(),
            PaletteHistoryEntry {
                last_used: now - 3600 * 24 * 30, // 30 days ago
                use_count: 5,
            },
        );
        let matcher = SkimMatcherV2::default();
        let ranked = rank_actions(&actions, "", &history, &matcher);
        assert_eq!(ranked[0].action, "Beta", "most recent use comes first");
        assert_eq!(ranked[1].action, "Gamma", "Gamma is next (has history)");
        assert_eq!(ranked[2].action, "Alpha", "no-history entries come last");
    }

    #[test]
    fn rank_actions_with_query_history_bonus_boosts() {
        let actions = dummy_actions();
        let matcher = SkimMatcherV2::default();
        // Query "a" matches Alpha and Gamma.
        let no_hist = PaletteHistory::new();
        let ranked_no = rank_actions(&actions, "a", &no_hist, &matcher);

        let mut hist = PaletteHistory::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Gamma was used 10 times recently.
        hist.insert(
            "Gamma".to_string(),
            PaletteHistoryEntry {
                last_used: now,
                use_count: 10,
            },
        );
        let ranked_h = rank_actions(&actions, "a", &hist, &matcher);

        // Both runs include Gamma, but the run with history ranks Gamma higher.
        assert!(ranked_no.iter().any(|a| a.action == "Gamma"));
        assert!(ranked_h.iter().any(|a| a.action == "Gamma"));
        // With history, Gamma must rank above Alpha.
        let pos_gamma = ranked_h.iter().position(|a| a.action == "Gamma").unwrap();
        let pos_alpha = ranked_h.iter().position(|a| a.action == "Alpha").unwrap();
        assert!(
            pos_gamma < pos_alpha,
            "with history Gamma should rank above Alpha (pos_gamma={}, pos_alpha={})",
            pos_gamma,
            pos_alpha
        );
    }

    #[test]
    fn history_bonus_for_missing_entry_is_0() {
        assert_eq!(history_bonus(None), 0);
    }

    #[test]
    fn history_bonus_reflects_recent_use_count() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entry = PaletteHistoryEntry {
            last_used: now,
            use_count: 3,
        };
        // Within a day: recency=100, use_count*10=30 → 130.
        assert_eq!(history_bonus(Some(&entry)), 130);
    }

    #[test]
    fn history_bonus_clamps_use_count_at_100() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entry = PaletteHistoryEntry {
            last_used: now,
            use_count: 100,
        };
        // use_count*10 = 1000, clamped to 100 + recency 100 = 200.
        assert_eq!(history_bonus(Some(&entry)), 200);
    }

    #[test]
    fn history_bonus_for_old_use_has_zero_recency() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entry = PaletteHistoryEntry {
            last_used: now.saturating_sub(86_400 * 30), // 30 days ago
            use_count: 5,
        };
        // recency=0, use_count*10=50 → 50.
        assert_eq!(history_bonus(Some(&entry)), 50);
    }

    #[test]
    fn record_use_updates_use_count_and_last_used() {
        // `record_use` performs file I/O, so swap in a temp file via env var.
        let tmp = std::env::temp_dir().join(format!(
            "nexterm-test-palette-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // SAFETY: This env var name is test-specific; the path is unique so it
        // cannot interfere with other tests. The `unsafe` block is required
        // because of the Rust 2024 / std API change.
        unsafe {
            std::env::set_var(
                "__NEXTERM_TEST_PALETTE_HISTORY_PATH__",
                tmp.to_string_lossy().to_string(),
            );
        }

        let mut p = CommandPalette::new();
        p.record_use("Alpha");
        let entry_a = p.history.get("Alpha").expect("Alpha should be recorded");
        assert_eq!(entry_a.use_count, 1);
        let last_used_first = entry_a.last_used;

        // Second call: use_count must increase.
        p.record_use("Alpha");
        let entry_a2 = p.history.get("Alpha").unwrap();
        assert_eq!(entry_a2.use_count, 2);
        assert!(entry_a2.last_used >= last_used_first);

        // The history file must have been written.
        assert!(tmp.exists(), "history file should have been written");

        // Clean up.
        let _ = std::fs::remove_file(&tmp);
        unsafe {
            std::env::remove_var("__NEXTERM_TEST_PALETTE_HISTORY_PATH__");
        }
    }

    // ---- Phase 2c-F: @ prefix named-block search -----------------------

    fn block_action(id: u64, name: &str) -> PaletteAction {
        PaletteAction {
            label: name.to_string(),
            action: format!("BlockSelect:{}", id),
        }
    }

    #[test]
    fn build_named_block_actions_encodes_id_in_action_field() {
        let acts = build_named_block_actions(vec![
            (0x0000_0001_0000_000A, "deploy".to_string()),
            (0x0000_0001_0000_0014, "tests".to_string()),
        ]);
        assert_eq!(acts.len(), 2);
        assert_eq!(acts[0].label, "deploy");
        assert_eq!(acts[0].action, "BlockSelect:4294967306");
        assert_eq!(acts[1].action, "BlockSelect:4294967316");
    }

    #[test]
    fn filtered_switches_to_blocks_on_at_prefix() {
        let mut p = CommandPalette::new();
        p.set_named_block_actions(vec![
            block_action(1, "deploy"),
            block_action(2, "tests"),
            block_action(3, "lint"),
        ]);
        p.query = "@dep".to_string();
        let results = p.filtered();
        assert!(!results.is_empty(), "@dep must hit at least one block");
        assert_eq!(results[0].label, "deploy");
        // Static palette actions must NOT leak into @-mode results.
        assert!(
            results.iter().all(|a| a.action.starts_with("BlockSelect:")),
            "block-mode results must only carry BlockSelect ids"
        );
    }

    #[test]
    fn filtered_at_with_empty_subquery_returns_all_named_blocks() {
        let mut p = CommandPalette::new();
        p.set_named_block_actions(vec![block_action(1, "deploy"), block_action(2, "tests")]);
        p.query = "@".to_string();
        let results = p.filtered();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn filtered_at_with_no_named_blocks_is_empty() {
        let mut p = CommandPalette::new();
        // No set_named_block_actions call → empty list.
        p.query = "@anything".to_string();
        assert!(p.filtered().is_empty());
    }

    #[test]
    fn filtered_without_at_prefix_returns_static_actions() {
        let mut p = CommandPalette::new();
        p.set_named_block_actions(vec![block_action(1, "deploy")]);
        p.query.clear();
        let results = p.filtered();
        assert!(
            !results.is_empty(),
            "empty query must still surface the default actions"
        );
        // No BlockSelect entries in non-@ mode.
        assert!(
            results
                .iter()
                .all(|a| !a.action.starts_with("BlockSelect:"))
        );
    }
}
