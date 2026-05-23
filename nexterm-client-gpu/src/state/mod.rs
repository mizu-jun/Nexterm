//! クライアント状態 — グリッド・スクロールバック・パレット・検索を統合管理する
//!
//! Sprint 5-6 でファイル分割した構成:
//! - `pane` — `PaneState` / `PlacedImage` / `FloatRect`
//! - `search` — `SearchState` と `ClientState` のインクリメンタル検索メソッド
//! - `selection` — `DetectedUrl` / `MouseSelection` / `CopyModeState`
//! - `menus` — `ContextMenu*` / `FileTransferDialog` / `QuickSelect*`
//! - `consent` — `ConsentDialog` / `ConsentKind` / `SessionConsentOverrides`
//! - `server_message` — `apply_server_message` と scroll / jump-to-prompt メソッド + tests
//!
//! 旧 `state.rs` で公開していた型はすべて本モジュールから `pub use` で再エクスポートする
//! ため、`crate::state::Foo` 形式の参照は変更不要。

use std::collections::HashMap;

use nexterm_proto::PaneLayout;
use winit::window::WindowId;

use crate::host_manager::HostManager;
use crate::macro_picker::MacroPicker;
use crate::palette::CommandPalette;
use crate::settings_panel::SettingsPanel;

mod consent;
mod menus;
mod pane;
mod search;
mod selection;
mod server_message;

pub use consent::{ConsentDialog, ConsentKind, SessionConsentOverrides};
// `ContextMenuItem` / `QuickSelectMatch` / `DetectedUrl` は現状クレート内から直接参照
// されていないが、`ContextMenu` / `QuickSelectState` / `detect_urls_in_row` の戻り値型
// として公開 API の一部となっているため再エクスポートを維持する。
#[allow(unused_imports)]
pub use menus::{
    ContextMenu, ContextMenuAction, ContextMenuItem, FileTransferDialog, QuickSelectMatch,
    QuickSelectState,
};
pub use pane::{FloatRect, PaneState, PlacedImage};
pub use search::SearchState;
#[allow(unused_imports)]
pub use selection::{CopyModeState, DetectedUrl, MouseSelection, detect_urls_in_row};

/// SR 向けアラートエントリ（Sprint 5-11-5 / Phase 5-11-5）。
///
/// `Bell`（VT BEL `0x07`）/ `OSC 9`（iTerm2 互換通知）/ `OSC 777`（urxvt 互換通知）を
/// AccessKit `Role::Alert` ノードとして公開するためのデータホルダー。
///
/// **ライフサイクル**:
/// - サーバーから `ServerToClient::Bell` / `ServerToClient::DesktopNotification` を
///   受信した時点で `ClientState::add_alert` が `alerts` キューに追加する
/// - `update_accesskit_tree_if_needed` の冒頭で `expire_alerts` を呼び TTL 切れを除去
/// - キュー長が `ALERTS_MAX_LEN` を超えた場合、古い順に drop
///
/// **NodeId**: `accessibility::alert_node_id(seq) = NODE_ID_ALERT_OFFSET + seq`。
/// `seq` はクライアント起動以来の単調増加カウンター（`u64`）で衝突しない。
#[derive(Debug, Clone)]
pub struct AlertEntry {
    /// 単調増加シーケンス番号（NodeId 計算用）
    pub seq: u64,
    /// アラート種別
    pub kind: AlertKind,
    /// 発火元ペイン ID（将来の「ペイン X からの通知」表記やソース絞り込み用に保持）
    #[allow(dead_code)]
    pub pane_id: u32,
    /// 表題（OSC 9 はサーバーから "Nexterm" として届く、OSC 777 はサーバーで与えられたタイトル、Bell はローカライズ済み）
    pub title: String,
    /// 本文（Bell は空文字列、Notification は VT パーサで決定された本文）
    pub body: String,
    /// 追加時刻（TTL 判定用）
    pub created_at: std::time::Instant,
}

