//! Mouse-related handlers among `winit::WindowEvent` variants.
//!
//! Extracted from `event_handler.rs`:
//! - `on_cursor_moved`
//! - `on_mouse_right_pressed` — show the context menu
//! - `on_mouse_left_pressed` — tab clicks / settings panel / start selection
//! - `on_mouse_left_released` — finalize selection / clipboard copy / open URL / focus switch
//! - `on_mouse_wheel`

use std::sync::Arc;
use std::time::{Duration, Instant};

use nexterm_proto::ClientToServer;
use winit::event::MouseScrollDelta;

use super::EventHandler;
use super::settings_panel_hit::SettingsPanelHit;
use crate::state::ContextMenu;
use crate::vertex_util::visual_width;

/// Compute the new tab order after a drag (Sprint 5-7 / Phase 2-3).
///
/// Take `dragged_id` out of `current` and insert it at the position of
/// `target_id`. Behavior: "push the dragged tab into the target tab's spot."
///
/// - `from < target_pos` (moving right): removing `dragged` shifts `target`
///   left by one, so `insert_at = target_pos - 1` lands at the same on-screen
///   position as the original `target_pos`.
/// - `from > target_pos` (moving left): removing `dragged` does not affect
///   `target`, so `insert_at = target_pos` pushes `target` one slot to the right.
///
/// In an adjacent right-drag swap (`|from - target_pos| == 1`), this model
/// produces a result identical to the original and returns `None` (avoiding a
/// pointless round trip). If left/right disambiguation is needed in the future,
/// extend `hover_target` to `(pane_id, Before/After)`.
///
/// Returns `None` when `current` does not contain `dragged_id` or `target_id`.
pub(super) fn compute_reordered_tab_order(
    current: &[u32],
    dragged_id: u32,
    target_id: u32,
) -> Option<Vec<u32>> {
    if dragged_id == target_id {
        return None;
    }
    let from = current.iter().position(|&id| id == dragged_id)?;
    let target_pos = current.iter().position(|&id| id == target_id)?;

    let mut new_order: Vec<u32> = current.to_vec();
    new_order.remove(from);
    let insert_at = if from < target_pos {
        target_pos - 1
    } else {
        target_pos
    };
    new_order.insert(insert_at, dragged_id);

    if new_order == current {
        return None;
    }
    Some(new_order)
}

impl EventHandler {
    /// Handle a drop that landed outside the tab bar (Sprint 5-8 Phase 4-2).
    ///
    /// Call `compute_drop_target` with `drag.current_screen_pos` and the bounds
    /// of every registered OS window to decide the drop destination:
    ///
    /// - `SameWindow`: dropped inside the same window's pane area → do nothing
    ///   (matches existing behavior).
    /// - `OtherWindowTabBar`: dropped on another window's tab bar → merge
    ///   implementation in Phase 4-4. Currently log only.
    /// - `NewWindow`: dropped outside every window → call `spawn_os_window`
    ///   (skeleton in Phase 4-2).
    ///
    /// If `current_screen_pos` is `None` (fallback failure on Wayland and
    /// similar), do nothing (decision #4: the alternative UX provides feature
    /// parity).
    fn handle_tab_drag_drop_outside(&mut self, drag: &crate::state::TabDragState) {
        let Some(drop_pos) = drag.current_screen_pos else {
            tracing::debug!(
                "Drop outside tab bar: global coordinates unavailable (Wayland, etc.) → disabling feature"
            );
            return;
        };
        let Some(source_id) = drag.source_os_window_id else {
            tracing::debug!("Drop outside tab bar: source_os_window_id not set → skipping");
            return;
        };

        // Collect the bounds of every registered OS window.
        // As of Phase 4-2 there is only the primary window; multi-window
        // support arrives from Phase 4-4 onward.
        let tab_bar_h = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f32
        } else {
            0.0
        };
        let mut bounds_vec: Vec<crate::drop_target::OsWindowBounds<winit::window::WindowId>> =
            Vec::new();
        if let Some(w) = &self.window
            && let Ok(outer_pos) = w.outer_position()
        {
            let outer_size = w.outer_size();
            bounds_vec.push(crate::drop_target::OsWindowBounds {
                window_id: w.id(),
                position: (outer_pos.x, outer_pos.y),
                size: (outer_size.width, outer_size.height),
                tab_bar_y_range: (0.0, tab_bar_h),
            });
        }
        // Also collect OS windows from the `self.windows` HashMap introduced in
        // Phase 4-1. Filter by id to avoid duplicating the primary window
        // (`self.window` will be retired in Phase 4-4).
        for (id, cw) in &self.windows {
            if Some(*id) == self.window.as_ref().map(|w| w.id()) {
                continue;
            }
            if let Ok(outer_pos) = cw.window.outer_position() {
                let outer_size = cw.window.outer_size();
                bounds_vec.push(crate::drop_target::OsWindowBounds {
                    window_id: *id,
                    position: (outer_pos.x, outer_pos.y),
                    size: (outer_size.width, outer_size.height),
                    tab_bar_y_range: (0.0, tab_bar_h),
                });
            }
        }

