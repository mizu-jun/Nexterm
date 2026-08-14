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

        // Hover hit-test for the cursor icon. Skipped while a drag is in
        // flight so we don't fight the existing affordances. Priority:
        // window outline (custom title bar resize) → settings panel
        // (default) → pane borders → OSC 22 pointer shape.
        if self.app.state.tab_drag.is_none()
            && let Some(w) = &self.window
        {
            let pad_x = self.app.config.window.padding_x as f32;
            let pad_y = self.app.config.window.padding_y as f32;
            let origin_x = pad_x;
            let origin_y = tab_bar_h_f64 as f32 + pad_y;
            let next_cursor =
                if let Some(dir) = self.custom_titlebar_resize_edge(position.x, position.y) {
                    // The window outline is never covered by UI, so it wins
                    // even while the settings panel is open.
                    crate::chrome_resize::resize_cursor(dir)
                } else if self.app.state.settings_panel.is_open {
                    // Keep the default cursor over the settings panel; this
                    // also clears a stale resize cursor when the pointer
                    // leaves the outline.
                    winit::window::CursorIcon::Default
                } else {
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
                    match hit {
                        Some(h) => match h.axis {
                            crate::state::PaneResizeAxis::Horizontal => {
                                winit::window::CursorIcon::EwResize
                            }
                            crate::state::PaneResizeAxis::Vertical => {
                                winit::window::CursorIcon::NsResize
                            }
                        },
                        // Over the grid area, honor the focused pane's OSC 22 pointer
                        // shape; above the tab bar keep the platform default.
                        None if position.y >= origin_y as f64 => {
                            self.app.state.focused_pane_pointer_icon()
                        }
                        None => winit::window::CursorIcon::Default,
                    }
                };
            if self.app.state.last_cursor_icon != next_cursor {
                w.set_cursor(next_cursor);
                self.app.state.last_cursor_icon = next_cursor;
            }
        }
        // Phase 3b (UI/UX v2): live theme preview. When the settings
        // panel is open AND the cursor is hovering a Theme color dot,
        // mark the dot index in `theme_hover_preview` so the renderer
        // can swap the colour scheme transiently. Anything else clears
        // the preview (mouse-leave reverts to the configured scheme).
        if self.app.state.settings_panel.is_open {
            let hit = self.hit_test_settings_panel(position.x as f32, position.y as f32);
            let new_preview = match hit {
                SettingsPanelHit::ThemeColor(idx) => Some(idx),
                _ => None,
            };
            if self.app.state.settings_panel.theme_hover_preview != new_preview {
                self.app.state.settings_panel.theme_hover_preview = new_preview;
            }

            // UI/UX v3 P1b: dwell tracking for tooltips. Only migrated
            // categories report a widget; everything else clears the dwell so
            // no stale tooltip lingers.
            use crate::renderer::overlay::widgets::settings_blocks::BLOCKS_CATEGORY;
            use crate::renderer::overlay::widgets::settings_font::FONT_CATEGORY;
            use crate::renderer::overlay::widgets::settings_profiles::PROFILES_CATEGORY;
            use crate::renderer::overlay::widgets::settings_security::SECURITY_CATEGORY;
            use crate::renderer::overlay::widgets::settings_ssh::SSH_CATEGORY;
            use crate::renderer::overlay::widgets::settings_startup::STARTUP_CATEGORY;
            use crate::renderer::overlay::widgets::settings_theme::{
                THEME_CATEGORY, THEME_SWATCH_BASE,
            };
            use crate::renderer::overlay::widgets::settings_window::WINDOW_CATEGORY;
            let hovered = match hit {
                SettingsPanelHit::ThemeColor(i) => {
                    Some((THEME_CATEGORY, THEME_SWATCH_BASE + i as u16))
                }
                SettingsPanelHit::ThemeRow(index) => Some((THEME_CATEGORY, index)),
                SettingsPanelHit::WindowRow(index) => Some((WINDOW_CATEGORY, index)),
                SettingsPanelHit::FontRow(index) => Some((FONT_CATEGORY, index)),
                SettingsPanelHit::StartupRow(index) => Some((STARTUP_CATEGORY, index)),
                SettingsPanelHit::BlocksRow(index) => Some((BLOCKS_CATEGORY, index)),
                SettingsPanelHit::SecurityRow(index) => Some((SECURITY_CATEGORY, index)),
                SettingsPanelHit::ProfilesRow(index) => Some((PROFILES_CATEGORY, index)),
                SettingsPanelHit::SshRow(index) => Some((SSH_CATEGORY, index)),
                _ => None,
            };
            let sp = &mut self.app.state.settings_panel;
            sp.hover_widget = hovered.map(|(category, index)| {
                crate::settings_panel::HoverDwell::enter(
                    sp.hover_widget,
                    category,
                    index,
                    std::time::Instant::now(),
                )
            });
        } else if self.app.state.settings_panel.theme_hover_preview.is_some()
            || self.app.state.settings_panel.hover_widget.is_some()
        {
            // Panel closed while a preview or a hover dwell was active (e.g.
            // Esc dismiss): clear both so the next open starts clean. Leaving
            // the dwell behind would make a tooltip reappear the instant the
            // panel reopens under a stationary cursor, skipping its delay.
            self.app.state.settings_panel.theme_hover_preview = None;
            self.app.state.settings_panel.hover_widget = None;
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

        // Custom title bar: hover tracking for the window buttons (the
        // renderer fills the hovered one — close gets the error colour).
        let prev_button = self.app.state.hovered_window_button;
        let new_button = if position.y < tab_bar_h_f64 {
            let px = position.x as f32;
            let hit = |rect: Option<(f32, f32)>| rect.is_some_and(|(x0, x1)| px >= x0 && px < x1);
            if hit(self.app.state.window_minimize_hit_rect) {
                Some(crate::state::WindowButton::Minimize)
            } else if hit(self.app.state.window_maximize_hit_rect) {
                Some(crate::state::WindowButton::Maximize)
            } else if hit(self.app.state.window_close_hit_rect) {
                Some(crate::state::WindowButton::Close)
            } else {
                None
            }
        } else {
            None
        };
        if prev_button != new_button {
            self.app.state.hovered_window_button = new_button;
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

    /// P1 (WT-like UX): open the new-tab profile dropdown just below the
    /// tab bar, left-aligned with the `▾` button (clamped to the window).
    /// Lists the configured `Config.profiles` followed by the WSL distros
    /// detected at startup, and reuses the context-menu machinery for
    /// rendering, hover, and click dispatch.
    fn open_new_tab_dropdown(&mut self, anchor_x: f64) {
        let cell_w = self.app.font.cell_width() as f64;
        let cell_h = self.app.font.cell_height() as f64;
        let mut profile_list: Vec<(String, String)> = self
            .app
            .config
            .profiles
            .iter()
            .map(|p| (p.name.clone(), p.icon.clone()))
            .collect();
        profile_list.extend(
            self.app
                .state
                .wsl_profiles
                .iter()
                .map(|p| (p.name.clone(), p.icon.clone())),
        );

        let tmp = ContextMenu::new_tab_dropdown(0.0, 0.0, &profile_list);
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
        let menu_w_px = ((max_label + max_hint + 5) as f64).max(16.0) * cell_w;
        let menu_h_px = item_count as f64 * cell_h;
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
        let tab_bar_h = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f64
        } else {
            0.0
        };
        let menu_x = anchor_x.min(win_w - menu_w_px).max(0.0) as f32;
        let menu_y = tab_bar_h.min(win_h - menu_h_px).max(0.0) as f32;
        self.app.state.context_menu =
            Some(ContextMenu::new_tab_dropdown(menu_x, menu_y, &profile_list));
        if let Some(w) = &self.window {
            w.request_redraw();
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
            // Custom title bar: right-click on the tab bar opens the native
            // window/system menu on Windows (the only platform where winit
            // supports `show_window_menu`). Other platforms fall through to
            // the in-app `ContextMenu::new_window_system_menu` below.
            #[cfg(windows)]
            {
                let tab_bar_h = if self.app.config.tab_bar.enabled {
                    self.app.config.tab_bar.height as f64
                } else {
                    0.0
                };
                if self.app.config.window.decorations.wants_custom_titlebar()
                    && py < tab_bar_h
                    && let Some(w) = &self.window
                {
                    w.show_window_menu(winit::dpi::PhysicalPosition::new(px, py));
                    return;
                }
            }
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

            // Custom title bar on non-Windows: the tab bar right-click gets
            // the in-app system-menu replacement (maximize-or-restore /
            // minimize / close). On Windows the native menu already
            // returned above, so this arm is effectively non-Windows only.
            let on_custom_titlebar = self.app.config.window.decorations.wants_custom_titlebar()
                && py < tab_bar_h
                && tab_bar_h > 0.0;
            let is_maximized = self.window.as_ref().is_some_and(|w| w.is_maximized());
            let build = |x: f32, y: f32| {
                if on_custom_titlebar {
                    return ContextMenu::new_window_system_menu(x, y, is_maximized);
                }
                match block_under_cursor {
                    Some((id, has_name)) => {
                        ContextMenu::new_for_block(x, y, &profile_list, id, has_name)
                    }
                    None => ContextMenu::new_default(x, y, &profile_list),
                }
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
    /// Resize-edge hit test for the custom title bar
    /// (`window.decorations = "notitle"`). Returns `None` when the custom
    /// title bar is off, the window is maximized (a maximized window is not
    /// resizable by its edges), or the cursor is not on the window outline.
    fn custom_titlebar_resize_edge(
        &self,
        px: f64,
        py: f64,
    ) -> Option<winit::window::ResizeDirection> {
        if !self.app.config.window.decorations.wants_custom_titlebar() {
            return None;
        }
        let w = self.window.as_ref()?;
        if w.is_maximized() {
            return None;
        }
        let size = w.inner_size();
        let border_px = 6.0 * w.scale_factor() as f32;
        let chrome_h = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f32
        } else {
            0.0
        };
        let excluded: Vec<(f32, f32)> = [
            self.app.state.window_minimize_hit_rect,
            self.app.state.window_maximize_hit_rect,
            self.app.state.window_close_hit_rect,
        ]
        .into_iter()
        .flatten()
        .collect();
        crate::chrome_resize::resize_edge_at(
            px as f32,
            py as f32,
            size.width as f32,
            size.height as f32,
            border_px,
            chrome_h,
            &excluded,
        )
    }

    pub(super) fn on_mouse_left_pressed(&mut self) {
        if let Some((px, py)) = self.cursor_position {
            // Custom title bar: a press on the window outline starts an
            // OS-driven resize. Checked before every other hit test — the
            // grab band overlaps the tab bar's top edge and the settings
            // panel's "click outside closes" region.
            if let Some(dir) = self.custom_titlebar_resize_edge(px, py) {
                if let Some(w) = &self.window
                    && let Err(e) = w.drag_resize_window(dir)
                {
                    tracing::warn!("drag_resize_window failed: {e}");
                }
                return;
            }
            // When the settings panel is open, run the hit test first.
            if self.app.state.settings_panel.is_open {
                let hit = self.hit_test_settings_panel(px as f32, py as f32);
                use crate::settings_panel::SliderType;
                match hit {
                    SettingsPanelHit::Outside => {
                        // Click outside the panel → close the panel.
                        self.app.state.settings_panel.close();
                    }
                    SettingsPanelHit::OpenConfigFile => {
                        // P4 (WT-like UX): open config.toml with the OS
                        // default editor (WT's "Open JSON file" equivalent).
                        crate::platform::open_config_file();
                    }
                    SettingsPanelHit::ResetCategory => {
                        // P2-A: reset the current category's panel fields to
                        // their defaults; the change reaches disk only via
                        // the existing save path (Cancel still reverts).
                        if self.app.state.settings_panel.reset_category_to_defaults()
                            && let Some(w) = &self.window
                        {
                            w.request_redraw();
                        }
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
                            // Phase B1: the new category has its own content
                            // height, so any prior scroll offset is stale.
                            self.app.state.settings_panel.scroll.reset();
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
                        // Click a theme color dot → commit. Phase 3b: clear
                        // the hover preview so the renderer falls back to
                        // the configured scheme path (now that the commit
                        // has updated `scheme_index`).
                        self.app.state.settings_panel.scheme_index = idx;
                        self.app.state.settings_panel.dirty = true;
                        self.app.state.settings_panel.theme_hover_preview = None;
                    }
                    SettingsPanelHit::ThemeRow(index) => {
                        // UI/UX v3 P1b: click focuses the row; clicking the
                        // follow-system row also flips it, matching the
                        // toggle-on-click convention the Window rows use.
                        use crate::renderer::overlay::widgets::settings_theme::THEME_FOLLOW_SYSTEM;
                        let sp = &mut self.app.state.settings_panel;
                        sp.theme_field_focus = index;
                        if index == THEME_FOLLOW_SYSTEM {
                            sp.colors_follow_system = !sp.colors_follow_system;
                            sp.dirty = true;
                        }
                    }
                    SettingsPanelHit::WindowRow(row) => {
                        // UI/UX v3 P1c: the same router the keyboard and the
                        // accessibility path use, so the three cannot drift.
                        use crate::renderer::overlay::widgets::action::WidgetAction;
                        use crate::renderer::overlay::widgets::settings_window::apply_window_action;
                        apply_window_action(
                            &mut self.app.state.settings_panel,
                            row,
                            WidgetAction::Activate,
                        );
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
                        // UI/UX v3 P1c: routed through the shared action
                        // router, so a click and a screen reader apply the
                        // same transition. Blocks still auto-saves.
                        use crate::renderer::overlay::widgets::action::WidgetAction;
                        use crate::renderer::overlay::widgets::settings_blocks::apply_blocks_action;
                        let sp = &mut self.app.state.settings_panel;
                        if apply_blocks_action(sp, row, WidgetAction::Activate) {
                            let _ = sp.save_to_toml();
                            sp.dirty = false;
                        }
                    }
                    SettingsPanelHit::SecurityRow(row) => {
                        // Click focuses the row; policy rows cycle forward and
                        // byte-cap rows start editing. Changes persist on
                        // Save/close rather than auto-saving.
                        use crate::renderer::overlay::widgets::action::WidgetAction;
                        use crate::renderer::overlay::widgets::settings_security::apply_security_action;
                        apply_security_action(
                            &mut self.app.state.settings_panel,
                            row,
                            WidgetAction::Activate,
                        );
                    }
                    SettingsPanelHit::ProfilesRow(row) => {
                        // UI/UX v3 P1c: the profile list had no mouse path at
                        // all before (selection was AccessKit-only). Clicking
                        // the cycler steps the active profile; clicking an
                        // entry selects it — the same router the screen
                        // reader uses.
                        use crate::renderer::overlay::widgets::action::WidgetAction;
                        use crate::renderer::overlay::widgets::settings_profiles::apply_profiles_action;
                        apply_profiles_action(
                            &mut self.app.state.settings_panel,
                            row,
                            WidgetAction::Activate,
                        );
                    }
                    SettingsPanelHit::SshRow(row) => {
                        // UI/UX v3 P1c: nothing on this tab was clickable
                        // before. An entry click selects it, a field click
                        // focuses (text fields also open their edit buffer),
                        // Add appends, Delete opens the confirmation dialog —
                        // all through the router the screen reader uses.
                        use crate::renderer::overlay::widgets::action::WidgetAction;
                        use crate::renderer::overlay::widgets::settings_ssh::apply_ssh_action;
                        apply_ssh_action(
                            &mut self.app.state.settings_panel,
                            row,
                            WidgetAction::Activate,
                        );
                    }
                    SettingsPanelHit::FontRow(row) => {
                        // UI/UX v3 P1c: the Font rows had no click handling
                        // before — only the size slider reacted.
                        use crate::renderer::overlay::widgets::action::WidgetAction;
                        use crate::renderer::overlay::widgets::settings_font::apply_font_action;
                        apply_font_action(
                            &mut self.app.state.settings_panel,
                            row,
                            WidgetAction::Activate,
                        );
                    }
                    SettingsPanelHit::StartupRow(row) => {
                        // UI/UX v3 P1c: likewise new — the Startup rows were
                        // keyboard-only.
                        use crate::renderer::overlay::widgets::action::WidgetAction;
                        use crate::renderer::overlay::widgets::settings_startup::apply_startup_action;
                        apply_startup_action(
                            &mut self.app.state.settings_panel,
                            row,
                            WidgetAction::Activate,
                        );
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
                let hit_rect = |rect: Option<(f32, f32)>| {
                    rect.is_some_and(|(x0, x1)| px_f32 >= x0 && px_f32 < x1)
                };
                // Custom title bar: window buttons first (they occupy the
                // true right edge, outside every other tab-bar control).
                let hit_minimize = hit_rect(self.app.state.window_minimize_hit_rect);
                let hit_maximize = hit_rect(self.app.state.window_maximize_hit_rect);
                let hit_close = hit_rect(self.app.state.window_close_hit_rect);
                // Hit test for the settings button.
                let hit_settings = self
                    .app
                    .state
                    .settings_tab_rect
                    .map(|(x0, x1)| px_f32 >= x0 && px_f32 < x1)
                    .unwrap_or(false);
                if hit_minimize {
                    if let Some(w) = &self.window {
                        w.set_minimized(true);
                    }
                } else if hit_maximize {
                    // Skip while Quake mode drives the window geometry — a
                    // maximize toggle would fight its manual size management.
                    if !self.quake.visible
                        && let Some(w) = &self.window
                    {
                        w.set_maximized(!w.is_maximized());
                        w.request_redraw();
                    }
                } else if hit_close {
                    // Same path as the native close button so
                    // `window.close_action` (prompt / detach / quit) applies.
                    if let Some(w) = &self.window
                        && let Err(e) =
                            self.proxy
                                .send_event(crate::renderer::UserEvent::RequestClose {
                                    window_id: w.id(),
                                })
                    {
                        tracing::warn!("failed to send RequestClose UserEvent: {}", e);
                    }
                } else if hit_settings {
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
                } else if let Some((dd_x0, dd_x1)) = self.app.state.new_tab_dropdown_hit_rect
                    && px_f32 >= dd_x0
                    && px_f32 < dd_x1
                {
                    // P1 (WT-like UX): the `▾` button opens the new-tab
                    // profile dropdown (configured profiles + WSL distros).
                    self.open_new_tab_dropdown(dd_x0 as f64);
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
                        //
                        // Custom title bar: a double-click on the blank chrome
                        // toggles maximize, like a native title bar. Disabled
                        // while Quake mode drives the window geometry.
                        let now = Instant::now();
                        let is_double_click = self
                            .last_chrome_click
                            .map(|t| now.duration_since(t) < Duration::from_millis(300))
                            .unwrap_or(false);
                        let custom_titlebar =
                            self.app.config.window.decorations.wants_custom_titlebar();
                        if custom_titlebar && is_double_click && !self.quake.visible {
                            self.last_chrome_click = None;
                            if let Some(w) = &self.window {
                                w.set_maximized(!w.is_maximized());
                                w.request_redraw();
                            }
                        } else {
                            self.last_chrome_click = Some(now);
                            if let Some(w) = &self.window
                                && let Err(e) = w.drag_window()
                            {
                                // Backends without an OS drag-move loop
                                // (some Wayland compositors, headless) fail
                                // silently otherwise — surface it in the log
                                // so "the title bar doesn't drag" reports
                                // are diagnosable.
                                tracing::warn!("drag_window failed: {e}");
                            }
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
        // Phase B1: while the settings panel is open, the wheel scrolls its
        // content area instead of the terminal scrollback underneath it.
        if self.app.state.settings_panel.is_open {
            self.on_settings_panel_wheel(delta);
            return;
        }
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                (y * self.app.config.scrolling.effective_multiplier()) as i32
            }
            MouseScrollDelta::PixelDelta(p) => {
                // Windows touchpads send PixelDelta. Accumulate and scroll one
                // row per cell height, carrying the remainder into the next
                // event.
                self.pixel_scroll_accumulator += p.y;
                let cell_h = self.app.font.cell_height() as f64;
                let lines = (self.pixel_scroll_accumulator / cell_h) as i32;
                self.pixel_scroll_accumulator -= lines as f64 * cell_h;
                // Momentum: estimate the velocity (rows/s) from the raw pixel
                // deltas. Only pixel-precision (touchpad) events feed the
                // estimate — discrete wheels never get inertia.
                if self.app.config.scrolling.momentum {
                    let now = Instant::now();
                    if let Some(prev) = self.scroll_momentum.last_event {
                        let dt = now.duration_since(prev).as_secs_f64();
                        if dt > 0.0 && dt < 0.2 {
                            let inst = (p.y / cell_h) / dt;
                            let v = &mut self.scroll_momentum.velocity;
                            *v = 0.6 * *v + 0.4 * inst;
                        } else {
                            // A long gap means a new gesture; drop stale speed.
                            self.scroll_momentum.velocity = 0.0;
                        }
                    }
                    self.scroll_momentum.last_event = Some(now);
                    self.scroll_momentum.last_tick = None;
                    self.scroll_momentum.row_accum = 0.0;
                }
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

    /// Phase B1: route mouse-wheel scrolling to the settings panel's content
    /// area. Mirrors the line/pixel handling in `on_mouse_wheel` but drives
    /// `ScrollState` instead of the terminal scrollback. Wheel-up (positive
    /// `y`) scrolls toward the top of the content, matching typical GUI
    /// scroll-area convention.
    pub(super) fn on_settings_panel_wheel(&mut self, delta: MouseScrollDelta) {
        let cell_h = self.app.font.cell_height();
        let delta_px = match delta {
            MouseScrollDelta::LineDelta(_, y) => -y * cell_h * 3.0,
            MouseScrollDelta::PixelDelta(p) => -p.y as f32,
        };
        self.app.state.settings_panel.scroll.scroll_by(delta_px);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Advance the touchpad momentum coast (called once per redraw).
    ///
    /// While the fingers are still on the pad (events within the grace
    /// period) this only keeps the redraw loop alive; once events stop and
    /// the estimated velocity is meaningful, the scroll continues with
    /// exponential friction until it decays below the stop threshold.
    pub(super) fn tick_scroll_momentum(&mut self) {
        if !self.app.config.scrolling.momentum {
            return;
        }
        let Some(last_event) = self.scroll_momentum.last_event else {
            return;
        };
        let now = Instant::now();
        if now.duration_since(last_event).as_millis() < 60 {
            // Grace period — the gesture may still be in progress.
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }
        if self.scroll_momentum.velocity.abs() < MOMENTUM_STOP_ROWS_PER_SEC {
            self.scroll_momentum = ScrollMomentum::default();
            return;
        }
        let dt = self
            .scroll_momentum
            .last_tick
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(1.0 / 60.0)
            .min(0.1);
        self.scroll_momentum.last_tick = Some(now);
        let (rows, velocity, row_accum) = momentum_step(
            self.scroll_momentum.velocity,
            self.scroll_momentum.row_accum,
            dt,
        );
        self.scroll_momentum.velocity = velocity;
        self.scroll_momentum.row_accum = row_accum;
        if rows > 0 {
            self.app.state.scroll_up(rows as usize);
        } else if rows < 0 {
            self.app.state.scroll_down((-rows) as usize);
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

/// Touchpad momentum-scroll state (config `[scrolling] momentum`).
#[derive(Default)]
pub(in crate::renderer) struct ScrollMomentum {
    /// Estimated velocity in rows/second (EMA over recent PixelDelta events).
    pub(super) velocity: f64,
    /// Time of the most recent touchpad scroll event.
    pub(super) last_event: Option<Instant>,
    /// Time of the previous momentum tick while coasting.
    pub(super) last_tick: Option<Instant>,
    /// Fractional-row accumulator while coasting.
    pub(super) row_accum: f64,
}

/// Velocities below this many rows/second stop the coast.
const MOMENTUM_STOP_ROWS_PER_SEC: f64 = 1.0;

/// One friction step of the momentum coast: the velocity decays
/// exponentially (to ~5% within one second) and fractional rows carry over
/// between ticks. Returns `(whole rows to scroll, new velocity, new
/// fractional accumulator)`. Pure so it can be unit-tested without winit.
fn momentum_step(velocity: f64, row_accum: f64, dt: f64) -> (i32, f64, f64) {
    let new_velocity = velocity * 0.05_f64.powf(dt);
    let total = row_accum + new_velocity * dt;
    let rows = total.trunc() as i32;
    (rows, new_velocity, total - rows as f64)
}

#[cfg(test)]
mod momentum_tests {
    use super::momentum_step;

    #[test]
    fn velocity_decays_exponentially() {
        let (_, v1, _) = momentum_step(100.0, 0.0, 0.5);
        let (_, v2, _) = momentum_step(v1, 0.0, 0.5);
        assert!(v1 < 100.0, "velocity must shrink");
        assert!(v2 < v1);
        // ~5% left after a full second of coasting.
        assert!((v2 - 5.0).abs() < 1.0, "expected ≈5, got {v2}");
    }

    #[test]
    fn fractional_rows_carry_over_between_ticks() {
        // Slow coast: each tick produces less than one row, but the
        // accumulator eventually crosses a whole row.
        let mut v = 10.0;
        let mut accum = 0.0;
        let mut total_rows = 0;
        for _ in 0..30 {
            let (rows, nv, na) = momentum_step(v, accum, 1.0 / 60.0);
            v = nv;
            accum = na;
            total_rows += rows;
        }
        // Analytically: ∫10·0.05^t dt over 0.5 s ≈ 2.6 rows.
        assert!(
            total_rows >= 2,
            "expected a couple of rows, got {total_rows}"
        );
        assert!(accum.abs() < 1.0, "accumulator keeps only the fraction");
    }

    #[test]
    fn negative_velocity_scrolls_the_other_way() {
        let (rows, v, _) = momentum_step(-120.0, 0.0, 0.1);
        assert!(rows < 0);
        assert!(v < 0.0 && v > -120.0);
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
