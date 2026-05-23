//! Sprint 5-11-1〜5-11-2 / H1: スクリーンリーダー対応のノードツリー生成
//!
//! 監査ラウンド 2 タスク **H1**（スクリーンリーダー対応）の実装。
//! 競合 OSS（kitty / WezTerm / Alacritty / Ghostty）はいずれもスクリーンリーダー対応が
//! 薄いため、本対応が完成すれば明確な差別化ポイントとなる（`project_audit_round2.md` 参照）。
//!
//! ## このモジュールが提供するもの
//!
//! - **NodeId 体系**: 固定 ID + ペイン/タブ/オーバーレイ項目の動的 ID
//! - **動的ツリー生成**: `build_tree_from_state(&ClientState)` でタブ・ペイン + 最前面オーバーレイを反映
//! - `accesskit_winit::Adapter::update_if_active` に渡すツリー（OS の a11y API に転送される）
//!
//! ## ロードマップ
//!
//! - Phase 5-11-1 PoC ✅: 固定ツリー + Adapter 統合
//! - Phase 5-11-2 Step 2-1 ✅: ClientState から動的ツリー生成（タブ・ペイン）
//! - **Phase 5-11-2 Step 2-2 ⬅️**: オーバーレイ（CommandPalette / ContextMenu / CloseWindowDialog / SettingsPanel / HostManager / MacroPicker / update_banner）
//! - Phase 5-11-2 Step 2-3: 複数 OS Window 対応
//! - Phase 5-11-2 Step 2-4: Action 応答（Focus / Click）
//! - Phase 5-11-3: ターミナル grid 差分通知（100ms スロットリング）
//! - Phase 5-11-4: OSC 133 連動レビューモード
//! - Phase 5-11-5: 設定 UI + i18n + ドキュメント

use accesskit::{Live, Node, NodeId, Role, Tree, TreeId, TreeUpdate};

use crate::host_manager::HostManager;
use crate::macro_picker::MacroPicker;
use crate::palette::CommandPalette;
use crate::settings_panel::SettingsPanel;
use crate::state::{ClientState, CloseWindowDialog, ContextMenu, QuickSelectState};

// ===== 固定 NodeId =====
//
// プラットフォーム a11y アダプタはノード ID をキャッシュ・追跡するため、
// **安定** であることが重要。ペイン削除後も同じ ID が再利用されないように
// オフセット付きで割り当てる。

/// ルートノード（OS ウィンドウ全体）
pub const ROOT_ID: NodeId = NodeId(1);

/// タブバー（`Role::TabList`）
pub const TAB_BAR_ID: NodeId = NodeId(2);

/// ペイン領域コンテナ（`Role::Group`）
pub const PANE_AREA_ID: NodeId = NodeId(3);

// ===== オーバーレイ固定 NodeId（Step 2-2）=====

/// 設定パネル（Ctrl+,）のルート
pub const SETTINGS_PANEL_ID: NodeId = NodeId(4);

/// コマンドパレット（Ctrl+Shift+P）のルート
pub const PALETTE_ID: NodeId = NodeId(5);

/// ホストマネージャ
pub const HOST_MANAGER_ID: NodeId = NodeId(6);

/// マクロピッカー
pub const MACRO_PICKER_ID: NodeId = NodeId(7);

/// コンテキストメニュー（右クリック）
pub const CONTEXT_MENU_ID: NodeId = NodeId(8);

/// 「Window を閉じますか？」確認ダイアログ
pub const CLOSE_DIALOG_ID: NodeId = NodeId(9);

/// 更新通知バナー
pub const UPDATE_BANNER_ID: NodeId = NodeId(10);

/// Quick Select オーバーレイのルート（Step 2-2-h）
pub const QUICK_SELECT_ID: NodeId = NodeId(11);

/// コマンドパレットの検索入力フィールド
pub const PALETTE_SEARCH_ID: NodeId = NodeId(12);

/// コマンドパレットの候補リスト
pub const PALETTE_LIST_ID: NodeId = NodeId(13);

/// 確認ダイアログの「閉じる/プロセスを終了」ボタン
pub const CLOSE_DIALOG_KILL_BTN: NodeId = NodeId(14);

/// 確認ダイアログの「キャンセル」ボタン
pub const CLOSE_DIALOG_CANCEL_BTN: NodeId = NodeId(15);

/// Quick Select マッチ一覧の `ListBox`（Step 2-2-h）
pub const QUICK_SELECT_LIST_ID: NodeId = NodeId(16);

// ===== SettingsPanel フィールド固定 NodeId（Step 2-2-e'）=====

/// 設定パネルのカテゴリ TabList
pub const SETTINGS_TABLIST_ID: NodeId = NodeId(17);

// 18〜24 は `SettingsCategory::ALL` のインデックスに対応するタブ（`settings_tab_id_at` 参照）

/// 設定パネルの現在カテゴリ内容コンテナ（`Group`）
pub const SETTINGS_CONTENT_ID: NodeId = NodeId(25);

// 26〜29 は将来のコンテナ（サイドバー等）用に予約

/// Font カテゴリ: フォントファミリー入力欄
pub const SETTINGS_FONT_FAMILY_ID: NodeId = NodeId(30);

/// Font カテゴリ: フォントサイズスライダー
pub const SETTINGS_FONT_SIZE_ID: NodeId = NodeId(31);

/// Theme カテゴリ: カラースキーム選択
pub const SETTINGS_THEME_SCHEME_ID: NodeId = NodeId(32);

/// Window カテゴリ: 不透明度スライダー
pub const SETTINGS_WINDOW_OPACITY_ID: NodeId = NodeId(33);

/// Startup カテゴリ: 言語選択
pub const SETTINGS_STARTUP_LANGUAGE_ID: NodeId = NodeId(34);

/// Startup カテゴリ: 起動時更新確認 CheckBox
pub const SETTINGS_STARTUP_AUTO_UPDATE_ID: NodeId = NodeId(35);

// 36〜99 は将来のフィールド（SSH / Keybindings / Profiles など）用に予約

/// 設定パネルカテゴリタブのベース NodeId。
///
/// 値域は `[18, 18 + SettingsCategory::ALL.len()) = [18, 25)`。`SETTINGS_CONTENT_ID = 25` と隣接するが、
/// `decode_node_id` のレンジマッチで衝突を防ぐ。
const SETTINGS_TAB_BASE: u64 = 18;

/// `SettingsCategory::ALL` のインデックスから対応するタブの NodeId を計算する。
pub fn settings_tab_id_at(idx: usize) -> NodeId {
    NodeId(SETTINGS_TAB_BASE + idx as u64)
}

// ===== 動的 NodeId オフセット =====
//
// オーバーレイ内部の繰り返し要素（リスト項目）に割り当てる。
// タブ範囲 [1e9, 5.3e9] と衝突しないよう、すべて < 999_999_999 に収める。

/// コマンドパレット候補（`100_000_000 + idx`）
const NODE_ID_PALETTE_ITEM_OFFSET: u64 = 100_000_000;

/// ホスト一覧項目（`200_000_000 + idx`）
const NODE_ID_HOST_ITEM_OFFSET: u64 = 200_000_000;

/// マクロ一覧項目（`300_000_000 + idx`）
const NODE_ID_MACRO_ITEM_OFFSET: u64 = 300_000_000;

/// コンテキストメニュー項目（`400_000_000 + idx`）
const NODE_ID_CONTEXT_ITEM_OFFSET: u64 = 400_000_000;

/// Quick Select マッチ項目（`500_000_000 + idx`、Step 2-2-h）。
///
/// 600M〜999M は将来 SettingsField 動的展開（プロファイル一覧 / キーバインド一覧）用に予約済み。
/// Step 2-2-e' の現状実装は固定 NodeId のみで完結し動的範囲は使わない。
const NODE_ID_QUICKSELECT_ITEM_OFFSET: u64 = 500_000_000;

/// タブノードの NodeId 計算用オフセット。
///
/// 内部表現: `NODE_ID_TAB_OFFSET + pane_id as u64`。pane_id は u32 のため
/// 値域は `[1_000_000_000, 1_000_000_000 + u32::MAX] ≈ [1e9, 5.3e9]`。
/// `NODE_ID_PANE_OFFSET` との衝突がないことを保証する（差は 4e9 以上）。
const NODE_ID_TAB_OFFSET: u64 = 1_000_000_000;

/// ペインノードの NodeId 計算用オフセット。
///
/// 値域は `[10_000_000_000, 10_000_000_000 + u32::MAX] ≈ [1e10, 1.43e10]`。
const NODE_ID_PANE_OFFSET: u64 = 10_000_000_000;

/// ペイン行ノードの NodeId 計算用オフセット（Sprint 5-11-3）。
///
/// ペイン本体ノードの子として、ターミナルグリッドの各行を `Role::ContentInfo` で公開する。
/// 内部表現: `NODE_ID_PANE_ROW_OFFSET + pane_id as u64 * MAX_ROWS_PER_PANE + row as u64`。
///
/// 値域: `[2e10, 2e10 + u32::MAX * 1000 + 999] ≈ [2e10, 4.31e12]`。
/// `NODE_ID_PANE_OFFSET` の上限 ≈ 1.43e10 との間に十分なギャップがある。
const NODE_ID_PANE_ROW_OFFSET: u64 = 20_000_000_000;

/// 1 ペインあたりの最大行数（Sprint 5-11-3）。
///
/// 実用上のターミナル行数は 200 行程度。1000 行は十分な余裕を持たせた値。
/// この値を超える行は SR から不可視となるが、現実的な表示行数では発生しない。
pub const MAX_ROWS_PER_PANE: u64 = 1000;

/// pane_id（u32）からタブノードの NodeId を計算する。
pub fn tab_node_id(pane_id: u32) -> NodeId {
    NodeId(NODE_ID_TAB_OFFSET + pane_id as u64)
}

/// pane_id（u32）からペイン（ターミナル）ノードの NodeId を計算する。
pub fn pane_node_id(pane_id: u32) -> NodeId {
    NodeId(NODE_ID_PANE_OFFSET + pane_id as u64)
}

/// pane_id × row_idx からペイン行ノードの NodeId を計算する（Sprint 5-11-3）。
///
/// `row` が [`MAX_ROWS_PER_PANE`] 以上の場合は NodeId が衝突する可能性があるため、
/// 呼び出し側で `row < MAX_ROWS_PER_PANE` を保証すること。
pub fn pane_row_node_id(pane_id: u32, row: u16) -> NodeId {
    NodeId(NODE_ID_PANE_ROW_OFFSET + (pane_id as u64) * MAX_ROWS_PER_PANE + row as u64)
}

/// `Grid` の指定行を SR 向けテキストに変換する純関数（Sprint 5-11-3）。
///
/// 仕様:
/// - 各セルの `ch` を順次連結する（SGR / 色情報は捨てる、SR には不要）
/// - 末尾の半角空白は `trim_end()` で除去（SR が「半角空白 60 連続」を読み上げないため）
/// - 結果が空文字列なら `" "` を返す（SR が「空行」と認識する境界を保つ）
/// - `row` が範囲外なら `" "` を返す（panic 回避）
///
/// 全角文字（CJK・絵文字）は保持。`trim_end` は半角空白のみ除去するため、
/// 全角空白（U+3000）が連続している場合は保持される（意図的）。
pub fn pane_row_text(grid: &nexterm_proto::Grid, row: usize) -> String {
    let Some(cells) = grid.rows.get(row) else {
        return " ".to_string();
    };
    let mut text: String = cells.iter().map(|c| c.ch).collect();
    // 半角空白の末尾連続を除去（行右側のパディング除去）
    let trimmed = text.trim_end_matches(' ');
    if trimmed.is_empty() {
        " ".to_string()
    } else {
        text.truncate(trimmed.len());
        text
    }
}

/// 指定ペインの各行テキストハッシュを計算する（Sprint 5-11-3）。
///
/// `EventHandler::last_grid_row_hashes` のキャッシュ用。各行 [`pane_row_text`] 結果の
/// `DefaultHasher` ハッシュを `Vec<u64>` で返す。長さは `grid.height` と `grid.rows.len()` の最小値。
pub fn compute_grid_row_hashes(grid: &nexterm_proto::Grid) -> Vec<u64> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let row_count = (grid.height as usize).min(grid.rows.len());
    let mut hashes = Vec::with_capacity(row_count);
    for r in 0..row_count {
        let text = pane_row_text(grid, r);
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        hashes.push(h.finish());
    }
    hashes
}

/// パレット候補 idx から NodeId を計算する。
fn palette_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_PALETTE_ITEM_OFFSET + idx as u64)
}

/// ホスト一覧 idx から NodeId を計算する。
fn host_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_HOST_ITEM_OFFSET + idx as u64)
}

/// マクロ一覧 idx から NodeId を計算する。
fn macro_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_MACRO_ITEM_OFFSET + idx as u64)
}

/// コンテキストメニュー項目 idx から NodeId を計算する。
fn context_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_CONTEXT_ITEM_OFFSET + idx as u64)
}

