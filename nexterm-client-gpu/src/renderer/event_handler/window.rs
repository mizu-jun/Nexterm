//! Window- and IME-related handlers among `winit::WindowEvent` variants.
//!
//! Extracted from `event_handler.rs`:
//! - `on_close_requested`
//! - `on_resized` / `on_scale_factor_changed`
//! - `on_modifiers_changed`
//! - `on_ime`
//! - `on_redraw_requested`

use nexterm_config::CloseAction;
use nexterm_proto::ClientToServer;
use tracing::{info, warn};
use winit::{event::Ime, event_loop::ActiveEventLoop, keyboard::ModifiersState, window::WindowId};

use super::EventHandler;
use crate::glyph_atlas::GlyphAtlas;
use crate::state::PendingCloseRequest;

/// Decide whether a fresh `QueryForegroundProcess` should be sent for an
/// incoming close request.
///
/// Returns `false` when a close is already pending for the same window, so
/// repeated `CloseRequested` events — for example the user clicking the
/// window's close button several times before the confirmation dialog
/// responds — do not spam the IPC channel.
fn should_send_close_query(pending: &Option<PendingCloseRequest>, target_window_id: u32) -> bool {
    !matches!(pending, Some(req) if req.server_window_id == target_window_id)
}

impl EventHandler {
    /// `WindowEvent::CloseRequested`
    ///
    /// Sprint 5-8 Phase 4-4 introduced the 3-way branch; Phase 4-5 finalized the
    /// real implementation of `Prompt`.
    ///
    /// Behavior:
    /// - **`Prompt`** (default): send a `QueryForegroundProcess` IPC and defer.
    ///   Run detach / kill based on the response (or the dialog choice). While
    ///   waiting for the response, do not call `event_loop.exit()` — keep state
    ///   in `pending_close_request`.
    /// - **`Detach`**: keep the server window alive and disconnect only the client
    ///   (tmux-style detached session).
    ///   - In the single-binary configuration, the embedded server thread is also
    ///     told to shut down via `signal_server_shutdown`, so the practical effect
    ///     is the same as Kill. The distinction is meaningful only with
    ///     multi-process setups (e.g. `nexterm-ctl attach`).
    /// - **`Kill`**: destroy the server session with `KillSession` IPC and then
    ///   exit.
    ///
    /// Multi-window note: only the **main** window runs the prompt/detach/kill
    /// flow above, because closing it ends the application. A detached /
    /// additional OS window closes on its own via `close_os_window` without a
    /// prompt and without tearing down the rest of the app — matching the
    /// `CloseOsWindow` action. This is why the handler needs `window_id`:
    /// previously every close targeted the main window's state, so closing a
    /// detached window queried the wrong server window and showed its dialog on
    /// the main window, which looked unresponsive and invited repeated clicks.
    pub(super) fn on_close_requested(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId) {
        // Route a non-main (detached) window's close to the per-window path.
        let is_main_window = self.window.as_ref().map(|w| w.id()) == Some(window_id);
        if !is_main_window {
            info!(
                "CloseRequested: closing the detached OS window only (window_id={:?})",
                window_id
            );
            self.close_os_window(event_loop, window_id);
            return;
        }

        let action = self.app.config.window.close_action;
        // The session name is currently fixed ("main" is used for `Attach`).
        // When multi-session support lands, fetch this from
        // `EventHandler.current_session`.
        let session_name = "main".to_string();

        match action {
            CloseAction::Prompt => {
                // Phase 4-5: send QueryForegroundProcess and wait for the response.
                // Record the deferral in pending_close_request and do not call
                // event_loop.exit(). The response lands in
                // `foreground_process_status` via `apply_server_message` and is
                // consumed by `poll_pending_close_request` in about_to_wait.
                let target_window_id = self.app.state.focused_server_window_id;
                // Guard against repeated close requests for the same window
                // while one is already pending (e.g. the user clicking the
                // close button several times). Re-sending the query would spam
                // the IPC channel without changing the outcome.
                if !should_send_close_query(&self.app.state.pending_close_request, target_window_id)
                {
                    info!(
                        "CloseRequested: a close is already pending for window_id={}; ignoring the repeat",
                        target_window_id
                    );
                    return;
                }
                info!(
                    "CloseRequested: close_action = Prompt. Sending QueryForegroundProcess for window_id={}",
                    target_window_id
                );
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::QueryForegroundProcess {
                            window_id: target_window_id,
                        });
                }
                self.app.state.pending_close_request = Some(crate::state::PendingCloseRequest {
                    server_window_id: target_window_id,
                    close_action: crate::state::CloseActionKind::Prompt,
                });
                // Early return: finalize_close runs after the response arrives.
                return;
            }
            CloseAction::Detach => {
                info!(
                    "CloseRequested: close_action = Detach. Keeping the server window alive and disconnecting the client only"
                );
                // Do not send KillSession; disconnect only on the client side.
                // In the single-binary configuration, signal_server_shutdown()
                // ends things in practice.
            }
            CloseAction::Kill => {
                info!(
                    "CloseRequested: close_action = Kill. Destroying the server session and exiting"
                );
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::KillSession {
                        name: session_name.clone(),
                    });
                }
            }
        }

        // Tear down additional OS windows, disconnect, abort the server task, and exit.
        self.windows.clear();
        self.connection = None;
        self.signal_server_shutdown();
        event_loop.exit();
    }

    /// Handle the `pending_close_request` response / dialog confirmation
    /// (Sprint 5-8 Phase 4-5).
    ///
    /// Called every frame from `about_to_wait`. When the latest response in
    /// `foreground_process_status` matches `pending_close_request`, do the
    /// following:
    /// - No foreground process → exit immediately via the Kill path.
    /// - Has a foreground process → set `close_window_dialog` so the renderer
    ///   draws it.
    ///
    /// If `close_window_dialog` is already in a "confirmed" state (externally
    /// `selected_button = u8::MAX` for cancel, `selected_button = 0` for Kill),
    /// process that as well.
    pub(super) fn poll_pending_close_request(&mut self, event_loop: &ActiveEventLoop) {
        // 1. Handle the case where the dialog was "confirmed".
        let dialog_decision: Option<bool> = if let Some(dlg) = &self.app.state.close_window_dialog {
            // selected_button = 0xFF signals cancel, 0xFE signals Kill confirmed.
            match dlg.selected_button {
                0xFE => Some(true),  // Kill confirmed
                0xFF => Some(false), // Cancel
                _ => None,
            }
        } else {
            None
        };
        if let Some(kill) = dialog_decision {
            self.app.state.close_window_dialog = None;
            self.app.state.pending_close_request = None;
            if kill {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::KillSession {
                        name: "main".to_string(),
                    });
                }
                self.windows.clear();
                self.connection = None;
                self.signal_server_shutdown();
                event_loop.exit();
            }
            return;
        }

        // 2. Check whether the IPC response has arrived.
        let Some(req) = self.app.state.pending_close_request else {
            return;
        };
        let Some(status) = self.app.state.foreground_process_status else {
            return;
        };
        // Verify the window_id matches.
        if status.window_id != req.server_window_id {
            // Response for a different window → ignore (do not clear).
            return;
        }
        // Consume the response.
        self.app.state.foreground_process_status = None;

        if status.has_foreground {
            // Show the confirmation dialog.
            info!(
                "Foreground process detected: window_id={}; displaying confirmation dialog",
                req.server_window_id
            );
            // Fetch wording from the i18n keys. When a key is missing, `t`
            // returns the key itself, so the wording is assumed to be defined
            // on the i18n JSON side.
            let message = nexterm_i18n::fl!("close_window_confirm_foreground");
            let kill_label = nexterm_i18n::fl!("close_window_button_kill");
            let cancel_label = nexterm_i18n::fl!("close_window_button_cancel");
            self.app.state.close_window_dialog = Some(crate::state::CloseWindowDialog {
                server_window_id: req.server_window_id,
                message,
                kill_label,
                cancel_label,
                selected_button: 1, // Default focus to Cancel (the safer side).
            });
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        } else {
            // No foreground process → Kill immediately.
            info!("No foreground process: proceeding from Prompt to Kill");
            self.app.state.pending_close_request = None;
            if let Some(conn) = &self.connection {
                let _ = conn.send_tx.try_send(ClientToServer::KillSession {
                    name: "main".to_string(),
                });
            }
            self.windows.clear();
            self.connection = None;
            self.signal_server_shutdown();
            event_loop.exit();
        }
    }

    /// `WindowEvent::Resized`
    pub(super) fn on_resized(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        let cell_h_r = self.app.font.cell_height();
        let tab_bar_h_r = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f32
        } else {
            0.0
        };
        let pad_x_r = self.app.config.window.padding_x as f32;
        let pad_y_r = self.app.config.window.padding_y as f32;
        let cols =
            ((size.width as f32 - pad_x_r * 2.0) / self.app.font.cell_width()).max(1.0) as u16;
        let rows = ((size.height as f32 - tab_bar_h_r - cell_h_r - pad_y_r * 2.0) / cell_h_r)
            .max(1.0) as u16;
        if let Some(wgpu) = &mut self.wgpu_state {
            wgpu.resize(size);
        }
        self.app.state.resize(cols, rows);
        // Notify the server of the resize.
        if let Some(conn) = &self.connection {
            let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
        }
    }

    /// `WindowEvent::ScaleFactorChanged`
    pub(super) fn on_scale_factor_changed(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor as f32;
        self.app.font = crate::font::FontManager::new(
            &self.app.config.font.family,
            self.app.config.font.size,
            &self.app.config.font.font_fallbacks,
            self.scale_factor,
            self.app.config.font.ligatures,
        );
        // A scale change invalidates the glyphs, so recreate the atlas.
        let atlas_size = self.app.config.gpu.atlas_size;
        if let Some(wgpu) = &self.wgpu_state {
            let mut atlas = GlyphAtlas::new_with_config(&wgpu.device, atlas_size);
            atlas.update_capacity_hint(
                self.app.font.cell_width() as u32,
                self.app.font.cell_height() as u32,
            );
            self.atlas = Some(atlas);
        }
        // After the DPI change, recompute cols/rows to match the new cell size
        // and notify the server.
        if let Some(win) = &self.window {
            let size = win.inner_size();
            let cell_h_sf = self.app.font.cell_height();
            let tab_bar_h_sf = if self.app.config.tab_bar.enabled {
                self.app.config.tab_bar.height as f32
            } else {
                0.0
            };
            let pad_x_sf = self.app.config.window.padding_x as f32;
            let pad_y_sf = self.app.config.window.padding_y as f32;
            let cols =
                ((size.width as f32 - pad_x_sf * 2.0) / self.app.font.cell_width()).max(1.0) as u16;
            let rows = ((size.height as f32 - tab_bar_h_sf - cell_h_sf - pad_y_sf * 2.0)
                / cell_h_sf)
                .max(1.0) as u16;
            self.app.state.resize(cols, rows);
            if let Some(conn) = &self.connection {
                let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
            }
        }
    }

    /// `WindowEvent::ModifiersChanged`
    pub(super) fn on_modifiers_changed(&mut self, mods: ModifiersState) {
        self.modifiers = mods;
    }

    /// `WindowEvent::ThemeChanged` — Sprint 5-15 / UI/UX Modernization v2 Phase 3.
    ///
    /// Records the OS-reported light/dark preference so the next frame's
    /// [`nexterm_config::Config::effective_color_scheme`] can pick the
    /// matching built-in scheme when `colors_follow_system` is on. Triggers
    /// a redraw so the user sees the new theme immediately.
    pub(super) fn on_theme_changed(&mut self, theme: winit::window::Theme) {
        let is_dark = matches!(theme, winit::window::Theme::Dark);
        self.app.state.os_dark_mode = Some(is_dark);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// `WindowEvent::Ime` — handle IME input for Japanese, Chinese, and others.
    pub(super) fn on_ime(&mut self, ime_event: Ime) {
        // Phase 5-11-8 Step 8-3 (Sub-phase B): while editing an SSH field in the
        // settings panel, route IME input to TextInputState instead of the
        // terminal. Sub-phase A prepared TextInputState.preedit / insert_str.
        let ssh_editing = self.app.state.settings_panel.is_open
            && self.app.state.settings_panel.ssh_field_editing.is_some();

        if ssh_editing {
            match ime_event {
                Ime::Enabled => {
                    // Only signals IME enablement; preedit is set on the next Preedit event.
                }
                Ime::Preedit(text, _cursor_range) => {
                    // Sub-phase B: route preedit to the active editor (SSH or keybinding).
                    use crate::settings_panel::KeyEditMode;
                    if let Some(state) = self.app.state.settings_panel.ssh_field_editing.as_mut() {
                        state.preedit = if text.is_empty() { None } else { Some(text) };
                    } else if let Some(KeyEditMode::Text(state)) =
                        self.app.state.settings_panel.key_editing.as_mut()
                    {
                        state.preedit = if text.is_empty() { None } else { Some(text) };
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                Ime::Commit(text) => {
                    // Insert the committed text at the cursor position in the
                    // active editor. Phase 5-11-9 Sub-phase B extends the
                    // 5-11-8 SSH path to the keybinding key field.
                    use crate::settings_panel::KeyEditMode;
                    self.app.state.settings_panel.ssh_field_insert_str(&text);
                    self.app.state.settings_panel.key_field_insert_str(&text);
                    if let Some(state) = self.app.state.settings_panel.ssh_field_editing.as_mut() {
                        state.preedit = None;
                    }
                    if let Some(KeyEditMode::Text(state)) =
                        self.app.state.settings_panel.key_editing.as_mut()
                    {
                        state.preedit = None;
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                Ime::Disabled => {
                    use crate::settings_panel::KeyEditMode;
                    if let Some(state) = self.app.state.settings_panel.ssh_field_editing.as_mut() {
                        state.preedit = None;
                    }
                    if let Some(KeyEditMode::Text(state)) =
                        self.app.state.settings_panel.key_editing.as_mut()
                    {
                        state.preedit = None;
                    }
                }
            }
            // Keep the IME cursor area tracking the SSH field row.
            self.update_ime_cursor_area_for_ssh_field();
            return;
        }

        // Normal terminal path (forward to PTY).
        match ime_event {
            Ime::Enabled => {
                // IME became enabled (no special handling required).
            }
            Ime::Preedit(text, _cursor_range) => {
                // Store the in-progress text in state and request a redraw.
                if text.is_empty() {
                    self.app.state.ime_preedit = None;
                } else {
                    self.app.state.ime_preedit = Some(text);
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Ime::Commit(text) => {
                // Clear preedit and send the committed text to the PTY.
                self.app.state.ime_preedit = None;
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Ime::Disabled => {
                self.app.state.ime_preedit = None;
            }
        }
        // Update the IME cursor area to follow the focused pane's cursor.
        if let Some(pane) = self.app.state.focused_pane() {
            let cell_w = self.app.font.cell_width();
            let cell_h = self.app.font.cell_height();
            let ime_x = pane.cursor_col as f32 * cell_w;
            let ime_y = (pane.cursor_row + 1) as f32 * cell_h;
            if let Some(w) = &self.window {
                w.set_ime_cursor_area(
                    winit::dpi::PhysicalPosition::new(ime_x as i32, ime_y as i32),
                    winit::dpi::PhysicalSize::new(cell_w as u32, cell_h as u32),
                );
            }
        }
    }

    /// Phase 5-11-8 Step 8-3 (Sub-phase B): track the IME cursor area to the
    /// pixel position of the SSH field row while editing an SSH field.
    ///
    /// The formula uses the same layout logic as the SSH edit-row renderer in
    /// `overlay/settings.rs`. Keep the two in sync on any change.
    ///
    /// Called from `input_handler` on a successful `begin_ssh_field_edit` and
    /// on arrow-key cursor moves, so it is exposed as `pub(in crate::renderer)`.
    pub(in crate::renderer) fn update_ime_cursor_area_for_ssh_field(&self) {
        let Some(w) = &self.window else { return };
        let sp = &self.app.state.settings_panel;
        if !sp.is_open || sp.ssh_field_editing.is_none() {
            return;
        }
        // Do nothing for non-TextInput fields (port=3 / auth_type=5).
        let row_index = match sp.ssh_field_focus {
            1 => 0u32, // name
            2 => 1,    // host
            4 => 3,    // username
            _ => return,
        };
        let Some(state) = sp.ssh_field_editing.as_ref() else {
            return;
        };

        let inner = w.inner_size();
        let sw = inner.width as f32;
        let sh = inner.height as f32;
        let cell_w = self.app.font.cell_width();
        let cell_h = self.app.font.cell_height();

        // Reproduce the layout formula from overlay/settings.rs.
        let panel_w = (sw * 0.72).min(sw - cell_w * 4.0);
        let panel_h = (sh * 0.75).min(sh - cell_h * 4.0);
        let px = (sw - panel_w) / 2.0;
        let py = (sh - panel_h) / 2.0; // Use the fixed position (eased=1.0 equivalent) even during animation.
        let sidebar_w = cell_w * 18.0;
        let content_x = px + sidebar_w;
        let content_inner_x = content_x + cell_w;
        let title_h = cell_h * 1.4;
        let content_top = py + title_h + cell_h * 0.5;
        let host_count = sp.ssh_hosts.len() as f32;
        let fields_top = content_top + cell_h * (1.5 + host_count * 1.2 + 0.6);
        let row_y = fields_top + cell_h * (1.3 + row_index as f32 * 1.1);

        // Cursor position (14-cell prefix + display_cursor's character count).
        const PREFIX_COLS: f32 = 14.0;
        let cursor_byte = state.display_cursor();
        let display = state.display_string();
        let cursor_col = display
            .get(..cursor_byte.min(display.len()))
            .map(|s| s.chars().count() as f32)
            .unwrap_or(0.0);
        let ime_x = content_inner_x + cell_w * (PREFIX_COLS + cursor_col);
        // The IME candidate panel naturally appears below the row.
        // row_y + cell_h points just below the row.
        let ime_y = row_y + cell_h;

        w.set_ime_cursor_area(
            winit::dpi::PhysicalPosition::new(ime_x as i32, ime_y as i32),
            winit::dpi::PhysicalSize::new(cell_w as u32, cell_h as u32),
        );
    }

    /// `WindowEvent::RedrawRequested`
    pub(super) fn on_redraw_requested(&mut self) {
        // Sprint 5-15 / UI/UX Modernization v2 Phase 3: pick the effective
        // color scheme by combining the configured `colors` with the
        // OS-reported light/dark preference. `colors_follow_system = false`
        // (the default) keeps the configured scheme verbatim.
        let configured_scheme = self
            .app
            .config
            .effective_color_scheme(self.app.state.os_dark_mode);
        // Phase 3b (UI/UX v2): when the settings panel is open and the
        // mouse is hovering a Theme dot, render with the previewed
        // scheme instead of the configured one. Mouse-leave clears
        // `theme_hover_preview`, which reverts the renderer to the
        // configured scheme on the next frame; clicking a dot commits
        // via the existing `ThemeColor` hit handler.
        let effective_scheme = if self.app.state.settings_panel.is_open
            && let Some(idx) = self.app.state.settings_panel.theme_hover_preview
        {
            nexterm_config::ColorScheme::Builtin(crate::settings_panel::index_to_builtin_scheme(
                idx,
            ))
        } else {
            configured_scheme
        };
        if let (Some(wgpu), Some(atlas)) = (&mut self.wgpu_state, &mut self.atlas)
            && let Err(e) = wgpu.render(
                &mut self.app.state,
                &mut self.app.font,
                atlas,
                &self.app.config.tab_bar,
                &effective_scheme,
                self.app.config.gpu.fps_limit,
                self.app.config.window.background_opacity,
                &self.app.config.cursor_style,
                self.app.config.window.padding_x as f32,
                self.app.config.window.padding_y as f32,
                &self.app.config,
            )
        {
            warn!("Render error: {}", e);
        }

        // Dynamic GlyphAtlas growth: when full, recreate at 2× the size.
        // Temporarily move atlas out to avoid borrow conflicts.
        if let Some(mut atlas) = self.atlas.take() {
            if atlas.needs_grow
                && let Some(wgpu) = &self.wgpu_state
            {
                atlas = atlas.grow(&wgpu.device);
            }
            self.atlas = Some(atlas);
        }

        // Phase 5-11-8 Step 8-3 (Sub-phase B): while editing an SSH field, sync
        // the IME cursor area to the latest field position after the frame is
        // drawn. This keeps the IME candidate window in the correct place after
        // every editing operation — character insertion, cursor movement,
        // Backspace, and so on.
        if self.app.state.settings_panel.is_open
            && self.app.state.settings_panel.ssh_field_editing.is_some()
        {
            self.update_ime_cursor_area_for_ssh_field();
        }
    }

    /// `WindowEvent::DroppedFile` (Phase 5 / UI 4-tasks, 2026-06-12).
    ///
    /// Pastes the dropped file's path into the focused pane via the same IPC
    /// path used by clipboard paste (`ClientToServer::PasteText`). When the
    /// shell has bracketed-paste mode enabled the server wraps the text in
    /// `ESC [ 200 ~ … ESC [ 201 ~`, so paths with embedded special characters
    /// are not interpreted as keystrokes.
    ///
    /// winit fires one event per dropped file even when several files are
    /// dropped at once. `last_file_drop_at` lets us insert a single space
    /// between paths so the resulting command line is `file1 file2 file3`.
    pub(super) fn on_dropped_file(&mut self, path: std::path::PathBuf) {
        /// Maximum gap between two `DroppedFile` events to still be considered
        /// part of the same multi-file drop. winit fires them effectively
        /// back-to-back, so 500 ms is loose enough for slow runtimes and tight
        /// enough not to merge unrelated user gestures.
        const FILE_DROP_BATCH_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

        let formatted = format_dropped_path(&path);
        let now = std::time::Instant::now();
        let prepend_space = self
            .last_file_drop_at
            .is_some_and(|t| now.duration_since(t) <= FILE_DROP_BATCH_WINDOW);
        self.last_file_drop_at = Some(now);

        let text = if prepend_space {
            format!(" {}", formatted)
        } else {
            formatted
        };

        if let Some(conn) = &self.connection {
            // Failure is swallowed for the same reason as every other IPC
            // try_send in this module: the channel is unbounded in practice,
            // and a dropped frame is preferable to a panic in the UI thread.
            let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
            tracing::info!("Dropped file pasted as terminal input: {:?}", path);
        } else {
            tracing::warn!(
                "Dropped file ignored — no server connection yet: {:?}",
                path
            );
        }
    }
}

/// Phase 5 (UI 4-tasks, 2026-06-12): format a dropped-file path for pasting
/// into the terminal.
///
/// Wraps the path in double quotes when it contains whitespace or an embedded
/// double quote, escaping any embedded `"` with a backslash. This works for
/// bash / zsh / fish / PowerShell. Path separators are preserved verbatim —
/// on Windows the `\` characters are emitted as-is because PowerShell
/// tolerates them and the user is closest to the shell that can interpret
/// the original path. Pure function — covered by `format_dropped_path_tests`.
pub(super) fn format_dropped_path(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let needs_quote = s.chars().any(|c| c.is_whitespace() || c == '"');
    if needs_quote {
        let escaped = s.replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        s.into_owned()
    }
}

#[cfg(test)]
mod format_dropped_path_tests {
    use super::format_dropped_path;
    use std::path::Path;

    /// Plain ASCII paths without any shell-sensitive characters are passed
    /// through verbatim — no quotes, no escapes.
    #[test]
    fn plain_path_is_unquoted() {
        assert_eq!(
            format_dropped_path(Path::new("/tmp/foo.txt")),
            "/tmp/foo.txt"
        );
    }

    /// Spaces in the path must trigger double-quoting so the shell sees a
    /// single argument.
    #[test]
    fn path_with_space_gets_double_quoted() {
        assert_eq!(
            format_dropped_path(Path::new("/tmp/my file.txt")),
            "\"/tmp/my file.txt\""
        );
    }

    /// A path containing a literal double quote must escape it inside the
    /// double-quoted wrapper.
    #[test]
    fn path_with_double_quote_is_escaped() {
        assert_eq!(
            format_dropped_path(Path::new("/tmp/he said \"hi\".txt")),
            "\"/tmp/he said \\\"hi\\\".txt\""
        );
    }

    /// Tabs / newlines also count as whitespace and must trigger quoting.
    #[test]
    fn path_with_tab_is_quoted() {
        assert_eq!(
            format_dropped_path(Path::new("/tmp/with\ttab.txt")),
            "\"/tmp/with\ttab.txt\""
        );
    }

    /// Windows-style paths with backslashes go through unmodified — the
    /// receiving shell (PowerShell, cmd, WSL) can decide how to interpret
    /// them. Only the space-induced quoting applies.
    #[cfg(windows)]
    #[test]
    fn windows_path_with_space_quotes_but_keeps_backslashes() {
        assert_eq!(
            format_dropped_path(Path::new(r"C:\Users\Jane Doe\file.txt")),
            "\"C:\\Users\\Jane Doe\\file.txt\""
        );
    }

    /// Sanity check: a file with no extension and no spaces still passes
    /// through unquoted.
    #[test]
    fn extension_less_file_is_unquoted() {
        assert_eq!(format_dropped_path(Path::new("Makefile")), "Makefile");
    }
}

#[cfg(test)]
mod tests {
    use super::should_send_close_query;
    use crate::state::{CloseActionKind, PendingCloseRequest};

    fn pending(window_id: u32) -> Option<PendingCloseRequest> {
        Some(PendingCloseRequest {
            server_window_id: window_id,
            close_action: CloseActionKind::Prompt,
        })
    }

    #[test]
    fn sends_query_when_nothing_is_pending() {
        assert!(should_send_close_query(&None, 0));
    }

    #[test]
    fn skips_resend_when_same_window_is_already_pending() {
        // Repeated CloseRequested events for the same window must not spam IPC.
        assert!(!should_send_close_query(&pending(0), 0));
    }

    #[test]
    fn sends_query_for_a_different_window() {
        // A close request for a different window is independent and proceeds.
        assert!(should_send_close_query(&pending(0), 1));
    }
}
