//! クライアント状態 — グリッド・スクロールバック・パレット・検索を統合管理する

use std::collections::HashMap;

use nexterm_proto::{Grid, PaneLayout, ServerToClient};

use crate::host_manager::HostManager;
use crate::macro_picker::MacroPicker;
use crate::palette::CommandPalette;
use crate::scrollback::Scrollback;
use crate::settings_panel::SettingsPanel;

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
}

impl PaneState {
    fn new(cols: u16, rows: u16, scrollback_capacity: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            cursor_col: 0,
            cursor_row: 0,
            scrollback: Scrollback::new(scrollback_capacity),
            scroll_offset: 0,
            images: HashMap::new(),
            has_activity: false,
            title: String::new(),
        }
    }

    fn apply_diff(
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

/// インクリメンタル検索の状態
pub struct SearchState {
    pub query: String,
    pub is_active: bool,
    /// 現在ハイライト中の行インデックス（スクロールバック内）
    pub current_match: Option<usize>,
}

impl SearchState {
    fn new() -> Self {
        Self {
            query: String::new(),
            is_active: false,
            current_match: None,
        }
    }
}

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
    fn new() -> Self {
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

/// コンテキストメニューの各項目が実行するアクション
#[derive(Debug, Clone, PartialEq)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    SelectAll,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    InlineSearch,
    OpenSettings,
    /// プロファイル名を指定してシェルを開く
    OpenProfile {
        profile_name: String,
    },
    /// セパレーター（クリック不可）
    Separator,
}

/// コンテキストメニューの1項目
#[derive(Debug, Clone)]
pub struct ContextMenuItem {
    pub label: String,
    /// キーヒント（右端に薄く表示）
    pub hint: String,
    pub action: ContextMenuAction,
}

impl ContextMenuItem {
    fn new(label: impl Into<String>, action: ContextMenuAction) -> Self {
        Self {
            label: label.into(),
            hint: String::new(),
            action,
        }
    }

    fn with_hint(
        label: impl Into<String>,
        hint: impl Into<String>,
        action: ContextMenuAction,
    ) -> Self {
        Self {
            label: label.into(),
            hint: hint.into(),
            action,
        }
    }

    fn separator() -> Self {
        Self {
            label: String::new(),
            hint: String::new(),
            action: ContextMenuAction::Separator,
        }
    }
}

/// 右クリックで表示するコンテキストメニュー
#[derive(Debug, Clone)]
pub struct ContextMenu {
    /// メニューを表示するピクセル座標（左上）
    pub x: f32,
    pub y: f32,
    pub items: Vec<ContextMenuItem>,
    /// 現在ホバー中の項目インデックス
    pub hovered: Option<usize>,
}

impl ContextMenu {
    /// 標準メニュー項目を持つコンテキストメニューを生成する
    /// profiles: プロファイル名とアイコンのペア一覧
    pub fn new_default(x: f32, y: f32, profiles: &[(String, String)]) -> Self {
        let mut items = vec![
            ContextMenuItem::with_hint("コピー", "Ctrl+C", ContextMenuAction::Copy),
            ContextMenuItem::with_hint("貼り付け", "Ctrl+V", ContextMenuAction::Paste),
            ContextMenuItem::with_hint("すべて選択", "Ctrl+A", ContextMenuAction::SelectAll),
            ContextMenuItem::separator(),
            ContextMenuItem::with_hint("垂直分割", "Ctrl+B  %", ContextMenuAction::SplitVertical),
            ContextMenuItem::with_hint(
                "水平分割",
                "Ctrl+B  \"",
                ContextMenuAction::SplitHorizontal,
            ),
            ContextMenuItem::with_hint("ペインを閉じる", "Ctrl+B  x", ContextMenuAction::ClosePane),
        ];

        // プロファイルが登録されていればサブセクションを追加する
        if !profiles.is_empty() {
            items.push(ContextMenuItem::separator());
            for (name, icon) in profiles {
                let label = if icon.is_empty() {
                    format!("> {}", name)
                } else {
                    format!("{} {}", icon, name)
                };
                items.push(ContextMenuItem::new(
                    label,
                    ContextMenuAction::OpenProfile {
                        profile_name: name.clone(),
                    },
                ));
            }
        }

        items.push(ContextMenuItem::separator());
        items.push(ContextMenuItem::with_hint(
            "検索...",
            "Ctrl+F",
            ContextMenuAction::InlineSearch,
        ));
        items.push(ContextMenuItem::with_hint(
            "設定...",
            "Ctrl+,",
            ContextMenuAction::OpenSettings,
        ));

        Self {
            x,
            y,
            items,
            hovered: None,
        }
    }
}

/// ファイル転送ダイアログの状態
pub struct FileTransferDialog {
    pub is_open: bool,
    /// "upload" または "download"
    pub mode: String,
    /// 入力フィールドのインデックス（0 = ホスト名, 1 = ローカルパス, 2 = リモートパス）
    pub field: usize,
    pub host_name: String,
    pub local_path: String,
    pub remote_path: String,
}

impl FileTransferDialog {
    pub fn new() -> Self {
        Self {
            is_open: false,
            mode: "upload".to_string(),
            field: 0,
            host_name: String::new(),
            local_path: String::new(),
            remote_path: String::new(),
        }
    }

    pub fn open_upload(&mut self) {
        self.mode = "upload".to_string();
        self.field = 0;
        self.host_name.clear();
        self.local_path.clear();
        self.remote_path.clear();
        self.is_open = true;
    }

    pub fn open_download(&mut self) {
        self.mode = "download".to_string();
        self.field = 0;
        self.host_name.clear();
        self.local_path.clear();
        self.remote_path.clear();
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn current_field_mut(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.host_name,
            1 => &mut self.local_path,
            _ => &mut self.remote_path,
        }
    }

    pub fn next_field(&mut self) {
        self.field = (self.field + 1).min(2);
    }

    pub fn prev_field(&mut self) {
        self.field = self.field.saturating_sub(1);
    }
}

/// Quick Select モードのマッチ結果
#[derive(Debug, Clone)]
pub struct QuickSelectMatch {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub text: String,
    /// 選択ラベル（a, b, c, ... / aa, ab, ...）
    pub label: String,
}

/// Quick Select モードの状態
pub struct QuickSelectState {
    pub is_active: bool,
    pub matches: Vec<QuickSelectMatch>,
    /// 現在タイプ中のラベル
    pub typed_label: String,
}

impl QuickSelectState {
    fn new() -> Self {
        Self {
            is_active: false,
            matches: Vec::new(),
            typed_label: String::new(),
        }
    }

    pub fn enter(&mut self, grid_rows: &[Vec<nexterm_proto::Cell>]) {
        self.is_active = true;
        self.typed_label.clear();
        self.matches = find_quick_select_matches(grid_rows);
    }

    pub fn exit(&mut self) {
        self.is_active = false;
        self.matches.clear();
        self.typed_label.clear();
    }

    /// タイプされたラベルが一致するマッチを返す
    pub fn accept(&self) -> Option<&QuickSelectMatch> {
        if self.typed_label.is_empty() {
            return None;
        }
        self.matches.iter().find(|m| m.label == self.typed_label)
    }
}

/// グリッドから Quick Select マッチを検索する（URL・パス・単語）
fn find_quick_select_matches(rows: &[Vec<nexterm_proto::Cell>]) -> Vec<QuickSelectMatch> {
    let patterns: &[(&str, &str)] = &[
        // URL
        (r#"https?://[^\s<>"'\]]+"#, "url"),
        // ファイルパス (Unix)
        (r"(?:^|[\s(])((?:/[^\s/:]+)+/?)", "path"),
        // IPv4 アドレス
        (r"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?\b", "ip"),
        // SHA / Git ハッシュ (7-40 hex)
        (r"\b[0-9a-f]{7,40}\b", "hash"),
        // 数字
        (r"\b\d+\b", "num"),
    ];

    let mut all_matches: Vec<QuickSelectMatch> = Vec::new();
    let label_chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();

    for (row_idx, cells) in rows.iter().enumerate() {
        let line: String = cells.iter().map(|c| c.ch).collect();
        for (pat_str, _) in patterns {
            if let Ok(re) = regex::Regex::new(pat_str) {
                for m in re.find_iter(&line) {
                    all_matches.push(QuickSelectMatch {
                        row: row_idx as u16,
                        col_start: m.start() as u16,
                        col_end: m.end() as u16,
                        text: m.as_str().to_string(),
                        label: String::new(), // will be filled below
                    });
                }
            }
        }
    }

    // ラベルを割り当てる（a, b, ..., z, aa, ab, ...）
    let n = all_matches.len();
    for (i, m) in all_matches.iter_mut().enumerate() {
        m.label = index_to_label(i, n, &label_chars);
    }

    all_matches
}

fn index_to_label(i: usize, total: usize, chars: &[char]) -> String {
    let base = chars.len();
    if total <= base {
        return chars[i % base].to_string();
    }
    let second = i / base;
    let first = i % base;
    if second == 0 {
        chars[first].to_string()
    } else {
        format!("{}{}", chars[second - 1], chars[first])
    }
}

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
    /// 更新通知バナー（Some(version) = 表示中、None = 非表示）
    pub update_banner: Option<String>,
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
            update_banner: None,
        }
    }

    pub fn apply_server_message(&mut self, msg: ServerToClient) {
        match msg {
            ServerToClient::FullRefresh { pane_id, grid } => {
                let cursor_col = grid.cursor_col;
                let cursor_row = grid.cursor_row;
                let capacity = self.scrollback_capacity;
                let state = self
                    .panes
                    .entry(pane_id)
                    .or_insert_with(|| PaneState::new(grid.width, grid.height, capacity));
                state.grid = grid;
                state.cursor_col = cursor_col;
                state.cursor_row = cursor_row;
                if self.focused_pane_id.is_none() {
                    self.focused_pane_id = Some(pane_id);
                }
            }
            ServerToClient::GridDiff {
                pane_id,
                dirty_rows,
                cursor_col,
                cursor_row,
            } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.apply_diff(dirty_rows, cursor_col, cursor_row);
                    // 非フォーカスペインへの出力はアクティビティとしてマーク
                    if self.focused_pane_id != Some(pane_id) {
                        pane.has_activity = true;
                    }
                }
            }
            ServerToClient::Pong => {}
            ServerToClient::Error { message } => {
                tracing::error!("サーバーエラー: {}", message);
            }
            ServerToClient::SessionList { .. } => {}
            ServerToClient::ImagePlaced {
                pane_id,
                image_id,
                col,
                row,
                width,
                height,
                rgba,
            } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.images.insert(
                        image_id,
                        PlacedImage {
                            col,
                            row,
                            width,
                            height,
                            rgba,
                        },
                    );
                    if self.focused_pane_id != Some(pane_id) {
                        pane.has_activity = true;
                    }
                }
            }
            ServerToClient::Bell { .. } => {
                // OS のウィンドウ注目要求をトリガーするためフラグを立てる
                self.pending_bell = true;
            }
            ServerToClient::RecordingStarted { .. } | ServerToClient::RecordingStopped { .. } => {}
            ServerToClient::WindowListChanged { .. } | ServerToClient::PaneClosed { .. } => {}
            // OSC 0/2 タイトル変更 — ペインのタイトルフィールドを更新する
            ServerToClient::TitleChanged { pane_id, title } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.title = title;
                }
            }
            ServerToClient::DesktopNotification { .. } => {}
            ServerToClient::BroadcastModeChanged { enabled } => {
                self.broadcast_mode = enabled;
            }
            ServerToClient::AsciicastStarted { .. } | ServerToClient::AsciicastStopped { .. } => {}
            ServerToClient::TemplateSaved { .. }
            | ServerToClient::TemplateLoaded { .. }
            | ServerToClient::TemplateList { .. } => {}
            ServerToClient::ZoomChanged { is_zoomed } => {
                self.is_zoomed = is_zoomed;
            }
            // ペイン分離・シリアル接続はサーバーから LayoutChanged / WindowListChanged が後続するため状態更新不要
            ServerToClient::PaneBroken { .. } | ServerToClient::SerialConnected { .. } => {}
            // SFTP 転送進捗・完了はステータスバーに表示する
            ServerToClient::SftpProgress {
                path,
                transferred,
                total,
            } => {
                let pct = if total > 0 {
                    transferred * 100 / total
                } else {
                    0
                };
                self.status_bar_text = format!("SFTP {} {}%", path, pct);
            }
            ServerToClient::SftpDone { path, error } => {
                if let Some(err) = error {
                    self.status_bar_text = format!("SFTP ERR: {}", err);
                } else {
                    self.status_bar_text = format!("SFTP OK: {}", path);
                }
            }
            // OSC 133 セマンティックゾーンマーク — ステータスバーに最新コマンド終了コードを表示
            ServerToClient::SemanticMark {
                pane_id,
                kind,
                exit_code,
                ..
            } => {
                if kind == "D"
                    && self.focused_pane_id == Some(pane_id)
                    && let Some(code) = exit_code
                {
                    if code != 0 {
                        self.status_bar_text = format!("[exit: {}]", code);
                    } else {
                        self.status_bar_text.clear();
                    }
                }
            }
            // フローティングペインイベント — 位置情報をキャッシュするが、
            // レンダラー側での描画は renderer.rs で別途実装する
            ServerToClient::FloatingPaneOpened {
                pane_id,
                col_off,
                row_off,
                cols,
                rows,
            } => {
                self.floating_pane_rects.insert(
                    pane_id,
                    FloatRect {
                        col_off,
                        row_off,
                        cols,
                        rows,
                    },
                );
            }
            ServerToClient::FloatingPaneMoved {
                pane_id,
                col_off,
                row_off,
                cols,
                rows,
            } => {
                self.floating_pane_rects.insert(
                    pane_id,
                    FloatRect {
                        col_off,
                        row_off,
                        cols,
                        rows,
                    },
                );
            }
            ServerToClient::FloatingPaneClosed { pane_id } => {
                self.floating_pane_rects.remove(&pane_id);
            }
            ServerToClient::LayoutChanged {
                panes,
                focused_pane_id,
            } => {
                // レイアウトを全更新する
                self.pane_layouts.clear();
                for layout in panes {
                    self.pane_layouts.insert(layout.pane_id, layout);
                }
                // フォーカスペインを更新してアクティビティフラグをクリアする
                self.focused_pane_id = Some(focused_pane_id);
                if let Some(pane) = self.panes.get_mut(&focused_pane_id) {
                    pane.has_activity = false;
                }
            }
            // プラグイン操作応答は GPU クライアントでは無視する
            ServerToClient::PluginList { .. } | ServerToClient::PluginOk { .. } => {}
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

    /// スクロールバック検索を開始する
    pub fn start_search(&mut self) {
        self.search.is_active = true;
        self.search.query.clear();
        self.search.current_match = None;
    }

    /// 検索クエリに文字を追加してインクリメンタルに検索する
    pub fn push_search_char(&mut self, ch: char) {
        self.search.query.push(ch);
        self.search_next_from(0);
    }

    /// 検索クエリの末尾を削除する
    pub fn pop_search_char(&mut self) {
        self.search.query.pop();
        self.search_next_from(0);
    }

    /// 次のマッチへ移動する
    pub fn search_next(&mut self) {
        let from = self.search.current_match.map(|m| m + 1).unwrap_or(0);
        self.search_next_from(from);
    }

    /// 前のマッチへ移動する
    pub fn search_prev(&mut self) {
        let query = self.search.query.clone();
        let current = self.search.current_match.unwrap_or(0);
        let result = self
            .focused_pane_mut()
            .and_then(|pane| pane.scrollback.search_prev(&query, current));
        self.search.current_match = result;
        if let Some(row) = result
            && let Some(pane) = self.focused_pane_mut()
        {
            pane.scroll_offset = row;
        }
    }

    fn search_next_from(&mut self, from: usize) {
        let query = self.search.query.clone();
        // 先に検索結果を取得してからボローを解放する
        let result = self
            .focused_pane_mut()
            .and_then(|pane| pane.scrollback.search_next(&query, from));
        self.search.current_match = result;
        if let Some(row) = result
            && let Some(pane) = self.focused_pane_mut()
        {
            pane.scroll_offset = row;
        }
    }

    /// スクロールバックを1画面分上にスクロールする
    pub fn scroll_up(&mut self, lines: usize) {
        if let Some(pane) = self.focused_pane_mut() {
            let max_offset = pane.scrollback.len().saturating_sub(1);
            pane.scroll_offset = (pane.scroll_offset + lines).min(max_offset);
        }
    }

    /// スクロールバックを1画面分下にスクロールする
    pub fn scroll_down(&mut self, lines: usize) {
        if let Some(pane) = self.focused_pane_mut() {
            pane.scroll_offset = pane.scroll_offset.saturating_sub(lines);
        }
    }

    /// 検索を終了する
    pub fn end_search(&mut self) {
        self.search.is_active = false;
        self.search.query.clear();
        self.search.current_match = None;
        if let Some(pane) = self.focused_pane_mut() {
            pane.scroll_offset = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_proto::{Cell, DirtyRow, Grid};

    #[test]
    fn full_refreshでペインが登録される() {
        let mut state = ClientState::new(80, 24, 1000);
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        assert!(state.panes.contains_key(&1));
        assert_eq!(state.focused_pane_id, Some(1));
    }

    #[test]
    fn grid_diffで差分が適用される() {
        let mut state = ClientState::new(80, 24, 1000);
        state.apply_server_message(ServerToClient::FullRefresh {
            pane_id: 1,
            grid: Grid::new(80, 24),
        });
        let mut row = vec![Cell::default(); 80];
        row[0].ch = 'X';
        state.apply_server_message(ServerToClient::GridDiff {
            pane_id: 1,
            dirty_rows: vec![DirtyRow { row: 0, cells: row }],
            cursor_col: 1,
            cursor_row: 0,
        });
        let pane = state.focused_pane().unwrap();
        assert_eq!(pane.grid.rows[0][0].ch, 'X');
    }

    #[test]
    fn 検索のライフサイクル() {
        let mut state = ClientState::new(80, 24, 1000);
        state.start_search();
        assert!(state.search.is_active);
        state.push_search_char('a');
        assert_eq!(state.search.query, "a");
        state.end_search();
        assert!(!state.search.is_active);
        assert!(state.search.query.is_empty());
    }
}
