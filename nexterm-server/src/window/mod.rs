//! Window — a collection of panes (tab equivalent).
//!
//! Pane split layout is managed with a BSP (Binary Space Partition) tree.
//! Each pane's (col_offset, row_offset, cols, rows) is derived by recursive computation on the tree.

mod bsp;
mod floating;
mod tiling;

pub use bsp::{PaneRect, SplitDir};
pub use floating::FloatRect;

use std::collections::HashMap;

use anyhow::Result;
use tokio::sync::broadcast;

use nexterm_proto::{PaneLayout, ServerToClient};

use crate::pane::Pane;
use crate::serial::SerialPane;
use crate::snapshot::{SplitNodeSnapshot, WindowSnapshot};
use bsp::SplitNode;
use tiling::{compute_pane_sizes, compute_tiling_layouts, find_cwd_in_snapshot};

/// Insertion-position decision logic for `insert_pane_at` (Sprint 5-8 Phase 4-4, split out as a
/// pure function for testability).
///
/// Returns the insertion index into `pane_order`.
///
/// - If `requested_position` is `Some(p)`: `p.min(current_len)` (out-of-range values append to the end).
/// - If `requested_position` is `None`: `focused_index + 1` (immediately after the focused pane).
///   When `focused_index` is `None` or `focused_index + 1 >= current_len`, it falls back to appending.
///
/// This behavior is fully compatible with the existing `insert_pane` (which appends and inserts
/// right after the focused pane).
pub(crate) fn compute_insert_position(
    current_len: usize,
    requested_position: Option<usize>,
    focused_index: Option<usize>,
) -> usize {
    match requested_position {
        Some(pos) => pos.min(current_len),
        None => match focused_index {
            Some(idx) if idx + 1 < current_len => idx + 1,
            _ => current_len,
        },
    }
}

/// Core logic of `reorder_panes` (split out as a pure function for testability).
///
/// Performs a stable "requested-first + fill the rest" reordering based on `current`:
/// 1. Walk `requested` in order and pick up every ID present in `known` (deduplicated).
/// 2. Then walk `current` and append any remaining known IDs that were not picked yet (preserving
///    their original relative order).
/// 3. Any known IDs that are still missing (also absent from `current`) are appended in ascending order.
pub(crate) fn compute_reordered(
    current: &[u32],
    requested: &[u32],
    known: &std::collections::HashSet<u32>,
) -> Vec<u32> {
    let mut next: Vec<u32> = Vec::with_capacity(known.len());
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for &id in requested {
        if known.contains(&id) && seen.insert(id) {
            next.push(id);
        }
    }
    for &id in current {
        if known.contains(&id) && seen.insert(id) {
            next.push(id);
        }
    }
    let mut leftover: Vec<u32> = known
        .iter()
        .copied()
        .filter(|id| !seen.contains(id))
        .collect();
    leftover.sort();
    next.extend(leftover);
    next
}

// ---- Layout mode ----

/// Window layout mode.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum LayoutMode {
    /// BSP (binary space partitioning) — manual splits with preserved ratios (default).
    #[default]
    Bsp,
    /// Tiling — automatically arrange panes into an even grid.
    Tiling,
}

impl LayoutMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "tiling" => LayoutMode::Tiling,
            _ => LayoutMode::Bsp,
        }
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            LayoutMode::Bsp => "bsp",
            LayoutMode::Tiling => "tiling",
        }
    }
}

// ---- Window ----

/// Window (container for panes).
pub struct Window {
    pub id: u32,
    pub name: String,
    /// Map of PTY panes (ID -> Pane).
    panes: HashMap<u32, Pane>,
    /// Map of serial port panes (ID -> SerialPane).
    serial_panes: HashMap<u32, SerialPane>,
    /// Currently focused pane ID.
    focused_pane_id: u32,
    /// BSP split tree.
    layout: SplitNode,
    /// Whether zoom mode is active (the focused pane fills the entire window).
    zoomed: bool,
    /// Layout mode (Bsp / Tiling).
    pub layout_mode: LayoutMode,
    /// Floating panes (panes overlaid on top of the regular layout).
    floating_panes: HashMap<u32, (Pane, FloatRect)>,
    /// Tab display order (Sprint 5-7 / Phase 2-3).
    ///
    /// `panes`/`serial_panes` are `HashMap`s without a stable key order, so the logical order
    /// shown in the tab bar is tracked separately in a `Vec<u32>`. New panes are pushed to the
    /// end, deletion uses `retain`, and user-initiated drag-and-drop reorder is applied via
    /// [`Window::reorder_panes`].
    ///
    /// The `panes` array order in `LayoutChanged` messages is sorted according to this order.
    pane_order: Vec<u32>,
}