        let target = crate::drop_target::compute_drop_target(drop_pos, source_id, &bounds_vec);
        match target {
            crate::drop_target::DropTarget::SameWindow { .. } => {
                // Drop into the pane area: matches the existing behavior (do nothing).
            }
            crate::drop_target::DropTarget::OtherWindowTabBar { window_id } => {
                // Sprint 5-8 Phase 4-4 Step D: dropped on another OS window's tab bar.
                //
                // Resolve the server window ID shown by the target OS window
                // (`focused_server_window_id`) and send
                // `MovePaneToWindow { target_window_id }`.
                //
                // Resolution order:
                // 1. Additional OS windows registered in `self.windows` →
                //    `view_state.focused_server_window_id`.
                // 2. Primary window (`self.window`) →
                //    `self.app.state.focused_server_window_id`
                //    (kept up to date by `WindowListChanged`).
                let target_server_id = if let Some(cw) = self.windows.get(&window_id) {
                    Some(cw.view_state.focused_server_window_id)
                } else if self.window.as_ref().map(|w| w.id()) == Some(window_id) {
                    let id = self.app.state.focused_server_window_id;
                    if id == 0 { None } else { Some(id) }
                } else {
                    None
                };

                match target_server_id {
                    Some(target) => {
                        tracing::info!(
                            "Drop outside tab bar: dropped on another OS window's tab bar (os_window={:?}, target_server_window={})",
                            window_id,
                            target
                        );
                        if let Some(conn) = &self.connection {
                            let _ = conn.send_tx.try_send(
                                nexterm_proto::ClientToServer::MovePaneToWindow {
                                    pane_id: drag.pane_id,
                                    target_window_id: target,
                                    insert_at: None, // Phase 4-5 adds position support based on hover.
                                },
                            );
                        }
                    }
                    None => {
                        tracing::warn!(
                            "OtherWindowTabBar branch: could not resolve target OS window's server_window_id (window_id={:?})",
                            window_id
                        );
                    }
                }
            }
            crate::drop_target::DropTarget::NewWindow => {
                tracing::info!(
                    "Drop outside tab bar: sending new-window creation request (drop_pos={:?}, pane_id={})",
                    drop_pos,
                    drag.pane_id
                );
                // Sprint 5-8 Phase 4-3 + 4-4:
                // 1. Send `MovePaneToWindow { target_window_id: 0 }` to the
                //    server, which creates a new server window and moves the
                //    pane.
                // 2. The client-side OS window is spawned when the server's
                //    `WindowListChanged` reports a new window ID; the spawn
                //    fires via `EventLoopProxy<UserEvent::SpawnOsWindow>`
                //    (implemented in Step C).
                // 3. Record the drop position in
                //    `pending_new_window_drop_pos` and use it as the position
                //    for the spawned window.
                self.pending_new_window_drop_pos =
                    Some(winit::dpi::PhysicalPosition::new(drop_pos.0, drop_pos.1));
                if let Some(conn) = &self.connection {
                    let _ =
                        conn.send_tx
                            .try_send(nexterm_proto::ClientToServer::MovePaneToWindow {
                                pane_id: drag.pane_id,
                                target_window_id: 0,
                                insert_at: None,
                            });
                }
            }
        }
    }

    /// Resolve the mouse cursor's global screen coordinates (Sprint 5-8 Phase 4-2).
    ///
    /// Priority order:
    /// 1. A platform-specific OS API (Windows: `GetCursorPos`).
    /// 2. winit's `window.outer_position()` plus the client-area cursor coordinates.
    /// 3. Both fail (Wayland, etc.) → `None`.
    ///
    /// `client_x` / `client_y` are the winit `CursorMoved.position` values
    /// (origin at the top-left of the window's client area, in pixels). Used
    /// by the fallback computation.
    ///
    /// When the return value is `None`, the caller skips the out-of-tab-bar
    /// drop test and falls back to the existing `ReorderPanes` path
    /// (decision #4: Wayland uses the alternative UX).
    fn resolve_screen_pos(
        window: &Option<Arc<winit::window::Window>>,
        client_x: i32,
        client_y: i32,
    ) -> Option<(i32, i32)> {
        if let Some(pos) = crate::platform::cursor_screen_pos() {
            return Some(pos);
        }
        let outer = window.as_ref()?.outer_position().ok()?;
        Some((outer.x + client_x, outer.y + client_y))
    }

    /// `WindowEvent::CursorMoved` — track the cursor position and update the
    /// selection while dragging.
    pub(super) fn on_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        self.cursor_position = Some((position.x, position.y));
        let cell_w = self.app.font.cell_width() as f64;
        let cell_h = self.app.font.cell_height() as f64;
        let tab_bar_h_f64 = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f64
        } else {
            0.0_f64
        };

        // Phase 4 (UI/UX v2): pane-border resize. If a drag is in flight,
        // convert the cursor delta into a ratio delta and stream it to the
        // server. Otherwise, hover hit-test sets the resize cursor icon so
        // the affordance is discoverable.
        if let Some(mut drag) = self.app.state.pane_resize_drag {
            let (px_f32, py_f32) = (position.x as f32, position.y as f32);
            let pixel_delta = match drag.axis {
                crate::state::PaneResizeAxis::Horizontal => px_f32 - drag.last_cursor.0,
                crate::state::PaneResizeAxis::Vertical => py_f32 - drag.last_cursor.1,
            };
            // span_px is the parent split's total length; clamp the resulting
            // ratio delta to the same band the server applies (clamp 0.1..0.9
            // inside adjust_ratio_for, so per-frame deltas above 0.8 are
            // effectively a no-op anyway).
            let ratio_delta = (pixel_delta / drag.span_px).clamp(-0.5, 0.5);
            if ratio_delta.abs() > 0.0005 {
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(nexterm_proto::ClientToServer::ResizeSplit {
                            delta: ratio_delta,
                        });
                }
                drag.last_cursor = (px_f32, py_f32);
                self.app.state.pane_resize_drag = Some(drag);
                if let Some(w) = &self.window {
                    w.set_cursor(match drag.axis {
                        crate::state::PaneResizeAxis::Horizontal => {
                            winit::window::CursorIcon::EwResize
                        }
                        crate::state::PaneResizeAxis::Vertical => {
                            winit::window::CursorIcon::NsResize
                        }
                    });
                    w.request_redraw();
                }
            }
            return;
        }

        // Hover hit-test against pane borders for the resize cursor icon.
        // Skipped while any other modal UI is open or any drag is in flight,
        // so we don't fight the existing affordances.
        if !self.app.state.settings_panel.is_open
            && self.app.state.tab_drag.is_none()
            && let Some(w) = &self.window
        {
            let pad_x = self.app.config.window.padding_x as f32;
            let pad_y = self.app.config.window.padding_y as f32;
            let origin_x = pad_x;
            let origin_y = tab_bar_h_f64 as f32 + pad_y;
            let hit = if position.y >= origin_y as f64 {
                crate::state::hit_test_pane_border(
                    &self.app.state.pane_layouts,
                    position.x as f32,
                    position.y as f32,
                    cell_w as f32,
                    cell_h as f32,
                    origin_x,
                    origin_y,
                )
            } else {
                None
            };
            let next_cursor = match hit {
                Some(h) => match h.axis {
                    crate::state::PaneResizeAxis::Horizontal => winit::window::CursorIcon::EwResize,
                    crate::state::PaneResizeAxis::Vertical => winit::window::CursorIcon::NsResize,
                },
                None => winit::window::CursorIcon::Default,
            };
            if self.app.state.last_cursor_icon != next_cursor {
                w.set_cursor(next_cursor);
                self.app.state.last_cursor_icon = next_cursor;
            }
        }
        let col = (position.x / cell_w) as u16;
        let row = ((position.y - tab_bar_h_f64).max(0.0) / cell_h) as u16;

        // Sprint 5-7 / UI-1-1: hover tracking over the tab bar.
        // When the cursor is inside the tab-bar area (y < tab_bar_h), hit-test
        // by x and update the hovered tab ID. Out of range or tab bar
        // disabled → always None.
        let prev_hovered = self.app.state.hovered_tab_id;
        let new_hovered = if self.app.config.tab_bar.enabled && position.y < tab_bar_h_f64 {
            let px = position.x as f32;
            self.app
                .state
                .tab_hit_rects
                .iter()
                .find(|&(_, &(x0, x1))| px >= x0 && px < x1)
                .map(|(&id, _)| id)
        } else {
            None
        };
        if prev_hovered != new_hovered {
            self.app.state.hovered_tab_id = new_hovered;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }

        // Sprint 5-7 / Phase 2-3: track an in-progress tab drag.
        // While inside the tab-bar area with a drag in progress, update
        // current_x / hover_target / committed.
        //
        // Sprint 5-8 Phase 4-2: also update `current_screen_pos` for the
        // out-of-tab-bar drop test. On Windows we read it from the OS API
        // (GetCursorPos); elsewhere we fall back to winit's outer_position +
        // client coordinates.
        if self.app.state.tab_drag.is_some() {
            let new_screen_pos =
                Self::resolve_screen_pos(&self.window, position.x as i32, position.y as i32);
            if let Some(drag) = self.app.state.tab_drag.as_mut() {
                let px_f32 = position.x as f32;
                drag.current_x = px_f32;
                drag.current_screen_pos = new_screen_pos;
                // Confirm the drag once the cursor has moved 6 px or more.
                const DRAG_THRESHOLD: f32 = 6.0;
                if !drag.committed && (px_f32 - drag.start_x).abs() >= DRAG_THRESHOLD {
                    drag.committed = true;
                }
                // Decide the insertion target (any tab hit inside the tab-bar area).
                let on_tab_bar = position.y < tab_bar_h_f64;
                drag.hover_target = if on_tab_bar {
                    self.app
                        .state
                        .tab_hit_rects
                        .iter()
                        .find(|&(_, &(x0, x1))| px_f32 >= x0 && px_f32 < x1)
                        .map(|(&id, _)| id)
                } else {
                    None
                };
                if drag.committed
                    && let Some(w) = &self.window
                {
                    w.request_redraw();
                }
            }
        }
        if self.app.state.mouse_sel.is_dragging {
            self.app.state.mouse_sel.update(col, row);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            // Report the motion while dragging too (button 0 = left drag).
            if let Some(conn) = &self.connection {
                let _ = conn.send_tx.try_send(ClientToServer::MouseReport {
                    button: 0,
                    col,
                    row,
                    pressed: true,
                    motion: true,
                });
            }
        }

        // Phase 3 (UI 4-tasks, 2026-06-12): if a title-bar drag is in flight,
        // update the panel's drag offset before any other hit-tests so the
        // rendered position tracks the cursor on this frame. `update_drag` is
        // a no-op when no drag is active, so the unconditional call is cheap.
        {
            let fx = position.x as f32;
            let fy = position.y as f32;
            let sp = &mut self.app.state.settings_panel;
            if sp.is_dragging() {
                sp.update_drag(fx, fy);
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }

        // While dragging a settings-panel slider, update the value live.
        {
            let fx = position.x as f32;
            let sp = &mut self.app.state.settings_panel;
            if let Some(drag) = &sp.drag_slider.clone() {
                use crate::settings_panel::SliderType;
                match drag.slider_type {
                    SliderType::FontSize => {
                        sp.set_font_size_from_slider(fx, drag.track_x, drag.track_w);
                    }
                    SliderType::WindowOpacity => {
                        sp.set_opacity_from_slider(fx, drag.track_x, drag.track_w);
                    }
                    SliderType::WindowPaddingX => {
                        sp.set_padding_x_from_slider(fx, drag.track_x, drag.track_w);
                    }
                    SliderType::WindowPaddingY => {
                        sp.set_padding_y_from_slider(fx, drag.track_x, drag.track_w);
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }

        // When the context menu is open, update the hovered item.
        if let Some(menu) = &mut self.app.state.context_menu {
            let cw = self.app.font.cell_width();
            let ch = self.app.font.cell_height();
            let menu_w = 18.0 * cw;
            let fx = position.x as f32;
            let fy = position.y as f32;
            let mut new_hovered = None;
            if fx >= menu.x && fx <= menu.x + menu_w {
                for (i, _item) in menu.items.iter().enumerate() {
                    let item_y = menu.y + i as f32 * ch;
                    if fy >= item_y && fy < item_y + ch {
                        new_hovered = Some(i);
                        break;
                    }
                }
            }
            if menu.hovered != new_hovered {
                menu.hovered = new_hovered;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }
    }

    /// Right button press: open the context menu.
    ///
    /// Phase 2c follow-up: when the click landed inside a known command
    /// block we build a block-aware menu via `ContextMenu::new_for_block`
    /// instead of the plain default. The block-action entries are prepended
    /// so they are the first thing the user sees.
    pub(super) fn on_mouse_right_pressed(&mut self) {
        if let Some((px, py)) = self.cursor_position {
            let cell_w_ctx = self.app.font.cell_width() as f64;
            let cell_h_ctx = self.app.font.cell_height() as f64;
            let profile_list: Vec<(String, String)> = self
                .app
                .config
                .profiles
                .iter()
                .map(|p| (p.name.clone(), p.icon.clone()))
                .collect();

            // Determine whether the right-click landed on a known block.
            let tab_bar_h = if self.app.config.tab_bar.enabled {
                self.app.config.tab_bar.height as f64
            } else {
                0.0
            };
            let block_under_cursor: Option<(u64, bool)> =
                self.block_under_cursor(px, py, tab_bar_h, cell_h_ctx);

            let build = |x: f32, y: f32| match block_under_cursor {
                Some((id, has_name)) => {
                    ContextMenu::new_for_block(x, y, &profile_list, id, has_name)
                }
                None => ContextMenu::new_default(x, y, &profile_list),
            };

            let tmp = build(0.0, 0.0);
            let item_count = tmp.items.len();
            let max_label = tmp
                .items
                .iter()
                .map(|i| visual_width(&i.label))
                .max()
                .unwrap_or(8);
            let max_hint = tmp
                .items
                .iter()
                .map(|i| visual_width(&i.hint))
                .max()
                .unwrap_or(0);
            let menu_w_px = ((max_label + max_hint + 5) as f64).max(16.0) * cell_w_ctx;
            let menu_h_px = item_count as f64 * cell_h_ctx;

            let win_w = self
                .window
                .as_ref()
                .map(|w| w.inner_size().width as f64)
                .unwrap_or(800.0);
            let win_h = self
                .window
                .as_ref()
                .map(|w| w.inner_size().height as f64)
                .unwrap_or(600.0);
            let menu_x = (px).min(win_w - menu_w_px).max(0.0) as f32;
            let menu_y = (py).min(win_h - menu_h_px).max(0.0) as f32;

            self.app.state.context_menu = Some(build(menu_x, menu_y));
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    /// Phase 2c follow-up: identify the command block under a (px, py)
    /// cursor position on the focused pane. Returns `(block_id, has_name)`
    /// when the click landed inside a block's row range; `None` otherwise.
    ///
    /// Mirrors the row-resolution logic in `handle_block_mouse_click` so the
    /// right-click and left-border-click code paths agree on which block
    /// the cursor refers to.
    fn block_under_cursor(
        &self,
        px: f64,
        py: f64,
        tab_bar_h: f64,
        cell_h: f64,
    ) -> Option<(u64, bool)> {
        let cfg = &self.app.config.blocks;
        if !cfg.enabled {
            return None;
        }
        if py < tab_bar_h {
            return None;
        }
        let win_h = self
            .window
            .as_ref()
            .map(|w| w.inner_size().height as f64)
            .unwrap_or(0.0);
        if win_h > 0.0 && py >= win_h - cell_h {
            return None;
        }
        let _ = px; // px not needed; we treat the entire pane width as in-range.
        let pane_id = self.app.state.focused_pane_id?;
        let pane = self.app.state.panes.get(&pane_id)?;
        if pane.blocks.is_empty() {
            return None;
        }
        let visual_row = ((py - tab_bar_h) / cell_h) as usize;
        let abs_row = if pane.scroll_offset > 0 {
            crate::command_blocks::resolve_clicked_scrollback_row(
                &pane.blocks,
                pane.scrollback.len(),
                pane.scroll_offset,
                visual_row,
            )?
        } else {
            pane.scrollback.len() + visual_row
        };
        let block = crate::command_blocks::block_containing_row(&pane.blocks, abs_row)?;
        let id = block.id;
        let has_name = self.app.state.named_blocks.get(id).is_some();
        Some((id, has_name))
    }

    /// Left button press: handle tab-bar hits + start selection + mouse report.
    pub(super) fn on_mouse_left_pressed(&mut self) {
        if let Some((px, py)) = self.cursor_position {
            // When the settings panel is open, run the hit test first.
            if self.app.state.settings_panel.is_open {
                let hit = self.hit_test_settings_panel(px as f32, py as f32);
                use crate::settings_panel::SliderType;
                match hit {
                    SettingsPanelHit::Outside => {
                        // Click outside the panel → close the panel.
                        self.app.state.settings_panel.close();
                    }
                    SettingsPanelHit::Category(idx) => {
                        // Click on a sidebar category → switch category. With
                        // a non-empty Phase 4 search, the rendered list is the
                        // filtered subset, so resolve via `filtered_categories`
                        // to honour the user-visible order.
                        let filtered = self.app.state.settings_panel.filtered_categories();
                        if let Some(cat) = filtered.get(idx) {
                            self.app.state.settings_panel.category = cat.clone();
                            // Clicking a category implicitly defocuses the
                            // search input so subsequent keyboard navigation
                            // (Tab / ↑ / ↓) operates on the panel again.
                            self.app.state.settings_panel.unfocus_search();
                        }
                    }
                    SettingsPanelHit::SearchInput => {
                        // Phase 4 (UI/UX v2): grab keyboard focus for the
                        // search field. The next keystroke will edit
                        // `search_query`.
                        self.app.state.settings_panel.focus_search();
                    }
                    SettingsPanelHit::Slider {
                        slider_type,
                        track_x,
                        track_w,
                        min: _,
                        max: _,
                    } => {
                        // Click on a slider → apply the value immediately and start drag state.
                        let fx = px as f32;
                        let sp = &mut self.app.state.settings_panel;
                        // Phase 5-11-6 #6: align focus when clicking a Window-category slider.
                        match slider_type {
                            SliderType::FontSize => {
                                sp.set_font_size_from_slider(fx, track_x, track_w)
                            }
                            SliderType::WindowOpacity => {
                                sp.window_field_focus = 0;
                                sp.set_opacity_from_slider(fx, track_x, track_w);
                            }
                            SliderType::WindowPaddingX => {
                                sp.window_field_focus = 2;
                                sp.set_padding_x_from_slider(fx, track_x, track_w);
                            }
                            SliderType::WindowPaddingY => {
                                sp.window_field_focus = 3;
                                sp.set_padding_y_from_slider(fx, track_x, track_w);
                            }
                        }
                        let (min_val, max_val) = match slider_type {
                            SliderType::FontSize => (8.0, 32.0),
                            SliderType::WindowOpacity => (0.1, 1.0),
                            SliderType::WindowPaddingX | SliderType::WindowPaddingY => (0.0, 32.0),
                        };
                        sp.drag_slider = Some(crate::settings_panel::SliderDrag {
                            slider_type,
                            track_x,
                            track_w,
                            min_val,
                            max_val,
                        });
                    }
                    SettingsPanelHit::ThemeColor(idx) => {
                        // Click a theme color dot → switch scheme.
                        self.app.state.settings_panel.scheme_index = idx;
                        self.app.state.settings_panel.dirty = true;
                    }
                    SettingsPanelHit::WindowRow(row) => {
                        // Phase 5-11-6 #6: click on a row inside the Window category.
                        // Change focus; clicking the label of rows 1/4 also cycles the value.
                        let sp = &mut self.app.state.settings_panel;
                        sp.window_field_focus = row;
                        match row {
                            1 => sp.next_cursor_style(),
                            4 => sp.next_present_mode(),
                            _ => {}
                        }
                    }
                    SettingsPanelHit::TitleBar => {
                        // Phase 3 (UI 4-tasks, 2026-06-12): pressing the title
                        // bar starts a drag-to-move. The actual offset update
                        // happens in `on_cursor_moved`, and `on_mouse_left_released`
                        // ends the drag — same pattern as the slider drag right
                        // above. We capture `cursor_position` (already in
                        // physical pixels) as the grab anchor.
                        let fx = px as f32;
                        let fy = py as f32;
                        self.app.state.settings_panel.start_drag(fx, fy);
                    }
                    SettingsPanelHit::BlocksRow(row) => {
                        // Phase 2c follow-up: interactive Blocks toggles.
                        // row 0 = enabled, row 1 = border width (cycle 1..=8),
                        // row 2 = status badge. Each click marks the panel
                        // dirty so the next Save (or panel close) persists.
                        let sp = &mut self.app.state.settings_panel;
                        match row {
                            0 => sp.blocks_enabled = !sp.blocks_enabled,
                            1 => {
                                sp.blocks_border_width_px = if sp.blocks_border_width_px >= 8 {
                                    1
                                } else {
                                    sp.blocks_border_width_px + 1
                                };
                            }
                            2 => {
                                sp.blocks_show_exit_code_badge = !sp.blocks_show_exit_code_badge;
                            }
                            _ => {}
                        }
                        sp.dirty = true;
                        let _ = sp.save_to_toml();
                        sp.dirty = false;
                    }
                    SettingsPanelHit::PanelBackground => {
                        // Other clicks inside the panel → do nothing.
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                return; // While the settings panel is open, do not pass the click to the terminal.
            }

            let cell_w = self.app.font.cell_width() as f64;
            let cell_h = self.app.font.cell_height() as f64;
            // Sprint 5-15 / UI/UX Modernization v2 Phase 2b: mirror
            // `render_frame::tab_bar_visible` so clicks in the reclaimed
            // top-of-window region do not accidentally hit tab-bar logic.
            let tab_bar_visible = self.app.config.tab_bar.enabled
                && !(self.app.config.tab_bar.hide_when_single
                    && self.app.state.pane_layouts.len() <= 1);
            let tab_bar_h_f64 = if tab_bar_visible {
                self.app.config.tab_bar.height as f64
            } else {
                0.0_f64
            };

            // Phase 4 (UI/UX v2): pane-border resize start. Run before tab
            // bar / pane click handling so a click inside the tolerance band
            // of an internal border kicks off a resize drag instead of
            // landing in the underlying terminal cell. Out-of-terminal areas
            // (tab bar, padding above grid) cannot host a border, so we
            // only test when the cursor is in the grid area.
            let pad_x = self.app.config.window.padding_x as f32;
            let pad_y = self.app.config.window.padding_y as f32;
            let origin_x = pad_x;
            let origin_y = tab_bar_h_f64 as f32 + pad_y;
            if py >= origin_y as f64
                && let Some(hit) = crate::state::hit_test_pane_border(
                    &self.app.state.pane_layouts,
                    px as f32,
                    py as f32,
                    cell_w as f32,
                    cell_h as f32,
                    origin_x,
                    origin_y,
                )
            {
                // Focus the adjacent pane locally + on the server so the
                // subsequent `ResizeSplit` updates target the right split
                // ancestor (see `window/bsp.rs::adjust_ratio_for`).
                self.app.state.set_focused_pane(hit.adjacent_pane_id);
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(nexterm_proto::ClientToServer::FocusPane {
                            pane_id: hit.adjacent_pane_id,
                        });
                }
                // Compute the span of the parent split in pixels (used
                // to convert pixel deltas into ratio deltas). Approximate
                // it from the adjacent pane's own size — this matches
                // the typical 50/50 split and is corrected by `clamp` on
                // the server side regardless.
                let span_px =
                    if let Some(layout) = self.app.state.pane_layouts.get(&hit.adjacent_pane_id) {
                        match hit.axis {
                            crate::state::PaneResizeAxis::Horizontal => {
                                layout.cols as f32 * cell_w as f32 * 2.0
                            }
                            crate::state::PaneResizeAxis::Vertical => {
                                layout.rows as f32 * cell_h as f32 * 2.0
                            }
                        }
                    } else {
                        // Fallback: 256 px guarantees a finite span.
                        256.0
                    };
                self.app.state.pane_resize_drag = Some(crate::state::PaneResizeDrag {
                    focused_pane_id: hit.adjacent_pane_id,
                    axis: hit.axis,
                    span_px: span_px.max(32.0),
                    last_cursor: (px as f32, py as f32),
                });
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                return;
            }

            // Handle clicks in the tab-bar area (py < tab_bar_h).
            if tab_bar_visible && py < tab_bar_h_f64 {
                let px_f32 = px as f32;
                // Hit test for the settings button.
                let hit_settings = self
                    .app
                    .state
                    .settings_tab_rect
                    .map(|(x0, x1)| px_f32 >= x0 && px_f32 < x1)
                    .unwrap_or(false);
                if hit_settings {
                    self.app.state.settings_panel.is_open = !self.app.state.settings_panel.is_open;
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                } else if self
                    .app
                    .state
                    .new_tab_hit_rect
                    .map(|(x0, x1)| px_f32 >= x0 && px_f32 < x1)
                    .unwrap_or(false)
                {
                    // Sprint 5-15 / UI/UX Modernization v2 Phase 2b:
                    // clicking the tab-bar `+` button creates a new pane in
                    // the current window. Modelled on `SplitVertical` since
                    // Nexterm renders one tab per pane.
                    tracing::info!("[+] new-tab button click: dispatching SplitVertical");
                    if let Some(conn) = &self.connection {
                        let _ = conn
                            .send_tx
                            .try_send(nexterm_proto::ClientToServer::SplitVertical);
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                } else if let Some(close_pane_id) = self
                    .app
                    .state
                    .tab_close_hit_rects
                    .iter()
                    .find(|&(_, &(x0, x1))| px_f32 >= x0 && px_f32 < x1)
                    .map(|(&id, _)| id)
                {
                    // Phase 2 (UI/UX modernization): clicking the tab-hover `[×]`
                    // button closes the pane. Evaluated before tear-out and tab-click
                    // hit-tests so the close region (which overlaps the tab and is
                    // adjacent to `[↗]`) wins. The path matches `execute_action("ClosePane")`.
                    tracing::info!("[×] close button click: closing pane_id={}", close_pane_id);
                    if let Some(conn) = &self.connection {
                        // Focus the target pane first so the server's ClosePane
                        // (which targets the focused pane) applies to the clicked tab.
                        if self.app.state.focused_pane_id != Some(close_pane_id) {
                            let _ =
                                conn.send_tx
                                    .try_send(nexterm_proto::ClientToServer::FocusPane {
                                        pane_id: close_pane_id,
                                    });
                        }
                        let _ = conn
                            .send_tx
                            .try_send(nexterm_proto::ClientToServer::ClosePane);
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                } else if let Some(tearout_pane_id) = self
                    .app
                    .state
                    .tab_tearout_hit_rects
                    .iter()
                    .find(|&(_, &(x0, x1))| px_f32 >= x0 && px_f32 < x1)
                    .map(|(&id, _)| id)
                {
                    // Sprint 5-9 Phase 4-6: clicking the tab-hover `[↗]` button
                    // tears the tab out. Evaluate this before the tab-click
                    // hit-test so the tear-out region (which overlaps the tab)
                    // does not also trigger a focus change. The path is
                    // identical to `execute_action("DetachToNewWindow")` —
                    // BreakPane + setting `pending_new_window_drop_pos`.
                    tracing::info!(
                        "[↗] tear-out button click: detaching pane_id={} into a new OS window",
                        tearout_pane_id
                    );
                    // Record pos = (0, 0) (no mouse-coordinate dependency; winit decides the position).
                    self.pending_new_window_drop_pos =
                        Some(winit::dpi::PhysicalPosition::new(0, 0));
                    if let Some(conn) = &self.connection {
                        // It is safer to focus the target pane before sending
                        // BreakPane, but `[↗]` only appears on the hovered
                        // tab, so a click on a non-focused tab is unlikely.
                        // If reliability is needed in the future, send
                        // FocusPane first. For safety, prepend FocusPane when
                        // pane_id is not focused.
                        if self.app.state.focused_pane_id != Some(tearout_pane_id) {
                            let _ =
                                conn.send_tx
                                    .try_send(nexterm_proto::ClientToServer::FocusPane {
                                        pane_id: tearout_pane_id,
                                    });
                        }
                        let _ = conn
                            .send_tx
                            .try_send(nexterm_proto::ClientToServer::BreakPane);
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                } else {
                    // A tab click switches pane focus.
                    let hit_pane = self
                        .app
                        .state
                        .tab_hit_rects
                        .iter()
                        .find(|&(_, &(x0, x1))| px_f32 >= x0 && px_f32 < x1)
                        .map(|(&id, _)| id);
                    if let Some(pane_id) = hit_pane {
                        let now = Instant::now();
                        // Double-click detection (same pane re-clicked within 300 ms).
                        let is_double_click = self
                            .last_tab_click
                            .map(|(t, id)| {
                                id == pane_id && now.duration_since(t) < Duration::from_millis(300)
                            })
                            .unwrap_or(false);

                        if is_double_click {
                            // Double-click → enter tab-rename mode.
                            let current_name = self
                                .app
                                .state
                                .panes
                                .get(&pane_id)
                                .map(|p| p.title.clone())
                                .filter(|t| !t.is_empty())
                                .unwrap_or_else(|| format!("pane:{}", pane_id));
                            self.app
                                .state
                                .settings_panel
                                .begin_tab_rename(pane_id, &current_name);
                            self.last_tab_click = None;
                        } else {
                            self.last_tab_click = Some((now, pane_id));
                            if self.app.state.focused_pane_id != Some(pane_id)
                                && let Some(conn) = &self.connection
                            {
                                let _ =
                                    conn.send_tx.try_send(ClientToServer::FocusPane { pane_id });
                            }
                            // Sprint 5-7 / Phase 2-3: record drag potential
                            // (committed=false). CursorMoved sets committed=true
                            // once the threshold is exceeded, and Released
                            // sends the reorder.
                            //
                            // Sprint 5-8 Phase 4-2 added fields:
                            // - `source_os_window_id`: the source OS window
                            //   (holds the primary window's id).
                            // - `start_screen_pos` / `current_screen_pos`:
                            //   global coordinates. On Windows we obtain them
                            //   from the OS via `platform::cursor_screen_pos`;
                            //   elsewhere we fall back to winit's
                            //   `outer_position` + client coordinates. On
                            //   Wayland `outer_position` is unavailable and
                            //   stays `None`, which disables the out-of-tab
                            //   drop test.
                            let screen_pos =
                                Self::resolve_screen_pos(&self.window, px as i32, py as i32);
                            self.app.state.tab_drag = Some(crate::state::TabDragState {
                                pane_id,
                                start_x: px_f32,
                                current_x: px_f32,
                                hover_target: Some(pane_id),
                                committed: false,
                                source_os_window_id: self.window.as_ref().map(|w| w.id()),
                                start_screen_pos: screen_pos,
                                current_screen_pos: screen_pos,
                            });
                        }
                    } else {
                        // Phase 4 (UI 4-tasks, 2026-06-12): the press landed in
                        // the tab bar but missed every interactive element
                        // (no tab, no settings button, no `[×]`, no `[↗]`). Treat
                        // it as a "grab the window" affordance the same way most
                        // native title bars do, so the user can reposition the
                        // window even when `WindowDecorations::None` hides the OS
                        // title bar.
                        //
                        // The pane body is intentionally excluded above (text
                        // selection lives there). Errors from `drag_window` are
                        // swallowed: backends that do not implement it (Wayland
                        // before xdg-shell drag, headless tests) should simply
                        // be a no-op rather than crash. winit's contract is that
                        // calling this during a pressed button starts an OS-driven
                        // drag-move loop that ends when the button is released —
                        // we therefore do *not* need an `on_mouse_left_released`
                        // counterpart for it.
                        if let Some(w) = &self.window {
                            let _ = w.drag_window();
                        }
                    }
                }
                return; // Do not pass tab-bar clicks to the terminal.
            }

            let col = (px / cell_w) as u16;
            let row = ((py - tab_bar_h_f64).max(0.0) / cell_h) as u16;

            // Phase 2c-E: command-block mouse hit-test. A click inside the
            // configured left-border width selects the block under the cursor;
            // a click in the badge cell at the prompt row toggles collapse.
            // Falls through to normal text selection when the click landed
            // anywhere else, or when the feature is disabled.
            if self.handle_block_mouse_click(px, py, tab_bar_h_f64, cell_w, cell_h) {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                return;
            }

            self.app.state.mouse_sel.begin(col, row);
            // When mouse reporting is enabled, send the event to the PTY.
            if let Some(conn) = &self.connection {
                let _ = conn.send_tx.try_send(ClientToServer::MouseReport {
                    button: 0,
                    col,
                    row,
                    pressed: true,
                    motion: false,
                });
            }
        }
    }

    /// Phase 2c-E: command-block mouse hit-test.
    ///
    /// Returns `true` when the click was consumed by a block interaction:
    /// - **Left-border**: a click within `BlocksConfig.effective_border_width_px`
    ///   of the pane's left edge selects the block whose row range covers the
    ///   clicked row. Idempotent — clicking the already-selected block returns
    ///   `true` but does not request the same redraw twice.
    /// - **Status badge / chevron**: a click in the rightmost cell of the row
    ///   that hosts a block's *prompt* toggles that block's collapse flag.
    ///   Only available when `show_exit_code_badge` is enabled (no visual cue
    ///   to click otherwise).
    ///
    /// `false` when the feature is disabled, when no block sits under the
    /// cursor, or when the click landed outside the grid area.
    fn handle_block_mouse_click(
        &mut self,
        px: f64,
        py: f64,
        tab_bar_h: f64,
        cell_w: f64,
        cell_h: f64,
    ) -> bool {
        let cfg = &self.app.config.blocks;
        if !cfg.enabled {
            return false;
        }
        // Below the tab bar (clicks in the tab bar are already handled above).
        if py < tab_bar_h {
            return false;
        }
        // The status bar at the bottom occupies the final cell row in the
        // fallback renderer — exclude clicks there.
        let win_h = self
            .window
            .as_ref()
            .map(|w| w.inner_size().height as f64)
            .unwrap_or(0.0);
        if win_h > 0.0 && py >= win_h - cell_h {
            return false;
        }
        let win_w = self
            .window
            .as_ref()
            .map(|w| w.inner_size().width as f64)
            .unwrap_or(0.0);

        let Some(pane_id) = self.app.state.focused_pane_id else {
            return false;
        };
        let Some(pane) = self.app.state.panes.get(&pane_id) else {
            return false;
        };
        if pane.blocks.is_empty() {
            return false;
        }

        let visual_row = ((py - tab_bar_h) / cell_h) as usize;
        let abs_row = if pane.scroll_offset > 0 {
            match crate::command_blocks::resolve_clicked_scrollback_row(
                &pane.blocks,
                pane.scrollback.len(),
                pane.scroll_offset,
                visual_row,
            ) {
                Some(r) => r,
                None => return false,
            }
        } else {
            pane.scrollback.len() + visual_row
        };

        let Some(block) = crate::command_blocks::block_containing_row(&pane.blocks, abs_row) else {
            return false;
        };
        let block_id = block.id;
        let prompt_row = block.prompt_row;

        let border_w = cfg.effective_border_width_px() as f64;

        // 1. Chevron / badge cell: only clickable on the prompt row of the
        //    block, and only when the badge is actually being rendered.
        if cfg.show_exit_code_badge && win_w > 0.0 && abs_row == prompt_row {
            // The renderer places the glyph at `region_w - cell_w * 1.5`, so a
            // hit zone spanning the rightmost cell catches the click without
            // demanding pixel-perfect aim.
            if px >= win_w - cell_w * 2.0 && px < win_w {
                self.app.state.toggle_block_collapse_by_id(block_id);
                return true;
            }
        }

        // 2. Left border zone: a 1-px sliver is hard to hit, so widen the hit
        //    zone to at least 6 px regardless of the configured visual width.
        let border_hit_w = border_w.max(6.0);
        if px < border_hit_w {
            self.app.state.select_block_by_id(block_id);
            return true;
        }

        false
    }

    /// Left button release: finalize selection → copy to clipboard or switch focus.
    pub(super) fn on_mouse_left_released(&mut self) {
        // Phase 4 (UI/UX v2): finalize any pane-border resize drag.
        if self.app.state.pane_resize_drag.take().is_some() {
            if let Some(w) = &self.window {
                w.set_cursor(winit::window::CursorIcon::Default);
                w.request_redraw();
            }
            self.app.state.last_cursor_icon = winit::window::CursorIcon::Default;
            // Do not fall through to other release paths — a border-drag
            // release is not a click on the underlying terminal cell.
            return;
        }
        // End any settings-panel slider drag and save the settings.
        if self.app.state.settings_panel.drag_slider.take().is_some() {
            let _ = self.app.state.settings_panel.save_to_toml();
            self.app.state.settings_panel.dirty = false;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }

        // Phase 3 (UI 4-tasks, 2026-06-12): end any in-flight title-bar drag.
        // `end_drag` only clears the anchor — the accumulated `drag_offset`
        // sticks until the panel closes, so the new position persists.
        if self.app.state.settings_panel.is_dragging() {
            self.app.state.settings_panel.end_drag();
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }

        // Sprint 5-7 / Phase 2-3: end-of-tab-drag handling.
        // If committed, compute the new order and send ReorderPanes; if not
        // committed, treat it as a normal click (do nothing).
        //
        // Sprint 5-8 Phase 4-2: if dropped outside the tab bar
        // (hover_target=None) and committed, call `compute_drop_target` with
        // global coordinates and branch on the result:
        // - `SameWindow`: dropped on the pane area → do nothing (existing behavior).
        // - `OtherWindowTabBar`: dropped on another OS window's tab bar →
        //   Phase 4-4 will send `MovePaneToWindow`; currently log only.
        // - `NewWindow`: dropped outside every OS window → call
        //   `spawn_os_window`. As of Phase 4-2 this is a skeleton without the
        //   real implementation — log + fall back to the primary window.
        if let Some(drag) = self.app.state.tab_drag.take() {
            if drag.committed
                && let Some(target_id) = drag.hover_target
                && target_id != drag.pane_id
                && let Some(new_order) =
                    compute_reordered_tab_order(&self.app.state.tab_order, drag.pane_id, target_id)
                && let Some(conn) = &self.connection
            {
                let _ = conn.send_tx.try_send(ClientToServer::ReorderPanes {
                    pane_ids: new_order,
                });
            } else if drag.committed && drag.hover_target.is_none() {
                // Released outside the tab bar → out-of-tab-bar drop test (Phase 4-2).
                self.handle_tab_drag_drop_outside(&drag);
            }
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }

        // If a context menu is open, handle the click on it.
        if let Some((px, py)) = self.cursor_position
            && let Some(menu) = self.app.state.context_menu.take()
        {
            let cell_w = self.app.font.cell_width();
            let cell_h = self.app.font.cell_height();
            // Use the same value as the drawn width
            // (changing this misaligns drawing and click detection).
            let menu_w = 18.0 * cell_w;
            let fx = px as f32;
            let fy = py as f32;
            if fx >= menu.x && fx <= menu.x + menu_w {
                for (i, item) in menu.items.iter().enumerate() {
                    let item_y = menu.y + i as f32 * cell_h;
                    if fy >= item_y && fy < item_y + cell_h {
                        self.execute_context_menu_action(&item.action);
                        break;
                    }
                }
            }
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }

        if let Some((px, py)) = self.cursor_position {
            let cell_w = self.app.font.cell_width() as f64;
            let cell_h = self.app.font.cell_height() as f64;
            let tab_bar_h_f64 = if self.app.config.tab_bar.enabled {
                self.app.config.tab_bar.height as f64
            } else {
                0.0_f64
            };
            let click_col = (px / cell_w) as u16;
            let click_row = ((py - tab_bar_h_f64).max(0.0) / cell_h) as u16;

            // Finish the drag selection and copy the selected text.
            self.app.state.mouse_sel.update(click_col, click_row);
            self.app.state.mouse_sel.finish();

            if let Some(((sc, sr), (ec, er))) = self.app.state.mouse_sel.normalized() {
                // When a selection exists, extract the text and copy it to the clipboard.
                let text = if let Some(pane) = self.app.state.focused_pane() {
                    let mut lines = Vec::new();
                    for row_idx in sr..=er {
                        if let Some(row) = pane.grid.rows.get(row_idx as usize) {
                            let col_start = if row_idx == sr { sc as usize } else { 0 };
                            let col_end = if row_idx == er {
                                (ec + 1) as usize
                            } else {
                                row.len()
                            };
                            let line: String = row
                                [col_start.min(row.len())..col_end.min(row.len())]
                                .iter()
                                .map(|c| c.ch)
                                .collect();
                            lines.push(line.trim_end().to_string());
                        }
                    }
                    lines.join("\n")
                } else {
                    String::new()
                };

                if !text.is_empty()
                    && let Ok(mut clipboard) = arboard::Clipboard::new()
                {
                    let _ = clipboard.set_text(text);
                }
                // After selecting, return (do not switch pane focus).
                return;
            }

            // No selection (simple click): Ctrl+click opens a URL.
            // Goes through the consent flow per the SecurityConfig.external_url policy.
            if self.modifiers.control_key()
                && let Some(url) = self.find_url_at(click_col, click_row)
            {
                self.request_open_url(url);
                return;
            }

            // Find the pane that contains the click coordinates and move focus to it.
            let target_pane = self
                .app
                .state
                .pane_layouts
                .values()
                .find(|l| {
                    click_col >= l.col_offset
                        && click_col < l.col_offset + l.cols
                        && click_row >= l.row_offset
                        && click_row < l.row_offset + l.rows
                })
                .map(|l| l.pane_id);
            if let Some(pane_id) = target_pane
                && self.app.state.focused_pane_id != Some(pane_id)
                && let Some(conn) = &self.connection
            {
                let _ = conn.send_tx.try_send(ClientToServer::FocusPane { pane_id });
            }
        }
    }

    /// `WindowEvent::MouseWheel` — scroll the scrollback with the mouse wheel.
    pub(super) fn on_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as i32,
            MouseScrollDelta::PixelDelta(p) => {
                // Windows touchpads send PixelDelta. Accumulate and scroll one
                // row per cell height, carrying the remainder into the next
                // event.
                self.pixel_scroll_accumulator += p.y;
                let cell_h = self.app.font.cell_height() as f64;
                let lines = (self.pixel_scroll_accumulator / cell_h) as i32;
                self.pixel_scroll_accumulator -= lines as f64 * cell_h;
                lines
            }
        };
        if lines > 0 {
            self.app.state.scroll_up(lines as usize);
        } else if lines < 0 {
            self.app.state.scroll_down((-lines) as usize);
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::compute_reordered_tab_order;

    #[test]
    fn tab_drag_drop_on_self_returns_none() {
        let current = vec![1, 2, 3];
        assert!(compute_reordered_tab_order(&current, 2, 2).is_none());
    }

    #[test]
    fn tab_drag_move_right() {
        // Drop 1 onto 3 in [1, 2, 3]: the implementation "inserts at target_id's
        // position", so dropping 1 onto 3 yields [2, 1, 3]
        // (1 is inserted at the original position of target_id=3).
        let current = vec![1, 2, 3];
        let next = compute_reordered_tab_order(&current, 1, 3).unwrap();
        assert_eq!(next, vec![2, 1, 3]);
    }

    #[test]
    fn tab_drag_move_left() {
        // Drop 3 onto 1 in [1, 2, 3] → [3, 1, 2].
        let current = vec![1, 2, 3];
        let next = compute_reordered_tab_order(&current, 3, 1).unwrap();
        assert_eq!(next, vec![3, 1, 2]);
    }

    #[test]
    fn tab_drag_adjacent_right_drop_is_noop() {
        // Dropping 1 onto 2 in [1, 2] with the "insert at target" model yields
        // [1, 2], the same as `current`. Return None to avoid a network round
        // trip.
        let current = vec![1, 2];
        assert!(compute_reordered_tab_order(&current, 1, 2).is_none());
    }

    #[test]
    fn tab_drag_adjacent_left_drop_swaps() {
        // Drop 2 onto 1 in [1, 2]: from=1, target_pos=0, from > target_pos
        // → insert_at=0, new=[1] → [2, 1]. Adjacent swaps are only possible in
        // the left-drag direction.
        let current = vec![1, 2];
        let next = compute_reordered_tab_order(&current, 2, 1).unwrap();
        assert_eq!(next, vec![2, 1]);
    }

    #[test]
    fn tab_drag_unknown_ids_return_none() {
        let current = vec![1, 2, 3];
        assert!(compute_reordered_tab_order(&current, 99, 1).is_none());
        assert!(compute_reordered_tab_order(&current, 1, 99).is_none());
    }

    #[test]
    fn tab_drag_move_to_center_of_three() {
        // Drop 1 onto 3 in [1, 2, 3, 4, 5] → [2, 1, 3, 4, 5].
        let current = vec![1, 2, 3, 4, 5];
        let next = compute_reordered_tab_order(&current, 1, 3).unwrap();
        assert_eq!(next, vec![2, 1, 3, 4, 5]);

        // Drop 5 onto 2 → [1, 5, 2, 3, 4].
        let next = compute_reordered_tab_order(&current, 5, 2).unwrap();
        assert_eq!(next, vec![1, 5, 2, 3, 4]);
    }
}
