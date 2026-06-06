//! Server message dispatch and scrollback / prompt-jump operations
//!
//! Extracted from `state/mod.rs`:
//! - `impl ClientState { apply_server_message }` — dispatch every received
//!   `ServerToClient` message and apply it to the state
//! - `impl ClientState { scroll_up / scroll_down / jump_prev_prompt / jump_next_prompt }` —
//!   scrollback offset manipulation and prompt jumps based on OSC 133 PromptStart anchors
//! - Unit tests (FullRefresh / GridDiff / search lifecycle / Quick Select expansion)

use nexterm_proto::ServerToClient;

use super::AlertKind;
use super::ClientState;
use super::ForegroundProcessStatus;
use super::pane::{FloatRect, PaneState, PlacedImage};

impl ClientState {
    pub fn apply_server_message(&mut self, msg: ServerToClient) {
        match msg {
            ServerToClient::FullRefresh { pane_id, grid } => {
                let cursor_col = grid.cursor_col;
                let cursor_row = grid.cursor_row;
                let capacity = self.scrollback_capacity;
                let state = self
                    .panes
                    .entry(pane_id)
                    .or_insert_with(|| PaneState::new(grid.width, grid.height, capacity));
                state.grid = grid;
                state.cursor_col = cursor_col;
                state.cursor_row = cursor_row;
                state.content_dirty = true;
                if self.focused_pane_id.is_none() {
                    self.focused_pane_id = Some(pane_id);
                }
            }
            ServerToClient::GridDiff {
                pane_id,
                dirty_rows,
                cursor_col,
                cursor_row,
            } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.apply_diff(dirty_rows, cursor_col, cursor_row);
                    // Output to a non-focused pane is marked as activity
                    if self.focused_pane_id != Some(pane_id) {
                        pane.has_activity = true;
                    }
                }
            }
            ServerToClient::Pong => {}
            ServerToClient::HelloAck {
                proto_version,
                server_version,
            } => {
                tracing::info!(
                    "received HelloAck from server: proto={}, server_version={}",
                    proto_version,
                    server_version
                );
            }
            ServerToClient::Error { message } => {
                tracing::error!("server error: {}", message);
                // Sprint 5-12 Phase 1: reflect on the UI banner so the user can see it.
                // E.g. PTY spawn failure (PowerShell spawn failure etc.), config-load
                // error, pane-split failure. Dismissed via the Esc key.
                self.error_banner = Some(message);
            }
            ServerToClient::SessionList { .. } => {}
            ServerToClient::ImagePlaced {
                pane_id,
                image_id,
                col,
                row,
                width,
                height,
                rgba,
            } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.images.insert(
                        image_id,
                        PlacedImage {
                            col,
                            row,
                            width,
                            height,
                            rgba,
                        },
                    );
                    if self.focused_pane_id != Some(pane_id) {
                        pane.has_activity = true;
                    }
                }
            }
            ServerToClient::Bell { pane_id } => {
                // Set the flag to trigger the OS user-attention request
                self.pending_bell = true;
                // Sprint 5-11-5: also push to the Role::Alert queue so SRs get notified
                self.add_alert(AlertKind::Bell, pane_id, "Bell".to_string(), String::new());
            }
            ServerToClient::RecordingStarted { .. } | ServerToClient::RecordingStopped { .. } => {}
            // Sprint 5-8 Phase 4-4: track the focused Window ID via WindowListChanged.
            // Used to resolve `target_window_id` when the primary OS Window is the
            // target in the out-of-tab drop path (`OtherWindowTabBar` branch of
            // `handle_tab_drag_drop_outside`).
            ServerToClient::WindowListChanged { windows } => {
                if let Some(focused) = windows.iter().find(|w| w.is_focused) {
                    self.focused_server_window_id = focused.window_id;
                }
            }
            ServerToClient::PaneClosed { .. } => {}
            // OSC 0/2 title change — update the pane's title field
            ServerToClient::TitleChanged { pane_id, title } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.title = title;
                }
            }
            // DesktopNotification and ClipboardWriteRequest are handled by event_handler
            // according to the SecurityConfig policy (state.rs does nothing).
            ServerToClient::DesktopNotification { .. } => {}
            ServerToClient::ClipboardWriteRequest { .. } => {}
            ServerToClient::BroadcastModeChanged { enabled } => {
                self.broadcast_mode = enabled;
            }
            ServerToClient::AsciicastStarted { .. } | ServerToClient::AsciicastStopped { .. } => {}
            ServerToClient::TemplateSaved { .. }
            | ServerToClient::TemplateLoaded { .. }
            | ServerToClient::TemplateList { .. } => {}
            ServerToClient::ZoomChanged { is_zoomed } => {
                self.is_zoomed = is_zoomed;
            }
            // Pane detach / serial connect are followed by LayoutChanged / WindowListChanged from the server, so no state update is needed here
            ServerToClient::PaneBroken { .. } | ServerToClient::SerialConnected { .. } => {}
            // SFTP transfer progress / completion is shown in the status bar
            ServerToClient::SftpProgress {
                path,
                transferred,
                total,
            } => {
                let pct = (transferred * 100).checked_div(total).unwrap_or(0);
                self.status_bar_text = format!("SFTP {} {}%", path, pct);
            }
            ServerToClient::SftpDone { path, error } => {
                if let Some(err) = error {
                    self.status_bar_text = format!("SFTP ERR: {}", err);
                } else {
                    self.status_bar_text = format!("SFTP OK: {}", path);
                }
            }
            // OSC 133 semantic zone marks — show the latest command's exit code in the status bar,
            //   and on A (PromptStart) record an anchor for jump-to-prompt (Sprint 5-2 / B1).
            ServerToClient::SemanticMark {
                pane_id,
                kind,
                exit_code,
                ..
            } => {
                if kind == "A"
                    && let Some(pane) = self.panes.get_mut(&pane_id)
                {
                    // Deduplicate: skip when identical to the most recent anchor
                    let next_idx = pane.scrollback.len();
                    if pane.prompt_anchors.last().copied() != Some(next_idx) {
                        pane.prompt_anchors.push(next_idx);
                    }
                    // Cap on retained anchors (memory DoS guard). Drop the oldest.
                    const MAX_PROMPT_ANCHORS: usize = 1024;
                    if pane.prompt_anchors.len() > MAX_PROMPT_ANCHORS {
                        let excess = pane.prompt_anchors.len() - MAX_PROMPT_ANCHORS;
                        pane.prompt_anchors.drain(..excess);
                    }
                }
                if kind == "D"
                    && self.focused_pane_id == Some(pane_id)
                    && let Some(code) = exit_code
                {
                    if code != 0 {
                        self.status_bar_text = format!("[exit: {}]", code);
                    } else {
                        self.status_bar_text.clear();
                    }
                }
            }
            // Floating pane events — cache the position info, but the actual
            // rendering is implemented separately in renderer.rs.
            ServerToClient::FloatingPaneOpened {
                pane_id,
                col_off,
                row_off,
                cols,
                rows,
            } => {
                self.floating_pane_rects.insert(
                    pane_id,
                    FloatRect {
                        col_off,
                        row_off,
                        cols,
                        rows,
                    },
                );
            }
            ServerToClient::FloatingPaneMoved {
                pane_id,
                col_off,
                row_off,
                cols,
                rows,
            } => {
                self.floating_pane_rects.insert(
                    pane_id,
                    FloatRect {
                        col_off,
                        row_off,
                        cols,
                        rows,
                    },
                );
            }
            ServerToClient::FloatingPaneClosed { pane_id } => {
                self.floating_pane_rects.remove(&pane_id);
            }
            ServerToClient::LayoutChanged {
                panes,
                focused_pane_id,
            } => {
                let prev_focused = self.focused_pane_id;
                // Sprint 5-7 / Phase 3-2: detect newly added panes and record the fade-in animation
                let now = std::time::Instant::now();
                let prev_pane_ids: std::collections::HashSet<u32> =
                    self.pane_layouts.keys().copied().collect();
                for layout in &panes {
                    if !prev_pane_ids.contains(&layout.pane_id) {
                        self.animations.record_pane_added(layout.pane_id, now);
                    }
                }
                // Clean up state for panes that disappeared
                let new_pane_ids: std::collections::HashSet<u32> =
                    panes.iter().map(|l| l.pane_id).collect();
                for removed_id in prev_pane_ids.difference(&new_pane_ids) {
                    self.animations.record_pane_removed(*removed_id);
                }

                // Refresh the whole layout
                self.pane_layouts.clear();
                // Sprint 5-7 / Phase 2-3: reflect the order panes appear in the array into tab_order
                // (the server orders them by Window.pane_order, so that is the logical tab order).
                self.tab_order = panes.iter().map(|l| l.pane_id).collect();
                for layout in panes {
                    self.pane_layouts.insert(layout.pane_id, layout);
                }
                // Update the focused pane and clear its activity flag
                self.focused_pane_id = Some(focused_pane_id);
                if let Some(pane) = self.panes.get_mut(&focused_pane_id) {
                    pane.has_activity = false;
                }
                // Sprint 5-7 / Phase 3-2: record the tab-switch animation (only when it actually changed)
                if prev_focused != Some(focused_pane_id) {
                    self.animations.record_tab_switch(focused_pane_id, now);
                }
                // Phase 4 (UI/UX modernization): sync pane-dim spring targets unconditionally
                // so new/removed panes get their springs initialized or cleaned up.
                let all_ids: Vec<u32> = self.pane_layouts.keys().copied().collect();
                self.animations
                    .record_focus_changed(focused_pane_id, &all_ids);
            }
            // Plugin operation responses are ignored in the GPU client
            ServerToClient::PluginList { .. } | ServerToClient::PluginOk { .. } => {}
            // Sprint 5-2 / B2: OSC 7 CWD notification — store on PaneState (UI display to come later)
            ServerToClient::CwdChanged { pane_id, cwd } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.cwd = Some(cwd);
                }
            }
            // Sprint 5-7 / Phase 2-1: workspace list / switch notification
            ServerToClient::WorkspaceList {
                current,
                workspaces: _,
            } => {
                self.current_workspace = current;
            }
            ServerToClient::WorkspaceSwitched { name } => {
                self.current_workspace = name;
            }
            // Sprint 5-7 / Phase 2-2: Quake mode toggle request.
            //
            // When a toggle request comes in over IPC (e.g. from nexterm-ctl), we only
            // record the "pending Quake action" here. The actual window manipulation
            // runs on the lifecycle side, which holds mutable access to the winit Window.
            ServerToClient::QuakeToggleRequest { action } => {
                self.pending_quake_action = Some(action);
            }
            // Phase 4-5: response to QueryForegroundProcess.
            // Only reflected when `pending_close_request` is waiting on this `window_id`'s
            // response; the result drives the "show confirmation dialog vs. detach
            // immediately" decision.
            ServerToClient::ForegroundProcessStatus {
                window_id,
                has_foreground,
            } => {
                self.foreground_process_status = Some(ForegroundProcessStatus {
                    window_id,
                    has_foreground,
                });
            }
        }
    }

    /// Scroll the scrollback up by one screen
    pub fn scroll_up(&mut self, lines: usize) {
        if let Some(pane) = self.focused_pane_mut() {
            let max_offset = pane.scrollback.len().saturating_sub(1);
            pane.scroll_offset = (pane.scroll_offset + lines).min(max_offset);
        }
    }

    /// Scroll the scrollback down by one screen
    pub fn scroll_down(&mut self, lines: usize) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.scroll_offset = pane.scroll_offset.saturating_sub(lines);
        }
    }

    /// Jump in scrollback to the previous shell prompt (Sprint 5-2 / B1).
    ///
    /// Walk `prompt_anchors` from newest to oldest and jump to the smallest anchor
    /// greater than the current `scroll_offset` (= the prompt one screen earlier).
    /// No-op if there are no anchors or we've already reached the oldest one.
    /// Returns `true` on a successful jump.
    pub fn jump_prev_prompt(&mut self) -> bool {
        let Some(pane) = self.focused_pane_mut() else {
            return false;
        };
        let current = pane.scroll_offset;
        let max_offset = pane.scrollback.len().saturating_sub(1);
        // Anchors are expressed in the same scrollback-length space, so compare against scroll_offset directly
        let target = pane
            .prompt_anchors
            .iter()
            .rev()
            .copied()
            .find(|&idx| idx > current && idx <= max_offset);
        if let Some(idx) = target {
            pane.scroll_offset = idx;
            true
        } else {
            false
        }
    }

    /// Jump in scrollback to the next shell prompt (Sprint 5-2 / B1).
    ///
    /// Jump to the largest anchor smaller than the current `scroll_offset`
    /// (= the prompt one screen later). No-op if there are no anchors.
    /// If we're past the newest prompt, snap back to the live screen by setting
    /// `scroll_offset = 0`. Returns `true` on a successful jump.
    pub fn jump_next_prompt(&mut self) -> bool {
        let Some(pane) = self.focused_pane_mut() else {
            return false;
        };
        let current = pane.scroll_offset;
        let target = pane
            .prompt_anchors
            .iter()
            .copied()
            .rev()
            .find(|&idx| idx < current);
        if let Some(idx) = target {
            pane.scroll_offset = idx;
            true
        } else if current > 0 && !pane.prompt_anchors.is_empty() {
            // If we are below every anchor, snap back to the live screen
            pane.scroll_offset = 0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::menus::find_quick_select_matches;
    use nexterm_proto::{Cell, DirtyRow, Grid};

    #[test]
    fn full_refresh_registers_pane() {
        let mut state = ClientState::new(80, 24, 1000);
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        assert!(state.panes.contains_key(&1));
        assert_eq!(state.focused_pane_id, Some(1));
    }

    #[test]
    fn grid_diff_applies_diff() {
        let mut state = ClientState::new(80, 24, 1000);
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        let mut row = vec![Cell::default(); 80];
        row[0].ch = 'X';
        state.apply_server_message(ServerToClient::GridDiff {
            pane_id: 1,
            dirty_rows: vec![DirtyRow { row: 0, cells: row }],
            cursor_col: 1,
            cursor_row: 0,
        });
        let pane = state.focused_pane().unwrap();
        assert_eq!(pane.grid.rows[0][0].ch, 'X');
    }

    #[test]
    fn search_lifecycle() {
        let mut state = ClientState::new(80, 24, 1000);
        state.start_search();
        assert!(state.search.is_active);
        state.push_search_char('a');
        assert_eq!(state.search.query, "a");
        state.end_search();
        assert!(!state.search.is_active);
        assert!(state.search.query.is_empty());
    }

    // ---- Sprint 5-4 / D1: Quick Select expansion tests ----

    /// Helper that converts text into `Vec<Vec<Cell>>`
    fn text_to_rows(lines: &[&str]) -> Vec<Vec<nexterm_proto::Cell>> {
        lines
            .iter()
            .map(|line| {
                line.chars()
                    .map(|c| nexterm_proto::Cell {
                        ch: c,
                        ..Default::default()
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn quick_select_detects_url() {
        let rows = text_to_rows(&["Visit https://example.com/path?q=1 today"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text.starts_with("https://")));
    }

    #[test]
    fn quick_select_detects_email() {
        let rows = text_to_rows(&["Contact alice@example.com for details"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text == "alice@example.com"));
    }

    #[test]
    fn quick_select_detects_uuid() {
        let rows = text_to_rows(&["session-id: 550e8400-e29b-41d4-a716-446655440000"]);
        let matches = find_quick_select_matches(&rows);
        assert!(
            matches
                .iter()
                .any(|m| m.text == "550e8400-e29b-41d4-a716-446655440000")
        );
    }

    #[test]
    fn quick_select_detects_file_with_line_column() {
        let rows = text_to_rows(&["error in src/main.rs:42:10 — unused variable"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text.contains("src/main.rs:42:10")));
    }

    #[test]
    fn quick_select_detects_jira_ticket() {
        let rows = text_to_rows(&["See PROJ-1234 for tracking"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text == "PROJ-1234"));
    }

    #[test]
    fn quick_select_detects_windows_path() {
        let rows = text_to_rows(&[r"open C:\Users\test\file.txt in editor"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text.starts_with("C:\\")));
    }

    #[test]
    fn quick_select_detects_ipv4_with_port() {
        let rows = text_to_rows(&["connect to 192.168.1.100:8080"]);
        let matches = find_quick_select_matches(&rows);
        assert!(matches.iter().any(|m| m.text == "192.168.1.100:8080"));
    }

    #[test]
    fn quick_select_url_priority_over_path() {
        // URLs contain `//`, so they can overlap the path pattern, but URL must win
        let rows = text_to_rows(&["url: https://github.com/foo/bar"]);
        let matches = find_quick_select_matches(&rows);
        let url_match = matches
            .iter()
            .find(|m| m.text.starts_with("https://"))
            .expect("failed to detect URL");
        // The path pattern must not steal `/foo/bar` (it is contained within the URL)
        assert!(url_match.text.contains("github.com/foo/bar"));
        // The URL match must not completely duplicate a path match
        let path_count = matches.iter().filter(|m| m.text == "/foo/bar").count();
        assert_eq!(path_count, 0, "path contained inside a URL must not match");
    }

    #[test]
    fn quick_select_labels_are_assigned() {
        let rows = text_to_rows(&["https://a.com https://b.com https://c.com"]);
        let matches = find_quick_select_matches(&rows);
        assert_eq!(matches.len(), 3);
        let labels: Vec<&str> = matches.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, vec!["a", "b", "c"]);
    }

    #[test]
    fn quick_select_empty_grid_yields_no_matches() {
        let rows: Vec<Vec<nexterm_proto::Cell>> = vec![];
        let matches = find_quick_select_matches(&rows);
        assert!(matches.is_empty());
    }
}
