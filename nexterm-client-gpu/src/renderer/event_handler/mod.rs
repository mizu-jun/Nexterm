//! winit event handler.
//!
//! Sprint 5-6 split the former `event_handler.rs` (1,318 lines) into seven submodules:
//! - `consent` — Sprint 4-1 consent flow for sensitive operations
//! - `settings_panel_hit` — settings-panel mouse hit-test
//! - `lifecycle` — `new_events` / `resumed` / `about_to_wait`
//! - `window` — window / IME / redraw events
//! - `mouse` — cursor move / click / wheel
//! - `keyboard` — key input
//!
//! Each submodule extends behavior via an `impl EventHandler` block. This file
//! holds the `EventHandler` struct and the `ApplicationHandler` trait
//! implementation (dispatch only).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use nexterm_config::{Config, StatusBarEvaluator};
use tracing::{info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy},
    keyboard::ModifiersState,
    window::{Window, WindowId},
};

use crate::connection::Connection;
use crate::glyph_atlas::GlyphAtlas;

use super::{ClientWindow, NextermApp, PerWindowViewState, WgpuState};

/// Asynchronous user event sent to the EventLoop (Sprint 5-8 Phase 4-4 / Sprint 5-11-1).
///
/// Used to issue "spawn/close an OS window" requests from contexts that do not
/// hold `&ActiveEventLoop`, such as mouse handlers or the network receive
/// thread. Firing one via `EventLoopProxy::send_event(...)` causes the next
/// winit event-loop iteration to invoke `user_event`, where processing can
/// safely happen with `&ActiveEventLoop` in hand.
///
/// Sprint 5-11-1 added the `Accessibility` variant. Because
/// `accesskit_winit::Adapter::new` requires
/// `EventLoopProxy<T: From<accesskit_winit::Event>>`, a `From` impl is provided
/// below. The three event kinds delivered as `accesskit_winit::WindowEvent`
/// are `InitialTreeRequested` / `ActionRequested` / `AccessibilityDeactivated`.
///
/// `Clone` is intentionally not derived (`accesskit_winit::Event` is not
/// clonable, and there is no need to clone UserEvent).
#[derive(Debug)]
pub enum UserEvent {
    /// Spawn a new OS window and tie it to a server-side window ID.
    ///
    /// - `server_window_id`: the server-side window ID to attach to
    /// - `pos`: desired position for the new window (`None` uses winit's default placement)
    SpawnOsWindow {
        server_window_id: u32,
        pos: Option<PhysicalPosition<i32>>,
    },
    /// Close the specified OS window. If it is the last one, exit the entire app.
    ///
    /// As of Phase 4-4, `on_close_requested` destroys all OS windows in bulk,
    /// so there is no direct firing path (suppress the dead_code warning).
    /// Phase 4-5 plans to fire this from the context-menu "Close this window
    /// only" action.
    #[allow(dead_code)]
    CloseOsWindow { window_id: WindowId },
    /// Sprint 5-11-1 / H1 PoC: event from the AccessKit platform adapter.
    ///
    /// Delivered when a screen reader connects (`InitialTreeRequested`), when
    /// the user acts via the screen reader (`ActionRequested`), or when the
    /// connection is lost (`AccessibilityDeactivated`).
    Accessibility(accesskit_winit::Event),
}

/// impl that satisfies the `T: From<accesskit_winit::Event>` bound required by
/// `accesskit_winit::Adapter::new`. Added in Sprint 5-11-1.
impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        UserEvent::Accessibility(event)
    }
}

// ---- submodules ----
mod accessibility;
mod consent;
mod keyboard;
mod lifecycle;
mod mouse;
mod settings_panel_hit;
mod window;

