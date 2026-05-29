//! Client-side state — keeps the rendering data received from the server.

use std::collections::HashMap;

use nexterm_proto::{Grid, PaneLayout, ServerToClient};

/// Per-pane rendering state.
pub struct PaneState {
    pub grid: Grid,
    pub cursor_col: u16,
    pub cursor_row: u16,
}

impl PaneState {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            cursor_col: 0,
            cursor_row: 0,
        }
    }

    /// Apply a grid diff.
    fn apply_diff(
        &mut self,
        dirty_rows: Vec<nexterm_proto::DirtyRow>,
        cursor_col: u16,
        cursor_row: u16,
    ) {
        for dirty in dirty_rows {
            if let Some(row) = self.grid.rows.get_mut(dirty.row as usize) {
                *row = dirty.cells;
            }
        }
        self.cursor_col = cursor_col;
        self.cursor_row = cursor_row;
    }
}

/// `Ctrl+B` prefix mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixMode {
    /// Normal mode.
    None,
    /// `Ctrl+B` was pressed; waiting for the next key.
    CtrlB,
    /// The help overlay is shown.
    Help,
}

/// Error toast (an error message that auto-dismisses after a short delay).
pub struct ErrorToast {
    pub message: String,
    /// When the toast started being shown.
    pub shown_at: std::time::Instant,
}

/// Aggregated client state.
pub struct ClientState {
    /// Pane id → render state.
    pub panes: HashMap<u32, PaneState>,
    /// Currently focused pane id.
    pub focused_pane_id: Option<u32>,
    /// Pane layouts as published by the server.
    pub pane_layouts: Vec<PaneLayout>,
    /// Terminal size.
    pub cols: u16,
    pub rows: u16,
    /// Session name (the name used in the Attach request).
    pub session_name: String,
    /// Current `Ctrl+B` prefix mode.
    pub prefix_mode: PrefixMode,
    /// Most recent error toast, if any.
    pub error_toast: Option<ErrorToast>,
}

impl ClientState {
    pub fn new() -> Self {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        Self {
            panes: HashMap::new(),
            focused_pane_id: None,
            pane_layouts: Vec::new(),
            cols,
            rows,
            session_name: "main".to_string(),
            prefix_mode: PrefixMode::None,
            error_toast: None,
        }
    }