/// Quick Select マッチ項目 idx から NodeId を計算する（Step 2-2-h）。
fn quickselect_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_QUICKSELECT_ITEM_OFFSET + idx as u64)
}

// ===== NodeId 逆引き（Step 2-4）=====

/// `NodeId` の種別（Action 応答のディスパッチに使用）。
///
/// プラットフォーム a11y アダプタから受け取った `ActionRequest::target_node` を
/// `decode_node_id` で本 enum に変換し、種別に応じて Focus / Click / SetValue を処理する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeIdKind {
    /// ルート（OS Window 全体）
    Root,
    /// タブバー（`TabList`）
    TabBar,
    /// ペイン領域（`Group`）
    PaneArea,
    /// 設定パネルのルート
    SettingsPanel,
    /// コマンドパレットのルート
    Palette,
    /// ホストマネージャのルート
    HostManager,
    /// マクロピッカーのルート
    MacroPicker,
    /// コンテキストメニューのルート
    ContextMenu,
    /// 閉じる確認ダイアログのルート
    CloseDialog,
    /// 更新通知バナー
    UpdateBanner,
    /// Quick Select オーバーレイのルート
    QuickSelect,
    /// パレットの検索入力欄
    PaletteSearch,
    /// パレットの候補リスト（ListBox）
    PaletteList,
    /// 閉じる確認ダイアログの「Kill」ボタン
    CloseDialogKill,
    /// 閉じる確認ダイアログの「Cancel」ボタン
    CloseDialogCancel,
    /// Quick Select のマッチリスト（ListBox）
    QuickSelectList,
    /// タブノード（`pane_id` で識別）
    Tab { pane_id: u32 },
    /// ペインノード（`pane_id` で識別）
    Pane { pane_id: u32 },
    /// パレット候補項目（`filtered()` 上の `idx`）
    PaletteItem { idx: usize },
    /// ホスト一覧項目（`filtered()` 上の `idx`）
    HostItem { idx: usize },
    /// マクロ一覧項目（`filtered()` 上の `idx`）
    MacroItem { idx: usize },
    /// コンテキストメニュー項目（`items` 上の `idx`）
    ContextItem { idx: usize },
    /// Quick Select マッチ項目（`matches` 上の `idx`）
    QuickSelectItem { idx: usize },
    /// 設定パネル: カテゴリ TabList
    SettingsTabList,
    /// 設定パネル: 各カテゴリタブ（`SettingsCategory::ALL` の `idx`）
    SettingsTab { idx: usize },
    /// 設定パネル: 現在カテゴリ内容のコンテナ
    SettingsContent,
    /// 設定パネル: フォントファミリー入力欄
    SettingsFontFamily,
    /// 設定パネル: フォントサイズスライダー
    SettingsFontSize,
    /// 設定パネル: カラースキーム選択
    SettingsThemeScheme,
    /// 設定パネル: 不透明度スライダー
    SettingsWindowOpacity,
    /// 設定パネル: 言語選択
    SettingsStartupLanguage,
    /// 設定パネル: 起動時更新確認 CheckBox
    SettingsStartupAutoUpdate,
    /// ペイン行ノード（Sprint 5-11-3、`pane_id` と `row` で識別）
    PaneRow { pane_id: u32, row: u16 },
    /// 未知 / 範囲外の NodeId
    Unknown,
}

/// `NodeId` から `NodeIdKind` を逆引きする（Step 2-4）。
///
/// オフセット範囲表（`accessibility.rs` 冒頭の定数と整合）:
///
/// | 範囲 | 種別 |
/// |---|---|
/// | 1〜16 | 固定ノード（基本 + オーバーレイルート） |
/// | 17 | `SettingsTabList` |
/// | 18〜24 | `SettingsTab { idx: id - 18 }` |
/// | 25 | `SettingsContent` |
/// | 30〜35 | 設定フィールド（FontFamily / FontSize / ThemeScheme / WindowOpacity / StartupLanguage / StartupAutoUpdate） |
/// | 26〜29, 36〜99 | 予約 |
/// | 100M..200M | `PaletteItem { idx: id - 100M }` |
/// | 200M..300M | `HostItem { idx: id - 200M }` |
/// | 300M..400M | `MacroItem { idx: id - 300M }` |
/// | 400M..500M | `ContextItem { idx: id - 400M }` |
/// | 500M..600M | `QuickSelectItem { idx: id - 500M }` |
/// | 600M..1G | 予約（SettingsField 動的展開用） |
/// | 1G..1G+u32::MAX | `Tab { pane_id: id - 1G }` |
/// | 10G..10G+u32::MAX | `Pane { pane_id: id - 10G }` |
/// | 20G..~4.31T | `PaneRow { pane_id, row }`（Sprint 5-11-3） |
/// | その他 | `Unknown` |
pub fn decode_node_id(id: NodeId) -> NodeIdKind {
    let raw = id.0;
    match raw {
        1 => NodeIdKind::Root,
        2 => NodeIdKind::TabBar,
        3 => NodeIdKind::PaneArea,
        4 => NodeIdKind::SettingsPanel,
        5 => NodeIdKind::Palette,
        6 => NodeIdKind::HostManager,
        7 => NodeIdKind::MacroPicker,
        8 => NodeIdKind::ContextMenu,
        9 => NodeIdKind::CloseDialog,
        10 => NodeIdKind::UpdateBanner,
        11 => NodeIdKind::QuickSelect,
        12 => NodeIdKind::PaletteSearch,
        13 => NodeIdKind::PaletteList,
        14 => NodeIdKind::CloseDialogKill,
        15 => NodeIdKind::CloseDialogCancel,
        16 => NodeIdKind::QuickSelectList,
        17 => NodeIdKind::SettingsTabList,
        18..=24 => NodeIdKind::SettingsTab {
            idx: (raw - SETTINGS_TAB_BASE) as usize,
        },
        25 => NodeIdKind::SettingsContent,
        30 => NodeIdKind::SettingsFontFamily,
        31 => NodeIdKind::SettingsFontSize,
        32 => NodeIdKind::SettingsThemeScheme,
        33 => NodeIdKind::SettingsWindowOpacity,
        34 => NodeIdKind::SettingsStartupLanguage,
        35 => NodeIdKind::SettingsStartupAutoUpdate,
        _ => decode_dynamic(raw),
    }
}

/// 動的オフセット範囲の判定（`decode_node_id` の補助）。
fn decode_dynamic(raw: u64) -> NodeIdKind {
    // 各動的オフセットレンジ幅。次オフセットまでの差分で計算する。
    const DYN_RANGE: u64 = 100_000_000;

    if (NODE_ID_PALETTE_ITEM_OFFSET..NODE_ID_PALETTE_ITEM_OFFSET + DYN_RANGE).contains(&raw) {
        return NodeIdKind::PaletteItem {
            idx: (raw - NODE_ID_PALETTE_ITEM_OFFSET) as usize,
        };
    }
    if (NODE_ID_HOST_ITEM_OFFSET..NODE_ID_HOST_ITEM_OFFSET + DYN_RANGE).contains(&raw) {
        return NodeIdKind::HostItem {
            idx: (raw - NODE_ID_HOST_ITEM_OFFSET) as usize,
        };
    }
    if (NODE_ID_MACRO_ITEM_OFFSET..NODE_ID_MACRO_ITEM_OFFSET + DYN_RANGE).contains(&raw) {
        return NodeIdKind::MacroItem {
            idx: (raw - NODE_ID_MACRO_ITEM_OFFSET) as usize,
        };
    }
    if (NODE_ID_CONTEXT_ITEM_OFFSET..NODE_ID_CONTEXT_ITEM_OFFSET + DYN_RANGE).contains(&raw) {
        return NodeIdKind::ContextItem {
            idx: (raw - NODE_ID_CONTEXT_ITEM_OFFSET) as usize,
        };
    }
    if (NODE_ID_QUICKSELECT_ITEM_OFFSET..NODE_ID_QUICKSELECT_ITEM_OFFSET + DYN_RANGE).contains(&raw)
    {
        return NodeIdKind::QuickSelectItem {
            idx: (raw - NODE_ID_QUICKSELECT_ITEM_OFFSET) as usize,
        };
    }
    // タブ範囲: [1e9, 1e9 + u32::MAX] = [1e9, 1e9 + ~4.29e9] ≈ [1e9, 5.3e9]
    if (NODE_ID_TAB_OFFSET..NODE_ID_TAB_OFFSET + (u32::MAX as u64) + 1).contains(&raw) {
        return NodeIdKind::Tab {
            pane_id: (raw - NODE_ID_TAB_OFFSET) as u32,
        };
    }
    // ペイン範囲: [1e10, 1e10 + u32::MAX]
    if (NODE_ID_PANE_OFFSET..NODE_ID_PANE_OFFSET + (u32::MAX as u64) + 1).contains(&raw) {
        return NodeIdKind::Pane {
            pane_id: (raw - NODE_ID_PANE_OFFSET) as u32,
        };
    }
    // ペイン行範囲（Sprint 5-11-3）: [2e10, 2e10 + u32::MAX * MAX_ROWS_PER_PANE + (MAX_ROWS_PER_PANE - 1)]
    let pane_row_range_end =
        NODE_ID_PANE_ROW_OFFSET + (u32::MAX as u64) * MAX_ROWS_PER_PANE + MAX_ROWS_PER_PANE;
    if (NODE_ID_PANE_ROW_OFFSET..pane_row_range_end).contains(&raw) {
        let normalized = raw - NODE_ID_PANE_ROW_OFFSET;
        return NodeIdKind::PaneRow {
            pane_id: (normalized / MAX_ROWS_PER_PANE) as u32,
            row: (normalized % MAX_ROWS_PER_PANE) as u16,
        };
    }
    NodeIdKind::Unknown
}

/// `ClientState` から AccessKit ツリーを構築する。
///
/// ## 構造
///
/// **基本（タブ・ペイン）:**
/// ```text
/// Window "Nexterm"
///   ├─ TabList "ターミナルタブ"
///   │    ├─ Tab "タブ 1: <title>"  (selected if focused)
///   │    └─ Tab ...
///   └─ Group "ペイン"
///        ├─ Terminal "<title>"  (description: 作業ディレクトリ: <cwd>)
///        └─ Terminal ...
/// ```
///
/// **オーバーレイ表示時（最前面 1 つを追加 + フォーカス移動）:**
/// 優先順位（高 → 低）:
/// 1. `CloseWindowDialog` (AlertDialog, モーダル)
/// 2. `ContextMenu` (Menu, モーダル)
/// 3. `CommandPalette` (Dialog with SearchInput + ListBox)
/// 4. `HostManager` (Dialog with ListBox)
/// 5. `MacroPicker` (Dialog with ListBox)
/// 6. `SettingsPanel` (Dialog, 詳細実装は Step 2-2-e で展開)
///
/// **非モーダル**:
/// - `update_banner`: `Role::Alert`。フォーカスは取らないが ROOT の child に追加されて読み上げ可能になる
///
/// ## フォーカス
///
/// - オーバーレイ表示中: そのオーバーレイ内の選択中項目（または検索入力）にフォーカス
/// - オーバーレイなし: `state.focused_pane_id` のペインノード（未設定なら ROOT）
pub fn build_tree_from_state(state: &ClientState) -> TreeUpdate {
    // ===== 基本ノード（タブ・ペイン）を構築 =====
    let (mut nodes, mut root_children, default_focus) = build_base_nodes(state);

    let mut focus = default_focus;

    // ===== オーバーレイを優先順位順にチェック =====
    // 一度に表示されるのは 1 つ。最も優先度が高いものだけを追加する。
    //
    // 優先順位 (高 → 低):
    //   1. CloseWindowDialog (AlertDialog, 最強モーダル)
    //   2. QuickSelect (ラベルキーが他のキー入力を全消費するため最モーダル相当)
    //   3. ContextMenu
    //   4. CommandPalette
    //   5. HostManager
    //   6. MacroPicker
    //   7. SettingsPanel
    if let Some(dialog) = &state.close_window_dialog {
        let (overlay_nodes, overlay_focus) = build_close_dialog_nodes(dialog);
        nodes.extend(overlay_nodes);
        root_children.push(CLOSE_DIALOG_ID);
        focus = overlay_focus;
    } else if state.quick_select.is_active {
        let (overlay_nodes, overlay_focus) = build_quick_select_nodes(&state.quick_select);
        nodes.extend(overlay_nodes);
        root_children.push(QUICK_SELECT_ID);
        focus = overlay_focus;
    } else if let Some(menu) = &state.context_menu {
        let (overlay_nodes, overlay_focus) = build_context_menu_nodes(menu);
        nodes.extend(overlay_nodes);
        root_children.push(CONTEXT_MENU_ID);
        focus = overlay_focus;
    } else if state.palette.is_open {
        let (overlay_nodes, overlay_focus) = build_palette_nodes(&state.palette);
        nodes.extend(overlay_nodes);
        root_children.push(PALETTE_ID);
        focus = overlay_focus;
    } else if state.host_manager.is_open {
        let (overlay_nodes, overlay_focus) = build_host_manager_nodes(&state.host_manager);
        nodes.extend(overlay_nodes);
        root_children.push(HOST_MANAGER_ID);
        focus = overlay_focus;
    } else if state.macro_picker.is_open {
        let (overlay_nodes, overlay_focus) = build_macro_picker_nodes(&state.macro_picker);
        nodes.extend(overlay_nodes);
        root_children.push(MACRO_PICKER_ID);
        focus = overlay_focus;
    } else if state.settings_panel.is_open {
        let (overlay_nodes, overlay_focus) = build_settings_panel_nodes(&state.settings_panel);
        nodes.extend(overlay_nodes);
        root_children.push(SETTINGS_PANEL_ID);
        focus = overlay_focus;
    }

    // ===== 非モーダル: 更新バナー =====
    if let Some(version) = &state.update_banner {
        nodes.push(build_update_banner_node(version));
        root_children.push(UPDATE_BANNER_ID);
    }

    // ===== ROOT ノードを最終 children で確定 =====
    // build_base_nodes が tentative な ROOT を入れているので、ここで子要素を上書きする。
    let mut root = Node::new(Role::Window);
    root.set_label("Nexterm");
    root.set_children(root_children);
    nodes[0] = (ROOT_ID, root);

    let mut tree = Tree::new(ROOT_ID);
    tree.toolkit_name = Some(env!("CARGO_PKG_NAME").into());
    tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());

    TreeUpdate {
        nodes,
        tree: Some(tree),
        tree_id: TreeId::ROOT,
        focus,
    }
}

