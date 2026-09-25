//! BSP (Binary Space Partitioning) tree implementation.
//!
//! Manages the pane split layout inside a window using a BSP tree.

use crate::snapshot::{SplitDirSnapshot, SplitNodeSnapshot};

/// Pane split direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SplitDir {
    /// Left/right split (vertical separator).
    Vertical,
    /// Top/bottom split (horizontal separator).
    Horizontal,
}

/// Server-side pane rectangle (grid coordinates).
#[derive(Clone, Debug)]
pub struct PaneRect {
    pub pane_id: u32,
    pub col_off: u16,
    pub row_off: u16,
    pub cols: u16,
    pub rows: u16,
}

/// Result of `remove()`.
pub(super) enum RemoveResult {
    /// This node is the removal target; the caller must replace it with its sibling.
    RemoveSelf,
    /// A descendant was removed successfully.
    Removed,
    /// The target was not found.
    NotFound,
}

/// Node of the BSP split tree.
#[derive(Clone, Debug)]
pub(super) enum SplitNode {
    /// Leaf node (a single pane).
    Pane { pane_id: u32 },
    /// Split node with left/top and right/bottom children.
    Split {
        dir: SplitDir,
        /// Occupancy ratio for the left/top child (0.0..1.0).
        ratio: f32,
        left: Box<SplitNode>,
        right: Box<SplitNode>,
    },
}

/// Minimum extent (columns or rows) either child of a split may end up with when there
/// is genuinely room for both. Below this, `split_extent` gives up on splitting at all
/// and hands the whole available extent to the first child (see its doc comment for the
/// full policy).
const MIN_CHILD_EXTENT: u16 = 1;

/// Split `total` (a column or row count) into `(first, separator, second)` for a
/// left/top vs. right/bottom pair of children, honoring `ratio` for the first child.
///
/// Shared by the live BSP tree (`SplitNode::compute`) and the snapshot-restore path
/// (`tiling::compute_pane_sizes`) — architecture-comparison audit follow-up (2026-09,
/// item #8): the two call sites had independently drifted. `tiling::compute_pane_sizes`
/// still carried the pre-round-4-fix clamp (`.max(1).min(total.saturating_sub(2))`) that
/// could make `first + separator + second` exceed `total` for a tiny parent — the same
/// class of bug already fixed here in `compute()` (round 4, #19) but never ported over.
///
/// Guarantees, for any `total` (including 0):
/// - `first + separator + second == total` exactly (never overflows the parent).
/// - `separator` is 1 when `total > 0`, else 0 (no room for a divider in an empty extent).
/// - When there is room for both children plus the separator (`total >=
///   2 * MIN_CHILD_EXTENT + 1`), both `first` and `second` are at least
///   `MIN_CHILD_EXTENT`.
///
/// Explicit policy for the too-small case (architecture-comparison audit follow-up,
/// item #7 — "no equivalent of tmux's outright refusal or sway's clamp-and-disable
/// fix"): rather than reject the split outright (which would require plumbing the
/// current pane size into every split call site) or letting rounding produce a
/// zero-width *second* child asymmetrically, `total < 2 * MIN_CHILD_EXTENT + 1` hands
/// the entire available extent to `first` and leaves `second` at 0. The rect is still
/// exact and non-overflowing (per the invariant above); a 0-sized pane cannot be typed
/// into, but it also cannot corrupt layout math for its siblings, and resizing the
/// window back up immediately gives it a share of the newly available space again.
pub(super) fn split_extent(total: u16, ratio: f32) -> (u16, u16, u16) {
    let separator = if total == 0 { 0 } else { 1 };
    let available = total - separator;
    let first = if available < 2 * MIN_CHILD_EXTENT {
        available
    } else {
        ((total as f32 * ratio).round() as u16)
            .clamp(MIN_CHILD_EXTENT, available - MIN_CHILD_EXTENT)
    };
    let second = available - first;
    (first, separator, second)
}

impl SplitNode {
    /// Split the specified pane and insert a new pane on the right/bottom side.
    pub(super) fn insert_after(&mut self, target_id: u32, new_id: u32, dir: SplitDir) -> bool {
        match self {
            SplitNode::Pane { pane_id } if *pane_id == target_id => {
                let old = std::mem::replace(self, SplitNode::Pane { pane_id: 0 });
                *self = SplitNode::Split {
                    dir,
                    ratio: 0.5,
                    left: Box::new(old),
                    right: Box::new(SplitNode::Pane { pane_id: new_id }),
                };
                true
            }
            SplitNode::Pane { .. } => false,
            SplitNode::Split { left, right, .. } => {
                left.insert_after(target_id, new_id, dir.clone())
                    || right.insert_after(target_id, new_id, dir)
            }
        }
    }

