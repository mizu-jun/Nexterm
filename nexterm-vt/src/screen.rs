//! 仮想スクリーン — グリッド + ダーティフラグ + カーソル状態

use nexterm_proto::{Attrs, Cell, Color, DirtyRow, Grid};
use unicode_width::UnicodeWidthChar;

use crate::image::{decode_kitty, decode_sixel};

/// DCS Sixel バッファの最大サイズ。
///
/// 悪意ある PTY が DCS を終端しないまま延々送り続ける DoS への対策（CRITICAL #7）。
/// 16 MiB は 1 ページの大きな Sixel 画像 + マージン。
const MAX_DCS_BUF_LEN: usize = 16 * 1024 * 1024;

/// Kitty 分割転送ペイロードの最大サイズ。
///
/// `m=1` チャンクで延々と送られ続ける場合の DoS 対策（CRITICAL #7）。
/// 64 MiB は分割画像の合計上限。
const MAX_KITTY_CHUNK_LEN: usize = 64 * 1024 * 1024;

/// OSC タイトル文字列の最大長（バイト）。
///
/// 長大なタイトルでターミナルバーやウィンドウマネージャーへの DoS を防ぐ
/// （CRITICAL #13 対応）。256 バイトはほぼ全ターミナルの実用範囲をカバー。
const MAX_TITLE_LEN: usize = 256;

/// OSC 9 デスクトップ通知のタイトル/本文の最大長（バイト）。
const MAX_NOTIFICATION_LEN: usize = 1024;

/// OSC 8 ハイパーリンク URI の最大長（バイト）。
const MAX_HYPERLINK_URI_LEN: usize = 2048;

/// OSC 7 CWD (working directory) パスの最大長（バイト）。
///
/// Linux の `PATH_MAX` 相当の 4096。長すぎるパスは DoS 防止のため切り詰める。
const MAX_CWD_LEN: usize = 4096;

/// OSC 8 ハイパーリンクで許可する URI スキーム。
///
/// 旧実装は `javascript:` / `file:` 等を含む全スキームを通過させていたため、
/// 悪意ある SSH 接続先がターミナル経由でクリックジャッキング・ローカル
/// ファイル参照を誘発できる脆弱性があった（CRITICAL #13）。
const ALLOWED_HYPERLINK_SCHEMES: &[&str] = &[
    "http://", "https://", "mailto:", "ftp://", "ftps://", "ssh://",
];

/// OSC 由来の文字列をサニタイズする。
///
/// - 制御文字（C0/C1）を除去（ログインジェクション・改行偽装防止）
/// - 長さ上限で切り詰め（CJK 等の UTF-8 境界を尊重）
fn sanitize_osc_string(s: String, max_len: usize) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| {
            // C0 (0x00-0x1F、ただし TAB/LF/CR は除外)、DEL、C1 (0x80-0x9F) を除去
            let cp = *c as u32;
            !(cp <= 0x1F && *c != '\t' && *c != '\n' && *c != '\r')
                && cp != 0x7F
                && !(0x80..=0x9F).contains(&cp)
        })
        .collect();

    if cleaned.len() <= max_len {
        return cleaned;
    }
    // バイト境界で安全に切り詰める
    let mut end = max_len;
    while end > 0 && !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    cleaned[..end].to_string()
}

/// OSC 8 ハイパーリンク URI を検証する。
///
/// 許可スキームに該当しない / 長すぎる URI は `None` を返す（無効化）。
pub(crate) fn validate_hyperlink_uri(uri: &str) -> Option<String> {
    if uri.is_empty() || uri.len() > MAX_HYPERLINK_URI_LEN {
        return None;
    }
    let lower = uri.to_lowercase();
    if !ALLOWED_HYPERLINK_SCHEMES
        .iter()
        .any(|s| lower.starts_with(s))
    {
        tracing::warn!(
            "OSC 8: 許可されていない URI スキーム — 無効化: {}",
            &uri[..uri.len().min(80)]
        );
        return None;
    }
    // 制御文字を除去
    let cleaned: String = uri
        .chars()
        .filter(|c| {
            let cp = *c as u32;
            cp >= 0x20 && cp != 0x7F && !(0x80..=0x9F).contains(&cp)
        })
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned)
}

