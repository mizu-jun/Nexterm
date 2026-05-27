use super::tiling::compute_tiling_layouts;
use super::*;
use crate::snapshot::*;

use proptest::prelude::*;

// ---- BSP layout tests ----

#[test]
fn bsp_vertical_split_layout() {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    tree.insert_after(1, 2, SplitDir::Vertical);
    let mut out = Vec::new();
    tree.compute(0, 0, 80, 24, &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].pane_id, 1);
    assert_eq!(out[0].col_off, 0);
    assert!(out[1].col_off > 0);
    assert_eq!(out[0].cols + 1 + out[1].cols, 80);
}

#[test]
fn bsp_horizontal_split_layout() {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    tree.insert_after(1, 2, SplitDir::Horizontal);
    let mut out = Vec::new();
    tree.compute(0, 0, 80, 24, &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].row_off, 0);
    assert!(out[1].row_off > 0);
    assert_eq!(out[0].rows + 1 + out[1].rows, 24);
}

#[test]
fn bsp_three_pane_layout() {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    tree.insert_after(1, 2, SplitDir::Vertical);
    tree.insert_after(2, 3, SplitDir::Horizontal);
    let mut out = Vec::new();
    tree.compute(0, 0, 80, 24, &mut out);
    assert_eq!(out.len(), 3);
}

#[test]
fn focus_navigation_boundary() {
    let ids = [10u32, 20, 30];
    let pos = ids.iter().position(|&id| id == 30).unwrap();
    let next = ids[(pos + 1) % ids.len()];
    assert_eq!(next, 10);
    let pos = ids.iter().position(|&id| id == 10).unwrap();
    let prev = if pos == 0 { ids.len() - 1 } else { pos - 1 };
    assert_eq!(ids[prev], 30);
}

#[test]
fn bsp_four_pane_layout() {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    tree.insert_after(1, 2, SplitDir::Vertical);
    tree.insert_after(1, 3, SplitDir::Horizontal);
    tree.insert_after(2, 4, SplitDir::Horizontal);
    let mut out = Vec::new();
    tree.compute(0, 0, 80, 24, &mut out);
    assert_eq!(out.len(), 4);
    for p in &out {
        assert!(p.col_off < 80);
        assert!(p.row_off < 24);
        assert!(p.cols > 0);
        assert!(p.rows > 0);
    }
}

#[test]
fn snapshot_conversion_roundtrip() {
    let snap_before = SplitNodeSnapshot::Split {
        dir: SplitDirSnapshot::Vertical,
        ratio: 0.5,
        left: Box::new(SplitNodeSnapshot::Pane {
            pane_id: 1,
            cwd: None,
        }),
        right: Box::new(SplitNodeSnapshot::Pane {
            pane_id: 2,
            cwd: None,
        }),
    };
    let node = bsp::SplitNode::from_snapshot(&snap_before);
    let snap_after = node.to_snapshot();
    let mut sizes_before = Vec::new();
    let mut sizes_after = Vec::new();
    tiling::compute_pane_sizes(&snap_before, 80, 24, &mut sizes_before);
    tiling::compute_pane_sizes(&snap_after, 80, 24, &mut sizes_after);
    assert_eq!(sizes_before, sizes_after);
}

// ---- Tiling layout tests ----

#[test]
fn tiling_single_pane_full_screen() {
    let rects = compute_tiling_layouts(&[1], 80, 24);
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[0].cols, 80);
    assert_eq!(rects[0].rows, 24);
    assert_eq!(rects[0].col_off, 0);
    assert_eq!(rects[0].row_off, 0);
}

#[test]
fn tiling_two_panes_split_horizontally() {
    let rects = compute_tiling_layouts(&[1, 2], 80, 24);
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0].col_off, 0);
    assert!(rects[1].col_off > 0);
    assert_eq!(rects[0].rows, 24);
    assert_eq!(rects[1].rows, 24);
}

