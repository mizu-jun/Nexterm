//! スクロールバック検索 — `SearchState` と `ClientState` 上のインクリメンタル検索メソッド
//!
//! `state/mod.rs` から抽出した:
//! - `SearchState` — 検索クエリと現在マッチ位置の状態
//! - `impl ClientState` — `start_search` / `push_search_char` / `pop_search_char` /
//!   `search_next` / `search_prev` / `end_search` などのインクリメンタル検索操作

use super::ClientState;

/// インクリメンタル検索の状態
pub struct SearchState {
    pub query: String,
    pub is_active: bool,
    /// 現在ハイライト中の行インデックス（スクロールバック内）
    pub current_match: Option<usize>,
}

impl SearchState {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            is_active: false,
            current_match: None,
        }
    }
}

impl ClientState {
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

    /// 前のマッチへ移動する
    pub fn search_prev(&mut self) {
        let query = self.search.query.clone();
        let current = self.search.current_match.unwrap_or(0);
        let result = self
            .focused_pane_mut()
            .and_then(|pane| pane.scrollback.search_prev(&query, current));
        self.search.current_match = result;
        if let Some(row) = result
            && let Some(pane) = self.focused_pane_mut()
        {
            pane.scroll_offset = row;
        }
    }

    pub(super) fn search_next_from(&mut self, from: usize) {
        let query = self.search.query.clone();
        // 先に検索結果を取得してからボローを解放する
        let result = self
            .focused_pane_mut()
            .and_then(|pane| pane.scrollback.search_next(&query, from));
        self.search.current_match = result;
        if let Some(row) = result
            && let Some(pane) = self.focused_pane_mut()
        {
            pane.scroll_offset = row;
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