/// アラート種別（Sprint 5-11-5）。
///
/// OSC 9 / OSC 777 はサーバー側 `ServerToClient::DesktopNotification` に統合されているため
/// クライアント層では区別できない（両者とも VT パーサで `set_pending_notification` に集約）。
/// SR 観点でも「通知」として 1 種に扱って差し支えないため `Notification` の単一バリアントにする。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// VT BEL `0x07` 受信
    Bell,
    /// OSC 9（iTerm2 互換）/ OSC 777（urxvt 互換）デスクトップ通知
    Notification,
}

/// アラートキューの最大長（Sprint 5-11-5）。
///
/// 超過時は最も古いエントリから順に drop される。SR は新しいアラートのみを
/// アナウンスするため、過去のものを保持する価値は低い。
pub const ALERTS_MAX_LEN: usize = 16;

/// アラートの TTL（Sprint 5-11-5）。
///
/// SR が読み上げた後にツリーから自動削除して肥大化を防ぐ。5 秒は典型的な
/// SR アナウンス時間と人間が認知する時間の双方を考慮した値。
pub const ALERT_TTL: std::time::Duration = std::time::Duration::from_secs(5);

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
    /// ステータスバー左側ウィジェットの最終評価テキスト（キャッシュ）
    pub status_bar_text: String,
    /// ステータスバー右側ウィジェットの最終評価テキスト（キャッシュ）
    pub status_bar_right_text: String,
    /// BEL 受信フラグ（次の about_to_wait で OS 通知をトリガーする）
    pub pending_bell: bool,
    /// コピーモード（Vim 風テキスト選択）
    pub copy_mode: CopyModeState,
    /// マウスドラッグ選択
    pub mouse_sel: MouseSelection,
    /// IME 変換中テキスト（プリエディット）
    pub ime_preedit: Option<String>,
    /// ブロードキャストモード中か
    pub broadcast_mode: bool,
    /// ペイン番号オーバーレイ表示中か
    pub display_panes_mode: bool,
    /// 右クリックで開いたコンテキストメニュー（None = 非表示）
    pub context_menu: Option<ContextMenu>,
    /// ペインズームが有効かどうか
    pub is_zoomed: bool,
    /// Quick Select モード
    pub quick_select: QuickSelectState,
    /// ホストマネージャ UI
    pub host_manager: HostManager,
    /// Lua マクロピッカー UI
    pub macro_picker: MacroPicker,
    /// SFTP ファイル転送ダイアログ
    pub file_transfer: FileTransferDialog,
    /// 設定パネル（Ctrl+,）
    pub settings_panel: SettingsPanel,
    /// マウスレポーティングモード（サーバーから通知される: 0=無効, 1=X11, 2=SGR）
    #[allow(dead_code)]
    pub mouse_reporting_mode: u8,
    /// フローティングペインの位置情報キャッシュ
    pub floating_pane_rects: HashMap<u32, FloatRect>,
    /// タブバーの各タブのクリック範囲（pane_id → (x_start, x_end)）
    /// レンダラーが毎フレーム更新し、マウスハンドラが参照する
    pub tab_hit_rects: HashMap<u32, (f32, f32)>,
    /// タブホバー時の `[↗]` 分離ボタンのクリック範囲（Sprint 5-9 Phase 4-6）。
    ///
    /// `pane_id → (x_start, x_end)`。レンダラーが毎フレーム hover 中タブのみに対して
    /// 登録し、`event_handler/mouse.rs::on_mouse_left_pressed` がタブ判定より優先して
    /// 検出する。クリックされた場合は `DetachToNewWindow` 経路を発火し、対象ペインを
    /// 新規 OS Window に分離する。Wayland 環境でもグローバル座標非依存に動作する。
    pub tab_tearout_hit_rects: HashMap<u32, (f32, f32)>,
    /// タブバーの設定ボタンのクリック範囲（x_start, x_end）
    pub settings_tab_rect: Option<(f32, f32)>,
    /// 現在マウスがホバーしているタブの pane_id（Sprint 5-7 / UI-1-1）。
    /// マウス移動時に renderer/event_handler/mouse.rs が更新し、タブバー描画で背景を明るくする。
    pub hovered_tab_id: Option<u32>,
    /// キーヒントオーバーレイの表示終了時刻（Sprint 5-7 / UI-1-4）。
    /// Leader 単独押下で 2 秒後の時刻をセットし、`lifecycle` でこの時刻を過ぎたら None に戻す。
    /// Some の間は画面下部に config.keys の prefix 系バインドを半透明表示する。
    pub key_hint_visible_until: Option<std::time::Instant>,
    /// tmux 風 prefix モード（Leader 押下直後）の終了時刻（Sprint 5-7 / UI-1-4 bug fix）。
    /// Leader 単独押下時に key_hint_visible_until と同時にセットされる。
    /// Some の間に来たキー入力は `<leader> X` 形式のバインドのみ照合され、
    /// マッチしなければ通常入力にフォールスルーする。期限切れまたはマッチ時に None に戻る。
    pub prefix_pending_until: Option<std::time::Instant>,
    /// 更新通知バナー（Some(version) = 表示中、None = 非表示）
    pub update_banner: Option<String>,
    /// 機密操作の同意ダイアログ（Sprint 4-1）
    /// Some の間はキー入力をすべてダイアログが消費する
    pub pending_consent: Option<ConsentDialog>,
    /// セッション中の「常に許可」決定（次回起動時はリセットされる）
    pub session_consent_overrides: SessionConsentOverrides,
    /// 現在アクティブなワークスペース名（Sprint 5-7 / Phase 2-1）。
    /// サーバーから `WorkspaceList` / `WorkspaceSwitched` を受信した時点で更新する。
    /// ステータスバーの `workspace` ビルトインウィジェットで参照される。
    pub current_workspace: String,
    /// Quake モード トグル要求の保留キュー（Sprint 5-7 / Phase 2-2）。
    ///
    /// `apply_server_message` で `QuakeToggleRequest` を受信した時点で値を入れ、
    /// lifecycle が次フレームで取り出して実際にウィンドウ操作を行う（winit Window
    /// への mutable アクセスを ClientState 内に閉じ込めない設計）。
    /// 値は `"toggle"` / `"show"` / `"hide"` のいずれか。
    pub pending_quake_action: Option<String>,
    /// タブ表示順序（Sprint 5-7 / Phase 2-3）。
    ///
    /// サーバーから受信した `LayoutChanged.panes` の配列順序を反映する（サーバーが
    /// `Window.pane_order` に従って並べた論理タブ順）。タブバー描画ループはこの
    /// 順序に従う。
    pub tab_order: Vec<u32>,
    /// タブドラッグ中の状態（Sprint 5-7 / Phase 2-3）。
    /// `Some` の間はゴーストタブを描画し、ドロップ時に並べ替えを実施する。
    pub tab_drag: Option<TabDragState>,
    /// アニメーション管理（Sprint 5-7 / Phase 3-2）。
    ///
    /// タブ切替・ペイン追加の時刻を記録し、レンダラーから [0,1] 進捗値を取得する。
    /// `AnimationsConfig.enabled = false` または `intensity = "off"` の場合、
    /// `scaled_duration_ms` が 0 を返すため進捗は常に 1.0 となり実質無効になる。
    pub animations: crate::animations::AnimationManager,
    /// 主 OS Window が表示しているサーバー Window ID（Sprint 5-8 Phase 4-4）。
    ///
    /// `WindowListChanged` を受信したとき、`is_focused = true` の Window ID を
    /// このフィールドに反映する。Tab tearing で主 Window のタブバーにドロップされた場合、
    /// このフィールド経由で `MovePaneToWindow.target_window_id` を解決する。
    pub focused_server_window_id: u32,
    /// `QueryForegroundProcess` の最新応答（Sprint 5-8 Phase 4-5）。
    ///
    /// `apply_server_message` で `ForegroundProcessStatus` を受信した時点で値を入れる。
    /// `event_handler` 側が `pending_close_request` と突き合わせて確認ダイアログ表示 /
    /// 即時 detach の判定を行ったあとに `take()` で取り出してクリアする。
    pub foreground_process_status: Option<ForegroundProcessStatus>,
    /// OS Window 閉じ要求の保留状態（Sprint 5-8 Phase 4-5）。
    ///
    /// `close_action = "prompt"` 設定時、ユーザーが OS Window の閉じ操作を発火させると
    /// `QueryForegroundProcess` を送信して本フィールドに記録する。応答（または確認ダイアログ
    /// での選択）に応じて detach / kill / cancel のいずれかを実行する。
    pub pending_close_request: Option<PendingCloseRequest>,
    /// 「Window を閉じますか？」確認ダイアログの表示状態（Sprint 5-8 Phase 4-5）。
    ///
    /// `Some` の間はレンダラーがモーダルダイアログを描画する。`Enter` で確定、
    /// `Esc` でキャンセル。Wayland 環境では `[↗]` 経路でも同じダイアログを再利用する。
    pub close_window_dialog: Option<CloseWindowDialog>,
    /// SR 向けアラート キュー（Sprint 5-11-5）。
    ///
    /// Bell / OSC 9 / OSC 777 を `Role::Alert` ノードとして公開するための FIFO。
    /// 長さは `ALERTS_MAX_LEN` 以下に抑えられ、TTL (`ALERT_TTL`) を超えたものは
    /// `update_accesskit_tree_if_needed` 冒頭の `expire_alerts` で自動除去される。
    pub alerts: std::collections::VecDeque<AlertEntry>,
    /// 次に発行する `AlertEntry.seq` 値（Sprint 5-11-5）。
    ///
    /// 単調増加カウンタ。クライアント 1 起動につき u64 を使い切ることは現実的に不可能
    /// （1 秒間 1000 件発火で約 5.84 億年）。NodeId 衝突回避の根拠。
    pub next_alert_seq: u64,
}