#[test]
fn tiling_four_panes_2x2_grid() {
    let rects = compute_tiling_layouts(&[1, 2, 3, 4], 80, 24);
    assert_eq!(rects.len(), 4);
    for r in &rects {
        assert!(r.cols > 0);
        assert!(r.rows > 0);
        assert!(r.col_off < 80);
        assert!(r.row_off < 24);
    }
    assert_eq!(rects[0].col_off, rects[1].col_off);
    assert_eq!(rects[2].col_off, rects[3].col_off);
    assert_ne!(rects[0].col_off, rects[2].col_off);
}

#[test]
fn tiling_five_panes_three_columns() {
    let rects = compute_tiling_layouts(&[1, 2, 3, 4, 5], 80, 24);
    assert_eq!(rects.len(), 5);
    for r in &rects {
        assert!(r.cols > 0);
        assert!(r.rows > 0);
    }
}

#[test]
fn tiling_empty_list_returns_empty() {
    let rects = compute_tiling_layouts(&[], 80, 24);
    assert!(rects.is_empty());
}

// ---- LayoutMode tests ----

#[test]
fn layout_mode_from_str() {
    assert_eq!(LayoutMode::from_str("tiling"), LayoutMode::Tiling);
    assert_eq!(LayoutMode::from_str("bsp"), LayoutMode::Bsp);
    assert_eq!(LayoutMode::from_str("unknown"), LayoutMode::Bsp);
}

#[test]
fn layout_mode_default_is_bsp() {
    let mode: LayoutMode = Default::default();
    assert_eq!(mode, LayoutMode::Bsp);
}

#[test]
fn layout_mode_as_str() {
    assert_eq!(LayoutMode::Bsp.as_str(), "bsp");
    assert_eq!(LayoutMode::Tiling.as_str(), "tiling");
}

// ---- BSP / Tiling property tests (Sprint 4-4) ----

/// Pseudo-op for generating BSP operation sequences.
#[derive(Clone, Debug)]
enum BspOp {
    /// Select an existing pane and split it by inserting a new pane.
    Insert {
        /// Index into the current pane list (normalized by modulo).
        target_idx: usize,
        /// Split direction.
        vertical: bool,
    },
    /// Remove an existing pane (the last remaining pane is preserved).
    Remove {
        /// Index into the current pane list.
        target_idx: usize,
    },
}

fn arb_bsp_op() -> impl Strategy<Value = BspOp> {
    prop_oneof![
        (any::<usize>(), any::<bool>()).prop_map(|(target_idx, vertical)| BspOp::Insert {
            target_idx,
            vertical,
        }),
        any::<usize>().prop_map(|target_idx| BspOp::Remove { target_idx }),
    ]
}

/// Execute an operation sequence and return (BSP tree, list of all pane IDs).
fn run_bsp_ops(ops: &[BspOp]) -> (bsp::SplitNode, Vec<u32>) {
    let mut tree = bsp::SplitNode::Pane { pane_id: 1 };
    let mut active_ids: Vec<u32> = vec![1];
    let mut next_id: u32 = 2;
    for op in ops {
        match op {
            BspOp::Insert {
                target_idx,
                vertical,
            } => {
                if active_ids.is_empty() {
                    continue;
                }
                let target = active_ids[*target_idx % active_ids.len()];
                let new_id = next_id;
                next_id += 1;
                let dir = if *vertical {
                    SplitDir::Vertical
                } else {
                    SplitDir::Horizontal
                };
                if tree.insert_after(target, new_id, dir) {
                    active_ids.push(new_id);
                }
            }
            BspOp::Remove { target_idx } => {
                // Preserve the last remaining pane (invariant: BSP tree never becomes empty).
                if active_ids.len() <= 1 {
                    continue;
                }
                let idx = *target_idx % active_ids.len();
                let target = active_ids[idx];
                // The "root alone -> RemoveSelf" case is guarded by `active_ids.len() <= 1`.
                if matches!(tree.remove(target), bsp::RemoveResult::Removed) {
                    active_ids.remove(idx);
                }
            }
        }
    }
    (tree, active_ids)
}

