//! winit `ApplicationHandler` lifecycle hooks.
//!
//! Extracted from `event_handler.rs`:
//! - `on_new_events` — 60 fps timer setup
//! - `on_resumed` — window / wgpu initialization / server connection
//! - `on_about_to_wait` — server-message polling / hot-reload handling

use std::sync::Arc;
use std::time::{Duration, Instant};

use nexterm_proto::{ClientToServer, ServerToClient};
use tracing::{info, warn};
use winit::{
    dpi::PhysicalSize,
    event::StartCause,
    event_loop::{ActiveEventLoop, ControlFlow},
    window::Window,
};

use super::EventHandler;
use crate::connection::{Connection, ConnectionExt};
use crate::glyph_atlas::{GlyphAtlas, GlyphKey};
use crate::renderer::WgpuState;

impl EventHandler {
    /// `ApplicationHandler::new_events` implementation.
    pub(super) fn on_new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        // Poll PTY output every 16 ms (about 60 fps).
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(16),
        ));
    }

    /// `ApplicationHandler::resumed` implementation.
    pub(super) fn on_resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Create the window (apply transparency, blur, and decorations per config).
        use nexterm_config::WindowDecorations;
        let win_cfg = &self.app.config.window;
        let transparent = win_cfg.background_opacity < 1.0;
        let decorations = !matches!(win_cfg.decorations, WindowDecorations::None);

        // Sprint 5-11-1 / H1 PoC: the AccessKit Adapter must be created
        // **before the window is made visible** (see the docs for
        // `accesskit_winit::Adapter::new`). Therefore follow the order
        // `with_visible(false)` → Adapter init → `set_visible(true)`. If the
        // Adapter is not installed before visibility, the platform-side a11y
        // tree is not initialized correctly.
        let attrs = Window::default_attributes()
            .with_title("Nexterm")
            .with_inner_size(PhysicalSize::new(1280u32, 800u32))
            .with_transparent(transparent)
            .with_decorations(decorations)
            .with_visible(false);

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("Failed to create window"),
        );

        // Sprint 5-11-1 / H1 PoC: initialize the AccessKit Adapter (before visibility).
        //
        // `Adapter::with_event_loop_proxy` configures the activation /
        // action / deactivation handlers to send events through an
        // `EventLoopProxy<UserEvent>`. The proxy reaches the `user_event`
        // handler as `UserEvent::Accessibility(...)` via
        // `From<accesskit_winit::Event> for UserEvent`.
        //
        // **Default-on policy (Q2=a auto-detect)**: when no screen reader is
        // running, the platform keeps the adapter inactive, so the CPU/memory
        // overhead is essentially zero. An explicit opt-in is not required.
        let accesskit_adapter = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        info!("Initialized AccessKit Adapter (waiting for a screen reader to connect)");
        self.accesskit_adapter = Some(accesskit_adapter);

        // Adapter initialization is complete, so make the window visible.
        window.set_visible(true);

        // Set the application icon.
        {
            let icon_bytes = include_bytes!("../../../../assets/nexterm-source.png");
            if let Ok(img) = image::load_from_memory(icon_bytes) {
                let rgba = img.into_rgba8();
                let (iw, ih) = (rgba.width(), rgba.height());
                if let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), iw, ih) {
                    window.set_window_icon(Some(icon));
                }
            }
        }

        // Enable IME input.
        window.set_ime_allowed(true);

        // Read the DPI scale factor and re-apply it to the font. The font was
        // already built at scale 1.0 in `NextermApp::new`; rescaling reuses that
        // font system instead of running a second full system-font scan.
        let scale_factor = window.scale_factor() as f32;
        self.scale_factor = scale_factor;
        self.app
            .font
            .set_scale_factor(self.app.config.font.size, scale_factor);

        // Apply the Acrylic (frosted-glass) background (Windows 11 only).
        #[cfg(windows)]
        crate::platform::apply_acrylic_blur(&window);

        // Initialize wgpu asynchronously (requires a tokio runtime).
        let mut wgpu_state = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(WgpuState::new(Arc::clone(&window), &self.app.config.gpu))
        })
        .expect("Failed to initialize wgpu");

        // Load the background image (Sprint 5-7 / Phase 3-1). On failure, a warn log is emitted internally.
        wgpu_state.load_background(&self.app.config.window);

        let mut atlas =
            GlyphAtlas::new_with_config(&wgpu_state.device, self.app.config.gpu.atlas_size);

        // Pre-load printable ASCII characters (0x20-0x7E) into the glyph atlas.
        // This eliminates the first-keystroke delay and makes rendering smooth
        // from startup.
        for ch in ' '..='~' {
            for bold in [false, true] {
                let key = GlyphKey {
                    ch,
                    bold,
                    italic: false,
                    wide: false,
                };
                let (w, h, pixels) =
                    self.app
                        .font
                        .rasterize_char(ch, bold, false, [220, 220, 220, 255], false);
                if w > 0 && h > 0 {
                    atlas.get_or_insert(key, &pixels, w, h, &wgpu_state.queue);
                }
            }
        }

        // Compute the cell count from the window size and initialize state.
        // Exclude the tab bar (top) and status bar (bottom 1 cell) from the area.
        let size = window.inner_size();
        let cell_h_init = self.app.font.cell_height();
        let tab_bar_h_init = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f32
        } else {
            0.0
        };
        let status_bar_h_init = cell_h_init;
        let pad_x = self.app.config.window.padding_x as f32;
        let pad_y = self.app.config.window.padding_y as f32;
        let cols = ((size.width as f32 - pad_x * 2.0) / self.app.font.cell_width()).max(1.0) as u16;
        let rows = ((size.height as f32 - tab_bar_h_init - status_bar_h_init - pad_y * 2.0)
            / cell_h_init)
            .max(1.0) as u16;
        self.app.state.resize(cols, rows);

        self.window = Some(Arc::clone(&window));
        self.atlas = Some(atlas);
        self.wgpu_state = Some(wgpu_state);

        // Sprint 5-8 Phase 4-1 Step 1.5: pin the Quake-mode target OS window to
        // the **primary window**. Even after Phase 4-2 added multiple OS
        // windows, Quake show/hide always goes through the `target_window_id`
        // set here (other windows are intentionally never toggled into Quake).
        self.quake.target_window_id = Some(window.id());

        // Connect to the server and attach to the default session.
        //
        // The single-binary build (`nexterm` bin in `nexterm-client-gpu`) spawns
        // `nexterm_server::run_server` as an internal Tokio task. On a cold start
        // the IPC pipe/socket may not be listening yet (Windows `os error 2`).
        //
        // We do exactly ONE quick, non-blocking attempt here. A multi-second
        // blocking retry loop at this point would freeze the window *and* starve
        // the embedded server task (both run on the same Tokio runtime / main
        // thread), paradoxically delaying the pipe the client is waiting for. If
        // this first attempt fails, `on_about_to_wait` keeps retrying on a fixed
        // cadence until the server is up — the window stays responsive and the
        // server gets the CPU it needs to bind the pipe.
        if self.try_connect() {
            info!("Connected to nexterm server");
        } else {
            info!("Server not ready yet; will connect in the background");
        }
        self.last_connect_attempt = Some(std::time::Instant::now());

        info!("wgpu renderer initialized");
    }

    /// Cadence for background reconnection attempts while offline.
    const RECONNECT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

    /// P1-A diagnostic: log a `WARN` summary every N consecutive failures.
    /// At a 200 ms cadence, 25 attempts is roughly one summary every 5 s — enough
    /// to make a stuck offline state visible in the log without flooding it.
    const CONNECT_FAILURE_LOG_INTERVAL: u64 = 25;

    /// P2-1 (Sprint 5-14 / v1.7.8): show the "Connecting…" banner once the
    /// offline streak has lasted this long. Short enough that the user sees it
    /// when the server takes >1 s to come up, long enough that quick startups
    /// (~5 failed attempts at the 200 ms cadence) never flash it.
    const OFFLINE_BANNER_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(1000);

    /// Make a single, non-blocking attempt to connect to the embedded server and
    /// attach to the "main" session. Returns `true` on success.
    ///
    /// A failed `connect_gpu` returns immediately (the pipe/socket simply does
    /// not exist yet), so this never stalls the caller. Shared by the initial
    /// `on_resumed` attempt and the `on_about_to_wait` background retry.
    pub(super) fn try_connect(&mut self) -> bool {
        let cols = self.app.state.cols;
        let rows = self.app.state.rows;
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(Connection::connect_gpu())
        });
        match result {
            Ok(conn) => {
                // Attach to the session → notify the real size.
                let _ = conn.send_tx.try_send(ClientToServer::Attach {
                    session_name: "main".to_string(),
                });
                let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
                self.connection = Some(conn);
                // P1-A diagnostic: report how long the offline streak lasted so we can
                // tell "connected immediately" from "took 30 s of background retries".
                if self.connect_failure_count > 0 {
                    let elapsed = self
                        .connect_failure_started_at
                        .map(|t| t.elapsed())
                        .unwrap_or_default();
                    info!(
                        "Connected after {} failed attempt(s) over {:.1}s",
                        self.connect_failure_count,
                        elapsed.as_secs_f64()
                    );
                }
                self.connect_failure_count = 0;
                self.connect_failure_started_at = None;
                // P2-1: clear the offline banner if it was visible.
                self.app.state.offline_banner_since = None;
                true
            }
            Err(e) => {
                self.connect_failure_count = self.connect_failure_count.saturating_add(1);
                if self.connect_failure_count == 1 {
                    // First failure in this offline streak — surface the underlying
                    // error and start the offline timer. Subsequent attempts at the
                    // 200 ms cadence stay at `debug` until the WARN interval below.
                    self.connect_failure_started_at = Some(std::time::Instant::now());
                    info!("Initial connect attempt failed: {}", e);
                } else if self
                    .connect_failure_count
                    .is_multiple_of(Self::CONNECT_FAILURE_LOG_INTERVAL)
                {
                    let elapsed = self
                        .connect_failure_started_at
                        .map(|t| t.elapsed())
                        .unwrap_or_default();
                    tracing::warn!(
                        "Still offline after {} attempts over {:.1}s (last error: {})",
                        self.connect_failure_count,
                        elapsed.as_secs_f64(),
                        e
                    );
                } else {
                    tracing::debug!("connect attempt failed: {}", e);
                }

                // P2-1: once the offline streak has lasted past the threshold,
                // raise the visible "Connecting…" banner. The renderer reads
                // `offline_banner_since` and formats the elapsed seconds.
                if self.app.state.offline_banner_since.is_none()
                    && let Some(started) = self.connect_failure_started_at
                    && started.elapsed() >= Self::OFFLINE_BANNER_THRESHOLD
                {
                    self.app.state.offline_banner_since = Some(started);
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }

                false
            }
        }
    }

    /// `ApplicationHandler::about_to_wait` implementation.
    pub(super) fn on_about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Background reconnect: if the embedded server was not yet listening when
        // the window came up, keep retrying on a fixed cadence until it connects.
        // `on_new_events` keeps the loop awake (`WaitUntil(now + 16 ms)`), so this
        // fires ~60 times/sec without any extra redraw requests. This replaces the
        // old blocking startup retry that froze the window for ~3 s.
        if self.connection.is_none()
            && self.window.is_some()
            && self
                .last_connect_attempt
                .is_none_or(|t| t.elapsed() >= Self::RECONNECT_INTERVAL)
        {
            self.last_connect_attempt = Some(std::time::Instant::now());
            if self.try_connect() {
                info!("Connected to nexterm server (background reconnect)");
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }

        // Poll server messages and update state.
        // To satisfy the borrow checker, first collect the received messages
        // into a Vec and then process them.
        let mut had_messages = false;
        let mut messages = Vec::new();
        if let Some(conn) = &mut self.connection {
            while let Ok(msg) = conn.recv_rx.try_recv() {
                messages.push(msg);
                had_messages = true;
            }
        }
        for msg in messages {
            // Sprint 5-8 Phase 4-4 Step C: detect `WindowListChanged`. If a new
            // window was created by a drop outside the tab bar, request an OS
            // window spawn. This only fires when `pending_new_window_drop_pos`
            // is `Some` (manual break_pane or windows from other clients are
            // intentionally not spawned as new OS windows).
            if let ServerToClient::WindowListChanged { ref windows } = msg {
                let current_ids: std::collections::HashSet<u32> =
                    windows.iter().map(|w| w.window_id).collect();
                let new_ids: Vec<u32> = current_ids
                    .difference(&self.known_server_window_ids)
                    .copied()
                    .collect();
                if !new_ids.is_empty()
                    && let Some(pos) = self.pending_new_window_drop_pos.take()
                {
                    // Use the smallest ID (= the newest window); creating
                    // multiple at once is not expected.
                    let server_window_id = *new_ids.iter().min().expect("new_ids non-empty");
                    if let Err(e) =
                        self.proxy
                            .send_event(crate::renderer::UserEvent::SpawnOsWindow {
                                server_window_id,
                                pos: Some(pos),
                            })
                    {
                        tracing::warn!("Failed to send SpawnOsWindow UserEvent: {}", e);
                    } else {
                        tracing::info!(
                            "Sent new OS window spawn request (server_window_id={}, pos={:?})",
                            server_window_id,
                            pos
                        );
                    }
                }
                self.known_server_window_ids = current_ids;
            }

            // Handle sensitive-operation requests per the SecurityConfig policy (Sprint 4-1).
            match msg {
                ServerToClient::DesktopNotification {
                    pane_id,
                    title,
                    body,
                } => {
                    // Sprint 5-11-5: always notify the screen reader regardless
                    // of the consent setting. The SR is an accessibility
                    // channel rather than a substitute for OS notifications,
                    // so even when the consent policy suppresses the OS
                    // notification, the SR must still receive it.
                    self.app.state.add_alert(
                        crate::state::AlertKind::Notification,
                        pane_id,
                        title.clone(),
                        body.clone(),
                    );
                    self.handle_notification_request(pane_id, title, body);
                }
                ServerToClient::ClipboardWriteRequest { pane_id, text } => {
                    self.handle_clipboard_write_request(pane_id, text);
                }
                other => {
                    self.app.state.apply_server_message(other);
                }
            }
        }

        // If a BEL was received, request user attention on the window.
        if self.app.state.pending_bell {
            self.app.state.pending_bell = false;
            if let Some(w) = &self.window {
                w.request_user_attention(Some(winit::window::UserAttentionType::Informational));
            }
        }

        // Phase 4-5: process any pending window-close request
        // (QueryForegroundProcess response → show confirmation dialog or kill immediately).
        self.poll_pending_close_request(event_loop);

        // Poll for config hot-reload and apply the latest config.
        if let Some(rx) = &mut self.config_rx
            && let Ok(new_config) = rx.try_recv()
        {
            info!(
                "Config reloaded: font={} {}pt",
                new_config.font.family, new_config.font.size
            );
            // Sprint 5-13 / v1.7.7: forward the new config to the embedded
            // server's `SharedRuntimeConfig` so its dispatch layer (Lua hooks,
            // log policy, SSH hosts) picks up the change. Previously the
            // server spawned its own `notify::Watcher` over the same TOML —
            // having a single client-owned watcher do both halves of the work
            // removes the duplication observed in `nexterm-client.log.2026-06-05`.
            if let Some(runtime_cfg) = &self.runtime_cfg {
                let new_runtime = nexterm_server::RuntimeConfig::from_config(&new_config);
                runtime_cfg.store(std::sync::Arc::new(new_runtime));
            }
            // When the font size changes, rebuild the glyph atlas as well.
            let font_changed = self.app.config.font != new_config.font;
            self.app.config = new_config;
            if font_changed {
                self.app.font = crate::font::FontManager::new(
                    &self.app.config.font.family,
                    self.app.config.font.size,
                    &self.app.config.font.font_fallbacks,
                    self.scale_factor,
                    self.app.config.font.ligatures,
                );
                let atlas_size = self.app.config.gpu.atlas_size;
                if let Some(wgpu) = &self.wgpu_state {
                    self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
                }
            }
            had_messages = true;
        }

        // Poll for custom-shader file changes and rebuild the pipelines.
        if let Some(rx) = &mut self.shader_reload_rx
            && rx.try_recv().is_ok()
        {
            // Drain the channel to collapse multiple events into one.
            while rx.try_recv().is_ok() {}
            if let Some(wgpu) = &mut self.wgpu_state {
                wgpu.reload_shader_pipelines(&self.app.config.gpu);
            }
            had_messages = true;
        }

        // Re-evaluate the status bar every second and update the cache.
        if self.app.config.status_bar.enabled
            && self.last_status_eval.elapsed() >= Duration::from_secs(1)
            && let Some(eval) = &self.status_eval
        {
            // Fetch the focused pane's cwd and pack it into a WidgetContext
            // (Sprint 5-7 / UI-1-2: for the cwd / cwd_short / git_branch widgets).
            let cwd = self
                .app
                .state
                .focused_pane_id
                .and_then(|id| self.app.state.panes.get(&id))
                .and_then(|p| p.cwd.clone());
            let ctx = nexterm_config::WidgetContext {
                session_name: Some("main".to_string()),
                pane_id: self.app.state.focused_pane_id,
                cwd,
                workspace_name: Some(self.app.state.current_workspace.clone()),
            };
            let sep = &self.app.config.status_bar.separator;
            self.app.state.status_bar_text =
                eval.evaluate_with_context(&self.app.config.status_bar.widgets, &ctx, sep);
            self.app.state.status_bar_right_text =
                eval.evaluate_with_context(&self.app.config.status_bar.right_widgets, &ctx, sep);
            self.last_status_eval = Instant::now();
            had_messages = true;
        }

        // Sprint 5-7 / UI-1-4: detect key-hint overlay expiration.
        if let Some(deadline) = self.app.state.key_hint_visible_until
            && Instant::now() >= deadline
        {
            self.app.state.key_hint_visible_until = None;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        // Sprint 5-7 / UI-1-4 bug fix: detect prefix-mode expiration (2-second timeout).
        if let Some(deadline) = self.app.state.prefix_pending_until
            && Instant::now() >= deadline
        {
            self.app.state.prefix_pending_until = None;
        }

        // Poll for notifications from the update checker and show the banner.
        if self.update_rx.has_changed().unwrap_or(false)
            && let Some(ver) = self.update_rx.borrow_and_update().clone()
            && self.app.state.update_banner.is_none()
        {
            self.app.state.update_banner = Some(ver);
            had_messages = true;
        }

        if had_messages && let Some(w) = &self.window {
            w.request_redraw();
        }

        // Advance the settings-panel open/close animation
        // (assumes 60 fps: about 8 frames = 0.13 s).
        let sp = &mut self.app.state.settings_panel;
        if sp.is_open && sp.open_progress < 1.0 {
            sp.open_progress = (sp.open_progress + 0.15).min(1.0);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }

        // Sprint 5-7 / Phase 2-2: Quake-mode handling.
        // 1) Drain global-hotkey press events. Any press is treated as "toggle".
        // 2) Take a server-issued toggle request (`pending_quake_action`).
        // 3) Combine them and perform the window operation at most once per
        //    frame (even if multiple events fire in the same frame).
        self.handle_quake_tick();

        // Sprint 5-11-2 Step 2-5: live update of the AccessKit tree.
        // Internally applies 100 ms throttling and a state-hash comparison, so
        // calling this every frame is safe. When no SR is connected,
        // `update_if_active` is a no-op, so the overhead is essentially zero.
        self.update_accesskit_tree_if_needed();
    }

    /// Process Quake-mode toggle requests at most once per frame.
    pub(super) fn handle_quake_tick(&mut self) {
        // Drain hotkey presses.
        let hotkey_pressed = self.quake.drain_pressed();
        // Pending action from IPC.
        let pending = self.app.state.pending_quake_action.take();

        // If nothing to do, return early.
        if !hotkey_pressed && pending.is_none() {
            return;
        }

        // Merge: hotkey is always "toggle". If IPC specifies an action, that takes precedence.
        let action = pending.unwrap_or_else(|| "toggle".to_string());

        let Some(window) = self.window.as_ref().cloned() else {
            warn!("Received a Quake toggle request but the window is not initialized");
            return;
        };

        let cfg = self.app.config.quake_mode.clone();
        match action.as_str() {
            "toggle" => {
                if self.quake.visible {
                    crate::quake::hide_window(&window, &cfg, self.quake.saved.as_ref());
                    self.quake.visible = false;
                } else {
                    let saved = crate::quake::show_window(&window, &cfg);
                    if saved.is_some() {
                        self.quake.saved = saved;
                    }
                    self.quake.visible = true;
                }
            }
            "show" => {
                if !self.quake.visible {
                    let saved = crate::quake::show_window(&window, &cfg);
                    if saved.is_some() {
                        self.quake.saved = saved;
                    }
                    self.quake.visible = true;
                } else {
                    window.focus_window();
                }
            }
            "hide" => {
                if self.quake.visible {
                    crate::quake::hide_window(&window, &cfg, self.quake.saved.as_ref());
                    self.quake.visible = false;
                }
            }
            other => {
                warn!("Received unknown Quake action '{}'; ignoring", other);
            }
        }
        window.request_redraw();
    }
}
