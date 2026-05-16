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
    /// タブバーの設定ボタンのクリック範囲（x_start, x_end）
    pub settings_tab_rect: Option<(f32, f32)>,
    /// 現在マウスがホバーしているタブの pane_id（Sprint 5-7 / UI-1-1）。
    /// マウス移動時に renderer/event_handler/mouse.rs が更新し、タブバー描画で背景を明るくする。
    pub hovered_tab_id: Option<u32>,
    /// キーヒントオーバーレイの表示終了時刻（Sprint 5-7 / UI-1-4）。
    /// Leader 単独押下で 2 秒後の時刻をセットし、`lifecycle` でこの時刻を過ぎたら None に戻す。
    /// Some の間は画面下部に config.keys の prefix 系バインドを半透明表示する。
    pub key_hint_visible_until: Option<std::time::Instant>,
    /// 更新通知バナー（Some(version) = 表示中、None = 非表示）
    pub update_banner: Option<String>,
    /// 機密操作の同意ダイアログ（Sprint 4-1）
    /// Some の間はキー入力をすべてダイアログが消費する
    pub pending_consent: Option<ConsentDialog>,
    /// セッション中の「常に許可」決定（次回起動時はリセットされる）
    pub session_consent_overrides: SessionConsentOverrides,
}

impl ClientState {
    pub fn new(cols: u16, rows: u16, scrollback_capacity: usize) -> Self {
        Self {
            panes: HashMap::new(),
            focused_pane_id: None,
            pane_layouts: HashMap::new(),
            cols,
            rows,
            palette: CommandPalette::new(),
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
            settings_tab_rect: None,
            hovered_tab_id: None,
            key_hint_visible_until: None,
            update_banner: None,
            pending_consent: None,
            session_consent_overrides: SessionConsentOverrides::default(),
        }
    }

    /// フォーカスペインを切り替え、アクティビティフラグをクリアする
    #[allow(dead_code)]
    pub fn set_focused_pane(&mut self, pane_id: u32) {
        self.focused_pane_id = Some(pane_id);
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.has_activity = false;
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