/// Decide whether rectangles `a` and `b` overlap.
fn rects_overlap(a: &PaneRect, b: &PaneRect) -> bool {
    let a_right = a.col_off + a.cols;
    let a_bottom = a.row_off + a.rows;
    let b_right = b.col_off + b.cols;
    let b_bottom = b.row_off + b.rows;
    !(a_right <= b.col_off
        || b_right <= a.col_off
        || a_bottom <= b.row_off
        || b_bottom <= a.row_off)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_local_rejects: 1024,
        ..ProptestConfig::default()
    })]

    /// `compute()` must never panic for any BSP operation sequence
    /// (even when the window is so small that zero-sized rectangles appear).
    ///
    /// Implementation note: the current BSP can produce a rectangle with `rows = 0`
    /// when the window is extremely small (e.g. `rows < pane count`). The renderer
    /// enforces a minimum size; here we only verify that no panic occurs.
    #[test]
    fn bsp_compute_never_panics(
        ops in proptest::collection::vec(arb_bsp_op(), 0..40),
        cols in 1u16..=300,
        rows in 1u16..=120,
    ) {
        let (tree, _) = run_bsp_ops(&ops);
        let mut layout = Vec::new();
        // Must complete without panicking.
        tree.compute(0, 0, cols, rows, &mut layout);
    }

    /// When the area is sufficient, BSP semantic invariants must hold:
    /// - The computed set of pane_ids matches active_ids.
    /// - Every rectangle fits inside the screen.
    /// - cols > 0 && rows > 0.
    /// - pane_id is unique.
    ///
    /// ## Rationale for the chosen window size
    ///
    /// The current BSP halves space recursively at ratio 0.5, so a right-leaning tree of depth
    /// `d` demands at least `2^d` cells in each direction. The proptest input `target_idx=0`
    /// (its shrunken form) always points at the root and produces a right-leaning tree of depth
    /// `ops.len()`, which requires at least `2^ops.len()` cells.
    ///
    /// Capping operations at 6 gives `2^6 = 64` cells, and we use 1024 with a safety margin.
    /// This validates the property within the current implementation contract.
    /// (If the implementation gains a "minimum size guarantee" later, this bound can be relaxed.
    /// proptest's role here is to pin the contract boundary.)
    #[test]
    fn bsp_invariants_with_sufficient_space(
        ops in proptest::collection::vec(arb_bsp_op(), 0..=6),
    ) {
        let (tree, active_ids) = run_bsp_ops(&ops);
        let cols: u16 = 1024;
        let rows: u16 = 1024;

        let mut layout = Vec::new();
        tree.compute(0, 0, cols, rows, &mut layout);

        // Pane count must match.
        prop_assert_eq!(layout.len(), active_ids.len(),
            "compute() pane count {} != active_ids count {}",
            layout.len(), active_ids.len());

        // Every rectangle must be valid.
        for r in &layout {
            prop_assert!(r.cols > 0, "cols = 0 is invalid: {:?}", r);
            prop_assert!(r.rows > 0, "rows = 0 is invalid: {:?}", r);
            prop_assert!(r.col_off + r.cols <= cols,
                "rectangle exceeds right edge: col_off={} + cols={} > {}",
                r.col_off, r.cols, cols);
            prop_assert!(r.row_off + r.rows <= rows,
                "rectangle exceeds bottom edge: row_off={} + rows={} > {}",
                r.row_off, r.rows, rows);
        }

        // pane_id uniqueness.
        let mut ids: Vec<u32> = layout.iter().map(|r| r.pane_id).collect();
        ids.sort();
        let dup_count = ids.windows(2).filter(|w| w[0] == w[1]).count();
        prop_assert_eq!(dup_count, 0, "duplicate pane_id: {:?}", ids);

        // The pane_id sets of active_ids and layout must match.
        let mut expected = active_ids.clone();
        expected.sort();
        prop_assert_eq!(ids, expected);
    }

    /// When the area is sufficient, BSP rectangles must not overlap each other
    /// (a separator row/column keeps them apart).
    /// See `bsp_invariants_with_sufficient_space` for the window-size rationale.
    #[test]
    fn bsp_rects_do_not_overlap(
        ops in proptest::collection::vec(arb_bsp_op(), 0..=6),
    ) {
        let (tree, _active_ids) = run_bsp_ops(&ops);
        let cols: u16 = 1024;
        let rows: u16 = 1024;

        let mut layout = Vec::new();
        tree.compute(0, 0, cols, rows, &mut layout);

        for i in 0..layout.len() {
            for j in (i + 1)..layout.len() {
                prop_assert!(!rects_overlap(&layout[i], &layout[j]),
                    "rectangles {:?} and {:?} overlap", layout[i], layout[j]);
            }
        }
    }

    /// A BSP tree snapshot round-trip must preserve pane IDs and rectangles.
    /// See `bsp_invariants_with_sufficient_space` for the window-size rationale.
    #[test]
    fn bsp_snapshot_roundtrip(
        ops in proptest::collection::vec(arb_bsp_op(), 0..=6),
    ) {
        let (tree, _active_ids) = run_bsp_ops(&ops);
        let cols: u16 = 1024;
        let rows: u16 = 1024;
        let snap = tree.to_snapshot();
        let rebuilt = bsp::SplitNode::from_snapshot(&snap);

        let mut layout_a = Vec::new();
        let mut layout_b = Vec::new();
        tree.compute(0, 0, cols, rows, &mut layout_a);
        rebuilt.compute(0, 0, cols, rows, &mut layout_b);

        prop_assert_eq!(layout_a.len(), layout_b.len());
        // Compare after sorting by pane_id (order differences are allowed).
        let mut a: Vec<(u32, u16, u16, u16, u16)> = layout_a.iter()
            .map(|r| (r.pane_id, r.col_off, r.row_off, r.cols, r.rows)).collect();
        let mut b: Vec<(u32, u16, u16, u16, u16)> = layout_b.iter()
            .map(|r| (r.pane_id, r.col_off, r.row_off, r.cols, r.rows)).collect();
        a.sort();
        b.sort();
        prop_assert_eq!(a, b);
    }

    /// Tiling layout invariants:
    /// - pane count equals the number of input IDs;
    /// - every rectangle fits inside the screen;
    /// - cols > 0 and rows > 0 for every rectangle (given sufficient area);
    /// - pane_ids match the input.
    #[test]
    fn tiling_invariants_hold(
        ids in proptest::collection::vec(1u32..=u32::MAX, 1..12),
    ) {
        // Deduplicate input IDs (compute_tiling_layouts does not expect duplicates).
        let mut unique_ids = ids.clone();
        unique_ids.sort();
        unique_ids.dedup();

        let n = unique_ids.len() as u16;
        // Tiling arranges panes in a sqrt(N) x sqrt(N) grid, so each direction needs at least
        // sqrt(N) * 2 cells.
        let cols = (n * 4).max(20);
        let rows = (n * 4).max(20);

        let layout = compute_tiling_layouts(&unique_ids, cols, rows);
        prop_assert_eq!(layout.len(), unique_ids.len());

        for r in &layout {
            prop_assert!(r.cols > 0, "cols = 0 is invalid: {:?}", r);
            prop_assert!(r.rows > 0, "rows = 0 is invalid: {:?}", r);
            prop_assert!(r.col_off + r.cols <= cols,
                "tiling exceeds right edge: {:?} window={}", r, cols);
            prop_assert!(r.row_off + r.rows <= rows,
                "tiling exceeds bottom edge: {:?} window={}", r, rows);
        }

        // The set of pane_ids must equal the input.
        let mut layout_ids: Vec<u32> = layout.iter().map(|r| r.pane_id).collect();
        layout_ids.sort();
        prop_assert_eq!(layout_ids, unique_ids);
    }

    /// `compute_tiling_layouts` must never panic (for any cols/rows).
    #[test]
    fn tiling_compute_never_panics(
        ids in proptest::collection::vec(1u32..=u32::MAX, 0..16),
        cols in 0u16..=200,
        rows in 0u16..=80,
    ) {
        let _ = compute_tiling_layouts(&ids, cols, rows);
    }
}

