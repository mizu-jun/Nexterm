//! Sprint 5-11-1 / H1 PoC: AccessKit event handler.
//!
//! Receives `UserEvent::Accessibility(accesskit_winit::Event)` and sends back
//! the appropriate response (initial tree, action handling, deactivation).
//!
//! Phase 5-11-1 PoC scope:
//! - `InitialTreeRequested`: return a fixed tree when a screen reader connects.
//! - `ActionRequested`: log only (real actions land in Phase 5-11-2 and later).
//! - `AccessibilityDeactivated`: log only (resource release happens inside the
//!   adapter).
//!
//! Sprint 5-11-2 Step 2-5 added `update_accesskit_tree_if_needed`. Called at the
//! end of `on_about_to_wait` to perform live updates.

use std::time::{Duration, Instant};

use accesskit::{Action, ActionData, ActionRequest};
use nexterm_proto::ClientToServer;
use tracing::{debug, info};
use winit::event_loop::ActiveEventLoop;

use crate::accessibility::{
    NodeIdKind, build_tree_from_state, compute_grid_row_hashes, compute_tree_state_hash,
    decode_node_id, dispatch_settings_action,
};

use super::EventHandler;

/// Throttling interval for AccessKit live updates (100 ms, agreed under Q3=a).
///
/// `compute_tree_state_hash` and `update_if_active` run at most this often.
/// Screen-reader recognition lag is kept within roughly 100 ms.
const TREE_UPDATE_THROTTLE: Duration = Duration::from_millis(100);

