//! Pure layout math for the settings panel's two-column rows.
//!
//! Every category (Font, Theme, Window, ...) lays out its fields as a label
//! column on the left and a control/value column on the right (Windows
//! Terminal style). The split point used to be a mix of ad-hoc fixed cell
//! offsets (`cell_w * 16.0`, `cell_w * 26.0`, ...) scattered across the
//! per-category code. [`compute_row_layout`] centralizes that computation so
//! the label column scales with the available content width instead of
//! being hard-coded, and every row builder in [`super::row`] shares it.

/// Horizontal layout for a single label+control row, in pixels, relative to
/// the content area's inner-left x (`content_inner_x`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::renderer) struct RowLayout {
    /// Width reserved for the label column, in pixels.
    pub label_w: f32,
    /// X offset (from `content_inner_x`) where the control column starts.
    pub control_x_off: f32,
    /// Width available for the control column, in pixels.
    pub control_w: f32,
}

/// Label column target as a fraction of the content width.
const LABEL_FRACTION: f32 = 0.38;
/// Minimum/maximum label column width, in character cells, so the label
/// column stays readable at both very narrow and very wide panel sizes.
const LABEL_MIN_COLS: f32 = 10.0;
const LABEL_MAX_COLS: f32 = 28.0;
/// Gap between the label and control columns, in character cells.
const COLUMN_GAP_COLS: f32 = 2.0;
/// Minimum control column width to reserve, in character cells, so the
/// label column never crowds out the control entirely on narrow panels.
const CONTROL_MIN_COLS: f32 = 6.0;

/// Compute the label/control column split for a row inside a content area
/// `content_w_px` pixels wide, using `cell_w`-wide character cells.
///
/// Degenerates to an all-zero layout when either input is non-positive so
/// callers do not need to special-case a not-yet-measured frame.
pub(in crate::renderer) fn compute_row_layout(content_w_px: f32, cell_w: f32) -> RowLayout {
    if cell_w <= 0.0 || content_w_px <= 0.0 {
        return RowLayout {
            label_w: 0.0,
            control_x_off: 0.0,
            control_w: 0.0,
        };
    }

    let content_cols = content_w_px / cell_w;
    let target_cols = content_cols * LABEL_FRACTION;

    // Upper bound: keep at least `CONTROL_MIN_COLS` (+ the gap) for the
    // control column, and never exceed `LABEL_MAX_COLS` or the content
    // width itself.
    let upper_cols = LABEL_MAX_COLS
        .min((content_cols - COLUMN_GAP_COLS - CONTROL_MIN_COLS).max(0.0))
        .min(content_cols);
    // Lower bound: `LABEL_MIN_COLS`, but never above `upper_cols` — on very
    // narrow content areas the control-column reservation above can push
    // `upper_cols` below `LABEL_MIN_COLS`, and `clamp` requires min <= max.
    let lower_cols = LABEL_MIN_COLS.min(upper_cols);
    let label_cols = target_cols.clamp(lower_cols, upper_cols);

    let label_w = label_cols * cell_w;
    let control_x_off = label_w + COLUMN_GAP_COLS * cell_w;
    let control_w = (content_w_px - control_x_off).max(0.0);

    RowLayout {
        label_w,
        control_x_off,
        control_w,
    }
}

/// Maximum list rows the list-shaped categories (Ssh, Keybindings) draw at
/// once. Anything longer is windowed around the selection so the edit panel
/// and the Add/Delete buttons below the list stay reachable no matter how
/// many entries exist.
pub(in crate::renderer) const MAX_LIST_ROWS: usize = 8;

/// Vertical pitch of one list row, in character-cell heights. Shared by the
/// list-shaped categories so [`ListWindow::block_rows`] and the per-row `y`
/// computations cannot drift apart.
pub(in crate::renderer) const LIST_ROW_PITCH: f32 = 1.2;

/// The contiguous slice of a settings list to draw: at most `max_rows`
/// items, positioned so the selected item is always inside the window
/// (centered where possible, clamped at either end of the list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::renderer) struct ListWindow {
    /// Index of the first drawn item.
    pub first: usize,
    /// Number of drawn items.
    pub visible: usize,
    /// True when the window shows only part of the list; the caller draws a
    /// range-indicator row right below the last visible item.
    pub clipped: bool,
}

impl ListWindow {
    /// Height of the drawn list block in character-cell heights: the visible
    /// rows plus the range-indicator row when the list is clipped. An
    /// unclipped window occupies exactly the pre-windowing
    /// `len * LIST_ROW_PITCH`, so short lists keep their historical layout.
    pub(in crate::renderer) fn block_rows(&self) -> f32 {
        let indicator = if self.clipped { LIST_ROW_PITCH } else { 0.0 };
        self.visible as f32 * LIST_ROW_PITCH + indicator
    }
}

