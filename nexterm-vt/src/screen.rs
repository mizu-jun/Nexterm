//! 仮想スクリーン — グリッド + ダーティフラグ + カーソル状態

use nexterm_proto::{Attrs, Cell, Color, DirtyRow, Grid};

use crate::image::{decode_kitty, decode_sixel};

/// 配置待ち画像（クライアントへの送信前）
pub struct PendingImage {
    pub id: u32,
    pub col: u16,
    pub row: u16,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 仮想スクリーン（PTY 出力を反映する内部状態）
pub struct Screen {
    /// セル配列と寸法
    grid: Grid,
    /// 行単位ダーティフラグ（true = 変更あり）
    dirty: Vec<bool>,
    /// カーソル列（0始まり）
    cursor_col: u16,
    /// カーソル行（0始まり）
    cursor_row: u16,
    /// 現在の前景色
    current_fg: Color,
    /// 現在の背景色
    current_bg: Color,
    /// 現在の文字属性
    current_attrs: Attrs,
    /// スクロール領域の上端行（0始まり）
    scroll_top: u16,
    /// スクロール領域の下端行（0始まり）
    scroll_bottom: u16,
    // ---- DCS / Sixel 受信状態 ----
    /// Sixel DCS 受信中フラグ
    dcs_sixel_active: bool,
    /// DCS データバッファ
    dcs_buf: Vec<u8>,
    /// Sixel 開始時のカーソル位置
    dcs_cursor: (u16, u16),
    /// 完成した画像のキュー（take_pending_images で取り出す）
    pending_images: Vec<PendingImage>,
    /// 次の画像 ID
    next_image_id: u32,
    /// BEL 受信フラグ（take_pending_bell で取り出す）
    pending_bell: bool,
}

impl Screen {
    /// 指定サイズのスクリーンを生成する
    pub fn new(cols: u16, rows: u16) -> Self {
        let scroll_bottom = rows.saturating_sub(1);
        Self {
            grid: Grid::new(cols, rows),
            dirty: vec![false; rows as usize],
            cursor_col: 0,
            cursor_row: 0,
            current_fg: Color::Default,
            current_bg: Color::Default,
            current_attrs: Attrs::default(),
            scroll_top: 0,
            scroll_bottom,
            dcs_sixel_active: false,
            dcs_buf: Vec::new(),
            dcs_cursor: (0, 0),
            pending_images: Vec::new(),
            next_image_id: 1,
            pending_bell: false,
        }
    }

    /// グリッドへの参照を返す
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// カーソル位置を返す（列, 行）
    pub fn cursor(&self) -> (u16, u16) {
        (self.cursor_col, self.cursor_row)
    }

    /// 指定行がダーティかどうかを返す
    pub fn is_dirty(&self, row: u16) -> bool {
        self.dirty.get(row as usize).copied().unwrap_or(false)
    }

    /// 全ダーティフラグをクリアする
    pub fn clear_dirty(&mut self) {
        self.dirty.fill(false);
    }

    /// ダーティ行のみを収集して返す（差分転送用）
    pub fn take_dirty_rows(&mut self) -> Vec<DirtyRow> {
        let mut result = Vec::new();
        for (row_idx, dirty) in self.dirty.iter_mut().enumerate() {
            if *dirty {
                let cells = self.grid.rows[row_idx].clone();
                result.push(DirtyRow {
                    row: row_idx as u16,
                    cells,
                });
                *dirty = false;
            }
        }
        result
    }

    /// 全画面スナップショット（Full Refresh 用）
    pub fn full_refresh_grid(&self) -> Grid {
        let mut g = self.grid.clone();
        g.cursor_col = self.cursor_col;
        g.cursor_row = self.cursor_row;
        g
    }