/// `QueryForegroundProcess` への応答情報（Sprint 5-8 Phase 4-5）
#[derive(Debug, Clone, Copy)]
pub struct ForegroundProcessStatus {
    /// 問い合わせ対象の Server Window ID
    pub window_id: u32,
    /// 前景プロセスが動作中なら `true`
    pub has_foreground: bool,
}

/// OS Window 閉じ要求の保留状態（Sprint 5-8 Phase 4-5）。
///
/// `close_action` フィールドは将来 `Detach`/`Kill` でも保留経路を通す拡張用に保持する。
/// 現状 `Prompt` のみ pending 状態を取るため、レンダラー側からは未読 (dead_code 抑制)。
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct PendingCloseRequest {
    /// 閉じ要求の発火元 OS Window が表示している Server Window ID
    pub server_window_id: u32,
    /// `window.close_action` 設定値
    pub close_action: CloseActionKind,
}

/// `WindowConfig.close_action` のクライアント側ミラー（Sprint 5-8 Phase 4-5）。
///
/// サーバー側 `nexterm_config::CloseAction` と意味的に等価。クライアント側で
/// `pending_close_request` を判断するため独立した enum を持つ（クレート間依存を増やさない）。
///
/// `Detach` / `Kill` バリアントは現状 `pending_close_request.close_action` には設定されない
/// （`Prompt` のときのみ pending 状態に入るため）が、将来 `Detach` でも確認ダイアログを出す
/// 設定や、複数 Window 個別 close で `Kill` を保留する経路で利用される予定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CloseActionKind {
    /// 前景プロセス検知時のみ確認ダイアログを表示（デフォルト）
    Prompt,
    /// 確認なしで detach（サーバー側は維持）
    Detach,
    /// 確認なしで kill（既存挙動）
    Kill,
}

