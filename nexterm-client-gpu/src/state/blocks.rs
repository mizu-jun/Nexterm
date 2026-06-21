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

use super::ClientState;
use crate::command_blocks::{
    BlockId, CommandBlock, find_block_by_id, next_block_id, prev_block_id,
};

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
