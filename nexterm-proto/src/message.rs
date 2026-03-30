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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// セッション録音を開始する
    StartRecording { session_name: String, output_path: String },
    /// セッション録音を停止する
    StopRecording { session_name: String },
    /// フォーカスペインを閉じる
    ClosePane,
    /// フォーカスペインの分割比率を変更する（正: 広げる、負: 縮める）
    ResizeSplit { delta: f32 },
    /// SSH 接続（設定済みホスト名を指定）
    ConnectSsh {
        host: String,
        port: u16,
        username: String,
        /// 認証方式: "password", "key", "agent"
        auth_type: String,
        /// パスワード認証時のパスワード（平文、将来はキーチェーンから取得）
        password: Option<String>,
        /// 公開鍵認証時の秘密鍵パス
        key_path: Option<String>,
        /// リモートポートフォワーディング指定（"remote_port:local_host:local_port" 形式、複数可）
        #[serde(default)]
        remote_forwards: Vec<String>,
        /// X11 フォワーディングを有効にするか（ssh -X 相当）
        #[serde(default)]
        x11_forward: bool,
        /// 信頼された X11 フォワーディング（ssh -Y 相当）
        #[serde(default)]
        x11_trusted: bool,
    },
    /// 新しいウィンドウを作成する
    NewWindow,
    /// 指定ウィンドウを閉じる（最後のウィンドウは閉じられない）
    CloseWindow { window_id: u32 },
    /// 指定ウィンドウにフォーカスを移動する
    FocusWindow { window_id: u32 },
    /// 指定ウィンドウをリネームする
    RenameWindow { window_id: u32, name: String },
    /// ブロードキャストモードを設定する（true: 全ペインに入力、false: フォーカスペインのみ）
    SetBroadcast { enabled: bool },
    /// ペイン番号オーバーレイを表示/非表示
    DisplayPanes { show: bool },
    /// asciicast v2 形式での録画を開始する
    StartAsciicast { session_name: String, output_path: String },
    /// asciicast v2 形式での録画を停止する
    StopAsciicast { session_name: String },
    /// レイアウトテンプレートを保存する
    SaveTemplate { name: String },
    /// レイアウトテンプレートを読み込んで適用する
    LoadTemplate { name: String },
    /// 保存済みテンプレートの一覧を取得する
    ListTemplates,
    /// フォーカスペインをウィンドウ全体にズーム（トグル）
    ToggleZoom,
    /// フォーカスペインと指定ペインを入れ替える（BSP ツリー内の ID swap）
    SwapPane { target_pane_id: u32 },
    /// フォーカスペインを新しいウィンドウとして切り離す
    BreakPane,
    /// フォーカスペインを指定ウィンドウに移動する
    JoinPane { target_window_id: u32 },
    /// SFTP アップロード: ローカルファイルをリモートに転送する
    SftpUpload {
        /// 接続先 SSH ホスト設定名（config.hosts のエントリ名）
        host_name: String,
        /// ローカルファイルパス
        local_path: String,
        /// リモート保存先パス
        remote_path: String,
    },
    /// SFTP ダウンロード: リモートファイルをローカルに転送する
    SftpDownload {
        /// 接続先 SSH ホスト設定名（config.hosts のエントリ名）
        host_name: String,
        /// リモートファイルパス
        remote_path: String,
        /// ローカル保存先パス
        local_path: String,
    },
    /// Lua マクロを実行して結果をフォーカスペインに送信する
    RunMacro {
        /// nexterm.lua 内の Lua 関数名
        macro_fn: String,
        /// コマンドパレット / UI に表示する表示名（ログ用）
        #[serde(default)]
        display_name: String,
    },
    /// シリアルポートに接続する
    ConnectSerial {
        /// デバイスパス（例: "/dev/ttyUSB0", "COM3"）
        port: String,
        /// ボーレート（例: 115200）
        baud_rate: u32,
        /// データビット: 5, 6, 7, 8
        #[serde(default = "default_data_bits")]
        data_bits: u8,
        /// ストップビット: 1, 2
        #[serde(default = "default_stop_bits")]
        stop_bits: u8,
        /// パリティ: "none", "odd", "even"
        #[serde(default = "default_parity")]
        parity: String,
    },
}

fn default_data_bits() -> u8 { 8 }
fn default_stop_bits() -> u8 { 1 }
fn default_parity() -> String { "none".to_string() }

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
    /// BEL 通知（\x07 を受信したペインから発行）
    Bell { pane_id: u32 },
    /// セッション録音開始通知
    RecordingStarted { pane_id: u32, path: String },
    /// セッション録音停止通知
    RecordingStopped { pane_id: u32 },
    /// ウィンドウ一覧変更通知
    WindowListChanged { windows: Vec<WindowInfo> },
    /// ペインが閉じられた通知（ウィンドウも一緒に閉じられた場合は pane_id を 0 にする）
    PaneClosed { pane_id: u32 },
    /// ウィンドウ/ペインタイトル変更通知
    TitleChanged { pane_id: u32, title: String },
    /// デスクトップ通知
    DesktopNotification { pane_id: u32, title: String, body: String },
    /// ブロードキャストモード状態通知
    BroadcastModeChanged { enabled: bool },
    /// asciicast v2 録画開始通知
    AsciicastStarted { pane_id: u32, path: String },
    /// asciicast v2 録画停止通知
    AsciicastStopped { pane_id: u32 },
    /// テンプレート保存完了通知
    TemplateSaved { name: String, path: String },
    /// テンプレート読み込み完了通知
    TemplateLoaded { name: String },
    /// テンプレート一覧
    TemplateList { names: Vec<String> },
    /// ペインズーム状態変化通知
    ZoomChanged { is_zoomed: bool },
    /// BreakPane 完了通知（新ウィンドウの ID）
    PaneBroken { new_window_id: u32, pane_id: u32 },
    /// シリアル接続成功通知
    SerialConnected { pane_id: u32, port: String },
    /// SFTP 転送進捗通知
    SftpProgress {
        /// 転送元ローカルパスまたはリモートパス（UI 表示用）
        path: String,
        /// 転送済みバイト数
        transferred: u64,
        /// 合計バイト数（0 = 不明）
        total: u64,
    },
    /// SFTP 転送完了通知
    SftpDone {
        /// 転送元/先パス（UI 表示用）
        path: String,
        /// 成功時は None, エラー時はメッセージ
        error: Option<String>,
    },
}

/// セッション情報
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub window_count: u32,
    pub attached: bool,
}

/// ウィンドウ情報
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub window_id: u32,
    pub name: String,
    pub pane_count: u32,
    pub is_focused: bool,
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
