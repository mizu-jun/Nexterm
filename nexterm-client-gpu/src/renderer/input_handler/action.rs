//! Action execution for key bindings and the context menu
//!
//! Extracted from `input_handler.rs`:
//! - `execute_action` — dispatch action strings from `config.keys`
//! - `execute_context_menu_action` — execute right-click menu entries

use nexterm_proto::ClientToServer;
use tracing::{debug, info};
use winit::event_loop::ActiveEventLoop;

use super::EventHandler;
use crate::state::ContextMenuAction;
use crate::vertex_util::grid_to_text;

impl EventHandler {
    /// Execute the action string coming from a key binding or CommandPalette.
    ///
    /// Sprint 5-11-2 Step 2-4: widened visibility to `pub(in crate::renderer)`
    /// so AccessKit ActionRequested can call it via `event_handler`.
    pub(in crate::renderer) fn execute_action(
        &mut self,
        action: &str,
        event_loop: &ActiveEventLoop,
    ) {
        match action {
            "Quit" => event_loop.exit(),
            "SearchScrollback" => self.app.state.start_search(),
            "SplitVertical" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitVertical);
                }
            }
            "SplitHorizontal" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitHorizontal);
                }
            }
            "FocusNextPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::FocusNextPane);
                }
            }
            "FocusPrevPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::FocusPrevPane);
                }
            }
            "ClosePane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ClosePane);
                }
            }
            "NewWindow" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::NewWindow);
                }
            }
            "Detach" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::Detach);
                }
            }
            "CommandPalette" => {
                self.app.state.toggle_palette();
            }
            "SetBroadcastOn" => {
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::SetBroadcast { enabled: true });
                }
            }
            "SetBroadcastOff" => {
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::SetBroadcast { enabled: false });
                }
            }
            "ToggleZoom" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ToggleZoom);
                }
            }
            "QuickSelect" => {
                if let Some(pane) = self.app.state.focused_pane() {
                    let rows = pane.grid.rows.clone();
                    self.app.state.quick_select.enter(&rows);
                }
            }
            "SwapPaneNext" => {
                // Get the next pane ID after the focused pane and swap with it
                if let Some(conn) = &self.connection {
                    // Find the neighbour of the focused pane in pane_layouts
                    let layouts: Vec<_> = self.app.state.pane_layouts.values().collect();
                    if layouts.len() >= 2 {
                        let focused = self.app.state.focused_pane_id.unwrap_or(0);
                        // Pick the non-focused pane whose pane_id is closest (next)
                        let target = layouts
                            .iter()
                            .filter(|l| l.pane_id != focused)
                            .map(|l| l.pane_id)
                            .min_by_key(|&id| if id > focused { id - focused } else { u32::MAX })
                            .or_else(|| {
                                layouts.iter().map(|l| l.pane_id).find(|&id| id != focused)
                            });
                        if let Some(target_id) = target {
                            let _ = conn.send_tx.try_send(ClientToServer::SwapPane {
                                target_pane_id: target_id,
                            });
                        }
                    }
                }
            }
            "SwapPanePrev" => {
                if let Some(conn) = &self.connection {
                    let layouts: Vec<_> = self.app.state.pane_layouts.values().collect();
                    if layouts.len() >= 2 {
                        let focused = self.app.state.focused_pane_id.unwrap_or(0);
                        let target = layouts
                            .iter()
                            .filter(|l| l.pane_id != focused)
                            .map(|l| l.pane_id)
                            .min_by_key(|&id| if id < focused { focused - id } else { u32::MAX })
                            .or_else(|| {
                                layouts.iter().map(|l| l.pane_id).find(|&id| id != focused)
                            });
                        if let Some(target_id) = target {
                            let _ = conn.send_tx.try_send(ClientToServer::SwapPane {
                                target_pane_id: target_id,
                            });
                        }
                    }
                }
            }
            "BreakPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::BreakPane);
                }
            }
            "ShowSettings" => {
                self.app.state.settings_panel.open();
            }
            "ShowHostManager" => {
                self.app
                    .state
                    .host_manager
                    .reload(self.app.config.hosts.clone());
                self.app.state.host_manager.open();
            }
            "ShowMacroPicker" => {
                self.app
                    .state
                    .macro_picker
                    .reload(self.app.config.macros.clone());
                self.app.state.macro_picker.open();
            }
            "SftpUploadDialog" => {
                self.app.state.file_transfer.open_upload();
            }
            "SftpDownloadDialog" => {
                self.app.state.file_transfer.open_download();
            }
            "ConnectSerialPrompt" => {
                // Connect using the first serial-port entry in the config.
                // Fall back to common defaults when no entry exists.
                if let Some(conn) = &self.connection {
                    let serial_cfg = self.app.config.serial_ports.first().cloned();
                    let (port, baud_rate, data_bits, stop_bits, parity) =
                        if let Some(cfg) = serial_cfg {
                            (
                                cfg.port,
                                cfg.baud_rate,
                                cfg.data_bits,
                                cfg.stop_bits,
                                cfg.parity,
                            )
                        } else {
                            // Platform defaults
                            #[cfg(unix)]
                            let default_port = "/dev/ttyUSB0".to_string();
                            #[cfg(windows)]
                            let default_port = "COM1".to_string();
                            (default_port, 115200, 8, 1, "none".to_string())
                        };
                    let _ = conn.send_tx.try_send(ClientToServer::ConnectSerial {
                        port,
                        baud_rate,
                        data_bits,
                        stop_bits,
                        parity,
                    });
                }
            }
            // Sprint 5-2 / B1: prompt jumps via OSC 133 semantic marks
            "JumpPrevPrompt" => {
                self.app.state.jump_prev_prompt();
            }
            "JumpNextPrompt" => {
                self.app.state.jump_next_prompt();
            }
            // Sprint 5-8 / Phase 4-5: tab-tearing actions (Wayland alternative UX)
            //
            // **`DetachToNewWindow`** — detach the currently focused pane into
            // a new OS Window. Starts from the same `BreakPane` used by the
            // Phase 4-2 out-of-tab-drop path, and sets
            // `pending_new_window_drop_pos` to `None` to indicate a
            // "mouse-coordinate-independent detach". On Wayland, global
            // coordinates are unavailable and drag detection is impossible,
            // so this action is the alternative entry point.
            //
            // Note: after `BreakPane` is sent and the server responds with
            // `WindowListChanged`, the new_ids detection logic in
            // `lifecycle::on_about_to_wait` sends `SpawnOsWindow`. Even when
            // `pending_new_window_drop_pos` is `Some(None)`, `take()` still
            // returns `Some(_)`, so the decision stands (on Wayland with
            // `pos = None` we let winit place the window).
            "DetachToNewWindow" => {
                info!("DetachToNewWindow: BreakPane + new OS Window spawn request");
                // Record pos = None (no off-screen hint, so Wayland works too)
                self.pending_new_window_drop_pos = Some(winit::dpi::PhysicalPosition::new(0, 0));
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::BreakPane);
                }
            }
            // **`CloseOsWindow`** — close only the current OS Window (does
            // not terminate the process). Emit the `CloseOsWindow` UserEvent;
            // `EventHandler::close_os_window` performs the close.
            // `event_loop.exit()` is only reached if this was the last OS Window.
            "CloseOsWindow" => {
                info!("CloseOsWindow: emitting UserEvent to close the current OS Window");
                if let Some(w) = &self.window {
                    let wid = w.id();
                    if let Err(e) = self
                        .proxy
                        .send_event(crate::renderer::UserEvent::CloseOsWindow { window_id: wid })
                    {
                        tracing::warn!("failed to send CloseOsWindow UserEvent: {}", e);
                    }
                }
            }
            // Phase 2c-F: palette `@<name>` selection. The action id is the
            // synthetic `BlockSelect:<u64>` string built by
            // `palette::build_named_block_actions`. Parse the suffix and
            // delegate to `ClientState::jump_to_block`, which both scrolls
            // the focused pane to the block's prompt row and updates
            // `selected_block`. Malformed suffixes (parse failure, unknown
            // id) silently no-op.
            other if other.starts_with("BlockSelect:") => {
                if let Ok(id) = other["BlockSelect:".len()..].parse::<u64>() {
                    let _ = self.app.state.jump_to_block(id);
                }
            }
            // Roadmap Phase 3: palette workspace actions. The switch entry
            // carries the target name; the server answers with
            // `WorkspaceSwitched` + `WorkspaceList`, which refresh the
            // palette entries.
            other if other.starts_with("WorkspaceSwitch:") => {
                let name = other["WorkspaceSwitch:".len()..].to_string();
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::SwitchWorkspace { name });
                }
            }
            // Creates `workspace-N` with the first free N, then switches to
            // it (kitty-style: a fresh workspace becomes active right away).
            // The generated name can be renamed later via `nexterm-ctl`.
            "WorkspaceCreate" => {
                let taken: Vec<&str> = self
                    .app
                    .state
                    .workspaces
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect();
                let name = (1..)
                    .map(|i| format!("workspace-{i}"))
                    .find(|c| !taken.contains(&c.as_str()))
                    .expect("unbounded counter always finds a free name");
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::CreateWorkspace { name: name.clone() });
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::SwitchWorkspace { name });
                }
            }
            _ => debug!("Execute action: {}", action),
        }
    }

    /// Execute a context-menu action
    pub(in crate::renderer) fn execute_context_menu_action(&mut self, action: &ContextMenuAction) {
        match action {
            ContextMenuAction::Copy => {
                // Copy the visible grid of the focused pane to the clipboard
                if let Some(pane) = self.app.state.focused_pane() {
                    let text = grid_to_text(pane);
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                }
            }
            ContextMenuAction::Paste => {
                if let Ok(mut clipboard) = arboard::Clipboard::new()
                    && let Ok(text) = clipboard.get_text()
                    && let Some(conn) = &self.connection
                {
                    let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                }
            }
            ContextMenuAction::SelectAll => {
                // Copy the entire grid text to the clipboard
                if let Some(pane) = self.app.state.focused_pane() {
                    let text = grid_to_text(pane);
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                }
            }
            ContextMenuAction::SplitVertical => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitVertical);
                }
            }
            ContextMenuAction::SplitHorizontal => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitHorizontal);
                }
            }
            ContextMenuAction::ClosePane => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ClosePane);
                }
            }
            ContextMenuAction::InlineSearch => {
                self.app.state.start_search();
            }
            ContextMenuAction::OpenSettings => {
                self.app.state.settings_panel.open();
            }
            ContextMenuAction::OpenProfile { profile_name } => {
                // Open a new tab running the profile's shell / cwd / env
                // (PROTOCOL_VERSION 11 `SplitWithShell`). Searches the
                // configured profiles first, then the WSL distros detected
                // at startup (the new-tab dropdown lists both).
                let profile = self
                    .app
                    .config
                    .profiles
                    .iter()
                    .chain(self.app.state.wsl_profiles.iter())
                    .find(|p| &p.name == profile_name)
                    .cloned();
                if let (Some(prof), Some(conn)) = (profile, &self.connection) {
                    let msg =
                        crate::state::split_message_for_profile(&prof, &self.app.config.shell);
                    info!("opening profile '{}' as a new tab: {:?}", profile_name, msg);
                    let _ = conn.send_tx.try_send(msg);
                }
            }
            ContextMenuAction::Separator => {
                // Separators are not clickable, so do nothing
            }
            // Sprint 5-8 / Phase 4-5: tab-tearing entries (Wayland alternative UX).
            // Delegate to the same-named action on `execute_action` to centralize the path.
            ContextMenuAction::DetachToNewWindow => {
                info!("ContextMenu: DetachToNewWindow");
                // Record pos = (0,0) (works on Wayland too; winit picks the position)
                self.pending_new_window_drop_pos = Some(winit::dpi::PhysicalPosition::new(0, 0));
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::BreakPane);
                }
            }
            ContextMenuAction::CloseOsWindow => {
                info!("ContextMenu: CloseOsWindow");
                if let Some(w) = &self.window {
                    let wid = w.id();
                    if let Err(e) = self
                        .proxy
                        .send_event(crate::renderer::UserEvent::CloseOsWindow { window_id: wid })
                    {
                        tracing::warn!("failed to send CloseOsWindow UserEvent: {}", e);
                    }
                }
            }
            // ---- Custom title bar system menu (non-Windows fallback) ----
            ContextMenuAction::MinimizeWindow => {
                if let Some(w) = &self.window {
                    w.set_minimized(true);
                }
            }
            ContextMenuAction::ToggleMaximizeWindow => {
                // Same guard as the tab-bar maximize button: don't fight
                // Quake mode's manual window geometry.
                if !self.quake.visible
                    && let Some(w) = &self.window
                {
                    w.set_maximized(!w.is_maximized());
                    w.request_redraw();
                }
            }
            ContextMenuAction::RequestCloseWindow => {
                // Native-close path, so `window.close_action` applies.
                if let Some(w) = &self.window
                    && let Err(e) = self
                        .proxy
                        .send_event(crate::renderer::UserEvent::RequestClose { window_id: w.id() })
                {
                    tracing::warn!("failed to send RequestClose UserEvent: {}", e);
                }
            }
            // ---- Phase 2c follow-up: block-aware context-menu entries ----
            ContextMenuAction::CopyBlock { block_id } => {
                if let Some(text) = self.app.state.block_text_by_id(*block_id)
                    && let Ok(mut clipboard) = arboard::Clipboard::new()
                {
                    let _ = clipboard.set_text(text);
                }
            }
            ContextMenuAction::ReplayBlock { block_id } => {
                if let Some(cmd) = self.app.state.block_replay_command_by_id(*block_id)
                    && let Some(conn) = &self.connection
                {
                    let mut payload = cmd;
                    payload.push('\n');
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::PasteText { text: payload });
                }
            }
            ContextMenuAction::ToggleBlockCollapse { block_id } => {
                let _ = self.app.state.toggle_block_collapse_by_id(*block_id);
            }
            ContextMenuAction::SetBlockName { block_id } => {
                let _ = self.app.state.open_block_name_modal_for(*block_id);
            }
            ContextMenuAction::RemoveBlockName { block_id } => {
                let _ = self.app.state.remove_block_name_by_id(*block_id);
            }
        }
    }
}