    /// Recursively compute the rectangle (col_off, row_off, cols, rows) and append it to `out`.
    pub(super) fn compute(
        &self,
        col_off: u16,
        row_off: u16,
        cols: u16,
        rows: u16,
        out: &mut Vec<PaneRect>,
    ) {
        match self {
            SplitNode::Pane { pane_id } => {
                out.push(PaneRect {
                    pane_id: *pane_id,
                    col_off,
                    row_off,
                    cols,
                    rows,
                });
            }
            SplitNode::Split {
                dir,
                ratio,
                left,
                right,
            } => match dir {
                SplitDir::Vertical => {
                    // Left/right split. `split_extent` (audit round 4 #19, plus its
                    // rounding/dedup follow-up in items #5/#7/#8) guarantees
                    // `left_cols + separator + right_cols == cols` exactly, for any
                    // `cols` down to 0.
                    let (left_cols, separator, right_cols) = split_extent(cols, *ratio);
                    left.compute(col_off, row_off, left_cols, rows, out);
                    right.compute(
                        col_off + left_cols + separator,
                        row_off,
                        right_cols,
                        rows,
                        out,
                    );
                }
                SplitDir::Horizontal => {
                    // Top/bottom split — same helper as the vertical case above.
                    let (top_rows, separator, bot_rows) = split_extent(rows, *ratio);
                    left.compute(col_off, row_off, cols, top_rows, out);
                    right.compute(col_off, row_off + top_rows + separator, cols, bot_rows, out);
                }
            },
        }
    }

    /// Remove the specified pane from the BSP tree and promote its sibling.
    /// Returns `Some(self_after_removal)` on success.
    /// `None` indicates that the current node was the removal target itself (caller
    /// must replace it with its sibling).
    pub(super) fn remove(&mut self, target_id: u32) -> RemoveResult {
        match self {
            SplitNode::Pane { pane_id } if *pane_id == target_id => RemoveResult::RemoveSelf,
            SplitNode::Pane { .. } => RemoveResult::NotFound,
            SplitNode::Split { left, right, .. } => {
                match left.remove(target_id) {
                    RemoveResult::RemoveSelf => {
                        // Removed the left child -> promote the right child into this slot.
                        let sibling =
                            std::mem::replace(right.as_mut(), SplitNode::Pane { pane_id: 0 });
                        *self = sibling;
                        RemoveResult::Removed
                    }
                    RemoveResult::Removed => RemoveResult::Removed,
                    RemoveResult::NotFound => match right.remove(target_id) {
                        RemoveResult::RemoveSelf => {
                            // Removed the right child -> promote the left child into this slot.
                            let sibling =
                                std::mem::replace(left.as_mut(), SplitNode::Pane { pane_id: 0 });
                            *self = sibling;
                            RemoveResult::Removed
                        }
                        other => other,
                    },
                }
            }
        }
    }

    /// Adjust by `delta` the ratio of the Split node closest to the focused pane.
    /// `delta > 0` enlarges the focused pane; `delta < 0` shrinks it.
    ///
    /// Architecture-comparison audit follow-up (2026-09, item #6): the previous
    /// implementation checked `left.contains(target_id) || right.contains(target_id)`
    /// at the *current* node before ever recursing. Because a root split's two
    /// children cover the entire tree, that check is trivially true on the very
    /// first call whenever `target_id` exists anywhere in the tree at all — so the
    /// `else` branch (the actual recursive descent) was unreachable dead code, and
    /// every resize silently landed on the outermost (root) split regardless of how
    /// deeply nested the focused pane actually was. Fixed by recursing into the
    /// containing child *first* and only adjusting this node's own ratio when that
    /// recursive call bottoms out (returns `false`, i.e. the child is a `Pane` leaf,
    /// not a further `Split`) — that is the split truly closest to the target pane.
    pub(super) fn adjust_ratio_for(&mut self, target_id: u32, delta: f32) -> bool {
        match self {
            SplitNode::Pane { .. } => false,
            SplitNode::Split {
                ratio, left, right, ..
            } => {
                let in_left = left.contains(target_id);
                let in_right = right.contains(target_id);
                if !in_left && !in_right {
                    return false;
                }
                let handled_deeper = if in_left {
                    left.adjust_ratio_for(target_id, delta)
                } else {
                    right.adjust_ratio_for(target_id, delta)
                };
                if handled_deeper {
                    return true;
                }
                let new_ratio = if in_left {
                    (*ratio + delta).clamp(0.1, 0.9)
                } else {
                    (*ratio - delta).clamp(0.1, 0.9)
                };
                *ratio = new_ratio;
                true
            }
        }
    }

