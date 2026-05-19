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

use accesskit::{Node, NodeId, Role, Tree, TreeId, TreeUpdate};

use crate::host_manager::HostManager;
use crate::macro_picker::MacroPicker;
use crate::palette::CommandPalette;
use crate::settings_panel::SettingsPanel;
use crate::state::{ClientState, CloseWindowDialog, ContextMenu};

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

/// Quick Select オーバーレイ
#[allow(dead_code)] // Phase 5-11-2 Step 2-2 では Quick Select は範囲外。Step 2-2-h で対応
pub const QUICK_SELECT_ID: NodeId = NodeId(11);

/// コマンドパレットの検索入力フィールド
pub const PALETTE_SEARCH_ID: NodeId = NodeId(12);

/// コマンドパレットの候補リスト
pub const PALETTE_LIST_ID: NodeId = NodeId(13);

/// 確認ダイアログの「閉じる/プロセスを終了」ボタン
pub const CLOSE_DIALOG_KILL_BTN: NodeId = NodeId(14);

/// 確認ダイアログの「キャンセル」ボタン
pub const CLOSE_DIALOG_CANCEL_BTN: NodeId = NodeId(15);

// 16〜99 は将来の固定オーバーレイ（SettingsPanel カテゴリタブ等）用に予約

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

/// pane_id（u32）からタブノードの NodeId を計算する。
pub fn tab_node_id(pane_id: u32) -> NodeId {
    NodeId(NODE_ID_TAB_OFFSET + pane_id as u64)
}

/// pane_id（u32）からペイン（ターミナル）ノードの NodeId を計算する。
pub fn pane_node_id(pane_id: u32) -> NodeId {
    NodeId(NODE_ID_PANE_OFFSET + pane_id as u64)
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
    /// 未知 / 範囲外の NodeId
    Unknown,
}

/// `NodeId` から `NodeIdKind` を逆引きする（Step 2-4）。
///
/// オフセット範囲表（`accessibility.rs` 冒頭の定数と整合）:
///
/// | 範囲 | 種別 |
/// |---|---|
/// | 1〜15 | 固定ノード |
/// | 16〜99 | 予約 |
/// | 100M..200M | `PaletteItem { idx: id - 100M }` |
/// | 200M..300M | `HostItem { idx: id - 200M }` |
/// | 300M..400M | `MacroItem { idx: id - 300M }` |
/// | 400M..500M | `ContextItem { idx: id - 400M }` |
/// | 1G..1G+u32::MAX | `Tab { pane_id: id - 1G }` |
/// | 10G..10G+u32::MAX | `Pane { pane_id: id - 10G }` |
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
    if let Some(dialog) = &state.close_window_dialog {
        let (overlay_nodes, overlay_focus) = build_close_dialog_nodes(dialog);
        nodes.extend(overlay_nodes);
        root_children.push(CLOSE_DIALOG_ID);
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

    // ===== 各ペインノード =====
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
        let mut pane_node = Node::new(Role::Terminal);
        pane_node.set_label(title);
        if let Some(cwd) = &pane.cwd {
            pane_node.set_description(format!("作業ディレクトリ: {}", cwd));
        }
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

/// SettingsPanel のノード群を構築する（Step 2-2-e、最小実装）。
///
/// 設定パネル内部のフィールド構造は非常に多岐にわたるため、Step 2-2-e の本最小実装では
/// 現在のカテゴリ名のラベルのみ提供する。Step 2-2-e' 以降で各フィールド
/// （CheckBox / TextInput / ComboBox）を展開する。
fn build_settings_panel_nodes(panel: &SettingsPanel) -> (Vec<(NodeId, Node)>, NodeId) {
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("設定");
    dialog.set_modal();
    // 現在表示中のカテゴリを description として伝える（SR で「設定: フォント」と読み上げ）
    dialog.set_description(format!("カテゴリ: {}", panel.category.label()));

    let nodes = vec![(SETTINGS_PANEL_ID, dialog)];
    (nodes, SETTINGS_PANEL_ID)
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
        // SettingsCategory は Hash 未実装のため label() 文字列で代用
        state.settings_panel.category.label().hash(&mut h);
    }

    // === update_banner（非モーダル）===
    state.update_banner.hash(&mut h);

    h.finish()
}

#[cfg(test)]
mod tests {
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
        // 異なるオーバーレイの ID 範囲が交差しないこと（10万件まで安全と想定）
        const ITEM_CAP: u64 = 100_000_000; // 各オフセット間の差
        const _: () = assert!(NODE_ID_HOST_ITEM_OFFSET - NODE_ID_PALETTE_ITEM_OFFSET >= ITEM_CAP);
        const _: () = assert!(NODE_ID_MACRO_ITEM_OFFSET - NODE_ID_HOST_ITEM_OFFSET >= ITEM_CAP);
        const _: () = assert!(NODE_ID_CONTEXT_ITEM_OFFSET - NODE_ID_MACRO_ITEM_OFFSET >= ITEM_CAP);
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

        assert_eq!(update.nodes.len(), 5);
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

        assert_eq!(update.nodes.len(), 9);
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

        assert_eq!(update.nodes.len(), 5);
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
        assert_eq!(decode_node_id(NodeId(16)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(99)), NodeIdKind::Unknown);
        // 予約: 500M / 600M は QuickSelect / SettingsField で将来使う
        assert_eq!(decode_node_id(NodeId(500_000_000)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(999_999_999)), NodeIdKind::Unknown);
        // タブとペインの間の隙間（5.3e9〜1e10）も Unknown
        assert_eq!(decode_node_id(NodeId(7_000_000_000)), NodeIdKind::Unknown);
        // 1e10 + u32::MAX を超えた範囲
        assert_eq!(decode_node_id(NodeId(20_000_000_000)), NodeIdKind::Unknown);
    }
}
