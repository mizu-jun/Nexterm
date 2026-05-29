//! Tab-drag drop-target detection (Sprint 5-8 Phase 4-2).
//!
//! Given a tab drop location (screen coordinates, pixels) and the bounds of every
//! OS Window, this module decides where the tab was dropped: same window, another
//! window's tab bar, or in empty space (new window).
//!
//! - `compute_drop_target` itself is a pure, side-effect-free function and does not
//!   depend directly on winit. It is generic over `Id: Copy + Eq` so tests can pass
//!   `u32` while production code passes `winit::window::WindowId`.
//! - `OsWindowBounds` is built by the caller (`event_handler/mouse.rs`) from each
//!   `ClientWindow.window.outer_position()` / `outer_size()`.
//! - Phase 4-4 will wire the `OtherWindowTabBar` branch into the
//!   `MovePaneToWindow` IPC.

/// Outer rectangle of an OS Window in screen coordinates (pixels).
///
/// `position` is the top-left screen coordinate from `winit::Window::outer_position()`.
/// `size` is the outer size from `winit::Window::outer_size()`.
/// `tab_bar_y_range` is **local to the window's client area** (top..bottom, pixels).
/// Pass `(0.0, 0.0)` when the tab bar is disabled so tab-bar hit detection is bypassed.
///
/// Sprint 5-8 Phase 4-2 lands only the pure decision function up front, so it is
/// marked `#[allow(dead_code)]`. Step 2.5 (the `on_mouse_left_released` wiring)
/// starts using it.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsWindowBounds<Id> {
    pub window_id: Id,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub tab_bar_y_range: (f32, f32),
}

/// Result of drop-target detection.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropTarget<Id> {
    /// Dropped inside the same OS Window (the existing `ReorderPanes` path handles it).
    SameWindow { window_id: Id },
    /// Dropped on another OS Window's tab bar (Phase 4-4 will fire `MovePaneToWindow`;
    /// currently logs only).
    OtherWindowTabBar { window_id: Id },
    /// The drop position is not within any window's bounds, so a new OS Window
    /// should be spawned (`spawn_os_window`).
    NewWindow,
}

