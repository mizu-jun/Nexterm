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
                    // Left/right split (one column reserved for the separator).
                    let left_cols = ((cols as f32 * ratio) as u16)
                        .max(1)
                        .min(cols.saturating_sub(2));
                    let right_cols = cols.saturating_sub(left_cols + 1).max(1);
                    left.compute(col_off, row_off, left_cols, rows, out);
                    right.compute(col_off + left_cols + 1, row_off, right_cols, rows, out);
                }
                SplitDir::Horizontal => {
                    // Top/bottom split (one row reserved for the separator).
                    let top_rows = ((rows as f32 * ratio) as u16)
                        .max(1)
                        .min(rows.saturating_sub(2));
                    let bot_rows = rows.saturating_sub(top_rows + 1).max(1);
                    left.compute(col_off, row_off, cols, top_rows, out);
                    right.compute(col_off, row_off + top_rows + 1, cols, bot_rows, out);
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
    pub(super) fn adjust_ratio_for(&mut self, target_id: u32, delta: f32) -> bool {
        match self {
            SplitNode::Pane { .. } => false,
            SplitNode::Split {
                ratio, left, right, ..
            } => {
                let in_left = left.contains(target_id);
                let in_right = right.contains(target_id);
                if in_left || in_right {
                    let new_ratio = if in_left {
                        (*ratio + delta).clamp(0.1, 0.9)
                    } else {
                        (*ratio - delta).clamp(0.1, 0.9)
                    };
                    *ratio = new_ratio;
                    true
                } else {
                    left.adjust_ratio_for(target_id, delta)
                        || right.adjust_ratio_for(target_id, delta)
                }
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
