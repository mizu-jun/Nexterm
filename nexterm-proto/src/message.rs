//! IPC メッセージ型定義
//!
//! クライアント → サーバー、サーバー → クライアントの両方向メッセージを定義する。

use serde::{Deserialize, Serialize};

use crate::{DirtyRow, Grid};

/// キー修飾キーのビットフラグ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Modifiers(pub u8);

impl Modifiers {
    pub const SHIFT: u8 = 0b0001;
    pub const CTRL: u8 = 0b0010;
    pub const ALT: u8 = 0b0100;
    pub const META: u8 = 0b1000;

    pub fn is_ctrl(self) -> bool {
        self.0 & Self::CTRL != 0
    }
    pub fn is_shift(self) -> bool {
        self.0 & Self::SHIFT != 0
    }
}

/// キーイベント
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyCode {
    /// 通常文字
    Char(char),
    /// ファンクションキー F1〜F12
    F(u8),
    Enter,
    Backspace,
    Delete,
    Escape,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
}

/// クライアント → サーバー メッセージ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientToServer {
    /// キー入力イベント
    KeyEvent {
        code: KeyCode,
        modifiers: Modifiers,
    },
    /// 端末サイズ変更
    Resize {
        cols: u16,
        rows: u16,
    },
    /// セッションからデタッチ（クライアント終了）
    Detach,
    /// セッション名を指定してアタッチ
    Attach {
        session_name: String,
    },
    /// ペイン作成（垂直分割）
    SplitVertical,
    /// ペイン作成（水平分割）
    SplitHorizontal,
    /// 次のペインにフォーカス移動
    FocusNextPane,
    /// 前のペインにフォーカス移動
    FocusPrevPane,
    /// 指定 ID のペインにフォーカスを移動する（クリック操作など）
    FocusPane { pane_id: u32 },
    /// テキストをフォーカスペインにペーストする
    PasteText { text: String },
    /// 接続確認
    Ping,
    /// セッション一覧を取得する（アタッチなし）
    ListSessions,
    /// セッションを強制終了する
    KillSession { name: String },
}

/// ペインのレイアウト情報（グリッド座標系）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneLayout {
    pub pane_id: u32,
    /// ウィンドウ内の列オフセット（0 始まり）
    pub col_offset: u16,
    /// ウィンドウ内の行オフセット（0 始まり）
    pub row_offset: u16,
    pub cols: u16,
    pub rows: u16,
    pub is_focused: bool,
}

/// サーバー → クライアント メッセージ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerToClient {
    /// 差分グリッド更新（通常の描画更新）
    GridDiff {
        /// 対象ペイン ID
        pane_id: u32,
        /// ダーティ行のみ
        dirty_rows: Vec<DirtyRow>,
        /// カーソル位置
        cursor_col: u16,
        cursor_row: u16,
    },
    /// 全画面スナップショット（アタッチ時・再接続時）
    FullRefresh {
        pane_id: u32,
        grid: Grid,
    },
    /// セッション一覧
    SessionList {
        sessions: Vec<SessionInfo>,
    },
    /// Ping の応答
    Pong,
    /// エラー通知
    Error {
        message: String,
    },
    /// 画像配置通知（Sixel / Kitty プロトコル）
    ImagePlaced {
        pane_id: u32,
        image_id: u32,
        /// グリッド上の配置列（0始まり）
        col: u16,
        /// グリッド上の配置行（0始まり）
        row: u16,
        /// 画像ピクセル幅
        width: u32,
        /// 画像ピクセル高さ
        height: u32,
        /// RGBA ピクセルデータ
        rgba: Vec<u8>,
    },
    /// レイアウト変更通知（分割・フォーカス変更・リサイズ時）
    LayoutChanged {
        panes: Vec<PaneLayout>,
        focused_pane_id: u32,
    },
}

/// セッション情報
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub window_count: u32,
    pub attached: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cell, Grid};

    #[test]
    fn キーイベントのbincode往復() {
        let msg = ClientToServer::KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: Modifiers(Modifiers::CTRL),
        };
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ClientToServer = bincode::deserialize(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn full_refreshのbincode往復() {
        let grid = Grid::new(80, 24);
        let msg = ServerToClient::FullRefresh { pane_id: 1, grid };
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ServerToClient = bincode::deserialize(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn grid_diffのbincode往復() {
        let msg = ServerToClient::GridDiff {
            pane_id: 0,
            dirty_rows: vec![DirtyRow {
                row: 3,
                cells: vec![Cell::default(); 80],
            }],
            cursor_col: 5,
            cursor_row: 3,
        };
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ServerToClient = bincode::deserialize(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn modifiers_ビットフラグ() {
        let m = Modifiers(Modifiers::CTRL | Modifiers::SHIFT);
        assert!(m.is_ctrl());
        assert!(m.is_shift());
    }
}
