//! Settings-panel title-bar drag (mouse-driven repositioning) and the
//! pure `clamp_panel_position` helper used to keep the panel onscreen.
//! Split out of `mod.rs` to stay under the 800-line-per-file limit.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::SettingsPanel;

impl SettingsPanel {
    /// Phase 3 (UI 4-tasks): start a title-bar drag.
    ///
    /// Records the grab anchor so subsequent `update_drag` calls can compute
    /// the new offset as a simple subtraction. Called from
    /// `on_mouse_left_pressed` when the hit-test reports `TitleBar`.
    pub fn start_drag(&mut self, cursor_x: f32, cursor_y: f32) {
        self.drag_anchor = Some((cursor_x - self.drag_offset.0, cursor_y - self.drag_offset.1));
    }

    /// Phase 3 (UI 4-tasks): update the live drag offset.
    ///
    /// No-op when no drag is in flight, so it is safe to call on every cursor
    /// movement.
    pub fn update_drag(&mut self, cursor_x: f32, cursor_y: f32) {
        if let Some((ax, ay)) = self.drag_anchor {
            self.drag_offset = (cursor_x - ax, cursor_y - ay);
        }
    }

    /// Phase 3 (UI 4-tasks): end a title-bar drag.
    ///
    /// Called from `on_mouse_left_released` regardless of where the cursor
    /// ended up — the final `drag_offset` is whatever the latest
    /// `update_drag` recorded.
    pub fn end_drag(&mut self) {
        self.drag_anchor = None;
    }

    /// Phase 3 (UI 4-tasks): is a title-bar drag currently in flight?
    pub fn is_dragging(&self) -> bool {
        self.drag_anchor.is_some()
    }

    /// Panel visibility in `[0, 1]`: 0 fully hidden, 1 fully shown.
    ///
    /// The curve now comes from `SurfaceMotion` (UI/UX v3 P3b) rather than the
    /// hand-rolled ease-out this used to apply.
    pub fn eased_progress(&self, now: std::time::Instant) -> f32 {
        self.motion.progress(now)
    }
}

/// Phase 3 (UI 4-tasks, 2026-06-12): apply a drag-to-move offset to the panel's
/// centered base position, clamping the result so the title bar is always
/// reachable. Pure function — covered by the tests in `panel_drag_tests`.
///
/// `base_x` / `base_y` are the panel's default centered top-left in pixels.
/// `offset` is the cumulative drag delta from `SettingsPanel.drag_offset`.
/// `panel_w` / `panel_h` size the panel; `sw` / `sh` size the window;
/// `title_h` is the title-bar height (the portion that must stay onscreen
/// vertically so the user can always grab it back). The clamp keeps the panel
/// fully visible horizontally (`0 ..= sw - panel_w`) and ensures at least the
/// title bar height stays inside the window vertically (`0 ..= sh - title_h`).
#[allow(clippy::too_many_arguments)]
pub fn clamp_panel_position(
    base_x: f32,
    base_y: f32,
    panel_w: f32,
    _panel_h: f32,
    sw: f32,
    sh: f32,
    title_h: f32,
    offset: (f32, f32),
) -> (f32, f32) {
    let raw_x = base_x + offset.0;
    let raw_y = base_y + offset.1;
    let max_x = (sw - panel_w).max(0.0);
    let max_y = (sh - title_h).max(0.0);
    (raw_x.clamp(0.0, max_x), raw_y.clamp(0.0, max_y))
}

#[cfg(test)]
mod panel_drag_tests {
    use super::*;

    /// Default-size panel with a zero offset must render at its base center.
    #[test]
    fn zero_offset_returns_base_position() {
        let (x, y) =
            clamp_panel_position(100.0, 80.0, 800.0, 600.0, 1280.0, 800.0, 30.0, (0.0, 0.0));
        assert_eq!(x, 100.0);
        assert_eq!(y, 80.0);
    }

