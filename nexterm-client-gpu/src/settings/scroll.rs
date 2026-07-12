//! Vertical scroll state for the settings panel's content area.
//!
//! Moved out of `settings_panel.rs` verbatim (Phase B6 mechanical split).

/// Vertical scroll state for the settings panel's content area (Phase B1).
///
/// Tracks how far the content has been scrolled down (`offset_px`), the
/// total height of the current category's content (`content_h_px`), and the
/// visible height of the content viewport (`viewport_h_px`). Kept as plain
/// data plus pure methods so the clamping logic can be unit-tested without
/// any GPU or windowing state. The renderer recomputes `content_h_px` /
/// `viewport_h_px` every frame (layout depends on category + list lengths)
/// and calls `clamp()`; input handlers only ever call `scroll_by()`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrollState {
    pub offset_px: f32,
    pub content_h_px: f32,
    pub viewport_h_px: f32,
}

impl ScrollState {
    /// Maximum valid scroll offset: content taller than the viewport can
    /// scroll by the overflow amount; content that fits needs no scroll.
    pub fn max_offset(&self) -> f32 {
        (self.content_h_px - self.viewport_h_px).max(0.0)
    }

    /// Clamp `offset_px` into `0..=max_offset()`. Call after any change to
    /// `offset_px`, `content_h_px`, or `viewport_h_px` (e.g. a category
    /// switch resizes the content, or the viewport is resized).
    pub fn clamp(&mut self) {
        self.offset_px = self.offset_px.clamp(0.0, self.max_offset());
    }

    /// Scroll by `delta_px` (positive = scroll down / reveal content below),
    /// clamping the result to the valid range.
    pub fn scroll_by(&mut self, delta_px: f32) {
        self.offset_px += delta_px;
        self.clamp();
    }

    /// Whether the content overflows the viewport (the scrollbar should be drawn).
    pub fn is_scrollable(&self) -> bool {
        self.content_h_px > self.viewport_h_px
    }

    /// Reset to the top. Called on category switch and panel close so the
    /// next view (or the next open) starts unscrolled.
    pub fn reset(&mut self) {
        self.offset_px = 0.0;
    }
}

#[cfg(test)]
mod scroll_state_tests {
    use super::ScrollState;

    #[test]
    fn max_offset_is_zero_when_content_fits_viewport() {
        let s = ScrollState {
            offset_px: 0.0,
            content_h_px: 100.0,
            viewport_h_px: 200.0,
        };
        assert_eq!(s.max_offset(), 0.0);
        assert!(!s.is_scrollable());
    }

    #[test]
    fn max_offset_is_overflow_when_content_exceeds_viewport() {
        let s = ScrollState {
            offset_px: 0.0,
            content_h_px: 500.0,
            viewport_h_px: 200.0,
        };
        assert_eq!(s.max_offset(), 300.0);
        assert!(s.is_scrollable());
    }

    #[test]
    fn clamp_keeps_offset_at_or_above_zero() {
        let mut s = ScrollState {
            offset_px: -50.0,
            content_h_px: 500.0,
            viewport_h_px: 200.0,
        };
        s.clamp();
        assert_eq!(s.offset_px, 0.0);
    }

    #[test]
    fn clamp_keeps_offset_at_or_below_max_offset() {
        let mut s = ScrollState {
            offset_px: 9999.0,
            content_h_px: 500.0,
            viewport_h_px: 200.0,
        };
        s.clamp();
        assert_eq!(s.offset_px, 300.0);
    }

    #[test]
    fn clamp_pins_offset_to_zero_when_content_shrinks_below_viewport() {
        // Simulates switching from a long list (Keybindings) to a short one
        // (Font): the previous offset is no longer valid once content_h_px
        // drops below viewport_h_px.
        let mut s = ScrollState {
            offset_px: 300.0,
            content_h_px: 500.0,
            viewport_h_px: 200.0,
        };
        s.clamp();
        assert_eq!(s.offset_px, 300.0, "still valid before content shrinks");

        s.content_h_px = 100.0;
        s.clamp();
        assert_eq!(s.offset_px, 0.0, "clamped back to 0 once content shrinks");
    }

    #[test]
    fn scroll_by_accumulates_and_clamps() {
        let mut s = ScrollState {
            offset_px: 0.0,
            content_h_px: 500.0,
            viewport_h_px: 200.0,
        };
        s.scroll_by(100.0);
        assert_eq!(s.offset_px, 100.0);
        s.scroll_by(100.0);
        assert_eq!(s.offset_px, 200.0);
        // Overshoot past max_offset (300.0) clamps instead of going negative-adjacent.
        s.scroll_by(1000.0);
        assert_eq!(s.offset_px, 300.0);
        // Scrolling back up clamps at 0 instead of going negative.
        s.scroll_by(-99999.0);
        assert_eq!(s.offset_px, 0.0);
    }

    #[test]
    fn reset_returns_to_top_without_touching_measured_sizes() {
        let mut s = ScrollState {
            offset_px: 250.0,
            content_h_px: 500.0,
            viewport_h_px: 200.0,
        };
        s.reset();
        assert_eq!(s.offset_px, 0.0);
        assert_eq!(s.content_h_px, 500.0);
        assert_eq!(s.viewport_h_px, 200.0);
    }

    #[test]
    fn default_state_is_unscrolled_and_empty() {
        let s = ScrollState::default();
        assert_eq!(s.offset_px, 0.0);
        assert_eq!(s.content_h_px, 0.0);
        assert_eq!(s.viewport_h_px, 0.0);
        assert!(!s.is_scrollable());
    }
}
