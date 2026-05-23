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

use accesskit::{Live, Node, NodeId, Role, TextPosition, TextSelection, Tree, TreeId, TreeUpdate};

use crate::host_manager::HostManager;
use crate::macro_picker::MacroPicker;
use crate::palette::CommandPalette;
use crate::settings_panel::SettingsPanel;
use crate::state::{
    AlertEntry, AlertKind, ClientState, CloseWindowDialog, ContextMenu, QuickSelectState,
};

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

/// SR 向けアラート領域のルート（Sprint 5-11-5）。
///
/// Bell / OSC 9 / OSC 777 を `Role::Alert` で公開するためのコンテナ。
/// ROOT の子として常に存在し、配下の各 Alert ノードを SR がアナウンスする。
/// `Live::Assertive` を設定して新規アラートを即時アナウンス対象とする。
pub const ALERT_REGION_ID: NodeId = NodeId(26);

/// ターミナル入力バッファ（Sprint 5-11-7、Phase 5-11-7）。
///
/// `PANE_AREA_ID` の末尾の子として常に存在する単一の `Role::TextInput` ノード。
/// SR ユーザーがここに `SetValue` で文字列を書き込むと、フォーカスペインに対して
/// `PasteText` IPC を送信して PTY へ転送する。書き込み完了後は `value` を空文字列に
/// 戻して再入力可能にする。
///
/// 設計理由（Q2 (b) 採用）:
/// - 表示用の TextRun 行（`PaneRow` / `PaneScrollbackRow`）と入力用の TextInput を
///   セマンティクス的に分離し、AccessKit ツリーの責務を明確化
/// - `Role::Terminal` の SetValue 動作は AccessKit 0.24 で標準化されていないため、
///   汎用 `Role::TextInput` で代替
/// - 改行を含む複数行入力も `\n` をそのまま PTY へ転送可能
pub const PANE_INPUT_BUFFER_ID: NodeId = NodeId(27);

// 28〜29 は将来のコンテナ（サイドバー等）用に予約

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

/// Phase 5-11-6 #6 - Window カテゴリ: カーソル形状（block / beam / underline）
pub const SETTINGS_CURSOR_STYLE_ID: NodeId = NodeId(36);

/// Phase 5-11-6 #6 - Window カテゴリ: 水平パディング (0〜32 px)
pub const SETTINGS_PADDING_X_ID: NodeId = NodeId(37);

/// Phase 5-11-6 #6 - Window カテゴリ: 垂直パディング (0〜32 px)
pub const SETTINGS_PADDING_Y_ID: NodeId = NodeId(38);

/// Phase 5-11-6 #6 - Window カテゴリ: GPU プレゼンテーションモード（fifo / mailbox / auto）
pub const SETTINGS_PRESENT_MODE_ID: NodeId = NodeId(39);

// 40〜99 は将来のフィールド（SSH / Keybindings / Profiles など）用に予約

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
const NODE_ID_QUICKSELECT_ITEM_OFFSET: u64 = 500_000_000;

/// SettingsPanel Profiles カテゴリの動的項目（`600_000_000 + idx`、Phase 5-11-7）。
///
/// `SettingsPanel.profiles` の各 `ProfileEntry` を `Role::ListBoxOption` として
/// 公開する。`selected_profile` で選択中の項目を判定する。
///
/// 値域は `[600_000_000, 700_000_000)`。プロファイル数の現実的上限を考えると
/// 10M 範囲で十分。`NODE_ID_TAB_OFFSET = 1e9` との間に 300M の余裕がある。
const NODE_ID_SETTINGS_PROFILE_OFFSET: u64 = 600_000_000;

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

/// SR 向けアラート個別ノードの NodeId 計算用オフセット（Sprint 5-11-5）。
///
/// 内部表現: `NODE_ID_ALERT_OFFSET + AlertEntry.seq`。
///
/// **値域選定の根拠**:
/// - ペイン行範囲 = `[2e10, 2e10 + u32::MAX × 10000 + 10000] ≈ [2e10, 4.3e13]`
///   が pane_row / pane_scrollback で連続使用される
/// - その上限を超えた安全な値域として `50e12` (50 兆) を採用
/// - `ClientState.next_alert_seq` の現実的な上限は 1 秒間 1000 件発火でも
///   約 5.84 億年に渡るため、`u64::MAX` まで安全に伸ばせる
const NODE_ID_ALERT_OFFSET: u64 = 50_000_000_000_000;

/// `AlertEntry.seq` から Alert ノードの NodeId を計算する（Sprint 5-11-5）。
pub fn alert_node_id(seq: u64) -> NodeId {
    NodeId(NODE_ID_ALERT_OFFSET + seq)
}

/// ペイン行ノードの NodeId 計算用オフセット（Sprint 5-11-3 / 5-11-4）。
///
/// ペイン本体ノードの子として、ターミナルグリッドの各行を `Role::TextRun` で公開する。
/// 内部表現: `NODE_ID_PANE_ROW_OFFSET + pane_id as u64 * MAX_ROWS_PER_PANE + row_offset`。
///
/// `row_offset` 内訳:
/// - `0..MAX_VIEWPORT_ROWS_PER_PANE` (0..1000): ビューポート行（`pane_row_node_id`）
/// - `MAX_VIEWPORT_ROWS_PER_PANE..MAX_ROWS_PER_PANE` (1000..10000):
///   スクロールバック行（Sprint 5-11-4、`pane_scrollback_row_node_id`）
///
/// 値域: `[2e10, 2e10 + u32::MAX * 10000 + 9999] ≈ [2e10, 4.3e13]`。
/// `NODE_ID_PANE_OFFSET` の上限 ≈ 1.43e10 との間に十分なギャップがある。
const NODE_ID_PANE_ROW_OFFSET: u64 = 20_000_000_000;

/// 1 ペインあたりの最大行数（Sprint 5-11-3 → 5-11-4 で 1000 → 10000 に拡張）。
///
/// 内訳:
/// - `0..MAX_VIEWPORT_ROWS_PER_PANE` (0..1000): ターミナルのビューポート行
/// - `MAX_VIEWPORT_ROWS_PER_PANE..MAX_ROWS_PER_PANE` (1000..10000): スクロールバック行（Sprint 5-11-4）
///
/// 実用上のターミナル行数は 200 行程度、スクロールバックは数千行が一般的。
/// この値を超える行は SR から不可視となるが、現実的な表示行数では発生しない。
pub const MAX_ROWS_PER_PANE: u64 = 10_000;

/// 1 ペインあたりのビューポート（grid）公開行数の上限（Sprint 5-11-4）。
///
/// `pane_row_node_id` で割り当てられる行 NodeId のうち、
/// `0..MAX_VIEWPORT_ROWS_PER_PANE` を占める範囲がビューポート行。
pub const MAX_VIEWPORT_ROWS_PER_PANE: u64 = 1_000;

/// 1 ペインあたりのスクロールバック公開行数の上限（Sprint 5-11-4）。
///
/// `pane_scrollback_row_node_id` で割り当てられる NodeId が占める範囲。
/// `MAX_ROWS_PER_PANE - MAX_VIEWPORT_ROWS_PER_PANE` と一致する。
pub const MAX_SCROLLBACK_ROWS_PER_PANE: u64 = MAX_ROWS_PER_PANE - MAX_VIEWPORT_ROWS_PER_PANE;

/// スクロールバックを SR に公開する窓スライドの半径（Sprint 5-11-4）。
///
/// 現在のスクロール位置を中心に前後 `SCROLLBACK_WINDOW_RADIUS` 行を AccessKit ツリーに含める。
/// 一般的なターミナルのスクロールバックは数千行に達するため、全行公開はパフォーマンス上不利。
/// 100 行の窓は SR の矢印キーナビゲーションを違和感なく支える十分な範囲。
pub const SCROLLBACK_WINDOW_RADIUS: usize = 100;

/// pane_id（u32）からタブノードの NodeId を計算する。
pub fn tab_node_id(pane_id: u32) -> NodeId {
    NodeId(NODE_ID_TAB_OFFSET + pane_id as u64)
}

/// pane_id（u32）からペイン（ターミナル）ノードの NodeId を計算する。
pub fn pane_node_id(pane_id: u32) -> NodeId {
    NodeId(NODE_ID_PANE_OFFSET + pane_id as u64)
}

/// pane_id × row_idx からビューポート行ノードの NodeId を計算する（Sprint 5-11-3）。
///
/// `row` が [`MAX_VIEWPORT_ROWS_PER_PANE`] 以上の場合は NodeId が衝突する可能性があるため、
/// 呼び出し側で `row < MAX_VIEWPORT_ROWS_PER_PANE` を保証すること。
pub fn pane_row_node_id(pane_id: u32, row: u16) -> NodeId {
    debug_assert!((row as u64) < MAX_VIEWPORT_ROWS_PER_PANE);
    NodeId(NODE_ID_PANE_ROW_OFFSET + (pane_id as u64) * MAX_ROWS_PER_PANE + row as u64)
}