/// タブ・ペインまでの基本ノードを構築する。
///
/// 戻り値:
/// - `nodes`: ROOT (tentative) / TAB_BAR / PANE_AREA + 各タブ + 各ペインノード
/// - `root_children`: 暫定の ROOT 子要素（[TAB_BAR_ID, PANE_AREA_ID]）。
///   オーバーレイがある場合は呼び出し側が追加して上書きする。
/// - `focus`: オーバーレイなしの場合のデフォルトフォーカス
fn build_base_nodes(state: &ClientState) -> (Vec<(NodeId, Node)>, Vec<NodeId>, NodeId) {
    // タブ順序を決定（tab_order が空ならフォールバック）
    let tab_order: Vec<u32> = if state.tab_order.is_empty() {
        state.panes.keys().copied().collect()
    } else {
        state.tab_order.clone()
    };

    // ===== ROOT ノード（tentative） =====
    // 最終的な children はオーバーレイ判定後に build_tree_from_state が再構築する
    let mut root = Node::new(Role::Window);
    root.set_label("Nexterm");
    root.set_children(vec![TAB_BAR_ID, PANE_AREA_ID]);

    // ===== TAB_BAR ノード =====
    let mut tab_bar = Node::new(Role::TabList);
    tab_bar.set_label("ターミナルタブ");
    let tab_child_ids: Vec<NodeId> = tab_order.iter().copied().map(tab_node_id).collect();
    tab_bar.set_children(tab_child_ids);

    // ===== 各タブノード =====
    let mut tab_nodes: Vec<(NodeId, Node)> = Vec::with_capacity(tab_order.len());
    for (idx, &pane_id) in tab_order.iter().enumerate() {
        let title = state
            .panes
            .get(&pane_id)
            .map(|p| p.title.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Untitled");
        let label = format!("タブ {}: {}", idx + 1, title);
        let mut tab = Node::new(Role::Tab);
        tab.set_label(label);
        if state.focused_pane_id == Some(pane_id) {
            tab.set_selected(true);
        }
        tab_nodes.push((tab_node_id(pane_id), tab));
    }

    // ===== PANE_AREA ノード =====
    let mut pane_area = Node::new(Role::Group);
    pane_area.set_label("ペイン");
    let pane_child_ids: Vec<NodeId> = tab_order.iter().copied().map(pane_node_id).collect();
    pane_area.set_children(pane_child_ids);

    // ===== 各ペインノード + ペイン行ノード（Sprint 5-11-3） =====
    //
    // ペインの子として `Role::ContentInfo` の行ノードを並べる。SR ユーザーは矢印キーで
    // 行間を移動し、各行のテキストを順次読み上げられる。フォーカスペインの行ノードは
    // `Live::Polite` を設定して、出力差分が SR にアナウンスされるようにする。
    let mut pane_nodes: Vec<(NodeId, Node)> = Vec::with_capacity(state.panes.len());
    for &pane_id in &tab_order {
        let Some(pane) = state.panes.get(&pane_id) else {
            continue;
        };
        let title = if pane.title.is_empty() {
            format!("Pane {}", pane_id)
        } else {
            pane.title.clone()
        };
        let is_focused_pane = state.focused_pane_id == Some(pane_id);

        // 行ノードを生成し、ペインノードの子 ID リストに積む。
        // NodeId 衝突を避けるため `row < MAX_ROWS_PER_PANE` でクランプする。
        let row_count = (pane.grid.height as u64)
            .min(pane.grid.rows.len() as u64)
            .min(MAX_ROWS_PER_PANE) as u16;
        let mut row_child_ids: Vec<NodeId> = Vec::with_capacity(row_count as usize);
        for row in 0..row_count {
            let text = pane_row_text(&pane.grid, row as usize);
            let mut row_node = Node::new(Role::ContentInfo);
            row_node.set_value(text);
            if is_focused_pane {
                // フォーカスペインのみ Live::Polite。SR は出力差分を他読み上げ後にアナウンスする。
                row_node.set_live(Live::Polite);
            }
            let row_id = pane_row_node_id(pane_id, row);
            row_child_ids.push(row_id);
            pane_nodes.push((row_id, row_node));
        }

        let mut pane_node = Node::new(Role::Terminal);
        pane_node.set_label(title);
        if let Some(cwd) = &pane.cwd {
            pane_node.set_description(format!("作業ディレクトリ: {}", cwd));
        }
        pane_node.set_children(row_child_ids);
        pane_nodes.push((pane_node_id(pane_id), pane_node));
    }

    let default_focus = state.focused_pane_id.map_or(ROOT_ID, pane_node_id);

    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(3 + tab_nodes.len() + pane_nodes.len());
    nodes.push((ROOT_ID, root));
    nodes.push((TAB_BAR_ID, tab_bar));
    nodes.push((PANE_AREA_ID, pane_area));
    nodes.extend(tab_nodes);
    nodes.extend(pane_nodes);

    (nodes, vec![TAB_BAR_ID, PANE_AREA_ID], default_focus)
}

// ===== オーバーレイノードビルダー（Step 2-2-b〜g） =====

/// CommandPalette のノード群を構築する（Step 2-2-b）。
///
/// 構造:
/// ```text
/// Dialog "コマンドパレット"
///   ├─ SearchInput "検索" (value: query)
///   └─ ListBox "候補"
///        ├─ ListBoxOption "<label>"  (selected if idx == palette.selected)
///        └─ ...
/// ```
///
/// フォーカス: 候補が 1 つ以上あれば選択中候補、なければ検索入力欄
fn build_palette_nodes(palette: &CommandPalette) -> (Vec<(NodeId, Node)>, NodeId) {
    let filtered = palette.filtered();
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(3 + filtered.len());

    // ===== Dialog ルート =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("コマンドパレット");
    dialog.set_modal();
    dialog.set_children(vec![PALETTE_SEARCH_ID, PALETTE_LIST_ID]);
    nodes.push((PALETTE_ID, dialog));

    // ===== SearchInput =====
    let mut search = Node::new(Role::SearchInput);
    search.set_label("検索");
    search.set_value(palette.query.clone());
    nodes.push((PALETTE_SEARCH_ID, search));

    // ===== ListBox =====
    let mut list = Node::new(Role::ListBox);
    list.set_label(format!("候補 {} 件", filtered.len()));
    let item_ids: Vec<NodeId> = (0..filtered.len()).map(palette_item_id).collect();
    list.set_children(item_ids);
    nodes.push((PALETTE_LIST_ID, list));

    // ===== 各候補項目 =====
    for (idx, action) in filtered.iter().enumerate() {
        let mut item = Node::new(Role::ListBoxOption);
        item.set_label(action.label.clone());
        if idx == palette.selected {
            item.set_selected(true);
        }
        nodes.push((palette_item_id(idx), item));
    }

    // フォーカス: 選択中候補（候補ありなら）または検索入力
    let focus = if filtered.is_empty() || palette.selected >= filtered.len() {
        PALETTE_SEARCH_ID
    } else {
        palette_item_id(palette.selected)
    };

    (nodes, focus)
}

/// ContextMenu のノード群を構築する（Step 2-2-c）。
///
/// 構造:
/// ```text
/// Menu (no label, ItemList で position 0)
///   ├─ MenuItem "<label>" (description: hint, focused if hovered)
///   ├─ Splitter (separator)
///   └─ ...
/// ```
///
/// フォーカス: hover 中項目、なければメニュー自身
fn build_context_menu_nodes(menu: &ContextMenu) -> (Vec<(NodeId, Node)>, NodeId) {
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(1 + menu.items.len());

    // ===== Menu ルート =====
    let mut menu_node = Node::new(Role::Menu);
    menu_node.set_label("コンテキストメニュー");
    let item_ids: Vec<NodeId> = (0..menu.items.len()).map(context_item_id).collect();
    menu_node.set_children(item_ids);
    nodes.push((CONTEXT_MENU_ID, menu_node));

    // ===== 各メニュー項目 =====
    for (idx, item) in menu.items.iter().enumerate() {
        let role = if matches!(item.action, crate::state::ContextMenuAction::Separator) {
            Role::Splitter
        } else {
            Role::MenuItem
        };
        let mut node = Node::new(role);
        if !item.label.is_empty() {
            node.set_label(item.label.clone());
        }
        if !item.hint.is_empty() {
            // キーバインド ヒントを description にする（SR で「Ctrl+C」等が補足読み上げされる）
            node.set_description(item.hint.clone());
        }
        nodes.push((context_item_id(idx), node));
    }

    // フォーカス: hover 中項目、なければメニュー自身
    let focus = menu
        .hovered
        .filter(|&idx| idx < menu.items.len())
        .map(context_item_id)
        .unwrap_or(CONTEXT_MENU_ID);

    (nodes, focus)
}

/// CloseWindowDialog のノード群を構築する（Step 2-2-d）。
///
/// 構造:
/// ```text
/// AlertDialog "Window を閉じますか？" (modal)
///   ├─ Label <message>  (Paragraph として組み込み)
///   ├─ Button <kill_label>  (selected if selected_button == 0)
///   └─ Button <cancel_label>  (selected if selected_button == 1)
/// ```
///
/// フォーカス: selected_button が示すボタン
fn build_close_dialog_nodes(dialog: &CloseWindowDialog) -> (Vec<(NodeId, Node)>, NodeId) {
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(3);

    // ===== AlertDialog ルート =====
    let mut alert = Node::new(Role::AlertDialog);
    alert.set_label("Window を閉じますか？");
    // メッセージ本文を description として埋め込む（SR がダイアログ概要として読み上げる）
    alert.set_description(dialog.message.clone());
    alert.set_modal();
    alert.set_children(vec![CLOSE_DIALOG_KILL_BTN, CLOSE_DIALOG_CANCEL_BTN]);
    nodes.push((CLOSE_DIALOG_ID, alert));

    // ===== Kill (プロセス終了 / 強制クローズ) ボタン =====
    let mut kill_btn = Node::new(Role::Button);
    kill_btn.set_label(dialog.kill_label.clone());
    if dialog.selected_button == 0 {
        kill_btn.set_selected(true);
    }
    nodes.push((CLOSE_DIALOG_KILL_BTN, kill_btn));

    // ===== Cancel ボタン =====
    let mut cancel_btn = Node::new(Role::Button);
    cancel_btn.set_label(dialog.cancel_label.clone());
    if dialog.selected_button == 1 {
        cancel_btn.set_selected(true);
    }
    nodes.push((CLOSE_DIALOG_CANCEL_BTN, cancel_btn));

    let focus = match dialog.selected_button {
        0 => CLOSE_DIALOG_KILL_BTN,
        1 => CLOSE_DIALOG_CANCEL_BTN,
        // 確定済み (0xFE / 0xFF) は描画タイミングの edge case。Kill にフォーカスを当てる
        _ => CLOSE_DIALOG_KILL_BTN,
    };

    (nodes, focus)
}

/// HostManager のノード群を構築する（Step 2-2-f）。
fn build_host_manager_nodes(manager: &HostManager) -> (Vec<(NodeId, Node)>, NodeId) {
    let filtered = manager.filtered();
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(1 + filtered.len());

    // ===== Dialog ルート =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("SSH ホストマネージャ");
    dialog.set_modal();
    let item_ids: Vec<NodeId> = (0..filtered.len()).map(host_item_id).collect();
    dialog.set_children(item_ids);
    nodes.push((HOST_MANAGER_ID, dialog));

    // ===== 各ホスト項目 =====
    for (idx, host) in filtered.iter().enumerate() {
        let mut item = Node::new(Role::ListBoxOption);
        let label = if host.name.is_empty() {
            format!("{}@{}", host.username, host.host)
        } else {
            host.name.clone()
        };
        item.set_label(label);
        // ホスト名・ユーザー名を description で補足
        let desc = format!(
            "ホスト: {}, ユーザー: {}, ポート: {}",
            host.host, host.username, host.port
        );
        item.set_description(desc);
        if idx == manager.selected {
            item.set_selected(true);
        }
        nodes.push((host_item_id(idx), item));
    }

    let focus = if filtered.is_empty() || manager.selected >= filtered.len() {
        HOST_MANAGER_ID
    } else {
        host_item_id(manager.selected)
    };

    (nodes, focus)
}

/// Quick Select のノード群を構築する（Step 2-2-h）。
///
/// 構造:
/// ```text
/// Dialog "Quick Select" (modal)
///   ├─ description: "ラベル入力中: '<typed_label>'" (空なら「ラベルキーで項目を選択」)
///   └─ ListBox "マッチ {n} 件" (id=16)
///        ├─ ListBoxOption "[a] <text>"  (selected if matches[idx].label.starts_with(typed_label))
///        └─ ...
/// ```
///
/// **フォーカス戦略**:
/// - `typed_label` が prefix で 1 件以上に絞られているなら最初の prefix 一致項目
/// - そうでない場合: マッチがあれば最初の項目、なければ ListBox 自身
///
/// **設計メモ**:
/// - 検索入力欄を別ノードにしない理由: Quick Select はキー押下ごとに即座に確定する
///   UX なので、AccessKit の `SearchInput` モデルに合わない。`typed_label` は Dialog の
///   `description` として補足する（SR がダイアログ状態として読み上げる）
fn build_quick_select_nodes(qs: &QuickSelectState) -> (Vec<(NodeId, Node)>, NodeId) {
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(2 + qs.matches.len());

    // ===== Dialog ルート =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("Quick Select");
    dialog.set_modal();
    let desc = if qs.typed_label.is_empty() {
        "ラベルキーで項目を選択してクリップボードにコピー".to_string()
    } else {
        format!("ラベル入力中: '{}'", qs.typed_label)
    };
    dialog.set_description(desc);
    dialog.set_children(vec![QUICK_SELECT_LIST_ID]);
    nodes.push((QUICK_SELECT_ID, dialog));

    // ===== ListBox =====
    let mut list = Node::new(Role::ListBox);
    list.set_label(format!("マッチ {} 件", qs.matches.len()));
    let item_ids: Vec<NodeId> = (0..qs.matches.len()).map(quickselect_item_id).collect();
    list.set_children(item_ids);
    nodes.push((QUICK_SELECT_LIST_ID, list));

    // ===== 各マッチ項目 =====
    // typed_label が prefix で一致する最初の項目をフォーカス候補にする
    let mut focus_idx: Option<usize> = None;
    for (idx, m) in qs.matches.iter().enumerate() {
        let mut item = Node::new(Role::ListBoxOption);
        item.set_label(format!("[{}] {}", m.label, m.text));
        if !qs.typed_label.is_empty() && m.label.starts_with(&qs.typed_label) {
            item.set_selected(true);
            if focus_idx.is_none() {
                focus_idx = Some(idx);
            }
        }
        nodes.push((quickselect_item_id(idx), item));
    }

    // フォーカス: prefix 一致項目 → 最初のマッチ → ListBox 自身（マッチなし時）
    let focus = match focus_idx {
        Some(idx) => quickselect_item_id(idx),
        None if !qs.matches.is_empty() => quickselect_item_id(0),
        None => QUICK_SELECT_LIST_ID,
    };

    (nodes, focus)
}

/// MacroPicker のノード群を構築する（Step 2-2-f）。
fn build_macro_picker_nodes(picker: &MacroPicker) -> (Vec<(NodeId, Node)>, NodeId) {
    let filtered = picker.filtered();
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(1 + filtered.len());

    // ===== Dialog ルート =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("Lua マクロピッカー");
    dialog.set_modal();
    let item_ids: Vec<NodeId> = (0..filtered.len()).map(macro_item_id).collect();
    dialog.set_children(item_ids);
    nodes.push((MACRO_PICKER_ID, dialog));

    // ===== 各マクロ項目 =====
    for (idx, mac) in filtered.iter().enumerate() {
        let mut item = Node::new(Role::ListBoxOption);
        item.set_label(mac.name.clone());
        if !mac.description.is_empty() {
            item.set_description(mac.description.clone());
        }
        if idx == picker.selected {
            item.set_selected(true);
        }
        nodes.push((macro_item_id(idx), item));
    }

    let focus = if filtered.is_empty() || picker.selected >= filtered.len() {
        MACRO_PICKER_ID
    } else {
        macro_item_id(picker.selected)
    };

    (nodes, focus)
}

/// SettingsPanel のノード群を構築する（Step 2-2-e'、TabList + 各カテゴリ詳細フィールド）。
///
/// ## ツリー構造
///
/// ```text
/// Dialog "設定"
///   ├─ TabList "カテゴリ"
///   │    ├─ Tab "スタートアップ"
///   │    ├─ Tab "フォント"  (selected if category == Font)
///   │    ├─ Tab "テーマ"
///   │    ├─ Tab "ウィンドウ"
///   │    ├─ Tab "SSH"
///   │    ├─ Tab "キーバインド"
///   │    └─ Tab "プロファイル"
///   └─ Group "<現在カテゴリ名>"
///        ├─ TextInput "フォントファミリー" (Font カテゴリのみ)
///        ├─ Slider "フォントサイズ" with numeric_value (Font カテゴリのみ)
///        ├─ ComboBox "カラースキーム" (Theme カテゴリのみ)
///        ├─ Slider "不透明度" (Window カテゴリのみ)
///        ├─ ComboBox "言語" (Startup カテゴリのみ)
///        └─ CheckBox "起動時に更新確認" (Startup カテゴリのみ)
/// ```
///
/// SSH / Keybindings / Profiles カテゴリは現状 Group の description のみ
/// （詳細フィールドは将来 600M〜オフセットで動的展開予定）。
///
/// フォーカス: font_family_editing 中はそのフィールド、それ以外は現在カテゴリのタブ。
fn build_settings_panel_nodes(panel: &SettingsPanel) -> (Vec<(NodeId, Node)>, NodeId) {
    use crate::settings_panel::SettingsCategory;

    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(16);

    let current_idx = SettingsCategory::ALL
        .iter()
        .position(|c| c == &panel.category)
        .unwrap_or(0);

    // ===== Dialog (ルート) =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("設定");
    dialog.set_modal();
    dialog.set_description(format!("カテゴリ: {}", panel.category.label()));
    dialog.set_children(vec![SETTINGS_TABLIST_ID, SETTINGS_CONTENT_ID]);
    nodes.push((SETTINGS_PANEL_ID, dialog));

    // ===== TabList (カテゴリタブ) =====
    let tab_ids: Vec<NodeId> = (0..SettingsCategory::ALL.len())
        .map(settings_tab_id_at)
        .collect();
    let mut tablist = Node::new(Role::TabList);
    tablist.set_label("カテゴリ");
    tablist.set_children(tab_ids);
    nodes.push((SETTINGS_TABLIST_ID, tablist));

    for (idx, cat) in SettingsCategory::ALL.iter().enumerate() {
        let mut tab = Node::new(Role::Tab);
        tab.set_label(cat.label());
        if idx == current_idx {
            tab.set_selected(true);
        }
        nodes.push((settings_tab_id_at(idx), tab));
    }

    // ===== Content Group (現在カテゴリのフィールド) =====
    let mut content_children: Vec<NodeId> = Vec::new();

    match panel.category {
        SettingsCategory::Font => {
            let mut family = Node::new(Role::TextInput);
            family.set_label("フォントファミリー");
            family.set_value(panel.font_family.as_str());
            if panel.font_family_editing {
                family.set_description("編集中（Tab で確定）");
            }
            nodes.push((SETTINGS_FONT_FAMILY_ID, family));
            content_children.push(SETTINGS_FONT_FAMILY_ID);

            let mut size = Node::new(Role::Slider);
            size.set_label("フォントサイズ");
            size.set_value(format!("{:.1}", panel.font_size));
            size.set_numeric_value(panel.font_size as f64);
            size.set_min_numeric_value(8.0);
            size.set_max_numeric_value(32.0);
            size.set_numeric_value_step(0.5);
            nodes.push((SETTINGS_FONT_SIZE_ID, size));
            content_children.push(SETTINGS_FONT_SIZE_ID);
        }
        SettingsCategory::Theme => {
            let mut scheme = Node::new(Role::ComboBox);
            scheme.set_label("カラースキーム");
            scheme.set_value(panel.scheme_name());
            scheme.set_description("←/→ で切り替え");
            nodes.push((SETTINGS_THEME_SCHEME_ID, scheme));
            content_children.push(SETTINGS_THEME_SCHEME_ID);
        }
        SettingsCategory::Window => {
            let mut opacity = Node::new(Role::Slider);
            opacity.set_label("背景不透明度");
            opacity.set_value(format!("{:.0}%", panel.opacity * 100.0));
            opacity.set_numeric_value(panel.opacity as f64);
            opacity.set_min_numeric_value(0.1);
            opacity.set_max_numeric_value(1.0);
            opacity.set_numeric_value_step(0.05);
            nodes.push((SETTINGS_WINDOW_OPACITY_ID, opacity));
            content_children.push(SETTINGS_WINDOW_OPACITY_ID);
        }
        SettingsCategory::Startup => {
            let mut lang = Node::new(Role::ComboBox);
            lang.set_label("言語");
            lang.set_value(panel.language_code());
            lang.set_description("←/→ で切り替え");
            nodes.push((SETTINGS_STARTUP_LANGUAGE_ID, lang));
            content_children.push(SETTINGS_STARTUP_LANGUAGE_ID);

            let mut auto_update = Node::new(Role::CheckBox);
            auto_update.set_label("起動時に更新を確認する");
            auto_update.set_toggled(if panel.auto_check_update {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
            nodes.push((SETTINGS_STARTUP_AUTO_UPDATE_ID, auto_update));
            content_children.push(SETTINGS_STARTUP_AUTO_UPDATE_ID);
        }
        SettingsCategory::Ssh | SettingsCategory::Keybindings | SettingsCategory::Profiles => {
            // 詳細フィールドは将来実装（600M〜オフセット動的展開予定）
        }
    }

    let mut content = Node::new(Role::Group);
    content.set_label(panel.category.label());
    if content_children.is_empty() {
        content.set_description("このカテゴリの詳細はまだ実装されていません");
    }
    content.set_children(content_children);
    nodes.push((SETTINGS_CONTENT_ID, content));

    // ===== フォーカス決定 =====
    let focus = if matches!(panel.category, SettingsCategory::Font) && panel.font_family_editing {
        SETTINGS_FONT_FAMILY_ID
    } else {
        settings_tab_id_at(current_idx)
    };

    (nodes, focus)
}

/// 更新通知バナーのノードを構築する（Step 2-2-g）。
fn build_update_banner_node(version: &str) -> (NodeId, Node) {
    let mut alert = Node::new(Role::Alert);
    alert.set_label(format!("新しいバージョンが利用可能です: {}", version));
    (UPDATE_BANNER_ID, alert)
}

// ===== Step 2-5: ライブ更新用ステートハッシュ =====

/// `build_tree_from_state` が読み取る `ClientState` の各フィールドをハッシュ化する。
///
/// **設計方針**:
/// - `build_tree_from_state` 内で参照される **全フィールド** を反映する（過不足あると
///   SR が古い情報のまま停まる/逆に過剰更新で重くなる）
/// - `filtered()` 系メソッドは呼ばない（毎回 alloc + sort で重い）。代わりに `query` /
///   `selected` / `is_open` などの **入力側** をハッシュする。`actions` / `hosts` /
///   `macros` の中身自体は実行中ほぼ変化しないため、入力ハッシュで十分検知できる。
/// - `panes` の反復順序はタブ順序を尊重して決定論的にする（`HashMap` のままだと
///   毎回ハッシュが変動する）。
///
/// **コスト**: O(panes + overlay 項目数)。100ms スロットリングと組み合わせて使うこと前提。
pub fn compute_tree_state_hash(state: &ClientState) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut h = DefaultHasher::new();

    // === 基本（タブ・ペイン） ===
    state.tab_order.hash(&mut h);
    state.focused_pane_id.hash(&mut h);

    // panes はタブ順序を尊重して反復（HashMap の非決定的順序回避）。
    // tab_order が空の場合のフォールバックは `build_base_nodes` と同じく `panes.keys()` だが、
    // ハッシュの安定性を優先して **ソートしてから** 反復する。
    if state.tab_order.is_empty() {
        let mut keys: Vec<u32> = state.panes.keys().copied().collect();
        keys.sort();
        for id in &keys {
            if let Some(p) = state.panes.get(id) {
                id.hash(&mut h);
                p.title.hash(&mut h);
                p.cwd.hash(&mut h);
            }
        }
    } else {
        for id in &state.tab_order {
            if let Some(p) = state.panes.get(id) {
                id.hash(&mut h);
                p.title.hash(&mut h);
                p.cwd.hash(&mut h);
            }
        }
    }

    // === CloseWindowDialog ===
    if let Some(d) = &state.close_window_dialog {
        1u8.hash(&mut h); // tag: present
        d.message.hash(&mut h);
        d.kill_label.hash(&mut h);
        d.cancel_label.hash(&mut h);
        d.selected_button.hash(&mut h);
    } else {
        0u8.hash(&mut h);
    }

    // === ContextMenu ===
    if let Some(m) = &state.context_menu {
        1u8.hash(&mut h);
        m.items.len().hash(&mut h);
        m.hovered.hash(&mut h);
        for item in &m.items {
            item.label.hash(&mut h);
            item.hint.hash(&mut h);
        }
    } else {
        0u8.hash(&mut h);
    }

    // === CommandPalette ===
    // actions / hosts / macros 本体は実行中ほぼ変化しないため、
    // query / selected の変化のみで十分（filtered() の中身を間接的に追跡できる）。
    state.palette.is_open.hash(&mut h);
    if state.palette.is_open {
        state.palette.query.hash(&mut h);
        state.palette.selected.hash(&mut h);
    }

    // === HostManager ===
    state.host_manager.is_open.hash(&mut h);
    if state.host_manager.is_open {
        state.host_manager.query.hash(&mut h);
        state.host_manager.selected.hash(&mut h);
    }

    // === MacroPicker ===
    state.macro_picker.is_open.hash(&mut h);
    if state.macro_picker.is_open {
        state.macro_picker.query.hash(&mut h);
        state.macro_picker.selected.hash(&mut h);
    }

    // === SettingsPanel ===
    state.settings_panel.is_open.hash(&mut h);
    if state.settings_panel.is_open {
        let p = &state.settings_panel;
        // SettingsCategory は Hash 未実装のため label() 文字列で代用
        p.category.label().hash(&mut h);
        // build_settings_panel_nodes が読む現在カテゴリのフィールドをすべてハッシュする
        // （カテゴリ切替時にフィールド集合が変わるため全反映）
        p.font_family.hash(&mut h);
        p.font_family_editing.hash(&mut h);
        // f32 は Hash 未実装。to_bits() で u32 化してハッシュする
        p.font_size.to_bits().hash(&mut h);
        p.opacity.to_bits().hash(&mut h);
        p.scheme_index.hash(&mut h);
        p.language_index.hash(&mut h);
        p.auto_check_update.hash(&mut h);
    }

    // === Quick Select（Step 2-2-h）===
    // typed_label 変化でフォーカス先の選択状態が変わるため必須。
    // matches.len() + 各 label / text を反映してマッチ集合の変化（enter() 時）も検知する。
    state.quick_select.is_active.hash(&mut h);
    if state.quick_select.is_active {
        state.quick_select.typed_label.hash(&mut h);
        state.quick_select.matches.len().hash(&mut h);
        for m in &state.quick_select.matches {
            m.label.hash(&mut h);
            m.text.hash(&mut h);
        }
    }

    // === update_banner（非モーダル）===
    state.update_banner.hash(&mut h);

    h.finish()
}

/// Sprint 5-11-2 Step 2-4 拡張: 設定パネルの AccessKit アクションを処理する純関数。
///
/// `EventHandler::handle_accesskit_action` から呼び出される。`EventHandler` を構築せず
/// 単体テスト可能にするため独立関数として切り出している。
///
/// # 戻り値
///
/// `true` の場合、呼び出し側は再描画を要求する（処理が状態変更を伴う）。
/// `false` の場合、対象 NodeId が設定パネル系でないか、対象アクションに対応がない。
///
/// # 設計メモ
///
/// - `Focus` を SR 経路の状態変更トリガーに使うのは「タブ/ペイン/カテゴリタブ」のみ
///   （仮想カーソル移動 = 制御遷移と解釈）。CheckBox や TextInput では Focus で副作用を
///   起こさず描画状態のみ。
/// - `SettingsFontSize` / `SettingsWindowOpacity` の SetValue は `set_*_value` の純関数
///   setter（0.5 / 0.05 単位丸めと clamp）に委譲する。
/// - ThemeScheme / Language の Click と Increment は同等扱い（ComboBox の「次へ」）。
pub fn dispatch_settings_action(
    panel: &mut SettingsPanel,
    action: accesskit::Action,
    kind: &NodeIdKind,
    data: Option<accesskit::ActionData>,
) -> bool {
    use crate::settings_panel::SettingsCategory;
    use accesskit::{Action, ActionData};

    match (action, kind) {
        // ===== カテゴリタブ =====
        (Action::Focus | Action::Click, NodeIdKind::SettingsTab { idx }) => {
            if let Some(cat) = SettingsCategory::ALL.get(*idx) {
                panel.category = cat.clone();
                panel.font_family_editing = false;
                true
            } else {
                false
            }
        }

        // ===== フォントファミリー (TextInput) =====
        (Action::Click, NodeIdKind::SettingsFontFamily) => {
            panel.font_family_editing = true;
            true
        }
        (Action::SetValue, NodeIdKind::SettingsFontFamily) => {
            if let Some(ActionData::Value(s)) = data {
                panel.font_family = s.into_string();
                panel.dirty = true;
                true
            } else {
                false
            }
        }

        // ===== フォントサイズ (Slider) =====
        (Action::SetValue, NodeIdKind::SettingsFontSize) => {
            if let Some(ActionData::NumericValue(v)) = data {
                panel.set_font_size_value(v);
                true
            } else {
                false
            }
        }
        (Action::Increment, NodeIdKind::SettingsFontSize) => {
            panel.increase_font_size();
            true
        }
        (Action::Decrement, NodeIdKind::SettingsFontSize) => {
            panel.decrease_font_size();
            true
        }

        // ===== テーマスキーム (ComboBox) =====
        (Action::Click | Action::Increment, NodeIdKind::SettingsThemeScheme) => {
            panel.next_scheme();
            true
        }
        (Action::Decrement, NodeIdKind::SettingsThemeScheme) => {
            panel.prev_scheme();
            true
        }

        // ===== ウィンドウ不透明度 (Slider) =====
        (Action::SetValue, NodeIdKind::SettingsWindowOpacity) => {
            if let Some(ActionData::NumericValue(v)) = data {
                panel.set_opacity_value(v);
                true
            } else {
                false
            }
        }
        (Action::Increment, NodeIdKind::SettingsWindowOpacity) => {
            panel.increase_opacity();
            true
        }
        (Action::Decrement, NodeIdKind::SettingsWindowOpacity) => {
            panel.decrease_opacity();
            true
        }

        // ===== 言語 (ComboBox) =====
        (Action::Click | Action::Increment, NodeIdKind::SettingsStartupLanguage) => {
            panel.next_language();
            true
        }
        (Action::Decrement, NodeIdKind::SettingsStartupLanguage) => {
            panel.prev_language();
            true
        }

        // ===== 自動更新確認 (CheckBox) =====
        // Focus でトグルすると SR の仮想カーソル通過で値が変わるため Click のみで反応する。
        (Action::Click, NodeIdKind::SettingsStartupAutoUpdate) => {
            panel.toggle_auto_check_update();
            true
        }

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    // テスト内で `SettingsPanel::default()` の後にフィールドを個別代入するパターンは
    // SR ディスパッチ仕様を読みやすく示すために許容する（多フィールド struct のため
    // 構造体リテラルで書くと冗長になる）。
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use crate::state::ClientState;

    /// NodeId オフセットの安全性: タブとペインの ID 範囲が衝突しないこと
    #[test]
    fn node_id_offsets_do_not_overlap() {
        let max_tab = tab_node_id(u32::MAX).0;
        let min_pane = pane_node_id(0).0;
        assert!(
            max_tab < min_pane,
            "タブ ID 範囲 [{}, {}] とペイン ID 範囲 [{}, ...] が衝突する",
            NODE_ID_TAB_OFFSET,
            max_tab,
            min_pane
        );
        const _: () = assert!(NODE_ID_TAB_OFFSET > 99);
    }

    /// オーバーレイ動的 ID オフセットがタブ範囲と衝突しないこと
    #[test]
    fn overlay_offsets_do_not_overlap_with_tabs() {
        // 各オーバーレイ ID オフセット < タブオフセット
        const _: () = assert!(NODE_ID_PALETTE_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        const _: () = assert!(NODE_ID_HOST_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        const _: () = assert!(NODE_ID_MACRO_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        const _: () = assert!(NODE_ID_CONTEXT_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        const _: () = assert!(NODE_ID_QUICKSELECT_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        // 異なるオーバーレイの ID 範囲が交差しないこと（10万件まで安全と想定）
        const ITEM_CAP: u64 = 100_000_000; // 各オフセット間の差
        const _: () = assert!(NODE_ID_HOST_ITEM_OFFSET - NODE_ID_PALETTE_ITEM_OFFSET >= ITEM_CAP);
        const _: () = assert!(NODE_ID_MACRO_ITEM_OFFSET - NODE_ID_HOST_ITEM_OFFSET >= ITEM_CAP);
        const _: () = assert!(NODE_ID_CONTEXT_ITEM_OFFSET - NODE_ID_MACRO_ITEM_OFFSET >= ITEM_CAP);
        const _: () =
            assert!(NODE_ID_QUICKSELECT_ITEM_OFFSET - NODE_ID_CONTEXT_ITEM_OFFSET >= ITEM_CAP);
    }

    /// 空の ClientState でツリー構築（初期状態）
    #[test]
    fn build_tree_from_empty_state() {
        let state = ClientState::new(80, 24, 1000);
        let update = build_tree_from_state(&state);

        // ROOT / TAB_BAR / PANE_AREA の 3 ノードのみ
        assert_eq!(update.nodes.len(), 3);
        assert_eq!(update.focus, ROOT_ID);
        assert!(update.tree.is_some());
    }

    /// 単一ペイン構成のツリー
    #[test]
    fn build_tree_with_single_pane() {
        let mut state = ClientState::new(80, 24, 1000);
        state
            .panes
            .insert(42, crate::state::PaneState::new(80, 24, 1000));
        state.tab_order = vec![42];
        state.focused_pane_id = Some(42);

        let update = build_tree_from_state(&state);

        // ROOT + TAB_BAR + PANE_AREA + Tab + Pane + 24 PaneRow = 29
        assert_eq!(update.nodes.len(), 29);
        assert_eq!(update.focus, pane_node_id(42));

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&tab_node_id(42).0));
        assert!(ids.contains(&pane_node_id(42).0));
    }

    /// 複数ペイン構成: タブ順序が tab_order に従うこと
    #[test]
    fn build_tree_respects_tab_order() {
        let mut state = ClientState::new(80, 24, 1000);
        for id in [10u32, 20, 30] {
            state
                .panes
                .insert(id, crate::state::PaneState::new(80, 24, 1000));
        }
        state.tab_order = vec![30, 10, 20];
        state.focused_pane_id = Some(10);

        let update = build_tree_from_state(&state);

        // ROOT + TAB_BAR + PANE_AREA + 3 Tab + 3 Pane + 3 * 24 PaneRow = 81
        assert_eq!(update.nodes.len(), 81);
        assert_eq!(update.focus, pane_node_id(10));
    }

    /// タイトル付きペインのラベル生成
    #[test]
    fn build_tree_uses_pane_title() {
        let mut state = ClientState::new(80, 24, 1000);
        let mut pane = crate::state::PaneState::new(80, 24, 1000);
        pane.title = "vim main.rs".to_string();
        pane.cwd = Some("/home/user/project".to_string());
        state.panes.insert(1, pane);
        state.tab_order = vec![1];

        let update = build_tree_from_state(&state);

        // ROOT + TAB_BAR + PANE_AREA + Tab + Pane + 24 PaneRow = 29
        assert_eq!(update.nodes.len(), 29);
    }

    /// CommandPalette 表示時にダイアログ + 検索 + 候補リストが含まれること
    #[test]
    fn build_tree_with_open_palette() {
        let mut state = ClientState::new(80, 24, 1000);
        state.palette.is_open = true;
        state.palette.query = "edit".to_string();
        state.palette.selected = 0;

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&PALETTE_ID.0), "PALETTE_ID が含まれていない");
        assert!(
            ids.contains(&PALETTE_SEARCH_ID.0),
            "PALETTE_SEARCH_ID が含まれていない"
        );
        assert!(
            ids.contains(&PALETTE_LIST_ID.0),
            "PALETTE_LIST_ID が含まれていない"
        );

        // フォーカスは検索入力（候補なしのため）または最初の候補
        // 標準デフォルトでは候補があるはずだが、ここでは存在性のみ確認
        assert!(update.focus == PALETTE_SEARCH_ID || update.focus == palette_item_id(0));
    }

    /// CloseWindowDialog 表示時に AlertDialog + 2 ボタンが含まれること
    #[test]
    fn build_tree_with_close_dialog() {
        let mut state = ClientState::new(80, 24, 1000);
        state.close_window_dialog = Some(CloseWindowDialog {
            server_window_id: 1,
            message: "プロセスがまだ動いています。本当に閉じますか？".to_string(),
            kill_label: "強制終了".to_string(),
            cancel_label: "キャンセル".to_string(),
            selected_button: 1, // Cancel
        });

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&CLOSE_DIALOG_ID.0));
        assert!(ids.contains(&CLOSE_DIALOG_KILL_BTN.0));
        assert!(ids.contains(&CLOSE_DIALOG_CANCEL_BTN.0));

        // フォーカスは Cancel ボタン
        assert_eq!(update.focus, CLOSE_DIALOG_CANCEL_BTN);
    }

    /// ContextMenu 表示時に Menu + MenuItem が含まれること
    #[test]
    fn build_tree_with_context_menu() {
        let mut state = ClientState::new(80, 24, 1000);
        state.context_menu = Some(ContextMenu::new_default(100.0, 100.0, &[]));

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&CONTEXT_MENU_ID.0));
        // 標準メニューは複数項目を持つ。コンテキストメニュー項目の NodeId 範囲は
        // [NODE_ID_CONTEXT_ITEM_OFFSET, NODE_ID_TAB_OFFSET) （次のオフセットがタブ）
        let item_count = ids
            .iter()
            .filter(|&&id| (NODE_ID_CONTEXT_ITEM_OFFSET..NODE_ID_TAB_OFFSET).contains(&id))
            .count();
        assert!(item_count > 0, "コンテキストメニュー項目が含まれていない");
    }

    /// 優先順位: CloseWindowDialog が他のオーバーレイより優先されること
    #[test]
    fn close_dialog_takes_priority_over_palette() {
        let mut state = ClientState::new(80, 24, 1000);
        state.palette.is_open = true;
        state.close_window_dialog = Some(CloseWindowDialog {
            server_window_id: 1,
            message: "Test".to_string(),
            kill_label: "OK".to_string(),
            cancel_label: "Cancel".to_string(),
            selected_button: 0,
        });

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&CLOSE_DIALOG_ID.0));
        // パレットは追加されない（優先度が低いため）
        assert!(
            !ids.contains(&PALETTE_ID.0),
            "CloseWindowDialog 表示時はパレットは含まれないはず"
        );
    }

    /// Quick Select 表示時に Dialog + ListBox + マッチ項目が含まれること
    #[test]
    fn build_tree_with_quick_select_overlay() {
        use crate::state::QuickSelectMatch;

        let mut state = ClientState::new(80, 24, 1000);
        state.quick_select.is_active = true;
        state.quick_select.matches = vec![
            QuickSelectMatch {
                row: 0,
                col_start: 0,
                col_end: 19,
                text: "https://example.com".to_string(),
                label: "a".to_string(),
            },
            QuickSelectMatch {
                row: 1,
                col_start: 0,
                col_end: 13,
                text: "foo@bar.com".to_string(),
                label: "b".to_string(),
            },
        ];

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(
            ids.contains(&QUICK_SELECT_ID.0),
            "QUICK_SELECT_ID が含まれない"
        );
        assert!(
            ids.contains(&QUICK_SELECT_LIST_ID.0),
            "QUICK_SELECT_LIST_ID が含まれない"
        );
        assert!(
            ids.contains(&quickselect_item_id(0).0),
            "マッチ項目 0 が含まれない"
        );
        assert!(
            ids.contains(&quickselect_item_id(1).0),
            "マッチ項目 1 が含まれない"
        );
        // typed_label が空のときは最初のマッチにフォーカス
        assert_eq!(update.focus, quickselect_item_id(0));
    }

    /// typed_label が prefix で一致したらその項目にフォーカスが移ること
    #[test]
    fn quick_select_focus_follows_typed_label() {
        use crate::state::QuickSelectMatch;

        let mut state = ClientState::new(80, 24, 1000);
        state.quick_select.is_active = true;
        state.quick_select.typed_label = "b".to_string();
        state.quick_select.matches = vec![
            QuickSelectMatch {
                row: 0,
                col_start: 0,
                col_end: 5,
                text: "alpha".to_string(),
                label: "a".to_string(),
            },
            QuickSelectMatch {
                row: 1,
                col_start: 0,
                col_end: 4,
                text: "beta".to_string(),
                label: "b".to_string(),
            },
        ];

        let update = build_tree_from_state(&state);
        assert_eq!(update.focus, quickselect_item_id(1));
    }

    /// Quick Select マッチなし時は ListBox 自身にフォーカス
    #[test]
    fn quick_select_focus_falls_back_to_list_when_empty() {
        let mut state = ClientState::new(80, 24, 1000);
        state.quick_select.is_active = true;
        // matches は空のまま

        let update = build_tree_from_state(&state);
        assert_eq!(update.focus, QUICK_SELECT_LIST_ID);
    }

    /// CloseWindowDialog は Quick Select より優先される（最強モーダル）
    #[test]
    fn close_dialog_takes_priority_over_quick_select() {
        use crate::state::QuickSelectMatch;

        let mut state = ClientState::new(80, 24, 1000);
        state.quick_select.is_active = true;
        state.quick_select.matches = vec![QuickSelectMatch {
            row: 0,
            col_start: 0,
            col_end: 3,
            text: "foo".to_string(),
            label: "a".to_string(),
        }];
        state.close_window_dialog = Some(CloseWindowDialog {
            server_window_id: 1,
            message: "Test".to_string(),
            kill_label: "OK".to_string(),
            cancel_label: "Cancel".to_string(),
            selected_button: 0,
        });

        let update = build_tree_from_state(&state);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&CLOSE_DIALOG_ID.0));
        assert!(
            !ids.contains(&QUICK_SELECT_ID.0),
            "CloseDialog 表示時は Quick Select は含まれないはず"
        );
    }

    /// Quick Select は ContextMenu / Palette より優先される
    #[test]
    fn quick_select_takes_priority_over_context_menu_and_palette() {
        let mut state = ClientState::new(80, 24, 1000);
        state.quick_select.is_active = true;
        state.palette.is_open = true;
        state.context_menu = Some(ContextMenu::new_default(100.0, 100.0, &[]));

        let update = build_tree_from_state(&state);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&QUICK_SELECT_ID.0));
        assert!(
            !ids.contains(&CONTEXT_MENU_ID.0),
            "Quick Select 中は ContextMenu は含まれないはず"
        );
        assert!(
            !ids.contains(&PALETTE_ID.0),
            "Quick Select 中は Palette は含まれないはず"
        );
    }

    /// 更新バナーは非モーダルとして他のオーバーレイと共存
    #[test]
    fn update_banner_coexists_with_palette() {
        let mut state = ClientState::new(80, 24, 1000);
        state.palette.is_open = true;
        state.update_banner = Some("v1.6.0".to_string());

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&PALETTE_ID.0));
        assert!(ids.contains(&UPDATE_BANNER_ID.0));
    }

    // ===== Step 2-5: ライブ更新用ステートハッシュテスト =====

    /// 同じ状態は同じハッシュを返す（決定論的）
    #[test]
    fn tree_state_hash_is_deterministic() {
        let mut state = ClientState::new(80, 24, 1000);
        let mut pane = crate::state::PaneState::new(80, 24, 1000);
        pane.title = "vim".to_string();
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let h1 = compute_tree_state_hash(&state);
        let h2 = compute_tree_state_hash(&state);
        assert_eq!(h1, h2, "同一状態のハッシュが一致しない");
    }

    /// タイトル変更でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_title_change() {
        let mut state = ClientState::new(80, 24, 1000);
        let mut pane = crate::state::PaneState::new(80, 24, 1000);
        pane.title = "vim".to_string();
        state.panes.insert(1, pane);
        state.tab_order = vec![1];

        let h1 = compute_tree_state_hash(&state);

        state.panes.get_mut(&1).unwrap().title = "emacs".to_string();
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "タイトル変更でハッシュが変化しない");
    }

    /// フォーカス変更でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_focus_change() {
        let mut state = ClientState::new(80, 24, 1000);
        for id in [1u32, 2] {
            state
                .panes
                .insert(id, crate::state::PaneState::new(80, 24, 1000));
        }
        state.tab_order = vec![1, 2];
        state.focused_pane_id = Some(1);

        let h1 = compute_tree_state_hash(&state);

        state.focused_pane_id = Some(2);
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "フォーカス変更でハッシュが変化しない");
    }

    /// パレット開閉でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_palette_open() {
        let state_closed = ClientState::new(80, 24, 1000);
        let h_closed = compute_tree_state_hash(&state_closed);

        let mut state_open = ClientState::new(80, 24, 1000);
        state_open.palette.is_open = true;
        let h_open = compute_tree_state_hash(&state_open);

        assert_ne!(h_closed, h_open, "パレットの開閉でハッシュが変化しない");
    }

    /// パレット内クエリ変更でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_palette_query_change() {
        let mut state = ClientState::new(80, 24, 1000);
        state.palette.is_open = true;
        state.palette.query = "abc".to_string();
        let h1 = compute_tree_state_hash(&state);

        state.palette.query = "xyz".to_string();
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "パレットクエリ変更でハッシュが変化しない");
    }

    /// CloseWindowDialog の selected_button 変更でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_dialog_button_change() {
        let mut state = ClientState::new(80, 24, 1000);
        state.close_window_dialog = Some(CloseWindowDialog {
            server_window_id: 1,
            message: "Test".to_string(),
            kill_label: "OK".to_string(),
            cancel_label: "Cancel".to_string(),
            selected_button: 0,
        });
        let h1 = compute_tree_state_hash(&state);

        state.close_window_dialog.as_mut().unwrap().selected_button = 1;
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "CloseWindowDialog ボタン変更でハッシュが変化しない");
    }

    /// Quick Select の開閉でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_quick_select_open() {
        let state_closed = ClientState::new(80, 24, 1000);
        let h_closed = compute_tree_state_hash(&state_closed);

        let mut state_open = ClientState::new(80, 24, 1000);
        state_open.quick_select.is_active = true;
        let h_open = compute_tree_state_hash(&state_open);

        assert_ne!(
            h_closed, h_open,
            "Quick Select の開閉でハッシュが変化しない"
        );
    }

    /// Quick Select の typed_label 変更でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_quick_select_typed_label_change() {
        let mut state = ClientState::new(80, 24, 1000);
        state.quick_select.is_active = true;
        state.quick_select.typed_label = "a".to_string();
        let h1 = compute_tree_state_hash(&state);

        state.quick_select.typed_label = "ab".to_string();
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "typed_label 変更でハッシュが変化しない");
    }

    /// 更新バナーの追加/削除でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_update_banner() {
        let state_none = ClientState::new(80, 24, 1000);
        let h_none = compute_tree_state_hash(&state_none);

        let mut state_banner = ClientState::new(80, 24, 1000);
        state_banner.update_banner = Some("v1.6.0".to_string());
        let h_banner = compute_tree_state_hash(&state_banner);

        assert_ne!(h_none, h_banner, "バナー追加でハッシュが変化しない");
    }

    // ===== Step 2-4: decode_node_id ユニットテスト =====

    /// 固定 NodeId が正しく逆引きできること
    #[test]
    fn decode_fixed_node_ids() {
        assert_eq!(decode_node_id(ROOT_ID), NodeIdKind::Root);
        assert_eq!(decode_node_id(TAB_BAR_ID), NodeIdKind::TabBar);
        assert_eq!(decode_node_id(PANE_AREA_ID), NodeIdKind::PaneArea);
        assert_eq!(decode_node_id(SETTINGS_PANEL_ID), NodeIdKind::SettingsPanel);
        assert_eq!(decode_node_id(PALETTE_ID), NodeIdKind::Palette);
        assert_eq!(decode_node_id(HOST_MANAGER_ID), NodeIdKind::HostManager);
        assert_eq!(decode_node_id(MACRO_PICKER_ID), NodeIdKind::MacroPicker);
        assert_eq!(decode_node_id(CONTEXT_MENU_ID), NodeIdKind::ContextMenu);
        assert_eq!(decode_node_id(CLOSE_DIALOG_ID), NodeIdKind::CloseDialog);
        assert_eq!(decode_node_id(UPDATE_BANNER_ID), NodeIdKind::UpdateBanner);
        assert_eq!(decode_node_id(QUICK_SELECT_ID), NodeIdKind::QuickSelect);
        assert_eq!(decode_node_id(PALETTE_SEARCH_ID), NodeIdKind::PaletteSearch);
        assert_eq!(decode_node_id(PALETTE_LIST_ID), NodeIdKind::PaletteList);
        assert_eq!(
            decode_node_id(CLOSE_DIALOG_KILL_BTN),
            NodeIdKind::CloseDialogKill
        );
        assert_eq!(
            decode_node_id(CLOSE_DIALOG_CANCEL_BTN),
            NodeIdKind::CloseDialogCancel
        );
        assert_eq!(
            decode_node_id(QUICK_SELECT_LIST_ID),
            NodeIdKind::QuickSelectList
        );
    }

    /// Quick Select マッチ NodeId のラウンドトリップ
    #[test]
    fn decode_quick_select_item_ids() {
        assert_eq!(
            decode_node_id(quickselect_item_id(0)),
            NodeIdKind::QuickSelectItem { idx: 0 }
        );
        assert_eq!(
            decode_node_id(quickselect_item_id(42)),
            NodeIdKind::QuickSelectItem { idx: 42 }
        );
        assert_eq!(
            decode_node_id(NodeId(500_000_000)),
            NodeIdKind::QuickSelectItem { idx: 0 }
        );
        assert_eq!(
            decode_node_id(NodeId(500_000_099)),
            NodeIdKind::QuickSelectItem { idx: 99 }
        );
    }

    /// タブ NodeId（`tab_node_id(pane_id)`）の逆引きラウンドトリップ
    #[test]
    fn decode_tab_node_id_roundtrip() {
        for &pane_id in &[0u32, 1, 42, 12345, u32::MAX] {
            assert_eq!(
                decode_node_id(tab_node_id(pane_id)),
                NodeIdKind::Tab { pane_id }
            );
        }
    }

    /// ペイン NodeId（`pane_node_id(pane_id)`）の逆引きラウンドトリップ
    #[test]
    fn decode_pane_node_id_roundtrip() {
        for &pane_id in &[0u32, 1, 42, 12345, u32::MAX] {
            assert_eq!(
                decode_node_id(pane_node_id(pane_id)),
                NodeIdKind::Pane { pane_id }
            );
        }
    }

    /// 動的オフセット項目（パレット / ホスト / マクロ / コンテキスト）の逆引き
    #[test]
    fn decode_dynamic_item_ids() {
        assert_eq!(
            decode_node_id(NodeId(100_000_000)),
            NodeIdKind::PaletteItem { idx: 0 }
        );
        assert_eq!(
            decode_node_id(NodeId(100_000_042)),
            NodeIdKind::PaletteItem { idx: 42 }
        );
        assert_eq!(
            decode_node_id(NodeId(200_000_000)),
            NodeIdKind::HostItem { idx: 0 }
        );
        assert_eq!(
            decode_node_id(NodeId(200_000_007)),
            NodeIdKind::HostItem { idx: 7 }
        );
        assert_eq!(
            decode_node_id(NodeId(300_000_000)),
            NodeIdKind::MacroItem { idx: 0 }
        );
        assert_eq!(
            decode_node_id(NodeId(400_000_000)),
            NodeIdKind::ContextItem { idx: 0 }
        );
        assert_eq!(
            decode_node_id(NodeId(400_000_099)),
            NodeIdKind::ContextItem { idx: 99 }
        );
    }

    /// 未知 / 予約範囲は `Unknown` を返す
    #[test]
    fn decode_unknown_node_ids() {
        assert_eq!(decode_node_id(NodeId(0)), NodeIdKind::Unknown);
        // 17 は SettingsTabList、18〜24 は SettingsTab、25 は SettingsContent、
        // 30〜35 は設定フィールド（Step 2-2-e' で割り当て済）。
        // 26〜29, 36〜99 は将来用に予約。
        assert_eq!(decode_node_id(NodeId(26)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(29)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(36)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(99)), NodeIdKind::Unknown);
        // 600M〜999M は将来 SettingsField 動的展開で使う予約範囲
        assert_eq!(decode_node_id(NodeId(600_000_000)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(999_999_999)), NodeIdKind::Unknown);
        // タブとペインの間の隙間（5.3e9〜1e10）も Unknown
        assert_eq!(decode_node_id(NodeId(7_000_000_000)), NodeIdKind::Unknown);
        // ペイン範囲と行範囲の間の隙間（1e10 + u32::MAX 〜 2e10）も Unknown
        assert_eq!(decode_node_id(NodeId(15_000_000_000)), NodeIdKind::Unknown);
        // 行範囲を超えた範囲（u32::MAX * MAX_ROWS_PER_PANE + 2e10 以降）
        let row_range_end =
            NODE_ID_PANE_ROW_OFFSET + (u32::MAX as u64) * MAX_ROWS_PER_PANE + MAX_ROWS_PER_PANE;
        assert_eq!(decode_node_id(NodeId(row_range_end)), NodeIdKind::Unknown);
    }

    // ===== Step 2-2-e': SettingsField 展開 =====

    /// SettingsPanel TabList と各カテゴリタブの NodeId が正しく逆引きできること
    #[test]
    fn decode_settings_tab_node_ids() {
        assert_eq!(
            decode_node_id(SETTINGS_TABLIST_ID),
            NodeIdKind::SettingsTabList
        );
        assert_eq!(
            decode_node_id(SETTINGS_CONTENT_ID),
            NodeIdKind::SettingsContent
        );
        for idx in 0..7 {
            assert_eq!(
                decode_node_id(settings_tab_id_at(idx)),
                NodeIdKind::SettingsTab { idx },
                "settings_tab_id_at({}) が逆引きできない",
                idx
            );
        }
    }

    /// 各設定フィールド NodeId が正しく逆引きできること
    #[test]
    fn decode_settings_field_node_ids() {
        assert_eq!(
            decode_node_id(SETTINGS_FONT_FAMILY_ID),
            NodeIdKind::SettingsFontFamily
        );
        assert_eq!(
            decode_node_id(SETTINGS_FONT_SIZE_ID),
            NodeIdKind::SettingsFontSize
        );
        assert_eq!(
            decode_node_id(SETTINGS_THEME_SCHEME_ID),
            NodeIdKind::SettingsThemeScheme
        );
        assert_eq!(
            decode_node_id(SETTINGS_WINDOW_OPACITY_ID),
            NodeIdKind::SettingsWindowOpacity
        );
        assert_eq!(
            decode_node_id(SETTINGS_STARTUP_LANGUAGE_ID),
            NodeIdKind::SettingsStartupLanguage
        );
        assert_eq!(
            decode_node_id(SETTINGS_STARTUP_AUTO_UPDATE_ID),
            NodeIdKind::SettingsStartupAutoUpdate
        );
    }

    /// SettingsPanel を開いたとき Dialog + TabList + 全カテゴリタブ + Content が含まれること
    #[test]
    fn build_tree_with_settings_panel_open() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Font;

        let update = build_tree_from_state(&state);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();

        assert!(ids.contains(&SETTINGS_PANEL_ID.0));
        assert!(ids.contains(&SETTINGS_TABLIST_ID.0));
        assert!(ids.contains(&SETTINGS_CONTENT_ID.0));
        for idx in 0..SettingsCategory::ALL.len() {
            assert!(
                ids.contains(&settings_tab_id_at(idx).0),
                "カテゴリタブ {} が含まれない",
                idx
            );
        }
        // Font カテゴリのフィールドが含まれる
        assert!(ids.contains(&SETTINGS_FONT_FAMILY_ID.0));
        assert!(ids.contains(&SETTINGS_FONT_SIZE_ID.0));
    }

    /// Font 編集中はフォーカスが FontFamily 入力欄に移ること
    #[test]
    fn settings_panel_focus_follows_font_family_editing() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Font;
        state.settings_panel.font_family_editing = true;

        let update = build_tree_from_state(&state);
        assert_eq!(update.focus, SETTINGS_FONT_FAMILY_ID);
    }

    /// 編集中でなければフォーカスは現在カテゴリのタブ
    #[test]
    fn settings_panel_focus_defaults_to_current_tab() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Theme;

        let update = build_tree_from_state(&state);
        // Theme は SettingsCategory::ALL のインデックス 2
        assert_eq!(update.focus, settings_tab_id_at(2));
    }

    /// カテゴリ別に正しいフィールドだけが含まれること
    #[test]
    fn settings_panel_shows_only_current_category_fields() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;

        // Startup カテゴリ
        state.settings_panel.category = SettingsCategory::Startup;
        let update = build_tree_from_state(&state);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&SETTINGS_STARTUP_LANGUAGE_ID.0));
        assert!(ids.contains(&SETTINGS_STARTUP_AUTO_UPDATE_ID.0));
        assert!(
            !ids.contains(&SETTINGS_FONT_FAMILY_ID.0),
            "Startup カテゴリで Font フィールドが含まれている"
        );

        // Window カテゴリ
        state.settings_panel.category = SettingsCategory::Window;
        let update = build_tree_from_state(&state);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&SETTINGS_WINDOW_OPACITY_ID.0));
        assert!(
            !ids.contains(&SETTINGS_THEME_SCHEME_ID.0),
            "Window カテゴリで Theme フィールドが含まれている"
        );
    }

    /// SSH / Keybindings / Profiles カテゴリは Content Group のみで詳細フィールドは含まれない
    #[test]
    fn settings_panel_unimplemented_categories_have_empty_content() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;

        for cat in [
            SettingsCategory::Ssh,
            SettingsCategory::Keybindings,
            SettingsCategory::Profiles,
        ] {
            state.settings_panel.category = cat;
            let update = build_tree_from_state(&state);
            let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
            // Content Group は存在
            assert!(ids.contains(&SETTINGS_CONTENT_ID.0));
            // 詳細フィールドは含まれない
            assert!(!ids.contains(&SETTINGS_FONT_FAMILY_ID.0));
            assert!(!ids.contains(&SETTINGS_THEME_SCHEME_ID.0));
            assert!(!ids.contains(&SETTINGS_WINDOW_OPACITY_ID.0));
        }
    }

    /// カテゴリ切替でハッシュが変わる（タブの selected が変わるため）
    #[test]
    fn tree_state_hash_detects_settings_category_change() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Font;
        let h1 = compute_tree_state_hash(&state);

        state.settings_panel.category = SettingsCategory::Theme;
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "カテゴリ切替でハッシュが変化しない");
    }

    /// フォントサイズ変更でハッシュが変わる
    #[test]
    fn tree_state_hash_detects_settings_font_size_change() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Font;
        state.settings_panel.font_size = 14.0;
        let h1 = compute_tree_state_hash(&state);

        state.settings_panel.font_size = 16.0;
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "font_size 変更でハッシュが変化しない");
    }

    /// auto_check_update トグルでハッシュが変わる
    #[test]
    fn tree_state_hash_detects_settings_auto_update_toggle() {
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.auto_check_update = false;
        let h1 = compute_tree_state_hash(&state);

        state.settings_panel.auto_check_update = true;
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "auto_check_update トグルでハッシュが変化しない");
    }

    // ============================================================
    // Sprint 5-11-2 Step 2-4 拡張: dispatch_settings_action の単体テスト
    // ============================================================

    use crate::settings_panel::{SettingsCategory, SettingsPanel};
    use accesskit::{Action, ActionData};

    /// SettingsTab に Focus / Click を投げるとカテゴリが切り替わり、編集モードも抜ける
    #[test]
    fn dispatch_settings_tab_click_changes_category() {
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Font;
        panel.font_family_editing = true;

        // ALL の idx=2 は Theme
        let kind = NodeIdKind::SettingsTab { idx: 2 };
        let handled = dispatch_settings_action(&mut panel, Action::Click, &kind, None);

        assert!(handled, "SettingsTab Click は handled=true を返すべき");
        assert_eq!(panel.category, SettingsCategory::Theme);
        assert!(
            !panel.font_family_editing,
            "カテゴリ切替で font_family_editing が解除されるべき"
        );

        // Focus でも同様に動作する
        let kind2 = NodeIdKind::SettingsTab { idx: 0 };
        let handled = dispatch_settings_action(&mut panel, Action::Focus, &kind2, None);
        assert!(handled);
        assert_eq!(panel.category, SettingsCategory::Startup);
    }

    /// 範囲外の SettingsTab idx は handled=false（カテゴリ不変）
    #[test]
    fn dispatch_settings_tab_out_of_range_returns_false() {
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Font;
        let original = panel.category.clone();

        let kind = NodeIdKind::SettingsTab { idx: 99 };
        let handled = dispatch_settings_action(&mut panel, Action::Click, &kind, None);

        assert!(!handled, "範囲外 idx は handled=false");
        assert_eq!(panel.category, original, "カテゴリは変わらないべき");
    }

    /// SettingsFontFamily Click で編集モードに入る
    #[test]
    fn dispatch_settings_font_family_click_enters_editing() {
        let mut panel = SettingsPanel::default();
        panel.font_family_editing = false;

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsFontFamily,
            None,
        );

        assert!(handled);
        assert!(
            panel.font_family_editing,
            "Click で font_family_editing=true になるべき"
        );
    }

    /// SettingsFontFamily SetValue で文字列が反映され dirty=true
    #[test]
    fn dispatch_settings_font_family_set_value_updates_string() {
        let mut panel = SettingsPanel::default();
        panel.dirty = false;

        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsFontFamily,
            Some(ActionData::Value(
                "JetBrains Mono".to_string().into_boxed_str(),
            )),
        );

        assert!(handled);
        assert_eq!(panel.font_family, "JetBrains Mono");
        assert!(panel.dirty, "値設定で dirty=true になるべき");
    }

    /// SettingsFontFamily SetValue に NumericValue を渡しても無視（handled=false）
    #[test]
    fn dispatch_settings_font_family_set_value_with_numeric_returns_false() {
        let mut panel = SettingsPanel::default();
        let original = panel.font_family.clone();

        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsFontFamily,
            Some(ActionData::NumericValue(42.0)),
        );

        assert!(!handled, "NumericValue は TextInput では handled=false");
        assert_eq!(panel.font_family, original);
    }

    /// SettingsFontSize SetValue で 0.5 単位丸めと clamp 8.0〜32.0 が効く
    #[test]
    fn dispatch_settings_font_size_set_value_rounds_and_clamps() {
        let mut panel = SettingsPanel::default();

        // 14.37 → 14.5 に丸まる
        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsFontSize,
            Some(ActionData::NumericValue(14.37)),
        );
        assert!(handled);
        assert!(
            (panel.font_size - 14.5).abs() < f32::EPSILON,
            "0.5 単位丸め: 14.37 → 14.5, actual = {}",
            panel.font_size
        );

        // 100.0 → 32.0 にクランプ
        dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsFontSize,
            Some(ActionData::NumericValue(100.0)),
        );
        assert!(
            (panel.font_size - 32.0).abs() < f32::EPSILON,
            "上限 32.0 にクランプ"
        );

        // 1.0 → 8.0 にクランプ
        dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsFontSize,
            Some(ActionData::NumericValue(1.0)),
        );
        assert!(
            (panel.font_size - 8.0).abs() < f32::EPSILON,
            "下限 8.0 にクランプ"
        );
    }

    /// SettingsFontSize Increment / Decrement で 0.5 ステップ
    #[test]
    fn dispatch_settings_font_size_increment_decrement() {
        let mut panel = SettingsPanel::default();
        panel.font_size = 14.0;

        dispatch_settings_action(
            &mut panel,
            Action::Increment,
            &NodeIdKind::SettingsFontSize,
            None,
        );
        assert!((panel.font_size - 14.5).abs() < f32::EPSILON);

        dispatch_settings_action(
            &mut panel,
            Action::Decrement,
            &NodeIdKind::SettingsFontSize,
            None,
        );
        assert!((panel.font_size - 14.0).abs() < f32::EPSILON);
    }

    /// SettingsThemeScheme Click / Increment は next_scheme と同等（1 増える）
    #[test]
    fn dispatch_settings_theme_scheme_click_advances() {
        let mut panel = SettingsPanel::default();
        panel.scheme_index = 0;

        dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsThemeScheme,
            None,
        );
        assert_eq!(panel.scheme_index, 1, "Click で次のスキーム");

        dispatch_settings_action(
            &mut panel,
            Action::Increment,
            &NodeIdKind::SettingsThemeScheme,
            None,
        );
        assert_eq!(panel.scheme_index, 2, "Increment で次のスキーム");

        dispatch_settings_action(
            &mut panel,
            Action::Decrement,
            &NodeIdKind::SettingsThemeScheme,
            None,
        );
        assert_eq!(panel.scheme_index, 1, "Decrement で前のスキーム");
    }

    /// SettingsWindowOpacity SetValue で 0.05 単位丸めと clamp 0.1〜1.0 が効く
    #[test]
    fn dispatch_settings_opacity_set_value_rounds_and_clamps() {
        let mut panel = SettingsPanel::default();

        // 0.737 → 0.75
        dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsWindowOpacity,
            Some(ActionData::NumericValue(0.737)),
        );
        assert!(
            (panel.opacity - 0.75).abs() < 1e-4,
            "0.05 単位丸め: 0.737 → 0.75, actual = {}",
            panel.opacity
        );

        // 2.0 → 1.0
        dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsWindowOpacity,
            Some(ActionData::NumericValue(2.0)),
        );
        assert!((panel.opacity - 1.0).abs() < f32::EPSILON);

        // 0.0 → 0.1
        dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsWindowOpacity,
            Some(ActionData::NumericValue(0.0)),
        );
        assert!((panel.opacity - 0.1).abs() < f32::EPSILON);
    }

    /// SettingsStartupLanguage Click で next_language（インデックス +1）
    #[test]
    fn dispatch_settings_language_click_advances() {
        let mut panel = SettingsPanel::default();
        panel.language_index = 0;

        dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsStartupLanguage,
            None,
        );
        assert_eq!(panel.language_index, 1);

        dispatch_settings_action(
            &mut panel,
            Action::Decrement,
            &NodeIdKind::SettingsStartupLanguage,
            None,
        );
        assert_eq!(panel.language_index, 0);
    }

    /// SettingsStartupAutoUpdate Click でトグル、Focus は無反応
    #[test]
    fn dispatch_settings_auto_update_click_toggles() {
        let mut panel = SettingsPanel::default();
        panel.auto_check_update = false;
        panel.dirty = false;

        // Click でトグル
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsStartupAutoUpdate,
            None,
        );
        assert!(handled);
        assert!(panel.auto_check_update);
        assert!(panel.dirty);

        // もう一度 Click で false に戻る
        dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsStartupAutoUpdate,
            None,
        );
        assert!(!panel.auto_check_update);

        // Focus は無反応
        let before = panel.auto_check_update;
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Focus,
            &NodeIdKind::SettingsStartupAutoUpdate,
            None,
        );
        assert!(
            !handled,
            "Focus は handled=false（CheckBox は Focus でトグルしない）"
        );
        assert_eq!(panel.auto_check_update, before);
    }

    /// 設定パネル系以外の NodeIdKind では handled=false で何もしない
    #[test]
    fn dispatch_settings_action_ignores_non_settings_kinds() {
        let mut panel = SettingsPanel::default();
        let before = panel.font_size;

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::Tab { pane_id: 42 },
            None,
        );
        assert!(!handled);
        assert_eq!(panel.font_size, before);

        let handled =
            dispatch_settings_action(&mut panel, Action::Click, &NodeIdKind::Unknown, None);
        assert!(!handled);
    }

    // ===== Sprint 5-11-3: ペイン行ノード関連テスト =====

    /// テスト用に文字列から `nexterm_proto::Grid` を作る
    fn grid_from_lines(lines: &[&str]) -> nexterm_proto::Grid {
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let height = lines.len() as u16;
        let mut grid = nexterm_proto::Grid::new(width, height);
        for (r, line) in lines.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                let cell = nexterm_proto::Cell {
                    ch,
                    fg: nexterm_proto::Color::Default,
                    bg: nexterm_proto::Color::Default,
                    attrs: nexterm_proto::Attrs::default(),
                };
                grid.set(c as u16, r as u16, cell);
            }
        }
        grid
    }

    /// T1: 末尾の半角空白が除去される
    #[test]
    fn pane_row_text_strips_trailing_spaces() {
        let grid = grid_from_lines(&["hello   "]);
        assert_eq!(pane_row_text(&grid, 0), "hello");
    }

    /// T2: 空行は単一の半角空白を返す（SR が「空行」として認識する境界を保つ）
    #[test]
    fn pane_row_text_empty_row_returns_space() {
        let grid = grid_from_lines(&["        "]);
        assert_eq!(pane_row_text(&grid, 0), " ");
    }

    /// T3: 全角文字は保持される（末尾は半角空白だけ除去）
    #[test]
    fn pane_row_text_preserves_full_width() {
        let grid = grid_from_lines(&["あいう  "]);
        // grid.set は col 単位で書き込むので、3 文字 + パディング 5 セル = 8 セル
        // 結果は "あいう" + 末尾空白除去
        let text = pane_row_text(&grid, 0);
        assert!(text.starts_with("あいう"), "unexpected: {:?}", text);
        assert!(!text.ends_with(' '), "trailing space remains: {:?}", text);
    }

    /// T4: 範囲外の行を要求しても panic せず " " を返す
    #[test]
    fn pane_row_text_handles_out_of_range_row() {
        let grid = grid_from_lines(&["hello"]);
        assert_eq!(pane_row_text(&grid, 100), " ");
    }

    /// T5: ペイン行 NodeId はペイン NodeId と衝突しない
    #[test]
    fn pane_row_node_id_no_collision_with_pane() {
        let pane_min = pane_node_id(0).0;
        let pane_max = pane_node_id(u32::MAX).0;
        let row_min = pane_row_node_id(0, 0).0;
        assert!(
            pane_max < row_min,
            "ペイン範囲 [{}, {}] と行範囲 [{}, ...] が衝突する",
            pane_min,
            pane_max,
            row_min
        );
    }

    /// T6: ペイン行 NodeId はタブ NodeId と衝突しない
    #[test]
    fn pane_row_node_id_no_collision_with_tab() {
        let tab_max = tab_node_id(u32::MAX).0;
        let row_min = pane_row_node_id(0, 0).0;
        assert!(tab_max < row_min);
    }

    /// T7: pane_row_node_id ↔ decode_node_id の往復が成立する
    #[test]
    fn decode_pane_row_roundtrip() {
        for (pane_id, row) in [(0u32, 0u16), (42, 7), (1234, 23), (u32::MAX, 999)] {
            let id = pane_row_node_id(pane_id, row);
            match decode_node_id(id) {
                NodeIdKind::PaneRow { pane_id: p, row: r } => {
                    assert_eq!(
                        p, pane_id,
                        "pane_id round-trip failed for ({}, {})",
                        pane_id, row
                    );
                    assert_eq!(r, row, "row round-trip failed for ({}, {})", pane_id, row);
                }
                other => panic!(
                    "decode_node_id returned non-PaneRow variant: {:?} for ({}, {})",
                    other, pane_id, row
                ),
            }
        }
    }

    /// T8: build_tree_from_state が各ペインの行ノードを子として含む
    #[test]
    fn build_tree_includes_pane_rows() {
        let mut state = ClientState::new(10, 5, 1000);
        // ペイン 1 を 5 行 10 列で追加
        let mut pane = crate::state::PaneState::new(10, 5, 1000);
        pane.title = "test".to_string();
        // 行 0 に "hello" を書き込む
        for (c, ch) in "hello".chars().enumerate() {
            pane.grid.set(
                c as u16,
                0,
                nexterm_proto::Cell {
                    ch,
                    fg: nexterm_proto::Color::Default,
                    bg: nexterm_proto::Color::Default,
                    attrs: nexterm_proto::Attrs::default(),
                },
            );
        }
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        // 5 行ぶんの PaneRow ノードがツリーに含まれている
        let row_node_count = update
            .nodes
            .iter()
            .filter(|(id, _)| matches!(decode_node_id(*id), NodeIdKind::PaneRow { pane_id: 1, .. }))
            .count();
        assert_eq!(
            row_node_count, 5,
            "5 行ぶんの PaneRow ノードが含まれていない"
        );

        // 行 0 のノードに "hello" が value として設定されている
        let row0_id = pane_row_node_id(1, 0);
        let row0_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == row0_id)
            .map(|(_, n)| n)
            .expect("行 0 のノードが見つからない");
        assert_eq!(row0_node.value(), Some("hello"));
    }

    /// T9: フォーカスペインの行ノードのみ Live::Polite が設定される
    #[test]
    fn build_tree_focused_pane_has_live_polite() {
        let mut state = ClientState::new(5, 2, 1000);
        let pane1 = crate::state::PaneState::new(5, 2, 1000);
        let pane2 = crate::state::PaneState::new(5, 2, 1000);
        state.panes.insert(1, pane1);
        state.panes.insert(2, pane2);
        state.tab_order = vec![1, 2];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        // ペイン 1 (focused) の行ノードは Live::Polite
        let row1_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == pane_row_node_id(1, 0))
            .map(|(_, n)| n)
            .expect("ペイン 1 行 0 が見つからない");
        assert_eq!(row1_node.live(), Some(Live::Polite));

        // ペイン 2 (non-focused) の行ノードは Off（デフォルト）
        let row2_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == pane_row_node_id(2, 0))
            .map(|(_, n)| n)
            .expect("ペイン 2 行 0 が見つからない");
        // 非フォーカスペインは live が未設定 = None
        assert_eq!(row2_node.live(), None);
    }

    /// T10: compute_grid_row_hashes が行内容変化を検知する
    #[test]
    fn compute_grid_row_hashes_detects_change() {
        let mut grid = grid_from_lines(&["hello", "world", "     "]);
        let baseline = compute_grid_row_hashes(&grid);
        assert_eq!(baseline.len(), 3);

        // 同じ grid なら同じハッシュ
        let same = compute_grid_row_hashes(&grid);
        assert_eq!(baseline, same);

        // 行 1 の 1 セルだけ変更
        grid.set(
            0,
            1,
            nexterm_proto::Cell {
                ch: 'W',
                fg: nexterm_proto::Color::Default,
                bg: nexterm_proto::Color::Default,
                attrs: nexterm_proto::Attrs::default(),
            },
        );
        let after = compute_grid_row_hashes(&grid);
        assert_eq!(after.len(), 3);
        // 行 0 / 行 2 は変わらず、行 1 のみ変化
        assert_eq!(after[0], baseline[0], "行 0 は変わらないはず");
        assert_ne!(after[1], baseline[1], "行 1 は変化するはず");
        assert_eq!(after[2], baseline[2], "行 2 は変わらないはず");
    }
}
