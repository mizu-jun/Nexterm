//! クライアント状態 — サーバーから受信した描画データを保持する

use std::collections::HashMap;

use nexterm_proto::{Grid, ServerToClient};

/// ペインの描画状態
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

    /// 差分を適用する
    fn apply_diff(&mut self, dirty_rows: Vec<nexterm_proto::DirtyRow>, cursor_col: u16, cursor_row: u16) {
        for dirty in dirty_rows {
            if let Some(row) = self.grid.rows.get_mut(dirty.row as usize) {
                *row = dirty.cells;
            }
        }
        self.cursor_col = cursor_col;
        self.cursor_row = cursor_row;
    }
}

/// クライアント全体の状態
pub struct ClientState {
    /// ペイン ID → 描画状態
    pub panes: HashMap<u32, PaneState>,
    /// 現在フォーカス中のペイン ID
    pub focused_pane_id: Option<u32>,
    /// 端末サイズ
    pub cols: u16,
    pub rows: u16,
}

impl ClientState {
    pub fn new() -> Self {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        Self {
            panes: HashMap::new(),
            focused_pane_id: None,
            cols,
            rows,
        }
    }

    /// サーバーからのメッセージをステートに反映する
    pub fn apply_server_message(&mut self, msg: ServerToClient) {
        match msg {
            ServerToClient::FullRefresh { pane_id, grid } => {
                let cursor_col = grid.cursor_col;
                let cursor_row = grid.cursor_row;
                let state = self.panes.entry(pane_id).or_insert_with(|| {
                    PaneState::new(grid.width, grid.height)
                });
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
                tracing::debug!("Pong 受信");
            }
            ServerToClient::Error { message } => {
                tracing::error!("サーバーエラー: {}", message);
            }
            ServerToClient::SessionList { sessions } => {
                tracing::info!("セッション一覧: {:?}", sessions);
            }
            // TUI クライアントは画像プロトコル非対応のため無視する
            ServerToClient::ImagePlaced { .. } => {}
            // フォーカスペイン ID を更新する（TUI は単一ペイン表示のため位置情報は使わない）
            ServerToClient::LayoutChanged { focused_pane_id, .. } => {
                self.focused_pane_id = Some(focused_pane_id);
            }
            // TUI ではベル通知は無視する
            ServerToClient::Bell { .. } => {}
            // 録音状態通知は TUI では無視する
            ServerToClient::RecordingStarted { .. } | ServerToClient::RecordingStopped { .. } => {}
            // ウィンドウ一覧変更・ペイン閉鎖通知は TUI では無視する
            ServerToClient::WindowListChanged { .. } | ServerToClient::PaneClosed { .. } => {}
            // タイトル変更・デスクトップ通知は TUI では無視する
            ServerToClient::TitleChanged { .. } | ServerToClient::DesktopNotification { .. } => {}
            // ブロードキャストモード変更は TUI では無視する
            ServerToClient::BroadcastModeChanged { .. } => {}
            // asciicast 録音状態通知は TUI では無視する
            ServerToClient::AsciicastStarted { .. } | ServerToClient::AsciicastStopped { .. } => {}
            // テンプレート操作結果は TUI では無視する
            ServerToClient::TemplateSaved { .. }
            | ServerToClient::TemplateLoaded { .. }
            | ServerToClient::TemplateList { .. } => {}
            // ズーム・ペイン分離・シリアル接続は TUI では無視する
            ServerToClient::ZoomChanged { .. }
            | ServerToClient::PaneBroken { .. }
            | ServerToClient::SerialConnected { .. }
            | ServerToClient::SftpProgress { .. }
            | ServerToClient::SftpDone { .. } => {}
        }
    }

    /// 端末リサイズ時に状態を更新する
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
    }

    /// フォーカス中のペイン状態を返す
    pub fn focused_pane(&self) -> Option<&PaneState> {
        self.focused_pane_id
            .and_then(|id| self.panes.get(&id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_proto::{DirtyRow, Cell};

    #[test]
    fn full_refreshでペインが登録される() {
        let mut state = ClientState::new();
        let grid = Grid::new(80, 24);
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid,
        });
        assert!(state.panes.contains_key(&1));
        assert_eq!(state.focused_pane_id, Some(1));
    }

    #[test]
    fn grid_diffで差分が適用される() {
        let mut state = ClientState::new();
        // まず Full Refresh でペインを登録する
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });

        // 差分を適用する
        let mut cell = Cell::default();
        cell.ch = 'X';
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
}