/// winit event handler.
pub struct EventHandler {
    pub(super) app: NextermApp,
    pub(super) wgpu_state: Option<WgpuState>,
    pub(super) atlas: Option<GlyphAtlas>,
    pub(super) window: Option<Arc<Window>>,
    pub(super) modifiers: ModifiersState,
    /// IPC connection to the server.
    pub(super) connection: Option<Connection>,
    /// Mouse cursor position (in pixels).
    pub(super) cursor_position: Option<(f64, f64)>,
    /// Receive channel for config hot-reload.
    pub(super) config_rx: Option<tokio::sync::mpsc::Receiver<Config>>,
    /// File-watcher handle (held so it is not stopped on drop).
    pub(super) _config_watcher: Option<notify::RecommendedWatcher>,
    /// Lua status-bar evaluator.
    pub(super) status_eval: Option<StatusBarEvaluator>,
    /// Last time the status bar was evaluated.
    pub(super) last_status_eval: Instant,
    /// Display DPI scale factor (obtained from winit).
    pub(super) scale_factor: f32,
    /// Shader-file change notification channel (`Some` = watching custom shaders).
    pub(super) shader_reload_rx: Option<tokio::sync::mpsc::Receiver<()>>,
    /// Shader-file watcher handle.
    pub(super) _shader_watcher: Option<notify::RecommendedWatcher>,
    /// Tab double-click detection (last click time and pane ID).
    pub(super) last_tab_click: Option<(Instant, u32)>,
    /// Shutdown channel for the embedded server thread (Sprint 5-13 / v1.7.7).
    ///
    /// Send `()` to ask the server task to stop cleanly. Replaces the previous
    /// `server_handle: tokio::task::JoinHandle<()>` + `abort()` design — the
    /// server now runs on its own OS thread with its own Tokio runtime so it
    /// is no longer a Tokio task we can abort. `Option` because the channel
    /// can only be consumed once; subsequent close paths must handle `None`.
    pub(super) server_shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Shared runtime config (Sprint 5-13 / v1.7.7).
    ///
    /// The client owns the `notify::Watcher` and the `SharedRuntimeConfig`,
    /// then hands a clone to the embedded server via
    /// `run_server_with_config_and_runtime`. When `config_rx` delivers a new
    /// `Config`, we also rebuild the runtime config and `store` it here so
    /// the server's dispatch layer picks up the change without a duplicate
    /// watcher.
    pub(super) runtime_cfg: Option<nexterm_server::SharedRuntimeConfig>,
    /// Accumulator buffer for touchpad precision scroll (PixelDelta).
    pub(super) pixel_scroll_accumulator: f64,
    /// Receive channel for notifications from the update checker
    /// (`Some(version)` = new version available).
    pub(super) update_rx: tokio::sync::watch::Receiver<Option<String>>,
    /// Quake-mode runtime (Sprint 5-7 / Phase 2-2).
    /// Held so the global-hotkey manager is not dropped.
    /// Combined with `pending_quake_action` (state) to centralize handling of
    /// hotkey presses and toggle requests via IPC.
    pub(super) quake: crate::quake::QuakeRuntime,
    /// HashMap for multi-OS-window support (Sprint 5-8 Phase 4-1 Step 1.2 skeleton).
    ///
    /// Currently kept empty. From Step 1.3 onward the `on_resumed` flow will
    /// migrate to use `ClientWindow` entries in `windows` as the primary data
    /// source. Until the migration is complete, the existing `window` /
    /// `wgpu_state` / `atlas` fields remain the primary data source.
    pub(super) windows: HashMap<WindowId, ClientWindow>,
    /// `EventLoopProxy<UserEvent>` (Sprint 5-8 Phase 4-4).
    /// Used to fire `UserEvent::SpawnOsWindow` and similar from mouse handlers
    /// or network threads.
    pub(super) proxy: EventLoopProxy<UserEvent>,
    /// Set of known window IDs, used on receipt of `WindowListChanged` to
    /// decide whether a "spawn new OS window" request should fire
    /// (Sprint 5-8 Phase 4-4 Step C). Diff against the window set notified by
    /// the server: if the client does not know about a window, spawn an OS
    /// window at the position in `pending_new_window_drop_pos`.
    pub(super) known_server_window_ids: std::collections::HashSet<u32>,
    /// Drop position when a drop outside the tab bar requested a new OS window
    /// (Sprint 5-8 Phase 4-4). When a new window ID is detected via
    /// `WindowListChanged`, an OS window is spawned at this position. Set back
    /// to `None` once consumed.
    pub(super) pending_new_window_drop_pos: Option<PhysicalPosition<i32>>,
    /// Sprint 5-11-1 / H1 PoC: AccessKit platform adapter (for the primary window).
    ///
    /// Initialized in `on_resumed` when the primary window is created. When a
    /// screen reader connects, the `InitialTreeRequested` event arrives via
    /// `user_event`, and `update_if_active` returns the node tree. From Phase
    /// 5-11-2 onward this expands to support all OS windows.
    pub(super) accesskit_adapter: Option<accesskit_winit::Adapter>,
    /// Sprint 5-11-2 Step 2-5: timestamp of the last AccessKit tree update
    /// (for 100 ms throttling).
    ///
    /// Read and updated by `update_accesskit_tree_if_needed` at the end of
    /// `on_about_to_wait`. When `None`, the next `about_to_wait` always tries
    /// to update.
    pub(super) last_tree_update_at: Option<Instant>,
    /// Sprint 5-11-2 Step 2-5: state hash of the most recently sent `ClientState`.
    ///
    /// Compared against the result of `compute_tree_state_hash(&state)`; calls
    /// `update_if_active` only when the value has changed. When `None`, force
    /// the initial send.
    pub(super) last_tree_hash: Option<u64>,
    /// Sprint 5-11-3: cache of grid-row hashes per pane.
    ///
    /// `compute_tree_state_hash` tracks only structural changes (tabs, panes,
    /// overlays), so output differences in the terminal body are detected
    /// separately via this field. `update_accesskit_tree_if_needed` computes
    /// `compute_grid_row_hashes` on each throttle tick and compares.
    pub(super) last_grid_row_hashes: std::collections::HashMap<u32, Vec<u64>>,
    /// Timestamp of the most recent server connection attempt.
    ///
    /// On a cold start the embedded `nexterm_server::run_server` task may not be
    /// listening on the IPC pipe/socket yet. Rather than blocking the main
    /// thread in a multi-second retry loop (which starves the server task and
    /// *delays* the very pipe being awaited), `on_resumed` makes one quick
    /// non-blocking attempt and `on_about_to_wait` retries on a fixed cadence
    /// (`RECONNECT_INTERVAL`) until the connection succeeds. `None` means no
    /// attempt has been made yet.
    pub(super) last_connect_attempt: Option<Instant>,
    /// P1-A diagnostic: number of consecutive failed `try_connect` attempts.
    ///
    /// Reset to 0 once the connection succeeds. Used to throttle reconnect
    /// logging so we surface the first failure (`INFO`) and periodic summaries
    /// (`WARN` every `CONNECT_FAILURE_LOG_INTERVAL` attempts) without flooding
    /// stderr at the 200 ms reconnect cadence.
    pub(super) connect_failure_count: u64,
    /// P1-A diagnostic: timestamp of the first failed `try_connect` attempt in
    /// the current offline streak. `None` while connected. Used to report the
    /// total offline duration once the connection finally succeeds.
    pub(super) connect_failure_started_at: Option<Instant>,
    /// Phase 5 (UI 4-tasks, 2026-06-12): timestamp of the most recent
    /// `WindowEvent::DroppedFile` event.
    ///
    /// winit delivers one event per dropped file even when the user drops
    /// several files at once, so we batch them in time. If a new drop arrives
    /// within `FILE_DROP_BATCH_WINDOW` of the previous one, the formatted path
    /// is prefixed with a single space so the resulting command line looks
    /// like `file1 file2 file3` instead of a concatenated `file1file2file3`.
    /// `None` until the first drop occurs in this process.
    pub(super) last_file_drop_at: Option<Instant>,
    /// Phase 1 (UI 4-tasks, 2026-06-12): whether the post-connect "initial size
    /// drift sync" pass has run.
    ///
    /// `on_resumed` derives the initial cols/rows from `window.inner_size()`
    /// *immediately* after creating the window with `with_visible(false)` +
    /// `with_inner_size(1280x800)`. On Windows + winit 0.30 the very first
    /// `inner_size()` reading sometimes lags behind the requested size, and
    /// because the actual size matches the request, no `WindowEvent::Resized`
    /// is delivered to correct it. The terminal then stays pinned at the PTY
    /// default of 80x24 and the un-tiled portion of the surface renders as
    /// grey. After the IPC connection comes up, this flag triggers a single
    /// idempotent recompute: if the real `inner_size()` now disagrees with the
    /// cached `state.cols/rows`, we resize the state and forward the new size
    /// to the server. Set to `true` after the first attempt regardless of
    /// whether a drift was found.
    pub(super) initial_size_synced: bool,
}