/// Compute the [`ListWindow`] for a `len`-item list with `selected` as the
/// current selection, drawing at most `max_rows` rows.
///
/// An out-of-bounds `selected` is treated as the last item (the drawing code
/// clamps the same way), and `max_rows == 0` degenerates to an empty window
/// rather than panicking.
pub(in crate::renderer) fn list_window(len: usize, selected: usize, max_rows: usize) -> ListWindow {
    if len <= max_rows {
        return ListWindow {
            first: 0,
            visible: len,
            clipped: false,
        };
    }
    if max_rows == 0 {
        return ListWindow {
            first: 0,
            visible: 0,
            clipped: true,
        };
    }
    let sel = selected.min(len - 1);
    let first = sel.saturating_sub(max_rows / 2).min(len - max_rows);
    ListWindow {
        first,
        visible: max_rows,
        clipped: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_W: f32 = 10.0;

    #[test]
    fn zero_or_negative_inputs_degenerate_to_zero() {
        let l = compute_row_layout(0.0, CELL_W);
        assert_eq!(l.label_w, 0.0);
        assert_eq!(l.control_x_off, 0.0);
        assert_eq!(l.control_w, 0.0);

        let l = compute_row_layout(-100.0, CELL_W);
        assert_eq!(l.label_w, 0.0);

        let l = compute_row_layout(500.0, 0.0);
        assert_eq!(l.label_w, 0.0);
    }

    #[test]
    fn wide_content_clamps_label_to_max_cols() {
        // 200 cells wide: 38% = 76 cols, clamped to LABEL_MAX_COLS = 28.
        let l = compute_row_layout(200.0 * CELL_W, CELL_W);
        assert_eq!(l.label_w, LABEL_MAX_COLS * CELL_W);
        assert!(l.control_w > 0.0);
    }

    #[test]
    fn narrow_content_clamps_label_to_min_cols_or_less() {
        // 12 cells wide content: 38% = ~4.6 cols, clamped up to LABEL_MIN_COLS (10)
        // would leave no room for the control column, so the max-cols-for-control
        // bound must win and keep some control width available.
        let l = compute_row_layout(12.0 * CELL_W, CELL_W);
        assert!(l.label_w <= 12.0 * CELL_W);
        assert!(l.control_w >= 0.0);
        assert!(l.label_w + l.control_x_off - l.label_w <= 12.0 * CELL_W + f32::EPSILON);
    }

    #[test]
    fn typical_content_targets_38_percent() {
        // 60 cells wide: 38% = 22.8 cols, within [MIN, MAX] and within the
        // control-min-cols bound (60 - 2 - 6 = 52 >= 22.8), so it applies untouched.
        let l = compute_row_layout(60.0 * CELL_W, CELL_W);
        let expected_cols = 60.0 * LABEL_FRACTION;
        assert!((l.label_w - expected_cols * CELL_W).abs() < 0.01);
        assert!(l.control_w > 0.0);
    }

    #[test]
    fn control_width_never_negative() {
        for cols in [1.0, 5.0, 10.0, 12.0, 20.0, 60.0, 500.0] {
            let l = compute_row_layout(cols * CELL_W, CELL_W);
            assert!(
                l.control_w >= 0.0,
                "cols={cols} produced negative control_w"
            );
            assert!(l.label_w >= 0.0, "cols={cols} produced negative label_w");
        }
    }

    // ===== list_window =====

    #[test]
    fn short_list_is_shown_entirely() {
        let w = list_window(5, 2, 8);
        assert_eq!((w.first, w.visible), (0, 5));
        assert!(!w.clipped);

        // Exactly max_rows still fits without clipping.
        let w = list_window(8, 7, 8);
        assert_eq!((w.first, w.visible), (0, 8));
        assert!(!w.clipped);

        let w = list_window(0, 0, 8);
        assert_eq!((w.first, w.visible), (0, 0));
        assert!(!w.clipped);
    }

    #[test]
    fn long_list_is_clipped_to_max_rows() {
        let w = list_window(45, 0, 8);
        assert_eq!(w.visible, 8);
        assert!(w.clipped);
    }

    #[test]
    fn selection_near_start_pins_window_to_front() {
        // half = 8 / 2 = 4: selections 0..=4 all keep the window at 0.
        for sel in 0..=4 {
            let w = list_window(45, sel, 8);
            assert_eq!(w.first, 0, "sel={sel}");
        }
    }

    #[test]
    fn selection_in_middle_centers_window() {
        let w = list_window(45, 20, 8);
        assert_eq!(w.first, 16); // 20 - half(4)
    }

    #[test]
    fn selection_near_end_clamps_window_to_back() {
        let w = list_window(45, 44, 8);
        assert_eq!(w.first, 37); // len - max_rows
        let w = list_window(45, 41, 8);
        assert_eq!(w.first, 37);
    }

    #[test]
    fn out_of_bounds_selection_is_treated_as_last() {
        let w = list_window(45, 100, 8);
        assert_eq!(w.first, 37);
    }

    #[test]
    fn selected_item_always_falls_inside_the_window() {
        for len in [1usize, 2, 7, 8, 9, 20, 45, 300] {
            for sel in 0..len {
                let w = list_window(len, sel, 8);
                assert!(
                    w.first <= sel && sel < w.first + w.visible,
                    "len={len} sel={sel} window=({}, {})",
                    w.first,
                    w.visible
                );
            }
        }
    }

    #[test]
    fn zero_max_rows_degenerates_to_empty_window() {
        let w = list_window(45, 3, 0);
        assert_eq!((w.first, w.visible), (0, 0));
    }

    #[test]
    fn block_rows_matches_legacy_layout_when_not_clipped() {
        // Pre-fix code sized the list as `len * 1.2` rows; an unclipped
        // window must occupy exactly the same height.
        let w = list_window(5, 0, 8);
        assert!((w.block_rows() - 5.0 * LIST_ROW_PITCH).abs() < f32::EPSILON);
    }

    #[test]
    fn block_rows_adds_one_indicator_row_when_clipped() {
        let w = list_window(45, 0, 8);
        assert!((w.block_rows() - 9.0 * LIST_ROW_PITCH).abs() < f32::EPSILON);
    }
}
