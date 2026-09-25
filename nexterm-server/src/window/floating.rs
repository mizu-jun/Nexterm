//! Rectangle information for floating panes.

/// Rectangle information for a floating pane.
#[derive(Clone, Debug)]
pub struct FloatRect {
    pub col_off: u16,
    pub row_off: u16,
    pub cols: u16,
    pub rows: u16,
}

impl FloatRect {
    /// Minimum usable floating-pane size, matching the floors `open_floating_pane` /
    /// `resize_floating_pane` have always applied.
    pub const MIN_COLS: u16 = 10;
    pub const MIN_ROWS: u16 = 5;

    /// Clamp this rect so it stays fully inside a `total_cols` x `total_rows` window.
    ///
    /// This is the single source of truth for floating-pane bounds: `open_floating_pane`,
    /// `move_floating_pane` and `resize_floating_pane` all call it before returning, so the rect
    /// the server hands back to clients can never diverge between "what was computed when the
    /// user dragged/resized it" and "what's actually inside the window" — previously only the
    /// open path clamped, so a drag or resize could push the pane partly (or entirely) off the
    /// window with no correction.
    pub fn clamp_to(&mut self, total_cols: u16, total_rows: u16) {
        self.cols = self
            .cols
            .clamp(Self::MIN_COLS, total_cols.max(Self::MIN_COLS));
        self.rows = self
            .rows
            .clamp(Self::MIN_ROWS, total_rows.max(Self::MIN_ROWS));
        let max_col_off = total_cols.saturating_sub(self.cols);
        let max_row_off = total_rows.saturating_sub(self.rows);
        self.col_off = self.col_off.min(max_col_off);
        self.row_off = self.row_off.min(max_row_off);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_to_leaves_an_in_bounds_rect_untouched() {
        let mut rect = FloatRect {
            col_off: 5,
            row_off: 3,
            cols: 20,
            rows: 10,
        };
        rect.clamp_to(80, 24);
        assert_eq!(rect.col_off, 5);
        assert_eq!(rect.row_off, 3);
        assert_eq!(rect.cols, 20);
        assert_eq!(rect.rows, 10);
    }

    #[test]
    fn clamp_to_pulls_an_off_window_offset_back_inside() {
        // Simulates a drag that pushed the pane past the right/bottom edge.
        let mut rect = FloatRect {
            col_off: 999,
            row_off: 999,
            cols: 20,
            rows: 10,
        };
        rect.clamp_to(80, 24);
        assert_eq!(rect.col_off, 60); // 80 - 20
        assert_eq!(rect.row_off, 14); // 24 - 10
    }

    #[test]
    fn clamp_to_shrinks_a_too_large_size_to_the_window() {
        let mut rect = FloatRect {
            col_off: 0,
            row_off: 0,
            cols: 500,
            rows: 500,
        };
        rect.clamp_to(80, 24);
        assert_eq!(rect.cols, 80);
        assert_eq!(rect.rows, 24);
        assert_eq!(rect.col_off, 0);
        assert_eq!(rect.row_off, 0);
    }

    #[test]
    fn clamp_to_never_drops_below_the_minimum_size_even_for_a_tiny_window() {
        let mut rect = FloatRect {
            col_off: 0,
            row_off: 0,
            cols: 20,
            rows: 10,
        };
        rect.clamp_to(4, 2);
        assert_eq!(rect.cols, FloatRect::MIN_COLS);
        assert_eq!(rect.rows, FloatRect::MIN_ROWS);
    }
}
