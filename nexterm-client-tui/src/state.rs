//! クライアント状態 — サーバーから受信した描画データを保持する

use std::collections::HashMap;

use nexterm_proto::{Grid, PaneLayout, ServerToClient};

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

/// Ctrl+B プレフィックスモード
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixMode {
    /// 通常モード
    None,
    /// Ctrl+B を押した後、次のキーを待っている
    CtrlB,
    /// ヘルプオーバーレイ表示中
    Help,
}

/// エラートースト（画面上に一定時間表示するエラーメッセージ）
pub struct ErrorToast {
    pub message: String,
    /// 表示開始時刻（エポック秒）
    pub shown_at: std::time::Instant,
}

/// クライアント全体の状態
pub struct ClientState {
    /// ペイン ID → 描画状態
    pub panes: HashMap<u32, PaneState>,
    /// 現在フォーカス中のペイン ID
    pub focused_pane_id: Option<u32>,
    /// サーバーから受信したペインレイアウト一覧
    pub pane_layouts: Vec<PaneLayout>,
    /// 端末サイズ
    pub cols: u16,
    pub rows: u16,
    /// セッション名（サーバーへ Attach した名前）
    pub session_name: String,
    /// Ctrl+B プレフィックスモード
    pub prefix_mode: PrefixMode,
    /// エラートースト（直近のエラーメッセージ）
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

    /// サーバーからのメッセージをステートに反映する
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
                tracing::debug!("Pong 受信");
            }
            ServerToClient::HelloAck {
                proto_version,
                server_version,
            } => {
                tracing::info!(
                    "サーバー HelloAck 受信: proto={}, server_version={}",
                    proto_version,
                    server_version
                );
            }
            ServerToClient::Error { message } => {
                tracing::error!("サーバーエラー: {}", message);
                // エラートーストとして表示する
                self.error_toast = Some(ErrorToast {
                    message,
                    shown_at: std::time::Instant::now(),
                });
            }
            ServerToClient::SessionList { sessions } => {
                tracing::info!("セッション一覧: {:?}", sessions);
            }
            // TUI クライアントは画像プロトコル非対応のため無視する
            ServerToClient::ImagePlaced { .. } => {}
            // レイアウト変更：ペイン位置情報を更新する
            ServerToClient::LayoutChanged {
                panes,
                focused_pane_id,
            } => {
                self.pane_layouts = panes;
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
            | ServerToClient::SftpDone { .. }
            | ServerToClient::SemanticMark { .. } => {}
            // フローティングペインイベントは TUI では無視する（GPU クライアント専用）
            ServerToClient::FloatingPaneOpened { .. }
            | ServerToClient::FloatingPaneMoved { .. }
            | ServerToClient::FloatingPaneClosed { .. } => {}
            // プラグイン操作応答は TUI では無視する
            ServerToClient::PluginList { .. } | ServerToClient::PluginOk { .. } => {}
            // Sprint 4-1: TUI には同意ダイアログ UI がないため OSC 52 要求は無視する
            ServerToClient::ClipboardWriteRequest { .. } => {}
            // Sprint 5-2: TUI にはタブ/CWD 表示 UI がないため OSC 7 CWD 通知は無視する
            ServerToClient::CwdChanged { .. } => {}
            // Sprint 5-7 / Phase 2-1: TUI にはワークスペース UI がないため一覧/切替通知は無視する
            ServerToClient::WorkspaceList { .. } | ServerToClient::WorkspaceSwitched { .. } => {}
        }
    }

    /// 端末リサイズ時に状態を更新する
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
    }

    /// フォーカス中のペイン状態を返す
    pub fn focused_pane(&self) -> Option<&PaneState> {
        self.focused_pane_id.and_then(|id| self.panes.get(&id))
    }

    /// 期限切れのエラートーストを消去する（3秒後）
    pub fn tick_toasts(&mut self) {
        if let Some(toast) = &self.error_toast
            && toast.shown_at.elapsed().as_secs() >= 3
        {
            self.error_toast = None;
        }
    }

    /// Ctrl+B プレフィックスモードを開始する
    pub fn enter_prefix(&mut self) {
        self.prefix_mode = PrefixMode::CtrlB;
    }

    /// プレフィックスモードを解除する
    pub fn exit_prefix(&mut self) {
        self.prefix_mode = PrefixMode::None;
    }

    /// ヘルプオーバーレイの表示トグル
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
    fn full_refreshでペインが登録される() {
        let mut state = ClientState::new();
        let grid = Grid::new(80, 24);
        state.apply_server_message(ServerToClient::FullRefresh { pane_id: 1, grid });
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
    fn resizeで端末サイズが更新される() {
        let mut state = ClientState::new();
        state.resize(120, 40);
        assert_eq!(state.cols, 120);
        assert_eq!(state.rows, 40);
    }

    #[test]
    fn focused_pane_ペインなしはnone() {
        let state = ClientState::new();
        assert!(state.focused_pane().is_none());
    }

    #[test]
    fn 複数ペインの登録と最初のフォーカス() {
        let mut state = ClientState::new();
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 2,
            grid: Grid::new(80, 24),
        });
        // 最初の FullRefresh のペインがフォーカスされること
        assert_eq!(state.focused_pane_id, Some(1));
        assert!(state.panes.contains_key(&2));
    }

    #[test]
    fn pong_はパニックしない() {
        let mut state = ClientState::new();
        state.apply_server_message(ServerToClient::Pong);
    }

    #[test]
    fn error_はエラートーストに設定される() {
        let mut state = ClientState::new();
        state.apply_server_message(ServerToClient::Error {
            message: "test error".to_string(),
        });
        assert!(state.error_toast.is_some());
        assert_eq!(state.error_toast.as_ref().unwrap().message, "test error");
    }

    #[test]
    fn layout_changedでpane_layoutsが更新される() {
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
    fn prefix_modeのトグル() {
        let mut state = ClientState::new();
        assert_eq!(state.prefix_mode, PrefixMode::None);
        state.enter_prefix();
        assert_eq!(state.prefix_mode, PrefixMode::CtrlB);
        state.exit_prefix();
        assert_eq!(state.prefix_mode, PrefixMode::None);
    }

    #[test]
    fn floating_pane_events_は無視される() {
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
