//! In-app acrylic capture bookkeeping (UI/UX v3 P2b). Pure state — no wgpu
//! types — so the invalidation rules are unit-testable without a GPU.

/// Tracks when the offscreen `scene_color` capture needs to be redone.
/// The capture is a frozen snapshot taken once per overlay-open
/// transition; it is intentionally *not* refreshed every frame while an
/// overlay stays open (see the design spec's "Non-goals").
#[derive(Debug, Default)]
pub(crate) struct AcrylicCaptureState {
    last_overlay_open_count: usize,
    generation: u64,
    captured_generation: Option<u64>,
    captured_while_open: bool,
}

impl AcrylicCaptureState {
    /// Call once per frame with how many overlays are currently open.
    pub(crate) fn note_overlay_open_count(&mut self, count: usize) {
        let was_open = self.last_overlay_open_count > 0;
        let now_open = count > 0;
        if !was_open && now_open {
            // 0 -> N transition: force a fresh capture.
            self.captured_generation = None;
        }
        self.last_overlay_open_count = count;
    }

    /// Call whenever the window resizes or the DPI scale changes.
    pub(crate) fn note_resize(&mut self) {
        self.generation += 1;
    }

    /// Whether the caller should (re-)capture `scene_color` and re-run the
    /// blur chain this frame. `overlay_open` must match what
    /// `note_overlay_open_count` was last told.
    pub(crate) fn is_dirty(&self, overlay_open: bool) -> bool {
        overlay_open && self.captured_generation != Some(self.generation)
    }

    /// Call after a capture + blur pass has run this frame.
    pub(crate) fn mark_captured(&mut self) {
        self.captured_generation = Some(self.generation);
        self.captured_while_open = true;
        let _ = self.captured_while_open; // silence unused-field lint until Task 7 reads it
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_from_zero_to_one_overlay_is_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(0);
        assert!(!state.is_dirty(false));
        state.note_overlay_open_count(1);
        assert!(state.is_dirty(true));
    }

    #[test]
    fn staying_open_with_more_overlays_is_not_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        assert!(!state.is_dirty(true));
        state.note_overlay_open_count(2);
        assert!(!state.is_dirty(true));
    }

    #[test]
    fn resize_while_open_marks_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        assert!(!state.is_dirty(true));
        state.note_resize();
        assert!(state.is_dirty(true));
    }

    #[test]
    fn resize_while_closed_does_not_force_a_capture() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(0);
        state.note_resize();
        assert!(!state.is_dirty(false));
    }

    #[test]
    fn closing_and_reopening_recaptures() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        state.note_overlay_open_count(0);
        state.note_overlay_open_count(1);
        assert!(state.is_dirty(true));
    }
}
