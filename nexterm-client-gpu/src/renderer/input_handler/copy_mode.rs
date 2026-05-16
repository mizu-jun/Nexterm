//! コピーモード（tmux 互換）のキー入力処理
//!
//! `input_handler.rs` から抽出した:
//! - `handle_copy_mode_key` — 通常モードのキー入力
//! - `handle_copy_mode_search_key` — `/` で開く検索入力モード
//! - 単語境界ナビゲーション（w / b）
//! - インクリメンタル検索（n キー / Enter 確定）
//! - ヤンク（y / Y）— クリップボードへコピー

use winit::keyboard::KeyCode as WKeyCode;

use super::EventHandler;

impl EventHandler {
    /// コピーモードのキー入力を処理する（true = 消費済み）
    pub(super) fn handle_copy_mode_key(&mut self, code: WKeyCode) -> bool {
        // 検索入力中は専用ハンドラに委譲する
        if self.app.state.copy_mode.search_query.is_some() {
            return self.handle_copy_mode_search_key(code);
        }

        let cm = &mut self.app.state.copy_mode;
        let max_col = self.app.state.cols.saturating_sub(1);
        let max_row = self.app.state.rows.saturating_sub(1);

        match code {
            // q / Escape: コピーモードを終了する
            WKeyCode::KeyQ | WKeyCode::Escape => {
                cm.exit();
            }
            // h / Left: 左移動
            WKeyCode::KeyH | WKeyCode::ArrowLeft => {
                cm.cursor_col = cm.cursor_col.saturating_sub(1);
            }
            // l / Right: 右移動
            WKeyCode::KeyL | WKeyCode::ArrowRight => {
                if cm.cursor_col < max_col {
                    cm.cursor_col += 1;
                }
            }
            // j / Down: 下移動
            WKeyCode::KeyJ | WKeyCode::ArrowDown => {
                if cm.cursor_row < max_row {
                    cm.cursor_row += 1;
                }
            }
            // k / Up: 上移動
            WKeyCode::KeyK | WKeyCode::ArrowUp => {
                cm.cursor_row = cm.cursor_row.saturating_sub(1);
            }
            // 0: 行頭へ移動
            WKeyCode::Digit0 => {
                cm.cursor_col = 0;
            }
            // $: 行末へ移動
            WKeyCode::Digit4 => {
                // Shift+4 = '$' として扱う（WKeyCode には Dollar がないため）
                cm.cursor_col = max_col;
            }
            // w: 次の単語の先頭へ移動
            WKeyCode::KeyW => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_next_word_start(col, row, max_col, max_row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // b: 前の単語の先頭へ移動
            WKeyCode::KeyB => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_prev_word_start(col, row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // v: 選択開始/終了をトグル
            WKeyCode::KeyV => {
                cm.toggle_selection();
            }
            // y / Y: y=選択テキストをヤンク、Y=行全体をヤンク
            WKeyCode::KeyY => {
                if self.modifiers.shift_key() {
                    self.yank_current_line();
                } else {
                    self.yank_selection();
                }
            }
            // /: インクリメンタル検索モードへ
            WKeyCode::Slash => {
                self.app.state.copy_mode.search_query = Some(String::new());
            }
            // n: 次の検索結果へ
            WKeyCode::KeyN => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_query
                    .clone()
                    .unwrap_or_default();
                if !q.is_empty() {
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    if let Some((nc, nr)) = self.search_forward(&q, col + 1, row, max_col, max_row)
                    {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// 検索入力中のキー処理（true = 消費済み）
    fn handle_copy_mode_search_key(&mut self, code: WKeyCode) -> bool {
        match code {
            // Escape: 検索をキャンセルして通常コピーモードへ
            WKeyCode::Escape => {
                self.app.state.copy_mode.search_query = None;
            }
            // Enter: 検索確定して最初のマッチへジャンプ
            WKeyCode::Enter => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_query
                    .clone()
                    .unwrap_or_default();
                self.app.state.copy_mode.search_query = None;
                if !q.is_empty() {
                    let max_col = self.app.state.cols.saturating_sub(1);
                    let max_row = self.app.state.rows.saturating_sub(1);
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    if let Some((nc, nr)) = self.search_forward(&q, col, row, max_col, max_row) {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                        // 最後の検索クエリを保存して n キーで再利用できるようにする
                        self.app.state.copy_mode.search_query = Some(q);
                    }
                }
            }
            // Backspace: クエリの末尾を削除
            WKeyCode::Backspace => {
                if let Some(ref mut q) = self.app.state.copy_mode.search_query {
                    q.pop();
                }
            }
            _ => return false,
        }
        true
    }

    /// 次の単語の先頭位置を返す（見つからなければ None）
    fn find_next_word_start(
        &self,
        col: u16,
        row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as usize;
        let mut r = row as usize;

        // 現在位置が単語文字なら単語の終わりまでスキップ
        if let Some(cells) = pane.grid.rows.get(r) {
            while c < cells.len() && !cells[c].ch.is_whitespace() {
                c += 1;
            }
        }
        // 次の単語の先頭（空白をスキップ）
        loop {
            if let Some(cells) = pane.grid.rows.get(r) {
                while c < cells.len() {
                    if !cells[c].ch.is_whitespace() {
                        return Some((c as u16, r as u16));
                    }
                    c += 1;
                }
            }
            // 次の行へ
            if r >= max_row as usize {
                break;
            }
            r += 1;
            c = 0;
        }
        Some((max_col, max_row))
    }

    /// 前の単語の先頭位置を返す（見つからなければ None）
    fn find_prev_word_start(&self, col: u16, row: u16) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as isize - 1;
        let mut r = row as isize;

        // 現在位置の直前が空白ならスキップ
        loop {
            if c < 0 {
                if r <= 0 {
                    return Some((0, 0));
                }
                r -= 1;
                c = pane
                    .grid
                    .rows
                    .get(r as usize)
                    .map(|row| row.len() as isize - 1)
                    .unwrap_or(0);
            }
            if let Some(cells) = pane.grid.rows.get(r as usize)
                && c < cells.len() as isize
                && !cells[c as usize].ch.is_whitespace()
            {
                break;
            }
            c -= 1;
        }
        // 単語の先頭までスキップ
        loop {
            if c <= 0 {
                return Some((0, r as u16));
            }
            if let Some(cells) = pane.grid.rows.get(r as usize) {
                if c - 1 < cells.len() as isize && cells[(c - 1) as usize].ch.is_whitespace() {
                    break;
                }
            } else {
                break;
            }
            c -= 1;
        }
        Some((c as u16, r as u16))
    }

    /// 前方検索: クエリに最初にマッチする (col, row) を返す
    fn search_forward(
        &self,
        query: &str,
        start_col: u16,
        start_row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let rows_total = (max_row + 1) as usize;

        for dr in 0..rows_total {
            let r = ((start_row as usize) + dr) % rows_total;
            let cells = pane.grid.rows.get(r)?;
            let row_str: String = cells.iter().map(|c| c.ch).collect();
            let col_start = if dr == 0 { start_col as usize } else { 0 };
            let search_in = if col_start < row_str.len() {
                &row_str[col_start..]
            } else {
                continue;
            };
            if let Some(offset) = search_in.find(query) {
                let found_col = (col_start + offset).min(max_col as usize) as u16;
                return Some((found_col, r as u16));
            }
        }
        None
    }

    /// 選択範囲のテキストをクリップボードにコピーしてコピーモードを終了する
    fn yank_selection(&mut self) {
        let cm = &self.app.state.copy_mode;
        if let Some(((sc, sr), (ec, er))) = cm.normalized_selection() {
            // グリッドから選択テキストを抽出する
            let text = if let Some(pane) = self.app.state.focused_pane() {
                let mut lines = Vec::new();
                for row_idx in sr..=er {
                    if let Some(row) = pane.grid.rows.get(row_idx as usize) {
                        let col_start = if row_idx == sr { sc as usize } else { 0 };
                        let col_end = if row_idx == er {
                            (ec + 1) as usize
                        } else {
                            row.len()
                        };
                        let line: String = row[col_start.min(row.len())..col_end.min(row.len())]
                            .iter()
                            .map(|c| c.ch)
                            .collect();
                        lines.push(line);
                    }
                }
                lines.join("\n")
            } else {
                String::new()
            };

            if !text.is_empty()
                && let Ok(mut clipboard) = arboard::Clipboard::new()
            {
                let _ = clipboard.set_text(text);
            }
        }
        self.app.state.copy_mode.exit();
    }

    /// カーソル行全体をクリップボードにコピーしてコピーモードを終了する（Y キー）
    fn yank_current_line(&mut self) {
        let row_idx = self.app.state.copy_mode.cursor_row as usize;
        let text = if let Some(pane) = self.app.state.focused_pane() {
            pane.grid
                .rows
                .get(row_idx)
                .map(|row| row.iter().map(|c| c.ch).collect::<String>())
                .unwrap_or_default()
        } else {
            String::new()
        };
        if !text.is_empty()
            && let Ok(mut clipboard) = arboard::Clipboard::new()
        {
            let _ = clipboard.set_text(text);
        }
        self.app.state.copy_mode.exit();
    }
}