    /// リサイズ処理（内容は可能な範囲でコピー）
    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        let mut new_grid = Grid::new(new_cols, new_rows);
        let copy_rows = self.grid.height.min(new_rows) as usize;
        let copy_cols = self.grid.width.min(new_cols) as usize;
        for r in 0..copy_rows {
            for c in 0..copy_cols {
                new_grid.rows[r][c] = self.grid.rows[r][c].clone();
            }
        }
        self.grid = new_grid;
        self.dirty = vec![true; new_rows as usize]; // リサイズ後は全行ダーティ
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);
    }

    /// カーソル位置に文字を書き込み、カーソルを進める
    pub(crate) fn write_char(&mut self, ch: char) {
        if self.cursor_col >= self.grid.width {
            // 行末で折り返し
            self.cursor_col = 0;
            self.advance_line();
        }
        let cell = Cell {
            ch,
            fg: self.current_fg,
            bg: self.current_bg,
            attrs: self.current_attrs,
        };
        let col = self.cursor_col;
        let row = self.cursor_row;
        self.grid.set(col, row, cell);
        self.mark_dirty(row);
        self.cursor_col += 1;
    }

    /// カーソルを次の行へ進める（スクロール処理を含む）
    pub(crate) fn advance_line(&mut self) {
        if self.cursor_row >= self.scroll_bottom {
            self.scroll_up();
        } else {
            self.cursor_row += 1;
        }
    }

    /// スクロール領域を1行上にスクロールする
    fn scroll_up(&mut self) {
        let top = self.scroll_top as usize;
        let bottom = self.scroll_bottom as usize;
        // 領域内の行を1行ずつ上にコピー
        for r in top..bottom {
            self.grid.rows[r] = self.grid.rows[r + 1].clone();
            self.mark_dirty(r as u16);
        }
        // 最下行をクリア
        let width = self.grid.width as usize;
        self.grid.rows[bottom] = vec![Cell::default(); width];
        self.mark_dirty(bottom as u16);
    }

    /// 指定行をダーティとしてマークする
    fn mark_dirty(&mut self, row: u16) {
        if let Some(d) = self.dirty.get_mut(row as usize) {
            *d = true;
        }
    }

    /// SGR（Select Graphic Rendition）属性を適用する
    pub(crate) fn apply_sgr(&mut self, params: &[u16]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => {
                    // リセット
                    self.current_fg = Color::Default;
                    self.current_bg = Color::Default;
                    self.current_attrs = Attrs::default();
                }
                1 => self.current_attrs.0 |= Attrs::BOLD,
                3 => self.current_attrs.0 |= Attrs::ITALIC,
                4 => self.current_attrs.0 |= Attrs::UNDERLINE,
                5 => self.current_attrs.0 |= Attrs::BLINK,
                7 => self.current_attrs.0 |= Attrs::REVERSE,
                9 => self.current_attrs.0 |= Attrs::STRIKETHROUGH,
                22 => self.current_attrs.0 &= !Attrs::BOLD,
                24 => self.current_attrs.0 &= !Attrs::UNDERLINE,
                27 => self.current_attrs.0 &= !Attrs::REVERSE,
                // 前景色: 30〜37 (ANSI), 38 (拡張), 39 (デフォルト)
                30..=37 => self.current_fg = Color::Indexed(params[i] as u8 - 30),
                38 => {
                    if params.get(i + 1) == Some(&5) && params.get(i + 2).is_some() {
                        self.current_fg = Color::Indexed(params[i + 2] as u8);
                        i += 2;
                    } else if params.get(i + 1) == Some(&2)
                        && i + 4 < params.len()
                    {
                        self.current_fg = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                39 => self.current_fg = Color::Default,
                // 背景色: 40〜47 (ANSI), 48 (拡張), 49 (デフォルト)
                40..=47 => self.current_bg = Color::Indexed(params[i] as u8 - 40),
                48 => {
                    if params.get(i + 1) == Some(&5) && params.get(i + 2).is_some() {
                        self.current_bg = Color::Indexed(params[i + 2] as u8);
                        i += 2;
                    } else if params.get(i + 1) == Some(&2)
                        && i + 4 < params.len()
                    {
                        self.current_bg = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                49 => self.current_bg = Color::Default,
                // 明るい前景色: 90〜97
                90..=97 => self.current_fg = Color::Indexed(params[i] as u8 - 90 + 8),
                // 明るい背景色: 100〜107
                100..=107 => self.current_bg = Color::Indexed(params[i] as u8 - 100 + 8),
                _ => {} // 未対応の属性は無視
            }
            i += 1;
        }
    }

    /// カーソルを指定位置へ移動する（0始まり座標）
    pub(crate) fn move_cursor(&mut self, col: u16, row: u16) {
        self.cursor_col = col.min(self.grid.width.saturating_sub(1));
        self.cursor_row = row.min(self.grid.height.saturating_sub(1));
    }

    /// スクロール領域を設定する（DECSTBM）
    pub(crate) fn set_scroll_region(&mut self, top: u16, bottom: u16) {
        let max = self.grid.height.saturating_sub(1);
        self.scroll_top = top.min(max);
        self.scroll_bottom = bottom.min(max);
        // DECSTBM はカーソルをホームポジションへ移動する
        self.cursor_col = 0;
        self.cursor_row = 0;
    }

    /// カーソル行をクリアする（行の一部または全体）
    pub(crate) fn erase_in_line(&mut self, mode: u16) {
        let row = self.cursor_row;
        let width = self.grid.width as usize;
        match mode {
            0 => {
                // カーソルから行末までクリア
                for c in self.cursor_col as usize..width {
                    self.grid.rows[row as usize][c] = Cell::default();
                }
            }
            1 => {
                // 行頭からカーソルまでクリア
                for c in 0..=self.cursor_col as usize {
                    self.grid.rows[row as usize][c] = Cell::default();
                }
            }
            2 => {
                // 行全体をクリア
                self.grid.rows[row as usize] = vec![Cell::default(); width];
            }
            _ => {}
        }
        self.mark_dirty(row);
    }

    /// 画面をクリアする（一部または全体）
    pub(crate) fn erase_in_display(&mut self, mode: u16) {
        let height = self.grid.height as usize;
        let width = self.grid.width as usize;
        match mode {
            0 => {
                // カーソルから画面末までクリア
                self.erase_in_line(0);
                for r in (self.cursor_row as usize + 1)..height {
                    self.grid.rows[r] = vec![Cell::default(); width];
                    self.mark_dirty(r as u16);
                }
            }
            1 => {
                // 画面頭からカーソルまでクリア
                for r in 0..self.cursor_row as usize {
                    self.grid.rows[r] = vec![Cell::default(); width];
                    self.mark_dirty(r as u16);
                }
                self.erase_in_line(1);
            }
            2 | 3 => {
                // 画面全体をクリア
                for r in 0..height {
                    self.grid.rows[r] = vec![Cell::default(); width];
                    self.mark_dirty(r as u16);
                }
                if mode == 2 {
                    self.cursor_col = 0;
                    self.cursor_row = 0;
                }
            }
            _ => {}
        }
    }

    // ---- DCS / Sixel / Kitty 画像処理 ----

    /// Sixel DCS 受信を開始する（hook 呼び出し時）
    pub(crate) fn start_sixel(&mut self) {
        self.dcs_sixel_active = true;
        self.dcs_buf.clear();
        self.dcs_cursor = (self.cursor_col, self.cursor_row);
    }

    /// DCS バイトをバッファに追加する（put 呼び出し時）
    pub(crate) fn push_dcs_byte(&mut self, byte: u8) {
        if self.dcs_sixel_active {
            self.dcs_buf.push(byte);
        }
    }

    /// Sixel DCS を完了してデコードし、pending_images に積む（unhook 呼び出し時）
    pub(crate) fn finish_sixel(&mut self) {
        if !self.dcs_sixel_active {
            return;
        }
        self.dcs_sixel_active = false;
        if let Some(img) = decode_sixel(&self.dcs_buf) {
            let id = self.next_image_id;
            self.next_image_id += 1;
            self.pending_images.push(PendingImage {
                id,
                col: self.dcs_cursor.0,
                row: self.dcs_cursor.1,
                width: img.width,
                height: img.height,
                rgba: img.rgba,
            });
        }
        self.dcs_buf.clear();
    }

    /// Kitty APC 画像をデコードして pending_images に積む
    pub(crate) fn handle_kitty_apc(&mut self, data: &[u8]) {
        if let Some(img) = decode_kitty(data) {
            let id = self.next_image_id;
            self.next_image_id += 1;
            self.pending_images.push(PendingImage {
                id,
                col: self.cursor_col,
                row: self.cursor_row,
                width: img.width,
                height: img.height,
                rgba: img.rgba,
            });
        }
    }

    /// 蓄積された画像を取り出してキューをクリアする
    pub fn take_pending_images(&mut self) -> Vec<PendingImage> {
        std::mem::take(&mut self.pending_images)
    }

    /// BEL フラグを取り出してクリアする
    pub fn take_pending_bell(&mut self) -> bool {
        std::mem::replace(&mut self.pending_bell, false)
    }

    /// BEL を設定する（performer から呼ばれる）
    pub(crate) fn set_pending_bell(&mut self) {
        self.pending_bell = true;
    }
}