impl EventHandler {
    /// Sprint 5-13 / v1.7.7: ask the embedded server thread to stop cleanly.
    ///
    /// Replaces the previous `server_handle.abort()` calls. The shutdown
    /// channel can only be consumed once, so subsequent invocations are a
    /// no-op (the server is already on its way out, or running standalone
    /// without an embedded thread at all).
    pub(super) fn signal_server_shutdown(&mut self) {
        if let Some(tx) = self.server_shutdown_tx.take() {
            // The receiver may already be dropped if the server task exited on
            // its own (e.g. ipc::serve returned an error). Ignore the result.
            let _ = tx.send(());
        }
    }

    /// Spawn a new OS window and register it in the `windows` HashMap
    /// (Sprint 5-8 Phase 4-4 real implementation).
    ///
    /// Same pattern as the primary-window flow in `on_resumed`:
    /// 1. Create a winit `Window` via `event_loop.create_window(...)`.
    /// 2. Initialize the wgpu pipeline with `WgpuState::new` (also loads the background image).
    /// 3. Build a `PerWindowViewState { focused_server_window_id, .. Default::default() }`.
    /// 4. Assemble a `ClientWindow` and run `self.windows.insert(window_id, ...)`.
    ///
    /// Arguments:
    /// - `event_loop`: winit `ActiveEventLoop` (required to create the window).
    /// - `pos`: screen position for the new window. When `None`, use winit's default placement.
    /// - `server_window_id`: server-side window ID this new window will display (stored in `view_state`).
    ///
    /// Returns: the new window's `WindowId`. Returns `None` on window-creation or wgpu-init failure.
    ///
    /// **Note**: the glyph atlas, font, and server connection are shared at the
    /// EventHandler level, so the new OS window receives messages over the
    /// existing server connection (`self.connection`). The attach request via
    /// `connection.send_tx` is already issued by the call site (the
    /// `MovePaneToWindow` path in
    /// [[project_sprint5_8_phase4_3_progress]]), so this function does not
    /// send it.
    pub(super) fn spawn_os_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        pos: Option<PhysicalPosition<i32>>,
        server_window_id: u32,
    ) -> Option<WindowId> {
        use nexterm_config::WindowDecorations;

        let win_cfg = &self.app.config.window;
        let transparent = win_cfg.background_opacity < 1.0;
        let decorations = !matches!(win_cfg.decorations, WindowDecorations::None);

        // Sprint 5-11-2 Step 2-3: the AccessKit Adapter must be created **before** the window is made visible.
        // Same `with_visible(false)` → Adapter init → `set_visible(true)` sequence as `on_resumed`.
        let mut attrs = Window::default_attributes()
            .with_title(format!("Nexterm - Window {}", server_window_id))
            .with_inner_size(PhysicalSize::new(1280u32, 800u32))
            .with_transparent(transparent)
            .with_decorations(decorations)
            .with_visible(false);
        if let Some(p) = pos {
            attrs = attrs.with_position(p);
        }

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                warn!("Failed to create new OS window: {}", e);
                return None;
            }
        };
        window.set_ime_allowed(true);

        // Sprint 5-11-2 Step 2-3: initialize the AccessKit Adapter for the new OS window (before visibility).
        let accesskit_adapter = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        info!(
            "Initialized AccessKit Adapter for new OS window (window_id={:?})",
            window.id()
        );

        // Initialize wgpu asynchronously (requires a tokio runtime).
        let mut wgpu_state = match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(WgpuState::new(Arc::clone(&window), &self.app.config.gpu))
        }) {
            Ok(s) => s,
            Err(e) => {
                warn!("wgpu initialization failed for new OS window: {}", e);
                return None;
            }
        };
        wgpu_state.load_background(&self.app.config.window);

        // Adapter initialization is complete, so make the window visible.
        window.set_visible(true);

        let window_id = window.id();
        let view_state = PerWindowViewState {
            focused_server_window_id: server_window_id,
            ..Default::default()
        };

        self.windows.insert(
            window_id,
            ClientWindow {
                window: Arc::clone(&window),
                wgpu: wgpu_state,
                view_state,
                accesskit_adapter,
            },
        );

        info!(
            "spawn_os_window: created new OS window (window_id={:?}, server_window_id={}, pos={:?})",
            window_id, server_window_id, pos
        );

        // Request an immediate first-frame redraw.
        window.request_redraw();
        Some(window_id)
    }

    /// Destroy the OS window with the given `WindowId`
    /// (Sprint 5-8 Phase 4-4 real implementation).
    ///
    /// Behavior:
    /// 1. Take the matching `ClientWindow` out of `self.windows` and drop it
    ///    (releasing the wgpu surface and the window).
    /// 2. Determine whether the primary window was closed.
    /// 3. Only when **all OS windows have been closed**, abort the server task
    ///    and call `event_loop.exit()`. The app keeps running when only a
    ///    single additional window is closed.
    ///
    /// Arguments:
    /// - `event_loop`: used to call `exit()` when the last window closes.
    /// - `window_id`: ID of the window to destroy.
    pub(super) fn close_os_window(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId) {
        let removed = self.windows.remove(&window_id);
        if removed.is_some() {
            info!(
                "close_os_window: destroyed OS window (window_id={:?})",
                window_id
            );
        }

        // Was the primary window closed?
        let main_closed = self
            .window
            .as_ref()
            .map(|w| w.id() == window_id)
            .unwrap_or(true);

        // If the primary window was closed, also clear its reference.
        if main_closed && self.window.as_ref().map(|w| w.id()) == Some(window_id) {
            self.window = None;
            self.wgpu_state = None;
        }

        // Exit when all OS windows have been closed.
        if should_exit_after_window_close(self.windows.len(), self.window.is_some()) {
            self.connection = None;
            self.signal_server_shutdown();
            event_loop.exit();
        }
    }
}