// ---- compute_reordered (Sprint 5-7 / Phase 2-3) tests ----

use super::compute_reordered;
use std::collections::HashSet;

fn known_set(ids: &[u32]) -> HashSet<u32> {
    ids.iter().copied().collect()
}

#[test]
fn reorder_full_permutation_applied() {
    let current = vec![1, 2, 3, 4];
    let known = known_set(&current);
    let requested = vec![3, 1, 4, 2];
    let next = compute_reordered(&current, &requested, &known);
    assert_eq!(next, vec![3, 1, 4, 2]);
}

#[test]
fn reorder_unspecified_ids_kept_in_original_order_at_end() {
    let current = vec![10, 20, 30, 40];
    let known = known_set(&current);
    // Only 30 and 10 are specified; the remaining 20 and 40 retain their relative order at the end.
    let requested = vec![30, 10];
    let next = compute_reordered(&current, &requested, &known);
    assert_eq!(next, vec![30, 10, 20, 40]);
}

#[test]
fn reorder_unknown_ids_are_ignored() {
    let current = vec![1, 2, 3];
    let known = known_set(&current);
    // 99 is unknown -> ignored. Only 1 and 3 are picked, with 2 appended.
    let requested = vec![99, 3, 99, 1];
    let next = compute_reordered(&current, &requested, &known);
    assert_eq!(next, vec![3, 1, 2]);
}

