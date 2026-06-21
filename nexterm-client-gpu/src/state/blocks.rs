//! Command-block UI operations on `ClientState` (Phase 2a of the blocks feature).
//!
//! All public methods here are thin orchestrators over the pure helpers in
//! [`crate::command_blocks`] and the persistence layer in
//! [`crate::named_blocks`]. The pure helpers are unit-tested in their own
//! modules; the tests here cover the ClientState-level glue:
//! navigation across an empty / single / multi-block pane, selection drop
//! when the underlying block disappears, and `save()` being invoked on name
//! changes only when something actually changed.
//!
//! No rendering or keybinding wiring lives here — that is Phase 2b.
//!
//! NOTE: methods are unit-tested but only called from renderer / keybinding
//! code that lands in Phase 2b. `dead_code` is silenced module-wide and the
//! attribute should be removed once Phase 2b wires the helpers up.

#![allow(dead_code)]

use nexterm_proto::Cell;

use super::ClientState;
use super::pane::PaneState;
use crate::command_blocks::{
    BlockId, CommandBlock, find_block_by_id, next_block_id, prev_block_id, sanitize_replay_command,
};

/// Convert a row of `Cell`s back to a printable string.
///
/// Mirrors `scrollback::Scrollback::line_to_string` (kept private over there).
/// Trailing whitespace is trimmed so that "ls\n        " comes back as "ls".
fn cells_to_string(line: &[Cell]) -> String {
    line.iter()
        .map(|c| c.ch)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Read the scrollback rows in `start_row..=end_row` and join them with `\n`.
///
/// Rows that fall outside the live scrollback (e.g. because the ring has
/// rotated past them) are silently skipped. The function returns `None` when
/// not a single row inside the range survives in scrollback.
fn extract_rows(pane: &PaneState, start_row: usize, end_row: usize) -> Option<String> {
    if end_row < start_row {
        return None;
    }
    let mut lines = Vec::with_capacity(end_row - start_row + 1);
    for row in start_row..=end_row {
        if let Some(cells) = pane.scrollback.get(row) {
            lines.push(cells_to_string(cells));
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(lines.join("\n"))
}

impl ClientState {
    /// Resolve the block that is currently selected in the focused pane.
    ///
    /// Returns `None` when no pane is focused, the focused pane has no blocks,
    /// nothing is selected, or the selected ID is stale (the block left the
    /// scrollback window).
    pub fn selected_command_block(&self) -> Option<&CommandBlock> {
        let pane = self.focused_pane()?;
        let id = self.selected_block?;
        find_block_by_id(&pane.blocks, id)
    }

    /// Clear the selection if it no longer maps to a block on the focused pane.
    ///
    /// Called from event handling after the block list is refreshed (e.g. when
    /// a new `SemanticMark` arrives) so that the renderer never highlights a
    /// stale ID.
    pub fn prune_block_selection(&mut self) {
        let Some(id) = self.selected_block else {
            return;
        };
        let still_present = self
            .focused_pane()
            .map(|p| find_block_by_id(&p.blocks, id).is_some())
            .unwrap_or(false);
        if !still_present {
            self.selected_block = None;
        }
    }

    /// Advance the selection to the next block in the focused pane.
    ///
    /// `None` is returned when there is no focused pane, no blocks, or the
    /// selection is already at the last block. The internal state is updated
    /// only when a successor exists.
    pub fn select_next_block(&mut self) -> Option<BlockId> {
        let pane = self.focused_pane()?;
        let next = next_block_id(&pane.blocks, self.selected_block);
        if next.is_some() {
            self.selected_block = next;
        }
        next
    }

    /// Move the selection to the previous block in the focused pane.
    pub fn select_prev_block(&mut self) -> Option<BlockId> {
        let pane = self.focused_pane()?;
        let prev = prev_block_id(&pane.blocks, self.selected_block);
        if prev.is_some() {
            self.selected_block = prev;
        }
        prev
    }

    /// Assign (or replace) the name of the selected block.
    ///
    /// Empty / whitespace-only names are treated as a removal (see
    /// [`crate::named_blocks::NamedBlockStore::set`]). Returns `true` when the
    /// store changed; in that case the store is persisted to disk before
    /// returning.
    pub fn set_selected_block_name(&mut self, name: &str) -> bool {
        let Some(id) = self.selected_block else {
            return false;
        };
        let changed = self.named_blocks.set(id, name);
        if changed {
            self.named_blocks.save();
        }
        changed
    }

    /// Remove the user-assigned name (if any) from the selected block.
    pub fn remove_selected_block_name(&mut self) -> bool {
        let Some(id) = self.selected_block else {
            return false;
        };
        let changed = self.named_blocks.remove(id);
        if changed {
            self.named_blocks.save();
        }
        changed
    }

    /// Look up the user-assigned name of the selected block, if any.
    pub fn selected_block_name(&self) -> Option<&str> {
        let id = self.selected_block?;
        self.named_blocks.get(id)
    }

    /// Build the clipboard payload for the currently-selected block.
    ///
    /// The string covers `prompt_row..=end_row` (or `prompt_row..=scrollback
    /// tail` for a still-running block) on the focused pane, joined with `\n`.
    /// Trailing per-line whitespace is trimmed via [`cells_to_string`].
    ///
    /// Returns `None` when nothing is selected, no pane is focused, the
    /// referenced block is no longer present in the block list, or every row
    /// in the range has already rotated out of scrollback.
    pub fn selected_block_text(&self) -> Option<String> {
        let pane = self.focused_pane()?;
        let id = self.selected_block?;
        let block = find_block_by_id(&pane.blocks, id)?;
        let end_row = block
            .end_row
            .unwrap_or_else(|| pane.scrollback.len().saturating_sub(1));
        extract_rows(pane, block.prompt_row, end_row)
    }

    /// Extract the **command line** of the currently-selected block, sanitised
    /// for safe re-injection into the PTY.
    ///
    /// The command is read from `command_row..output_row` on the focused pane.
    /// When `B` was not emitted (`command_row == output_row`) the prompt row
    /// itself is used as a fallback, which usually carries the full
    /// "`$ command`" string — sanitise then strips the prompt prefix only
    /// where it does not contain control characters (defensive: see
    /// [`sanitize_replay_command`]).
    ///
    /// Returns `None` for any of: no selection, focused pane gone, missing
    /// scrollback rows, empty result, embedded control bytes, or embedded
    /// newlines (which would mean replay re-runs every captured line).
    pub fn selected_block_replay_command(&self) -> Option<String> {
        let pane = self.focused_pane()?;
        let id = self.selected_block?;
        let block = find_block_by_id(&pane.blocks, id)?;

        // Prefer the explicit command range. If `B` was missing we treat the
        // entire prompt row as the user's input and let the sanitiser decide.
        let (start, end_inclusive) = if block.command_row == block.output_row {
            (block.command_row, block.command_row)
        } else {
            (block.command_row, block.output_row.saturating_sub(1))
        };
        let raw = extract_rows(pane, start, end_inclusive)?;
        sanitize_replay_command(&raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_blocks::{SemanticMark, SemanticMarkKind, extract_command_blocks};
    use crate::state::pane::PaneState;

    /// Lock used by the tests that exercise the on-disk named-block store.
    /// We re-point the store at a tempfile per test so the assertions are
    /// hermetic, but only one test at a time may hold the env var.
    static STORE_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct StoreEnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        path: std::path::PathBuf,
    }

    impl StoreEnvGuard {
        fn new(tag: &str) -> Self {
            let lock = STORE_ENV_MUTEX.lock().expect("test mutex poisoned");
            let mut path = std::env::temp_dir();
            path.push(format!("nexterm-state-blocks-test-{}.json", tag));
            let _ = std::fs::remove_file(&path);
            // SAFETY: tests are serialised by STORE_ENV_MUTEX.
            unsafe {
                std::env::set_var("__NEXTERM_TEST_NAMED_BLOCKS_PATH__", &path);
            }
            Self { _lock: lock, path }
        }
    }

    impl Drop for StoreEnvGuard {
        fn drop(&mut self) {
            // SAFETY: tests are serialised by STORE_ENV_MUTEX.
            unsafe {
                std::env::remove_var("__NEXTERM_TEST_NAMED_BLOCKS_PATH__");
            }
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn m(row: usize, kind: SemanticMarkKind, exit_code: Option<i32>) -> SemanticMark {
        SemanticMark {
            row,
            kind,
            exit_code,
        }
    }

    /// Build a `ClientState` with a single focused pane containing
    /// `block_count` complete blocks, then return both.
    fn state_with_blocks(pane_id: u32, block_count: usize) -> ClientState {
        let mut state = ClientState::new(80, 24, 1024);
        let mut pane = PaneState::new(80, 24, 1024);
        let mut marks = Vec::new();
        for i in 0..block_count {
            let base = i * 10;
            marks.push(m(base, SemanticMarkKind::PromptStart, None));
            marks.push(m(base + 5, SemanticMarkKind::CommandEnd, Some(0)));
        }
        pane.marks = marks.clone();
        pane.blocks = extract_command_blocks(pane_id, &marks);
        state.panes.insert(pane_id, pane);
        state.focused_pane_id = Some(pane_id);
        state
    }

    #[test]
    fn selected_command_block_is_none_initially() {
        let state = state_with_blocks(1, 3);
        assert!(state.selected_command_block().is_none());
    }

    #[test]
    fn select_next_from_none_picks_first_block() {
        let mut state = state_with_blocks(1, 3);
        let picked = state.select_next_block();
        assert!(picked.is_some());
        assert_eq!(picked, state.selected_block);
        assert_eq!(
            state.selected_command_block().map(|b| b.prompt_row),
            Some(0)
        );
    }

    #[test]
    fn select_next_walks_to_end_then_stops() {
        let mut state = state_with_blocks(1, 3);
        assert!(state.select_next_block().is_some());
        assert!(state.select_next_block().is_some());
        assert!(state.select_next_block().is_some());
        // Already on the last block.
        assert_eq!(state.select_next_block(), None);
        // Selection sticks at the last block.
        assert_eq!(
            state.selected_command_block().map(|b| b.prompt_row),
            Some(20)
        );
    }

    #[test]
    fn select_prev_from_none_picks_last_block() {
        let mut state = state_with_blocks(1, 3);
        let picked = state.select_prev_block();
        assert_eq!(
            state.selected_command_block().map(|b| b.prompt_row),
            Some(20)
        );
        assert_eq!(picked, state.selected_block);
    }

    #[test]
    fn select_prev_walks_to_start_then_stops() {
        let mut state = state_with_blocks(1, 3);
        state.select_prev_block();
        state.select_prev_block();
        state.select_prev_block();
        assert_eq!(state.select_prev_block(), None);
        assert_eq!(
            state.selected_command_block().map(|b| b.prompt_row),
            Some(0)
        );
    }

    #[test]
    fn navigation_is_a_noop_without_a_focused_pane() {
        let mut state = ClientState::new(80, 24, 1024);
        assert_eq!(state.select_next_block(), None);
        assert_eq!(state.select_prev_block(), None);
        assert!(state.selected_block.is_none());
    }

    #[test]
    fn prune_clears_selection_when_block_disappears() {
        let mut state = state_with_blocks(1, 2);
        state.select_next_block();
        // Selection is on block 0.
        assert!(state.selected_block.is_some());
        // Drop blocks.
        state.panes.get_mut(&1).unwrap().blocks.clear();
        state.prune_block_selection();
        assert!(state.selected_block.is_none());
    }

    #[test]
    fn prune_keeps_selection_when_block_is_still_present() {
        let mut state = state_with_blocks(1, 2);
        let picked = state.select_next_block();
        state.prune_block_selection();
        assert_eq!(state.selected_block, picked);
    }

    #[test]
    fn set_and_lookup_block_name_round_trip() {
        let _g = StoreEnvGuard::new("set-lookup");
        let mut state = state_with_blocks(1, 2);
        state.select_next_block();
        assert!(state.set_selected_block_name("deploy"));
        assert_eq!(state.selected_block_name(), Some("deploy"));
    }

    #[test]
    fn set_block_name_returns_false_without_selection() {
        let _g = StoreEnvGuard::new("no-selection");
        let mut state = state_with_blocks(1, 2);
        assert!(!state.set_selected_block_name("anything"));
    }

    #[test]
    fn set_block_name_returns_false_when_unchanged() {
        let _g = StoreEnvGuard::new("unchanged");
        let mut state = state_with_blocks(1, 2);
        state.select_next_block();
        assert!(state.set_selected_block_name("build"));
        assert!(!state.set_selected_block_name("build"));
    }

    #[test]
    fn remove_block_name_clears_assignment() {
        let _g = StoreEnvGuard::new("remove");
        let mut state = state_with_blocks(1, 2);
        state.select_next_block();
        state.set_selected_block_name("temp");
        assert!(state.remove_selected_block_name());
        assert!(state.selected_block_name().is_none());
        // Second remove is a no-op.
        assert!(!state.remove_selected_block_name());
    }

    // ---- Block text extraction (Phase 2c-2/3) ----

    /// Build a `Cell`-row from a `&str`, padded to `width` blanks.
    fn row(text: &str, width: usize) -> Vec<nexterm_proto::Cell> {
        let mut cells: Vec<nexterm_proto::Cell> = text
            .chars()
            .map(|ch| nexterm_proto::Cell {
                ch,
                ..nexterm_proto::Cell::default()
            })
            .collect();
        while cells.len() < width {
            cells.push(nexterm_proto::Cell::default());
        }
        cells
    }

    /// Construct a focused-pane state with the given rows pushed to scrollback
    /// and a single complete block spanning rows 0..=`last_row`.
    fn pane_with_rows(
        rows: &[&str],
        block_id: u64,
        command_row: usize,
        output_row: usize,
    ) -> ClientState {
        let mut state = ClientState::new(80, 24, 1024);
        let mut pane = PaneState::new(80, 24, 1024);
        for line in rows {
            pane.scrollback.push_line(row(line, 80));
        }
        pane.blocks = vec![CommandBlock {
            id: block_id,
            pane_id: 1,
            prompt_row: 0,
            command_row,
            output_row,
            end_row: Some(rows.len() - 1),
            exit_code: Some(0),
            collapsed: false,
        }];
        state.panes.insert(1, pane);
        state.focused_pane_id = Some(1);
        state.selected_block = Some(block_id);
        state
    }

    #[test]
    fn selected_block_text_joins_rows_with_newlines() {
        let state = pane_with_rows(&["$ ls", "foo.txt", "bar.txt"], 1, 0, 1);
        let text = state.selected_block_text().expect("block text");
        assert_eq!(text, "$ ls\nfoo.txt\nbar.txt");
    }

    #[test]
    fn selected_block_text_returns_none_without_selection() {
        let mut state = pane_with_rows(&["$ ls", "x"], 1, 0, 1);
        state.selected_block = None;
        assert!(state.selected_block_text().is_none());
    }

    #[test]
    fn selected_block_text_returns_none_when_block_is_stale() {
        let mut state = pane_with_rows(&["$ ls", "x"], 1, 0, 1);
        // Point at a block that doesn't exist on the pane.
        state.selected_block = Some(0xDEAD_BEEF);
        assert!(state.selected_block_text().is_none());
    }

    #[test]
    fn selected_block_text_trims_trailing_padding() {
        // The fixture pads to width 80; trimming should drop the trailing spaces.
        let state = pane_with_rows(&["$ echo hi", "hi"], 1, 0, 1);
        let text = state.selected_block_text().unwrap();
        assert!(
            !text.contains("  "),
            "no double-spaces should leak from padding"
        );
    }

    #[test]
    fn selected_block_replay_uses_command_row_only() {
        // command_row=0, output_row=1 → replay reads only row 0.
        let state = pane_with_rows(&["ls -la", "file1", "file2"], 1, 0, 1);
        let cmd = state.selected_block_replay_command().expect("replay");
        assert_eq!(cmd, "ls -la");
    }

    #[test]
    fn selected_block_replay_falls_back_to_prompt_when_b_missing() {
        // Shell omits `B`: command_row == output_row → replay reads that single row.
        let state = pane_with_rows(&["$ pwd", "/home/me"], 1, 0, 0);
        let cmd = state.selected_block_replay_command().expect("replay");
        assert_eq!(cmd, "$ pwd");
    }

    #[test]
    fn selected_block_replay_rejects_embedded_escape() {
        let state = pane_with_rows(&["ls\u{1b}[31m", "out"], 1, 0, 1);
        assert!(state.selected_block_replay_command().is_none());
    }

    #[test]
    fn selected_block_replay_returns_none_without_selection() {
        let mut state = pane_with_rows(&["echo hi", "hi"], 1, 0, 1);
        state.selected_block = None;
        assert!(state.selected_block_replay_command().is_none());
    }

    #[test]
    fn empty_name_is_treated_as_removal() {
        let _g = StoreEnvGuard::new("empty");
        let mut state = state_with_blocks(1, 2);
        state.select_next_block();
        state.set_selected_block_name("temp");
        assert!(state.set_selected_block_name("   "));
        assert!(state.selected_block_name().is_none());
    }
}