impl Window {
    /// Construct a window with a single initial pane.
    pub fn new(
        id: u32,
        name: String,
        cols: u16,
        rows: u16,
        tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
    ) -> Result<Self> {
        let pane = Pane::spawn(cols, rows, tx, shell, args)?;
        let focused_pane_id = pane.id;
        let layout = SplitNode::Pane {
            pane_id: focused_pane_id,
        };
        let mut panes = HashMap::new();
        panes.insert(pane.id, pane);

        Ok(Self {
            id,
            name,
            panes,
            serial_panes: HashMap::new(),
            focused_pane_id,
            layout,
            zoomed: false,
            layout_mode: LayoutMode::Bsp,
            floating_panes: HashMap::new(),
            pane_order: vec![focused_pane_id],
        })
    }

    /// Construct a window from an existing pane (used by break-pane).
    pub fn new_with_pane(id: u32, name: String, pane: Pane) -> Result<Self> {
        let focused_pane_id = pane.id;
        let layout = SplitNode::Pane {
            pane_id: focused_pane_id,
        };
        let mut panes = HashMap::new();
        panes.insert(pane.id, pane);
        Ok(Self {
            id,
            name,
            panes,
            serial_panes: HashMap::new(),
            focused_pane_id,
            layout,
            zoomed: false,
            layout_mode: LayoutMode::Bsp,
            floating_panes: HashMap::new(),
            pane_order: vec![focused_pane_id],
        })
    }

    /// Return the focused pane ID.
    pub fn focused_pane_id(&self) -> u32 {
        self.focused_pane_id
    }

