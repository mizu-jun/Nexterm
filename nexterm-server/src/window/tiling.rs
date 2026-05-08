//! タイリングレイアウト計算

use super::bsp::PaneRect;
use crate::snapshot::{SplitDirSnapshot, SplitNodeSnapshot};

/// タイリングレイアウトを計算する（ペインを均等グリッドに自動配置）
///
/// N ペインを ceil(sqrt(N)) 列に均等分配し、各列内でも均等な行高さを割り当てる。
/// 境界線は設けず全スペースをペインに割り当てる。
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

    // 列数: ceil(sqrt(n))
    let ncols = ((n as f64).sqrt().ceil() as usize).max(1).min(n);
    // 各列のペイン数: 最初の (n % ncols) 列に 1 つ多く割り当てる
    let base = n / ncols;
    let extra = n % ncols;

    let mut result = Vec::with_capacity(n);
    let mut pane_idx = 0;

    for col_idx in 0..ncols {
        let count_in_col = base + if col_idx < extra { 1 } else { 0 };
        if count_in_col == 0 {
            continue;
        }

        // 列の X 範囲（整数除算で均等に分配）
        let col_off = (col_idx * total_cols as usize / ncols) as u16;
        let next_col_off = ((col_idx + 1) * total_cols as usize / ncols) as u16;
        let col_width = next_col_off.saturating_sub(col_off).max(1);

        // 列内の各行の Y 範囲
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

/// BSP スナップショットから各ペインのサイズを計算する
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

/// BSP スナップショット内の指定ペインの作業ディレクトリを返す
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
