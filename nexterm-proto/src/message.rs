//! IPC メッセージ型定義
//!
//! クライアント → サーバー、サーバー → クライアントの両方向メッセージを定義する。

use serde::{Deserialize, Serialize};

use crate::{DirtyRow, Grid};

/// キー修飾キーのビットフラグ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Modifiers(pub u8);

impl Modifiers {
    /// Shift キーのビットマスク
    pub const SHIFT: u8 = 0b0001;
    /// Ctrl キーのビットマスク
    pub const CTRL: u8 = 0b0010;
    /// Alt / Option キーのビットマスク
    pub const ALT: u8 = 0b0100;
    /// Meta / Super / Windows キーのビットマスク
    pub const META: u8 = 0b1000;

    /// Ctrl キーが押されているか確認する
    pub fn is_ctrl(self) -> bool {
        self.0 & Self::CTRL != 0
    }
    /// Shift キーが押されているか確認する
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
    /// Enter / Return キー
    Enter,
    /// Backspace キー
    Backspace,
    /// Delete キー
    Delete,
    /// Escape キー
    Escape,
    /// Tab キー
    Tab,
    /// Shift+Tab（逆方向タブ）
    BackTab,
    /// 上矢印キー
    Up,
    /// 下矢印キー
    Down,
    /// 左矢印キー
    Left,
    /// 右矢印キー
    Right,
    /// Home キー
    Home,
    /// End キー
    End,
    /// Page Up キー
    PageUp,
    /// Page Down キー
    PageDown,
    /// Insert キー
    Insert,
}