/// Pure function that decides **which** OS Window received the drop.
///
/// Decision rules (evaluated in order):
/// 1. The screen position hits the **source_window_id** window's **tab bar area**
///    → `SameWindow` (the caller triggers the existing `ReorderPanes`).
/// 2. The screen position hits **another** window's **tab bar area**
///    → `OtherWindowTabBar` (Phase 4-4 will trigger `MovePaneToWindow`).
/// 3. The screen position is inside **source_window_id**'s window but **outside the
///    tab bar** → `SameWindow` (the caller does nothing; pane-area drops do not
///    trigger reorder).
/// 4. The screen position is inside **another** window but **outside the tab bar**
///    → treated as `OtherWindowTabBar` (anything outside the explicit tab bar does
///    not merge; Phase 4-4 will finalize the requirement).
/// 5. The screen position does not hit any window's bounds
///    → `NewWindow` (spawn a fresh OS Window).
///
/// **Design note**: drops on another window's pane area resolve to
/// `OtherWindowTabBar` rather than `NewWindow` because "the user dropped on a
/// window that is already visible on screen" is more naturally interpreted as
/// some form of integration. Phase 4-4 will decide whether to narrow this to
/// "only tab-bar drops count as a merge" or to add "merge as pane split".
#[allow(dead_code)]
pub fn compute_drop_target<Id>(
    drop_pos: (i32, i32),
    source_window_id: Id,
    os_windows: &[OsWindowBounds<Id>],
) -> DropTarget<Id>
where
    Id: Copy + Eq,
{
    let (px, py) = drop_pos;
    for w in os_windows {
        let (wx, wy) = w.position;
        let (ww, wh) = w.size;
        let in_bounds = px >= wx && px < wx + ww as i32 && py >= wy && py < wy + wh as i32;
        if !in_bounds {
            continue;
        }
        let local_y = (py - wy) as f32;
        let (tab_y0, tab_y1) = w.tab_bar_y_range;
        let tab_bar_enabled = tab_y1 > tab_y0;
        let on_tab_bar = tab_bar_enabled && local_y >= tab_y0 && local_y < tab_y1;

        if w.window_id == source_window_id {
            return DropTarget::SameWindow {
                window_id: w.window_id,
            };
        } else if on_tab_bar {
            return DropTarget::OtherWindowTabBar {
                window_id: w.window_id,
            };
        } else {
            // Inside another window but outside the tab bar: provisional behavior
            // pending Phase 4-4 (treat as a no-op merge target).
            return DropTarget::OtherWindowTabBar {
                window_id: w.window_id,
            };
        }
    }
    DropTarget::NewWindow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(id: u32, x: i32, y: i32, w: u32, h: u32) -> OsWindowBounds<u32> {
        OsWindowBounds {
            window_id: id,
            position: (x, y),
            size: (w, h),
            tab_bar_y_range: (0.0, 32.0),
        }
    }

    #[test]
    fn same_window_drop_on_tab_bar() {
        // Tab bar is local_y 0..32 = screen y 100..132.
        let windows = [bounds(1, 100, 100, 800, 600)];
        let result = compute_drop_target((200, 110), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }

    #[test]
    fn same_window_drop_on_pane_area() {
        let windows = [bounds(1, 100, 100, 800, 600)];
        // y=200 is outside the tab bar → still treated as the same window's pane area.
        let result = compute_drop_target((200, 200), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }

    #[test]
    fn other_window_drop_on_tab_bar() {
        let windows = [
            bounds(1, 100, 100, 800, 600),
            bounds(2, 1000, 100, 800, 600),
        ];
        let result = compute_drop_target((1200, 110), 1, &windows);
        assert_eq!(result, DropTarget::OtherWindowTabBar { window_id: 2 });
    }

    #[test]
    fn no_window_hit_spawns_new_window() {
        let windows = [bounds(1, 100, 100, 800, 600)];
        let result = compute_drop_target((2000, 2000), 1, &windows);
        assert_eq!(result, DropTarget::NewWindow);
    }

    #[test]
    fn empty_os_windows_spawns_new_window() {
        let windows: [OsWindowBounds<u32>; 0] = [];
        let result = compute_drop_target((100, 100), 1, &windows);
        assert_eq!(result, DropTarget::NewWindow);
    }

    #[test]
    fn top_left_corner_counts_as_inside() {
        let windows = [bounds(1, 100, 100, 800, 600)];
        // (100, 100) is the top-left corner; px >= wx && py >= wy, so it is in_bounds.
        let result = compute_drop_target((100, 100), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }

    #[test]
    fn bottom_right_corner_counts_as_outside() {
        let windows = [bounds(1, 100, 100, 800, 600)];
        // (900, 700) is the outer right=100+800=900 and bottom=100+600=700; px < wx+ww
        // (900 < 900) is false.
        let result = compute_drop_target((900, 700), 1, &windows);
        assert_eq!(result, DropTarget::NewWindow);
    }

    #[test]
    fn tab_bar_disabled_falls_back_to_pane_area() {
        let windows = [OsWindowBounds {
            window_id: 1u32,
            position: (100, 100),
            size: (800, 600),
            tab_bar_y_range: (0.0, 0.0), // tab bar disabled
        }];
        // With the tab bar disabled, in_bounds still resolves to the same window
        // (reorder is a no-op).
        let result = compute_drop_target((200, 110), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }

    #[test]
    fn other_window_pane_area_is_provisional_other_window_tab_bar() {
        // Phase 4-4 will revisit this once the merge requirements are finalized.
        let windows = [
            bounds(1, 100, 100, 800, 600),
            bounds(2, 1000, 100, 800, 600),
        ];
        let result = compute_drop_target((1200, 200), 1, &windows);
        assert_eq!(result, DropTarget::OtherWindowTabBar { window_id: 2 });
    }

    #[test]
    fn overlapping_windows_first_one_wins() {
        // Evaluation runs from the head of the Vec, so the first window wins on overlap.
        let windows = [
            bounds(1, 100, 100, 800, 600),
            bounds(2, 100, 100, 800, 600), // fully overlapping
        ];
        let result = compute_drop_target((200, 110), 1, &windows);
        assert_eq!(result, DropTarget::SameWindow { window_id: 1 });
    }
}