/// pane_id × scrollback_idx からスクロールバック行ノードの NodeId を計算する（Sprint 5-11-4）。
///
/// スクロールバック行 NodeId は同一ペインのビューポート行 NodeId と連続した空間に配置される:
/// `pane_row` 範囲 = `[base, base + MAX_VIEWPORT_ROWS_PER_PANE)`,
/// `pane_scrollback` 範囲 = `[base + MAX_VIEWPORT_ROWS_PER_PANE, base + MAX_ROWS_PER_PANE)`
/// （ここで `base = NODE_ID_PANE_ROW_OFFSET + pane_id * MAX_ROWS_PER_PANE`）。
///
/// `scrollback_idx` が [`MAX_SCROLLBACK_ROWS_PER_PANE`] 以上の場合は次ペインの行 NodeId と
/// 衝突する可能性があるため、呼び出し側で `scrollback_idx < MAX_SCROLLBACK_ROWS_PER_PANE` を保証すること。
pub fn pane_scrollback_row_node_id(pane_id: u32, scrollback_idx: u16) -> NodeId {
    debug_assert!((scrollback_idx as u64) < MAX_SCROLLBACK_ROWS_PER_PANE);
    NodeId(
        NODE_ID_PANE_ROW_OFFSET
            + (pane_id as u64) * MAX_ROWS_PER_PANE
            + MAX_VIEWPORT_ROWS_PER_PANE
            + scrollback_idx as u64,
    )
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

/// セル列を SR 向けテキスト + `character_lengths` に変換する内部ヘルパー（Sprint 5-11-4）。
///
/// 戻り値 `(text, lengths)`:
/// - `text`: `pane_row_text` と同じロジックで生成（trim_end + 空行は `" "`）
/// - `lengths`: `text` 中の各 `char` の UTF-8 バイト長配列。
///   `lengths.iter().map(|&b| b as usize).sum::<usize>() == text.len()` が常に成り立つ。
///
/// AccessKit `Node::set_character_lengths` の仕様に従い「1 文字 = 1 character」とし、
/// 全角・絵文字も 1 character として扱う（半角と統一）。半角・全角の幅の違いは
/// `character_widths` で表現すべきだが本実装では省略（SR 動作には十分）。
fn cells_to_row_text_with_lengths(cells: &[nexterm_proto::Cell]) -> (String, Vec<u8>) {
    let mut text: String = cells.iter().map(|c| c.ch).collect();
    let trimmed_len_bytes = text.trim_end_matches(' ').len();
    if trimmed_len_bytes == 0 {
        // 空行は " " で SR の境界を保つ
        return (" ".to_string(), vec![1]);
    }
    text.truncate(trimmed_len_bytes);
    let lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
    (text, lengths)
}

/// `Grid` の指定行を SR 向けテキスト + `character_lengths` に変換する（Sprint 5-11-4）。
///
/// `pane_row_text` の text 部分と同じ結果になる。AccessKit の `Role::TextRun` ノード
/// に `set_value` / `set_character_lengths` を設定する際に使用する。
pub fn pane_row_text_with_lengths(grid: &nexterm_proto::Grid, row: usize) -> (String, Vec<u8>) {
    let Some(cells) = grid.rows.get(row) else {
        return (" ".to_string(), vec![1]);
    };
    cells_to_row_text_with_lengths(cells)
}

/// スクロールバック 1 行を SR 向けテキスト + `character_lengths` に変換する（Sprint 5-11-4）。
///
/// `pane_row_text_with_lengths` と同じセル → テキスト変換ロジックを使用。
pub fn scrollback_row_text_with_lengths(line: &[nexterm_proto::Cell]) -> (String, Vec<u8>) {
    cells_to_row_text_with_lengths(line)
}

/// セル列 `cursor_col` から AccessKit `TextPosition::character_index` を計算する（Sprint 5-11-4）。
///
/// 仕様:
/// - 行テキストはセル列と 1:1 対応で構築される (`cells.iter().map(|c| c.ch).collect()`)。
/// - `cursor_col` は grid のセル列。`text.chars().count()` を超える場合は末尾位置にクランプ。
/// - 全角文字の placeholder セル (' ') も 1 文字としてカウントされるため、
///   grid 上で cursor_col が指すセル列はそのまま character_index として使える。
///
/// 例:
/// - text="abc" (chars=3), cursor_col=1 → 1
/// - text="abc" (chars=3), cursor_col=5 → 3（末尾にクランプ）
/// - text="あい" (chars=2、placeholder 含めるとセル幅 4), cursor_col=2 → 2
pub fn cursor_character_index(text: &str, cursor_col: u16) -> usize {
    let char_count = text.chars().count();
    (cursor_col as usize).min(char_count)
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

/// SettingsPanel Profiles カテゴリ項目 idx から NodeId を計算する（Phase 5-11-7）。
fn settings_profile_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_SETTINGS_PROFILE_OFFSET + idx as u64)
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
    /// Phase 5-11-6 #6: 設定パネル: カーソル形状（block / beam / underline）
    SettingsCursorStyle,
    /// Phase 5-11-6 #6: 設定パネル: 水平パディング (0〜32 px) スライダー
    SettingsPaddingX,
    /// Phase 5-11-6 #6: 設定パネル: 垂直パディング (0〜32 px) スライダー
    SettingsPaddingY,
    /// Phase 5-11-6 #6: 設定パネル: GPU プレゼンテーションモード（fifo / mailbox / auto）
    SettingsPresentMode,
    /// ペイン行ノード（Sprint 5-11-3、`pane_id` と `row` で識別）
    PaneRow { pane_id: u32, row: u16 },
    /// ペインのスクロールバック行ノード（Sprint 5-11-4、`pane_id` と
    /// `idx`（スクロールバック先頭からのインデックス）で識別）
    PaneScrollbackRow { pane_id: u32, idx: u16 },
    /// SR 向けアラート領域コンテナ（Sprint 5-11-5）
    AlertRegion,
    /// SR 向けアラート個別ノード（Sprint 5-11-5、`AlertEntry.seq` で識別）
    Alert { seq: u64 },
    /// Phase 5-11-7: ターミナル入力バッファ（フォーカスペインへの PTY 書き込み用）
    PaneInputBuffer,
    /// Phase 5-11-7: SettingsPanel Profiles カテゴリの動的項目（`idx` は `SettingsPanel.profiles` のインデックス）
    SettingsProfileItem { idx: usize },
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
/// | 26 | `AlertRegion`（Sprint 5-11-5） |
/// | 27 | `PaneInputBuffer`（Phase 5-11-7） |
/// | 28〜29 | 予約 |
/// | 30〜35 | 設定フィールド（FontFamily / FontSize / ThemeScheme / WindowOpacity / StartupLanguage / StartupAutoUpdate） |
/// | 36〜39 | 設定フィールド Phase 5-11-6 #6（CursorStyle / PaddingX / PaddingY / PresentMode） |
/// | 40〜99 | 予約 |
/// | 100M..200M | `PaletteItem { idx: id - 100M }` |
/// | 200M..300M | `HostItem { idx: id - 200M }` |
/// | 300M..400M | `MacroItem { idx: id - 300M }` |
/// | 400M..500M | `ContextItem { idx: id - 400M }` |
/// | 500M..600M | `QuickSelectItem { idx: id - 500M }` |
/// | 600M..700M | `SettingsProfileItem { idx: id - 600M }`（Phase 5-11-7） |
/// | 700M..1G | 予約（将来の SettingsField 動的展開用） |
/// | 1G..1G+u32::MAX | `Tab { pane_id: id - 1G }` |
/// | 10G..10G+u32::MAX | `Pane { pane_id: id - 10G }` |
/// | 20G..~4.3T | `PaneRow` / `PaneScrollbackRow`（Sprint 5-11-3 / 5-11-4） |
/// | 50T..u64::MAX | `Alert { seq: id - 50T }`（Sprint 5-11-5） |
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
        26 => NodeIdKind::AlertRegion,
        // Phase 5-11-7: ターミナル入力バッファ
        27 => NodeIdKind::PaneInputBuffer,
        30 => NodeIdKind::SettingsFontFamily,
        31 => NodeIdKind::SettingsFontSize,
        32 => NodeIdKind::SettingsThemeScheme,
        33 => NodeIdKind::SettingsWindowOpacity,
        34 => NodeIdKind::SettingsStartupLanguage,
        35 => NodeIdKind::SettingsStartupAutoUpdate,
        // Phase 5-11-6 #6: Window カテゴリの 4 新フィールド
        36 => NodeIdKind::SettingsCursorStyle,
        37 => NodeIdKind::SettingsPaddingX,
        38 => NodeIdKind::SettingsPaddingY,
        39 => NodeIdKind::SettingsPresentMode,
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
    // Phase 5-11-7: SettingsPanel Profiles 項目範囲: [600M, 700M)
    if (NODE_ID_SETTINGS_PROFILE_OFFSET..NODE_ID_SETTINGS_PROFILE_OFFSET + DYN_RANGE).contains(&raw)
    {
        return NodeIdKind::SettingsProfileItem {
            idx: (raw - NODE_ID_SETTINGS_PROFILE_OFFSET) as usize,
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
    // ペイン行範囲（Sprint 5-11-3 + 5-11-4）:
    //   [2e10, 2e10 + u32::MAX * MAX_ROWS_PER_PANE + (MAX_ROWS_PER_PANE - 1)]
    // 内部レイアウト（pane_id 単位）:
    //   - offset 0..MAX_VIEWPORT_ROWS_PER_PANE (0..1000): ビューポート行 → PaneRow
    //   - offset MAX_VIEWPORT_ROWS_PER_PANE..MAX_ROWS_PER_PANE (1000..10000):
    //     スクロールバック行 → PaneScrollbackRow
    let pane_row_range_end =
        NODE_ID_PANE_ROW_OFFSET + (u32::MAX as u64) * MAX_ROWS_PER_PANE + MAX_ROWS_PER_PANE;
    if (NODE_ID_PANE_ROW_OFFSET..pane_row_range_end).contains(&raw) {
        let normalized = raw - NODE_ID_PANE_ROW_OFFSET;
        let pane_id = (normalized / MAX_ROWS_PER_PANE) as u32;
        let offset_in_pane = normalized % MAX_ROWS_PER_PANE;
        if offset_in_pane < MAX_VIEWPORT_ROWS_PER_PANE {
            return NodeIdKind::PaneRow {
                pane_id,
                row: offset_in_pane as u16,
            };
        } else {
            return NodeIdKind::PaneScrollbackRow {
                pane_id,
                idx: (offset_in_pane - MAX_VIEWPORT_ROWS_PER_PANE) as u16,
            };
        }
    }
    // SR アラート範囲（Sprint 5-11-5）: [50T, u64::MAX]。
    // `next_alert_seq` の実用上限が遥か上なので、上限は事実上 u64::MAX。
    // ペイン行範囲の上限 `pane_row_range_end` (~4.3e13) と十分に離れているため衝突なし。
    if raw >= NODE_ID_ALERT_OFFSET {
        return NodeIdKind::Alert {
            seq: raw - NODE_ID_ALERT_OFFSET,
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

    // ===== 非モーダル: SR アラート領域（Sprint 5-11-5） =====
    // 空のときは含めない（SR の混乱回避）。
    // Bell / OSC 9 / OSC 777 は `ClientState::add_alert` でキュー追加され、TTL 経過後に
    // `expire_alerts` で除去されるため、ここでは現在のスナップショットを反映するだけでよい。
    let alert_nodes = build_alert_region_nodes(&state.alerts);
    if !alert_nodes.is_empty() {
        nodes.extend(alert_nodes);
        root_children.push(ALERT_REGION_ID);
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
    //
    // Phase 5-11-7: ペイン本体に加えて末尾に PANE_INPUT_BUFFER_ID を追加し、
    // SR ユーザーが SetValue で PTY へ書き込めるようにする。
    let mut pane_area = Node::new(Role::Group);
    pane_area.set_label("ペイン");
    let mut pane_child_ids: Vec<NodeId> = tab_order.iter().copied().map(pane_node_id).collect();
    pane_child_ids.push(PANE_INPUT_BUFFER_ID);
    pane_area.set_children(pane_child_ids);

    // ===== 各ペインノード + ペイン行ノード（Sprint 5-11-3 / 5-11-4） =====
    //
    // ペインの子として以下を並べる:
    //   1. スクロールバック行ノード（Sprint 5-11-4、`Role::TextRun`）
    //      - 公開範囲: `pane.scroll_offset` 中心の前後 `SCROLLBACK_WINDOW_RADIUS` 行
    //      - Live::Off（明示せず）: アナウンス対象外
    //   2. ビューポート行ノード（Sprint 5-11-3 / 5-11-4、`Role::TextRun`）
    //      - フォーカスペインのカーソル行のみ `Live::Polite`（過剰アナウンス抑止）
    //
    // ペイン本体ノード（`Role::Terminal`）にはフォーカスペインのカーソル位置を
    // `TextSelection` で設定する（Sprint 5-11-4）。SR ユーザーはキャレットの位置を
    // 行 NodeId + character_index で知ることができ、矢印キーで読み上げが進む。
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
        let cursor_row = pane.grid.cursor_row;
        let cursor_col = pane.grid.cursor_col;

        let mut child_ids: Vec<NodeId> = Vec::new();
        let mut pane_text_selection: Option<TextSelection> = None;

        // ----- スクロールバック行ノード（窓スライド、Sprint 5-11-4） -----
        let scrollback_len = pane.scrollback.len();
        if scrollback_len > 0 {
            // 窓中心: ビューポート直前のスクロールバック行（最新側）。
            // `scroll_offset = 0` は最新画面、`scroll_offset = K` は K 行上にスクロール済。
            let center = scrollback_len.saturating_sub(pane.scroll_offset.saturating_add(1));
            let start = center.saturating_sub(SCROLLBACK_WINDOW_RADIUS);
            let end = (center + SCROLLBACK_WINDOW_RADIUS + 1)
                .min(scrollback_len)
                .min(MAX_SCROLLBACK_ROWS_PER_PANE as usize);
            for idx in start..end {
                let Some(line) = pane.scrollback.get(idx) else {
                    continue;
                };
                let (text, lengths) = scrollback_row_text_with_lengths(line);
                let mut row_node = Node::new(Role::TextRun);
                row_node.set_value(text);
                row_node.set_character_lengths(lengths);
                // スクロールバック行は Live::Off（デフォルト）でアナウンス対象外
                let row_id = pane_scrollback_row_node_id(pane_id, idx as u16);
                child_ids.push(row_id);
                pane_nodes.push((row_id, row_node));
            }
        }

        // ----- ビューポート行ノード（Sprint 5-11-3 / 5-11-4 Role::TextRun 化） -----
        let row_count = (pane.grid.height as u64)
            .min(pane.grid.rows.len() as u64)
            .min(MAX_VIEWPORT_ROWS_PER_PANE) as u16;
        for row in 0..row_count {
            let (text, lengths) = pane_row_text_with_lengths(&pane.grid, row as usize);
            let is_cursor_row = is_focused_pane && row == cursor_row;
            let char_index_for_cursor = cursor_character_index(&text, cursor_col);

            let mut row_node = Node::new(Role::TextRun);
            row_node.set_value(text);
            row_node.set_character_lengths(lengths);
            // Sprint 5-11-4: Live::Polite はフォーカスペインのカーソル行のみに限定。
            // ビューポート全行を Polite にすると SR が画面再描画ごとに大量アナウンスしてしまう。
            if is_cursor_row {
                row_node.set_live(Live::Polite);
            }
            let row_id = pane_row_node_id(pane_id, row);

            // フォーカスペインのカーソル行: TextSelection をペイン側に設定するための情報を残す
            if is_cursor_row {
                pane_text_selection = Some(TextSelection {
                    anchor: TextPosition {
                        node: row_id,
                        character_index: char_index_for_cursor,
                    },
                    focus: TextPosition {
                        node: row_id,
                        character_index: char_index_for_cursor,
                    },
                });
            }

            child_ids.push(row_id);
            pane_nodes.push((row_id, row_node));
        }

        let mut pane_node = Node::new(Role::Terminal);
        pane_node.set_label(title);
        if let Some(cwd) = &pane.cwd {
            pane_node.set_description(format!("作業ディレクトリ: {}", cwd));
        }
        pane_node.set_children(child_ids);
        if let Some(sel) = pane_text_selection {
            pane_node.set_text_selection(sel);
        }
        pane_nodes.push((pane_node_id(pane_id), pane_node));
    }

    let default_focus = state.focused_pane_id.map_or(ROOT_ID, pane_node_id);

    // ===== ターミナル入力バッファ（Phase 5-11-7） =====
    //
    // フォーカスペインの情報を description に含め、SR ユーザーにどのペインに対する
    // 入力かを示す。SetValue 受信時に `PasteText` IPC でフォーカスペインへ転送する。
    let mut input_buffer = Node::new(Role::TextInput);
    input_buffer.set_label("ターミナル入力バッファ");
    input_buffer.set_value("");
    let pane_hint = state
        .focused_pane_id
        .and_then(|pid| state.panes.get(&pid))
        .map(|p| {
            if p.title.is_empty() {
                format!("Pane {}", state.focused_pane_id.unwrap_or(0))
            } else {
                p.title.clone()
            }
        })
        .unwrap_or_else(|| "フォーカスペインなし".to_string());
    input_buffer.set_description(format!(
        "現在のペイン: {} — 入力した文字列を確定すると PTY へ送信されます（改行は \\n で送信可能）",
        pane_hint
    ));

    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(4 + tab_nodes.len() + pane_nodes.len());
    nodes.push((ROOT_ID, root));
    nodes.push((TAB_BAR_ID, tab_bar));
    nodes.push((PANE_AREA_ID, pane_area));
    nodes.extend(tab_nodes);
    nodes.extend(pane_nodes);
    nodes.push((PANE_INPUT_BUFFER_ID, input_buffer));

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
///        ├─ CheckBox "起動時に更新確認" (Startup カテゴリのみ)
///        ├─ ListBox "プロファイル一覧" (Profiles カテゴリのみ、Phase 5-11-7)
///        │    └─ ListBoxOption × N
///        ├─ (Ssh カテゴリのみ、Phase 5-11-7): 案内文を description で公開（フィールドなし）
///        └─ (Keybindings カテゴリのみ、Phase 5-11-7): 案内文を description で公開（フィールドなし）
/// ```
///
/// フォーカス: font_family_editing 中はそのフィールド、Window カテゴリは
/// `window_field_focus` に応じて、それ以外は現在カテゴリのタブ。
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
    // SSH / Keybindings 等のフィールドなしカテゴリ向けに、コンテンツ Group の
    // description に案内文を入れる。デフォルトは None（変更がなければそのまま）。
    let mut content_description: Option<String> = None;

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
            // Phase 5-11-6 #6: 5 フィールド構成
            //   0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode
            let mut opacity = Node::new(Role::Slider);
            opacity.set_label("背景不透明度");
            opacity.set_value(format!("{:.0}%", panel.opacity * 100.0));
            opacity.set_numeric_value(panel.opacity as f64);
            opacity.set_min_numeric_value(0.1);
            opacity.set_max_numeric_value(1.0);
            opacity.set_numeric_value_step(0.05);
            nodes.push((SETTINGS_WINDOW_OPACITY_ID, opacity));
            content_children.push(SETTINGS_WINDOW_OPACITY_ID);

            let mut cs = Node::new(Role::ComboBox);
            cs.set_label("カーソル形状");
            cs.set_value(panel.cursor_style_label());
            cs.set_description("←/→ で切り替え");
            nodes.push((SETTINGS_CURSOR_STYLE_ID, cs));
            content_children.push(SETTINGS_CURSOR_STYLE_ID);

            let mut px = Node::new(Role::Slider);
            px.set_label("水平パディング");
            px.set_value(format!("{} px", panel.padding_x));
            px.set_numeric_value(panel.padding_x as f64);
            px.set_min_numeric_value(0.0);
            px.set_max_numeric_value(32.0);
            px.set_numeric_value_step(1.0);
            nodes.push((SETTINGS_PADDING_X_ID, px));
            content_children.push(SETTINGS_PADDING_X_ID);

            let mut py = Node::new(Role::Slider);
            py.set_label("垂直パディング");
            py.set_value(format!("{} px", panel.padding_y));
            py.set_numeric_value(panel.padding_y as f64);
            py.set_min_numeric_value(0.0);
            py.set_max_numeric_value(32.0);
            py.set_numeric_value_step(1.0);
            nodes.push((SETTINGS_PADDING_Y_ID, py));
            content_children.push(SETTINGS_PADDING_Y_ID);

            let mut pm = Node::new(Role::ComboBox);
            pm.set_label("描画モード");
            pm.set_value(panel.present_mode_label());
            pm.set_description("←/→ で切り替え");
            nodes.push((SETTINGS_PRESENT_MODE_ID, pm));
            content_children.push(SETTINGS_PRESENT_MODE_ID);
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
        SettingsCategory::Profiles => {
            // Phase 5-11-7: プロファイル一覧を ListBox + ListBoxOption で公開する。
            // 各 ProfileEntry は `settings_profile_item_id(idx)` で識別し、
            // Click / Focus で `selected_profile` を更新する。
            if panel.profiles.is_empty() {
                content_description = Some(
                    "プロファイルがありません。nexterm.toml に [[profiles]] を追加してください"
                        .to_string(),
                );
            } else {
                let item_ids: Vec<NodeId> = (0..panel.profiles.len())
                    .map(settings_profile_item_id)
                    .collect();
                for (idx, prof) in panel.profiles.iter().enumerate() {
                    let mut item = Node::new(Role::ListBoxOption);
                    let label = if prof.icon.is_empty() {
                        prof.name.clone()
                    } else {
                        format!("{} {}", prof.icon, prof.name)
                    };
                    item.set_label(label);
                    if idx == panel.selected_profile {
                        item.set_selected(true);
                    }
                    nodes.push((settings_profile_item_id(idx), item));
                    // ListBoxOption は item_ids 経由で ListBox の子として配置するが、
                    // content_children へは ListBox 1 個のみを追加する（下記）。
                    let _ = idx; // 名前空間整理: 上の `nodes.push` で利用済
                }
                // ListBox 親ノード。`content_children` には ListBox を 1 個だけ含める。
                // ListBox 自体は固定 NodeId を使わず、便宜上 `SETTINGS_CONTENT_ID` の
                // 子として直接 ListBoxOption 群を並べる代わりに、Group の説明を簡略化する。
                //
                // Q: なぜ ListBox 専用の固定 NodeId を割り当てないか？
                // A: SETTINGS_CONTENT_ID 自体を Group → ListBox に Role 変更したいが、
                //    現在 Group は他カテゴリでも使うため不可。代わりに各 ListBoxOption を
                //    SETTINGS_CONTENT_ID の直接の子として並べる（NVDA / Orca 等の SR は
                //    Group の子の ListBoxOption も適切に読み上げる）。
                for id in &item_ids {
                    content_children.push(*id);
                }
                content_description = Some(format!(
                    "プロファイル一覧（{} 件）。↑↓ で選択、Enter で適用",
                    panel.profiles.len()
                ));
            }
        }
        SettingsCategory::Ssh => {
            // Phase 5-11-7: SSH ホストは nexterm.toml 経由のため、設定パネル内では
            // 編集できない。SR には案内文として description で公開する。
            content_description = Some(
                "SSH ホストは nexterm.toml の [[hosts]] セクションで管理します。\
                 設定パネル内では編集できません"
                    .to_string(),
            );
        }
        SettingsCategory::Keybindings => {
            // Phase 5-11-7: キーバインドも nexterm.toml 経由のため、設定パネル内では
            // 編集できない。SR には案内文として description で公開する。
            content_description = Some(
                "キーバインドは nexterm.toml の [[keys]] セクションで管理します。\
                 設定パネル内では編集できません"
                    .to_string(),
            );
        }
    }

    let mut content = Node::new(Role::Group);
    content.set_label(panel.category.label());
    if let Some(desc) = content_description {
        content.set_description(desc);
    } else if content_children.is_empty() {
        content.set_description("このカテゴリの詳細はまだ実装されていません");
    }
    content.set_children(content_children);
    nodes.push((SETTINGS_CONTENT_ID, content));

    // ===== フォーカス決定 =====
    let focus = if matches!(panel.category, SettingsCategory::Font) && panel.font_family_editing {
        SETTINGS_FONT_FAMILY_ID
    } else if matches!(panel.category, SettingsCategory::Window) {
        // Phase 5-11-6 #6: Window カテゴリは window_field_focus に応じてフィールドに焦点を当てる。
        match panel.window_field_focus {
            0 => SETTINGS_WINDOW_OPACITY_ID,
            1 => SETTINGS_CURSOR_STYLE_ID,
            2 => SETTINGS_PADDING_X_ID,
            3 => SETTINGS_PADDING_Y_ID,
            4 => SETTINGS_PRESENT_MODE_ID,
            _ => settings_tab_id_at(current_idx),
        }
    } else if matches!(panel.category, SettingsCategory::Profiles) && !panel.profiles.is_empty() {
        // Phase 5-11-7: Profiles カテゴリでは selected_profile のノードへフォーカス
        settings_profile_item_id(panel.selected_profile.min(panel.profiles.len() - 1))
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

/// SR 向けアラート領域ノード群を構築する（Sprint 5-11-5）。
///
/// ## ツリー構造
///
/// ```text
/// Group "通知" (id=ALERT_REGION_ID, live=Assertive)
///   ├─ Alert (id=alert_node_id(seq)) "ベル" / "通知: <title>"
///   │    - value: "<body>" （Bell は空、Notification は本文）
///   ├─ Alert ...
/// ```
///
/// **Live::Assertive** は領域コンテナに設定する。子ノードが追加されたタイミングで
/// SR が即座に読み上げる契約（accesskit の標準的な使い方）。
///
/// **空キュー時**: `(nodes, ids)` どちらも空を返す。呼び出し側は ALERT_REGION_ID を
/// ROOT の child に含めない（空のコンテナで SR を混乱させないため）。
///
/// 戻り値:
/// - `nodes`: ALERT_REGION 自身 + 各 Alert ノードのペア（キューが空の場合は空 Vec）
/// - `region_child_ids`: ALERT_REGION の children に設定する各 Alert NodeId 列
fn build_alert_region_nodes(
    alerts: &std::collections::VecDeque<AlertEntry>,
) -> Vec<(NodeId, Node)> {
    if alerts.is_empty() {
        return Vec::new();
    }
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(1 + alerts.len());

    // ===== 領域コンテナ =====
    let mut region = Node::new(Role::Group);
    region.set_label("通知");
    // Live::Assertive: 新規 Alert ノード追加時に SR が即時アナウンス
    region.set_live(Live::Assertive);
    let child_ids: Vec<NodeId> = alerts.iter().map(|a| alert_node_id(a.seq)).collect();
    region.set_children(child_ids);
    nodes.push((ALERT_REGION_ID, region));

    // ===== 各 Alert ノード =====
    for alert in alerts {
        let mut node = Node::new(Role::Alert);
        // ラベル: 種別 + タイトル
        let label = match alert.kind {
            AlertKind::Bell => alert.title.clone(),
            AlertKind::Notification => format!("通知: {}", alert.title),
        };
        node.set_label(label);
        // 本文（空でなければ）: SR は description として補足読み上げ
        if !alert.body.is_empty() {
            node.set_description(alert.body.clone());
        }
        nodes.push((alert_node_id(alert.seq), node));
    }

    nodes
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

    /// 単一ペインの「構造に影響する」フィールドをまとめてハッシュする内部ヘルパー。
    ///
    /// Sprint 5-11-4 で追加: `cursor_col` / `cursor_row` / `scrollback.len()` / `scroll_offset`
    /// は AccessKit ツリー構造（TextSelection 位置 / スクロールバック窓スライド範囲）に
    /// 直接影響するため、これらが変化したら全体再生成が必要。
    fn hash_pane(p: &crate::state::PaneState, h: &mut DefaultHasher) {
        p.title.hash(h);
        p.cwd.hash(h);
        // Sprint 5-11-4: カーソル位置 / スクロールバック構造
        p.grid.cursor_col.hash(h);
        p.grid.cursor_row.hash(h);
        p.scrollback.len().hash(h);
        p.scroll_offset.hash(h);
    }

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
                hash_pane(p, &mut h);
            }
        }
    } else {
        for id in &state.tab_order {
            if let Some(p) = state.panes.get(id) {
                id.hash(&mut h);
                hash_pane(p, &mut h);
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
        // Phase 5-11-6 #6: Window カテゴリの 4 新フィールド + フィールドフォーカス
        // window_field_focus はフォーカス変化のみ生じても tree update が必要
        p.window_field_focus.hash(&mut h);
        // CursorStyle / PresentModeConfig は Hash 未実装なので toml_key 文字列で代用
        p.cursor_style_toml_key().hash(&mut h);
        p.present_mode_toml_key().hash(&mut h);
        p.padding_x.hash(&mut h);
        p.padding_y.hash(&mut h);
        // Phase 5-11-7: Profiles カテゴリ用に selected_profile + profiles の要素数 +
        // 各 ProfileEntry の name / icon を反映する。
        p.selected_profile.hash(&mut h);
        p.profiles.len().hash(&mut h);
        for prof in &p.profiles {
            prof.name.hash(&mut h);
            prof.icon.hash(&mut h);
        }
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

    // === SR アラート（Sprint 5-11-5）===
    // 長さ + 各 seq + kind を反映。kind は `as u8` でハッシュ可能化。
    // body / title はキュー追加時に固定なので seq の変化だけで十分追跡できる（同じ seq に
    // 対して title/body が後から書き換わることはない）。
    state.alerts.len().hash(&mut h);
    for entry in &state.alerts {
        entry.seq.hash(&mut h);
        (entry.kind as u8).hash(&mut h);
    }

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

        // ===== Phase 5-11-6 #6 - カーソル形状 (ComboBox) =====
        (Action::Click | Action::Increment, NodeIdKind::SettingsCursorStyle) => {
            panel.next_cursor_style();
            panel.window_field_focus = 1;
            true
        }
        (Action::Decrement, NodeIdKind::SettingsCursorStyle) => {
            panel.prev_cursor_style();
            panel.window_field_focus = 1;
            true
        }
        (Action::Focus, NodeIdKind::SettingsCursorStyle) => {
            panel.window_field_focus = 1;
            true
        }

        // ===== Phase 5-11-6 #6 - 水平パディング (Slider) =====
        (Action::SetValue, NodeIdKind::SettingsPaddingX) => {
            if let Some(ActionData::NumericValue(v)) = data {
                panel.set_padding_x_value(v);
                panel.window_field_focus = 2;
                true
            } else {
                false
            }
        }
        (Action::Increment, NodeIdKind::SettingsPaddingX) => {
            panel.increase_padding_x();
            panel.window_field_focus = 2;
            true
        }
        (Action::Decrement, NodeIdKind::SettingsPaddingX) => {
            panel.decrease_padding_x();
            panel.window_field_focus = 2;
            true
        }
        (Action::Focus, NodeIdKind::SettingsPaddingX) => {
            panel.window_field_focus = 2;
            true
        }

        // ===== Phase 5-11-6 #6 - 垂直パディング (Slider) =====
        (Action::SetValue, NodeIdKind::SettingsPaddingY) => {
            if let Some(ActionData::NumericValue(v)) = data {
                panel.set_padding_y_value(v);
                panel.window_field_focus = 3;
                true
            } else {
                false
            }
        }
        (Action::Increment, NodeIdKind::SettingsPaddingY) => {
            panel.increase_padding_y();
            panel.window_field_focus = 3;
            true
        }
        (Action::Decrement, NodeIdKind::SettingsPaddingY) => {
            panel.decrease_padding_y();
            panel.window_field_focus = 3;
            true
        }
        (Action::Focus, NodeIdKind::SettingsPaddingY) => {
            panel.window_field_focus = 3;
            true
        }

        // ===== Phase 5-11-6 #6 - GPU プレゼンテーションモード (ComboBox) =====
        (Action::Click | Action::Increment, NodeIdKind::SettingsPresentMode) => {
            panel.next_present_mode();
            panel.window_field_focus = 4;
            true
        }
        (Action::Decrement, NodeIdKind::SettingsPresentMode) => {
            panel.prev_present_mode();
            panel.window_field_focus = 4;
            true
        }
        (Action::Focus, NodeIdKind::SettingsPresentMode) => {
            panel.window_field_focus = 4;
            true
        }

        // ===== Phase 5-11-7 - Profiles 項目 (ListBoxOption) =====
        // Click / Focus いずれも仮想カーソル移動 = 制御遷移として扱い、selected_profile を更新する。
        (Action::Click | Action::Focus, NodeIdKind::SettingsProfileItem { idx })
            if *idx < panel.profiles.len() =>
        {
            panel.selected_profile = *idx;
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

        // ROOT / TAB_BAR / PANE_AREA + PaneInputBuffer (Phase 5-11-7) = 4 ノード
        assert_eq!(update.nodes.len(), 4);
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

        // ROOT + TAB_BAR + PANE_AREA + Tab + Pane + 24 PaneRow + PaneInputBuffer = 30
        assert_eq!(update.nodes.len(), 30);
        assert_eq!(update.focus, pane_node_id(42));

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&tab_node_id(42).0));
        assert!(ids.contains(&pane_node_id(42).0));
        assert!(ids.contains(&PANE_INPUT_BUFFER_ID.0));
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

        // ROOT + TAB_BAR + PANE_AREA + 3 Tab + 3 Pane + 3 * 24 PaneRow + PaneInputBuffer = 82
        assert_eq!(update.nodes.len(), 82);
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

        // ROOT + TAB_BAR + PANE_AREA + Tab + Pane + 24 PaneRow + PaneInputBuffer = 30
        assert_eq!(update.nodes.len(), 30);
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
        // 26 は AlertRegion（Sprint 5-11-5 で割当）、27 は PaneInputBuffer（Phase 5-11-7）、
        // 30〜35 は設定フィールド（Step 2-2-e'）、36〜39 は Phase 5-11-6 #6 の設定フィールド。
        // 28〜29, 40〜99 は将来用に予約。
        assert_eq!(decode_node_id(NodeId(28)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(29)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(40)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(99)), NodeIdKind::Unknown);
        // 700M〜999M は将来 SettingsField 動的展開で使う予約範囲（600M〜700M は
        // Phase 5-11-7 で SettingsProfileItem に割当済み）。
        assert_eq!(decode_node_id(NodeId(700_000_000)), NodeIdKind::Unknown);
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

    // ===== Phase 5-11-6 #6: Window カテゴリ 4 新フィールドのテスト =====

    #[test]
    fn decode_node_id_returns_settings_cursor_style() {
        assert_eq!(
            decode_node_id(SETTINGS_CURSOR_STYLE_ID),
            NodeIdKind::SettingsCursorStyle
        );
    }

    #[test]
    fn decode_node_id_returns_settings_padding_x() {
        assert_eq!(
            decode_node_id(SETTINGS_PADDING_X_ID),
            NodeIdKind::SettingsPaddingX
        );
    }

    #[test]
    fn decode_node_id_returns_settings_padding_y() {
        assert_eq!(
            decode_node_id(SETTINGS_PADDING_Y_ID),
            NodeIdKind::SettingsPaddingY
        );
    }

    #[test]
    fn decode_node_id_returns_settings_present_mode() {
        assert_eq!(
            decode_node_id(SETTINGS_PRESENT_MODE_ID),
            NodeIdKind::SettingsPresentMode
        );
    }

    /// CursorStyle: Click は次にサイクル、フォーカスも 1 に
    #[test]
    fn dispatch_cursor_style_click_cycles_and_focuses() {
        let mut panel = SettingsPanel::default();
        assert_eq!(panel.cursor_style, nexterm_config::CursorStyle::Block);

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsCursorStyle,
            None,
        );
        assert!(handled);
        assert_eq!(panel.cursor_style, nexterm_config::CursorStyle::Beam);
        assert_eq!(panel.window_field_focus, 1);
    }

    /// CursorStyle: Decrement は前にサイクル
    #[test]
    fn dispatch_cursor_style_decrement_goes_back() {
        let mut panel = SettingsPanel::default();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Decrement,
            &NodeIdKind::SettingsCursorStyle,
            None,
        );
        assert!(handled);
        assert_eq!(panel.cursor_style, nexterm_config::CursorStyle::Underline);
    }

    /// CursorStyle: Focus はフォーカスのみ移動（値変更しない）
    #[test]
    fn dispatch_cursor_style_focus_only_moves_focus() {
        let mut panel = SettingsPanel::default();
        let before = panel.cursor_style.clone();
        panel.window_field_focus = 0;
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Focus,
            &NodeIdKind::SettingsCursorStyle,
            None,
        );
        assert!(handled);
        assert_eq!(panel.cursor_style, before, "Focus では値を変えない");
        assert_eq!(panel.window_field_focus, 1);
    }

    /// PaddingX: SetValue で四捨五入 + clamp
    #[test]
    fn dispatch_padding_x_set_value_rounds_and_clamps() {
        let mut panel = SettingsPanel::default();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsPaddingX,
            Some(ActionData::NumericValue(15.7)),
        );
        assert!(handled);
        assert_eq!(panel.padding_x, 16, "15.7 → 16 に丸める");
        assert_eq!(panel.window_field_focus, 2);

        // 上限 clamp
        let _ = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsPaddingX,
            Some(ActionData::NumericValue(100.0)),
        );
        assert_eq!(panel.padding_x, 32, "上限 32 にクランプ");
    }

    /// PaddingX: Increment / Decrement
    #[test]
    fn dispatch_padding_x_increment_decrement() {
        let mut panel = SettingsPanel::default();
        assert_eq!(panel.padding_x, 0);

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Increment,
            &NodeIdKind::SettingsPaddingX,
            None,
        );
        assert!(handled);
        assert_eq!(panel.padding_x, 1);

        let _ = dispatch_settings_action(
            &mut panel,
            Action::Decrement,
            &NodeIdKind::SettingsPaddingX,
            None,
        );
        assert_eq!(panel.padding_x, 0);
    }

    /// PaddingY: SetValue + Increment / Decrement の確認
    #[test]
    fn dispatch_padding_y_actions() {
        let mut panel = SettingsPanel::default();

        let _ = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsPaddingY,
            Some(ActionData::NumericValue(8.0)),
        );
        assert_eq!(panel.padding_y, 8);
        assert_eq!(panel.window_field_focus, 3);

        let _ = dispatch_settings_action(
            &mut panel,
            Action::Increment,
            &NodeIdKind::SettingsPaddingY,
            None,
        );
        assert_eq!(panel.padding_y, 9);

        let _ = dispatch_settings_action(
            &mut panel,
            Action::Decrement,
            &NodeIdKind::SettingsPaddingY,
            None,
        );
        assert_eq!(panel.padding_y, 8);
    }

    /// PresentMode: Click はサイクル、Decrement は逆方向
    #[test]
    fn dispatch_present_mode_click_and_decrement() {
        let mut panel = SettingsPanel::default();
        assert_eq!(
            panel.present_mode,
            nexterm_config::PresentModeConfig::Mailbox
        );

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsPresentMode,
            None,
        );
        assert!(handled);
        assert_eq!(panel.present_mode, nexterm_config::PresentModeConfig::Auto);
        assert_eq!(panel.window_field_focus, 4);

        let _ = dispatch_settings_action(
            &mut panel,
            Action::Decrement,
            &NodeIdKind::SettingsPresentMode,
            None,
        );
        assert_eq!(
            panel.present_mode,
            nexterm_config::PresentModeConfig::Mailbox
        );
    }

    /// build_settings_panel_nodes: Window カテゴリで 5 ノードが公開されること
    #[test]
    fn build_settings_panel_nodes_window_exposes_five_fields() {
        let mut panel = SettingsPanel::default();
        panel.category = crate::settings_panel::SettingsCategory::Window;
        let (nodes, _focus) = build_settings_panel_nodes(&panel);
        let ids: Vec<u64> = nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&SETTINGS_WINDOW_OPACITY_ID.0));
        assert!(ids.contains(&SETTINGS_CURSOR_STYLE_ID.0));
        assert!(ids.contains(&SETTINGS_PADDING_X_ID.0));
        assert!(ids.contains(&SETTINGS_PADDING_Y_ID.0));
        assert!(ids.contains(&SETTINGS_PRESENT_MODE_ID.0));
    }

    /// build_settings_panel_nodes: window_field_focus に応じてフォーカスが正しく移動する
    #[test]
    fn build_settings_panel_nodes_window_focus_follows_field() {
        let cases = [
            (0_u8, SETTINGS_WINDOW_OPACITY_ID),
            (1, SETTINGS_CURSOR_STYLE_ID),
            (2, SETTINGS_PADDING_X_ID),
            (3, SETTINGS_PADDING_Y_ID),
            (4, SETTINGS_PRESENT_MODE_ID),
        ];
        for (focus_idx, expected_node) in cases {
            let mut panel = SettingsPanel::default();
            panel.category = crate::settings_panel::SettingsCategory::Window;
            panel.window_field_focus = focus_idx;
            let (_nodes, focus) = build_settings_panel_nodes(&panel);
            assert_eq!(
                focus, expected_node,
                "window_field_focus={} ではフォーカスが {:?} を指すべき",
                focus_idx, expected_node
            );
        }
    }

    /// compute_tree_state_hash: window_field_focus / cursor_style / padding / present_mode の
    /// 変化を検出する
    #[test]
    fn tree_hash_detects_window_field_changes() {
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = crate::settings_panel::SettingsCategory::Window;
        let h0 = compute_tree_state_hash(&state);

        // フォーカス変化
        state.settings_panel.window_field_focus = 1;
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "window_field_focus 変化はハッシュに反映される");

        // cursor_style 変化
        state.settings_panel.cursor_style = nexterm_config::CursorStyle::Beam;
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(h1, h2, "cursor_style 変化はハッシュに反映される");

        // padding_x 変化
        state.settings_panel.padding_x = 8;
        let h3 = compute_tree_state_hash(&state);
        assert_ne!(h2, h3, "padding_x 変化はハッシュに反映される");

        // padding_y 変化
        state.settings_panel.padding_y = 12;
        let h4 = compute_tree_state_hash(&state);
        assert_ne!(h3, h4, "padding_y 変化はハッシュに反映される");

        // present_mode 変化
        state.settings_panel.present_mode = nexterm_config::PresentModeConfig::Fifo;
        let h5 = compute_tree_state_hash(&state);
        assert_ne!(h4, h5, "present_mode 変化はハッシュに反映される");
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

    /// T9: Sprint 5-11-4 で挙動変更 — Live::Polite はフォーカスペインの
    /// **カーソル行のみ**（旧: フォーカスペインの全行）。
    ///
    /// 過剰アナウンス抑止のため、SR は cursor_row 上の差分のみ読み上げる。
    /// 非カーソル行・非フォーカスペインは Live::None（明示せず）。
    #[test]
    fn build_tree_focused_pane_has_live_polite() {
        let mut state = ClientState::new(5, 3, 1000);
        let mut pane1 = crate::state::PaneState::new(5, 3, 1000);
        // cursor_row を 1 にして「カーソル行のみ Polite」を確実に検証
        pane1.grid.cursor_row = 1;
        let pane2 = crate::state::PaneState::new(5, 3, 1000);
        state.panes.insert(1, pane1);
        state.panes.insert(2, pane2);
        state.tab_order = vec![1, 2];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        // ペイン 1 (focused) のカーソル行 (row 1) のみ Live::Polite
        let row1_cursor = update
            .nodes
            .iter()
            .find(|(id, _)| *id == pane_row_node_id(1, 1))
            .map(|(_, n)| n)
            .expect("ペイン 1 行 1 (カーソル行) が見つからない");
        assert_eq!(row1_cursor.live(), Some(Live::Polite));

        // ペイン 1 (focused) の非カーソル行 (row 0 / 2) は Live::None
        for row in [0u16, 2u16] {
            let n = update
                .nodes
                .iter()
                .find(|(id, _)| *id == pane_row_node_id(1, row))
                .map(|(_, n)| n)
                .unwrap_or_else(|| panic!("ペイン 1 行 {row} が見つからない"));
            assert_eq!(
                n.live(),
                None,
                "ペイン 1 row {row} は非カーソル行なので Live::None のはず"
            );
        }

        // ペイン 2 (non-focused) の全行は Live::None
        for row in 0u16..3u16 {
            let n = update
                .nodes
                .iter()
                .find(|(id, _)| *id == pane_row_node_id(2, row))
                .map(|(_, n)| n)
                .unwrap_or_else(|| panic!("ペイン 2 行 {row} が見つからない"));
            assert_eq!(
                n.live(),
                None,
                "非フォーカスペイン 2 row {row} は Live::None のはず"
            );
        }
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

    // ===== Sprint 5-11-4: カーソル TextSelection + スクロールバック =====

    /// 5-11-4 T1: ASCII 行の character_lengths は各 1 バイト
    #[test]
    fn pane_row_text_with_lengths_ascii() {
        let grid = grid_from_lines(&["abc"]);
        let (text, lengths) = pane_row_text_with_lengths(&grid, 0);
        assert_eq!(text, "abc");
        assert_eq!(lengths, vec![1, 1, 1]);
    }

    /// 5-11-4 T2: 全角 CJK は UTF-8 で 3 バイトずつ
    #[test]
    fn pane_row_text_with_lengths_cjk() {
        let grid = grid_from_lines(&["あい"]);
        let (text, lengths) = pane_row_text_with_lengths(&grid, 0);
        // grid 上は全角 2 セル + 各セル後ろにスペース placeholder 1 個 = 4 セル
        // しかし `grid_from_lines` ヘルパーが char をそのまま set すると placeholder が入らない可能性。
        // pane_row_text の挙動と一致させて検証。
        assert!(text.starts_with("あ"));
        assert!(text.contains("い"));
        // 各 char はそれぞれ 1〜3 バイト範囲
        assert!(lengths.iter().all(|&b| (1..=4).contains(&b)));
        // バイト長合計 == text.len()
        let sum: usize = lengths.iter().map(|&b| b as usize).sum();
        assert_eq!(sum, text.len());
    }

    /// 5-11-4 T3: 空行は (" ", [1])
    #[test]
    fn pane_row_text_with_lengths_empty_row() {
        // 1 行ぶんの空セルがある grid を作る（grid_from_lines は空 string で空 grid を作るため代用）
        let grid = grid_from_lines(&[" "]);
        let (text, lengths) = pane_row_text_with_lengths(&grid, 0);
        assert_eq!(text, " ");
        assert_eq!(lengths, vec![1]);
    }

    /// 5-11-4 T4: 範囲外の row は空行と同じ扱い
    #[test]
    fn pane_row_text_with_lengths_out_of_range_row() {
        let grid = grid_from_lines(&["abc"]);
        let (text, lengths) = pane_row_text_with_lengths(&grid, 99);
        assert_eq!(text, " ");
        assert_eq!(lengths, vec![1]);
    }

    /// 5-11-4 T5: cursor_character_index は cursor_col をそのまま返す（範囲内）
    #[test]
    fn cursor_character_index_within_range() {
        assert_eq!(cursor_character_index("hello", 0), 0);
        assert_eq!(cursor_character_index("hello", 3), 3);
        assert_eq!(cursor_character_index("hello", 5), 5);
    }

    /// 5-11-4 T6: cursor_character_index は char 数を超えるとクランプ
    #[test]
    fn cursor_character_index_clamps_to_char_count() {
        // "hello" は 5 chars
        assert_eq!(cursor_character_index("hello", 10), 5);
        // 空文字列（実際には pane_row_text が " " を返すので使われないが念のため）
        assert_eq!(cursor_character_index("", 5), 0);
    }

    /// 5-11-4 T7: 全角文字 (CJK) は 1 char としてカウント（バイト数ではない）
    #[test]
    fn cursor_character_index_cjk_is_char_based() {
        // "あい" は 2 chars (6 バイト)
        assert_eq!(cursor_character_index("あい", 2), 2);
        // クランプも 2 chars 基準
        assert_eq!(cursor_character_index("あい", 5), 2);
    }

    /// 5-11-4 T8: pane_scrollback_row_node_id がビューポート行 NodeId と衝突しない
    #[test]
    fn pane_scrollback_row_node_id_no_collision_with_viewport_row() {
        let pane_id = 7u32;
        // ビューポート行 [0..1000) と スクロールバック行 [0..9000) が同じペイン内で衝突しない
        for row in [0u16, 100, 500, 999] {
            let v_id = pane_row_node_id(pane_id, row);
            for sb in [0u16, 100, 500, 8999] {
                let sb_id = pane_scrollback_row_node_id(pane_id, sb);
                assert_ne!(
                    v_id, sb_id,
                    "viewport row {row} と scrollback {sb} の NodeId が衝突"
                );
            }
        }
    }

    /// 5-11-4 T9: 異なるペイン間で scrollback 行 NodeId が衝突しない
    #[test]
    fn pane_scrollback_row_node_id_no_collision_between_panes() {
        // ペイン 1 のスクロールバック末尾 (idx=8999) とペイン 2 のスクロールバック先頭 (idx=0)
        // は MAX_ROWS_PER_PANE 単位で分離されているので衝突しない
        let id1_last = pane_scrollback_row_node_id(1, (MAX_SCROLLBACK_ROWS_PER_PANE - 1) as u16);
        let id2_first = pane_scrollback_row_node_id(2, 0);
        assert_ne!(id1_last, id2_first);
        // 値域の確認
        assert!(id1_last.0 < id2_first.0);
    }

    /// 5-11-4 T10: decode_node_id が scrollback 行を正しく PaneScrollbackRow にデコードする
    #[test]
    fn decode_scrollback_row_roundtrip() {
        for pane_id in [0u32, 1, 42, u32::MAX] {
            for idx in [0u16, 1, 100, 8999] {
                let id = pane_scrollback_row_node_id(pane_id, idx);
                let decoded = decode_node_id(id);
                match decoded {
                    NodeIdKind::PaneScrollbackRow { pane_id: p, idx: i } => {
                        assert_eq!(p, pane_id);
                        assert_eq!(i, idx);
                    }
                    other => panic!(
                        "expected PaneScrollbackRow {{ pane_id: {pane_id}, idx: {idx} }}, got {other:?}"
                    ),
                }
            }
        }
    }

    /// 5-11-4 T11: スクロールバックが空ならスクロールバック行ノードは生成されない
    #[test]
    fn build_tree_no_scrollback_when_empty() {
        let mut state = ClientState::new(5, 2, 1000);
        let pane = crate::state::PaneState::new(5, 2, 1000);
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        let sb_node_count = update
            .nodes
            .iter()
            .filter(|(id, _)| matches!(decode_node_id(*id), NodeIdKind::PaneScrollbackRow { .. }))
            .count();
        assert_eq!(sb_node_count, 0, "スクロールバックが空なら行ノードは 0");
    }

    /// 5-11-4 T12: スクロールバックに行を push すると行ノードがツリーに含まれる
    #[test]
    fn build_tree_includes_scrollback_rows_when_present() {
        let mut state = ClientState::new(5, 2, 1000);
        let mut pane = crate::state::PaneState::new(5, 2, 1000);
        // スクロールバックに 3 行追加
        for i in 0..3 {
            let line: Vec<nexterm_proto::Cell> = format!("line{}", i)
                .chars()
                .map(|ch| nexterm_proto::Cell {
                    ch,
                    fg: nexterm_proto::Color::Default,
                    bg: nexterm_proto::Color::Default,
                    attrs: nexterm_proto::Attrs::default(),
                })
                .collect();
            pane.scrollback.push_line(line);
        }
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        let sb_node_count = update
            .nodes
            .iter()
            .filter(|(id, _)| {
                matches!(
                    decode_node_id(*id),
                    NodeIdKind::PaneScrollbackRow { pane_id: 1, .. }
                )
            })
            .count();
        assert_eq!(
            sb_node_count, 3,
            "スクロールバック 3 行ぶんが含まれているはず"
        );
    }

    /// 5-11-4 T13: スクロールバックが SCROLLBACK_WINDOW_RADIUS * 2 を大きく超えても窓内のみ公開
    #[test]
    fn build_tree_scrollback_window_radius_limit() {
        let mut state = ClientState::new(5, 2, 1000);
        let mut pane = crate::state::PaneState::new(5, 2, 1000);
        // 500 行のスクロールバックを push（SCROLLBACK_WINDOW_RADIUS=100 の 5 倍）
        for _ in 0..500 {
            let line: Vec<nexterm_proto::Cell> = "x"
                .chars()
                .map(|ch| nexterm_proto::Cell {
                    ch,
                    fg: nexterm_proto::Color::Default,
                    bg: nexterm_proto::Color::Default,
                    attrs: nexterm_proto::Attrs::default(),
                })
                .collect();
            pane.scrollback.push_line(line);
        }
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        let sb_node_count = update
            .nodes
            .iter()
            .filter(|(id, _)| {
                matches!(
                    decode_node_id(*id),
                    NodeIdKind::PaneScrollbackRow { pane_id: 1, .. }
                )
            })
            .count();
        // 窓幅は [center - RADIUS, center + RADIUS + 1) なので最大 2*RADIUS + 1 行
        let expected_max = SCROLLBACK_WINDOW_RADIUS * 2 + 1;
        assert!(
            sb_node_count <= expected_max,
            "スクロールバック行数 {sb_node_count} が窓上限 {expected_max} を超えている"
        );
        assert!(sb_node_count > 0, "窓内に最低 1 行は含まれるはず");
    }

    /// 5-11-4 T14: フォーカスペインのカーソル行に TextSelection が設定される
    #[test]
    fn build_tree_focused_pane_cursor_row_has_text_selection() {
        let mut state = ClientState::new(10, 5, 1000);
        let mut pane = crate::state::PaneState::new(10, 5, 1000);
        // 行 2 に "abc" を書き、カーソルを (col=2, row=2) に置く
        for (c, ch) in "abc".chars().enumerate() {
            pane.grid.set(
                c as u16,
                2,
                nexterm_proto::Cell {
                    ch,
                    fg: nexterm_proto::Color::Default,
                    bg: nexterm_proto::Color::Default,
                    attrs: nexterm_proto::Attrs::default(),
                },
            );
        }
        pane.grid.cursor_row = 2;
        pane.grid.cursor_col = 2;
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        let pane_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == pane_node_id(1))
            .map(|(_, n)| n)
            .expect("ペインノードが見つからない");
        let sel = pane_node
            .text_selection()
            .expect("フォーカスペインのカーソル行に TextSelection が設定されているはず");
        // anchor == focus == TextPosition { node: pane_row_node_id(1, 2), character_index: 2 }
        assert_eq!(sel.anchor.node, pane_row_node_id(1, 2));
        assert_eq!(sel.focus.node, pane_row_node_id(1, 2));
        assert_eq!(sel.anchor.character_index, 2);
        assert_eq!(sel.focus.character_index, 2);
    }

    /// 5-11-4 T15: 非フォーカスペインには TextSelection が設定されない
    #[test]
    fn build_tree_non_focused_pane_has_no_text_selection() {
        let mut state = ClientState::new(5, 2, 1000);
        let pane1 = crate::state::PaneState::new(5, 2, 1000);
        let pane2 = crate::state::PaneState::new(5, 2, 1000);
        state.panes.insert(1, pane1);
        state.panes.insert(2, pane2);
        state.tab_order = vec![1, 2];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        let pane2_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == pane_node_id(2))
            .map(|(_, n)| n)
            .expect("ペイン 2 が見つからない");
        assert!(
            pane2_node.text_selection().is_none(),
            "非フォーカスペインに TextSelection が設定されてはいけない"
        );
    }

    /// 5-11-4 T16: tree_state_hash がカーソル移動を検出する
    #[test]
    fn tree_state_hash_detects_cursor_move() {
        let mut state = ClientState::new(10, 5, 1000);
        let pane = crate::state::PaneState::new(10, 5, 1000);
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let h1 = compute_tree_state_hash(&state);
        state.panes.get_mut(&1).unwrap().grid.cursor_col = 3;
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(h1, h2, "cursor_col 変化でハッシュが変わるはず");

        state.panes.get_mut(&1).unwrap().grid.cursor_row = 2;
        let h3 = compute_tree_state_hash(&state);
        assert_ne!(h2, h3, "cursor_row 変化でハッシュが変わるはず");
    }

    /// 5-11-4 T17: tree_state_hash がスクロールバック追記を検出する
    #[test]
    fn tree_state_hash_detects_scrollback_grow() {
        let mut state = ClientState::new(5, 2, 1000);
        let pane = crate::state::PaneState::new(5, 2, 1000);
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let h1 = compute_tree_state_hash(&state);
        let line: Vec<nexterm_proto::Cell> = "a"
            .chars()
            .map(|ch| nexterm_proto::Cell {
                ch,
                fg: nexterm_proto::Color::Default,
                bg: nexterm_proto::Color::Default,
                attrs: nexterm_proto::Attrs::default(),
            })
            .collect();
        state.panes.get_mut(&1).unwrap().scrollback.push_line(line);
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(h1, h2, "scrollback.len 変化でハッシュが変わるはず");
    }

    /// 5-11-4 T18: tree_state_hash が scroll_offset 変化を検出する
    #[test]
    fn tree_state_hash_detects_scroll_offset_change() {
        let mut state = ClientState::new(5, 2, 1000);
        let mut pane = crate::state::PaneState::new(5, 2, 1000);
        // スクロールバックを 5 行追加（scroll_offset > 0 が意味を持つように）
        for _ in 0..5 {
            let line: Vec<nexterm_proto::Cell> = "x"
                .chars()
                .map(|ch| nexterm_proto::Cell {
                    ch,
                    fg: nexterm_proto::Color::Default,
                    bg: nexterm_proto::Color::Default,
                    attrs: nexterm_proto::Attrs::default(),
                })
                .collect();
            pane.scrollback.push_line(line);
        }
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let h1 = compute_tree_state_hash(&state);
        state.panes.get_mut(&1).unwrap().scroll_offset = 3;
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(h1, h2, "scroll_offset 変化でハッシュが変わるはず");
    }

    // ===== Sprint 5-11-5: Bell / OSC 9 / OSC 777 → Role::Alert テスト =====

    /// add_alert がキューに追加され、seq が単調増加すること
    #[test]
    fn add_alert_assigns_monotonic_seq() {
        let mut state = ClientState::new(80, 24, 1000);
        let s0 = state.add_alert(AlertKind::Bell, 1, "ベル".to_string(), String::new());
        let s1 = state.add_alert(
            AlertKind::Notification,
            1,
            "Title".to_string(),
            "Body".to_string(),
        );
        let s2 = state.add_alert(AlertKind::Bell, 2, "ベル".to_string(), String::new());
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(state.alerts.len(), 3);
        assert_eq!(state.alerts[0].kind, AlertKind::Bell);
        assert_eq!(state.alerts[1].kind, AlertKind::Notification);
        assert_eq!(state.alerts[2].pane_id, 2);
    }

    /// ALERTS_MAX_LEN (16) を超えると古い順に drop されること
    #[test]
    fn add_alert_drops_oldest_when_full() {
        use crate::state::ALERTS_MAX_LEN;
        let mut state = ClientState::new(80, 24, 1000);
        for i in 0..(ALERTS_MAX_LEN + 5) {
            state.add_alert(AlertKind::Bell, 1, format!("alert {}", i), String::new());
        }
        // 上限内に収まる
        assert_eq!(state.alerts.len(), ALERTS_MAX_LEN);
        // 先頭は ALERTS_MAX_LEN + 5 - ALERTS_MAX_LEN = 5 から始まる
        assert_eq!(state.alerts.front().unwrap().seq, 5);
        assert_eq!(
            state.alerts.back().unwrap().seq,
            (ALERTS_MAX_LEN + 5) as u64 - 1
        );
    }

    /// expire_alerts が TTL 切れエントリを除去し、期限内エントリを残すこと
    #[test]
    fn expire_alerts_removes_only_expired_entries() {
        use crate::state::ALERT_TTL;
        let mut state = ClientState::new(80, 24, 1000);
        // 古い 2 件は created_at を遡って手動で設定（直接 push_back）
        let now = std::time::Instant::now();
        let old = now - ALERT_TTL - std::time::Duration::from_secs(1);
        state.alerts.push_back(AlertEntry {
            seq: 0,
            kind: AlertKind::Bell,
            pane_id: 1,
            title: "old1".to_string(),
            body: String::new(),
            created_at: old,
        });
        state.alerts.push_back(AlertEntry {
            seq: 1,
            kind: AlertKind::Bell,
            pane_id: 1,
            title: "old2".to_string(),
            body: String::new(),
            created_at: old,
        });
        // 新しい 1 件は add_alert で追加
        state.add_alert(AlertKind::Bell, 1, "fresh".to_string(), String::new());

        let removed = state.expire_alerts(now);
        assert_eq!(removed, 2, "古い 2 件が除去される");
        assert_eq!(state.alerts.len(), 1);
        assert_eq!(state.alerts.front().unwrap().title, "fresh");
    }

    /// Phase 5-11-6 #4: `dismiss_alert(seq)` で指定 seq のみ除去できること
    #[test]
    fn dismiss_alert_removes_matching_seq_only() {
        let mut state = ClientState::new(80, 24, 1000);
        let seq_a = state.add_alert(AlertKind::Bell, 1, "a".to_string(), String::new());
        let seq_b = state.add_alert(AlertKind::Bell, 1, "b".to_string(), String::new());
        let seq_c = state.add_alert(AlertKind::Bell, 1, "c".to_string(), String::new());
        assert_eq!(state.alerts.len(), 3);

        // 真ん中の B のみ除去
        let dismissed = state.dismiss_alert(seq_b);
        assert!(dismissed, "存在する seq の dismiss は true");
        assert_eq!(state.alerts.len(), 2);
        let remaining: Vec<u64> = state.alerts.iter().map(|a| a.seq).collect();
        assert_eq!(remaining, vec![seq_a, seq_c], "A と C のみ残る");
    }

    /// Phase 5-11-6 #4: 存在しない seq への `dismiss_alert` は false 返却で副作用なし
    #[test]
    fn dismiss_alert_returns_false_for_unknown_seq() {
        let mut state = ClientState::new(80, 24, 1000);
        let seq = state.add_alert(AlertKind::Bell, 1, "only".to_string(), String::new());
        // 別の seq を指定
        let dismissed = state.dismiss_alert(seq.wrapping_add(99));
        assert!(!dismissed, "存在しない seq の dismiss は false");
        assert_eq!(state.alerts.len(), 1, "副作用なし");
    }

    /// alert_node_id が 50e12 オフセット + seq になり pane_row 範囲と衝突しないこと
    #[test]
    fn alert_node_id_in_correct_offset() {
        let id0 = alert_node_id(0).0;
        let id_big = alert_node_id(u32::MAX as u64).0;
        assert_eq!(id0, NODE_ID_ALERT_OFFSET);
        assert_eq!(id_big, NODE_ID_ALERT_OFFSET + u32::MAX as u64);
        // ペイン行範囲（最大 ~4.3e13）の上限を超えていること
        let pane_row_end =
            NODE_ID_PANE_ROW_OFFSET + (u32::MAX as u64) * MAX_ROWS_PER_PANE + MAX_ROWS_PER_PANE;
        assert!(
            NODE_ID_ALERT_OFFSET >= pane_row_end,
            "Alert オフセット ({}) はペイン行上限 ({}) 以上である必要がある",
            NODE_ID_ALERT_OFFSET,
            pane_row_end
        );
    }

    /// decode_node_id で Alert NodeId を逆引きできること
    #[test]
    fn decode_alert_node_id_roundtrip() {
        for seq in [0u64, 1, 16, 100, u32::MAX as u64] {
            let nid = alert_node_id(seq);
            let kind = decode_node_id(nid);
            assert_eq!(kind, NodeIdKind::Alert { seq });
        }
        // AlertRegion 固定 ID
        assert_eq!(decode_node_id(ALERT_REGION_ID), NodeIdKind::AlertRegion);
    }

    /// 空キューでは ALERT_REGION_ID は ROOT に含まれないこと
    #[test]
    fn build_tree_without_alerts_omits_alert_region() {
        let state = ClientState::new(80, 24, 1000);
        let update = build_tree_from_state(&state);
        let root_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == ROOT_ID)
            .expect("ROOT が存在");
        // ROOT の children に ALERT_REGION_ID は含まれない
        let children: Vec<NodeId> = root_node.1.children().to_vec();
        assert!(
            !children.contains(&ALERT_REGION_ID),
            "アラートなしでは ALERT_REGION_ID が ROOT に含まれない"
        );
        // ALERT_REGION_ID ノードそのものも存在しない
        assert!(
            !update.nodes.iter().any(|(id, _)| *id == ALERT_REGION_ID),
            "アラートなしでは ALERT_REGION ノードが含まれない"
        );
    }

    /// アラート追加で ALERT_REGION_ID と各 Alert ノードが ROOT 子要素に追加されること
    #[test]
    fn build_tree_with_alerts_includes_alert_region_and_children() {
        let mut state = ClientState::new(80, 24, 1000);
        let seq_bell = state.add_alert(AlertKind::Bell, 1, "ベル".to_string(), String::new());
        let seq_notify = state.add_alert(
            AlertKind::Notification,
            1,
            "ビルド完了".to_string(),
            "exit code 0".to_string(),
        );

        let update = build_tree_from_state(&state);

        // ROOT に ALERT_REGION_ID が含まれる
        let root = update.nodes.iter().find(|(id, _)| *id == ROOT_ID).unwrap();
        assert!(root.1.children().contains(&ALERT_REGION_ID));

        // ALERT_REGION 自身が存在し Live::Assertive
        let region = update
            .nodes
            .iter()
            .find(|(id, _)| *id == ALERT_REGION_ID)
            .expect("ALERT_REGION ノードが存在");
        assert_eq!(region.1.live(), Some(Live::Assertive));

        // 各 Alert ノードが存在
        let bell_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == alert_node_id(seq_bell))
            .expect("Bell ノードが存在");
        assert_eq!(bell_node.1.role(), Role::Alert);
        assert_eq!(bell_node.1.label(), Some("ベル"));

        let notify_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == alert_node_id(seq_notify))
            .expect("Notification ノードが存在");
        assert_eq!(notify_node.1.role(), Role::Alert);
        assert_eq!(notify_node.1.label(), Some("通知: ビルド完了"));
        assert_eq!(notify_node.1.description(), Some("exit code 0"));
    }

    /// tree_state_hash がアラート追加で変化すること
    #[test]
    fn tree_state_hash_detects_alert_added() {
        let mut state = ClientState::new(80, 24, 1000);
        let h0 = compute_tree_state_hash(&state);
        state.add_alert(AlertKind::Bell, 1, "ベル".to_string(), String::new());
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "アラート追加でハッシュが変化");
        // 同種のアラート追加でも seq が違うのでハッシュは変化
        state.add_alert(AlertKind::Bell, 1, "ベル".to_string(), String::new());
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(h1, h2, "2 件目追加でハッシュが変化");
    }

    /// tree_state_hash がアラート kind 変化で変化すること
    #[test]
    fn tree_state_hash_detects_alert_kind_difference() {
        let mut s1 = ClientState::new(80, 24, 1000);
        s1.add_alert(AlertKind::Bell, 1, "title".to_string(), String::new());

        let mut s2 = ClientState::new(80, 24, 1000);
        s2.add_alert(
            AlertKind::Notification,
            1,
            "title".to_string(),
            String::new(),
        );

        let h1 = compute_tree_state_hash(&s1);
        let h2 = compute_tree_state_hash(&s2);
        assert_ne!(h1, h2, "Bell と Notification でハッシュが異なる");
    }

    /// 本文が空の Alert (Bell) は description が設定されないこと
    #[test]
    fn build_tree_alert_without_body_omits_description() {
        let mut state = ClientState::new(80, 24, 1000);
        let seq = state.add_alert(AlertKind::Bell, 1, "ベル".to_string(), String::new());
        let update = build_tree_from_state(&state);
        let bell = update
            .nodes
            .iter()
            .find(|(id, _)| *id == alert_node_id(seq))
            .unwrap();
        assert_eq!(bell.1.description(), None);
    }

    // ===== Phase 5-11-7: PTY 入力バッファ =====

    /// PaneInputBuffer の NodeId(27) が `NodeIdKind::PaneInputBuffer` に decode されること
    #[test]
    fn decode_pane_input_buffer() {
        assert_eq!(
            decode_node_id(PANE_INPUT_BUFFER_ID),
            NodeIdKind::PaneInputBuffer
        );
        assert_eq!(decode_node_id(NodeId(27)), NodeIdKind::PaneInputBuffer);
    }

    /// PaneInputBuffer は PaneArea の子として常に存在し、Role::TextInput であること
    #[test]
    fn build_tree_includes_pane_input_buffer() {
        let state = ClientState::new(80, 24, 1000);
        let update = build_tree_from_state(&state);

        let input_buffer = update
            .nodes
            .iter()
            .find(|(id, _)| *id == PANE_INPUT_BUFFER_ID)
            .expect("PaneInputBuffer ノードが存在");
        assert_eq!(input_buffer.1.role(), Role::TextInput);
        assert_eq!(input_buffer.1.label(), Some("ターミナル入力バッファ"));
        assert_eq!(input_buffer.1.value(), Some(""));
    }

    /// PaneInputBuffer の description はフォーカスペインのタイトルを含むこと
    #[test]
    fn pane_input_buffer_description_includes_focused_pane_title() {
        let mut state = ClientState::new(80, 24, 1000);
        let mut pane = crate::state::PaneState::new(80, 24, 1000);
        pane.title = "vim main.rs".to_string();
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);
        let input_buffer = update
            .nodes
            .iter()
            .find(|(id, _)| *id == PANE_INPUT_BUFFER_ID)
            .unwrap();
        let desc = input_buffer.1.description().unwrap_or("");
        assert!(
            desc.contains("vim main.rs"),
            "description にペインタイトルが含まれる: {}",
            desc
        );
    }

    /// フォーカスペインが存在しない場合は「フォーカスペインなし」と表示されること
    #[test]
    fn pane_input_buffer_description_when_no_focus() {
        let state = ClientState::new(80, 24, 1000);
        let update = build_tree_from_state(&state);
        let input_buffer = update
            .nodes
            .iter()
            .find(|(id, _)| *id == PANE_INPUT_BUFFER_ID)
            .unwrap();
        let desc = input_buffer.1.description().unwrap_or("");
        assert!(
            desc.contains("フォーカスペインなし"),
            "フォーカスなしのメッセージが含まれる: {}",
            desc
        );
    }

    /// PaneArea の子に PaneInputBuffer が末尾追加されていること
    #[test]
    fn pane_area_children_include_input_buffer_as_last() {
        let mut state = ClientState::new(80, 24, 1000);
        state
            .panes
            .insert(1, crate::state::PaneState::new(80, 24, 1000));
        state.tab_order = vec![1];

        let update = build_tree_from_state(&state);
        let pane_area = update
            .nodes
            .iter()
            .find(|(id, _)| *id == PANE_AREA_ID)
            .unwrap();
        let children: Vec<NodeId> = pane_area.1.children().to_vec();
        assert_eq!(
            *children.last().unwrap(),
            PANE_INPUT_BUFFER_ID,
            "PaneArea の最後の子が PaneInputBuffer"
        );
        // ペイン本体 + PaneInputBuffer = 2 子
        assert_eq!(children.len(), 2);
    }

    /// フォーカスペインが変わると tree hash も変わること（入力バッファ description 反映）
    #[test]
    fn tree_state_hash_detects_focused_pane_title_change() {
        let mut state = ClientState::new(80, 24, 1000);
        let mut pane = crate::state::PaneState::new(80, 24, 1000);
        pane.title = "old title".to_string();
        state.panes.insert(1, pane);
        state.tab_order = vec![1];
        state.focused_pane_id = Some(1);
        let h0 = compute_tree_state_hash(&state);

        state.panes.get_mut(&1).unwrap().title = "new title".to_string();
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "フォーカスペインのタイトル変更でハッシュが変化");
    }

    // ===== Phase 5-11-7: SettingsPanel Profiles + Ssh/Keybindings description =====

    /// SettingsProfileItem の NodeId roundtrip
    #[test]
    fn settings_profile_item_id_roundtrip() {
        for idx in [0, 1, 50, 99_999] {
            let id = settings_profile_item_id(idx);
            let decoded = decode_node_id(id);
            assert_eq!(
                decoded,
                NodeIdKind::SettingsProfileItem { idx },
                "settings_profile_item_id({}) の roundtrip",
                idx
            );
        }
    }

    /// SettingsProfileItem オフセットが QuickSelect / Tab 範囲と衝突しないこと
    #[test]
    fn settings_profile_offset_does_not_overlap() {
        const _: () = assert!(NODE_ID_SETTINGS_PROFILE_OFFSET > NODE_ID_QUICKSELECT_ITEM_OFFSET);
        const _: () = assert!(
            NODE_ID_SETTINGS_PROFILE_OFFSET + 100_000_000 <= NODE_ID_TAB_OFFSET,
            "Profiles 範囲 [600M, 700M) は Tab 範囲 [1G, ...) と衝突しない"
        );
    }

    /// Profiles カテゴリが空のとき: 「プロファイルがありません」を案内する
    #[test]
    fn build_settings_panel_profiles_empty() {
        use crate::settings_panel::SettingsCategory;
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Profiles;
        panel.profiles = vec![];

        let (nodes, _focus) = build_settings_panel_nodes(&panel);
        let content = nodes
            .iter()
            .find(|(id, _)| *id == SETTINGS_CONTENT_ID)
            .unwrap();
        let desc = content.1.description().unwrap_or("");
        assert!(
            desc.contains("プロファイルがありません"),
            "空案内文が含まれる: {}",
            desc
        );
    }

    /// Profiles カテゴリにプロファイルがあるとき: ListBoxOption が公開される
    #[test]
    fn build_settings_panel_profiles_exposes_listbox_options() {
        use crate::settings_panel::{ProfileEntry, SettingsCategory};
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Profiles;
        panel.profiles = vec![
            ProfileEntry {
                name: "bash".to_string(),
                icon: "🐧".to_string(),
                shell_program: "/bin/bash".to_string(),
                working_dir: String::new(),
            },
            ProfileEntry {
                name: "powershell".to_string(),
                icon: "💠".to_string(),
                shell_program: "pwsh".to_string(),
                working_dir: String::new(),
            },
        ];
        panel.selected_profile = 1;

        let (nodes, focus) = build_settings_panel_nodes(&panel);

        // 各 ListBoxOption が公開される
        let opt0 = nodes
            .iter()
            .find(|(id, _)| *id == settings_profile_item_id(0))
            .unwrap();
        assert_eq!(opt0.1.role(), Role::ListBoxOption);
        assert!(opt0.1.label().unwrap_or("").contains("bash"));
        assert_eq!(opt0.1.is_selected(), None); // 未選択 (set_selected されない)

        let opt1 = nodes
            .iter()
            .find(|(id, _)| *id == settings_profile_item_id(1))
            .unwrap();
        assert_eq!(opt1.1.role(), Role::ListBoxOption);
        assert!(opt1.1.label().unwrap_or("").contains("powershell"));
        // selected_profile = 1 なのでこちらが選択中
        assert_eq!(opt1.1.is_selected(), Some(true));

        // フォーカスは選択中のプロファイル項目へ
        assert_eq!(focus, settings_profile_item_id(1));
    }

    /// dispatch_settings_action: SettingsProfileItem Click で selected_profile が更新される
    #[test]
    fn dispatch_settings_profile_item_click() {
        use crate::settings_panel::{ProfileEntry, SettingsCategory};
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Profiles;
        panel.profiles = vec![
            ProfileEntry {
                name: "a".to_string(),
                icon: String::new(),
                shell_program: String::new(),
                working_dir: String::new(),
            },
            ProfileEntry {
                name: "b".to_string(),
                icon: String::new(),
                shell_program: String::new(),
                working_dir: String::new(),
            },
        ];
        panel.selected_profile = 0;

        let handled = dispatch_settings_action(
            &mut panel,
            accesskit::Action::Click,
            &NodeIdKind::SettingsProfileItem { idx: 1 },
            None,
        );
        assert!(handled);
        assert_eq!(panel.selected_profile, 1);
    }

    /// dispatch_settings_action: SettingsProfileItem Focus でも selected_profile が更新される
    #[test]
    fn dispatch_settings_profile_item_focus() {
        use crate::settings_panel::{ProfileEntry, SettingsCategory};
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Profiles;
        panel.profiles = vec![ProfileEntry {
            name: "x".to_string(),
            icon: String::new(),
            shell_program: String::new(),
            working_dir: String::new(),
        }];
        panel.selected_profile = 0;

        let handled = dispatch_settings_action(
            &mut panel,
            accesskit::Action::Focus,
            &NodeIdKind::SettingsProfileItem { idx: 0 },
            None,
        );
        assert!(handled);
        assert_eq!(panel.selected_profile, 0);
    }

    /// dispatch_settings_action: 範囲外の idx は no-op で false を返す
    #[test]
    fn dispatch_settings_profile_item_out_of_range() {
        let mut panel = SettingsPanel::default();
        panel.profiles = vec![];

        let handled = dispatch_settings_action(
            &mut panel,
            accesskit::Action::Click,
            &NodeIdKind::SettingsProfileItem { idx: 5 },
            None,
        );
        assert!(!handled);
        assert_eq!(panel.selected_profile, 0);
    }

    /// SSH カテゴリは TOML 編集の案内 description を持つこと
    #[test]
    fn build_settings_panel_ssh_has_informative_description() {
        use crate::settings_panel::SettingsCategory;
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Ssh;

        let (nodes, _focus) = build_settings_panel_nodes(&panel);
        let content = nodes
            .iter()
            .find(|(id, _)| *id == SETTINGS_CONTENT_ID)
            .unwrap();
        let desc = content.1.description().unwrap_or("");
        assert!(desc.contains("nexterm.toml"), "案内文: {}", desc);
        assert!(desc.contains("[[hosts]]"), "案内文: {}", desc);
        assert!(
            !desc.contains("まだ実装されていません"),
            "「未実装」表記が消えている"
        );
    }

    /// Keybindings カテゴリも TOML 編集の案内 description を持つこと
    #[test]
    fn build_settings_panel_keybindings_has_informative_description() {
        use crate::settings_panel::SettingsCategory;
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Keybindings;

        let (nodes, _focus) = build_settings_panel_nodes(&panel);
        let content = nodes
            .iter()
            .find(|(id, _)| *id == SETTINGS_CONTENT_ID)
            .unwrap();
        let desc = content.1.description().unwrap_or("");
        assert!(desc.contains("nexterm.toml"), "案内文: {}", desc);
        assert!(desc.contains("[[keys]]"), "案内文: {}", desc);
        assert!(
            !desc.contains("まだ実装されていません"),
            "「未実装」表記が消えている"
        );
    }

    /// tree_state_hash が selected_profile 変更で変化すること
    #[test]
    fn tree_state_hash_detects_selected_profile_change() {
        use crate::settings_panel::ProfileEntry;
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.profiles = vec![
            ProfileEntry {
                name: "a".to_string(),
                icon: String::new(),
                shell_program: String::new(),
                working_dir: String::new(),
            },
            ProfileEntry {
                name: "b".to_string(),
                icon: String::new(),
                shell_program: String::new(),
                working_dir: String::new(),
            },
        ];
        state.settings_panel.selected_profile = 0;
        let h0 = compute_tree_state_hash(&state);

        state.settings_panel.selected_profile = 1;
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "selected_profile 変更でハッシュが変化");
    }

    /// tree_state_hash が profiles リスト変更で変化すること
    #[test]
    fn tree_state_hash_detects_profiles_change() {
        use crate::settings_panel::ProfileEntry;
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.profiles = vec![];
        let h0 = compute_tree_state_hash(&state);

        state.settings_panel.profiles = vec![ProfileEntry {
            name: "added".to_string(),
            icon: String::new(),
            shell_program: String::new(),
            working_dir: String::new(),
        }];
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "profiles 追加でハッシュが変化");
    }
}