    /// A small positive offset moves the panel without hitting any clamp.
    #[test]
    fn small_offset_applies_directly() {
        let (x, y) =
            clamp_panel_position(100.0, 80.0, 800.0, 600.0, 1280.0, 800.0, 30.0, (50.0, 25.0));
        assert_eq!(x, 150.0);
        assert_eq!(y, 105.0);
    }

    /// A huge positive offset must clamp at the right/bottom such that the
    /// panel is still fully visible horizontally and the title bar still fits.
    #[test]
    fn large_positive_offset_clamps_to_right_and_bottom() {
        let (x, y) = clamp_panel_position(
            100.0,
            80.0,
            800.0,
            600.0,
            1280.0,
            800.0,
            30.0,
            (9999.0, 9999.0),
        );
        // sw - panel_w = 1280 - 800 = 480
        assert_eq!(x, 480.0);
        // sh - title_h = 800 - 30 = 770
        assert_eq!(y, 770.0);
    }

    /// A large negative offset cannot push the panel past x=0 / y=0.
    #[test]
    fn large_negative_offset_clamps_to_origin() {
        let (x, y) = clamp_panel_position(
            100.0,
            80.0,
            800.0,
            600.0,
            1280.0,
            800.0,
            30.0,
            (-9999.0, -9999.0),
        );
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
    }

    /// When the panel is wider than the window, the clamp range collapses to
    /// `0..=0` and the panel pins to the left edge (never disappears off-screen).
    #[test]
    fn panel_wider_than_window_pins_to_left() {
        let (x, _) =
            clamp_panel_position(0.0, 80.0, 1500.0, 600.0, 1280.0, 800.0, 30.0, (200.0, 0.0));
        assert_eq!(x, 0.0);
    }

    /// `start_drag` → `update_drag` → `end_drag` records the expected offset
    /// and clears the anchor on release.
    #[test]
    fn start_update_end_drag_records_offset() {
        let config = nexterm_config::Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert_eq!(panel.drag_offset, (0.0, 0.0));
        assert!(!panel.is_dragging());

        // Press at (200, 100).
        panel.start_drag(200.0, 100.0);
        assert!(panel.is_dragging());

        // Move the cursor to (260, 130) → offset must be (60, 30).
        panel.update_drag(260.0, 130.0);
        assert_eq!(panel.drag_offset, (60.0, 30.0));

        // Release: offset persists but drag is no longer in flight.
        panel.end_drag();
        assert!(!panel.is_dragging());
        assert_eq!(panel.drag_offset, (60.0, 30.0));
    }

    /// Calling `update_drag` without a prior `start_drag` is a no-op.
    /// (Defensive check for the mouse-move path which fires unconditionally.)
    #[test]
    fn update_drag_without_start_is_noop() {
        let config = nexterm_config::Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.update_drag(500.0, 500.0);
        assert_eq!(panel.drag_offset, (0.0, 0.0));
    }

    /// A second drag starting from an already-offset panel must compound the
    /// previous offset rather than reset it.
    #[test]
    fn second_drag_compounds_previous_offset() {
        let config = nexterm_config::Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.start_drag(100.0, 100.0);
        panel.update_drag(150.0, 120.0);
        panel.end_drag();
        assert_eq!(panel.drag_offset, (50.0, 20.0));

        // Second drag from (300, 300) to (320, 310) → +20, +10.
        panel.start_drag(300.0, 300.0);
        panel.update_drag(320.0, 310.0);
        panel.end_drag();
        assert_eq!(panel.drag_offset, (70.0, 30.0));
    }

    /// `close()` must zero the offset (next open returns to centered).
    #[test]
    fn close_resets_drag_offset() {
        let config = nexterm_config::Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.start_drag(0.0, 0.0);
        panel.update_drag(123.0, 45.0);
        panel.close(
            std::time::Instant::now(),
            &nexterm_config::AnimationsConfig::default(),
        );
        assert_eq!(panel.drag_offset, (0.0, 0.0));
        assert!(!panel.is_dragging());
    }
}