/// Whether the process should exit after an OS window was closed.
///
/// The process exits only once no OS windows remain at all — neither the
/// main window nor any detached/additional ones. Closing a detached window
/// while others survive keeps the application running.
fn should_exit_after_window_close(additional_windows: usize, main_window_present: bool) -> bool {
    additional_windows == 0 && !main_window_present
}

impl ApplicationHandler<UserEvent> for EventHandler {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.on_new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.on_resumed(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.on_about_to_wait(event_loop);
    }

    /// Process requests delivered via `UserEvent` (Sprint 5-8 Phase 4-4).
    ///
    /// OS-window operations fired from mouse handlers or the network receive
    /// thread (which do not hold `&ActiveEventLoop`) are executed here with
    /// `&ActiveEventLoop` in hand.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::SpawnOsWindow {
                server_window_id,
                pos,
            } => {
                let _ = self.spawn_os_window(event_loop, pos, server_window_id);
            }
            UserEvent::CloseOsWindow { window_id } => {
                self.close_os_window(event_loop, window_id);
            }
            // Sprint 5-11-1 / H1 PoC: AccessKit event.
            // Sprint 5-11-2 Step 2-4: pass event_loop along to dispatch ActionRequested.
            UserEvent::Accessibility(ak_event) => {
                self.on_accesskit_event(ak_event, event_loop);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Sprint 5-11-1 / H1 PoC + Step 2-3: forward window events to the AccessKit Adapter.
        // Focus changes, cursor moves, and similar are handled inside the
        // adapter and fired as platform a11y events when needed. Some events
        // such as redraws are ignored by accesskit, but `process_event`
        // dispatches them appropriately, so it is safe to call on every event.
        //
        // Step 2-3 added multi-OS-window support: look up the appropriate
        // Adapter from `window_id`.
        // - Primary window: matches `self.window`'s ID → `self.accesskit_adapter`.
        // - Additional windows: `self.windows[window_id].accesskit_adapter`.
        let is_main = self.window.as_ref().map(|w| w.id()) == Some(window_id);
        if is_main {
            if let (Some(adapter), Some(window)) =
                (self.accesskit_adapter.as_mut(), self.window.as_ref())
            {
                adapter.process_event(window, &event);
            }
        } else if let Some(cw) = self.windows.get_mut(&window_id) {
            cw.accesskit_adapter.process_event(&cw.window, &event);
        }

        match event {
            WindowEvent::CloseRequested => {
                self.on_close_requested(event_loop, window_id);
            }
            WindowEvent::Resized(size) => {
                self.on_resized(size);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.on_scale_factor_changed(scale_factor);
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.on_modifiers_changed(mods.state());
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.on_cursor_moved(position);
            }
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state: ElementState::Pressed,
                ..
            } => {
                self.on_mouse_right_pressed();
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } => {
                self.on_mouse_left_pressed();
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } => {
                self.on_mouse_left_released();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.on_mouse_wheel(delta);
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } if key_event.state == ElementState::Pressed => {
                self.on_keyboard_input(key_event, event_loop);
            }
            WindowEvent::Ime(ime_event) => {
                self.on_ime(ime_event);
            }
            WindowEvent::RedrawRequested => {
                self.on_redraw_requested();
            }
            WindowEvent::DroppedFile(path) => {
                // Phase 5 (UI 4-tasks, 2026-06-12): the user dropped a file
                // onto the window — paste its (quoted if needed) path into
                // the focused pane.
                self.on_dropped_file(path);
            }
            _ => {}
        }

        // Request a redraw every frame.
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

#[cfg(test)]
mod close_lifecycle_tests {
    use super::should_exit_after_window_close;

    #[test]
    fn exits_only_when_no_windows_remain() {
        // No additional windows and no main window → exit the process.
        assert!(should_exit_after_window_close(0, false));
    }

    #[test]
    fn does_not_exit_while_main_window_remains() {
        assert!(!should_exit_after_window_close(0, true));
    }

    #[test]
    fn does_not_exit_while_additional_windows_remain() {
        // A detached window was closed but others (and/or main) still exist.
        assert!(!should_exit_after_window_close(2, false));
        assert!(!should_exit_after_window_close(2, true));
    }
}