    /// Apply a server-sent message to the state.
    pub fn apply_server_message(&mut self, msg: ServerToClient) {
        match msg {
            ServerToClient::FullRefresh { pane_id, grid } => {
                let cursor_col = grid.cursor_col;
                let cursor_row = grid.cursor_row;
                let state = self
                    .panes
                    .entry(pane_id)
                    .or_insert_with(|| PaneState::new(grid.width, grid.height));
                state.grid = grid;
                state.cursor_col = cursor_col;
                state.cursor_row = cursor_row;
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
                if let Some(state) = self.panes.get_mut(&pane_id) {
                    state.apply_diff(dirty_rows, cursor_col, cursor_row);
                }
            }
            ServerToClient::Pong => {
                tracing::debug!("received Pong");
            }
            ServerToClient::HelloAck {
                proto_version,
                server_version,
            } => {
                tracing::info!(
                    "received HelloAck: proto={}, server_version={}",
                    proto_version,
                    server_version
                );
            }
            ServerToClient::Error { message } => {
                tracing::error!("server error: {}", message);
                // Show the error as a toast notification.
                self.error_toast = Some(ErrorToast {
                    message,
                    shown_at: std::time::Instant::now(),
                });
            }
            ServerToClient::SessionList { sessions } => {
                tracing::info!("session list: {:?}", sessions);
            }
            // The TUI client does not support the image protocol; ignore it.
            ServerToClient::ImagePlaced { .. } => {}
            // Layout change: refresh the pane positions.
            ServerToClient::LayoutChanged {
                panes,
                focused_pane_id,
            } => {
                self.pane_layouts = panes;
                self.focused_pane_id = Some(focused_pane_id);
            }
            // The TUI ignores bell notifications.
            ServerToClient::Bell { .. } => {}
            // Recording state notifications are ignored by the TUI.
            ServerToClient::RecordingStarted { .. } | ServerToClient::RecordingStopped { .. } => {}
            // Window list changes and pane-closed notifications are ignored by the TUI.
            ServerToClient::WindowListChanged { .. } | ServerToClient::PaneClosed { .. } => {}
            // Title changes and desktop notifications are ignored by the TUI.
            ServerToClient::TitleChanged { .. } | ServerToClient::DesktopNotification { .. } => {}
            // Broadcast mode changes are ignored by the TUI.
            ServerToClient::BroadcastModeChanged { .. } => {}
            // asciicast recording state notifications are ignored by the TUI.
            ServerToClient::AsciicastStarted { .. } | ServerToClient::AsciicastStopped { .. } => {}
            // Template operation responses are ignored by the TUI.
            ServerToClient::TemplateSaved { .. }
            | ServerToClient::TemplateLoaded { .. }
            | ServerToClient::TemplateList { .. } => {}
            // Zoom / pane-break / serial connection events are ignored by the TUI.
            ServerToClient::ZoomChanged { .. }
            | ServerToClient::PaneBroken { .. }
            | ServerToClient::SerialConnected { .. }
            | ServerToClient::SftpProgress { .. }
            | ServerToClient::SftpDone { .. }
            | ServerToClient::SemanticMark { .. } => {}
            // Floating pane events are GPU-client only; ignore them in the TUI.
            ServerToClient::FloatingPaneOpened { .. }
            | ServerToClient::FloatingPaneMoved { .. }
            | ServerToClient::FloatingPaneClosed { .. } => {}
            // Plugin operation responses are ignored by the TUI.
            ServerToClient::PluginList { .. } | ServerToClient::PluginOk { .. } => {}
            // Sprint 4-1: the TUI lacks a consent dialog, so OSC 52 requests are ignored.
            ServerToClient::ClipboardWriteRequest { .. } => {}
            // Sprint 5-2: the TUI has no tab/CWD UI, so OSC 7 CWD notifications are ignored.
            ServerToClient::CwdChanged { .. } => {}
            // Sprint 5-7 / Phase 2-1: the TUI has no workspace UI, so list/switch events are ignored.
            ServerToClient::WorkspaceList { .. } | ServerToClient::WorkspaceSwitched { .. } => {}
            // Sprint 5-7 / Phase 2-2: the TUI has no Quake mode, so this is ignored.
            ServerToClient::QuakeToggleRequest { .. } => {}
            // Sprint 5-8 / Phase 4-5: the TUI has no OS-window close confirmation dialog,
            // so this is ignored (the TUI is single-process / single-terminal, so tab tearing
            // is disabled altogether).
            ServerToClient::ForegroundProcessStatus { .. } => {}
        }
    }

