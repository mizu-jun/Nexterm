//! クライアント状態 — グリッド・スクロールバック・パレット・検索を統合管理する

use std::collections::HashMap;

use nexterm_proto::{Grid, PaneLayout, ServerToClient};

use crate::palette::CommandPalette;
use crate::scrollback::Scrollback;

/// 配置済み画像
pub struct PlacedImage {
    pub col: u16,
    pub row: u16,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// ペインの描画状態
pub struct PaneState {
    pub grid: Grid,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub scrollback: Scrollback,
    /// スクロールバックのオフセット（0 = 最新画面）
    pub scroll_offset: usize,
    /// 配置済み画像（image_id → PlacedImage）
    pub images: HashMap<u32, PlacedImage>,
    /// バックグラウンドアクティビティフラグ（非フォーカス時に出力があると true）
    pub has_activity: bool,
}

impl PaneState {
    fn new(cols: u16, rows: u16, scrollback_capacity: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            cursor_col: 0,
            cursor_row: 0,
            scrollback: Scrollback::new(scrollback_capacity),
            scroll_offset: 0,
            images: HashMap::new(),
            has_activity: false,
        }
    }

    fn apply_diff(
        &mut self,
        dirty_rows: Vec<nexterm_proto::DirtyRow>,
        cursor_col: u16,
        cursor_row: u16,
    ) {
        for dirty in dirty_rows {
            if let Some(row) = self.grid.rows.get_mut(dirty.row as usize) {
                // スクロールアウト前の行をスクロールバックに積む
                self.scrollback.push_line(row.clone());
                *row = dirty.cells;
            }
        }
        self.cursor_col = cursor_col;
        self.cursor_row = cursor_row;
        // 新しい出力が来たら最新画面にスクロールバックする
        self.scroll_offset = 0;
    }
}

/// インクリメンタル検索の状態
pub struct SearchState {
    pub query: String,
    pub is_active: bool,
    /// 現在ハイライト中の行インデックス（スクロールバック内）
    pub current_match: Option<usize>,
}

impl SearchState {
    fn new() -> Self {
        Self {
            query: String::new(),
            is_active: false,
            current_match: None,
        }
    }
}

/// GPU クライアント全体の状態
pub struct ClientState {
    pub panes: HashMap<u32, PaneState>,
    pub focused_pane_id: Option<u32>,
    /// サーバーから受信したペインレイアウト情報（分割表示に使用）
    pub pane_layouts: HashMap<u32, PaneLayout>,
    pub cols: u16,
    pub rows: u16,
    pub palette: CommandPalette,
    pub search: SearchState,
    /// 設定で指定されたスクロールバック行数
    pub scrollback_capacity: usize,
    /// Lua ステータスバーの最終評価テキスト（キャッシュ）
    pub status_bar_text: String,
}