/// クライアント → サーバー メッセージ
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientToServer {
    /// キー入力イベント
    KeyEvent {
        /// 押されたキー
        code: KeyCode,
        /// 同時に押された修飾キー
        modifiers: Modifiers,
    },
    /// 端末サイズ変更
    Resize {
        /// 新しい列数
        cols: u16,
        /// 新しい行数
        rows: u16,
    },
    /// セッションからデタッチ（クライアント終了）
    Detach,
    /// セッション名を指定してアタッチ
    Attach {
        /// アタッチ先セッション名
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
    FocusPane {
        /// フォーカス先のペイン ID
        pane_id: u32,
    },
    /// テキストをフォーカスペインにペーストする
    PasteText {
        /// ペーストするテキスト
        text: String,
    },
    /// 接続確認
    Ping,
    /// セッション一覧を取得する（アタッチなし）
    ListSessions,
    /// セッションを強制終了する
    KillSession {
        /// 終了するセッション名
        name: String,
    },
    /// セッション録音を開始する
    StartRecording {
        /// 録音対象セッション名
        session_name: String,
        /// 録音ファイルの出力パス
        output_path: String,
    },
    /// セッション録音を停止する
    StopRecording {
        /// 停止するセッション名
        session_name: String,
    },
    /// フォーカスペインを閉じる
    ClosePane,
    /// フォーカスペインの分割比率を変更する（正: 広げる、負: 縮める）
    ResizeSplit {
        /// サイズ変更量（0.0〜1.0 の割合、正=拡大、負=縮小）
        delta: f32,
    },
    /// SSH 接続（設定済みホスト名を指定）
    ConnectSsh {
        /// 接続先ホスト名または IP アドレス
        host: String,
        /// SSH ポート番号（通常 22）
        port: u16,
        /// ログインユーザー名
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
    CloseWindow {
        /// 閉じるウィンドウの ID
        window_id: u32,
    },
    /// 指定ウィンドウにフォーカスを移動する
    FocusWindow {
        /// フォーカスするウィンドウ ID
        window_id: u32,
    },
    /// 指定ウィンドウをリネームする
    RenameWindow {
        /// リネームするウィンドウ ID
        window_id: u32,
        /// 新しいウィンドウ名
        name: String,
    },
    /// ブロードキャストモードを設定する（true: 全ペインに入力、false: フォーカスペインのみ）
    SetBroadcast {
        /// true = 全ペインに入力、false = フォーカスペインのみ
        enabled: bool,
    },
    /// ペイン番号オーバーレイを表示/非表示
    DisplayPanes {
        /// true = オーバーレイを表示
        show: bool,
    },
    /// asciicast v2 形式での録画を開始する
    StartAsciicast {
        /// 録画対象セッション名
        session_name: String,
        /// asciicast ファイルの出力パス
        output_path: String,
    },
    /// asciicast v2 形式での録画を停止する
    StopAsciicast {
        /// 停止するセッション名
        session_name: String,
    },
    /// レイアウトテンプレートを保存する
    SaveTemplate {
        /// 保存するテンプレート名
        name: String,
    },
    /// レイアウトテンプレートを読み込んで適用する
    LoadTemplate {
        /// 読み込むテンプレート名
        name: String,
    },
    /// 保存済みテンプレートの一覧を取得する
    ListTemplates,
    /// フォーカスペインをウィンドウ全体にズーム（トグル）
    ToggleZoom,
    /// フォーカスペインと指定ペインを入れ替える（BSP ツリー内の ID swap）
    SwapPane {
        /// 入れ替え先のペイン ID
        target_pane_id: u32,
    },
    /// フォーカスペインを新しいウィンドウとして切り離す
    BreakPane,
    /// フォーカスペインを指定ウィンドウに移動する
    JoinPane {
        /// 移動先ウィンドウ ID
        target_window_id: u32,
    },
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
    /// マウスイベントを PTY に送信する（マウスレポーティングモード時）
    MouseReport {
        /// ボタン番号（0=左, 1=中, 2=右, 64=ホイールアップ, 65=ホイールダウン）
        button: u8,
        /// グリッド列（0始まり）
        col: u16,
        /// グリッド行（0始まり）
        row: u16,
        /// 押下 = true、リリース = false
        pressed: bool,
        /// マウス移動イベント（ドラッグ）
        motion: bool,
    },
    /// レイアウトモードを設定する（"bsp" または "tiling"）
    SetLayoutMode {
        /// レイアウトモード文字列（"bsp" または "tiling"）
        mode: String,
    },
    /// フローティングペインを開く（Ctrl+B f）
    OpenFloatingPane,
    /// フローティングペインを閉じる
    CloseFloatingPane {
        /// 閉じるフローティングペイン ID
        pane_id: u32,
    },
    /// フローティングペインを移動する（マウスドラッグ）
    MoveFloatingPane {
        /// 移動するフローティングペイン ID
        pane_id: u32,
        /// 新しい列方向オフセット（0始まり）
        col_off: u16,
        /// 新しい行方向オフセット（0始まり）
        row_off: u16,
    },
    /// フローティングペインをリサイズする
    ResizeFloatingPane {
        /// リサイズするフローティングペイン ID
        pane_id: u32,
        /// 新しい列数
        cols: u16,
        /// 新しい行数
        rows: u16,
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
    /// ロード済みプラグイン一覧を取得する
    ListPlugins,
    /// WASM プラグインをロードする
    LoadPlugin {
        /// WASM ファイルのパス
        path: String,
    },
    /// ロード済みプラグインをアンロードする
    UnloadPlugin {
        /// アンロードするプラグインのパス
        path: String,
    },
    /// プラグインを再ロードする（ファイルが更新された場合）
    ReloadPlugin {
        /// 再ロードするプラグインのパス
        path: String,
    },
    /// プロトコルハンドシェイク。接続後の最初のメッセージとして送信する。
    ///
    /// サーバーは `proto_version` を `nexterm_proto::PROTOCOL_VERSION` と比較し、
    /// 不一致ならエラーを返して接続を切断する。
    Hello {
        /// `nexterm_proto::PROTOCOL_VERSION`
        proto_version: u32,
        /// クライアント種別
        client_kind: ClientKind,
        /// クライアントの Cargo バージョン文字列（ログ用）
        client_version: String,
    },
}

/// クライアント種別（IPC ハンドシェイクで識別）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientKind {
    /// GPU クライアント (winit + wgpu)
    Gpu,
    /// TUI クライアント (ratatui + crossterm)
    Tui,
    /// CLI ツール (nexterm-ctl)
    Ctl,
    /// その他（プラグイン等）
    Other,
}

fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_parity() -> String {
    "none".to_string()
}

/// ペインのレイアウト情報（グリッド座標系）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneLayout {
    /// ペインの一意 ID
    pub pane_id: u32,
    /// ウィンドウ内の列オフセット（0 始まり）
    pub col_offset: u16,
    /// ウィンドウ内の行オフセット（0 始まり）
    pub row_offset: u16,
    /// ペインの列数（文字単位）
    pub cols: u16,
    /// ペインの行数（文字単位）
    pub rows: u16,
    /// このペインがフォーカスを持っているか
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
        /// カーソルの列位置（0始まり）
        cursor_col: u16,
        /// カーソルの行位置（0始まり）
        cursor_row: u16,
    },
    /// 全画面スナップショット（アタッチ時・再接続時）
    FullRefresh {
        /// 対象ペイン ID
        pane_id: u32,
        /// スナップショットグリッド
        grid: Grid,
    },
    /// セッション一覧
    SessionList {
        /// セッション情報の一覧
        sessions: Vec<SessionInfo>,
    },
    /// Ping の応答
    Pong,
    /// エラー通知
    Error {
        /// エラーメッセージ
        message: String,
    },
    /// 画像配置通知（Sixel / Kitty プロトコル）
    ImagePlaced {
        /// 対象ペイン ID
        pane_id: u32,
        /// 画像の一意 ID（フレーム管理用）
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
        /// 全ペインのレイアウト一覧
        panes: Vec<PaneLayout>,
        /// 現在フォーカスを持つペイン ID
        focused_pane_id: u32,
    },
    /// BEL 通知（\x07 を受信したペインから発行）
    Bell {
        /// BEL を受信したペイン ID
        pane_id: u32,
    },
    /// セッション録音開始通知
    RecordingStarted {
        /// 録音対象ペイン ID
        pane_id: u32,
        /// 録音ファイルパス
        path: String,
    },
    /// セッション録音停止通知
    RecordingStopped {
        /// 録音対象ペイン ID
        pane_id: u32,
    },
    /// ウィンドウ一覧変更通知
    WindowListChanged {
        /// 最新のウィンドウ情報一覧
        windows: Vec<WindowInfo>,
    },
    /// ペインが閉じられた通知（ウィンドウも一緒に閉じられた場合は pane_id を 0 にする）
    PaneClosed {
        /// 閉じられたペイン ID（0 = ウィンドウごと閉じた）
        pane_id: u32,
    },
    /// ウィンドウ/ペインタイトル変更通知
    TitleChanged {
        /// タイトルが変わったペイン ID
        pane_id: u32,
        /// 新しいタイトル文字列
        title: String,
    },
    /// デスクトップ通知
    DesktopNotification {
        /// 通知元ペイン ID
        pane_id: u32,
        /// 通知タイトル
        title: String,
        /// 通知本文
        body: String,
    },
    /// OSC 52 クリップボード書き込み要求（Sprint 4-1）
    ///
    /// クライアントは `SecurityConfig.osc52_clipboard` ポリシーに従って
    /// 同意ダイアログを表示するか、即時許可/拒否するかを判断する。
    ClipboardWriteRequest {
        /// 要求元ペイン ID
        pane_id: u32,
        /// 書き込み内容（サーバー側で制御文字除去済み）
        text: String,
    },
    /// ブロードキャストモード状態通知
    BroadcastModeChanged {
        /// true = 全ペインに入力、false = フォーカスペインのみ
        enabled: bool,
    },
    /// asciicast v2 録画開始通知
    AsciicastStarted {
        /// 録音対象ペイン ID
        pane_id: u32,
        /// asciicast ファイルパス
        path: String,
    },
    /// asciicast v2 録画停止通知
    AsciicastStopped {
        /// 録音対象ペイン ID
        pane_id: u32,
    },
    /// テンプレート保存完了通知
    TemplateSaved {
        /// テンプレート名
        name: String,
        /// 保存ファイルパス
        path: String,
    },
    /// テンプレート読み込み完了通知
    TemplateLoaded {
        /// 読み込んだテンプレート名
        name: String,
    },
    /// テンプレート一覧
    TemplateList {
        /// 保存済みテンプレート名の一覧
        names: Vec<String>,
    },
    /// ペインズーム状態変化通知
    ZoomChanged {
        /// true = ズーム中、false = 通常表示
        is_zoomed: bool,
    },
    /// BreakPane 完了通知（新ウィンドウの ID）
    PaneBroken {
        /// 新しく作成されたウィンドウの ID
        new_window_id: u32,
        /// 分離されたペイン ID
        pane_id: u32,
    },
    /// シリアル接続成功通知
    SerialConnected {
        /// 割り当てられたペイン ID
        pane_id: u32,
        /// 接続したポート名（例: "/dev/ttyUSB0"）
        port: String,
    },
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
    /// OSC 133 セマンティックゾーンマーク通知
    SemanticMark {
        /// マークが付いたペイン ID
        pane_id: u32,
        /// マーク行（0始まり）
        row: u16,
        /// "A"=PromptStart, "B"=CommandStart, "C"=OutputStart, "D"=CommandEnd
        kind: String,
        /// D マーク時のみ Some
        exit_code: Option<i32>,
    },
    /// フローティングペイン開始通知
    FloatingPaneOpened {
        /// 開かれたフローティングペイン ID
        pane_id: u32,
        /// 列方向オフセット（0始まり）
        col_off: u16,
        /// 行方向オフセット（0始まり）
        row_off: u16,
        /// ペインの列数
        cols: u16,
        /// ペインの行数
        rows: u16,
    },
    /// フローティングペイン位置・サイズ変更通知
    FloatingPaneMoved {
        /// 移動されたフローティングペイン ID
        pane_id: u32,
        /// 列方向オフセット（0始まり）
        col_off: u16,
        /// 行方向オフセット（0始まり）
        row_off: u16,
        /// ペインの列数
        cols: u16,
        /// ペインの行数
        rows: u16,
    },
    /// フローティングペイン閉鎖通知
    FloatingPaneClosed {
        /// 閉じられたフローティングペイン ID
        pane_id: u32,
    },
    /// ロード済みプラグイン一覧
    PluginList {
        /// プラグインパスの一覧
        paths: Vec<String>,
    },
    /// プラグイン操作完了通知
    PluginOk {
        /// 操作対象のプラグインパス
        path: String,
        /// 操作種別: "loaded", "unloaded", "reloaded"
        action: String,
    },
    /// プロトコルハンドシェイク応答（サーバー → クライアント）。
    ///
    /// クライアントが Hello を送ってきた直後に、サーバーが自身のバージョン情報を返す。
    /// バージョン不一致でサーバーが接続を切断する場合は本メッセージは送信されない
    /// （`Error` バリアント + 切断のみ）。
    HelloAck {
        /// サーバー側がサポートする最低プロトコルバージョン
        proto_version: u32,
        /// サーバーの Cargo バージョン文字列
        server_version: String,
    },
}