/// OSC 7 の `file://[host]/percent-encoded-path` 形式から CWD パスを抽出する。
///
/// 受け付ける入力例:
/// - `file:///home/user/proj` (host 省略、先頭 `/` 維持)
/// - `file://hostname/home/user` (host あり、無視して path のみ取り出す)
/// - `file:///C:/Users/foo` (Windows 形式、先頭の `/` を除去して `C:/Users/foo` に正規化)
/// - `/home/user/proj` (スキームなし、互換目的で素通し)
///
/// 制御文字は除去し、`MAX_CWD_LEN` で切り詰める。
/// 完全に空になった場合は `None`。
pub(crate) fn parse_osc7_cwd(input: &str) -> Option<String> {
    if input.is_empty() || input.len() > MAX_CWD_LEN * 4 {
        // 過大な入力は DoS 防止で即拒否（パーセントデコード前なので *4 程度の余裕）
        return None;
    }

    // `file://` プレフィックスを除去し、ホスト部 (`//host/path`) もスキップする
    let after_scheme = if let Some(rest) = input.strip_prefix("file://") {
        // `/path` まで進める（host 部が空でもそうでなくても最初の `/` まで）
        match rest.find('/') {
            Some(idx) => &rest[idx..],
            None => return None, // パスがない
        }
    } else {
        input
    };

    // パーセントデコード
    let decoded = percent_decode(after_scheme);

    // Windows パス対応: `/C:/Users/foo` → `C:/Users/foo`
    #[cfg(windows)]
    let decoded = if decoded.len() >= 3
        && decoded.starts_with('/')
        && decoded.as_bytes()[2] == b':'
        && decoded.as_bytes()[1].is_ascii_alphabetic()
    {
        decoded[1..].to_string()
    } else {
        decoded
    };

    // 制御文字除去 + 長さ上限
    let cleaned = sanitize_osc_string(decoded, MAX_CWD_LEN);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// `%XX` 形式のパーセントエンコーディングをデコードする（OSC 7 用の最小実装）。
///
/// 不正な `%XX` は素通し（`%` を含むパス名を破壊しないため）。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // 不正な UTF-8 はロスでデコード（OSC 7 path には通常 UTF-8 が来る）
    String::from_utf8_lossy(&out).into_owned()
}

/// OSC 133 セマンティックゾーンのマーク種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticMarkKind {
    /// プロンプト開始（A）
    PromptStart,
    /// コマンド入力開始（B）
    CommandStart,
    /// コマンド実行開始・出力開始（C）
    OutputStart,
    /// コマンド終了（D）
    CommandEnd,
}

/// OSC 133 セマンティックゾーンのマーク（行番号 + 種別）
#[derive(Debug, Clone)]
pub struct SemanticMark {
    /// グリッド行番号（0始まり）
    pub row: u16,
    /// 種別
    pub kind: SemanticMarkKind,
    /// コマンド終了時の exit code（D マーク時のみ Some）
    pub exit_code: Option<i32>,
}

/// 配置待ち画像（クライアントへの送信前）
pub struct PendingImage {
    /// 画像 ID（Kitty プロトコルで使用）
    pub id: u32,
    /// 配置先の列（文字セル単位）
    pub col: u16,
    /// 配置先の行（文字セル単位）
    pub row: u16,
    /// 画像の幅（ピクセル）
    pub width: u32,
    /// 画像の高さ（ピクセル）
    pub height: u32,
    /// RGBA ピクセルデータ（width × height × 4 バイト）
    pub rgba: Vec<u8>,
}

