//! テキスト選択関連 — URL 検出、マウスドラッグ選択、コピーモード
//!
//! `state/mod.rs` から抽出した:
//! - `DetectedUrl` + `detect_urls_in_row` — グリッド行から URL を検出
//! - `MouseSelection` — マウスドラッグによるテキスト選択状態
//! - `CopyModeState` — tmux 互換の Vim 風コピーモード

/// グリッド上の URL とその範囲（アンダーライン描画・クリック判定に使用）
#[derive(Debug, Clone)]
pub struct DetectedUrl {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub url: String,
}

impl DetectedUrl {
    /// 指定のグリッドセルがこの URL の範囲内にあるかどうかを返す
    pub fn contains(&self, col: u16, row: u16) -> bool {
        row == self.row && col >= self.col_start && col < self.col_end
    }
}

/// グリッドの行テキストから URL を検出して返す
pub fn detect_urls_in_row(row_idx: u16, cells: &[nexterm_proto::Cell]) -> Vec<DetectedUrl> {
    let text: String = cells.iter().map(|c| c.ch).collect();
    let mut urls = Vec::new();

    // https:// または http:// から始まる URL を検出する
    let prefixes = ["https://", "http://"];
    for prefix in prefixes {
        let mut search_from = 0;
        while let Some(start) = text[search_from..].find(prefix) {
            let abs_start = search_from + start;
            // URL の終端はスペース・制御文字・括弧で区切られる
            let end = text[abs_start..]
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')'))
                .map(|i| abs_start + i)
                .unwrap_or(text.len());
            if end > abs_start {
                urls.push(DetectedUrl {
                    row: row_idx,
                    col_start: abs_start as u16,
                    col_end: end as u16,
                    url: text[abs_start..end].to_string(),
                });
            }
            search_from = abs_start + 1;
        }
    }
    urls
}

/// マウスドラッグによるテキスト選択状態
pub struct MouseSelection {
    /// ドラッグ中かどうか
    pub is_dragging: bool,
    /// 選択開始セル（グリッド座標）
    pub start: (u16, u16),
    /// 選択終了セル（グリッド座標、ドラッグ中は随時更新）
    pub end: (u16, u16),
}

impl MouseSelection {
    pub fn new() -> Self {
        Self {
            is_dragging: false,
            start: (0, 0),
            end: (0, 0),
        }
    }

    /// ドラッグ開始
    pub fn begin(&mut self, col: u16, row: u16) {
        self.is_dragging = true;
        self.start = (col, row);
        self.end = (col, row);
    }

    /// ドラッグ中の終端更新
    pub fn update(&mut self, col: u16, row: u16) {
        if self.is_dragging {
            self.end = (col, row);
        }
    }

    /// ドラッグ終了
    pub fn finish(&mut self) {
        self.is_dragging = false;
    }

    /// 選択範囲を正規化して返す（start <= end を保証）
    /// 何も選択されていない（start == end）場合は None を返す
    pub fn normalized(&self) -> Option<((u16, u16), (u16, u16))> {
        let (sc, sr) = self.start;
        let (ec, er) = self.end;
        if (sr, sc) == (er, ec) {
            return None;
        }
        if (sr, sc) <= (er, ec) {
            Some(((sc, sr), (ec, er)))
        } else {
            Some(((ec, er), (sc, sr)))
        }
    }

    /// 指定セルが選択範囲内かどうかを返す
    pub fn contains(&self, col: u16, row: u16) -> bool {
        if let Some(((sc, sr), (ec, er))) = self.normalized() {
            if row < sr || row > er {
                return false;
            }
            if row == sr && row == er {
                return col >= sc && col <= ec;
            }
            if row == sr {
                return col >= sc;
            }
            if row == er {
                return col <= ec;
            }
            true
        } else {
            false
        }
    }
}

/// コピーモード（Vim 風テキスト選択）の状態
pub struct CopyModeState {
    /// コピーモードが有効かどうか
    pub is_active: bool,
    /// カーソル列（グリッド座標、0始まり）
    pub cursor_col: u16,
    /// カーソル行（グリッド座標、0始まり）
    pub cursor_row: u16,
    /// 選択開始位置（v を押した時点のカーソル位置）
    pub selection_start: Option<(u16, u16)>,
    /// インクリメンタル検索クエリ（Some の間は検索入力中）
    pub search_query: Option<String>,
}

impl CopyModeState {
    pub(super) fn new() -> Self {
        Self {
            is_active: false,
            cursor_col: 0,
            cursor_row: 0,
            selection_start: None,
            search_query: None,
        }
    }

    /// コピーモードを開始してカーソルを現在のペインカーソルに合わせる
    pub fn enter(&mut self, pane_cursor_col: u16, pane_cursor_row: u16) {
        self.is_active = true;
        self.cursor_col = pane_cursor_col;
        self.cursor_row = pane_cursor_row;
        self.selection_start = None;
    }

    /// コピーモードを終了する
    pub fn exit(&mut self) {
        self.is_active = false;
        self.selection_start = None;
        self.search_query = None;
    }

    /// 選択開始/終了をトグルする（v キー）
    pub fn toggle_selection(&mut self) {
        if self.selection_start.is_some() {
            self.selection_start = None;
        } else {
            self.selection_start = Some((self.cursor_col, self.cursor_row));
        }
    }

    /// 選択範囲を正規化して返す（開始 ≤ 終了 を保証する）
    pub fn normalized_selection(&self) -> Option<((u16, u16), (u16, u16))> {
        let (sc, sr) = self.selection_start?;
        let (ec, er) = (self.cursor_col, self.cursor_row);
        if (sr, sc) <= (er, ec) {
            Some(((sc, sr), (ec, er)))
        } else {
            Some(((ec, er), (sc, sr)))
        }
    }
}