#[test]
fn reorder_duplicates_take_first_occurrence() {
    let current = vec![1, 2, 3];
    let known = known_set(&current);
    let requested = vec![2, 2, 1, 3, 1];
    let next = compute_reordered(&current, &requested, &known);
    assert_eq!(next, vec![2, 1, 3]);
}

#[test]
fn reorder_empty_request_preserves_original_order() {
    let current = vec![5, 6, 7];
    let known = known_set(&current);
    let next = compute_reordered(&current, &[], &known);
    assert_eq!(next, vec![5, 6, 7]);
}

#[test]
fn reorder_known_ids_missing_from_current_appended_in_ascending_order() {
    // Defensive case: known panes might be missing from `pane_order` due to a bug.
    let current = vec![1, 3];
    let mut known = known_set(&current);
    known.insert(5);
    known.insert(2);
    let requested = vec![3, 1];
    let next = compute_reordered(&current, &requested, &known);
    // 3, 1 -> append 5, 2 (missing from current) in ascending order at the end.
    assert_eq!(next, vec![3, 1, 2, 5]);
}

// ---- compute_insert_position tests (Sprint 5-8 Phase 4-4 / Step A-1) ----

#[test]
fn insert_position_none_inserts_after_focused() {
    // Focused index 1 -> insert at +1 = 2.
    assert_eq!(compute_insert_position(5, None, Some(1)), 2);
}

#[test]
fn insert_position_none_with_focused_at_end_appends() {
    // Focused at the end (index 2, len=3) -> +1 reaches len, so append.
    assert_eq!(compute_insert_position(3, None, Some(2)), 3);
}

#[test]
fn insert_position_none_without_focused_appends() {
    // No focused index found -> append.
    assert_eq!(compute_insert_position(3, None, None), 3);
}

#[test]
fn insert_position_some_zero_inserts_at_head() {
    // Position 0 -> insert at head.
    assert_eq!(compute_insert_position(3, Some(0), Some(1)), 0);
}

#[test]
fn insert_position_some_middle_uses_specified_position() {
    // Position 2 -> use as-is.
    assert_eq!(compute_insert_position(5, Some(2), Some(0)), 2);
}

#[test]
fn insert_position_some_out_of_range_clamps_to_end() {
    // Position 99 (len=3) -> clamped to len, i.e. append.
    assert_eq!(compute_insert_position(3, Some(99), None), 3);
}

#[test]
fn insert_position_empty_list_always_zero() {
    // Inserting into an empty list always returns index 0.
    assert_eq!(compute_insert_position(0, None, None), 0);
    assert_eq!(compute_insert_position(0, Some(0), None), 0);
    assert_eq!(compute_insert_position(0, Some(5), None), 0);
}