impl EventHandler {
    /// Handle an event delivered by the AccessKit platform adapter.
    ///
    /// Sprint 5-11-2 Step 2-1: migrated from a fixed tree to a dynamic tree
    /// (reflecting `ClientState`).
    /// Sprint 5-11-2 Step 2-3: multi-OS-window support. Look up the relevant
    /// Adapter from `event.window_id`.
    ///
    /// **Design note**: tree contents are generated from a single `ClientState`
    /// instance, so every OS window returns the same tree. If "per-window
    /// views" are introduced in the future, extend this to consult
    /// `PerWindowViewState` and feed each Adapter a different tree.
    pub(super) fn on_accesskit_event(
        &mut self,
        event: accesskit_winit::Event,
        event_loop: &ActiveEventLoop,
    ) {
        // Compute the tree first to keep the adapter mut borrow and the state ref borrow separate.
        let tree_update_for_initial = matches!(
            event.window_event,
            accesskit_winit::WindowEvent::InitialTreeRequested
        )
        .then(|| build_tree_from_state(&self.app.state));

        // Look up the target Adapter by window_id, distinguishing primary from additional windows.
        let event_window_id = event.window_id;
        let is_main = self.window.as_ref().map(|w| w.id()) == Some(event_window_id);

        match event.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => {
                info!(
                    "AccessKit: screen reader connected; sending initial tree (window_id={:?})",
                    event_window_id
                );
                let tree_update = tree_update_for_initial
                    .expect("InitialTreeRequested arm should have precomputed the tree");
                if is_main {
                    if let Some(adapter) = self.accesskit_adapter.as_mut() {
                        adapter.update_if_active(|| tree_update);
                    }
                } else if let Some(cw) = self.windows.get_mut(&event_window_id) {
                    cw.accesskit_adapter.update_if_active(|| tree_update);
                }
            }
            accesskit_winit::WindowEvent::ActionRequested(request) => {
                self.handle_accesskit_action(request, event_loop);
            }
            accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                info!(
                    "AccessKit: screen reader disconnected (window_id={:?})",
                    event_window_id
                );
            }
        }
    }

    /// Map an AccessKit `ActionRequest` to an internal Nexterm operation and
    /// run it (Step 2-4).
    ///
    /// **Dispatch table**:
    ///
    /// | target_node | Action | Effect |
    /// |---|---|---|
    /// | `Tab { pane_id }` | `Focus` / `Click` | send `FocusPane` IPC + update `state.focused_pane_id` |
    /// | `Pane { pane_id }` | `Focus` / `Click` | same as above |
    /// | `CloseDialogKill` | `Click` / `Focus` | `selected_button = 0xFE` (Kill confirmed) |
    /// | `CloseDialogCancel` | `Click` / `Focus` | `selected_button = 0xFF` (Cancel confirmed) |
    /// | `ContextItem { idx }` | `Click` | reuse the existing `execute_context_menu_action` + close the menu |
    /// | `PaletteItem { idx }` | `Click` | reuse the existing `execute_action` + close the palette |
    /// | `PaletteSearch` | `SetValue(s)` | `palette.query = s` + reset selection |
    /// | `QuickSelectItem { idx }` | `Click` | copy `matches[idx].text` to the clipboard + `quick_select.exit()` |
    /// | `SettingsTab { idx }` | `Focus` / `Click` | switch category + `font_family_editing = false` |
    /// | `SettingsFontFamily` | `Click` | enter edit mode |
    /// | `SettingsFontFamily` | `SetValue(s)` | `font_family = s`, `dirty = true` |
    /// | `SettingsFontSize` | `SetValue(v)` | round to 0.5 + clamp to 8.0–32.0 |
    /// | `SettingsFontSize` | `Increment` / `Decrement` | `increase_font_size` / `decrease_font_size` |
    /// | `SettingsThemeScheme` | `Click` / `Increment` | `next_scheme` |
    /// | `SettingsThemeScheme` | `Decrement` | `prev_scheme` |
    /// | `SettingsWindowOpacity` | `SetValue(v)` | round to 0.05 + clamp to 0.1–1.0 |
    /// | `SettingsWindowOpacity` | `Increment` / `Decrement` | `increase_opacity` / `decrease_opacity` |
    /// | `SettingsStartupLanguage` | `Click` / `Increment` | `next_language` |
    /// | `SettingsStartupLanguage` | `Decrement` | `prev_language` |
    /// | `SettingsStartupAutoUpdate` | `Click` | toggle + `dirty = true` |
    /// | other | — | `debug!` log only |
    ///
    /// **Design notes**:
    /// - Reason for treating `Focus` like `Click`: screen readers
    ///   (NVDA / VoiceOver / Orca) move focus with a virtual cursor and that
    ///   already implies a control transfer. To deliver the same UX in
    ///   Nexterm, the Focus action also sends `FocusPane` IPC.
    /// - Values of `selected_button`: `window.rs::poll_pending_close_request`
    ///   consumes `0xFE` as Kill and `0xFF` as Cancel (reusing the existing
    ///   half-open contract).
    /// - The palette `idx` is the position within `filtered()`. The dynamic
    ///   tree expands in the same order, so simply pass the `PaletteAction.action`
    ///   string at that index to `execute_action`.
    /// - The ContextMenu `idx` is the position in `items` — a raw index.
    fn handle_accesskit_action(&mut self, request: ActionRequest, event_loop: &ActiveEventLoop) {
        let kind = decode_node_id(request.target_node);
        debug!(
            "AccessKit: received action action={:?}, target={:?} ({:?})",
            request.action, request.target_node, kind
        );

        // Settings-panel actions are delegated to the pure function
        // `dispatch_settings_action`. On match, request a redraw and return early.
        //
        // Phase 5-11-7: extended the route so the four fields added in
        // Phase 5-11-6 #6 (CursorStyle / PaddingX / PaddingY / PresentMode)
        // and the new Profiles entry (SettingsProfileItem) are handled by the
        // same delegate.
        if matches!(
            kind,
            NodeIdKind::SettingsTab { .. }
                | NodeIdKind::SettingsFontFamily
                | NodeIdKind::SettingsFontSize
                | NodeIdKind::SettingsThemeScheme
                | NodeIdKind::SettingsWindowOpacity
                | NodeIdKind::SettingsStartupLanguage
                | NodeIdKind::SettingsStartupAutoUpdate
                | NodeIdKind::SettingsCursorStyle
                | NodeIdKind::SettingsPaddingX
                | NodeIdKind::SettingsPaddingY
                | NodeIdKind::SettingsPresentMode
                | NodeIdKind::SettingsProfileItem { .. }
                | NodeIdKind::SettingsSshHostItem { .. }
                | NodeIdKind::SettingsSshFieldName
                | NodeIdKind::SettingsSshFieldHost
                | NodeIdKind::SettingsSshFieldPort
                | NodeIdKind::SettingsSshFieldUsername
                | NodeIdKind::SettingsSshFieldAuthType
        ) {
            let handled = dispatch_settings_action(
                &mut self.app.state.settings_panel,
                request.action,
                &kind,
                request.data,
            );
            if handled {
                info!(
                    "AccessKit: settings-panel action handled action={:?}, kind={:?}",
                    request.action, kind
                );
                self.request_redraw_if_window();
            } else {
                debug!(
                    "AccessKit: settings-panel action not handled action={:?}, kind={:?}",
                    request.action, kind
                );
            }
            return;
        }

        match (request.action, kind) {
            // ===== Tab / pane focus and click =====
            (Action::Focus | Action::Click, NodeIdKind::Tab { pane_id })
            | (Action::Focus | Action::Click, NodeIdKind::Pane { pane_id }) => {
                info!("AccessKit: focus request for pane_id={}", pane_id);
                self.app.state.focused_pane_id = Some(pane_id);
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::FocusPane { pane_id });
                }
                self.request_redraw_if_window();
            }

            // ===== Close-confirmation dialog =====
            (Action::Click | Action::Focus, NodeIdKind::CloseDialogKill) => {
                info!("AccessKit: CloseDialog Kill button confirmed");
                if let Some(dlg) = self.app.state.close_window_dialog.as_mut() {
                    // 0xFE = Kill confirmed (consumed by the next frame's poll_pending_close_request).
                    dlg.selected_button = if matches!(request.action, Action::Click) {
                        0xFE
                    } else {
                        0
                    };
                    self.request_redraw_if_window();
                }
            }
            (Action::Click | Action::Focus, NodeIdKind::CloseDialogCancel) => {
                info!("AccessKit: CloseDialog Cancel button confirmed");
                if let Some(dlg) = self.app.state.close_window_dialog.as_mut() {
                    dlg.selected_button = if matches!(request.action, Action::Click) {
                        0xFF
                    } else {
                        1
                    };
                    self.request_redraw_if_window();
                }
            }

            // ===== Context menu =====
            (Action::Click, NodeIdKind::ContextItem { idx }) => {
                let action = self
                    .app
                    .state
                    .context_menu
                    .as_ref()
                    .and_then(|m| m.items.get(idx))
                    .map(|item| item.action.clone());
                if let Some(action) = action {
                    info!(
                        "AccessKit: executing ContextMenu item {}: {:?}",
                        idx, action
                    );
                    // Close the menu before running the action (same order as the existing mouse-click path).
                    self.app.state.context_menu = None;
                    self.execute_context_menu_action(&action);
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: ContextMenu item idx={} out of range (menu may already be closed)",
                        idx
                    );
                }
            }
            (Action::Focus, NodeIdKind::ContextItem { idx }) => {
                if let Some(menu) = self.app.state.context_menu.as_mut()
                    && idx < menu.items.len()
                {
                    menu.hovered = Some(idx);
                    self.request_redraw_if_window();
                }
            }

            // ===== Command palette =====
            (Action::Click, NodeIdKind::PaletteItem { idx }) => {
                let action_id = self
                    .app
                    .state
                    .palette
                    .filtered()
                    .get(idx)
                    .map(|a| a.action.clone());
                if let Some(action_id) = action_id {
                    info!("AccessKit: executing Palette item {}: {}", idx, action_id);
                    // Same order as the existing Enter-key path: close → record history → execute.
                    self.app.state.palette.close();
                    self.app.state.palette.record_use(&action_id);
                    self.execute_action(&action_id, event_loop);
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Palette item idx={} out of range (may have disappeared due to a query change)",
                        idx
                    );
                }
            }
            (Action::Focus, NodeIdKind::PaletteItem { idx }) => {
                if self.app.state.palette.is_open && idx < self.app.state.palette.filtered().len() {
                    self.app.state.palette.selected = idx;
                    self.request_redraw_if_window();
                }
            }
            (Action::SetValue, NodeIdKind::PaletteSearch) => {
                if let Some(ActionData::Value(s)) = request.data {
                    info!("AccessKit: set Palette search string: {:?}", s.as_ref());
                    self.app.state.palette.query = s.into_string();
                    self.app.state.palette.selected = 0;
                    self.request_redraw_if_window();
                }
            }

            // ===== Quick Select (Step 2-2-h) =====
            //
            // A SR Click is treated the same as "label-key input confirmed a match"
            // (following the existing `accept` branch of `handle_quick_select_key`).
            // Focus is a non-destructive operation that only changes drawing state,
            // so just emit a debug log.
            (Action::Click, NodeIdKind::QuickSelectItem { idx }) => {
                let text = self
                    .app
                    .state
                    .quick_select
                    .matches
                    .get(idx)
                    .map(|m| m.text.clone());
                if let Some(text) = text {
                    info!("AccessKit: confirmed Quick Select item {}: {}", idx, text);
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                    self.app.state.quick_select.exit();
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Quick Select item idx={} out of range (may have already exited)",
                        idx
                    );
                }
            }

            // ===== Host Manager (Phase 5-11-6 #2) =====
            //
            // A SR Click is the same as the existing Enter-key path (the
            // `host_manager.is_open` branch in `input_handler/mod.rs`):
            // - `auth_type == "password"` → open `PasswordModal` (SR support
            //   for password input itself is a Phase 5-11-7 candidate).
            // - Otherwise → `record_connection` + `connect_ssh_host_new_tab`.
            // Focus only updates `host_manager.selected` (non-destructive).
            (Action::Click, NodeIdKind::HostItem { idx }) => {
                let host = self
                    .app
                    .state
                    .host_manager
                    .filtered()
                    .get(idx)
                    .map(|h| (*h).clone());
                if let Some(host) = host {
                    info!("AccessKit: confirmed Host item {}: {}", idx, host.name);
                    self.app.state.host_manager.close();
                    if host.auth_type == "password" {
                        self.app.state.host_manager.password_modal =
                            Some(crate::host_manager::PasswordModal::new(host));
                    } else {
                        self.app.state.host_manager.record_connection(&host);
                        self.connect_ssh_host_new_tab(&host);
                    }
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Host item idx={} out of range (host_manager may be closed)",
                        idx
                    );
                }
            }
            (Action::Focus, NodeIdKind::HostItem { idx }) => {
                if self.app.state.host_manager.is_open
                    && idx < self.app.state.host_manager.filtered().len()
                {
                    self.app.state.host_manager.selected = idx;
                    self.request_redraw_if_window();
                }
            }

            // ===== Macro Picker (Phase 5-11-6 #3) =====
            //
            // A SR Click matches the existing Enter-key path (the
            // `macro_picker.is_open` branch in `input_handler/mod.rs`):
            // `selected = idx` → take the MacroConfig via `selected_macro()` →
            // `close()` → send IPC `RunMacro`. Focus only updates
            // `macro_picker.selected`.
            (Action::Click, NodeIdKind::MacroItem { idx }) => {
                self.app.state.macro_picker.selected = idx;
                let mac = self
                    .app
                    .state
                    .macro_picker
                    .selected_macro()
                    .map(|m| (m.lua_fn.clone(), m.name.clone()));
                if let Some((fn_name, display_name)) = mac {
                    info!("AccessKit: executing Macro item {}: {}", idx, display_name);
                    self.app.state.macro_picker.close();
                    if let Some(conn) = &self.connection {
                        let _ = conn.send_tx.try_send(ClientToServer::RunMacro {
                            macro_fn: fn_name,
                            display_name,
                        });
                    }
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Macro item idx={} out of range (macro_picker may be closed)",
                        idx
                    );
                }
            }
            (Action::Focus, NodeIdKind::MacroItem { idx }) => {
                if self.app.state.macro_picker.is_open {
                    self.app.state.macro_picker.selected = idx;
                    self.request_redraw_if_window();
                }
            }

            // ===== Alert Dismiss (Phase 5-11-6 #4) =====
            //
            // A SR Click dismisses the alert immediately without waiting for
            // the TTL (5 seconds). `Action::Default` does not exist in
            // accesskit 0.24, so only `Click` is wired up.
            (Action::Click, NodeIdKind::Alert { seq }) => {
                if self.app.state.dismiss_alert(seq) {
                    info!("AccessKit: dismissed Alert seq={} immediately", seq);
                    self.request_redraw_if_window();
                } else {
                    debug!(
                        "AccessKit: Alert seq={} not found (may have already expired)",
                        seq
                    );
                }
            }

            // ===== Scroll (Phase 5-11-6 #5) =====
            //
            // A SR Scroll request on the PaneArea → call
            // `state.scroll_up/down(rows/2)`. Half-screen units, matching the
            // existing PageUp/PageDown key path.
            //
            // Design (matched to the direction of the state API):
            // - `Action::ScrollUp` = show older content = `state.scroll_up`
            //   (increase the offset).
            // - `Action::ScrollDown` = return toward the newest content =
            //   `state.scroll_down` (decrease the offset).
            (Action::ScrollUp, NodeIdKind::PaneArea) => {
                let lines = (self.app.state.rows as usize / 2).max(1);
                self.app.state.scroll_up(lines);
                self.request_redraw_if_window();
            }
            (Action::ScrollDown, NodeIdKind::PaneArea) => {
                let lines = (self.app.state.rows as usize / 2).max(1);
                self.app.state.scroll_down(lines);
                self.request_redraw_if_window();
            }

            // ===== Phase 5-11-7: terminal input buffer =====
            //
            // Send the string the SR user wrote via `SetValue` to the focused
            // pane through a `PasteText` IPC. After the write, the `value` in
            // the AccessKit tree returns to an empty string on the next
            // `update_accesskit_tree_if_needed` (because `build_base_nodes`
            // always constructs it with `set_value("")`).
            //
            // The Focus action has no side effects (so the virtual cursor
            // passing over does not trigger a write).
            (Action::SetValue, NodeIdKind::PaneInputBuffer) => {
                if let Some(ActionData::Value(s)) = request.data {
                    let text = s.into_string();
                    if text.is_empty() {
                        debug!("AccessKit: ignoring empty PaneInputBuffer SetValue");
                    } else if let Some(conn) = &self.connection {
                        info!(
                            "AccessKit: forwarding {} characters from PaneInputBuffer SetValue to PTY",
                            text.chars().count()
                        );
                        let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                        self.request_redraw_if_window();
                    }
                } else {
                    debug!(
                        "AccessKit: PaneInputBuffer SetValue received non-ActionData::Value payload"
                    );
                }
            }
            (Action::Focus | Action::Click, NodeIdKind::PaneInputBuffer) => {
                // No side effects. Ensures no value change occurs when the SR virtual cursor passes by.
                debug!("AccessKit: PaneInputBuffer Focus/Click (no side effect)");
            }

            // ===== Anything else =====
            (action, kind) => {
                debug!(
                    "AccessKit: unsupported (action, target) pair: action={:?}, kind={:?}",
                    action, kind
                );
            }
        }
    }

    /// Helper to request a redraw on the primary window only.
    /// Additional OS windows are not redrawn here (their trees are reflected
    /// on the next frame via `update_accesskit_tree_if_needed`).
    fn request_redraw_if_window(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Sprint 5-11-2 Step 2-5: live update of the AccessKit tree.
    ///
    /// **Call site**: the end of `on_about_to_wait`. Invoked after the per-frame
    /// state changes — server messages, config reload, hotkey handling, and so on
    /// — have been applied.
    ///
    /// **Update strategy**:
    /// 1. **Sprint 5-11-5**: drop alert entries that have outlived the TTL (5 s)
    ///    via `expire_alerts(now)`. Run this before throttling so expired
    ///    entries are removed immediately and the SR tree stays accurate.
    /// 2. Return early if less than `TREE_UPDATE_THROTTLE` (100 ms) has elapsed
    ///    since the last update (throttling).
    /// 3. Compute the current state fingerprint via
    ///    `compute_tree_state_hash(&self.app.state)`.
    /// 4. Return early if it matches the previous hash (no state change).
    /// 5. On change: call `update_if_active(|| tree)` on the primary window's
    ///    Adapter and on every additional window's Adapter. When the adapter
    ///    is inactive, the call is a no-op, so it is safe to invoke every time.
    ///
    /// **Note**: a separate `TreeUpdate` is built per Adapter. `TreeUpdate` is
    /// not `Clone`, but `build_tree_from_state` is light enough (O(N), typically
    /// ~50 µs) that this is fine even with multiple windows.
    ///
    /// **Design rationale**: Q3=a (100 ms throttle) + design (a) hash-based.
    /// Design (b), "explicitly call from each event", was rejected because it
    /// scatters the call sites.
    pub(super) fn update_accesskit_tree_if_needed(&mut self) {
        let now = Instant::now();

        // Sprint 5-11-5: drop expired alerts (run every frame, very cheap).
        self.app.state.expire_alerts(now);

        if let Some(last) = self.last_tree_update_at
            && now.duration_since(last) < TREE_UPDATE_THROTTLE
        {
            return;
        }
        self.last_tree_update_at = Some(now);

        // Detect structural changes (tabs, panes, overlays, alerts).
        let current_hash = compute_tree_state_hash(&self.app.state);
        let tree_changed = self.last_tree_hash != Some(current_hash);
        self.last_tree_hash = Some(current_hash);

        // Sprint 5-11-3: detect differences in the terminal body (grid row contents).
        // Even when terminal output does not change the structure (e.g. cargo
        // build / log streaming), re-sending nodes that carry `Live::Polite`
        // for the focused pane lets the SR announce the new text.
        let grid_changed = self.detect_grid_row_changes();

        if !tree_changed && !grid_changed {
            return; // No structural or content change.
        }

        // Adapter for the primary window.
        if let Some(adapter) = self.accesskit_adapter.as_mut() {
            let update = build_tree_from_state(&self.app.state);
            adapter.update_if_active(|| update);
        }
        // Adapters for additional OS windows (currently all windows share the same tree).
        for cw in self.windows.values_mut() {
            let update = build_tree_from_state(&self.app.state);
            cw.accesskit_adapter.update_if_active(|| update);
        }
    }

    /// Sprint 5-11-3: recompute the grid-row hashes for each pane and detect changes.
    ///
    /// Returns `true` when any pane's row-hash list changed. As a side effect,
    /// `last_grid_row_hashes` is replaced with the latest values.
    ///
    /// Pane deletions or additions (changes in the HashMap key set) also
    /// return `true`. Structural changes are typically caught by
    /// `compute_tree_state_hash`, but this double check keeps this field
    /// consistent.
    fn detect_grid_row_changes(&mut self) -> bool {
        use std::collections::HashMap;

        let panes = &self.app.state.panes;
        let mut new_hashes: HashMap<u32, Vec<u64>> = HashMap::with_capacity(panes.len());
        let mut changed = false;

        for (&pane_id, pane) in panes {
            let hashes = compute_grid_row_hashes(&pane.grid);
            if self.last_grid_row_hashes.get(&pane_id) != Some(&hashes) {
                changed = true;
            }
            new_hashes.insert(pane_id, hashes);
        }

        // Detect pane deletion as well (when new_hashes shrinks).
        if new_hashes.len() != self.last_grid_row_hashes.len() {
            changed = true;
        }

        self.last_grid_row_hashes = new_hashes;
        changed
    }
}