/// Window 閉じ確認ダイアログの表示状態（Sprint 5-8 Phase 4-5）。
///
/// レンダラー側でのダイアログ描画は後続実装で接続予定のため、`server_window_id` /
/// `message` / `kill_label` / `cancel_label` は現状未読 (dead_code 抑制)。
/// 状態フローのみ `poll_pending_close_request` で読まれ、`selected_button` の
/// シグナル値（`0xFE` = Kill 確定、`0xFF` = キャンセル）で確定判定する。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CloseWindowDialog {
    /// 確認対象の Server Window ID
    pub server_window_id: u32,
    /// 表示メッセージ（i18n 済み）
    pub message: String,
    /// 「閉じる（Kill）」ボタンのラベル（i18n 済み）
    pub kill_label: String,
    /// 「キャンセル」ボタンのラベル（i18n 済み）
    pub cancel_label: String,
    /// 現在ハイライト中のボタン（0 = Kill, 1 = Cancel、0xFE = Kill 確定、0xFF = キャンセル確定）
    pub selected_button: u8,
}

/// タブドラッグ中の状態（Sprint 5-7 / Phase 2-3）
#[derive(Debug, Clone)]
pub struct TabDragState {
    /// ドラッグ開始時の pane ID（移動対象のタブ）
    pub pane_id: u32,
    /// ドラッグ開始時のマウス X 座標（クリック判定との閾値判定に使用）
    pub start_x: f32,
    /// 現在のマウス X 座標（ゴースト描画位置に使用）
    pub current_x: f32,
    /// 現在ホバー中の挿入先 pane ID（ドロップ時に target_id の位置に移動）
    /// `None` は挿入先未確定（タブバー外 or 自分自身の上）
    pub hover_target: Option<u32>,
    /// 実際にドラッグと判定済みか（X 移動量が閾値超え）。
    /// `false` のままリリースされた場合は通常クリック扱い。
    pub committed: bool,
    /// ドラッグ開始時の OS Window ID（Sprint 5-8 Phase 4-2）。
    ///
    /// Phase 4-2 ではタブ外ドロップ判定で source を識別するために使用する。
    /// 主 Window 未初期化時の安全性を `Option` で確保（実運用では常に `Some`）。
    #[allow(dead_code)]
    pub source_os_window_id: Option<WindowId>,
    /// ドラッグ開始時のスクリーン座標（Sprint 5-8 Phase 4-2）。
    ///
    /// `event_handler::mouse::on_mouse_left_pressed` で
    /// プラットフォーム別ヘルパー（Step 2.3 で追加）から取得する。
    /// グローバル座標が取得不能なプラットフォーム（Wayland）では `None`。
    #[allow(dead_code)]
    pub start_screen_pos: Option<(i32, i32)>,
    /// 現在のスクリーン座標（Sprint 5-8 Phase 4-2）。
    ///
    /// `event_handler::mouse::on_cursor_moved` で更新（Step 2.4 配線）。
    /// ドロップ時の判定（Step 2.5）で `compute_drop_target` の引数に渡す。
    /// `None` の場合は新規 OS Window 生成判定を行わない（既存挙動維持）。
    #[allow(dead_code)]
    pub current_screen_pos: Option<(i32, i32)>,
}