    /// Update state after a terminal resize.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
    }

    /// Return the focused pane state, if any.
    pub fn focused_pane(&self) -> Option<&PaneState> {
        self.focused_pane_id.and_then(|id| self.panes.get(&id))
    }

    /// Dismiss expired error toasts (after 3 seconds).
    pub fn tick_toasts(&mut self) {
        if let Some(toast) = &self.error_toast
            && toast.shown_at.elapsed().as_secs() >= 3
        {
            self.error_toast = None;
        }
    }

    /// Enter `Ctrl+B` prefix mode.
    pub fn enter_prefix(&mut self) {
        self.prefix_mode = PrefixMode::CtrlB;
    }

    /// Leave prefix mode.
    pub fn exit_prefix(&mut self) {
        self.prefix_mode = PrefixMode::None;
    }

    /// Toggle the help overlay.
    pub fn toggle_help(&mut self) {
        if self.prefix_mode == PrefixMode::Help {
            self.prefix_mode = PrefixMode::None;
        } else {
            self.prefix_mode = PrefixMode::Help;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_proto::{Cell, DirtyRow};

    #[test]
    fn full_refresh_registers_pane() {
        let mut state = ClientState::new();
        let grid = Grid::new(80, 24);
        state.apply_server_message(ServerToClient::FullRefresh { pane_id: 1, grid });
        assert!(state.panes.contains_key(&1));
        assert_eq!(state.focused_pane_id, Some(1));
    }

    #[test]
    fn grid_diff_applies_diff() {
        let mut state = ClientState::new();
        // Register the pane via a FullRefresh first.
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });

        // Apply a diff.
        let cell = Cell {
            ch: 'X',
            ..Cell::default()
        };
        state.apply_server_message(ServerToClient::GridDiff {
            pane_id: 1,
            dirty_rows: vec![DirtyRow {
                row: 0,
                cells: {
                    let mut row = vec![Cell::default(); 80];
                    row[0] = cell;
                    row
                },
            }],
            cursor_col: 1,
            cursor_row: 0,
        });

        let pane = state.focused_pane().unwrap();
        assert_eq!(pane.grid.rows[0][0].ch, 'X');
        assert_eq!(pane.cursor_col, 1);
    }

    #[test]
    fn resize_updates_terminal_size() {
        let mut state = ClientState::new();
        state.resize(120, 40);
        assert_eq!(state.cols, 120);
        assert_eq!(state.rows, 40);
    }

    #[test]
    fn focused_pane_returns_none_when_empty() {
        let state = ClientState::new();
        assert!(state.focused_pane().is_none());
    }

    #[test]
    fn multiple_panes_register_and_focus_first() {
        let mut state = ClientState::new();
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 2,
            grid: Grid::new(80, 24),
        });
        // The pane from the first FullRefresh must remain focused.
        assert_eq!(state.focused_pane_id, Some(1));
        assert!(state.panes.contains_key(&2));
    }

    #[test]
    fn pong_does_not_panic() {
        let mut state = ClientState::new();
        state.apply_server_message(ServerToClient::Pong);
    }

    #[test]
    fn error_message_sets_toast() {
        let mut state = ClientState::new();
        state.apply_server_message(ServerToClient::Error {
            message: "test error".to_string(),
        });
        assert!(state.error_toast.is_some());
        assert_eq!(state.error_toast.as_ref().unwrap().message, "test error");
    }

    #[test]
    fn layout_changed_updates_pane_layouts() {
        let mut state = ClientState::new();
        let layouts = vec![
            PaneLayout {
                pane_id: 1,
                col_offset: 0,
                row_offset: 0,
                cols: 40,
                rows: 24,
                is_focused: true,
            },
            PaneLayout {
                pane_id: 2,
                col_offset: 40,
                row_offset: 0,
                cols: 40,
                rows: 24,
                is_focused: false,
            },
        ];
        state.apply_server_message(ServerToClient::LayoutChanged {
            panes: layouts.clone(),
            focused_pane_id: 1,
        });
        assert_eq!(state.pane_layouts.len(), 2);
        assert_eq!(state.focused_pane_id, Some(1));
    }

    #[test]
    fn prefix_mode_toggles() {
        let mut state = ClientState::new();
        assert_eq!(state.prefix_mode, PrefixMode::None);
        state.enter_prefix();
        assert_eq!(state.prefix_mode, PrefixMode::CtrlB);
        state.exit_prefix();
        assert_eq!(state.prefix_mode, PrefixMode::None);
    }

    #[test]
    fn floating_pane_events_are_ignored() {
        let mut state = ClientState::new();
        state.apply_server_message(ServerToClient::FloatingPaneOpened {
            pane_id: 99,
            col_off: 5,
            row_off: 3,
            cols: 40,
            rows: 20,
        });
        state.apply_server_message(ServerToClient::FloatingPaneMoved {
            pane_id: 99,
            col_off: 10,
            row_off: 5,
            cols: 40,
            rows: 20,
        });
        state.apply_server_message(ServerToClient::FloatingPaneClosed { pane_id: 99 });
    }
}
