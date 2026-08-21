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

/// The 4 diagonal taps of one dual-Kawase downsample pass, in texels.
/// Mirrors the WGSL `kawase_downsample` entry point in `shaders.rs` —
/// keep the two in sync.
#[allow(dead_code)] // CPU-side spec for the WGSL shader's tap math; exercised only by its own unit tests (see Task 5).
pub(crate) fn kawase_downsample_offsets(texel_size: (f32, f32)) -> [(f32, f32); 4] {
    let (hx, hy) = (texel_size.0 * 0.5, texel_size.1 * 0.5);
    [(-hx, -hy), (hx, -hy), (-hx, hy), (hx, hy)]
}

/// The 8 taps of one dual-Kawase upsample pass (4 axis-aligned taps at
/// 2 texels, weight 1; 4 diagonal taps at 1 texel, weight 2 — applied by
/// the shader, not encoded here). Mirrors `kawase_upsample` in
/// `shaders.rs`.
#[allow(dead_code)] // CPU-side spec for the WGSL shader's tap math; exercised only by its own unit tests (see Task 5).
pub(crate) fn kawase_upsample_offsets(texel_size: (f32, f32)) -> [(f32, f32); 8] {
    let (x, y) = texel_size;
    [
        (-2.0 * x, 0.0),
        (2.0 * x, 0.0),
        (0.0, -2.0 * y),
        (0.0, 2.0 * y),
        (-x, y),
        (x, y),
        (-x, -y),
        (x, -y),
    ]
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

#[cfg(test)]
mod offset_tests {
    use super::*;

    #[test]
    fn downsample_offsets_are_symmetric_half_texel() {
        let offsets = kawase_downsample_offsets((2.0, 4.0));
        // half-texel = (1.0, 2.0); four diagonal corners.
        assert_eq!(
            offsets,
            [(-1.0, -2.0), (1.0, -2.0), (-1.0, 2.0), (1.0, 2.0)]
        );
    }

    #[test]
    fn upsample_offsets_are_symmetric_full_and_double_texel() {
        let offsets = kawase_upsample_offsets((2.0, 4.0));
        assert_eq!(offsets.len(), 8);
        // The four axis-aligned double-distance taps come first by
        // construction, then the four single-distance diagonal taps.
        assert_eq!(offsets[0], (-4.0, 0.0));
        assert_eq!(offsets[4], (-2.0, 4.0));
    }
}