impl ClientState {
    pub fn new(cols: u16, rows: u16, scrollback_capacity: usize) -> Self {
        Self {
            panes: HashMap::new(),
            focused_pane_id: None,
            pane_layouts: HashMap::new(),
            cols,
            rows,
            // Sprint 5-7 / Phase 3-3: 永続化された使用履歴をロード
            palette: CommandPalette::new_with_history(),
            search: SearchState::new(),
            scrollback_capacity,
            status_bar_text: String::new(),
            status_bar_right_text: String::new(),
            pending_bell: false,
            copy_mode: CopyModeState::new(),
            mouse_sel: MouseSelection::new(),
            ime_preedit: None,
            broadcast_mode: false,
            display_panes_mode: false,
            context_menu: None,
            is_zoomed: false,
            quick_select: QuickSelectState::new(),
            host_manager: HostManager::new(vec![]),
            macro_picker: MacroPicker::new(vec![]),
            file_transfer: FileTransferDialog::new(),
            settings_panel: SettingsPanel::default(),
            mouse_reporting_mode: 0,
            floating_pane_rects: HashMap::new(),
            tab_hit_rects: HashMap::new(),
            tab_tearout_hit_rects: HashMap::new(),
            settings_tab_rect: None,
            hovered_tab_id: None,
            key_hint_visible_until: None,
            prefix_pending_until: None,
            update_banner: None,
            pending_consent: None,
            session_consent_overrides: SessionConsentOverrides::default(),
            current_workspace: "default".to_string(),
            pending_quake_action: None,
            tab_order: Vec::new(),
            tab_drag: None,
            animations: crate::animations::AnimationManager::new(),
            // Phase 4-4: WindowListChanged 受信時に focused Window ID を反映する
            focused_server_window_id: 0,
            // Phase 4-5: Window 閉じ確認ダイアログ用
            foreground_process_status: None,
            pending_close_request: None,
            close_window_dialog: None,
            // Sprint 5-11-5: AccessKit Role::Alert 通知キュー
            alerts: std::collections::VecDeque::new(),
            next_alert_seq: 0,
        }
    }