/// セッション情報
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// セッション名
    pub name: String,
    /// ウィンドウ数
    pub window_count: u32,
    /// クライアントがアタッチ中かどうか
    pub attached: bool,
}

/// ウィンドウ情報
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// ウィンドウの一意 ID
    pub window_id: u32,
    /// ウィンドウ名
    pub name: String,
    /// ウィンドウ内のペイン数
    pub pane_count: u32,
    /// このウィンドウがフォーカスを持っているか
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

    #[test]
    fn hello_メッセージの_bincode_往復() {
        let msg = ClientToServer::Hello {
            proto_version: 1,
            client_kind: ClientKind::Gpu,
            client_version: "1.0.2".to_string(),
        };
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ClientToServer = bincode::deserialize(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn hello_ack_メッセージの_bincode_往復() {
        let msg = ServerToClient::HelloAck {
            proto_version: 1,
            server_version: "1.0.2".to_string(),
        };
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ServerToClient = bincode::deserialize(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn client_kind_の全バリアントが_bincode_往復可能() {
        for kind in [
            ClientKind::Gpu,
            ClientKind::Tui,
            ClientKind::Ctl,
            ClientKind::Other,
        ] {
            let encoded = bincode::serialize(&kind).unwrap();
            let decoded: ClientKind = bincode::deserialize(&encoded).unwrap();
            assert_eq!(kind, decoded);
        }
    }
}