    /// Swap two pane IDs within the BSP tree.
    pub(super) fn swap_ids(&mut self, id_a: u32, id_b: u32) -> bool {
        match self {
            SplitNode::Pane { pane_id } => {
                if *pane_id == id_a {
                    *pane_id = id_b;
                    true
                } else if *pane_id == id_b {
                    *pane_id = id_a;
                    true
                } else {
                    false
                }
            }
            SplitNode::Split { left, right, .. } => {
                left.swap_ids(id_a, id_b) | right.swap_ids(id_a, id_b)
            }
        }
    }

    /// Return the ID of an adjacent pane (sibling of the focused pane).
    #[allow(dead_code)]
    pub(super) fn neighbor_id(&self, target_id: u32) -> Option<u32> {
        match self {
            SplitNode::Pane { .. } => None,
            SplitNode::Split { left, right, .. } => {
                if left.contains(target_id) {
                    right.first_pane_id()
                } else if right.contains(target_id) {
                    left.first_pane_id()
                } else {
                    left.neighbor_id(target_id)
                        .or_else(|| right.neighbor_id(target_id))
                }
            }
        }
    }

    /// Return the first pane ID inside the subtree.
    #[allow(dead_code)]
    pub(super) fn first_pane_id(&self) -> Option<u32> {
        match self {
            SplitNode::Pane { pane_id } => Some(*pane_id),
            SplitNode::Split { left, .. } => left.first_pane_id(),
        }
    }

    /// Check whether the specified pane is contained in this subtree.
    pub(super) fn contains(&self, target_id: u32) -> bool {
        match self {
            SplitNode::Pane { pane_id } => *pane_id == target_id,
            SplitNode::Split { left, right, .. } => {
                left.contains(target_id) || right.contains(target_id)
            }
        }
    }

    /// Convert the BSP tree to a snapshot (CWD values are filled in later by `Window::to_snapshot()`).
    pub(super) fn to_snapshot(&self) -> SplitNodeSnapshot {
        match self {
            SplitNode::Pane { pane_id } => SplitNodeSnapshot::Pane {
                pane_id: *pane_id,
                cwd: None,
            },
            SplitNode::Split {
                dir,
                ratio,
                left,
                right,
            } => SplitNodeSnapshot::Split {
                dir: match dir {
                    SplitDir::Vertical => SplitDirSnapshot::Vertical,
                    SplitDir::Horizontal => SplitDirSnapshot::Horizontal,
                },
                ratio: *ratio,
                left: Box::new(left.to_snapshot()),
                right: Box::new(right.to_snapshot()),
            },
        }
    }

    /// Collect every `pane_id` referenced by a BSP snapshot's leaves, in DFS order, without
    /// deduplicating — used by [`find_duplicate_pane_id`] to detect a corrupt snapshot before any
    /// pane is spawned from it.
    fn collect_pane_ids_in_snapshot(node: &SplitNodeSnapshot, out: &mut Vec<u32>) {
        match node {
            SplitNodeSnapshot::Pane { pane_id, .. } => out.push(*pane_id),
            SplitNodeSnapshot::Split { left, right, .. } => {
                Self::collect_pane_ids_in_snapshot(left, out);
                Self::collect_pane_ids_in_snapshot(right, out);
            }
        }
    }

    /// Return the first `pane_id` that appears more than once among a BSP snapshot's leaves, if
    /// any.
    ///
    /// A restored snapshot's `panes: HashMap<u32, Pane>` is keyed by `pane_id`, so two leaves
    /// sharing an ID would silently collide on insert: the second `Pane::spawn_with_id` call
    /// overwrites the first entry, leaking the first pane's PTY/reader-thread resources while
    /// both BSP leaves keep pointing at the surviving pane (and `pane_order` ends up with a
    /// duplicate entry too). `Window::restore_from_snapshot` calls this before spawning anything
    /// so a corrupt snapshot is rejected outright rather than silently loaded into this state.
    pub(super) fn find_duplicate_pane_id(snap: &SplitNodeSnapshot) -> Option<u32> {
        let mut ids = Vec::new();
        Self::collect_pane_ids_in_snapshot(snap, &mut ids);
        let mut seen = std::collections::HashSet::with_capacity(ids.len());
        ids.into_iter().find(|id| !seen.insert(*id))
    }

    /// Reconstruct a BSP tree from a snapshot.
    pub(super) fn from_snapshot(snap: &SplitNodeSnapshot) -> Self {
        match snap {
            SplitNodeSnapshot::Pane { pane_id, .. } => SplitNode::Pane { pane_id: *pane_id },
            SplitNodeSnapshot::Split {
                dir,
                ratio,
                left,
                right,
            } => SplitNode::Split {
                dir: match dir {
                    SplitDirSnapshot::Vertical => SplitDir::Vertical,
                    SplitDirSnapshot::Horizontal => SplitDir::Horizontal,
                },
                ratio: *ratio,
                left: Box::new(SplitNode::from_snapshot(left)),
                right: Box::new(SplitNode::from_snapshot(right)),
            },
        }
    }
}
