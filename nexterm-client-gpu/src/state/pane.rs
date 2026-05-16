//! ペインの描画状態 — `PaneState`、配置済み画像、フローティングペイン位置情報
//!
//! `state/mod.rs` から抽出した:
//! - `FloatRect` — フローティングペインの位置・サイズ
//! - `PlacedImage` — Sixel / Kitty で配置された画像のメタデータ + RGBA
//! - `PaneState` — グリッド / カーソル / スクロールバック / 画像 / プロンプトアンカーを束ねるペイン単位の状態

use std::collections::HashMap;

use nexterm_proto::Grid;

use crate::scrollback::Scrollback;

/// フローティングペインの位置・サイズ情報
#[derive(Clone, Debug)]
pub struct FloatRect {
    #[allow(dead_code)]
    pub col_off: u16,
    #[allow(dead_code)]
    pub row_off: u16,
    #[allow(dead_code)]
    pub cols: u16,
    #[allow(dead_code)]
    pub rows: u16,
}

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
    /// OSC 0/2 で設定されたタイトル（シェルや vim がウィンドウタイトルを設定する）
    pub title: String,
    /// OSC 7 で報告された現在の作業ディレクトリ（Sprint 5-2 / B2）
    ///
    /// シェルが `printf '\\033]7;file://...' "$PWD"` 等を出力したときに更新される。
    /// タブのツールチップ表示・新規ペイン作成時の親 CWD 継承に利用する。
    /// 一度も OSC 7 が来ていない場合は `None`。
    pub cwd: Option<String>,
    /// OSC 133 A (PromptStart) マークが届いた時点の scrollback 長を保持する（Sprint 5-2 / B1）
    ///
    /// `scroll_offset` と同じ「scrollback 内の行インデックス」空間で表現する。
    /// `jump_prev_prompt` / `jump_next_prompt` でこのリストを辿って前後のプロンプトへジャンプする。
    /// 概算であり、画面再描画やリサイズで多少ズレる可能性がある。
    pub prompt_anchors: Vec<usize>,
}

impl PaneState {
    pub(super) fn new(cols: u16, rows: u16, scrollback_capacity: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            cursor_col: 0,
            cursor_row: 0,
            scrollback: Scrollback::new(scrollback_capacity),
            scroll_offset: 0,
            images: HashMap::new(),
            has_activity: false,
            title: String::new(),
            cwd: None,
            prompt_anchors: Vec::new(),
        }
    }

    pub(super) fn apply_diff(
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