/// スクリーンバッファの内容（主画面と代替画面で共有）
struct ScreenBuffer {
    rows: Vec<Vec<Cell>>,
    cursor_col: u16,
    cursor_row: u16,
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
    /// Kitty 分割転送のペイロード累積バッファ（m=1 チャンク）
    kitty_chunk_payload: Vec<u8>,
    /// Kitty 分割転送の最初のチャンクパラメータ（m=1 時に保存）
    kitty_chunk_params: Option<Vec<u8>>,
    /// BEL 受信フラグ（take_pending_bell で取り出す）
    pending_bell: bool,
    /// タイトル変更通知（OSC 0/1/2 で設定）
    pending_title: Option<String>,
    /// デスクトップ通知（OSC 9 で設定）
    pending_notification: Option<(String, String)>,
    /// CWD 変更通知（OSC 7 で設定、`file://` 除去 + パーセントデコード済み）
    pending_cwd: Option<String>,
    /// 代替スクリーンバッファ（None = 主画面モード）
    alt_screen: Option<Box<ScreenBuffer>>,
    /// 代替画面モード中か
    pub alt_mode: bool,
    /// ブラケットペーストモード（DEC ?2004）が有効か
    bracketed_paste: bool,
    /// 同期出力モード（DEC ?2026）が有効か（有効中はダーティフラグを溜める）
    synchronized_output: bool,
    /// マウスレポーティングモード（X11 ?1000=1, SGR ?1006=2, 0=無効）
    pub mouse_mode: u8,
    /// OSC 133 セマンティックゾーンのマーク一覧（行番号 + マーク種別）
    pub semantic_marks: Vec<SemanticMark>,
    /// 現在アクティブな OSC 8 ハイパーリンク URL（None = リンクなし）
    current_hyperlink_url: Option<String>,
    /// OSC 8 ハイパーリンクの開始列（current_hyperlink_url が Some の場合に有効）
    hyperlink_start_col: u16,
    /// OSC 8 ハイパーリンクの開始行
    hyperlink_start_row: u16,
    /// OSC 52 で受信したクリップボード書き込み要求のキュー（Sprint 4-1）
    /// クライアント側で SecurityConfig.osc52_clipboard ポリシーに従って処理する
    pending_clipboard_writes: Vec<String>,
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
            kitty_chunk_payload: Vec::new(),
            kitty_chunk_params: None,
            pending_bell: false,
            pending_title: None,
            pending_notification: None,
            pending_cwd: None,
            alt_screen: None,
            alt_mode: false,
            bracketed_paste: false,
            synchronized_output: false,
            mouse_mode: 0,
            semantic_marks: Vec::new(),
            current_hyperlink_url: None,
            hyperlink_start_col: 0,
            hyperlink_start_row: 0,
            pending_clipboard_writes: Vec::new(),
        }
    }

    /// 代替画面バッファに切り替える（SMCUP / DEC Private Mode 47/1047/1049）
    pub(crate) fn switch_to_alt(&mut self) {
        if self.alt_mode {
            return;
        }
        // 現在の主画面内容を保存する
        let saved = ScreenBuffer {
            rows: self.grid.rows.clone(),
            cursor_col: self.cursor_col,
            cursor_row: self.cursor_row,
        };
        self.alt_screen = Some(Box::new(saved));
        // 代替バッファを空白で初期化する
        let cols = self.grid.width;
        let rows = self.grid.height;
        self.grid = Grid::new(cols, rows);
        self.dirty = vec![true; rows as usize];
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.scroll_top = 0;
        self.scroll_bottom = rows.saturating_sub(1);
        self.alt_mode = true;
    }

    /// 主画面バッファに戻る（RMCUP / DEC Private Mode 47/1047/1049 リセット）
    pub(crate) fn switch_to_primary(&mut self) {
        if !self.alt_mode {
            return;
        }
        if let Some(saved) = self.alt_screen.take() {
            self.grid.rows = saved.rows;
            self.cursor_col = saved.cursor_col;
            self.cursor_row = saved.cursor_row;
            self.dirty = vec![true; self.grid.height as usize];
        }
        self.scroll_top = 0;
        self.scroll_bottom = self.grid.height.saturating_sub(1);
        self.alt_mode = false;
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
    ///
    /// 同期出力モード（DEC ?2026）が有効な場合は空を返し、
    /// 無効化されたタイミングで蓄積分をまとめて返す。
    pub fn take_dirty_rows(&mut self) -> Vec<DirtyRow> {
        // 同期出力モード中はレンダリングを保留する
        if self.synchronized_output {
            return Vec::new();
        }
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

    /// 同期出力モード（DEC ?2026）の状態を返す
    pub fn synchronized_output_mode(&self) -> bool {
        self.synchronized_output
    }

    /// 同期出力モードを設定する（performer から呼び出す）
    pub(crate) fn set_synchronized_output(&mut self, enabled: bool) {
        self.synchronized_output = enabled;
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
        // 文字の表示幅を取得する（CJK 全角は 2、通常は 1）
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(1) as u16;

        if self.cursor_col >= self.grid.width {
            // 行末で折り返し
            self.cursor_col = 0;
            self.advance_line();
        }

        // ワイド文字が行末からはみ出す場合は次行に折り返す
        if char_width == 2 && self.cursor_col + 1 >= self.grid.width {
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

        // ワイド文字の場合は次のカラムにプレースホルダーセルを配置する
        if char_width == 2 && self.cursor_col < self.grid.width {
            let placeholder = Cell {
                ch: ' ',
                fg: self.current_fg,
                bg: self.current_bg,
                attrs: self.current_attrs,
            };
            self.grid.set(self.cursor_col, row, placeholder);
            self.cursor_col += 1;
        }
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
        // 領域内の行を1行ずつ上にコピー（直接インデックスアクセスを避けてパニック防止）
        for r in top..bottom {
            self.grid.copy_row(r as u16, (r + 1) as u16);
            self.mark_dirty(r as u16);
        }
        // 最下行をクリア
        self.grid.clear_row(bottom as u16);
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
                    } else if params.get(i + 1) == Some(&2) && i + 4 < params.len() {
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
                    } else if params.get(i + 1) == Some(&2) && i + 4 < params.len() {
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
                // カーソルから行末までクリア（Grid::set で範囲チェック済み）
                for c in self.cursor_col as usize..width {
                    self.grid.set(c as u16, row, Cell::default());
                }
            }
            1 => {
                // 行頭からカーソルまでクリア（Grid::set で範囲チェック済み）
                for c in 0..=self.cursor_col as usize {
                    self.grid.set(c as u16, row, Cell::default());
                }
            }
            2 => {
                // 行全体をクリア（Grid::clear_row で範囲チェック済み）
                self.grid.clear_row(row);
            }
            _ => {}
        }
        self.mark_dirty(row);
    }

    /// 画面をクリアする（一部または全体）
    pub(crate) fn erase_in_display(&mut self, mode: u16) {
        let height = self.grid.height as usize;
        match mode {
            0 => {
                // カーソルから画面末までクリア（Grid::clear_row で範囲チェック済み）
                self.erase_in_line(0);
                for r in (self.cursor_row as usize + 1)..height {
                    self.grid.clear_row(r as u16);
                    self.mark_dirty(r as u16);
                }
            }
            1 => {
                // 画面頭からカーソルまでクリア（Grid::clear_row で範囲チェック済み）
                for r in 0..self.cursor_row as usize {
                    self.grid.clear_row(r as u16);
                    self.mark_dirty(r as u16);
                }
                self.erase_in_line(1);
            }
            2 | 3 => {
                // 画面全体をクリア（Grid::clear_row で範囲チェック済み）
                for r in 0..height {
                    self.grid.clear_row(r as u16);
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
    ///
    /// 上限超過時はバッファクリア + DCS 状態を破棄して通常パースに復帰する
    /// （CRITICAL #7: 悪意ある PTY が DCS を終端せずに送り続ける DoS 対策）。
    pub(crate) fn push_dcs_byte(&mut self, byte: u8) {
        if !self.dcs_sixel_active {
            return;
        }
        if self.dcs_buf.len() >= MAX_DCS_BUF_LEN {
            tracing::warn!(
                "DCS Sixel バッファが上限 ({} バイト) を超過。シーケンスを破棄します。",
                MAX_DCS_BUF_LEN
            );
            self.dcs_buf.clear();
            self.dcs_sixel_active = false;
            return;
        }
        self.dcs_buf.push(byte);
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

    /// Kitty APC 画像シーケンスを処理して pending_images に積む
    ///
    /// Kitty グラフィックスプロトコルの分割転送（m=1/m=0）にも対応する。
    /// data は ESC _ と ESC \ を除いた APC 内容（先頭は 'G'）。
    pub(crate) fn handle_kitty_apc(&mut self, data: &[u8]) {
        if data.first() != Some(&b'G') {
            return;
        }
        let inner = &data[1..];
        let sep = inner.iter().position(|&b| b == b';').unwrap_or(inner.len());
        let params_bytes = &inner[..sep];
        let payload = if sep < inner.len() {
            &inner[sep + 1..]
        } else {
            &[] as &[u8]
        };

        // m=1 フラグを確認する（more_data: 続きのチャンクあり）
        let more_data = params_bytes.split(|&b| b == b',').any(|p| p == b"m=1");

        if more_data {
            // 分割転送中: 最初のチャンクのパラメータを保存し、ペイロードを累積する
            if self.kitty_chunk_params.is_none() {
                self.kitty_chunk_params = Some(params_bytes.to_vec());
            }
            // 上限チェック: 累積ペイロードが MAX_KITTY_CHUNK_LEN を超えたら破棄
            if self.kitty_chunk_payload.len() + payload.len() > MAX_KITTY_CHUNK_LEN {
                tracing::warn!(
                    "Kitty 分割転送ペイロードが上限 ({} バイト) を超過。シーケンスを破棄します。",
                    MAX_KITTY_CHUNK_LEN
                );
                self.kitty_chunk_payload.clear();
                self.kitty_chunk_params = None;
                return;
            }
            self.kitty_chunk_payload.extend_from_slice(payload);
        } else {
            // 最終チャンク（または単一チャンク）: デコードして登録する
            let (decode_params, full_payload) =
                if let Some(first_params) = self.kitty_chunk_params.take() {
                    // 分割転送の最終チャンク — 蓄積分と結合する
                    self.kitty_chunk_payload.extend_from_slice(payload);
                    let combined_payload = std::mem::take(&mut self.kitty_chunk_payload);
                    (first_params, combined_payload)
                } else {
                    // 単一チャンク
                    (params_bytes.to_vec(), payload.to_vec())
                };

            // decode_kitty が期待する形式 `G<params>;<payload>` に組み立てる
            let mut full_apc = Vec::with_capacity(decode_params.len() + full_payload.len() + 2);
            full_apc.push(b'G');
            full_apc.extend_from_slice(&decode_params);
            full_apc.push(b';');
            full_apc.extend_from_slice(&full_payload);

            if let Some(img) = decode_kitty(&full_apc) {
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

    /// タイトル変更を設定する（performer から呼ばれる）
    ///
    /// OSC 0/1/2 由来の文字列は制御文字除去 + 長さ上限で
    /// サニタイズしてからセットする（CRITICAL #13）。
    pub(crate) fn set_pending_title(&mut self, title: String) {
        self.pending_title = Some(sanitize_osc_string(title, MAX_TITLE_LEN));
    }

    /// タイトル変更を取り出してクリアする
    pub fn take_pending_title(&mut self) -> Option<String> {
        self.pending_title.take()
    }

    /// デスクトップ通知を設定する（performer から呼ばれる）
    ///
    /// OSC 9 由来の文字列はサニタイズ（制御文字除去 + 長さ上限）する（CRITICAL #13）。
    pub(crate) fn set_pending_notification(&mut self, title: String, body: String) {
        self.pending_notification = Some((
            sanitize_osc_string(title, MAX_NOTIFICATION_LEN),
            sanitize_osc_string(body, MAX_NOTIFICATION_LEN),
        ));
    }

    /// デスクトップ通知を取り出してクリアする
    pub fn take_pending_notification(&mut self) -> Option<(String, String)> {
        self.pending_notification.take()
    }

    /// CWD 変更を設定する（performer が OSC 7 受信時に呼び出す）。
    ///
    /// `path` は既に `parse_osc7_cwd` で `file://` 除去 + パーセントデコード済みであることを期待する。
    /// ここで追加のサニタイズ（制御文字除去 + 長さ上限）を行う。
    pub(crate) fn set_pending_cwd(&mut self, path: String) {
        self.pending_cwd = Some(sanitize_osc_string(path, MAX_CWD_LEN));
    }

    /// CWD 変更を取り出してクリアする
    pub fn take_pending_cwd(&mut self) -> Option<String> {
        self.pending_cwd.take()
    }

    /// OSC 52 クリップボード書き込み要求をキューに追加する（Sprint 4-1）
    ///
    /// 1 度のフラッシュで複数の OSC 52 が来る可能性があるため Vec で蓄積する。
    /// 制御文字は除去し、長さは MAX_NOTIFICATION_LEN の 1024 倍（約 1 MiB）で打ち切る。
    /// 実際の上限はクライアント側の SecurityConfig.osc52_max_bytes でも再チェックされる。
    pub(crate) fn queue_clipboard_write(&mut self, text: String) {
        const MAX_CLIPBOARD_LEN: usize = MAX_NOTIFICATION_LEN * 1024; // 約 1 MiB
        let cleaned = sanitize_osc_string(text, MAX_CLIPBOARD_LEN);
        self.pending_clipboard_writes.push(cleaned);
    }

    /// OSC 52 クリップボード書き込み要求を取り出してクリアする
    pub fn take_pending_clipboard_writes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_clipboard_writes)
    }

    /// ブラケットペーストモード（DEC ?2004）の状態を返す
    pub fn bracketed_paste_mode(&self) -> bool {
        self.bracketed_paste
    }

    /// ブラケットペーストモードを設定する（performer から呼び出す）
    pub(crate) fn set_bracketed_paste(&mut self, enabled: bool) {
        self.bracketed_paste = enabled;
    }

    /// OSC 133 セマンティックゾーンマークを記録する
    pub(crate) fn add_semantic_mark(&mut self, kind: SemanticMarkKind, exit_code: Option<i32>) {
        self.semantic_marks.push(SemanticMark {
            row: self.cursor_row,
            kind,
            exit_code,
        });
    }

    /// 溜まったセマンティックマークを取り出してクリアする
    pub fn take_semantic_marks(&mut self) -> Vec<SemanticMark> {
        std::mem::take(&mut self.semantic_marks)
    }

    /// OSC 8 ハイパーリンクの開始（url が Some）または終了（None）を処理する
    ///
    /// 許可されていない URI スキーム（`javascript:` / `file:` 等）は無効化する
    /// （CRITICAL #13: 悪意ある SSH 接続先によるクリックジャッキング対策）。
    pub(crate) fn set_hyperlink(&mut self, url: Option<String>) {
        if let Some(active_url) = self.current_hyperlink_url.take() {
            // 既存リンクを確定させて grid.hyperlinks に追加する
            let col_end = self.cursor_col;
            let row = self.hyperlink_start_row;
            if self.hyperlink_start_row == row && col_end > self.hyperlink_start_col {
                use nexterm_proto::HyperlinkSpan;
                self.grid.hyperlinks.push(HyperlinkSpan {
                    row,
                    col_start: self.hyperlink_start_col,
                    col_end,
                    url: active_url,
                });
            }
        }
        if let Some(url) = url {
            // URI スキームと長さを検証 — 不正なら無効化（None として扱う）
            let validated = validate_hyperlink_uri(&url);
            if validated.is_none() {
                self.current_hyperlink_url = None;
                return;
            }
            // 新しいリンクの開始位置を記録する
            self.current_hyperlink_url = validated;
            self.hyperlink_start_col = self.cursor_col;
            self.hyperlink_start_row = self.cursor_row;
        }
    }
}

#[cfg(test)]
mod osc_security_tests {
    use super::*;

    #[test]
    fn 制御文字は_osc_文字列から除去される() {
        let input = "Title\x00\x01\x07\x1bWith Control".to_string();
        let cleaned = sanitize_osc_string(input, MAX_TITLE_LEN);
        assert!(!cleaned.contains('\x00'));
        assert!(!cleaned.contains('\x01'));
        assert!(!cleaned.contains('\x07'));
        assert!(!cleaned.contains('\x1b'));
        assert_eq!(cleaned, "TitleWith Control");
    }

    #[test]
    fn osc_文字列は_max_len_で切り詰められる() {
        let input = "a".repeat(1000);
        let cleaned = sanitize_osc_string(input, 100);
        assert_eq!(cleaned.len(), 100);
    }

    #[test]
    fn cjk_文字でも_utf8_境界で切り詰められる() {
        let input = "あいうえお".repeat(100);
        let cleaned = sanitize_osc_string(input, 10);
        assert!(cleaned.len() <= 10);
        // UTF-8 として有効
        assert!(cleaned.chars().count() > 0);
    }

    #[test]
    fn osc_文字列は_tab_lf_cr_を保持する() {
        let input = "Hello\tWorld\nNext\rLine".to_string();
        let cleaned = sanitize_osc_string(input, 100);
        assert_eq!(cleaned, "Hello\tWorld\nNext\rLine");
    }

    #[test]
    fn http_https_uri_は許可される() {
        assert!(validate_hyperlink_uri("https://example.com").is_some());
        assert!(validate_hyperlink_uri("http://example.com/path").is_some());
        assert!(validate_hyperlink_uri("HTTPS://EXAMPLE.COM").is_some());
    }

    #[test]
    fn javascript_uri_は拒否される() {
        // CRITICAL #13: クリックジャッキング対策の核心テスト
        assert!(validate_hyperlink_uri("javascript:alert(1)").is_none());
        assert!(validate_hyperlink_uri("JavaScript:void(0)").is_none());
    }

    #[test]
    fn file_uri_は拒否される() {
        assert!(validate_hyperlink_uri("file:///etc/passwd").is_none());
        assert!(validate_hyperlink_uri("FILE:///c:/windows/system32").is_none());
    }

    #[test]
    fn data_uri_は拒否される() {
        assert!(validate_hyperlink_uri("data:text/html,<script>alert(1)</script>").is_none());
    }

    #[test]
    fn 長すぎる_uri_は拒否される() {
        let long = "https://".to_string() + &"a".repeat(MAX_HYPERLINK_URI_LEN);
        assert!(validate_hyperlink_uri(&long).is_none());
    }

    #[test]
    fn 制御文字を含む_uri_は除去される() {
        // タブ・改行を含む URI も除去対象（厳密化）
        let result = validate_hyperlink_uri("https://example.com/\x00path").unwrap();
        assert!(!result.contains('\x00'));
    }

    #[test]
    fn mailto_ssh_ftp_uri_は許可される() {
        assert!(validate_hyperlink_uri("mailto:user@example.com").is_some());
        assert!(validate_hyperlink_uri("ssh://server.example.com").is_some());
        assert!(validate_hyperlink_uri("ftp://files.example.com").is_some());
    }

    #[test]
    fn 空文字列_uri_は拒否される() {
        assert!(validate_hyperlink_uri("").is_none());
    }

    // ---- OSC 7 (CWD reporting) のテスト群（Sprint 5-2 / B2） ----

    #[test]
    fn osc7_file_uri_スキーム付きパスを抽出する() {
        assert_eq!(
            parse_osc7_cwd("file:///home/user/projects"),
            Some("/home/user/projects".to_string())
        );
    }

    #[test]
    fn osc7_host_部分を無視する() {
        // file://hostname/path 形式（host 部は無視して path のみ採用）
        assert_eq!(
            parse_osc7_cwd("file://example.host/home/user"),
            Some("/home/user".to_string())
        );
    }

    #[test]
    fn osc7_スキームなしも素通しする() {
        assert_eq!(
            parse_osc7_cwd("/home/user/proj"),
            Some("/home/user/proj".to_string())
        );
    }

    #[test]
    fn osc7_パーセントエンコードをデコードする() {
        // " " (0x20) は %20
        assert_eq!(
            parse_osc7_cwd("file:///home/user/dir%20with%20space"),
            Some("/home/user/dir with space".to_string())
        );
        // 日本語パス（UTF-8: あ = E3 81 82）
        assert_eq!(parse_osc7_cwd("file:///%E3%81%82"), Some("/あ".to_string()));
    }

    #[test]
    fn osc7_不正なパーセント_xx_は素通しする() {
        // `%ZZ` は16進ではないので変換せずに元の文字列を残す
        assert_eq!(
            parse_osc7_cwd("file:///path/%ZZ/foo"),
            Some("/path/%ZZ/foo".to_string())
        );
    }

    #[test]
    fn osc7_空入力は_none() {
        assert!(parse_osc7_cwd("").is_none());
    }

    #[test]
    fn osc7_file_スキームだけでパスなしは_none() {
        assert!(parse_osc7_cwd("file://hostname").is_none());
    }

    #[test]
    fn osc7_制御文字は除去される() {
        let result = parse_osc7_cwd("file:///home/\x00user\x07/dir").unwrap();
        assert!(!result.contains('\x00'));
        assert!(!result.contains('\x07'));
        assert_eq!(result, "/home/user/dir");
    }

    #[test]
    fn osc7_長い入力は早期拒否される() {
        // 入力長が `MAX_CWD_LEN * 4` を超えると DoS 防止で None
        let huge_path = format!("file:///{}", "a".repeat(MAX_CWD_LEN * 5));
        assert!(parse_osc7_cwd(&huge_path).is_none());
    }

    #[test]
    fn osc7_max_cwd_len_で結果が切り詰められる() {
        // 入力が早期拒否されない範囲内で、結果が MAX_CWD_LEN 以下に収まることを検証
        let near_limit = format!("file:///{}", "a".repeat(MAX_CWD_LEN + 100));
        let result = parse_osc7_cwd(&near_limit).expect("早期拒否範囲内では Some を返すこと");
        assert!(
            result.len() <= MAX_CWD_LEN,
            "結果は MAX_CWD_LEN 以下に切り詰められること。実際: {}",
            result.len()
        );
    }

    #[test]
    fn osc7_screen_の_pending_cwd_で取り出せる() {
        let mut screen = Screen::new(80, 24);
        screen.set_pending_cwd("/home/user/test".to_string());
        assert_eq!(
            screen.take_pending_cwd(),
            Some("/home/user/test".to_string())
        );
        // take 後はクリアされる
        assert!(screen.take_pending_cwd().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn osc7_windows_パスの先頭スラッシュを除去する() {
        // file:///C:/Users/foo → C:/Users/foo
        assert_eq!(
            parse_osc7_cwd("file:///C:/Users/foo"),
            Some("C:/Users/foo".to_string())
        );
    }
}
