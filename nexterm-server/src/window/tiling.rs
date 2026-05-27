//! Tiling layout computation.

use super::bsp::PaneRect;
use crate::snapshot::{SplitDirSnapshot, SplitNodeSnapshot};

/// Compute a tiling layout (auto-arrange panes into an even grid).
///
/// `N` panes are distributed evenly across `ceil(sqrt(N))` columns, with even row heights
/// inside each column. No separators are reserved; all space is given to panes.
pub(crate) fn compute_tiling_layouts(
    pane_ids: &[u32],
    total_cols: u16,
    total_rows: u16,
) -> Vec<PaneRect> {
    let n = pane_ids.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![PaneRect {
            pane_id: pane_ids[0],
            col_off: 0,
            row_off: 0,
            cols: total_cols,
            rows: total_rows,
        }];
    }

    // Number of columns: ceil(sqrt(n)).
    let ncols = ((n as f64).sqrt().ceil() as usize).max(1).min(n);
    // Pane count per column: the first (n % ncols) columns get one extra pane.
    let base = n / ncols;
    let extra = n % ncols;

    let mut result = Vec::with_capacity(n);
    let mut pane_idx = 0;

    for col_idx in 0..ncols {
        let count_in_col = base + if col_idx < extra { 1 } else { 0 };
        if count_in_col == 0 {
            continue;
        }

        // X range for this column (distributed evenly with integer division).
        let col_off = (col_idx * total_cols as usize / ncols) as u16;
        let next_col_off = ((col_idx + 1) * total_cols as usize / ncols) as u16;
        let col_width = next_col_off.saturating_sub(col_off).max(1);

        // Y range for each row inside the column.
        for row_idx in 0..count_in_col {
            let row_off = (row_idx * total_rows as usize / count_in_col) as u16;
            let next_row_off = ((row_idx + 1) * total_rows as usize / count_in_col) as u16;
            let row_height = next_row_off.saturating_sub(row_off).max(1);

            result.push(PaneRect {
                pane_id: pane_ids[pane_idx],
                col_off,
                row_off,
                cols: col_width,
                rows: row_height,
            });
            pane_idx += 1;
            if pane_idx >= n {
                break;
            }
        }
    }

    result
}

/// Compute each pane's size from a BSP snapshot.
pub(super) fn compute_pane_sizes(
    node: &SplitNodeSnapshot,
    cols: u16,
    rows: u16,
    out: &mut Vec<(u32, u16, u16)>,
) {
    match node {
        SplitNodeSnapshot::Pane { pane_id, .. } => {
            out.push((*pane_id, cols, rows));
        }
        SplitNodeSnapshot::Split {
            dir,
            ratio,
            left,
            right,
        } => match dir {
            SplitDirSnapshot::Vertical => {
                let lc = ((cols as f32 * ratio) as u16)
                    .max(1)
                    .min(cols.saturating_sub(2));
                let rc = cols.saturating_sub(lc + 1).max(1);
                compute_pane_sizes(left, lc, rows, out);
                compute_pane_sizes(right, rc, rows, out);
            }
            SplitDirSnapshot::Horizontal => {
                let lr = ((rows as f32 * ratio) as u16)
                    .max(1)
                    .min(rows.saturating_sub(2));
                let rr = rows.saturating_sub(lr + 1).max(1);
                compute_pane_sizes(left, cols, lr, out);
                compute_pane_sizes(right, cols, rr, out);
            }
        },
    }
}

/// Return the working directory of the specified pane in a BSP snapshot.
pub(super) fn find_cwd_in_snapshot(
    node: &SplitNodeSnapshot,
    target_id: u32,
) -> Option<std::path::PathBuf> {
    match node {
        SplitNodeSnapshot::Pane { pane_id, cwd } if *pane_id == target_id => cwd.clone(),
        SplitNodeSnapshot::Pane { .. } => None,
        SplitNodeSnapshot::Split { left, right, .. } => {
            find_cwd_in_snapshot(left, target_id).or_else(|| find_cwd_in_snapshot(right, target_id))
        }
    }
}