    /// Return the list of pane IDs (PTY + serial).
    pub fn pane_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.panes.keys().copied().collect();
        ids.extend(self.serial_panes.keys().copied());
        ids
    }

    /// Add a new pane by splitting the BSP tree.
    ///
    /// `total_cols`/`total_rows` represent the full window size. Computes the per-pane size from the
    /// new layout before spawning the pane.
    pub fn add_pane(
        &mut self,
        total_cols: u16,
        total_rows: u16,
        tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
        dir: SplitDir,
    ) -> Result<u32> {
        // 1. Reserve a new ID up front and insert it into the tree.
        let new_id = crate::pane::new_pane_id();
        self.layout.insert_after(self.focused_pane_id, new_id, dir);

        // 2. Recompute the layout and look up the new pane's size.
        let layouts = self.compute_layouts(total_cols, total_rows);
        let new_rect = layouts
            .iter()
            .find(|r| r.pane_id == new_id)
            .cloned()
            .unwrap_or(PaneRect {
                pane_id: new_id,
                col_off: 0,
                row_off: 0,
                cols: total_cols,
                rows: total_rows,
            });

        // 3. Spawn the new pane with the computed size.
        // Sprint 5-2 / B2: inherit the parent pane's CWD (OSC 7 first, fall back to /proc/{pid}/cwd).
        let parent_cwd = self
            .panes
            .get(&self.focused_pane_id)
            .and_then(|p| p.osc7_cwd().or_else(|| p.working_dir()));
        let pane = match parent_cwd {
            Some(ref cwd) => {
                Pane::spawn_with_cwd(new_id, new_rect.cols, new_rect.rows, tx, shell, args, cwd)?
            }
            None => Pane::spawn_with_id(new_id, new_rect.cols, new_rect.rows, tx, shell, args)?,
        };
        self.panes.insert(new_id, pane);
        self.focused_pane_id = new_id;
        // Tab order: append at the end (new tab appears to the right of existing tabs).
        self.pane_order.push(new_id);

        // 4. Resize the existing panes to their new sizes.
        for rect in &layouts {
            if rect.pane_id != new_id
                && let Some(p) = self.panes.get_mut(&rect.pane_id)
            {
                let _ = p.resize_pty(rect.cols, rect.rows);
            }
        }

        Ok(new_id)
    }

    /// Compute layouts for all panes.
    pub fn compute_layouts(&self, cols: u16, rows: u16) -> Vec<PaneRect> {
        match self.layout_mode {
            LayoutMode::Bsp => {
                let mut out = Vec::new();
                self.layout.compute(0, 0, cols, rows, &mut out);
                out
            }
            LayoutMode::Tiling => {
                // Collect pane IDs from the BSP tree in insertion order (sorted).
                let mut pane_ids: Vec<u32> = self.panes.keys().copied().collect();
                pane_ids.extend(self.serial_panes.keys().copied());
                pane_ids.sort();
                compute_tiling_layouts(&pane_ids, cols, rows)
            }
        }
    }

    /// Change the layout mode and resize every pane to the new layout.
    pub fn set_layout_mode(&mut self, mode: LayoutMode, cols: u16, rows: u16) {
        self.layout_mode = mode;
        self.resize_all_panes(cols, rows);
    }

    // ---- Floating panes ----

    /// Create a floating pane and place it in the center of the window.
    ///
    /// Returns `(pane_id, FloatRect)` — used by the IPC layer to emit `FloatingPaneOpened`.
    pub fn open_floating_pane(
        &mut self,
        total_cols: u16,
        total_rows: u16,
        tx: broadcast::Sender<ServerToClient>,
        shell: &str,
        args: &[String],
    ) -> Result<(u32, FloatRect)> {
        // Default size: 60% x 70% of the window, centered.
        let fp_cols = (total_cols as f32 * 0.6) as u16;
        let fp_rows = (total_rows as f32 * 0.7) as u16;
        let col_off = (total_cols.saturating_sub(fp_cols)) / 2;
        let row_off = (total_rows.saturating_sub(fp_rows)) / 2;

        // Sprint 5-2 / B2: inherit the focused pane's CWD.
        let parent_cwd = self
            .panes
            .get(&self.focused_pane_id)
            .and_then(|p| p.osc7_cwd().or_else(|| p.working_dir()));
        let pane_id = crate::pane::new_pane_id();
        let pane = match parent_cwd {
            Some(ref cwd) => Pane::spawn_with_cwd(
                pane_id,
                fp_cols.max(10),
                fp_rows.max(5),
                tx,
                shell,
                args,
                cwd,
            )?,
            None => Pane::spawn_with_id(pane_id, fp_cols.max(10), fp_rows.max(5), tx, shell, args)?,
        };
        let rect = FloatRect {
            col_off,
            row_off,
            cols: fp_cols.max(10),
            rows: fp_rows.max(5),
        };
        self.floating_panes.insert(pane_id, (pane, rect.clone()));
        Ok((pane_id, rect))
    }

    /// Close a floating pane.
    pub fn close_floating_pane(&mut self, pane_id: u32) -> bool {
        self.floating_panes.remove(&pane_id).is_some()
    }

    /// Move a floating pane.
    pub fn move_floating_pane(
        &mut self,
        pane_id: u32,
        col_off: u16,
        row_off: u16,
    ) -> Option<FloatRect> {
        if let Some((_, rect)) = self.floating_panes.get_mut(&pane_id) {
            rect.col_off = col_off;
            rect.row_off = row_off;
            Some(rect.clone())
        } else {
            None
        }
    }

    /// Resize a floating pane.
    pub fn resize_floating_pane(
        &mut self,
        pane_id: u32,
        cols: u16,
        rows: u16,
    ) -> Option<FloatRect> {
        if let Some((pane, rect)) = self.floating_panes.get_mut(&pane_id) {
            rect.cols = cols.max(10);
            rect.rows = rows.max(5);
            let _ = pane.resize_pty(rect.cols, rect.rows);
            Some(rect.clone())
        } else {
            None
        }
    }

    /// Write input to a floating pane.
    #[allow(dead_code)]
    pub fn write_to_floating(&self, pane_id: u32, data: &[u8]) -> Result<()> {
        if let Some((pane, _)) = self.floating_panes.get(&pane_id) {
            pane.write_input(data)?;
        }
        Ok(())
    }

    /// Return the list of floating panes (for display).
    #[allow(dead_code)]
    pub fn floating_pane_rects(&self) -> Vec<(u32, FloatRect)> {
        self.floating_panes
            .iter()
            .map(|(&id, (_, rect))| (id, rect.clone()))
            .collect()
    }

    /// Check whether a floating pane with the given id exists.
    #[allow(dead_code)]
    pub fn has_floating_pane(&self, pane_id: u32) -> bool {
        self.floating_panes.contains_key(&pane_id)
    }

    /// Build a `LayoutChanged` message (for IPC).
    ///
    /// The order of the `panes` array follows the logical tab order in `pane_order` (Sprint 5-7 /
    /// Phase 2-3). It is managed independently from the physical layout (BSP recursion order), so
    /// a drag-and-drop reorder does not change any pane's on-screen position or size.
    pub fn layout_changed_msg(&self, cols: u16, rows: u16) -> ServerToClient {
        // While zoomed, return only the focused pane sized to the entire window.
        if self.zoomed {
            return ServerToClient::LayoutChanged {
                panes: vec![PaneLayout {
                    pane_id: self.focused_pane_id,
                    col_offset: 0,
                    row_offset: 0,
                    cols,
                    rows,
                    is_focused: true,
                }],
                focused_pane_id: self.focused_pane_id,
            };
        }
        let rects = self.compute_layouts(cols, rows);
        // Make a lookup map keyed by pane_id so we can iterate in pane_order.
        let rect_by_id: HashMap<u32, &PaneRect> = rects.iter().map(|r| (r.pane_id, r)).collect();
        let mut ordered: Vec<PaneLayout> = self
            .pane_order
            .iter()
            .filter_map(|id| {
                rect_by_id.get(id).map(|r| PaneLayout {
                    pane_id: r.pane_id,
                    col_offset: r.col_off,
                    row_offset: r.row_off,
                    cols: r.cols,
                    rows: r.rows,
                    is_focused: r.pane_id == self.focused_pane_id,
                })
            })
            .collect();
        // Fallback: append any panes not registered in pane_order (defensive against bugs).
        for rect in &rects {
            if !self.pane_order.contains(&rect.pane_id) {
                ordered.push(PaneLayout {
                    pane_id: rect.pane_id,
                    col_offset: rect.col_off,
                    row_offset: rect.row_off,
                    cols: rect.cols,
                    rows: rect.rows,
                    is_focused: rect.pane_id == self.focused_pane_id,
                });
            }
        }
        ServerToClient::LayoutChanged {
            panes: ordered,
            focused_pane_id: self.focused_pane_id,
        }
    }

    /// Replace the tab display order with a new permutation (Sprint 5-7 / Phase 2-3).
    ///
    /// `new_order` is safe even when it does not contain every currently known pane or contains
    /// nonexistent pane IDs:
    /// - Unknown IDs are ignored.
    /// - Known IDs not mentioned in the request are appended (preserving their relative order in
    ///   the existing `pane_order`).
    ///
    /// Returns `true` if the order actually changed, `false` otherwise.
    pub fn reorder_panes(&mut self, new_order: Vec<u32>) -> bool {
        let known: std::collections::HashSet<u32> = self
            .panes
            .keys()
            .copied()
            .chain(self.serial_panes.keys().copied())
            .collect();
        let next = compute_reordered(&self.pane_order, &new_order, &known);
        if next == self.pane_order {
            return false;
        }
        self.pane_order = next;
        true
    }

    /// Return the current tab display order (for tests and debugging).
    #[allow(dead_code)]
    pub fn pane_order(&self) -> &[u32] {
        &self.pane_order
    }

    /// Toggle zoom on the focused pane.
    /// While zoomed, the focused pane is expanded to the full window and other panes are hidden.
    /// Returns the post-toggle state (`true` = zoomed).
    pub fn toggle_zoom(&mut self, cols: u16, rows: u16) -> bool {
        self.zoomed = !self.zoomed;
        // While zoomed, resize the focused pane to the window size.
        if self.zoomed {
            if let Some(pane) = self.panes.get_mut(&self.focused_pane_id) {
                let _ = pane.resize_pty(cols, rows);
            }
        } else {
            // When unzooming, return every pane to the regular layout.
            self.resize_all_panes(cols, rows);
        }
        self.zoomed
    }

    /// Return the zoom state.
    #[allow(dead_code)]
    pub fn is_zoomed(&self) -> bool {
        self.zoomed
    }

    /// Move focus to the specified pane (e.g. on click).
    pub fn set_focused_pane(&mut self, pane_id: u32) {
        if self.panes.contains_key(&pane_id) {
            self.focused_pane_id = pane_id;
        }
    }

    /// Move focus to the next pane.
    pub fn focus_next(&mut self) {
        let ids: Vec<u32> = {
            let mut v: Vec<u32> = self.panes.keys().copied().collect();
            v.sort();
            v
        };
        if let Some(pos) = ids.iter().position(|&id| id == self.focused_pane_id) {
            self.focused_pane_id = ids[(pos + 1) % ids.len()];
        }
    }

    /// Move focus to the previous pane.
    pub fn focus_prev(&mut self) {
        let ids: Vec<u32> = {
            let mut v: Vec<u32> = self.panes.keys().copied().collect();
            v.sort();
            v
        };
        if let Some(pos) = ids.iter().position(|&id| id == self.focused_pane_id) {
            let prev = if pos == 0 { ids.len() - 1 } else { pos - 1 };
            self.focused_pane_id = ids[prev];
        }
    }

    /// Return a reference to the pane with the given id.
    pub fn pane(&self, id: u32) -> Option<&Pane> {
        self.panes.get(&id)
    }

    /// Remove the focused pane from the BSP tree.
    ///
    /// Returns `Err` if only one pane remains (the last pane cannot be removed).
    /// On success, focus shifts to the adjacent pane.
    pub fn remove_focused_pane(&mut self, cols: u16, rows: u16) -> Result<u32> {
        if self.panes.len() <= 1 {
            return Err(anyhow::anyhow!("cannot remove the last remaining pane"));
        }
        let target_id = self.focused_pane_id;

        // Remove from the BSP tree.
        self.layout.remove(target_id);

        // Remove from the pane map (PTY or serial port).
        self.panes.remove(&target_id);
        self.serial_panes.remove(&target_id);
        // Also drop from the tab order (Sprint 5-7 / Phase 2-3).
        self.pane_order.retain(|&id| id != target_id);

        // Move focus to one of the remaining panes (pick the smallest ID).
        let next_id = self
            .panes
            .keys()
            .copied()
            .min()
            .expect("panes guarded by len > 1, so at least one must remain");
        self.focused_pane_id = next_id;

        // Resize the remaining panes.
        let layouts = self.compute_layouts(cols, rows);
        for rect in &layouts {
            if let Some(pane) = self.panes.get_mut(&rect.pane_id) {
                let _ = pane.resize_pty(rect.cols, rect.rows);
            }
        }

        Ok(target_id)
    }

    /// Adjust the ratio of the split closest to the focused pane.
    pub fn adjust_split_ratio(&mut self, delta: f32, cols: u16, rows: u16) {
        if self.layout.adjust_ratio_for(self.focused_pane_id, delta) {
            let layouts = self.compute_layouts(cols, rows);
            for rect in &layouts {
                if let Some(pane) = self.panes.get_mut(&rect.pane_id) {
                    let _ = pane.resize_pty(rect.cols, rect.rows);
                }
            }
        }
    }

    /// Swap the focused pane with the specified pane in the BSP tree.
    pub fn swap_focused_with(&mut self, target_pane_id: u32) {
        let focused = self.focused_pane_id;
        if focused != target_pane_id {
            self.layout.swap_ids(focused, target_pane_id);
        }
    }

    /// Swap the focused pane with its neighbor in the "next" direction.
    #[allow(dead_code)]
    pub fn swap_with_next(&mut self) {
        let focused = self.focused_pane_id;
        let ids: Vec<u32> = {
            let mut v: Vec<u32> = self.panes.keys().copied().collect();
            v.sort();
            v
        };
        if let Some(pos) = ids.iter().position(|&id| id == focused) {
            let next_id = ids[(pos + 1) % ids.len()];
            self.layout.swap_ids(focused, next_id);
        }
    }

    /// Swap the focused pane with its neighbor in the "prev" direction.
    #[allow(dead_code)]
    pub fn swap_with_prev(&mut self) {
        let focused = self.focused_pane_id;
        let ids: Vec<u32> = {
            let mut v: Vec<u32> = self.panes.keys().copied().collect();
            v.sort();
            v
        };
        if let Some(pos) = ids.iter().position(|&id| id == focused) {
            let prev_id = if pos == 0 {
                ids[ids.len() - 1]
            } else {
                ids[pos - 1]
            };
            self.layout.swap_ids(focused, prev_id);
        }
    }

    /// Take the focused pane out of the pane map (used by break-pane / join-pane).
    ///
    /// Returns `None` if only one pane remains (the last pane cannot be taken).
    /// Also removes the pane from the BSP tree and moves focus to an adjacent pane.
    pub fn take_focused_pane(&mut self, cols: u16, rows: u16) -> Option<Pane> {
        if self.panes.len() <= 1 {
            return None;
        }
        let target_id = self.focused_pane_id;
        // Remove from the BSP tree.
        self.layout.remove(target_id);
        // Serial panes cannot be broken out (PTY only).
        if self.serial_panes.contains_key(&target_id) {
            return None;
        }
        // Take out of the pane map.
        let pane = self.panes.remove(&target_id)?;
        // Drop from the tab order (Sprint 5-7 / Phase 2-3).
        self.pane_order.retain(|&id| id != target_id);
        // Move focus to the smallest remaining ID.
        let next_id = self
            .panes
            .keys()
            .copied()
            .min()
            .expect("panes guarded by len > 1, so at least one must remain");
        self.focused_pane_id = next_id;
        self.zoomed = false;
        // Resize the remaining panes.
        let layouts = self.compute_layouts(cols, rows);
        for rect in &layouts {
            if let Some(p) = self.panes.get_mut(&rect.pane_id) {
                let _ = p.resize_pty(rect.cols, rect.rows);
            }
        }
        Some(pane)
    }

    /// Consume this window and take the only PTY pane remaining (Sprint 5-8 Phase 4-4).
    ///
    /// Used in the "drag the last pane out of the tab bar and move it into a new window" scenario,
    /// when the source window itself must be removed. Because `take_pane_by_id` blocks at
    /// `pane_count() <= 1`, callers should pull the window out via `Session::windows.remove(&id)`
    /// first and then call this API.
    ///
    /// Returns `None` in the following cases:
    /// - No PTY panes exist (`panes` is empty).
    /// - More than one PTY pane remains (wrong call order).
    /// - Serial panes are present (only PTY pane extraction is supported).
    pub fn into_single_pane(self) -> Option<Pane> {
        if !self.serial_panes.is_empty() {
            return None;
        }
        if self.panes.len() != 1 {
            return None;
        }
        self.panes.into_values().next()
    }

    /// Take the pane with the given `pane_id` out of this window (Sprint 5-8 Phase 4-3,
    /// for move-pane-to-window).
    ///
    /// A general-purpose variant of `take_focused_pane`. Takes an arbitrary `pane_id` as argument.
    /// Behavior matches `take_focused_pane`:
    /// - Returns `None` when only one pane remains (the last pane cannot be taken).
    /// - Serial panes cannot be taken (PTY only).
    /// - After removal, focus moves to the smallest remaining ID, and remaining panes are resized.
    ///
    /// Returns `None` in the following cases:
    /// - The specified `pane_id` does not exist.
    /// - `pane_id` is a serial pane.
    /// - This window has only one pane total (removing it would leave the window empty).
    pub fn take_pane_by_id(&mut self, pane_id: u32, cols: u16, rows: u16) -> Option<Pane> {
        if self.panes.len() <= 1 {
            return None;
        }
        if !self.panes.contains_key(&pane_id) {
            return None;
        }
        if self.serial_panes.contains_key(&pane_id) {
            return None;
        }
        // Remove from the BSP tree.
        self.layout.remove(pane_id);
        // Take out of the pane map.
        let pane = self.panes.remove(&pane_id)?;
        // Drop from the tab order.
        self.pane_order.retain(|&id| id != pane_id);
        // If the focus was on the taken pane, move it to the smallest remaining ID.
        if self.focused_pane_id == pane_id {
            let next_id = self
                .panes
                .keys()
                .copied()
                .min()
                .expect("panes guarded by len > 1, so at least one must remain");
            self.focused_pane_id = next_id;
        }
        self.zoomed = false;
        // Resize the remaining panes.
        let layouts = self.compute_layouts(cols, rows);
        for rect in &layouts {
            if let Some(p) = self.panes.get_mut(&rect.pane_id) {
                let _ = p.resize_pty(rect.cols, rect.rows);
            }
        }
        Some(pane)
    }

    /// Append an externally brought-in pane after the focused pane (used by join-pane).
    pub fn insert_pane(&mut self, pane: Pane, total_cols: u16, total_rows: u16, dir: SplitDir) {
        self.insert_pane_at(pane, total_cols, total_rows, dir, None);
    }

    /// Insert an externally brought-in pane at the specified position (Sprint 5-8 Phase 4-4,
    /// used by tab-tearing merge).
    ///
    /// `position` is the insertion index in `pane_order`:
    /// - `None`: same behavior as the existing `insert_pane` (insert immediately after the focused pane).
    /// - `Some(0)`: insert at the beginning.
    /// - `Some(n)`: insert at index `n` (values >= `pane_order.len()` append at the end).
    ///
    /// The BSP tree insertion position is always "next to the focused pane" (matching the existing
    /// `insert_pane`). `position` only affects the display tab order (`pane_order`). The BSP
    /// physical layout and the tab display order are managed independently (as designed in Phase 2-3).
    pub fn insert_pane_at(
        &mut self,
        pane: Pane,
        total_cols: u16,
        total_rows: u16,
        dir: SplitDir,
        position: Option<usize>,
    ) {
        let new_id = pane.id;
        self.layout.insert_after(self.focused_pane_id, new_id, dir);
        self.panes.insert(new_id, pane);
        self.focused_pane_id = new_id;
        self.zoomed = false;

        // Tab order: insert at `position` if provided, otherwise just after the focused pane
        // (delegated to the pure `compute_insert_position`).
        // Note: `new_id` has not been added to `pane_order` yet, so `current_len` uses the
        //       pre-insert length (`compute_insert_position`'s `Some(p).min` allows the
        //       past-the-end value `len`).
        let focused_index = self
            .pane_order
            .iter()
            .position(|&id| id == self.focused_pane_id);
        let new_pos = compute_insert_position(self.pane_order.len(), position, focused_index);
        self.pane_order.insert(new_pos, new_id);

        // Resize every pane.
        let layouts = self.compute_layouts(total_cols, total_rows);
        for rect in &layouts {
            if let Some(p) = self.panes.get_mut(&rect.pane_id) {
                let _ = p.resize_pty(rect.cols, rect.rows);
            }
        }
    }

    /// Return the pane count (PTY + serial).
    pub fn pane_count(&self) -> usize {
        self.panes.len() + self.serial_panes.len()
    }

    /// Return whether any pane in this window is running a foreground process (other than the shell).
    ///
    /// Implemented in Sprint 5-8 Phase 4-4. Aggregates `Pane::has_foreground_process()` (Linux
    /// implementation; currently `false` on macOS / Windows) with OR semantics. Used to decide
    /// whether to show the confirmation dialog when `window.close_action = "prompt"`.
    ///
    /// Returns:
    /// - `true`: at least one pane has a foreground process (vim, ssh, long-running job, ...).
    /// - `false`: every pane sits at the shell prompt, or the OS does not support detection.
    ///
    /// Serial panes are excluded from this aggregation (PTY-only).
    ///
    /// Note: as of Phase 4-4, the `QueryForegroundProcess` IPC has not been added yet, so no
    /// caller exists. Phase 4-5 will add an IPC handler that invokes this from the Prompt branch
    /// of `on_close_requested`.
    #[allow(dead_code)]
    pub fn has_foreground_process(&self) -> bool {
        self.panes.values().any(|p| p.has_foreground_process())
    }

    /// Write input data to the focused pane (PTY or serial port).
    pub fn write_to_focused(&self, data: &[u8]) -> Result<()> {
        if let Some(pane) = self.panes.get(&self.focused_pane_id) {
            return pane.write_input(data);
        }
        if let Some(sp) = self.serial_panes.get(&self.focused_pane_id) {
            return sp.write_input(data);
        }
        Err(anyhow::anyhow!("focused pane not found"))
    }

    /// Return whether the focused pane's bracketed-paste mode is enabled.
    pub fn focused_bracketed_paste_mode(&self) -> bool {
        self.panes
            .get(&self.focused_pane_id)
            .map(|p| p.bracketed_paste.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Return the focused pane's mouse-reporting mode (0 = disabled).
    pub fn focused_mouse_mode(&self) -> u8 {
        self.panes
            .get(&self.focused_pane_id)
            .map(|p| p.mouse_mode.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Resize only the focused pane (backward compatibility, single-pane use).
    #[allow(dead_code)]
    pub fn resize_focused(&mut self, cols: u16, rows: u16) -> Result<()> {
        let pane = self
            .panes
            .get_mut(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("focused pane not found"))?;
        pane.resize_pty(cols, rows)
    }

    /// Resize every pane according to a new total size.
    pub fn resize_all_panes(&mut self, cols: u16, rows: u16) {
        let layouts = self.compute_layouts(cols, rows);
        for rect in &layouts {
            if let Some(pane) = self.panes.get_mut(&rect.pane_id) {
                let _ = pane.resize_pty(rect.cols, rect.rows);
            } else if let Some(sp) = self.serial_panes.get_mut(&rect.pane_id) {
                let _ = sp.resize_pty(rect.cols, rect.rows);
            }
        }
    }

    /// Add a serial port pane to the BSP tree.
    #[allow(clippy::too_many_arguments)]
    pub fn add_serial_pane(
        &mut self,
        total_cols: u16,
        total_rows: u16,
        tx: broadcast::Sender<ServerToClient>,
        port_name: &str,
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: &str,
        dir: SplitDir,
    ) -> Result<u32> {
        let sp = SerialPane::spawn(
            port_name, baud_rate, data_bits, stop_bits, parity, total_cols, total_rows, tx,
        )?;
        let new_id = sp.id;
        self.layout.insert_after(self.focused_pane_id, new_id, dir);
        self.serial_panes.insert(new_id, sp);
        self.focused_pane_id = new_id;
        // Tab order: append at the end (Sprint 5-7 / Phase 2-3).
        self.pane_order.push(new_id);
        Ok(new_id)
    }

    /// Replace the PTY output channel for every pane — broadcast sharing means no swap is needed
    /// on client reattach (no-op).
    #[allow(dead_code)]
    pub fn update_tx_for_all(&self, _tx: &broadcast::Sender<ServerToClient>) {
        // `broadcast::Sender` is shared, so swapping is unnecessary when a client reattaches.
    }

    /// Start recording the focused pane (full implementation in Phase 5-A).
    pub fn start_recording(&self, path: &str) -> Result<u32> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("focused pane not found"))?;
        pane.start_recording(path)?;
        Ok(self.focused_pane_id)
    }

    /// Stop recording the focused pane (full implementation in Phase 5-A).
    pub fn stop_recording(&self) -> Result<u32> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("focused pane not found"))?;
        pane.stop_recording()?;
        Ok(self.focused_pane_id)
    }

    /// Start an asciicast recording on the focused pane.
    pub fn start_asciicast(&self, path: &str) -> Result<u32> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("focused pane not found"))?;
        pane.start_asciicast(path)?;
        Ok(self.focused_pane_id)
    }

    /// Stop the asciicast recording on the focused pane.
    pub fn stop_asciicast(&self) -> Result<u32> {
        let pane = self
            .panes
            .get(&self.focused_pane_id)
            .ok_or_else(|| anyhow::anyhow!("focused pane not found"))?;
        pane.stop_asciicast()?;
        Ok(self.focused_pane_id)
    }

    // ---- Snapshot ----

    /// Convert the window to a snapshot.
    pub fn to_snapshot(&self) -> WindowSnapshot {
        let mut layout = self.layout.to_snapshot();
        // Populate each pane's working directory into the snapshot.
        self.fill_cwd_in_snapshot(&mut layout);
        WindowSnapshot {
            id: self.id,
            name: self.name.clone(),
            focused_pane_id: self.focused_pane_id,
            layout,
        }
    }

    /// Restore a window from a snapshot.
    ///
    /// Each pane is launched as a fresh PTY with the saved shell and working directory.
    pub fn restore_from_snapshot(
        snap: &WindowSnapshot,
        tx: &broadcast::Sender<ServerToClient>,
        shell: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Self> {
        // Reconstruct the BSP tree.
        let layout = SplitNode::from_snapshot(&snap.layout);

        // Compute each pane's size from the BSP layout before starting the PTY.
        let mut size_map = Vec::new();
        compute_pane_sizes(&snap.layout, cols, rows, &mut size_map);

        let mut panes = HashMap::new();
        // Use the BSP tree's DFS order as the initial tab order.
        // Snapshot v3 and earlier do not store `pane_order`, so on restore we use the appearance
        // order from `size_map` (compute_pane_sizes traverses in DFS).
        let mut pane_order: Vec<u32> = Vec::with_capacity(size_map.len());
        for (pane_id, pane_cols, pane_rows) in size_map {
            let cwd = find_cwd_in_snapshot(&snap.layout, pane_id);
            let pane = match cwd {
                Some(ref cwd_path) => Pane::spawn_with_cwd(
                    pane_id,
                    pane_cols,
                    pane_rows,
                    tx.clone(),
                    shell,
                    &[],
                    cwd_path,
                )?,
                None => Pane::spawn_with_id(pane_id, pane_cols, pane_rows, tx.clone(), shell, &[])?,
            };
            panes.insert(pane_id, pane);
            pane_order.push(pane_id);
        }

        Ok(Self {
            id: snap.id,
            name: snap.name.clone(),
            panes,
            serial_panes: HashMap::new(),
            focused_pane_id: snap.focused_pane_id,
            layout,
            zoomed: false,
            layout_mode: LayoutMode::Bsp,
            floating_panes: HashMap::new(),
            pane_order,
        })
    }

    /// Populate each pane's working directory inside a BSP snapshot.
    fn fill_cwd_in_snapshot(&self, node: &mut SplitNodeSnapshot) {
        match node {
            SplitNodeSnapshot::Pane { pane_id, cwd } => {
                if let Some(pane) = self.panes.get(pane_id) {
                    *cwd = pane.working_dir();
                } else if let Some(sp) = self.serial_panes.get(pane_id) {
                    *cwd = sp.working_dir();
                }
            }
            SplitNodeSnapshot::Split { left, right, .. } => {
                self.fill_cwd_in_snapshot(left);
                self.fill_cwd_in_snapshot(right);
            }
        }
    }
}

#[cfg(test)]
mod tests;