impl ClientState {
    pub fn new(cols: u16, rows: u16, scrollback_capacity: usize) -> Self {
        Self {
            panes: HashMap::new(),
            focused_pane_id: None,
            pane_layouts: HashMap::new(),
            cols,
            rows,
            palette: CommandPalette::new(),
            search: SearchState::new(),
            scrollback_capacity,
            status_bar_text: String::new(),
        }
    }

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
                    // 非フォーカスペインへの出力はアクティビティとしてマーク
                    if self.focused_pane_id != Some(pane_id) {
                        pane.has_activity = true;
                    }
                }
            }
            ServerToClient::Pong => {}
            ServerToClient::Error { message } => {
                tracing::error!("サーバーエラー: {}", message);
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
                    pane.images.insert(image_id, PlacedImage { col, row, width, height, rgba });
                    if self.focused_pane_id != Some(pane_id) {
                        pane.has_activity = true;
                    }
                }
            }
            ServerToClient::LayoutChanged { panes, focused_pane_id } => {
                // レイアウトを全更新する
                self.pane_layouts.clear();
                for layout in panes {
                    self.pane_layouts.insert(layout.pane_id, layout);
                }
                // フォーカスペインを更新してアクティビティフラグをクリアする
                self.focused_pane_id = Some(focused_pane_id);
                if let Some(pane) = self.panes.get_mut(&focused_pane_id) {
                    pane.has_activity = false;
                }
            }
        }
    }

    /// フォーカスペインを切り替え、アクティビティフラグをクリアする
    pub fn set_focused_pane(&mut self, pane_id: u32) {
        self.focused_pane_id = Some(pane_id);
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.has_activity = false;
        }
    }

    /// バックグラウンドアクティビティのあるペイン ID 一覧を返す
    pub fn active_pane_ids(&self) -> Vec<u32> {
        self.panes
            .iter()
            .filter(|(_, p)| p.has_activity)
            .map(|(&id, _)| id)
            .collect()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
    }

    pub fn focused_pane(&self) -> Option<&PaneState> {
        self.focused_pane_id.and_then(|id| self.panes.get(&id))
    }

    pub fn focused_pane_mut(&mut self) -> Option<&mut PaneState> {
        self.focused_pane_id
            .and_then(|id| self.panes.get_mut(&id))
    }

    /// スクロールバック検索を開始する
    pub fn start_search(&mut self) {
        self.search.is_active = true;
        self.search.query.clear();
        self.search.current_match = None;
    }

    /// 検索クエリに文字を追加してインクリメンタルに検索する
    pub fn push_search_char(&mut self, ch: char) {
        self.search.query.push(ch);
        self.search_next_from(0);
    }

    /// 検索クエリの末尾を削除する
    pub fn pop_search_char(&mut self) {
        self.search.query.pop();
        self.search_next_from(0);
    }

    /// 次のマッチへ移動する
    pub fn search_next(&mut self) {
        let from = self.search.current_match.map(|m| m + 1).unwrap_or(0);
        self.search_next_from(from);
    }

    fn search_next_from(&mut self, from: usize) {
        let query = self.search.query.clone();
        // 先に検索結果を取得してからボローを解放する
        let result = self
            .focused_pane_mut()
            .and_then(|pane| pane.scrollback.search_next(&query, from));
        self.search.current_match = result;
        if let Some(row) = result {
            if let Some(pane) = self.focused_pane_mut() {
                pane.scroll_offset = row;
            }
        }
    }

    /// スクロールバックを1画面分上にスクロールする
    pub fn scroll_up(&mut self, lines: usize) {
        if let Some(pane) = self.focused_pane_mut() {
            let max_offset = pane.scrollback.len().saturating_sub(1);
            pane.scroll_offset = (pane.scroll_offset + lines).min(max_offset);
        }
    }

    /// スクロールバックを1画面分下にスクロールする
    pub fn scroll_down(&mut self, lines: usize) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.scroll_offset = pane.scroll_offset.saturating_sub(lines);
        }
    }

    /// 検索を終了する
    pub fn end_search(&mut self) {
        self.search.is_active = false;
        self.search.query.clear();
        self.search.current_match = None;
        if let Some(pane) = self.focused_pane_mut() {
            pane.scroll_offset = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_proto::{Cell, DirtyRow, Grid};

    #[test]
    fn full_refreshでペインが登録される() {
        let mut state = ClientState::new(80, 24, 1000);
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        assert!(state.panes.contains_key(&1));
        assert_eq!(state.focused_pane_id, Some(1));
    }

    #[test]
    fn grid_diffで差分が適用される() {
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
    fn 検索のライフサイクル() {
        let mut state = ClientState::new(80, 24, 1000);
        state.start_search();
        assert!(state.search.is_active);
        state.push_search_char('a');
        assert_eq!(state.search.query, "a");
        state.end_search();
        assert!(!state.search.is_active);
        assert!(state.search.query.is_empty());
    }
}