    /// SR 向けアラートをキューに追加する（Sprint 5-11-5）。
    ///
    /// `seq` は自動採番。キュー長が `ALERTS_MAX_LEN` を超えた場合は古い順に drop する
    /// （`pop_front`）。本メソッドはタイトル / 本文を所有権ごと受け取って所有する。
    ///
    /// 戻り値: 採番された `seq`。呼び出し側でログ等に使う想定。
    pub fn add_alert(&mut self, kind: AlertKind, pane_id: u32, title: String, body: String) -> u64 {
        let seq = self.next_alert_seq;
        self.next_alert_seq = self.next_alert_seq.wrapping_add(1);
        self.alerts.push_back(AlertEntry {
            seq,
            kind,
            pane_id,
            title,
            body,
            created_at: std::time::Instant::now(),
        });
        // 上限を超えたら古いものから drop
        while self.alerts.len() > ALERTS_MAX_LEN {
            self.alerts.pop_front();
        }
        seq
    }

    /// TTL を超えたアラートをキューから除去する（Sprint 5-11-5）。
    ///
    /// `now` は呼び出し側で `Instant::now()` を計算して渡す（テスタビリティのため）。
    /// `created_at + ALERT_TTL < now` のエントリを `pop_front` で順次除去する。
    /// アラートは時刻順に追加されるため、先頭から見て期限内のエントリが現れた時点で停止する。
    ///
    /// 戻り値: 除去したエントリ数。
    pub fn expire_alerts(&mut self, now: std::time::Instant) -> usize {
        let mut removed = 0;
        while let Some(front) = self.alerts.front() {
            if now.duration_since(front.created_at) >= ALERT_TTL {
                self.alerts.pop_front();
                removed += 1;
            } else {
                break;
            }
        }
        removed
    }

    /// 指定 `seq` のアラートを即時 dismiss する（Phase 5-11-6 #4）。
    ///
    /// SR の `Action::Click` 経路で TTL（5 秒）を待たずアラートを除去するために使う。
    /// 該当 seq が存在しない場合（既に `expire_alerts` で除去済み等）は副作用なし。
    ///
    /// 戻り値: 該当 seq を除去したら `true`、見つからなければ `false`。
    pub fn dismiss_alert(&mut self, seq: u64) -> bool {
        let before = self.alerts.len();
        self.alerts.retain(|a| a.seq != seq);
        before != self.alerts.len()
    }

    /// フォーカスペインを切り替え、アクティビティフラグをクリアする。
    ///
    /// Sprint 5-7 / Phase 3-2: 切替時にタブ切替アニメーションも記録する
    /// （前回と同じ pane を再フォーカスした場合はアニメーション再開なし）。
    #[allow(dead_code)]
    pub fn set_focused_pane(&mut self, pane_id: u32) {
        let prev = self.focused_pane_id;
        self.focused_pane_id = Some(pane_id);
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.has_activity = false;
        }
        if prev != Some(pane_id) {
            self.animations
                .record_tab_switch(pane_id, std::time::Instant::now());
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
        self.focused_pane_id.and_then(|id| self.panes.get_mut(&id))
    }

    /// コマンドパレットをトグルする
    pub fn toggle_palette(&mut self) {
        if self.palette.is_open {
            self.palette.close();
        } else {
            self.palette.open();
        }
    }
}
