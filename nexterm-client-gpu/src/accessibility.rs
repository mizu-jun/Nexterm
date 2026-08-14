//! Sprint 5-11-1 to 5-11-2 / H1: Screen reader node tree generation
//!
//! Implementation of audit round 2 task **H1** (screen reader support).
//! Competing OSS terminals (kitty / WezTerm / Alacritty / Ghostty) all have weak
//! screen reader support, so completing this work creates a clear differentiation
//! point (see `project_audit_round2.md`).
//!
//! ## What this module provides
//!
//! - **NodeId scheme**: fixed IDs + dynamic IDs for panes/tabs/overlay items
//! - **Dynamic tree generation**: `build_tree_from_state(&ClientState)` reflects tabs, panes, and the frontmost overlay
//! - Tree passed to `accesskit_winit::Adapter::update_if_active` (forwarded to the OS a11y API)
//!
//! ## Roadmap
//!
//! - Phase 5-11-1 PoC ✅: fixed tree + Adapter integration
//! - Phase 5-11-2 Step 2-1 ✅: dynamic tree generation from ClientState (tabs/panes)
//! - **Phase 5-11-2 Step 2-2 ⬅️**: overlays (CommandPalette / ContextMenu / CloseWindowDialog / SettingsPanel / HostManager / MacroPicker / update_banner)
//! - Phase 5-11-2 Step 2-3: multiple OS Window support
//! - Phase 5-11-2 Step 2-4: Action handling (Focus / Click)
//! - Phase 5-11-3: terminal grid diff notifications (100ms throttled)
//! - Phase 5-11-4: OSC 133-linked review mode
//! - Phase 5-11-5: settings UI + i18n + documentation

use accesskit::{
    Action, Live, Node, NodeId, Role, TextPosition, TextSelection, Tree, TreeId, TreeUpdate,
};

use crate::host_manager::HostManager;
use crate::macro_picker::MacroPicker;
use crate::palette::CommandPalette;
use crate::settings_panel::SettingsPanel;
use crate::state::{
    AlertEntry, AlertKind, ClientState, CloseWindowDialog, ContextMenu, QuickSelectState,
};

// ===== Fixed NodeIds =====
//
// Platform a11y adapters cache and track node IDs, so **stability** is critical.
// Allocate IDs with offsets so a pane's ID is never reused after deletion.

/// Root node (the entire OS window).
pub const ROOT_ID: NodeId = NodeId(1);

/// Tab bar (`Role::TabList`).
pub const TAB_BAR_ID: NodeId = NodeId(2);

/// Pane area container (`Role::Group`).
pub const PANE_AREA_ID: NodeId = NodeId(3);

// ===== Overlay fixed NodeIds (Step 2-2) =====

/// Root of the settings panel (Ctrl+,).
pub const SETTINGS_PANEL_ID: NodeId = NodeId(4);

/// Root of the command palette (Ctrl+Shift+P).
pub const PALETTE_ID: NodeId = NodeId(5);

/// Host manager.
pub const HOST_MANAGER_ID: NodeId = NodeId(6);

/// Macro picker.
pub const MACRO_PICKER_ID: NodeId = NodeId(7);

/// Context menu (right-click).
pub const CONTEXT_MENU_ID: NodeId = NodeId(8);

/// "Close window?" confirmation dialog.
pub const CLOSE_DIALOG_ID: NodeId = NodeId(9);

/// Update notification banner.
pub const UPDATE_BANNER_ID: NodeId = NodeId(10);

/// Root of the Quick Select overlay (Step 2-2-h).
pub const QUICK_SELECT_ID: NodeId = NodeId(11);

/// Search input field of the command palette.
pub const PALETTE_SEARCH_ID: NodeId = NodeId(12);

/// Candidate list of the command palette.
pub const PALETTE_LIST_ID: NodeId = NodeId(13);

/// "Close / kill process" button of the confirmation dialog.
pub const CLOSE_DIALOG_KILL_BTN: NodeId = NodeId(14);

/// "Cancel" button of the confirmation dialog.
pub const CLOSE_DIALOG_CANCEL_BTN: NodeId = NodeId(15);

/// `ListBox` of the Quick Select match list (Step 2-2-h).
pub const QUICK_SELECT_LIST_ID: NodeId = NodeId(16);

// ===== SettingsPanel field fixed NodeIds (Step 2-2-e') =====

/// Category TabList of the settings panel.
pub const SETTINGS_TABLIST_ID: NodeId = NodeId(17);

// Tabs for `SettingsCategory::ALL` live at `SETTINGS_TAB_BASE + idx`
// (see `settings_tab_id_at`). Phase 2c-G moved the base from 18 to 60 so
// the range cannot collide with `SETTINGS_CONTENT_ID = 25` once the
// category list grew past 7 entries.

/// Content container (`Group`) for the current settings panel category.
pub const SETTINGS_CONTENT_ID: NodeId = NodeId(25);

/// Root of the SR alert region (Sprint 5-11-5).
///
/// Container that exposes Bell / OSC 9 / OSC 777 as `Role::Alert`.
/// Always present as a child of ROOT; SR announces each Alert node beneath it.
/// `Live::Assertive` is set so that new alerts are announced immediately.
pub const ALERT_REGION_ID: NodeId = NodeId(26);

/// Terminal input buffer (Sprint 5-11-7, Phase 5-11-7).
///
/// A single `Role::TextInput` node that is always present as the last child of
/// `PANE_AREA_ID`. When an SR user writes a string here via `SetValue`, a
/// `PasteText` IPC is sent to the focused pane and forwarded to the PTY.
/// After the write completes, `value` is reset to an empty string so further
/// input is possible.
///
/// Design rationale (Q2 (b) adopted):
/// - Separate display-side TextRun rows (`PaneRow` / `PaneScrollbackRow`) from
///   the input-side TextInput so the AccessKit tree's responsibilities are clear.
/// - `Role::Terminal` SetValue behavior is not standardized in AccessKit 0.24,
///   so a generic `Role::TextInput` is used instead.
/// - Multi-line input containing `\n` is forwarded to the PTY verbatim.
pub const PANE_INPUT_BUFFER_ID: NodeId = NodeId(27);

// 28..29 reserved for future containers (sidebars, etc.).

// 30..=39 were hand-written settings-field nodes (font family/size, theme
// colour scheme, window opacity, startup language/auto-update, cursor style,
// padding x/y, present mode). Every one of those categories is now described
// by the widget layer, with nodes in the `SETTINGS_WIDGET_BASE` range, so the
// whole 30..=39 block is free.

// 40..=46 were the hand-written SSH field / Add / Delete nodes
// (Phase 5-11-8). The Ssh category is described by the widget layer since
// UI/UX v3 P1c, so that block is free; only the delete-confirmation dialog
// below keeps fixed ids.

/// Phase 5-11-8 Step 8-3 Sub-phase D - SSH delete confirmation dialog body (Role::AlertDialog).
pub const SETTINGS_SSH_DELETE_DIALOG_ID: NodeId = NodeId(47);

/// Phase 5-11-8 Step 8-3 Sub-phase D - "Delete" confirmation button in the SSH delete dialog.
pub const SETTINGS_SSH_DELETE_CONFIRM_BTN_ID: NodeId = NodeId(48);

/// Phase 5-11-8 Step 8-3 Sub-phase D - "Cancel" button in the SSH delete dialog.
pub const SETTINGS_SSH_DELETE_CANCEL_BTN_ID: NodeId = NodeId(49);

/// Phase 5-11-9 Sub-phase E - Keybindings category: key field of the selected binding (TextInput).
///
/// While `key_editing` is in `Record` mode, the SR-visible label/description
/// communicates "Press a key now"; outside Record mode the value field carries
/// the binding's literal key string (e.g. `"ctrl+shift+p"`).
pub const SETTINGS_KEY_FIELD_KEY_ID: NodeId = NodeId(50);

/// Phase 5-11-9 Sub-phase E - Keybindings category: action field of the selected binding (ComboBox).
pub const SETTINGS_KEY_FIELD_ACTION_ID: NodeId = NodeId(51);

/// Phase 5-11-9 Sub-phase E - Keybindings category: add-binding button.
pub const SETTINGS_KEY_ADD_BTN_ID: NodeId = NodeId(52);

/// Phase 5-11-9 Sub-phase E - Keybindings category: delete-binding button.
pub const SETTINGS_KEY_DELETE_BTN_ID: NodeId = NodeId(53);

/// Phase 5-11-9 Sub-phase E - Keybindings delete-confirmation dialog body (Role::AlertDialog).
pub const SETTINGS_KEY_DELETE_DIALOG_ID: NodeId = NodeId(54);

/// Phase 5-11-9 Sub-phase E - "Delete" confirmation button in the Keybindings delete dialog.
pub const SETTINGS_KEY_DELETE_CONFIRM_BTN_ID: NodeId = NodeId(55);

/// Phase 5-11-9 Sub-phase E - "Cancel" button in the Keybindings delete dialog.
pub const SETTINGS_KEY_DELETE_CANCEL_BTN_ID: NodeId = NodeId(56);

// ===== Custom title bar window buttons (`window.decorations = "notitle"`) =====

/// Tab bar: minimize window button.
pub const WINDOW_MINIMIZE_BTN_ID: NodeId = NodeId(57);

/// Tab bar: maximize / restore window button.
pub const WINDOW_MAXIMIZE_BTN_ID: NodeId = NodeId(58);

/// Tab bar: close window button.
pub const WINDOW_CLOSE_BTN_ID: NodeId = NodeId(59);

// 57..99 reserved for future fields.

/// Base NodeId for nodes derived from a `WidgetSpec` (UI/UX v3 P1b).
///
/// Occupies the `700M..800M` slot the offset table already reserved for
/// "future dynamic SettingsField expansion". A widget's node id is
/// `SETTINGS_WIDGET_BASE + WidgetId::as_u32()`, which packs the category and
/// index into two bytes — so the range holds every category comfortably.
pub const SETTINGS_WIDGET_BASE: u64 = 700_000_000;

/// NodeId of a widget-derived settings node.
pub fn settings_widget_id(id: crate::renderer::overlay::widgets::spec::WidgetId) -> NodeId {
    NodeId(SETTINGS_WIDGET_BASE + id.as_u32() as u64)
}

/// Base NodeId for settings panel category tabs.
///
/// Range: `[18, 18 + SettingsCategory::ALL.len()) = [18, 25)`. Adjacent to
/// `SETTINGS_CONTENT_ID = 25` previously overlapped `settings_tab_id_at(7)`;
/// Phase 2c-G moved the base into a non-conflicting range (see
/// `settings_tab_id_at`).
const SETTINGS_TAB_BASE: u64 = 60;

/// Compute the NodeId of the tab for the given `SettingsCategory::ALL` index.
///
/// Phase 2c-G note: the tab base was previously 18, with 7 slots reserved
/// before `SETTINGS_CONTENT_ID = 25`. Adding the Blocks category pushed
/// idx 7 into the `SETTINGS_CONTENT_ID` slot and broke node-id uniqueness
/// (the build_settings_panel_nodes function pushes both, and direct
/// equality lookups returned the wrong node). The base has been moved to
/// `60` — the gap between the `30..=39` settings-field range and the
/// `100..=199` dialog-element range, with 40 slots of headroom for future
/// categories.
pub fn settings_tab_id_at(idx: usize) -> NodeId {
    NodeId(SETTINGS_TAB_BASE + idx as u64)
}

// ===== Dynamic NodeId offsets =====
//
// Allocated to repeated elements (list items) inside overlays.
// Keep all values < 999_999_999 to avoid colliding with the tab range [1e9, 5.3e9].

/// Command palette candidate (`100_000_000 + idx`).
const NODE_ID_PALETTE_ITEM_OFFSET: u64 = 100_000_000;

/// Host list item (`200_000_000 + idx`).
const NODE_ID_HOST_ITEM_OFFSET: u64 = 200_000_000;

/// Macro list item (`300_000_000 + idx`).
const NODE_ID_MACRO_ITEM_OFFSET: u64 = 300_000_000;

/// Context menu item (`400_000_000 + idx`).
const NODE_ID_CONTEXT_ITEM_OFFSET: u64 = 400_000_000;

/// Quick Select match item (`500_000_000 + idx`, Step 2-2-h).
const NODE_ID_QUICKSELECT_ITEM_OFFSET: u64 = 500_000_000;

// The 600M..700M range used to carry `SettingsProfileItem` (Phase 5-11-7).
// Retired in UI/UX v3 P1c: the Profiles category now lives on the widget
// layer, whose ids sit in the 700M `SETTINGS_WIDGET_BASE` slot. Node ids are
// never persisted, so the range is free for reuse.

// The 800M..900M range used to carry `SettingsSshHostItem` (Phase 5-11-8).
// Retired in UI/UX v3 P1c: the Ssh category now lives on the widget layer,
// whose ids sit in the 700M `SETTINGS_WIDGET_BASE` slot. Node ids are never
// persisted, so the range is free for reuse.

/// Dynamic items of the SettingsPanel Keybindings category (`900_000_000 + idx`, Phase 5-11-9 Sub-phase E).
///
/// Each `KeyBindingEntry` of `SettingsPanel.keybindings` is exposed as
/// `Role::ListBoxOption`. `selected_key_index` identifies the currently
/// selected entry.
///
/// Range: `[900_000_000, 1_000_000_000)`. Sits just below
/// `NODE_ID_TAB_OFFSET = 1e9`, so 100M of headroom matches what other
/// dynamic offsets enjoy.
const NODE_ID_SETTINGS_KEY_BINDING_OFFSET: u64 = 900_000_000;

/// Phase 5-11-9 Sub-phase E - Compute the NodeId of a key binding list item.
pub fn settings_key_binding_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_SETTINGS_KEY_BINDING_OFFSET + idx as u64)
}

/// Offset used to compute a tab node's NodeId.
///
/// Internal representation: `NODE_ID_TAB_OFFSET + pane_id as u64`. Because `pane_id`
/// is a u32, the range is `[1_000_000_000, 1_000_000_000 + u32::MAX] ≈ [1e9, 5.3e9]`.
/// Guaranteed never to collide with `NODE_ID_PANE_OFFSET` (gap of at least 4e9).
const NODE_ID_TAB_OFFSET: u64 = 1_000_000_000;

/// Offset used to compute a pane node's NodeId.
///
/// Range: `[10_000_000_000, 10_000_000_000 + u32::MAX] ≈ [1e10, 1.43e10]`.
const NODE_ID_PANE_OFFSET: u64 = 10_000_000_000;

/// Offset used to compute the NodeId of an individual SR alert node (Sprint 5-11-5).
///
/// Internal representation: `NODE_ID_ALERT_OFFSET + AlertEntry.seq`.
///
/// **Rationale for the chosen range**:
/// - The pane row range `[2e10, 2e10 + u32::MAX × 10000 + 10000] ≈ [2e10, 4.3e13]`
///   is used continuously by pane_row / pane_scrollback.
/// - `50e12` (50 trillion) sits safely above that upper bound.
/// - `ClientState.next_alert_seq` would take about 584 million years to overflow
///   even at 1000 alerts per second, so we can safely extend to `u64::MAX`.
const NODE_ID_ALERT_OFFSET: u64 = 50_000_000_000_000;

/// Compute the NodeId of an Alert node from `AlertEntry.seq` (Sprint 5-11-5).
pub fn alert_node_id(seq: u64) -> NodeId {
    NodeId(NODE_ID_ALERT_OFFSET + seq)
}

/// Offset used to compute the NodeId of a pane row node (Sprint 5-11-3 / 5-11-4).
///
/// Each row of the terminal grid is exposed as a `Role::TextRun` child of the pane node.
/// Internal representation: `NODE_ID_PANE_ROW_OFFSET + pane_id as u64 * MAX_ROWS_PER_PANE + row_offset`.
///
/// `row_offset` breakdown:
/// - `0..MAX_VIEWPORT_ROWS_PER_PANE` (0..1000): viewport rows (`pane_row_node_id`)
/// - `MAX_VIEWPORT_ROWS_PER_PANE..MAX_ROWS_PER_PANE` (1000..10000):
///   scrollback rows (Sprint 5-11-4, `pane_scrollback_row_node_id`)
///
/// Range: `[2e10, 2e10 + u32::MAX * 10000 + 9999] ≈ [2e10, 4.3e13]`.
/// Plenty of gap before the upper bound of `NODE_ID_PANE_OFFSET` (~1.43e10).
const NODE_ID_PANE_ROW_OFFSET: u64 = 20_000_000_000;

/// Maximum number of rows exposed per pane (extended 1000 -> 10000 between Sprint 5-11-3 and 5-11-4).
///
/// Breakdown:
/// - `0..MAX_VIEWPORT_ROWS_PER_PANE` (0..1000): terminal viewport rows
/// - `MAX_VIEWPORT_ROWS_PER_PANE..MAX_ROWS_PER_PANE` (1000..10000): scrollback rows (Sprint 5-11-4)
///
/// Real terminals typically display around 200 rows with a few thousand rows of
/// scrollback. Rows beyond this cap become invisible to SR, but realistic
/// displays never reach it.
pub const MAX_ROWS_PER_PANE: u64 = 10_000;

/// Upper bound for the number of viewport (grid) rows exposed per pane (Sprint 5-11-4).
///
/// Of the row NodeIds assigned by `pane_row_node_id`, the range
/// `0..MAX_VIEWPORT_ROWS_PER_PANE` is reserved for viewport rows.
pub const MAX_VIEWPORT_ROWS_PER_PANE: u64 = 1_000;

/// Upper bound for the number of scrollback rows exposed per pane (Sprint 5-11-4).
///
/// Range occupied by NodeIds returned by `pane_scrollback_row_node_id`.
/// Equal to `MAX_ROWS_PER_PANE - MAX_VIEWPORT_ROWS_PER_PANE`.
pub const MAX_SCROLLBACK_ROWS_PER_PANE: u64 = MAX_ROWS_PER_PANE - MAX_VIEWPORT_ROWS_PER_PANE;

/// Radius of the sliding window used to expose scrollback to SR (Sprint 5-11-4).
///
/// `SCROLLBACK_WINDOW_RADIUS` rows on each side of the current scroll position are
/// included in the AccessKit tree. Real terminal scrollback can grow to thousands
/// of rows, so exposing every row would hurt performance. A 100-row window is
/// sufficient for comfortable SR arrow-key navigation.
pub const SCROLLBACK_WINDOW_RADIUS: usize = 100;

/// Compute the NodeId of a tab node from a `pane_id` (u32).
pub fn tab_node_id(pane_id: u32) -> NodeId {
    NodeId(NODE_ID_TAB_OFFSET + pane_id as u64)
}

/// Compute the NodeId of a pane (terminal) node from a `pane_id` (u32).
pub fn pane_node_id(pane_id: u32) -> NodeId {
    NodeId(NODE_ID_PANE_OFFSET + pane_id as u64)
}

/// Compute the NodeId of a viewport row node from `pane_id × row_idx` (Sprint 5-11-3).
///
/// The caller must guarantee `row < MAX_VIEWPORT_ROWS_PER_PANE`; otherwise the
/// resulting NodeId may collide with another row.
pub fn pane_row_node_id(pane_id: u32, row: u16) -> NodeId {
    debug_assert!((row as u64) < MAX_VIEWPORT_ROWS_PER_PANE);
    NodeId(NODE_ID_PANE_ROW_OFFSET + (pane_id as u64) * MAX_ROWS_PER_PANE + row as u64)
}

/// Compute the NodeId of a scrollback row node from `pane_id × scrollback_idx` (Sprint 5-11-4).
///
/// Scrollback row NodeIds occupy a contiguous space adjacent to the same pane's
/// viewport row NodeIds:
/// `pane_row` range = `[base, base + MAX_VIEWPORT_ROWS_PER_PANE)`,
/// `pane_scrollback` range = `[base + MAX_VIEWPORT_ROWS_PER_PANE, base + MAX_ROWS_PER_PANE)`
/// (where `base = NODE_ID_PANE_ROW_OFFSET + pane_id * MAX_ROWS_PER_PANE`).
///
/// The caller must guarantee `scrollback_idx < MAX_SCROLLBACK_ROWS_PER_PANE`;
/// otherwise the resulting NodeId may collide with the row of the next pane.
pub fn pane_scrollback_row_node_id(pane_id: u32, scrollback_idx: u16) -> NodeId {
    debug_assert!((scrollback_idx as u64) < MAX_SCROLLBACK_ROWS_PER_PANE);
    NodeId(
        NODE_ID_PANE_ROW_OFFSET
            + (pane_id as u64) * MAX_ROWS_PER_PANE
            + MAX_VIEWPORT_ROWS_PER_PANE
            + scrollback_idx as u64,
    )
}

/// Pure function that converts a row of `Grid` to SR-oriented text (Sprint 5-11-3).
///
/// Behavior:
/// - Concatenates each cell's `ch` (drops SGR / color info; SR does not need it).
/// - Trims trailing ASCII spaces with `trim_end()` (prevents SR from reading "60 spaces").
/// - Returns `" "` if the result is an empty string (preserves SR's empty-line boundary).
/// - Returns `" "` if `row` is out of range (panic safe).
///
/// CJK characters and emoji are preserved. `trim_end` only removes ASCII spaces,
/// so consecutive ideographic spaces (U+3000) are preserved (intentional).
pub fn pane_row_text(grid: &nexterm_proto::Grid, row: usize) -> String {
    let Some(cells) = grid.rows.get(row) else {
        return " ".to_string();
    };
    let mut text: String = cells.iter().map(|c| c.ch).collect();
    // Remove the trailing run of ASCII spaces (strips right-side padding).
    let trimmed = text.trim_end_matches(' ');
    if trimmed.is_empty() {
        " ".to_string()
    } else {
        text.truncate(trimmed.len());
        text
    }
}

/// Internal helper that converts a cell row to SR text + `character_lengths` (Sprint 5-11-4).
///
/// Return value `(text, lengths)`:
/// - `text`: built with the same logic as `pane_row_text` (trim_end + `" "` for empty).
/// - `lengths`: UTF-8 byte length of each `char` in `text`.
///   `lengths.iter().map(|&b| b as usize).sum::<usize>() == text.len()` always holds.
///
/// Following AccessKit's `Node::set_character_lengths` contract, we treat "1 char = 1
/// character" so CJK and emoji each count as 1 character (consistent with ASCII).
/// Width differences between half-width and full-width should ideally be expressed
/// with `character_widths`, but this implementation omits that (still works for SR).
fn cells_to_row_text_with_lengths(cells: &[nexterm_proto::Cell]) -> (String, Vec<u8>) {
    let mut text: String = cells.iter().map(|c| c.ch).collect();
    let trimmed_len_bytes = text.trim_end_matches(' ').len();
    if trimmed_len_bytes == 0 {
        // Empty rows use " " to preserve the SR boundary.
        return (" ".to_string(), vec![1]);
    }
    text.truncate(trimmed_len_bytes);
    let lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
    (text, lengths)
}

/// Convert the specified row of `Grid` to SR text + `character_lengths` (Sprint 5-11-4).
///
/// The `text` portion matches `pane_row_text`. Used when setting `set_value` /
/// `set_character_lengths` on an AccessKit `Role::TextRun` node.
pub fn pane_row_text_with_lengths(grid: &nexterm_proto::Grid, row: usize) -> (String, Vec<u8>) {
    let Some(cells) = grid.rows.get(row) else {
        return (" ".to_string(), vec![1]);
    };
    cells_to_row_text_with_lengths(cells)
}

/// Convert one scrollback line to SR text + `character_lengths` (Sprint 5-11-4).
///
/// Uses the same cell -> text conversion as `pane_row_text_with_lengths`.
pub fn scrollback_row_text_with_lengths(line: &[nexterm_proto::Cell]) -> (String, Vec<u8>) {
    cells_to_row_text_with_lengths(line)
}

/// Compute an AccessKit `TextPosition::character_index` from cell column `cursor_col` (Sprint 5-11-4).
///
/// Behavior:
/// - The row text is built 1:1 with the cell row (`cells.iter().map(|c| c.ch).collect()`).
/// - `cursor_col` is the grid cell column. If it exceeds `text.chars().count()`,
///   clamp to the end-of-text position.
/// - Placeholder cells for wide characters (' ') also count as 1 character, so the
///   cell column that `cursor_col` points to can be used as the character_index directly.
///
/// Examples:
/// - text="abc" (chars=3), cursor_col=1 -> 1
/// - text="abc" (chars=3), cursor_col=5 -> 3 (clamped to end)
/// - text="あい" (chars=2, cell width 4 including placeholder), cursor_col=2 -> 2
pub fn cursor_character_index(text: &str, cursor_col: u16) -> usize {
    let char_count = text.chars().count();
    (cursor_col as usize).min(char_count)
}

/// Compute per-row text hashes for the given pane (Sprint 5-11-3).
///
/// Used to populate the cache in `EventHandler::last_grid_row_hashes`. Returns a
/// `Vec<u64>` of `DefaultHasher` hashes for each row's `pane_row_text` output.
/// Length equals `min(grid.height, grid.rows.len())`.
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

/// Compute the NodeId for a palette candidate from its idx.
fn palette_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_PALETTE_ITEM_OFFSET + idx as u64)
}

/// Compute the NodeId for a host list entry from its idx.
fn host_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_HOST_ITEM_OFFSET + idx as u64)
}

/// Compute the NodeId for a macro list entry from its idx.
fn macro_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_MACRO_ITEM_OFFSET + idx as u64)
}

/// Compute the NodeId for a context menu item from its idx.
fn context_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_CONTEXT_ITEM_OFFSET + idx as u64)
}

/// Compute the NodeId for a Quick Select match item from its idx (Step 2-2-h).
fn quickselect_item_id(idx: usize) -> NodeId {
    NodeId(NODE_ID_QUICKSELECT_ITEM_OFFSET + idx as u64)
}

// ===== NodeId reverse lookup (Step 2-4) =====

/// `NodeId` kind (used to dispatch Action responses).
///
/// The `ActionRequest::target_node` received from the platform a11y adapter is
/// decoded into this enum via `decode_node_id`, and Focus / Click / SetValue are
/// handled according to the kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeIdKind {
    /// Root (the entire OS Window).
    Root,
    /// Tab bar (`TabList`).
    TabBar,
    /// Pane area (`Group`).
    PaneArea,
    /// Root of the settings panel.
    SettingsPanel,
    /// Root of the command palette.
    Palette,
    /// Root of the host manager.
    HostManager,
    /// Root of the macro picker.
    MacroPicker,
    /// Root of the context menu.
    ContextMenu,
    /// Root of the close confirmation dialog.
    CloseDialog,
    /// Update notification banner.
    UpdateBanner,
    /// Root of the Quick Select overlay.
    QuickSelect,
    /// Palette search input field.
    PaletteSearch,
    /// Palette candidate list (ListBox).
    PaletteList,
    /// "Kill" button of the close confirmation dialog.
    CloseDialogKill,
    /// "Cancel" button of the close confirmation dialog.
    CloseDialogCancel,
    /// Quick Select match list (ListBox).
    QuickSelectList,
    /// Tab node (identified by `pane_id`).
    Tab { pane_id: u32 },
    /// Pane node (identified by `pane_id`).
    Pane { pane_id: u32 },
    /// Palette candidate item (`idx` in `filtered()`).
    PaletteItem { idx: usize },
    /// Host list item (`idx` in `filtered()`).
    HostItem { idx: usize },
    /// Macro list item (`idx` in `filtered()`).
    MacroItem { idx: usize },
    /// Context menu item (`idx` in `items`).
    ContextItem { idx: usize },
    /// Quick Select match item (`idx` in `matches`).
    QuickSelectItem { idx: usize },
    /// Settings panel: category TabList.
    SettingsTabList,
    /// Settings panel: a category tab (`idx` in `SettingsCategory::ALL`).
    SettingsTab { idx: usize },
    /// Custom title bar: minimize window button.
    WindowMinimizeButton,
    /// Custom title bar: maximize / restore window button.
    WindowMaximizeButton,
    /// Custom title bar: close window button.
    WindowCloseButton,
    /// Settings panel: content container for the current category.
    SettingsContent,
    /// Settings panel: color scheme picker.
    /// A node built from a `WidgetSpec` (UI/UX v3 P1b). `category` is the
    /// `SettingsCategory::ALL` index, `index` the widget's position in it.
    SettingsWidget {
        /// Owning settings category.
        category: u8,
        /// Widget index within that category.
        index: u16,
    },
    /// Pane row node (Sprint 5-11-3, identified by `pane_id` and `row`).
    PaneRow { pane_id: u32, row: u16 },
    /// Pane scrollback row node (Sprint 5-11-4, identified by `pane_id` and
    /// `idx` = index from the start of scrollback).
    PaneScrollbackRow { pane_id: u32, idx: u16 },
    /// SR alert region container (Sprint 5-11-5).
    AlertRegion,
    /// Individual SR alert node (Sprint 5-11-5, identified by `AlertEntry.seq`).
    Alert { seq: u64 },
    /// Phase 5-11-7: terminal input buffer (for PTY writes to the focused pane).
    PaneInputBuffer,
    /// Phase 5-11-8 Step 8-3 Sub-phase D: SSH delete confirmation dialog body (Role::AlertDialog).
    SettingsSshDeleteDialog,
    /// Phase 5-11-8 Step 8-3 Sub-phase D: SSH delete confirmation dialog "Delete" confirm button.
    SettingsSshDeleteConfirmBtn,
    /// Phase 5-11-8 Step 8-3 Sub-phase D: SSH delete confirmation dialog "Cancel" button.
    SettingsSshDeleteCancelBtn,
    /// Phase 5-11-9 Sub-phase E: Keybindings category list item
    /// (`idx` is the index in `SettingsPanel.keybindings`).
    SettingsKeyBindingItem { idx: usize },
    /// Phase 5-11-9 Sub-phase E: key field of the selected binding (TextInput).
    SettingsKeyFieldKey,
    /// Phase 5-11-9 Sub-phase E: action field of the selected binding (ComboBox).
    SettingsKeyFieldAction,
    /// Phase 5-11-9 Sub-phase E: Keybindings category add-binding button.
    SettingsKeyAddBtn,
    /// Phase 5-11-9 Sub-phase E: Keybindings category delete-binding button.
    SettingsKeyDeleteBtn,
    /// Phase 5-11-9 Sub-phase E: Keybindings delete confirmation dialog body.
    SettingsKeyDeleteDialog,
    /// Phase 5-11-9 Sub-phase E: Keybindings delete confirmation "Delete" button.
    SettingsKeyDeleteConfirmBtn,
    /// Phase 5-11-9 Sub-phase E: Keybindings delete confirmation "Cancel" button.
    SettingsKeyDeleteCancelBtn,
    /// Unknown / out-of-range NodeId.
    Unknown,
}

/// Reverse-decode a `NodeId` into a `NodeIdKind` (Step 2-4).
///
/// Offset range table (consistent with the constants at the top of `accessibility.rs`):
///
/// | Range | Kind |
/// |---|---|
/// | 1..16 | Fixed nodes (base + overlay roots) |
/// | 17 | `SettingsTabList` |
/// | 60..99 | `SettingsTab { idx: id - 60 }` |
/// | 25 | `SettingsContent` |
/// | 26 | `AlertRegion` (Sprint 5-11-5) |
/// | 27 | `PaneInputBuffer` (Phase 5-11-7) |
/// | 28..29 | reserved |
/// | 30..39 | **free** — every hand-written settings-field node (font family/size, theme scheme, window opacity, startup language/auto-update, cursor style, padding x/y, present mode) was replaced by `SettingsWidget` in UI/UX v3 P1b/P1c |
/// | 40..44 | settings fields Phase 5-11-8 Step 8-2 (SshFieldName / Host / Port / Username / AuthType) |
/// | 45..49 | settings fields Phase 5-11-8 Step 8-3 (SshAddBtn / SshDeleteBtn / SshDeleteDialog / SshDeleteConfirmBtn / SshDeleteCancelBtn) |
/// | 50..56 | settings fields Phase 5-11-9 Sub-phase E (KeyFieldKey / KeyFieldAction / KeyAddBtn / KeyDeleteBtn / KeyDeleteDialog / KeyDeleteConfirmBtn / KeyDeleteCancelBtn) |
/// | 57..59 | custom title bar window buttons (Minimize / Maximize / Close) |
/// | 60..99 | `SettingsTab { idx: id - 60 }` (also listed above) |
/// | 100M..200M | `PaletteItem { idx: id - 100M }` |
/// | 200M..300M | `HostItem { idx: id - 200M }` |
/// | 300M..400M | `MacroItem { idx: id - 300M }` |
/// | 400M..500M | `ContextItem { idx: id - 400M }` |
/// | 500M..600M | `QuickSelectItem { idx: id - 500M }` |
/// | 600M..700M | retired — carried `SettingsProfileItem` until the Profiles category moved onto the widget layer (UI/UX v3 P1c) |
/// | 700M..800M | `SettingsWidget { category, index }` — `SETTINGS_WIDGET_BASE + WidgetId::as_u32()` (UI/UX v3 P1b/P1c). Widest encodable offset is `0xFF_FFFF` ≈ 16.8M, so the 100M-wide slot has room to spare |
/// | 800M..900M | retired — carried `SettingsSshHostItem` until the Ssh category moved onto the widget layer (UI/UX v3 P1c) |
/// | 900M..1G | `SettingsKeyBindingItem { idx: id - 900M }` (Phase 5-11-9 Sub-phase E) |
/// | 1G..1G+u32::MAX | `Tab { pane_id: id - 1G }` |
/// | 10G..10G+u32::MAX | `Pane { pane_id: id - 10G }` |
/// | 20G..~4.3T | `PaneRow` / `PaneScrollbackRow` (Sprint 5-11-3 / 5-11-4) |
/// | 50T..u64::MAX | `Alert { seq: id - 50T }` (Sprint 5-11-5) |
/// | other | `Unknown` |
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
        60..=99 => NodeIdKind::SettingsTab {
            idx: (raw - SETTINGS_TAB_BASE) as usize,
        },
        25 => NodeIdKind::SettingsContent,
        26 => NodeIdKind::AlertRegion,
        // Phase 5-11-7: terminal input buffer
        27 => NodeIdKind::PaneInputBuffer,
        700_000_000..=799_999_999 => {
            match crate::renderer::overlay::widgets::spec::WidgetId::from_u32(
                (raw - SETTINGS_WIDGET_BASE) as u32,
            ) {
                Some(w) => NodeIdKind::SettingsWidget {
                    category: w.category,
                    index: w.index,
                },
                None => NodeIdKind::Unknown,
            }
        }
        // Phase 5-11-6 #6: 4 new Window category fields
        // Phase 5-11-8 Step 8-3 Sub-phase D: delete confirmation dialog
        // (40..=46 were the hand-written SSH field/button nodes, retired in
        // UI/UX v3 P1c when the Ssh category moved onto the widget layer).
        47 => NodeIdKind::SettingsSshDeleteDialog,
        48 => NodeIdKind::SettingsSshDeleteConfirmBtn,
        49 => NodeIdKind::SettingsSshDeleteCancelBtn,
        // Phase 5-11-9 Sub-phase E: Keybindings editor fields + Add/Delete + dialog
        50 => NodeIdKind::SettingsKeyFieldKey,
        51 => NodeIdKind::SettingsKeyFieldAction,
        52 => NodeIdKind::SettingsKeyAddBtn,
        53 => NodeIdKind::SettingsKeyDeleteBtn,
        54 => NodeIdKind::SettingsKeyDeleteDialog,
        55 => NodeIdKind::SettingsKeyDeleteConfirmBtn,
        56 => NodeIdKind::SettingsKeyDeleteCancelBtn,
        // Custom title bar window buttons.
        57 => NodeIdKind::WindowMinimizeButton,
        58 => NodeIdKind::WindowMaximizeButton,
        59 => NodeIdKind::WindowCloseButton,
        _ => decode_dynamic(raw),
    }
}

/// Decode dynamic offset ranges (helper for `decode_node_id`).
fn decode_dynamic(raw: u64) -> NodeIdKind {
    // Width of each dynamic offset range. Computed as the gap to the next offset.
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
    // Phase 5-11-9 Sub-phase E: SettingsPanel Keybindings item range: [900M, 1G)
    if (NODE_ID_SETTINGS_KEY_BINDING_OFFSET..NODE_ID_SETTINGS_KEY_BINDING_OFFSET + DYN_RANGE)
        .contains(&raw)
    {
        return NodeIdKind::SettingsKeyBindingItem {
            idx: (raw - NODE_ID_SETTINGS_KEY_BINDING_OFFSET) as usize,
        };
    }
    // Tab range: [1e9, 1e9 + u32::MAX] = [1e9, 1e9 + ~4.29e9] ≈ [1e9, 5.3e9]
    if (NODE_ID_TAB_OFFSET..NODE_ID_TAB_OFFSET + (u32::MAX as u64) + 1).contains(&raw) {
        return NodeIdKind::Tab {
            pane_id: (raw - NODE_ID_TAB_OFFSET) as u32,
        };
    }
    // Pane range: [1e10, 1e10 + u32::MAX]
    if (NODE_ID_PANE_OFFSET..NODE_ID_PANE_OFFSET + (u32::MAX as u64) + 1).contains(&raw) {
        return NodeIdKind::Pane {
            pane_id: (raw - NODE_ID_PANE_OFFSET) as u32,
        };
    }
    // Pane row range (Sprint 5-11-3 + 5-11-4):
    //   [2e10, 2e10 + u32::MAX * MAX_ROWS_PER_PANE + (MAX_ROWS_PER_PANE - 1)]
    // Per-pane layout:
    //   - offset 0..MAX_VIEWPORT_ROWS_PER_PANE (0..1000): viewport row -> PaneRow
    //   - offset MAX_VIEWPORT_ROWS_PER_PANE..MAX_ROWS_PER_PANE (1000..10000):
    //     scrollback row -> PaneScrollbackRow
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
    // SR alert range (Sprint 5-11-5): [50T, u64::MAX].
    // The practical upper bound of `next_alert_seq` is far above this, so the upper
    // bound is effectively u64::MAX. Far enough from the pane row range upper bound
    // `pane_row_range_end` (~4.3e13) that no collision is possible.
    if raw >= NODE_ID_ALERT_OFFSET {
        return NodeIdKind::Alert {
            seq: raw - NODE_ID_ALERT_OFFSET,
        };
    }
    NodeIdKind::Unknown
}

/// Build an AccessKit tree from `ClientState`.
///
/// ## Structure
///
/// **Base (tabs and panes):**
/// ```text
/// Window "Nexterm"
///   ├─ TabList "Terminal tabs"
///   │    ├─ Tab "Tab 1: <title>"  (selected if focused)
///   │    └─ Tab ...
///   └─ Group "Panes"
///        ├─ Terminal "<title>"  (description: "Working directory: <cwd>")
///        └─ Terminal ...
/// ```
///
/// **With an overlay visible (one frontmost overlay is added and focus moves to it):**
/// Priority order (high to low):
/// 1. `CloseWindowDialog` (AlertDialog, modal)
/// 2. `ContextMenu` (Menu, modal)
/// 3. `CommandPalette` (Dialog with SearchInput + ListBox)
/// 4. `HostManager` (Dialog with ListBox)
/// 5. `MacroPicker` (Dialog with ListBox)
/// 6. `SettingsPanel` (Dialog; detailed expansion happens in Step 2-2-e)
///
/// **Non-modal**:
/// - `update_banner`: `Role::Alert`. Does not take focus, but is added as a child
///   of ROOT so it can be announced.
///
/// ## Focus
///
/// - With an overlay open: focus the selected item (or search input) inside the overlay.
/// - No overlay: the pane node for `state.focused_pane_id` (ROOT if unset).
pub fn build_tree_from_state(state: &ClientState) -> TreeUpdate {
    // ===== Build the base nodes (tabs and panes) =====
    let (mut nodes, mut root_children, default_focus) = build_base_nodes(state);

    let mut focus = default_focus;

    // ===== Check overlays in priority order =====
    // Only one overlay is visible at a time; add the highest-priority one.
    //
    // Priority (high to low):
    //   1. CloseWindowDialog (AlertDialog, strongest modal)
    //   2. QuickSelect (its label key consumes all other key input, so effectively modal)
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

    // ===== Non-modal: update banner =====
    if let Some(version) = &state.update_banner {
        nodes.push(build_update_banner_node(version));
        root_children.push(UPDATE_BANNER_ID);
    }

    // ===== Non-modal: SR alert region (Sprint 5-11-5) =====
    // Omit when empty (avoids confusing SR).
    // Bell / OSC 9 / OSC 777 are queued via `ClientState::add_alert` and removed
    // after their TTL by `expire_alerts`, so here we just reflect the current snapshot.
    let alert_nodes = build_alert_region_nodes(&state.alerts);
    if !alert_nodes.is_empty() {
        nodes.extend(alert_nodes);
        root_children.push(ALERT_REGION_ID);
    }

    // ===== Finalize the ROOT node with the final children =====
    // `build_base_nodes` inserts a tentative ROOT; overwrite its children here.
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

/// Build the base nodes (tabs and panes).
///
/// Return value:
/// - `nodes`: ROOT (tentative) / TAB_BAR / PANE_AREA + each tab and pane node.
/// - `root_children`: tentative ROOT children (`[TAB_BAR_ID, PANE_AREA_ID]`).
///   If an overlay is present, the caller appends to this list and overwrites the ROOT.
/// - `focus`: default focus when no overlay is open.
fn build_base_nodes(state: &ClientState) -> (Vec<(NodeId, Node)>, Vec<NodeId>, NodeId) {
    // Determine tab order (fallback if `tab_order` is empty).
    let tab_order: Vec<u32> = if state.tab_order.is_empty() {
        state.panes.keys().copied().collect()
    } else {
        state.tab_order.clone()
    };

    // ===== ROOT node (tentative) =====
    // `build_tree_from_state` rebuilds the final children after the overlay check.
    let mut root = Node::new(Role::Window);
    root.set_label("Nexterm");
    root.set_children(vec![TAB_BAR_ID, PANE_AREA_ID]);

    // ===== TAB_BAR node =====
    let mut tab_bar = Node::new(Role::TabList);
    tab_bar.set_label("Terminal tabs");
    let mut tab_child_ids: Vec<NodeId> = tab_order.iter().copied().map(tab_node_id).collect();
    // Custom title bar: expose the window buttons after the tabs. The hit
    // rects double as the "buttons are visible this frame" signal, so the
    // a11y tree needs no separate config plumbing.
    let window_buttons_visible = state.window_close_hit_rect.is_some();
    if window_buttons_visible {
        tab_child_ids.extend([
            WINDOW_MINIMIZE_BTN_ID,
            WINDOW_MAXIMIZE_BTN_ID,
            WINDOW_CLOSE_BTN_ID,
        ]);
    }
    tab_bar.set_children(tab_child_ids);

    // ===== Window button nodes (custom title bar) =====
    let mut window_button_nodes: Vec<(NodeId, Node)> = Vec::new();
    if window_buttons_visible {
        // Labels stay English like every other node in this tree
        // (localizing the a11y tree is tracked as a separate task).
        for (id, label) in [
            (WINDOW_MINIMIZE_BTN_ID, "Minimize window"),
            (WINDOW_MAXIMIZE_BTN_ID, "Maximize or restore window"),
            (WINDOW_CLOSE_BTN_ID, "Close window"),
        ] {
            let mut btn = Node::new(Role::Button);
            btn.set_label(label);
            btn.add_action(Action::Click);
            window_button_nodes.push((id, btn));
        }
    }

    // ===== Per-tab nodes =====
    let mut tab_nodes: Vec<(NodeId, Node)> = Vec::with_capacity(tab_order.len());
    for (idx, &pane_id) in tab_order.iter().enumerate() {
        let title = state
            .panes
            .get(&pane_id)
            .map(|p| p.title.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Untitled");
        let label = format!("Tab {}: {}", idx + 1, title);
        let mut tab = Node::new(Role::Tab);
        tab.set_label(label);
        if state.focused_pane_id == Some(pane_id) {
            tab.set_selected(true);
        }
        tab_nodes.push((tab_node_id(pane_id), tab));
    }

    // ===== PANE_AREA node =====
    //
    // Phase 5-11-7: in addition to the pane bodies, append PANE_INPUT_BUFFER_ID at the
    // end so SR users can write to the PTY via SetValue.
    let mut pane_area = Node::new(Role::Group);
    pane_area.set_label("Panes");
    let mut pane_child_ids: Vec<NodeId> = tab_order.iter().copied().map(pane_node_id).collect();
    pane_child_ids.push(PANE_INPUT_BUFFER_ID);
    pane_area.set_children(pane_child_ids);

    // ===== Per-pane nodes + pane row nodes (Sprint 5-11-3 / 5-11-4) =====
    //
    // Pane children, in order:
    //   1. Scrollback row nodes (Sprint 5-11-4, `Role::TextRun`)
    //      - Exposed range: `SCROLLBACK_WINDOW_RADIUS` rows around `pane.scroll_offset`.
    //      - Live::Off (implicit): not subject to announcement.
    //   2. Viewport row nodes (Sprint 5-11-3 / 5-11-4, `Role::TextRun`)
    //      - Only the cursor row of the focused pane gets `Live::Polite`
    //        (avoids excessive announcement).
    //
    // The pane body node (`Role::Terminal`) carries the focused pane's cursor
    // position as `TextSelection` (Sprint 5-11-4). SR users get the caret position
    // via row NodeId + character_index and can move through it with arrow keys.
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

        // ----- Scrollback row nodes (sliding window, Sprint 5-11-4) -----
        let scrollback_len = pane.scrollback.len();
        if scrollback_len > 0 {
            // Window center: the scrollback row immediately preceding the viewport (most recent side).
            // `scroll_offset = 0` is the latest screen; `scroll_offset = K` means scrolled up by K rows.
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
                // Scrollback rows stay at Live::Off (default) — not announced.
                let row_id = pane_scrollback_row_node_id(pane_id, idx as u16);
                child_ids.push(row_id);
                pane_nodes.push((row_id, row_node));
            }
        }

        // ----- Viewport row nodes (Sprint 5-11-3 / 5-11-4 promoted to Role::TextRun) -----
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
            // Sprint 5-11-4: Restrict Live::Polite to the cursor row of the focused pane.
            // Marking all viewport rows as Polite would cause SR to announce on every redraw.
            if is_cursor_row {
                row_node.set_live(Live::Polite);
            }
            let row_id = pane_row_node_id(pane_id, row);

            // Cursor row of the focused pane: remember info to set TextSelection on the pane.
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
            pane_node.set_description(format!("Working directory: {}", cwd));
        }
        pane_node.set_children(child_ids);
        if let Some(sel) = pane_text_selection {
            pane_node.set_text_selection(sel);
        }
        pane_nodes.push((pane_node_id(pane_id), pane_node));
    }

    let default_focus = state.focused_pane_id.map_or(ROOT_ID, pane_node_id);

    // ===== Terminal input buffer (Phase 5-11-7) =====
    //
    // Includes the focused pane's title in the description so SR users know which pane
    // they are typing into. On SetValue, the text is forwarded to the focused pane
    // via `PasteText` IPC.
    let mut input_buffer = Node::new(Role::TextInput);
    input_buffer.set_label("Terminal input buffer");
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
        .unwrap_or_else(|| "No focused pane".to_string());
    input_buffer.set_description(format!(
        "Current pane: {} — committing input sends the text to the PTY (use \\n for newline)",
        pane_hint
    ));

    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(4 + tab_nodes.len() + pane_nodes.len());
    nodes.push((ROOT_ID, root));
    nodes.push((TAB_BAR_ID, tab_bar));
    nodes.push((PANE_AREA_ID, pane_area));
    nodes.extend(tab_nodes);
    nodes.extend(window_button_nodes);
    nodes.extend(pane_nodes);
    nodes.push((PANE_INPUT_BUFFER_ID, input_buffer));

    (nodes, vec![TAB_BAR_ID, PANE_AREA_ID], default_focus)
}

// ===== Overlay node builders (Step 2-2-b to 2-2-g) =====

/// Build the nodes for CommandPalette (Step 2-2-b).
///
/// Structure:
/// ```text
/// Dialog "Command palette"
///   ├─ SearchInput "Search" (value: query)
///   └─ ListBox "Candidates"
///        ├─ ListBoxOption "<label>"  (selected if idx == palette.selected)
///        └─ ...
/// ```
///
/// Focus: the selected candidate if at least one exists, otherwise the search input.
fn build_palette_nodes(palette: &CommandPalette) -> (Vec<(NodeId, Node)>, NodeId) {
    let filtered = palette.filtered();
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(3 + filtered.len());

    // ===== Dialog root =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("Command palette");
    dialog.set_modal();
    dialog.set_children(vec![PALETTE_SEARCH_ID, PALETTE_LIST_ID]);
    nodes.push((PALETTE_ID, dialog));

    // ===== SearchInput =====
    let mut search = Node::new(Role::SearchInput);
    search.set_label("Search");
    search.set_value(palette.query.clone());
    nodes.push((PALETTE_SEARCH_ID, search));

    // ===== ListBox =====
    let mut list = Node::new(Role::ListBox);
    list.set_label(format!("{} candidate(s)", filtered.len()));
    let item_ids: Vec<NodeId> = (0..filtered.len()).map(palette_item_id).collect();
    list.set_children(item_ids);
    nodes.push((PALETTE_LIST_ID, list));

    // ===== Each candidate item =====
    for (idx, action) in filtered.iter().enumerate() {
        let mut item = Node::new(Role::ListBoxOption);
        item.set_label(action.label.clone());
        if idx == palette.selected {
            item.set_selected(true);
        }
        nodes.push((palette_item_id(idx), item));
    }

    // Focus: the selected candidate when available, otherwise the search input.
    let focus = if filtered.is_empty() || palette.selected >= filtered.len() {
        PALETTE_SEARCH_ID
    } else {
        palette_item_id(palette.selected)
    };

    (nodes, focus)
}

/// Build the nodes for ContextMenu (Step 2-2-c).
///
/// Structure:
/// ```text
/// Menu (no label, ItemList at position 0)
///   ├─ MenuItem "<label>" (description: hint, focused if hovered)
///   ├─ Splitter (separator)
///   └─ ...
/// ```
///
/// Focus: the hovered item, otherwise the menu itself.
fn build_context_menu_nodes(menu: &ContextMenu) -> (Vec<(NodeId, Node)>, NodeId) {
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(1 + menu.items.len());

    // ===== Menu root =====
    let mut menu_node = Node::new(Role::Menu);
    menu_node.set_label("Context menu");
    let item_ids: Vec<NodeId> = (0..menu.items.len()).map(context_item_id).collect();
    menu_node.set_children(item_ids);
    nodes.push((CONTEXT_MENU_ID, menu_node));

    // ===== Each menu item =====
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
            // Put the key-binding hint in the description (SR announces "Ctrl+C" etc. as supplement).
            node.set_description(item.hint.clone());
        }
        nodes.push((context_item_id(idx), node));
    }

    // Focus: the hovered item, otherwise the menu itself.
    let focus = menu
        .hovered
        .filter(|&idx| idx < menu.items.len())
        .map(context_item_id)
        .unwrap_or(CONTEXT_MENU_ID);

    (nodes, focus)
}

/// Build the nodes for CloseWindowDialog (Step 2-2-d).
///
/// Structure:
/// ```text
/// AlertDialog "Close window?" (modal)
///   ├─ Label <message>  (embedded as Paragraph)
///   ├─ Button <kill_label>  (selected if selected_button == 0)
///   └─ Button <cancel_label>  (selected if selected_button == 1)
/// ```
///
/// Focus: the button indicated by `selected_button`.
fn build_close_dialog_nodes(dialog: &CloseWindowDialog) -> (Vec<(NodeId, Node)>, NodeId) {
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(3);

    // ===== AlertDialog root =====
    let mut alert = Node::new(Role::AlertDialog);
    alert.set_label("Close window?");
    // Embed the message body as the description (SR reads it as the dialog summary).
    alert.set_description(dialog.message.clone());
    alert.set_modal();
    alert.set_children(vec![CLOSE_DIALOG_KILL_BTN, CLOSE_DIALOG_CANCEL_BTN]);
    nodes.push((CLOSE_DIALOG_ID, alert));

    // ===== Kill (kill process / force close) button =====
    let mut kill_btn = Node::new(Role::Button);
    kill_btn.set_label(dialog.kill_label.clone());
    if dialog.selected_button == 0 {
        kill_btn.set_selected(true);
    }
    nodes.push((CLOSE_DIALOG_KILL_BTN, kill_btn));

    // ===== Cancel button =====
    let mut cancel_btn = Node::new(Role::Button);
    cancel_btn.set_label(dialog.cancel_label.clone());
    if dialog.selected_button == 1 {
        cancel_btn.set_selected(true);
    }
    nodes.push((CLOSE_DIALOG_CANCEL_BTN, cancel_btn));

    let focus = match dialog.selected_button {
        0 => CLOSE_DIALOG_KILL_BTN,
        1 => CLOSE_DIALOG_CANCEL_BTN,
        // Confirmed values (0xFE / 0xFF) are a draw-timing edge case. Focus Kill.
        _ => CLOSE_DIALOG_KILL_BTN,
    };

    (nodes, focus)
}

/// Build the nodes for HostManager (Step 2-2-f).
fn build_host_manager_nodes(manager: &HostManager) -> (Vec<(NodeId, Node)>, NodeId) {
    let filtered = manager.filtered();
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(1 + filtered.len());

    // ===== Dialog root =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("SSH host manager");
    dialog.set_modal();
    let item_ids: Vec<NodeId> = (0..filtered.len()).map(host_item_id).collect();
    dialog.set_children(item_ids);
    nodes.push((HOST_MANAGER_ID, dialog));

    // ===== Each host item =====
    for (idx, host) in filtered.iter().enumerate() {
        let mut item = Node::new(Role::ListBoxOption);
        let label = if host.name.is_empty() {
            format!("{}@{}", host.username, host.host)
        } else {
            host.name.clone()
        };
        item.set_label(label);
        // Add host name / username as a supplement in the description.
        let desc = format!(
            "Host: {}, user: {}, port: {}",
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

/// Build the nodes for Quick Select (Step 2-2-h).
///
/// Structure:
/// ```text
/// Dialog "Quick Select" (modal)
///   ├─ description: "Typing label: '<typed_label>'" (empty -> "Pick an item by label key")
///   └─ ListBox "{n} match(es)" (id=16)
///        ├─ ListBoxOption "[a] <text>"  (selected if matches[idx].label.starts_with(typed_label))
///        └─ ...
/// ```
///
/// **Focus strategy**:
/// - If `typed_label` narrows down to one or more prefix-matched items, the first prefix match.
/// - Otherwise: the first match if any, or the ListBox itself.
///
/// **Design notes**:
/// - Reason for not making the search input a separate node: Quick Select commits
///   instantly on every key press, which does not fit the AccessKit `SearchInput`
///   model. `typed_label` is supplied as the Dialog's `description` instead (SR
///   reads it as the dialog state).
fn build_quick_select_nodes(qs: &QuickSelectState) -> (Vec<(NodeId, Node)>, NodeId) {
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(2 + qs.matches.len());

    // ===== Dialog root =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("Quick Select");
    dialog.set_modal();
    let desc = if qs.typed_label.is_empty() {
        "Press a label key to copy an item to the clipboard".to_string()
    } else {
        format!("Typing label: '{}'", qs.typed_label)
    };
    dialog.set_description(desc);
    dialog.set_children(vec![QUICK_SELECT_LIST_ID]);
    nodes.push((QUICK_SELECT_ID, dialog));

    // ===== ListBox =====
    let mut list = Node::new(Role::ListBox);
    list.set_label(format!("{} match(es)", qs.matches.len()));
    let item_ids: Vec<NodeId> = (0..qs.matches.len()).map(quickselect_item_id).collect();
    list.set_children(item_ids);
    nodes.push((QUICK_SELECT_LIST_ID, list));

    // ===== Each match item =====
    // Use the first prefix-matched item as the focus candidate.
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

    // Focus: prefix-matched item -> first match -> ListBox itself (no matches).
    let focus = match focus_idx {
        Some(idx) => quickselect_item_id(idx),
        None if !qs.matches.is_empty() => quickselect_item_id(0),
        None => QUICK_SELECT_LIST_ID,
    };

    (nodes, focus)
}

/// Build the nodes for MacroPicker (Step 2-2-f).
fn build_macro_picker_nodes(picker: &MacroPicker) -> (Vec<(NodeId, Node)>, NodeId) {
    let filtered = picker.filtered();
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(1 + filtered.len());

    // ===== Dialog root =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("Lua macro picker");
    dialog.set_modal();
    let item_ids: Vec<NodeId> = (0..filtered.len()).map(macro_item_id).collect();
    dialog.set_children(item_ids);
    nodes.push((MACRO_PICKER_ID, dialog));

    // ===== Each macro item =====
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

/// Build the nodes for SettingsPanel (Step 2-2-e', TabList + each category's detailed fields).
///
/// ## Tree structure
///
/// ```text
/// Dialog "Settings"
///   ├─ TabList "Categories"
///   │    ├─ Tab "Startup"
///   │    ├─ Tab "Font"  (selected if category == Font)
///   │    ├─ Tab "Theme"
///   │    ├─ Tab "Window"
///   │    ├─ Tab "SSH"
///   │    ├─ Tab "Keybindings"
///   │    └─ Tab "Profiles"
///   └─ Group "<current category name>"
///        ├─ TextInput "Font family" (Font category only)
///        ├─ Slider "Font size" with numeric_value (Font category only)
///        ├─ ComboBox "Color scheme" (Theme category only)
///        ├─ Slider "Opacity" (Window category only)
///        ├─ ComboBox "Language" (Startup category only)
///        ├─ CheckBox "Check for updates on startup" (Startup category only)
///        ├─ ListBox "Profile list" (Profiles category only, Phase 5-11-7)
///        │    └─ ListBoxOption × N
///        ├─ (Ssh category only, Phase 5-11-7): guidance text exposed via description (no fields)
///        └─ (Keybindings category only, Phase 5-11-7): guidance text exposed via description (no fields)
/// ```
///
/// Focus: the editing field while `font_family_editing` is true; for the Window
/// category, follows `window_field_focus`; otherwise the current category tab.
/// Convert one widget description into an AccessKit node (UI/UX v3 P1b).
///
/// The role is derived from the widget kind, so a control's accessible role
/// and its on-screen shape can never disagree: both come from `WidgetKind`.
fn widget_node(desc: &crate::renderer::overlay::widgets::spec::WidgetDesc) -> Node {
    use crate::renderer::overlay::widgets::spec::WidgetKind;

    let mut node = Node::new(match &desc.kind {
        WidgetKind::Label => Role::Label,
        WidgetKind::Toggle { .. } => Role::CheckBox,
        WidgetKind::Cycle { .. } => Role::ComboBox,
        WidgetKind::Slider { .. } => Role::Slider,
        WidgetKind::SpinButton { .. } => Role::SpinButton,
        WidgetKind::Text { .. } => Role::TextInput,
        // A key capture reads as a text input too: its value is a string the
        // user replaces, even though the replacement arrives as a key press.
        WidgetKind::KeyCapture { .. } => Role::TextInput,
        // A swatch is one choice among the scheme strip, which is what a
        // radio button models.
        WidgetKind::Swatch { .. } => Role::RadioButton,
        WidgetKind::ListItem { .. } => Role::ListBoxOption,
        WidgetKind::Button { .. } => Role::Button,
    });
    node.set_label(desc.label.clone());
    if let Some(value) = desc.value_text() {
        node.set_value(value);
    }
    if let Some(hint) = &desc.tooltip {
        node.set_description(hint.clone());
    }
    match &desc.kind {
        WidgetKind::Toggle { on } => node.set_toggled(if *on {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        }),
        WidgetKind::Swatch { selected: true, .. } | WidgetKind::ListItem { selected: true } => {
            node.set_selected(true)
        }
        WidgetKind::Slider {
            value,
            min,
            max,
            step,
            ..
        }
        | WidgetKind::SpinButton {
            value,
            min,
            max,
            step,
            ..
        } => {
            node.set_numeric_value(*value as f64);
            node.set_min_numeric_value(*min as f64);
            node.set_max_numeric_value(*max as f64);
            node.set_numeric_value_step(*step as f64);
        }
        _ => {}
    }
    if !desc.enabled {
        node.set_disabled();
    }
    node
}

/// NodeId the reported focus should sit on for a widget-migrated category.
///
/// Every migrated tab keeps its own focus counter until they are collapsed
/// into one `focused_widget_id`, so the mapping is per category. Returns
/// `None` for categories that are not migrated (or, like Blocks, have no
/// focus counter at all), letting the caller fall through to its own rules.
///
/// This must stay in step with the `Action::Focus` arm of
/// `dispatch_settings_action`: that arm writes the counter, and this reads it
/// back. If one knows about a category and the other does not, a screen
/// reader moves its virtual cursor and the reported focus never follows.
fn widget_focus_id(panel: &SettingsPanel) -> Option<NodeId> {
    use crate::settings_panel::SettingsCategory;

    use crate::renderer::overlay::widgets::settings_font::{FONT_CATEGORY, FONT_ROW_COUNT};
    use crate::renderer::overlay::widgets::settings_security::{
        SECURITY_CATEGORY, SECURITY_ROW_COUNT,
    };
    use crate::renderer::overlay::widgets::settings_startup::{
        STARTUP_CATEGORY, STARTUP_ROW_COUNT,
    };
    use crate::renderer::overlay::widgets::settings_theme::{THEME_CATEGORY, THEME_SWATCH_BASE};
    use crate::renderer::overlay::widgets::settings_window::{WINDOW_CATEGORY, WINDOW_ROW_COUNT};
    use crate::renderer::overlay::widgets::spec::WidgetId;

    let (category, index, count) = match panel.category {
        // Editing the family field pins focus there regardless of the counter.
        SettingsCategory::Font if panel.font_family_editing => {
            use crate::renderer::overlay::widgets::settings_font::row;
            (FONT_CATEGORY, row::FAMILY, FONT_ROW_COUNT)
        }
        SettingsCategory::Font => (FONT_CATEGORY, panel.font_field_focus, FONT_ROW_COUNT),
        SettingsCategory::Startup => (
            STARTUP_CATEGORY,
            panel.startup_field_focus,
            STARTUP_ROW_COUNT,
        ),
        // The Theme counter addresses rows only; the swatches are picked with
        // the mouse, so the counter can never point at one.
        SettingsCategory::Theme => (
            THEME_CATEGORY,
            panel.theme_field_focus,
            THEME_SWATCH_BASE as usize,
        ),
        SettingsCategory::Window => (WINDOW_CATEGORY, panel.window_field_focus, WINDOW_ROW_COUNT),
        SettingsCategory::Security => (
            SECURITY_CATEGORY,
            panel.security_field_focus,
            SECURITY_ROW_COUNT,
        ),
        // Profiles has no focus counter: the reported focus follows the list
        // selection, matching the pre-migration behaviour.
        SettingsCategory::Profiles if !panel.profiles.is_empty() => {
            use crate::renderer::overlay::widgets::settings_profiles::{PROFILES_CATEGORY, row};
            let sel = panel.selected_profile.min(panel.profiles.len() - 1);
            (
                PROFILES_CATEGORY,
                row::LIST_BASE + sel as u16,
                1 + panel.profiles.len(),
            )
        }
        // Ssh: the counter covers the list (0 → the selected windowed entry),
        // the five fields (1..=5, the identity onto the widget indices) and
        // the two buttons (6/7). While the delete dialog is open the
        // hand-written dialog buttons own the focus, so report nothing and
        // let the caller's dialog rule take over.
        SettingsCategory::Ssh if !panel.ssh_delete_dialog_open => {
            use crate::renderer::overlay::widgets::settings_ssh::{SSH_CATEGORY, row};
            let empty = panel.ssh_hosts.is_empty();
            let index = match panel.ssh_field_focus {
                0 if !empty => {
                    row::LIST_BASE + panel.selected_host_index.min(panel.ssh_hosts.len() - 1) as u16
                }
                f @ 1..=5 if !empty => f as u16,
                6 => row::ADD,
                7 if !empty => row::DELETE,
                _ => return None,
            };
            return Some(settings_widget_id(WidgetId::new(SSH_CATEGORY, index)));
        }
        // Blocks is mouse-only; Keybindings is not migrated.
        _ => return None,
    };
    ((index as usize) < count).then(|| settings_widget_id(WidgetId::new(category, index)))
}

fn build_settings_panel_nodes(panel: &SettingsPanel) -> (Vec<(NodeId, Node)>, NodeId) {
    use crate::settings_panel::{KeyEditMode, SettingsCategory};

    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(16);

    let current_idx = SettingsCategory::ALL
        .iter()
        .position(|c| c == &panel.category)
        .unwrap_or(0);

    // ===== Dialog (root) =====
    let mut dialog = Node::new(Role::Dialog);
    dialog.set_label("Settings");
    dialog.set_modal();
    dialog.set_description(format!("Category: {}", panel.category.label()));
    // Phase 5-11-8 Step 8-3 (Sub-phase D): dynamically add the SSH delete confirmation
    // dialog. SR recognizes it as a modal child of SettingsPanel.
    let mut panel_children = vec![SETTINGS_TABLIST_ID, SETTINGS_CONTENT_ID];
    if panel.ssh_delete_dialog_open
        && matches!(panel.category, SettingsCategory::Ssh)
        && !panel.ssh_hosts.is_empty()
    {
        panel_children.push(SETTINGS_SSH_DELETE_DIALOG_ID);
    }
    // Phase 5-11-9 Sub-phase E: Keybindings delete confirmation dialog.
    if panel.key_delete_dialog_open
        && matches!(panel.category, SettingsCategory::Keybindings)
        && !panel.keybindings.is_empty()
    {
        panel_children.push(SETTINGS_KEY_DELETE_DIALOG_ID);
    }
    dialog.set_children(panel_children);
    nodes.push((SETTINGS_PANEL_ID, dialog));

    // ===== TabList (category tabs) =====
    let tab_ids: Vec<NodeId> = (0..SettingsCategory::ALL.len())
        .map(settings_tab_id_at)
        .collect();
    let mut tablist = Node::new(Role::TabList);
    tablist.set_label("Categories");
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

    // ===== Content Group (fields of the current category) =====
    let mut content_children: Vec<NodeId> = Vec::new();
    // For field-less categories like SSH / Keybindings, expose guidance text via the
    // content Group's description. Default is None (leave unchanged when there is
    // nothing to show).
    let mut content_description: Option<String> = None;

    match panel.category {
        SettingsCategory::Font => {
            for desc in crate::renderer::overlay::widgets::settings_font::font_widget_descs(panel) {
                let id = settings_widget_id(desc.id);
                nodes.push((id, widget_node(&desc)));
                content_children.push(id);
            }
        }
        SettingsCategory::Theme => {
            // UI/UX v3 P1b: built from the same descriptions the renderer and
            // the hit-test consume, so the tree can no longer drift from what
            // is on screen. This also exposes the follow-system toggle and the
            // nine scheme swatches, which the hand-written tree omitted.
            for desc in crate::renderer::overlay::widgets::settings_theme::theme_widget_descs(panel)
            {
                let id = settings_widget_id(desc.id);
                nodes.push((id, widget_node(&desc)));
                content_children.push(id);
            }
        }
        SettingsCategory::Window => {
            // UI/UX v3 P1c: same treatment as Theme. The hand-written tree
            // carried 5 of the 14 rows; all 14 are now exposed, with the
            // slider ranges coming from the widget kind so the announced
            // min/max cannot drift from what the control accepts.
            for desc in
                crate::renderer::overlay::widgets::settings_window::window_widget_descs(panel)
            {
                let id = settings_widget_id(desc.id);
                nodes.push((id, widget_node(&desc)));
                content_children.push(id);
            }
        }
        SettingsCategory::Startup => {
            for desc in
                crate::renderer::overlay::widgets::settings_startup::startup_widget_descs(panel)
            {
                let id = settings_widget_id(desc.id);
                nodes.push((id, widget_node(&desc)));
                content_children.push(id);
            }
        }
        SettingsCategory::Profiles => {
            // UI/UX v3 P1c: the active-profile cycler and each entry are
            // widget nodes now, replacing the hand-written ListBoxOption list
            // (Phase 5-11-7). `widget_node` keeps the ListBoxOption role for
            // entries and marks the selected one, so a screen reader hears
            // the same list as before plus the cycler it could not reach.
            if panel.profiles.is_empty() {
                content_description = Some(
                    "No profiles defined. Add a [[profiles]] entry to nexterm.toml.".to_string(),
                );
            } else {
                for desc in
                    crate::renderer::overlay::widgets::settings_profiles::profiles_widget_descs(
                        panel,
                    )
                {
                    let id = settings_widget_id(desc.id);
                    nodes.push((id, widget_node(&desc)));
                    content_children.push(id);
                }
                content_description = Some(format!("Profiles ({} entries).", panel.profiles.len()));
            }
        }
        SettingsCategory::Ssh => {
            // UI/UX v3 P1c: the windowed host list, the five fields of the
            // selected host and the Add/Delete buttons are widget nodes now,
            // replacing the hand-written machinery of Phase 5-11-8 (the 800M
            // host-item range and fixed ids 40..=46, both retired). The
            // delete-confirmation dialog (NodeId 47-49) stays hand-written:
            // it is a modal over the panel, not a settings row.
            if panel.ssh_hosts.is_empty() {
                content_description = Some(
                    "No SSH hosts are registered. \
                     Press the Add new host button (Tab to the end of the list)."
                        .to_string(),
                );
            } else {
                let sel = panel.selected_host_index.min(panel.ssh_hosts.len() - 1);
                content_description = Some(format!(
                    "Editing host {} of {}. Use Up/Down to move between fields, Enter to save.",
                    sel + 1,
                    panel.ssh_hosts.len(),
                ));
            }
            for desc in crate::renderer::overlay::widgets::settings_ssh::ssh_widget_descs(panel) {
                let id = settings_widget_id(desc.id);
                nodes.push((id, widget_node(&desc)));
                content_children.push(id);
            }
        }
        SettingsCategory::Keybindings => {
            // Phase 5-11-9 Sub-phase E: expose the keybinding list as ListBox +
            // ListBoxOption (NodeId 900M+idx), expose the selected binding's key /
            // action as TextInput / ComboBox (NodeId 50 / 51), and expose
            // Add / Delete buttons (NodeId 52 / 53) plus the delete confirmation
            // dialog (NodeId 54..56). Mirrors the SSH Sub-phase D layout.
            if panel.keybindings.is_empty() {
                content_description = Some(
                    "No keybindings are registered. \
                     Press the Add new keybinding button (Tab to the end of the list)."
                        .to_string(),
                );
            } else {
                // ===== Keybinding list =====
                let item_ids: Vec<NodeId> = (0..panel.keybindings.len())
                    .map(settings_key_binding_item_id)
                    .collect();
                for (idx, kb) in panel.keybindings.iter().enumerate() {
                    let mut item = Node::new(Role::ListBoxOption);
                    item.set_label(kb.label());
                    if idx == panel.selected_key_index {
                        item.set_selected(true);
                    }
                    nodes.push((settings_key_binding_item_id(idx), item));
                }
                for id in &item_ids {
                    content_children.push(*id);
                }

                // ===== Field editing for the selected binding =====
                let sel = panel.selected_key_index.min(panel.keybindings.len() - 1);
                let kb = &panel.keybindings[sel];

                // Key field (TextInput). Q1 = (c): both Click (enters Record mode)
                // and SetValue (direct overwrite) are accepted. While Text-mode
                // editing is in flight expose the live buffer; while Record-mode
                // is active expose a guidance description.
                let mut key_node = Node::new(Role::TextInput);
                key_node.set_label("Key combination (key)");
                let key_val = match &panel.key_editing {
                    Some(KeyEditMode::Text(s)) => s.display_string(),
                    _ => kb.key.clone(),
                };
                key_node.set_value(key_val.as_str());
                if panel.is_key_recording() {
                    key_node.set_description(
                        "Recording: press the key combination to bind, or Esc to cancel.",
                    );
                } else {
                    key_node.set_description(
                        "Click to start recording the next key press, or SetValue to overwrite the spelling directly (e.g. \"ctrl+shift+p\").",
                    );
                }
                nodes.push((SETTINGS_KEY_FIELD_KEY_ID, key_node));
                content_children.push(SETTINGS_KEY_FIELD_KEY_ID);

                // Action field (ComboBox cycling KEYBINDING_ACTIONS).
                let mut action_node = Node::new(Role::ComboBox);
                action_node.set_label("Action");
                action_node.set_value(kb.action.as_str());
                action_node.set_description(
                    "Use Left/Right to cycle the action, or SetValue to set it directly. Unknown values are rejected.",
                );
                nodes.push((SETTINGS_KEY_FIELD_ACTION_ID, action_node));
                content_children.push(SETTINGS_KEY_FIELD_ACTION_ID);

                content_description = Some(format!(
                    "Editing binding {} of {}. Use Up/Down to move between fields, Enter to save.",
                    sel + 1,
                    panel.keybindings.len(),
                ));
            }

            // ===== Add / Delete buttons (always exposed) =====
            let mut add_btn = Node::new(Role::Button);
            add_btn.set_label("Add new keybinding");
            add_btn.set_description(
                "Enter or Click appends a fresh keybinding and immediately starts recording its key.",
            );
            if panel.key_field_focus == 3 {
                add_btn.set_selected(true);
            }
            nodes.push((SETTINGS_KEY_ADD_BTN_ID, add_btn));
            content_children.push(SETTINGS_KEY_ADD_BTN_ID);

            let mut delete_btn = Node::new(Role::Button);
            if panel.keybindings.is_empty() {
                delete_btn.set_label("Delete selected keybinding (disabled)");
                delete_btn.set_description(
                    "No keybinding is available to delete. Add a new keybinding first.",
                );
            } else {
                delete_btn.set_label("Delete selected keybinding");
                delete_btn.set_description(
                    "Enter or Click opens the delete confirmation dialog. Esc or Cancel dismisses it.",
                );
                if panel.key_field_focus == 4 {
                    delete_btn.set_selected(true);
                }
            }
            nodes.push((SETTINGS_KEY_DELETE_BTN_ID, delete_btn));
            content_children.push(SETTINGS_KEY_DELETE_BTN_ID);
        }
        // Phase 2c follow-up: Blocks page is now interactive (rows 0..=2
        // toggle / cycle the corresponding `[blocks]` keys; row 3 is a
        // hint). The description-only fallback from Phase 2c-G is kept so
        // screen-readers without specific row support still announce the
        // overall state.
        SettingsCategory::Blocks => {
            // UI/UX v3 P1c: each row is now its own node. Previously the
            // whole category was a single prose description, so none of the
            // three controls could be operated by an assistive client.
            for desc in
                crate::renderer::overlay::widgets::settings_blocks::blocks_widget_descs(panel)
            {
                let id = settings_widget_id(desc.id);
                nodes.push((id, widget_node(&desc)));
                content_children.push(id);
            }
            content_description =
                Some("Direct edits to config.toml [blocks] also live-reload.".to_string());
        }
        SettingsCategory::Security => {
            // UI/UX v3 P1c: as with Blocks, the seven controls replace what
            // was a single prose description of the whole category.
            for desc in
                crate::renderer::overlay::widgets::settings_security::security_widget_descs(panel)
            {
                let id = settings_widget_id(desc.id);
                nodes.push((id, widget_node(&desc)));
                content_children.push(id);
            }
            // The one caveat that belongs to no single control.
            content_description = Some(
                "The plugin read policy has no prompt path yet, so prompt behaves as deny."
                    .to_string(),
            );
        }
    }

    let mut content = Node::new(Role::Group);
    content.set_label(panel.category.label());
    if let Some(desc) = content_description {
        content.set_description(desc);
    } else if content_children.is_empty() {
        content.set_description("Details for this category are not implemented yet.");
    }
    content.set_children(content_children);
    nodes.push((SETTINGS_CONTENT_ID, content));

    // ===== Focus selection =====
    let focus = if let Some(id) = widget_focus_id(panel) {
        id
    } else if matches!(panel.category, SettingsCategory::Ssh)
        && panel.ssh_delete_dialog_open
        && !panel.ssh_hosts.is_empty()
    {
        // Phase 5-11-8 Step 8-3 (Sub-phase D): while the delete confirmation dialog is
        // open, move focus to the active button (Confirm/Cancel) inside the dialog.
        if panel.ssh_delete_dialog_confirm_focused {
            SETTINGS_SSH_DELETE_CONFIRM_BTN_ID
        } else {
            SETTINGS_SSH_DELETE_CANCEL_BTN_ID
        }
    } else if matches!(panel.category, SettingsCategory::Keybindings)
        && panel.key_delete_dialog_open
        && !panel.keybindings.is_empty()
    {
        // Phase 5-11-9 Sub-phase E: while the delete confirmation dialog is open,
        // move focus to the active button (Confirm/Cancel) inside the dialog.
        if panel.key_delete_dialog_confirm_focused {
            SETTINGS_KEY_DELETE_CONFIRM_BTN_ID
        } else {
            SETTINGS_KEY_DELETE_CANCEL_BTN_ID
        }
    } else if matches!(panel.category, SettingsCategory::Keybindings) && panel.key_field_focus == 3
    {
        // Add button is active even when the keybinding list is empty.
        SETTINGS_KEY_ADD_BTN_ID
    } else if matches!(panel.category, SettingsCategory::Keybindings)
        && panel.key_field_focus == 4
        && !panel.keybindings.is_empty()
    {
        // Delete button is focusable only when the list is non-empty.
        SETTINGS_KEY_DELETE_BTN_ID
    } else if matches!(panel.category, SettingsCategory::Keybindings)
        && !panel.keybindings.is_empty()
    {
        // Phase 5-11-9 Sub-phase E: focus selection for the Keybindings category.
        // key_field_focus = 0 -> selected item of the binding list
        // key_field_focus = 1 -> Key field
        // key_field_focus = 2 -> Action field
        match panel.key_field_focus {
            1 => SETTINGS_KEY_FIELD_KEY_ID,
            2 => SETTINGS_KEY_FIELD_ACTION_ID,
            _ => settings_key_binding_item_id(
                panel.selected_key_index.min(panel.keybindings.len() - 1),
            ),
        }
    } else {
        settings_tab_id_at(current_idx)
    };

    // ===== Phase 5-11-8 Step 8-3 (Sub-phase D): build the delete confirmation dialog nodes =====
    // `SETTINGS_SSH_DELETE_DIALOG_ID` was already pushed into `panel_children`. Here we
    // build the AlertDialog + Confirm/Cancel buttons. For empty lists there is nothing to
    // delete, so the top of `build_settings_panel_nodes` deliberately does not add the
    // dialog id to `panel_children` (treated as dialog_open=false).
    if panel.ssh_delete_dialog_open
        && matches!(panel.category, SettingsCategory::Ssh)
        && !panel.ssh_hosts.is_empty()
    {
        let sel = panel.selected_host_index.min(panel.ssh_hosts.len() - 1);
        let target = &panel.ssh_hosts[sel];
        let target_name = if target.name.is_empty() {
            target.host.clone()
        } else {
            target.name.clone()
        };

        let mut alert = Node::new(Role::AlertDialog);
        alert.set_label("Delete this host?");
        alert.set_description(format!(
            "Delete \"{}\"? This action cannot be undone.",
            target_name
        ));
        alert.set_modal();
        alert.set_children(vec![
            SETTINGS_SSH_DELETE_CANCEL_BTN_ID,
            SETTINGS_SSH_DELETE_CONFIRM_BTN_ID,
        ]);
        nodes.push((SETTINGS_SSH_DELETE_DIALOG_ID, alert));

        let mut cancel_btn = Node::new(Role::Button);
        cancel_btn.set_label("Cancel");
        cancel_btn.set_description("Esc / Left / Right / Tab to switch focus; Enter to confirm.");
        if !panel.ssh_delete_dialog_confirm_focused {
            cancel_btn.set_selected(true);
        }
        nodes.push((SETTINGS_SSH_DELETE_CANCEL_BTN_ID, cancel_btn));

        let mut confirm_btn = Node::new(Role::Button);
        confirm_btn.set_label("Delete");
        confirm_btn.set_description("Permanently deletes the selected host.");
        if panel.ssh_delete_dialog_confirm_focused {
            confirm_btn.set_selected(true);
        }
        nodes.push((SETTINGS_SSH_DELETE_CONFIRM_BTN_ID, confirm_btn));
    }

    // ===== Phase 5-11-9 Sub-phase E: build the Keybindings delete confirmation dialog =====
    // Mirrors the SSH dialog block: `SETTINGS_KEY_DELETE_DIALOG_ID` is already pushed
    // into `panel_children` (see the top of this function); here we build the AlertDialog
    // body and its Cancel / Confirm children. Skipped when the list is empty (treated
    // as dialog_open=false, since there is nothing to delete).
    if panel.key_delete_dialog_open
        && matches!(panel.category, SettingsCategory::Keybindings)
        && !panel.keybindings.is_empty()
    {
        let sel = panel.selected_key_index.min(panel.keybindings.len() - 1);
        let target = &panel.keybindings[sel];
        let target_label = target.label();

        let mut alert = Node::new(Role::AlertDialog);
        alert.set_label("Delete this keybinding?");
        alert.set_description(format!(
            "Delete \"{}\"? This action cannot be undone.",
            target_label
        ));
        alert.set_modal();
        alert.set_children(vec![
            SETTINGS_KEY_DELETE_CANCEL_BTN_ID,
            SETTINGS_KEY_DELETE_CONFIRM_BTN_ID,
        ]);
        nodes.push((SETTINGS_KEY_DELETE_DIALOG_ID, alert));

        let mut cancel_btn = Node::new(Role::Button);
        cancel_btn.set_label("Cancel");
        cancel_btn.set_description("Esc / Left / Right / Tab to switch focus; Enter to confirm.");
        if !panel.key_delete_dialog_confirm_focused {
            cancel_btn.set_selected(true);
        }
        nodes.push((SETTINGS_KEY_DELETE_CANCEL_BTN_ID, cancel_btn));

        let mut confirm_btn = Node::new(Role::Button);
        confirm_btn.set_label("Delete");
        confirm_btn.set_description("Permanently deletes the selected keybinding.");
        if panel.key_delete_dialog_confirm_focused {
            confirm_btn.set_selected(true);
        }
        nodes.push((SETTINGS_KEY_DELETE_CONFIRM_BTN_ID, confirm_btn));
    }

    (nodes, focus)
}

/// Build the update notification banner node (Step 2-2-g).
fn build_update_banner_node(version: &str) -> (NodeId, Node) {
    let mut alert = Node::new(Role::Alert);
    alert.set_label(format!("A new version is available: {}", version));
    (UPDATE_BANNER_ID, alert)
}

/// Build the SR alert region nodes (Sprint 5-11-5).
///
/// ## Tree structure
///
/// ```text
/// Group "Notifications" (id=ALERT_REGION_ID, live=Assertive)
///   ├─ Alert (id=alert_node_id(seq)) "Bell" / "Notification: <title>"
///   │    - value: "<body>" (empty for Bell, body text for Notification)
///   ├─ Alert ...
/// ```
///
/// **Live::Assertive** is set on the region container. The accesskit contract is that
/// SR announces immediately when child nodes are added (this is the standard usage).
///
/// **Empty queue**: both `(nodes, ids)` are empty. The caller must not include
/// ALERT_REGION_ID as a child of ROOT (an empty container would confuse SR).
///
/// Return value:
/// - `nodes`: ALERT_REGION itself + each Alert node pair (empty Vec if queue is empty)
/// - `region_child_ids`: each Alert NodeId to attach to ALERT_REGION's children
fn build_alert_region_nodes(
    alerts: &std::collections::VecDeque<AlertEntry>,
) -> Vec<(NodeId, Node)> {
    if alerts.is_empty() {
        return Vec::new();
    }
    let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(1 + alerts.len());

    // ===== Region container =====
    let mut region = Node::new(Role::Group);
    region.set_label("Notifications");
    // Live::Assertive: SR announces as soon as a new Alert child is added.
    region.set_live(Live::Assertive);
    let child_ids: Vec<NodeId> = alerts.iter().map(|a| alert_node_id(a.seq)).collect();
    region.set_children(child_ids);
    nodes.push((ALERT_REGION_ID, region));

    // ===== Each Alert node =====
    for alert in alerts {
        let mut node = Node::new(Role::Alert);
        // Label: kind + title
        let label = match alert.kind {
            AlertKind::Bell => alert.title.clone(),
            AlertKind::Notification => format!("Notification: {}", alert.title),
        };
        node.set_label(label);
        // Body (if non-empty): SR reads it as the supplemental description.
        if !alert.body.is_empty() {
            node.set_description(alert.body.clone());
        }
        nodes.push((alert_node_id(alert.seq), node));
    }

    nodes
}

// ===== Step 2-5: state hash for live updates =====

/// Hash every field of `ClientState` that `build_tree_from_state` reads.
///
/// **Design policy**:
/// - Reflect **every field** referenced inside `build_tree_from_state` (under-
///   counting leaves SR stuck on stale info; over-counting causes excess updates).
/// - Do not call the `filtered()` family of methods (each call allocates + sorts).
///   Instead, hash the **inputs**: `query` / `selected` / `is_open`. The actual
///   contents of `actions` / `hosts` / `macros` rarely change at runtime, so
///   hashing the inputs is enough to detect changes.
/// - Iterate `panes` in a deterministic order that respects the tab order
///   (raw `HashMap` order would cause the hash to flap each call).
///
/// **Cost**: O(panes + overlay items). Designed to be used together with 100 ms throttling.
pub fn compute_tree_state_hash(state: &ClientState) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    /// Internal helper that hashes the "structure-affecting" fields of a single pane.
    ///
    /// Added in Sprint 5-11-4: `cursor_col` / `cursor_row` / `scrollback.len()` /
    /// `scroll_offset` directly affect the AccessKit tree structure (TextSelection
    /// position / scrollback window slide range), so when any of them changes the
    /// whole tree must be rebuilt.
    fn hash_pane(p: &crate::state::PaneState, h: &mut DefaultHasher) {
        p.title.hash(h);
        p.cwd.hash(h);
        // Sprint 5-11-4: cursor position / scrollback structure
        p.grid.cursor_col.hash(h);
        p.grid.cursor_row.hash(h);
        p.scrollback.len().hash(h);
        p.scroll_offset.hash(h);
    }

    let mut h = DefaultHasher::new();

    // === Base (tabs and panes) ===
    state.tab_order.hash(&mut h);
    state.focused_pane_id.hash(&mut h);

    // Iterate panes in tab order to avoid HashMap's nondeterministic order.
    // If `tab_order` is empty we fall back to `panes.keys()` like `build_base_nodes`,
    // but we **sort** the keys first to keep the hash stable.
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
    // The bodies of actions / hosts / macros rarely change at runtime, so tracking
    // `query` / `selected` is enough (indirectly tracks the contents of `filtered()`).
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
        // SettingsCategory does not implement Hash; substitute its label() string.
        p.category.label().hash(&mut h);
        // Hash every field that `build_settings_panel_nodes` reads for the current
        // category (the field set differs per category, so reflect them all).
        p.font_family.hash(&mut h);
        p.font_family_editing.hash(&mut h);
        // f32 does not implement Hash; convert to u32 via to_bits() and hash that.
        p.font_size.to_bits().hash(&mut h);
        p.opacity.to_bits().hash(&mut h);
        p.scheme_index.hash(&mut h);
        p.language_index.hash(&mut h);
        p.auto_check_update.hash(&mut h);
        // Phase 5-11-6 #6: 4 new Window category fields + field focus.
        // window_field_focus needs a tree update even when only the focus changes.
        p.window_field_focus.hash(&mut h);
        // CursorStyle / PresentModeConfig do not implement Hash; use their toml_key strings.
        p.cursor_style_toml_key().hash(&mut h);
        p.present_mode_toml_key().hash(&mut h);
        p.padding_x.hash(&mut h);
        p.padding_y.hash(&mut h);
        // Phase 5-11-7: for the Profiles category, reflect selected_profile + the
        // number of profiles + each ProfileEntry's name / icon.
        p.selected_profile.hash(&mut h);
        // UI/UX v3 P1c: the Profiles cycler exposes the active profile's
        // name as its value, so the tree must rebuild when it changes.
        p.active_profile_index.hash(&mut h);
        p.profiles.len().hash(&mut h);
        for prof in &p.profiles {
            prof.name.hash(&mut h);
            prof.icon.hash(&mut h);
        }
        // Phase 5-11-8 Step 8-1 / 8-2: for the Ssh category, reflect
        // selected_host_index + the number of ssh_hosts + each SshHostEntry's
        // label-affecting fields + ssh_field_focus.
        p.selected_host_index.hash(&mut h);
        p.ssh_field_focus.hash(&mut h);
        p.ssh_hosts.len().hash(&mut h);
        for host in &p.ssh_hosts {
            host.name.hash(&mut h);
            host.host.hash(&mut h);
            host.port.hash(&mut h);
            host.username.hash(&mut h);
            host.auth_type.hash(&mut h);
        }
        // Phase 5-11-8 Step 8-3 (Sub-phase A): to live-reflect the in-progress GUI
        // editing buffer in the SR tree, hash buffer / cursor / preedit while editing.
        if let Some(state) = &p.ssh_field_editing {
            state.buffer.hash(&mut h);
            state.cursor.hash(&mut h);
            state.preedit.hash(&mut h);
        } else {
            // Editing mode OFF -> hash 0 (so ON/OFF transitions are also detected).
            0u8.hash(&mut h);
        }
        // Phase 5-11-8 Step 8-3 (Sub-phase D): propagate open/close of the delete
        // confirmation dialog and the button focus change. Add/Delete button focus
        // changes are tracked via the existing `ssh_field_focus`; ssh_hosts
        // additions/removals are already covered by `ssh_hosts.len()` and each
        // per-host field hash.
        p.ssh_delete_dialog_open.hash(&mut h);
        p.ssh_delete_dialog_confirm_focused.hash(&mut h);
        // Phase 5-11-9 Sub-phase E: Keybindings category fields.
        // Reflect everything `build_settings_panel_nodes` reads:
        //   - keybindings list (key / action per entry)
        //   - selected_key_index / key_field_focus
        //   - delete dialog open / confirm focus
        //   - key_editing mode (Record / Text + Text buffer) for live updates
        p.selected_key_index.hash(&mut h);
        p.key_field_focus.hash(&mut h);
        p.keybindings.len().hash(&mut h);
        for kb in &p.keybindings {
            kb.key.hash(&mut h);
            kb.action.hash(&mut h);
        }
        p.key_delete_dialog_open.hash(&mut h);
        p.key_delete_dialog_confirm_focused.hash(&mut h);
        match &p.key_editing {
            None => 0u8.hash(&mut h),
            Some(crate::settings_panel::KeyEditMode::Record) => 1u8.hash(&mut h),
            Some(crate::settings_panel::KeyEditMode::Text(s)) => {
                2u8.hash(&mut h);
                s.buffer.hash(&mut h);
                s.cursor.hash(&mut h);
                s.preedit.hash(&mut h);
            }
        }
    }

    // === Quick Select (Step 2-2-h) ===
    // Required because typed_label changes which item is selected.
    // Reflect matches.len() + each label / text so changes to the match set (on enter()) are detected too.
    state.quick_select.is_active.hash(&mut h);
    if state.quick_select.is_active {
        state.quick_select.typed_label.hash(&mut h);
        state.quick_select.matches.len().hash(&mut h);
        for m in &state.quick_select.matches {
            m.label.hash(&mut h);
            m.text.hash(&mut h);
        }
    }

    // === update_banner (non-modal) ===
    state.update_banner.hash(&mut h);

    // === offline_banner (non-modal, Sprint 5-14 / v1.7.8 — P2-1) ===
    // We only care whether the banner is visible (the elapsed-seconds count
    // updates every frame and would otherwise force a tree rebuild every
    // throttle tick — accessibility consumers do not need that granularity).
    state.offline_banner_since.is_some().hash(&mut h);

    // === SR alerts (Sprint 5-11-5) ===
    // Reflect length + each seq + kind. `kind` becomes hashable via `as u8`.
    // body / title are immutable once an entry is queued, so tracking `seq` is enough
    // (title/body for the same seq are never rewritten later).
    state.alerts.len().hash(&mut h);
    for entry in &state.alerts {
        entry.seq.hash(&mut h);
        (entry.kind as u8).hash(&mut h);
    }

    h.finish()
}

/// Sprint 5-11-2 Step 2-4 extension: pure function handling AccessKit actions on the settings panel.
///
/// Called from `EventHandler::handle_accesskit_action`. Extracted as a standalone
/// function so it can be unit-tested without constructing an `EventHandler`.
///
/// # Returns
///
/// `true` when the caller should request a redraw (handler caused a state change).
/// `false` when the target NodeId is not in the settings panel domain, or no matching
/// action handler exists.
///
/// # Design notes
///
/// - `Focus` is used as a state-change trigger via the SR path only for
///   "Tab / Pane / CategoryTab" (virtual-cursor traversal = control transition).
///   For CheckBox and TextInput, Focus has no side effects beyond rendering state.
/// - Widget-layer controls route every action through their tab's
///   `apply_*_action`, which reuses the same setters the mouse and keyboard
///   paths call (rounding and clamping included).
/// - ThemeScheme / Language treat Click and Increment equivalently (ComboBox "next").
pub fn dispatch_settings_action(
    panel: &mut SettingsPanel,
    action: accesskit::Action,
    kind: &NodeIdKind,
    data: Option<accesskit::ActionData>,
) -> bool {
    use crate::settings_panel::SettingsCategory;
    use accesskit::{Action, ActionData};

    match (action, kind) {
        // ===== Category tabs =====
        (Action::Focus | Action::Click, NodeIdKind::SettingsTab { idx }) => {
            if let Some(cat) = SettingsCategory::ALL.get(*idx) {
                panel.category = cat.clone();
                panel.font_family_editing = false;
                true
            } else {
                false
            }
        }

        // ===== Widget-layer nodes (UI/UX v3 P1b) =====
        // Routed back to the same state transition the mouse and keyboard
        // paths use, so a screen reader and a click never disagree.
        // Focus is virtual-cursor traversal: it moves the panel's focus but
        // changes no value, matching the retired per-field arms.
        (Action::Focus, NodeIdKind::SettingsWidget { category, index }) => {
            use crate::renderer::overlay::widgets::settings_font::{FONT_CATEGORY, FONT_ROW_COUNT};
            use crate::renderer::overlay::widgets::settings_profiles::{
                PROFILES_CATEGORY, row as profiles_row,
            };
            use crate::renderer::overlay::widgets::settings_security::{
                SECURITY_CATEGORY, SECURITY_ROW_COUNT,
            };
            use crate::renderer::overlay::widgets::settings_ssh::{SSH_CATEGORY, row as ssh_row};
            use crate::renderer::overlay::widgets::settings_startup::{
                STARTUP_CATEGORY, STARTUP_ROW_COUNT,
            };
            use crate::renderer::overlay::widgets::settings_theme::THEME_CATEGORY;
            use crate::renderer::overlay::widgets::settings_window::{
                WINDOW_CATEGORY, WINDOW_ROW_COUNT,
            };
            match *category {
                THEME_CATEGORY => {
                    panel.theme_field_focus = *index;
                    true
                }
                WINDOW_CATEGORY if (*index as usize) < WINDOW_ROW_COUNT => {
                    panel.window_field_focus = *index;
                    true
                }
                FONT_CATEGORY if (*index as usize) < FONT_ROW_COUNT => {
                    panel.font_field_focus = *index;
                    true
                }
                STARTUP_CATEGORY if (*index as usize) < STARTUP_ROW_COUNT => {
                    panel.startup_field_focus = *index;
                    true
                }
                SECURITY_CATEGORY if (*index as usize) < SECURITY_ROW_COUNT => {
                    panel.security_field_focus = *index;
                    true
                }
                // Profiles has no focus counter: focusing an entry moves the
                // list selection itself, exactly as the retired
                // `SettingsProfileItem` arm did. Focusing the cycler changes
                // nothing, so it reports unhandled.
                PROFILES_CATEGORY
                    if *index >= profiles_row::LIST_BASE
                        && ((*index - profiles_row::LIST_BASE) as usize) < panel.profiles.len() =>
                {
                    panel.selected_profile = (*index - profiles_row::LIST_BASE) as usize;
                    true
                }
                // Ssh: focusing a list entry selects it and hands the counter
                // back to the list, matching the retired host-item arm.
                SSH_CATEGORY
                    if *index >= ssh_row::LIST_BASE
                        && ((*index - ssh_row::LIST_BASE) as usize) < panel.ssh_hosts.len() =>
                {
                    panel.selected_host_index = (*index - ssh_row::LIST_BASE) as usize;
                    panel.ssh_field_focus = 0;
                    true
                }
                // Ssh fields and buttons: the widget indices mirror
                // `ssh_field_focus` exactly. The Delete button accepts focus
                // even while disabled so reader navigation stays stable,
                // matching the retired button arm.
                SSH_CATEGORY if (1..=7).contains(index) => {
                    panel.ssh_field_focus = *index as u8;
                    true
                }
                // Blocks has no focus counter.
                _ => false,
            }
        }
        (
            Action::Click | Action::Increment | Action::Decrement | Action::SetValue,
            NodeIdKind::SettingsWidget { category, index },
        ) => {
            use crate::renderer::overlay::widgets::action::WidgetAction;
            use crate::renderer::overlay::widgets::settings_blocks::{
                BLOCKS_CATEGORY, apply_blocks_action,
            };
            use crate::renderer::overlay::widgets::settings_font::{
                FONT_CATEGORY, apply_font_action,
            };
            use crate::renderer::overlay::widgets::settings_profiles::{
                PROFILES_CATEGORY, apply_profiles_action,
            };
            use crate::renderer::overlay::widgets::settings_security::{
                SECURITY_CATEGORY, apply_security_action,
            };
            use crate::renderer::overlay::widgets::settings_ssh::{SSH_CATEGORY, apply_ssh_action};
            use crate::renderer::overlay::widgets::settings_startup::{
                STARTUP_CATEGORY, apply_startup_action,
            };
            use crate::renderer::overlay::widgets::settings_theme::{
                THEME_CATEGORY, apply_theme_action,
            };
            use crate::renderer::overlay::widgets::settings_window::{
                WINDOW_CATEGORY, apply_window_action,
            };
            let widget_action = match (action, data) {
                (Action::Increment, _) => WidgetAction::Next,
                (Action::Decrement, _) => WidgetAction::Prev,
                (Action::SetValue, Some(ActionData::NumericValue(v))) => WidgetAction::SetValue(v),
                (Action::SetValue, Some(ActionData::Value(text))) => {
                    WidgetAction::SetText(text.into_string())
                }
                // SetValue without a usable payload is malformed, not a click.
                (Action::SetValue, _) => return false,
                _ => WidgetAction::Activate,
            };
            match *category {
                THEME_CATEGORY => apply_theme_action(panel, *index, widget_action),
                WINDOW_CATEGORY => apply_window_action(panel, *index, widget_action),
                FONT_CATEGORY => apply_font_action(panel, *index, widget_action),
                STARTUP_CATEGORY => apply_startup_action(panel, *index, widget_action),
                BLOCKS_CATEGORY => apply_blocks_action(panel, *index, widget_action),
                SECURITY_CATEGORY => apply_security_action(panel, *index, widget_action),
                PROFILES_CATEGORY => apply_profiles_action(panel, *index, widget_action),
                SSH_CATEGORY => apply_ssh_action(panel, *index, widget_action),
                // Keybindings keeps its own nodes for now.
                _ => false,
            }
        }

        // ===== Phase 5-11-8 Step 8-3 (Sub-phase D): delete confirmation dialog =====
        // We do not accept Actions on the dialog body itself (modal management is left to the SR).
        // Only Cancel / Confirm button Actions are handled.
        (Action::Focus, NodeIdKind::SettingsSshDeleteCancelBtn) => {
            panel.ssh_delete_dialog_confirm_focused = false;
            true
        }
        (Action::Click, NodeIdKind::SettingsSshDeleteCancelBtn) => {
            panel.cancel_ssh_delete_dialog();
            true
        }
        (Action::Focus, NodeIdKind::SettingsSshDeleteConfirmBtn) => {
            panel.ssh_delete_dialog_confirm_focused = true;
            true
        }
        (Action::Click, NodeIdKind::SettingsSshDeleteConfirmBtn) => {
            panel.confirm_ssh_delete_dialog();
            true
        }

        // ===== Phase 5-11-9 Sub-phase E: Keybinding list item (ListBoxOption) =====
        // Both Click and Focus update `selected_key_index` and reset focus to the list.
        (Action::Click | Action::Focus, NodeIdKind::SettingsKeyBindingItem { idx })
            if *idx < panel.keybindings.len() =>
        {
            panel.selected_key_index = *idx;
            panel.key_field_focus = 0;
            true
        }

        // ===== Phase 5-11-9 Sub-phase E: Key field (TextInput) =====
        // Q1 = (c): Click triggers Record mode AND SetValue writes the spelling directly.
        (Action::Focus, NodeIdKind::SettingsKeyFieldKey) => {
            panel.key_field_focus = 1;
            true
        }
        (Action::Click, NodeIdKind::SettingsKeyFieldKey) => {
            panel.key_field_focus = 1;
            panel.begin_key_record();
            true
        }
        (Action::SetValue, NodeIdKind::SettingsKeyFieldKey) => {
            if let Some(ActionData::Value(s)) = data {
                let updated = panel.set_keybinding_key_direct(s.into_string());
                panel.key_field_focus = 1;
                updated
            } else {
                false
            }
        }

        // ===== Phase 5-11-9 Sub-phase E: Action field (ComboBox) =====
        (Action::Focus, NodeIdKind::SettingsKeyFieldAction) => {
            panel.key_field_focus = 2;
            true
        }
        (Action::Click | Action::Increment, NodeIdKind::SettingsKeyFieldAction) => {
            panel.next_key_action();
            panel.key_field_focus = 2;
            true
        }
        (Action::Decrement, NodeIdKind::SettingsKeyFieldAction) => {
            panel.prev_key_action();
            panel.key_field_focus = 2;
            true
        }
        (Action::SetValue, NodeIdKind::SettingsKeyFieldAction) => {
            if let Some(ActionData::Value(s)) = data {
                let updated = panel.set_keybinding_action_direct(s.as_ref());
                panel.key_field_focus = 2;
                updated
            } else {
                false
            }
        }

        // ===== Phase 5-11-9 Sub-phase E: Add / Delete buttons =====
        (Action::Focus, NodeIdKind::SettingsKeyAddBtn) => {
            panel.key_field_focus = 3;
            true
        }
        (Action::Click, NodeIdKind::SettingsKeyAddBtn) => {
            panel.add_key_binding();
            true
        }
        (Action::Focus, NodeIdKind::SettingsKeyDeleteBtn) => {
            panel.key_field_focus = 4;
            true
        }
        (Action::Click, NodeIdKind::SettingsKeyDeleteBtn) => {
            panel.open_key_delete_dialog();
            true
        }

        // ===== Phase 5-11-9 Sub-phase E: delete confirmation dialog =====
        (Action::Focus, NodeIdKind::SettingsKeyDeleteCancelBtn) => {
            panel.key_delete_dialog_confirm_focused = false;
            true
        }
        (Action::Click, NodeIdKind::SettingsKeyDeleteCancelBtn) => {
            panel.cancel_key_delete_dialog();
            true
        }
        (Action::Focus, NodeIdKind::SettingsKeyDeleteConfirmBtn) => {
            panel.key_delete_dialog_confirm_focused = true;
            true
        }
        (Action::Click, NodeIdKind::SettingsKeyDeleteConfirmBtn) => {
            panel.confirm_key_delete_dialog();
            true
        }

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    // The pattern of assigning fields individually after `SettingsPanel::default()` in
    // tests is permitted to keep the SR dispatch spec readable (the struct has many
    // fields, so an inline struct literal becomes verbose).
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use crate::state::ClientState;

    /// NodeId offset safety: the Tab and Pane ID ranges must not collide.
    #[test]
    fn node_id_offsets_do_not_overlap() {
        let max_tab = tab_node_id(u32::MAX).0;
        let min_pane = pane_node_id(0).0;
        assert!(
            max_tab < min_pane,
            "Tab ID range [{}, {}] collides with Pane ID range [{}, ...]",
            NODE_ID_TAB_OFFSET,
            max_tab,
            min_pane
        );
        const _: () = assert!(NODE_ID_TAB_OFFSET > 99);
    }

    /// Overlay dynamic ID offsets must not collide with the Tab range.
    #[test]
    fn overlay_offsets_do_not_overlap_with_tabs() {
        // Each overlay ID offset must be below the Tab offset.
        const _: () = assert!(NODE_ID_PALETTE_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        const _: () = assert!(NODE_ID_HOST_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        const _: () = assert!(NODE_ID_MACRO_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        const _: () = assert!(NODE_ID_CONTEXT_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        const _: () = assert!(NODE_ID_QUICKSELECT_ITEM_OFFSET < NODE_ID_TAB_OFFSET);
        // The ID ranges of different overlays must not intersect (assumed safe up to 100k items).
        const ITEM_CAP: u64 = 100_000_000; // Spacing between offsets.
        const _: () = assert!(NODE_ID_HOST_ITEM_OFFSET - NODE_ID_PALETTE_ITEM_OFFSET >= ITEM_CAP);
        const _: () = assert!(NODE_ID_MACRO_ITEM_OFFSET - NODE_ID_HOST_ITEM_OFFSET >= ITEM_CAP);
        const _: () = assert!(NODE_ID_CONTEXT_ITEM_OFFSET - NODE_ID_MACRO_ITEM_OFFSET >= ITEM_CAP);
        const _: () =
            assert!(NODE_ID_QUICKSELECT_ITEM_OFFSET - NODE_ID_CONTEXT_ITEM_OFFSET >= ITEM_CAP);
    }

    /// Build a tree from an empty ClientState (initial state).
    #[test]
    fn build_tree_from_empty_state() {
        let state = ClientState::new(80, 24, 1000);
        let update = build_tree_from_state(&state);

        // ROOT / TAB_BAR / PANE_AREA + PaneInputBuffer (Phase 5-11-7) = 4 nodes
        assert_eq!(update.nodes.len(), 4);
        assert_eq!(update.focus, ROOT_ID);
        assert!(update.tree.is_some());
    }

    /// Tree for a single-pane configuration.
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

    /// Multi-pane configuration: tab order must follow `tab_order`.
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

    /// Label generation for a pane with a title.
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

    /// When the CommandPalette is open, the tree must include the dialog, search box, and candidate list.
    #[test]
    fn build_tree_with_open_palette() {
        let mut state = ClientState::new(80, 24, 1000);
        state.palette.is_open = true;
        state.palette.query = "edit".to_string();
        state.palette.selected = 0;

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&PALETTE_ID.0), "PALETTE_ID is missing");
        assert!(
            ids.contains(&PALETTE_SEARCH_ID.0),
            "PALETTE_SEARCH_ID is missing"
        );
        assert!(
            ids.contains(&PALETTE_LIST_ID.0),
            "PALETTE_LIST_ID is missing"
        );

        // Focus lands on either the search input (when there are no candidates) or the first candidate.
        // The default state has candidates, but here we only check that one of the two is present.
        assert!(update.focus == PALETTE_SEARCH_ID || update.focus == palette_item_id(0));
    }

    /// When the CloseWindowDialog is shown, the tree must include an AlertDialog and two buttons.
    #[test]
    fn build_tree_with_close_dialog() {
        let mut state = ClientState::new(80, 24, 1000);
        state.close_window_dialog = Some(CloseWindowDialog {
            server_window_id: 1,
            message: "A process is still running. Close anyway?".to_string(),
            kill_label: "Force kill".to_string(),
            cancel_label: "Cancel".to_string(),
            selected_button: 1, // Cancel
        });

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&CLOSE_DIALOG_ID.0));
        assert!(ids.contains(&CLOSE_DIALOG_KILL_BTN.0));
        assert!(ids.contains(&CLOSE_DIALOG_CANCEL_BTN.0));

        // Focus lands on the Cancel button.
        assert_eq!(update.focus, CLOSE_DIALOG_CANCEL_BTN);
    }

    /// When the ContextMenu is shown, the tree must include a Menu and MenuItem nodes.
    #[test]
    fn build_tree_with_context_menu() {
        let mut state = ClientState::new(80, 24, 1000);
        state.context_menu = Some(ContextMenu::new_default(100.0, 100.0, &[]));

        let update = build_tree_from_state(&state);

        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&CONTEXT_MENU_ID.0));
        // The default menu has multiple items. The NodeId range for context menu items is
        // [NODE_ID_CONTEXT_ITEM_OFFSET, NODE_ID_TAB_OFFSET) (the next offset is for tabs).
        let item_count = ids
            .iter()
            .filter(|&&id| (NODE_ID_CONTEXT_ITEM_OFFSET..NODE_ID_TAB_OFFSET).contains(&id))
            .count();
        assert!(item_count > 0, "context menu items are missing");
    }

    /// Priority: CloseWindowDialog takes precedence over other overlays.
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
        // The palette is not added because its priority is lower.
        assert!(
            !ids.contains(&PALETTE_ID.0),
            "Palette should not be present while CloseWindowDialog is shown"
        );
    }

    /// When Quick Select is active, the tree must include a Dialog, ListBox, and match items.
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
            "QUICK_SELECT_ID is missing"
        );
        assert!(
            ids.contains(&QUICK_SELECT_LIST_ID.0),
            "QUICK_SELECT_LIST_ID is missing"
        );
        assert!(
            ids.contains(&quickselect_item_id(0).0),
            "match item 0 is missing"
        );
        assert!(
            ids.contains(&quickselect_item_id(1).0),
            "match item 1 is missing"
        );
        // When `typed_label` is empty, focus lands on the first match.
        assert_eq!(update.focus, quickselect_item_id(0));
    }

    /// When `typed_label` matches as a prefix, focus moves to that item.
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

    /// When Quick Select has no matches, focus falls back to the ListBox itself.
    #[test]
    fn quick_select_focus_falls_back_to_list_when_empty() {
        let mut state = ClientState::new(80, 24, 1000);
        state.quick_select.is_active = true;
        // `matches` stays empty.

        let update = build_tree_from_state(&state);
        assert_eq!(update.focus, QUICK_SELECT_LIST_ID);
    }

    /// CloseWindowDialog takes precedence over Quick Select (highest-priority modal).
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
            "Quick Select must not appear while CloseDialog is shown"
        );
    }

    /// Quick Select takes precedence over the ContextMenu / Palette.
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
            "ContextMenu must not appear while Quick Select is active"
        );
        assert!(
            !ids.contains(&PALETTE_ID.0),
            "Palette must not appear while Quick Select is active"
        );
    }

    /// The update banner is non-modal and coexists with other overlays.
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

    // ===== Step 2-5: live-update state hash tests =====

    /// Same state must produce the same hash (deterministic).
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
        assert_eq!(h1, h2, "hash differs for identical state");
    }

    /// Title change must alter the hash.
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

        assert_ne!(h1, h2, "hash did not change after title change");
    }

    /// Focus change must alter the hash.
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

        assert_ne!(h1, h2, "hash did not change after focus change");
    }

    /// Opening or closing the palette must alter the hash.
    #[test]
    fn tree_state_hash_detects_palette_open() {
        let state_closed = ClientState::new(80, 24, 1000);
        let h_closed = compute_tree_state_hash(&state_closed);

        let mut state_open = ClientState::new(80, 24, 1000);
        state_open.palette.is_open = true;
        let h_open = compute_tree_state_hash(&state_open);

        assert_ne!(
            h_closed, h_open,
            "hash did not change after toggling the palette"
        );
    }

    /// Changing the palette query must alter the hash.
    #[test]
    fn tree_state_hash_detects_palette_query_change() {
        let mut state = ClientState::new(80, 24, 1000);
        state.palette.is_open = true;
        state.palette.query = "abc".to_string();
        let h1 = compute_tree_state_hash(&state);

        state.palette.query = "xyz".to_string();
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "hash did not change after palette query change");
    }

    /// Changing `selected_button` on CloseWindowDialog must alter the hash.
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

        assert_ne!(
            h1, h2,
            "hash did not change after CloseWindowDialog button change"
        );
    }

    /// Opening or closing Quick Select must alter the hash.
    #[test]
    fn tree_state_hash_detects_quick_select_open() {
        let state_closed = ClientState::new(80, 24, 1000);
        let h_closed = compute_tree_state_hash(&state_closed);

        let mut state_open = ClientState::new(80, 24, 1000);
        state_open.quick_select.is_active = true;
        let h_open = compute_tree_state_hash(&state_open);

        assert_ne!(
            h_closed, h_open,
            "hash did not change after toggling Quick Select"
        );
    }

    /// Changing the Quick Select `typed_label` must alter the hash.
    #[test]
    fn tree_state_hash_detects_quick_select_typed_label_change() {
        let mut state = ClientState::new(80, 24, 1000);
        state.quick_select.is_active = true;
        state.quick_select.typed_label = "a".to_string();
        let h1 = compute_tree_state_hash(&state);

        state.quick_select.typed_label = "ab".to_string();
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "hash did not change after typed_label change");
    }

    /// Adding or removing the update banner must alter the hash.
    #[test]
    fn tree_state_hash_detects_update_banner() {
        let state_none = ClientState::new(80, 24, 1000);
        let h_none = compute_tree_state_hash(&state_none);

        let mut state_banner = ClientState::new(80, 24, 1000);
        state_banner.update_banner = Some("v1.6.0".to_string());
        let h_banner = compute_tree_state_hash(&state_banner);

        assert_ne!(
            h_none, h_banner,
            "hash did not change after adding the banner"
        );
    }

    // ===== Step 2-4: decode_node_id unit tests =====

    /// Fixed NodeIds must round-trip correctly.
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

    /// Quick Select match NodeId round-trip.
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

    /// Tab NodeId (`tab_node_id(pane_id)`) decode round-trip.
    #[test]
    fn decode_tab_node_id_roundtrip() {
        for &pane_id in &[0u32, 1, 42, 12345, u32::MAX] {
            assert_eq!(
                decode_node_id(tab_node_id(pane_id)),
                NodeIdKind::Tab { pane_id }
            );
        }
    }

    /// Pane NodeId (`pane_node_id(pane_id)`) decode round-trip.
    #[test]
    fn decode_pane_node_id_roundtrip() {
        for &pane_id in &[0u32, 1, 42, 12345, u32::MAX] {
            assert_eq!(
                decode_node_id(pane_node_id(pane_id)),
                NodeIdKind::Pane { pane_id }
            );
        }
    }

    /// Decode dynamic offset items (palette / host / macro / context).
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

    /// Unknown / reserved ranges must return `Unknown`.
    #[test]
    fn decode_unknown_node_ids() {
        assert_eq!(decode_node_id(NodeId(0)), NodeIdKind::Unknown);
        // 17 is SettingsTabList, 25 is SettingsContent,
        // 26 is AlertRegion (assigned in Sprint 5-11-5), 27 is PaneInputBuffer (Phase 5-11-7),
        // 30..=35 are settings fields (Step 2-2-e'), 36..=39 are Phase 5-11-6 #6 settings fields,
        // 40..=44 are Phase 5-11-8 Step 8-2 SSH host fields,
        // 45..=49 are Phase 5-11-8 Step 8-3 Sub-phase D Add/Delete + delete confirmation dialog.
        // 50..=56 are Phase 5-11-9 Sub-phase E Keybindings fields + Add/Delete + dialog.
        // 60..=99 are SettingsTab (Phase 2c-G moved the base from 18 to 60 to make
        // room for an 8th category without colliding with SETTINGS_CONTENT_ID = 25).
        // 57..=59 are the custom title bar window buttons.
        // 18..=24, 28..=29 are now unused / reserved for future use.
        assert_eq!(decode_node_id(NodeId(18)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(24)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(28)), NodeIdKind::Unknown);
        assert_eq!(decode_node_id(NodeId(29)), NodeIdKind::Unknown);
        // 700M..800M was reserved for dynamic SettingsField expansion and is
        // now `SettingsWidget` (UI/UX v3 P1b), so it no longer decodes to
        // Unknown — except for values carrying bits outside the packed
        // (category, index) pair, which no `WidgetId` can produce.
        assert_eq!(
            decode_node_id(NodeId(700_000_000)),
            NodeIdKind::SettingsWidget {
                category: 0,
                index: 0
            }
        );
        assert_eq!(decode_node_id(NodeId(799_999_999)), NodeIdKind::Unknown);
        // The gap between the Tab and Pane ranges (5.3e9..1e10) is also Unknown.
        assert_eq!(decode_node_id(NodeId(7_000_000_000)), NodeIdKind::Unknown);
        // The gap between the Pane range and the row range (1e10 + u32::MAX .. 2e10) is also Unknown.
        assert_eq!(decode_node_id(NodeId(15_000_000_000)), NodeIdKind::Unknown);
        // Beyond the row range (u32::MAX * MAX_ROWS_PER_PANE + 2e10 onward).
        let row_range_end =
            NODE_ID_PANE_ROW_OFFSET + (u32::MAX as u64) * MAX_ROWS_PER_PANE + MAX_ROWS_PER_PANE;
        assert_eq!(decode_node_id(NodeId(row_range_end)), NodeIdKind::Unknown);
    }

    // ===== Step 2-2-e': SettingsField expansion =====

    /// The SettingsPanel TabList and each category tab NodeId must round-trip correctly.
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
                "settings_tab_id_at({}) failed to round-trip",
                idx
            );
        }
    }

    /// Each settings field NodeId must round-trip correctly.
    #[test]
    fn decode_settings_field_node_ids() {
        assert_eq!(
            decode_node_id(settings_widget_id(theme_scheme_widget_id())),
            NodeIdKind::SettingsWidget {
                category: 2,
                index: 0
            }
        );
    }

    /// When the SettingsPanel is open, the tree must include Dialog + TabList + all category tabs + Content.
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
                "category tab {} is missing",
                idx
            );
        }
        // Fields for the Font category must be present.
        use crate::renderer::overlay::widgets::settings_font::{FONT_CATEGORY, row as font_row};
        assert!(ids.contains(&widget_id(FONT_CATEGORY, font_row::FAMILY).0));
        assert!(ids.contains(&widget_id(FONT_CATEGORY, font_row::SIZE).0));
    }

    /// While editing the Font, focus must move to the FontFamily input.
    #[test]
    fn settings_panel_focus_follows_font_family_editing() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Font;
        state.settings_panel.font_family_editing = true;

        let update = build_tree_from_state(&state);
        assert_eq!(update.focus, widget_id(font_category(), font_family_row()));
    }

    /// A category with no widget focus counter falls back to its tab.
    #[test]
    fn settings_panel_focus_falls_back_to_the_tab() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        // Blocks is mouse-only, so nothing inside it can hold focus.
        state.settings_panel.category = SettingsCategory::Blocks;

        let update = build_tree_from_state(&state);
        // Blocks is index 7 in SettingsCategory::ALL.
        assert_eq!(update.focus, settings_tab_id_at(7));
    }

    /// Focus follows each migrated tab's field counter (UI/UX v3 P1c).
    ///
    /// Regression guard for a real defect: the `Action::Focus` dispatch arm
    /// wrote these counters while the reported focus ignored them, so a
    /// screen reader's virtual cursor moved but the announced focus never
    /// did. Every migrated category with a counter must appear here.
    #[test]
    fn settings_panel_focus_follows_every_migrated_counter() {
        use crate::renderer::overlay::widgets::settings_font::FONT_CATEGORY;
        use crate::renderer::overlay::widgets::settings_security::SECURITY_CATEGORY;
        use crate::renderer::overlay::widgets::settings_startup::STARTUP_CATEGORY;
        use crate::renderer::overlay::widgets::settings_theme::THEME_CATEGORY;
        use crate::renderer::overlay::widgets::settings_window::WINDOW_CATEGORY;
        use crate::settings_panel::SettingsCategory;

        /// One migrated category: its enum variant, its widget-category
        /// index, and the setter for its focus counter.
        type FocusCase = (SettingsCategory, u8, fn(&mut SettingsPanel, u16));

        let cases: [FocusCase; 5] = [
            (SettingsCategory::Theme, THEME_CATEGORY, |p, i| {
                p.theme_field_focus = i
            }),
            (SettingsCategory::Window, WINDOW_CATEGORY, |p, i| {
                p.window_field_focus = i
            }),
            (SettingsCategory::Font, FONT_CATEGORY, |p, i| {
                p.font_field_focus = i
            }),
            (SettingsCategory::Startup, STARTUP_CATEGORY, |p, i| {
                p.startup_field_focus = i
            }),
            (SettingsCategory::Security, SECURITY_CATEGORY, |p, i| {
                p.security_field_focus = i
            }),
        ];

        for (category, widget_category, set_focus) in cases {
            let mut state = ClientState::new(80, 24, 1000);
            state.settings_panel.is_open = true;
            state.settings_panel.category = category.clone();
            set_focus(&mut state.settings_panel, 1);

            let update = build_tree_from_state(&state);
            assert_eq!(
                update.focus,
                widget_id(widget_category, 1),
                "focus did not follow the counter for {category:?}"
            );
            assert!(
                update.nodes.iter().any(|(id, _)| *id == update.focus),
                "the focused node must exist in the tree for {category:?}"
            );
        }
    }

    /// An out-of-range counter must not point at a node that is not in the
    /// tree — a dangling focus id breaks the reader outright.
    #[test]
    fn settings_panel_focus_ignores_an_out_of_range_counter() {
        use crate::renderer::overlay::widgets::settings_security::{
            SECURITY_CATEGORY, SECURITY_ROW_COUNT,
        };
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Security;
        state.settings_panel.security_field_focus = SECURITY_ROW_COUNT as u16;

        let update = build_tree_from_state(&state);
        assert_ne!(
            update.focus,
            widget_id(SECURITY_CATEGORY, SECURITY_ROW_COUNT as u16)
        );
        assert!(update.nodes.iter().any(|(id, _)| *id == update.focus));
    }

    /// Each category must include only the fields belonging to it.
    #[test]
    fn settings_panel_shows_only_current_category_fields() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;

        // Startup category
        state.settings_panel.category = SettingsCategory::Startup;
        let update = build_tree_from_state(&state);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        use crate::renderer::overlay::widgets::settings_startup::{
            STARTUP_CATEGORY, row as startup_row,
        };
        assert!(ids.contains(&widget_id(STARTUP_CATEGORY, startup_row::LANGUAGE).0));
        assert!(ids.contains(&widget_id(STARTUP_CATEGORY, startup_row::CHECK_UPDATES).0));
        assert!(
            !ids.contains(&widget_id(font_category(), font_family_row()).0),
            "Font field must not appear in the Startup category"
        );

        // Window category
        state.settings_panel.category = SettingsCategory::Window;
        let update = build_tree_from_state(&state);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.contains(&settings_widget_id(window_opacity_widget_id()).0));
        assert!(
            !ids.contains(&settings_widget_id(theme_scheme_widget_id()).0),
            "Theme field must not appear in the Window category"
        );
    }

    /// SSH / Keybindings / Profiles categories only have a Content Group; no detail fields.
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
            // The Content Group is present.
            assert!(ids.contains(&SETTINGS_CONTENT_ID.0));
            // Detail fields are not present.
            assert!(!ids.contains(&widget_id(font_category(), font_family_row()).0));
            assert!(!ids.contains(&settings_widget_id(theme_scheme_widget_id()).0));
            assert!(!ids.contains(&settings_widget_id(window_opacity_widget_id()).0));
        }
    }

    /// Category switching must alter the hash (because the selected tab changes).
    #[test]
    fn tree_state_hash_detects_settings_category_change() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Font;
        let h1 = compute_tree_state_hash(&state);

        state.settings_panel.category = SettingsCategory::Theme;
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "hash did not change after category switch");
    }

    /// Changing the font size must alter the hash.
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

        assert_ne!(h1, h2, "hash did not change after font_size change");
    }

    /// Toggling `auto_check_update` must alter the hash.
    #[test]
    fn tree_state_hash_detects_settings_auto_update_toggle() {
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.auto_check_update = false;
        let h1 = compute_tree_state_hash(&state);

        state.settings_panel.auto_check_update = true;
        let h2 = compute_tree_state_hash(&state);

        assert_ne!(h1, h2, "hash did not change after auto_check_update toggle");
    }

    // ============================================================
    // Sprint 5-11-2 Step 2-4 extension: unit tests for dispatch_settings_action
    // ============================================================

    use crate::settings_panel::{SettingsCategory, SettingsPanel};
    use accesskit::{Action, ActionData};

    /// Focus / Click on a SettingsTab switches the category and exits edit mode.
    #[test]
    fn dispatch_settings_tab_click_changes_category() {
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Font;
        panel.font_family_editing = true;

        // ALL idx=2 is Theme.
        let kind = NodeIdKind::SettingsTab { idx: 2 };
        let handled = dispatch_settings_action(&mut panel, Action::Click, &kind, None);

        assert!(handled, "SettingsTab Click should return handled=true");
        assert_eq!(panel.category, SettingsCategory::Theme);
        assert!(
            !panel.font_family_editing,
            "category switch should clear font_family_editing"
        );

        // Focus must behave the same way.
        let kind2 = NodeIdKind::SettingsTab { idx: 0 };
        let handled = dispatch_settings_action(&mut panel, Action::Focus, &kind2, None);
        assert!(handled);
        assert_eq!(panel.category, SettingsCategory::Startup);
    }

    /// Out-of-range SettingsTab idx must return handled=false (category unchanged).
    #[test]
    fn dispatch_settings_tab_out_of_range_returns_false() {
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Font;
        let original = panel.category.clone();

        let kind = NodeIdKind::SettingsTab { idx: 99 };
        let handled = dispatch_settings_action(&mut panel, Action::Click, &kind, None);

        assert!(!handled, "out-of-range idx must return handled=false");
        assert_eq!(panel.category, original, "category should not change");
    }

    /// The Font/Startup dispatch tests below drive the widget-derived nodes
    /// introduced in UI/UX v3 P1c; the per-field nodes they used to target
    /// were retired with the migration.
    ///
    /// Click on the family row enters edit mode.
    #[test]
    fn dispatch_settings_font_family_click_enters_editing() {
        use crate::renderer::overlay::widgets::settings_font::{FONT_CATEGORY, row};

        let mut panel = SettingsPanel::default();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsWidget {
                category: FONT_CATEGORY,
                index: row::FAMILY,
            },
            None,
        );
        assert!(handled);
        assert!(panel.font_family_editing);
    }

    /// A string SetValue applies to the family field and marks it dirty.
    #[test]
    fn dispatch_settings_font_family_set_value_updates_string() {
        use crate::renderer::overlay::widgets::settings_font::{FONT_CATEGORY, row};

        let mut panel = SettingsPanel::default();
        panel.dirty = false;
        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsWidget {
                category: FONT_CATEGORY,
                index: row::FAMILY,
            },
            Some(ActionData::Value("Cascadia Code".into())),
        );
        assert!(handled);
        assert_eq!(panel.font_family, "Cascadia Code");
        assert!(panel.dirty);
    }

    /// A numeric SetValue on a text field is refused rather than coerced.
    #[test]
    fn dispatch_settings_font_family_set_value_with_numeric_returns_false() {
        use crate::renderer::overlay::widgets::settings_font::{FONT_CATEGORY, row};

        let mut panel = SettingsPanel::default();
        let before = panel.font_family.clone();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsWidget {
                category: FONT_CATEGORY,
                index: row::FAMILY,
            },
            Some(ActionData::NumericValue(12.0)),
        );
        assert!(!handled);
        assert_eq!(panel.font_family, before);
    }

    /// SetValue on the size slider keeps its 0.5-unit rounding and 8..=32 clamp.
    #[test]
    fn dispatch_settings_font_size_set_value_rounds_and_clamps() {
        use crate::renderer::overlay::widgets::settings_font::{FONT_CATEGORY, row};

        let mut panel = SettingsPanel::default();
        let kind = NodeIdKind::SettingsWidget {
            category: FONT_CATEGORY,
            index: row::SIZE,
        };
        let set = |panel: &mut SettingsPanel, v: f64| {
            dispatch_settings_action(
                panel,
                Action::SetValue,
                &kind,
                Some(ActionData::NumericValue(v)),
            )
        };

        set(&mut panel, 13.3);
        assert!(
            (panel.font_size - 13.5).abs() < 1e-4,
            "0.5-unit rounding, actual = {}",
            panel.font_size
        );
        set(&mut panel, 100.0);
        assert!((panel.font_size - 32.0).abs() < f32::EPSILON);
        set(&mut panel, 1.0);
        assert!((panel.font_size - 8.0).abs() < f32::EPSILON);
    }

    /// Increment / Decrement on the size slider move in 0.5 steps.
    #[test]
    fn dispatch_settings_font_size_increment_decrement() {
        use crate::renderer::overlay::widgets::settings_font::{FONT_CATEGORY, row};

        let mut panel = SettingsPanel::default();
        let kind = NodeIdKind::SettingsWidget {
            category: FONT_CATEGORY,
            index: row::SIZE,
        };
        let before = panel.font_size;
        dispatch_settings_action(&mut panel, Action::Increment, &kind, None);
        assert!((panel.font_size - (before + 0.5)).abs() < 1e-4);
        dispatch_settings_action(&mut panel, Action::Decrement, &kind, None);
        assert!((panel.font_size - before).abs() < 1e-4);
    }

    /// Click / Increment on the Theme scheme cycler advance by 1 (UI/UX v3 P1b:
    /// the node is now widget-derived, but the behaviour is unchanged).
    #[test]
    fn dispatch_settings_theme_scheme_click_advances() {
        let mut panel = SettingsPanel::default();
        panel.scheme_index = 0;
        let kind = NodeIdKind::SettingsWidget {
            category: 2,
            index: 0,
        };

        dispatch_settings_action(&mut panel, Action::Click, &kind, None);
        assert_eq!(panel.scheme_index, 1, "Click selects next scheme");

        dispatch_settings_action(&mut panel, Action::Increment, &kind, None);
        assert_eq!(panel.scheme_index, 2, "Increment selects next scheme");

        dispatch_settings_action(&mut panel, Action::Decrement, &kind, None);
        assert_eq!(panel.scheme_index, 1, "Decrement selects previous scheme");
    }

    /// Helper: the Font category index.
    fn font_category() -> u8 {
        crate::renderer::overlay::widgets::settings_font::FONT_CATEGORY
    }

    /// Helper: the Font family row index.
    fn font_family_row() -> u16 {
        crate::renderer::overlay::widgets::settings_font::row::FAMILY
    }

    /// Helper: the NodeId of a widget-derived settings node.
    fn widget_id(category: u8, index: u16) -> NodeId {
        settings_widget_id(crate::renderer::overlay::widgets::spec::WidgetId::new(
            category, index,
        ))
    }

    /// Helper: the WidgetId of the Window opacity slider.
    fn window_opacity_widget_id() -> crate::renderer::overlay::widgets::spec::WidgetId {
        use crate::renderer::overlay::widgets::settings_window::{WINDOW_CATEGORY, row};
        crate::renderer::overlay::widgets::spec::WidgetId::new(WINDOW_CATEGORY, row::OPACITY)
    }

    /// Helper: the WidgetId of the Theme colour-scheme cycler.
    fn theme_scheme_widget_id() -> crate::renderer::overlay::widgets::spec::WidgetId {
        use crate::renderer::overlay::widgets::settings_theme::{THEME_CATEGORY, THEME_SCHEME};
        crate::renderer::overlay::widgets::spec::WidgetId::new(THEME_CATEGORY, THEME_SCHEME)
    }

    /// The Theme category exposes every control the renderer draws, not just
    /// the colour-scheme cycler the hand-written tree used to carry.
    #[test]
    fn settings_panel_theme_exposes_every_widget() {
        use crate::settings_panel::SettingsCategory;

        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = SettingsCategory::Theme;
        let update = build_tree_from_state(&state);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();

        let descs = crate::renderer::overlay::widgets::settings_theme::theme_widget_descs(
            &state.settings_panel,
        );
        assert_eq!(descs.len(), 11, "2 rows + 9 swatches");
        for desc in &descs {
            assert!(
                ids.contains(&settings_widget_id(desc.id).0),
                "widget {:?} missing from the tree",
                desc.id
            );
        }
    }

    /// A screen reader can flip the follow-system toggle and pick a swatch —
    /// neither was reachable before the migration.
    #[test]
    fn dispatch_settings_theme_toggle_and_swatch() {
        use crate::renderer::overlay::widgets::settings_theme::{
            THEME_CATEGORY, THEME_FOLLOW_SYSTEM, THEME_SWATCH_BASE,
        };

        let mut panel = SettingsPanel::default();
        let before = panel.colors_follow_system;
        assert!(dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsWidget {
                category: THEME_CATEGORY,
                index: THEME_FOLLOW_SYSTEM,
            },
            None,
        ));
        assert_eq!(panel.colors_follow_system, !before);

        assert!(dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsWidget {
                category: THEME_CATEGORY,
                index: THEME_SWATCH_BASE + 4,
            },
            None,
        ));
        assert_eq!(panel.scheme_index, 4, "a swatch is selected, not stepped");
    }

    /// An action aimed at a category that has not been migrated is refused
    /// rather than silently applied to a migrated tab.
    #[test]
    fn dispatch_settings_widget_ignores_unmigrated_categories() {
        let mut panel = SettingsPanel::default();
        // 5 = Keybindings, which still has its own hand-written nodes.
        assert!(!dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsWidget {
                category: 5,
                index: 0,
            },
            None,
        ));
    }

    /// SetValue on the opacity slider applies 0.05-unit rounding and clamping
    /// to 0.1..=1.0. UI/UX v3 P1c routes it through the widget layer, but the
    /// setter — and therefore the behaviour — is unchanged.
    #[test]
    fn dispatch_settings_opacity_set_value_rounds_and_clamps() {
        use crate::renderer::overlay::widgets::settings_window::{WINDOW_CATEGORY, row};

        let mut panel = SettingsPanel::default();
        let kind = NodeIdKind::SettingsWidget {
            category: WINDOW_CATEGORY,
            index: row::OPACITY,
        };
        let set = |panel: &mut SettingsPanel, v: f64| {
            dispatch_settings_action(
                panel,
                Action::SetValue,
                &kind,
                Some(ActionData::NumericValue(v)),
            )
        };

        assert!(set(&mut panel, 0.737));
        assert!(
            (panel.opacity - 0.75).abs() < 1e-4,
            "0.05-unit rounding: 0.737 -> 0.75, actual = {}",
            panel.opacity
        );

        set(&mut panel, 2.0);
        assert!(
            (panel.opacity - 1.0).abs() < f32::EPSILON,
            "clamped to the max"
        );

        set(&mut panel, 0.0);
        assert!(
            (panel.opacity - 0.1).abs() < f32::EPSILON,
            "clamped to the min"
        );

        // A SetValue with no numeric payload is malformed and must be refused
        // rather than falling through to a click.
        let before = panel.opacity;
        assert!(!dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &kind,
            None
        ));
        assert_eq!(panel.opacity, before);
    }

    /// Click / Increment on the language row advances it; Decrement goes back.
    #[test]
    fn dispatch_settings_language_click_advances() {
        use crate::renderer::overlay::widgets::settings_startup::{STARTUP_CATEGORY, row};

        let mut panel = SettingsPanel::default();
        let kind = NodeIdKind::SettingsWidget {
            category: STARTUP_CATEGORY,
            index: row::LANGUAGE,
        };
        let before = panel.language_index;
        dispatch_settings_action(&mut panel, Action::Click, &kind, None);
        assert_ne!(panel.language_index, before);
        dispatch_settings_action(&mut panel, Action::Decrement, &kind, None);
        assert_eq!(panel.language_index, before);
    }

    /// Click on the update-check row toggles it; Focus only moves focus.
    #[test]
    fn dispatch_settings_auto_update_click_toggles() {
        use crate::renderer::overlay::widgets::settings_startup::{STARTUP_CATEGORY, row};

        let mut panel = SettingsPanel::default();
        let kind = NodeIdKind::SettingsWidget {
            category: STARTUP_CATEGORY,
            index: row::CHECK_UPDATES,
        };
        let before = panel.auto_check_update;
        dispatch_settings_action(&mut panel, Action::Click, &kind, None);
        assert_eq!(panel.auto_check_update, !before);

        let after_click = panel.auto_check_update;
        dispatch_settings_action(&mut panel, Action::Focus, &kind, None);
        assert_eq!(
            panel.auto_check_update, after_click,
            "Focus must not flip a checkbox as the virtual cursor passes by"
        );
        assert_eq!(panel.startup_field_focus, row::CHECK_UPDATES);
    }

    // ===== Window category (UI/UX v3 P1c: widget-derived nodes) =====

    /// Every Window row decodes back to its own widget, so an action can
    /// never be applied to the wrong control.
    #[test]
    fn decode_node_id_round_trips_every_window_row() {
        use crate::renderer::overlay::widgets::settings_window::{
            WINDOW_CATEGORY, WINDOW_ROW_COUNT,
        };
        use crate::renderer::overlay::widgets::spec::WidgetId;

        for index in 0..WINDOW_ROW_COUNT as u16 {
            assert_eq!(
                decode_node_id(settings_widget_id(WidgetId::new(WINDOW_CATEGORY, index))),
                NodeIdKind::SettingsWidget {
                    category: WINDOW_CATEGORY,
                    index
                }
            );
        }
    }

    /// The Window category exposes every row, not the five the hand-written
    /// tree used to carry (UI/UX v3 P1c).
    #[test]
    fn build_settings_panel_nodes_window_exposes_every_row() {
        use crate::renderer::overlay::widgets::settings_window::{
            WINDOW_ROW_COUNT, window_widget_descs,
        };

        let mut panel = SettingsPanel::default();
        panel.category = crate::settings_panel::SettingsCategory::Window;
        let (nodes, _focus) = build_settings_panel_nodes(&panel);
        let ids: Vec<u64> = nodes.iter().map(|(id, _)| id.0).collect();

        let descs = window_widget_descs(&panel);
        assert_eq!(descs.len(), WINDOW_ROW_COUNT);
        for desc in &descs {
            assert!(
                ids.contains(&settings_widget_id(desc.id).0),
                "row {:?} missing from the tree",
                desc.id
            );
        }
    }

    /// Focus follows `window_field_focus` across every row, and falls back to
    /// the category tab when the index is out of range.
    #[test]
    fn build_settings_panel_nodes_window_focus_follows_field() {
        use crate::renderer::overlay::widgets::settings_window::{
            WINDOW_CATEGORY, WINDOW_ROW_COUNT,
        };
        use crate::renderer::overlay::widgets::spec::WidgetId;

        for focus_idx in 0..WINDOW_ROW_COUNT as u16 {
            let mut panel = SettingsPanel::default();
            panel.category = crate::settings_panel::SettingsCategory::Window;
            panel.window_field_focus = focus_idx;
            let (_nodes, focus) = build_settings_panel_nodes(&panel);
            assert_eq!(
                focus,
                settings_widget_id(WidgetId::new(WINDOW_CATEGORY, focus_idx)),
                "with window_field_focus={focus_idx}"
            );
        }

        let mut panel = SettingsPanel::default();
        panel.category = crate::settings_panel::SettingsCategory::Window;
        panel.window_field_focus = WINDOW_ROW_COUNT as u16;
        let (_nodes, focus) = build_settings_panel_nodes(&panel);
        assert_ne!(
            focus,
            settings_widget_id(WidgetId::new(WINDOW_CATEGORY, WINDOW_ROW_COUNT as u16)),
            "an out-of-range focus must fall back, not point at a missing node"
        );
    }

    /// compute_tree_state_hash: detects changes in
    /// window_field_focus / cursor_style / padding / present_mode.
    #[test]
    fn tree_hash_detects_window_field_changes() {
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.category = crate::settings_panel::SettingsCategory::Window;
        let h0 = compute_tree_state_hash(&state);

        // Focus change.
        state.settings_panel.window_field_focus = 1;
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "window_field_focus change must affect the hash");

        // cursor_style change.
        state.settings_panel.cursor_style = nexterm_config::CursorStyle::Beam;
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(h1, h2, "cursor_style change must affect the hash");

        // padding_x change.
        state.settings_panel.padding_x = 8;
        let h3 = compute_tree_state_hash(&state);
        assert_ne!(h2, h3, "padding_x change must affect the hash");

        // padding_y change.
        state.settings_panel.padding_y = 12;
        let h4 = compute_tree_state_hash(&state);
        assert_ne!(h3, h4, "padding_y change must affect the hash");

        // present_mode change.
        state.settings_panel.present_mode = nexterm_config::PresentModeConfig::Fifo;
        let h5 = compute_tree_state_hash(&state);
        assert_ne!(h4, h5, "present_mode change must affect the hash");
    }

    /// Non-settings-panel NodeIdKind values must return handled=false (no-op).
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

    // ===== Sprint 5-11-3: pane row node tests =====

    /// Build a `nexterm_proto::Grid` from string lines for testing.
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

    /// T1: trailing ASCII spaces are stripped.
    #[test]
    fn pane_row_text_strips_trailing_spaces() {
        let grid = grid_from_lines(&["hello   "]);
        assert_eq!(pane_row_text(&grid, 0), "hello");
    }

    /// T2: An empty row returns a single ASCII space (preserves the boundary the SR
    /// recognises as "blank line").
    #[test]
    fn pane_row_text_empty_row_returns_space() {
        let grid = grid_from_lines(&["        "]);
        assert_eq!(pane_row_text(&grid, 0), " ");
    }

    /// T3: full-width characters are preserved (only trailing ASCII spaces are stripped).
    #[test]
    fn pane_row_text_preserves_full_width() {
        let grid = grid_from_lines(&["あいう  "]);
        // grid.set writes by column, so 3 chars + 5 padding cells = 8 cells.
        // The result is "あいう" with trailing spaces stripped.
        let text = pane_row_text(&grid, 0);
        assert!(text.starts_with("あいう"), "unexpected: {:?}", text);
        assert!(!text.ends_with(' '), "trailing space remains: {:?}", text);
    }

    /// T4: requesting an out-of-range row returns " " instead of panicking.
    #[test]
    fn pane_row_text_handles_out_of_range_row() {
        let grid = grid_from_lines(&["hello"]);
        assert_eq!(pane_row_text(&grid, 100), " ");
    }

    /// T5: pane-row NodeIds do not collide with pane NodeIds.
    #[test]
    fn pane_row_node_id_no_collision_with_pane() {
        let pane_min = pane_node_id(0).0;
        let pane_max = pane_node_id(u32::MAX).0;
        let row_min = pane_row_node_id(0, 0).0;
        assert!(
            pane_max < row_min,
            "pane range [{}, {}] collides with row range [{}, ...]",
            pane_min,
            pane_max,
            row_min
        );
    }

    /// T6: pane-row NodeIds do not collide with tab NodeIds.
    #[test]
    fn pane_row_node_id_no_collision_with_tab() {
        let tab_max = tab_node_id(u32::MAX).0;
        let row_min = pane_row_node_id(0, 0).0;
        assert!(tab_max < row_min);
    }

    /// T7: pane_row_node_id ↔ decode_node_id roundtrip holds.
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

    /// T8: build_tree_from_state includes row nodes as children of each pane.
    #[test]
    fn build_tree_includes_pane_rows() {
        let mut state = ClientState::new(10, 5, 1000);
        // Add pane 1 with 5 rows × 10 columns.
        let mut pane = crate::state::PaneState::new(10, 5, 1000);
        pane.title = "test".to_string();
        // Write "hello" into row 0.
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

        // 5 PaneRow nodes are included in the tree.
        let row_node_count = update
            .nodes
            .iter()
            .filter(|(id, _)| matches!(decode_node_id(*id), NodeIdKind::PaneRow { pane_id: 1, .. }))
            .count();
        assert_eq!(row_node_count, 5, "5 PaneRow nodes are not present");

        // The row-0 node has "hello" set as its value.
        let row0_id = pane_row_node_id(1, 0);
        let row0_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == row0_id)
            .map(|(_, n)| n)
            .expect("row 0 node not found");
        assert_eq!(row0_node.value(), Some("hello"));
    }

    /// T9: Behaviour changed in Sprint 5-11-4 — Live::Polite is applied only to the
    /// **cursor row** of the focused pane (previously: all rows of the focused pane).
    ///
    /// To suppress over-announcement, the SR only reads diffs on `cursor_row`.
    /// Non-cursor rows and non-focused panes use Live::None (unspecified).
    #[test]
    fn build_tree_focused_pane_has_live_polite() {
        let mut state = ClientState::new(5, 3, 1000);
        let mut pane1 = crate::state::PaneState::new(5, 3, 1000);
        // Set cursor_row to 1 to reliably verify "cursor row only is Polite".
        pane1.grid.cursor_row = 1;
        let pane2 = crate::state::PaneState::new(5, 3, 1000);
        state.panes.insert(1, pane1);
        state.panes.insert(2, pane2);
        state.tab_order = vec![1, 2];
        state.focused_pane_id = Some(1);

        let update = build_tree_from_state(&state);

        // Only the cursor row (row 1) of pane 1 (focused) is Live::Polite.
        let row1_cursor = update
            .nodes
            .iter()
            .find(|(id, _)| *id == pane_row_node_id(1, 1))
            .map(|(_, n)| n)
            .expect("pane 1 row 1 (cursor row) not found");
        assert_eq!(row1_cursor.live(), Some(Live::Polite));

        // Non-cursor rows (row 0 / 2) of pane 1 (focused) are Live::None.
        for row in [0u16, 2u16] {
            let n = update
                .nodes
                .iter()
                .find(|(id, _)| *id == pane_row_node_id(1, row))
                .map(|(_, n)| n)
                .unwrap_or_else(|| panic!("pane 1 row {row} not found"));
            assert_eq!(
                n.live(),
                None,
                "pane 1 row {row} is a non-cursor row so it must be Live::None"
            );
        }

        // All rows of pane 2 (non-focused) are Live::None.
        for row in 0u16..3u16 {
            let n = update
                .nodes
                .iter()
                .find(|(id, _)| *id == pane_row_node_id(2, row))
                .map(|(_, n)| n)
                .unwrap_or_else(|| panic!("pane 2 row {row} not found"));
            assert_eq!(
                n.live(),
                None,
                "non-focused pane 2 row {row} must be Live::None"
            );
        }
    }

    /// T10: compute_grid_row_hashes detects row content changes.
    #[test]
    fn compute_grid_row_hashes_detects_change() {
        let mut grid = grid_from_lines(&["hello", "world", "     "]);
        let baseline = compute_grid_row_hashes(&grid);
        assert_eq!(baseline.len(), 3);

        // The same grid yields the same hashes.
        let same = compute_grid_row_hashes(&grid);
        assert_eq!(baseline, same);

        // Change a single cell on row 1.
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
        // Row 0 and row 2 are unchanged; only row 1 changes.
        assert_eq!(after[0], baseline[0], "row 0 must be unchanged");
        assert_ne!(after[1], baseline[1], "row 1 must change");
        assert_eq!(after[2], baseline[2], "row 2 must be unchanged");
    }

    // ===== Sprint 5-11-4: cursor TextSelection + scrollback =====

    /// 5-11-4 T1: each ASCII row character_lengths entry is 1 byte.
    #[test]
    fn pane_row_text_with_lengths_ascii() {
        let grid = grid_from_lines(&["abc"]);
        let (text, lengths) = pane_row_text_with_lengths(&grid, 0);
        assert_eq!(text, "abc");
        assert_eq!(lengths, vec![1, 1, 1]);
    }

    /// 5-11-4 T2: full-width CJK is 3 bytes each in UTF-8.
    #[test]
    fn pane_row_text_with_lengths_cjk() {
        let grid = grid_from_lines(&["あい"]);
        let (text, lengths) = pane_row_text_with_lengths(&grid, 0);
        // On the grid this is 2 full-width cells + 1 placeholder space cell each = 4 cells.
        // However the `grid_from_lines` helper may not insert placeholders when set()
        // writes chars directly, so just verify behaviour matches pane_row_text.
        assert!(text.starts_with("あ"));
        assert!(text.contains("い"));
        // Each char is in the 1..=4 byte range.
        assert!(lengths.iter().all(|&b| (1..=4).contains(&b)));
        // Sum of byte lengths == text.len()
        let sum: usize = lengths.iter().map(|&b| b as usize).sum();
        assert_eq!(sum, text.len());
    }

    /// 5-11-4 T3: an empty row returns (" ", [1]).
    #[test]
    fn pane_row_text_with_lengths_empty_row() {
        // Build a grid with one row of empty cells (grid_from_lines makes an empty
        // grid for empty strings, so we use a single space instead).
        let grid = grid_from_lines(&[" "]);
        let (text, lengths) = pane_row_text_with_lengths(&grid, 0);
        assert_eq!(text, " ");
        assert_eq!(lengths, vec![1]);
    }

    /// 5-11-4 T4: an out-of-range row is treated like an empty row.
    #[test]
    fn pane_row_text_with_lengths_out_of_range_row() {
        let grid = grid_from_lines(&["abc"]);
        let (text, lengths) = pane_row_text_with_lengths(&grid, 99);
        assert_eq!(text, " ");
        assert_eq!(lengths, vec![1]);
    }

    /// 5-11-4 T5: cursor_character_index returns cursor_col unchanged (in-range case).
    #[test]
    fn cursor_character_index_within_range() {
        assert_eq!(cursor_character_index("hello", 0), 0);
        assert_eq!(cursor_character_index("hello", 3), 3);
        assert_eq!(cursor_character_index("hello", 5), 5);
    }

    /// 5-11-4 T6: cursor_character_index clamps when exceeding the char count.
    #[test]
    fn cursor_character_index_clamps_to_char_count() {
        // "hello" is 5 chars.
        assert_eq!(cursor_character_index("hello", 10), 5);
        // Empty string (in practice pane_row_text returns " ", so this is defensive).
        assert_eq!(cursor_character_index("", 5), 0);
    }

    /// 5-11-4 T7: full-width characters (CJK) count as 1 char each, not by byte length.
    #[test]
    fn cursor_character_index_cjk_is_char_based() {
        // "あい" is 2 chars (6 bytes).
        assert_eq!(cursor_character_index("あい", 2), 2);
        // Clamping is also based on 2 chars.
        assert_eq!(cursor_character_index("あい", 5), 2);
    }

    /// 5-11-4 T8: pane_scrollback_row_node_id does not collide with viewport-row NodeIds.
    #[test]
    fn pane_scrollback_row_node_id_no_collision_with_viewport_row() {
        let pane_id = 7u32;
        // Viewport rows [0..1000) and scrollback rows [0..9000) do not collide within
        // the same pane.
        for row in [0u16, 100, 500, 999] {
            let v_id = pane_row_node_id(pane_id, row);
            for sb in [0u16, 100, 500, 8999] {
                let sb_id = pane_scrollback_row_node_id(pane_id, sb);
                assert_ne!(
                    v_id, sb_id,
                    "viewport row {row} and scrollback {sb} NodeIds collide"
                );
            }
        }
    }

    /// 5-11-4 T9: scrollback-row NodeIds do not collide across panes.
    #[test]
    fn pane_scrollback_row_node_id_no_collision_between_panes() {
        // Pane 1's last scrollback entry (idx=8999) and pane 2's first scrollback entry
        // (idx=0) are separated by MAX_ROWS_PER_PANE, so they do not collide.
        let id1_last = pane_scrollback_row_node_id(1, (MAX_SCROLLBACK_ROWS_PER_PANE - 1) as u16);
        let id2_first = pane_scrollback_row_node_id(2, 0);
        assert_ne!(id1_last, id2_first);
        // Range check.
        assert!(id1_last.0 < id2_first.0);
    }

    /// 5-11-4 T10: decode_node_id correctly decodes scrollback rows as PaneScrollbackRow.
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

    /// 5-11-4 T11: no scrollback-row nodes are emitted when the scrollback is empty.
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
        assert_eq!(sb_node_count, 0, "an empty scrollback yields 0 row nodes");
    }

    /// 5-11-4 T12: pushing rows into the scrollback adds row nodes to the tree.
    #[test]
    fn build_tree_includes_scrollback_rows_when_present() {
        let mut state = ClientState::new(5, 2, 1000);
        let mut pane = crate::state::PaneState::new(5, 2, 1000);
        // Append 3 rows to the scrollback.
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
        assert_eq!(sb_node_count, 3, "3 scrollback row nodes must be present");
    }

    /// 5-11-4 T13: even when the scrollback far exceeds SCROLLBACK_WINDOW_RADIUS * 2,
    /// only rows within the window are exposed.
    #[test]
    fn build_tree_scrollback_window_radius_limit() {
        let mut state = ClientState::new(5, 2, 1000);
        let mut pane = crate::state::PaneState::new(5, 2, 1000);
        // Push 500 scrollback rows (5× SCROLLBACK_WINDOW_RADIUS=100).
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
        // Window width is [center - RADIUS, center + RADIUS + 1), so at most
        // 2*RADIUS + 1 rows.
        let expected_max = SCROLLBACK_WINDOW_RADIUS * 2 + 1;
        assert!(
            sb_node_count <= expected_max,
            "scrollback row count {sb_node_count} exceeds window limit {expected_max}"
        );
        assert!(
            sb_node_count > 0,
            "at least 1 row must fall within the window"
        );
    }

    /// 5-11-4 T14: a TextSelection is set on the focused pane's cursor row.
    #[test]
    fn build_tree_focused_pane_cursor_row_has_text_selection() {
        let mut state = ClientState::new(10, 5, 1000);
        let mut pane = crate::state::PaneState::new(10, 5, 1000);
        // Write "abc" to row 2 and place the cursor at (col=2, row=2).
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
            .expect("pane node not found");
        let sel = pane_node
            .text_selection()
            .expect("a TextSelection must be set on the focused pane's cursor row");
        // anchor == focus == TextPosition { node: pane_row_node_id(1, 2), character_index: 2 }
        assert_eq!(sel.anchor.node, pane_row_node_id(1, 2));
        assert_eq!(sel.focus.node, pane_row_node_id(1, 2));
        assert_eq!(sel.anchor.character_index, 2);
        assert_eq!(sel.focus.character_index, 2);
    }

    /// 5-11-4 T15: non-focused panes do not have a TextSelection.
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
            .expect("pane 2 not found");
        assert!(
            pane2_node.text_selection().is_none(),
            "a non-focused pane must not have a TextSelection set"
        );
    }

    /// 5-11-4 T16: tree_state_hash detects cursor movement.
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
        assert_ne!(h1, h2, "the hash must change when cursor_col changes");

        state.panes.get_mut(&1).unwrap().grid.cursor_row = 2;
        let h3 = compute_tree_state_hash(&state);
        assert_ne!(h2, h3, "the hash must change when cursor_row changes");
    }

    /// 5-11-4 T17: tree_state_hash detects scrollback growth.
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
        assert_ne!(h1, h2, "the hash must change when scrollback.len changes");
    }

    /// 5-11-4 T18: tree_state_hash detects scroll_offset changes.
    #[test]
    fn tree_state_hash_detects_scroll_offset_change() {
        let mut state = ClientState::new(5, 2, 1000);
        let mut pane = crate::state::PaneState::new(5, 2, 1000);
        // Push 5 scrollback rows so that scroll_offset > 0 is meaningful.
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
        assert_ne!(h1, h2, "the hash must change when scroll_offset changes");
    }

    // ===== Sprint 5-11-5: Bell / OSC 9 / OSC 777 → Role::Alert tests =====

    /// add_alert appends to the queue and seq increases monotonically.
    #[test]
    fn add_alert_assigns_monotonic_seq() {
        let mut state = ClientState::new(80, 24, 1000);
        let s0 = state.add_alert(AlertKind::Bell, 1, "Bell".to_string(), String::new());
        let s1 = state.add_alert(
            AlertKind::Notification,
            1,
            "Title".to_string(),
            "Body".to_string(),
        );
        let s2 = state.add_alert(AlertKind::Bell, 2, "Bell".to_string(), String::new());
        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(state.alerts.len(), 3);
        assert_eq!(state.alerts[0].kind, AlertKind::Bell);
        assert_eq!(state.alerts[1].kind, AlertKind::Notification);
        assert_eq!(state.alerts[2].pane_id, 2);
    }

    /// Entries beyond ALERTS_MAX_LEN (16) are dropped in oldest-first order.
    #[test]
    fn add_alert_drops_oldest_when_full() {
        use crate::state::ALERTS_MAX_LEN;
        let mut state = ClientState::new(80, 24, 1000);
        for i in 0..(ALERTS_MAX_LEN + 5) {
            state.add_alert(AlertKind::Bell, 1, format!("alert {}", i), String::new());
        }
        // Capped within the limit.
        assert_eq!(state.alerts.len(), ALERTS_MAX_LEN);
        // The head starts at ALERTS_MAX_LEN + 5 - ALERTS_MAX_LEN = 5.
        assert_eq!(state.alerts.front().unwrap().seq, 5);
        assert_eq!(
            state.alerts.back().unwrap().seq,
            (ALERTS_MAX_LEN + 5) as u64 - 1
        );
    }

    /// expire_alerts drops TTL-expired entries while keeping fresh entries.
    #[test]
    fn expire_alerts_removes_only_expired_entries() {
        use crate::state::ALERT_TTL;
        let mut state = ClientState::new(80, 24, 1000);
        // The two old entries get their created_at set in the past manually (direct push_back).
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
        // The one fresh entry is added via add_alert.
        state.add_alert(AlertKind::Bell, 1, "fresh".to_string(), String::new());

        let removed = state.expire_alerts(now);
        assert_eq!(removed, 2, "the 2 old entries are removed");
        assert_eq!(state.alerts.len(), 1);
        assert_eq!(state.alerts.front().unwrap().title, "fresh");
    }

    /// Phase 5-11-6 #4: `dismiss_alert(seq)` removes only the entry with the given seq.
    #[test]
    fn dismiss_alert_removes_matching_seq_only() {
        let mut state = ClientState::new(80, 24, 1000);
        let seq_a = state.add_alert(AlertKind::Bell, 1, "a".to_string(), String::new());
        let seq_b = state.add_alert(AlertKind::Bell, 1, "b".to_string(), String::new());
        let seq_c = state.add_alert(AlertKind::Bell, 1, "c".to_string(), String::new());
        assert_eq!(state.alerts.len(), 3);

        // Remove only the middle entry B.
        let dismissed = state.dismiss_alert(seq_b);
        assert!(dismissed, "dismiss returns true for an existing seq");
        assert_eq!(state.alerts.len(), 2);
        let remaining: Vec<u64> = state.alerts.iter().map(|a| a.seq).collect();
        assert_eq!(remaining, vec![seq_a, seq_c], "only A and C remain");
    }

    /// Phase 5-11-6 #4: `dismiss_alert` for an unknown seq returns false and has no side effects.
    #[test]
    fn dismiss_alert_returns_false_for_unknown_seq() {
        let mut state = ClientState::new(80, 24, 1000);
        let seq = state.add_alert(AlertKind::Bell, 1, "only".to_string(), String::new());
        // Specify a different seq.
        let dismissed = state.dismiss_alert(seq.wrapping_add(99));
        assert!(!dismissed, "dismiss returns false for an unknown seq");
        assert_eq!(state.alerts.len(), 1, "no side effects");
    }

    /// alert_node_id is at the 50e12 offset + seq and does not collide with the pane_row range.
    #[test]
    fn alert_node_id_in_correct_offset() {
        let id0 = alert_node_id(0).0;
        let id_big = alert_node_id(u32::MAX as u64).0;
        assert_eq!(id0, NODE_ID_ALERT_OFFSET);
        assert_eq!(id_big, NODE_ID_ALERT_OFFSET + u32::MAX as u64);
        // Must exceed the upper bound of the pane row range (~4.3e13).
        let pane_row_end =
            NODE_ID_PANE_ROW_OFFSET + (u32::MAX as u64) * MAX_ROWS_PER_PANE + MAX_ROWS_PER_PANE;
        assert!(
            NODE_ID_ALERT_OFFSET >= pane_row_end,
            "Alert offset ({}) must be at least the pane row upper bound ({})",
            NODE_ID_ALERT_OFFSET,
            pane_row_end
        );
    }

    /// decode_node_id can reverse-look-up an Alert NodeId.
    #[test]
    fn decode_alert_node_id_roundtrip() {
        for seq in [0u64, 1, 16, 100, u32::MAX as u64] {
            let nid = alert_node_id(seq);
            let kind = decode_node_id(nid);
            assert_eq!(kind, NodeIdKind::Alert { seq });
        }
        // AlertRegion fixed ID.
        assert_eq!(decode_node_id(ALERT_REGION_ID), NodeIdKind::AlertRegion);
    }

    /// ALERT_REGION_ID is not included in ROOT when the queue is empty.
    #[test]
    fn build_tree_without_alerts_omits_alert_region() {
        let state = ClientState::new(80, 24, 1000);
        let update = build_tree_from_state(&state);
        let root_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == ROOT_ID)
            .expect("ROOT exists");
        // ROOT's children do not contain ALERT_REGION_ID.
        let children: Vec<NodeId> = root_node.1.children().to_vec();
        assert!(
            !children.contains(&ALERT_REGION_ID),
            "ALERT_REGION_ID must not appear in ROOT when no alerts exist"
        );
        // The ALERT_REGION_ID node itself is also absent.
        assert!(
            !update.nodes.iter().any(|(id, _)| *id == ALERT_REGION_ID),
            "the ALERT_REGION node must not be present when no alerts exist"
        );
    }

    /// Adding an alert appends ALERT_REGION_ID and each Alert node as ROOT children.
    #[test]
    fn build_tree_with_alerts_includes_alert_region_and_children() {
        let mut state = ClientState::new(80, 24, 1000);
        let seq_bell = state.add_alert(AlertKind::Bell, 1, "Bell".to_string(), String::new());
        let seq_notify = state.add_alert(
            AlertKind::Notification,
            1,
            "Build finished".to_string(),
            "exit code 0".to_string(),
        );

        let update = build_tree_from_state(&state);

        // ROOT contains ALERT_REGION_ID.
        let root = update.nodes.iter().find(|(id, _)| *id == ROOT_ID).unwrap();
        assert!(root.1.children().contains(&ALERT_REGION_ID));

        // ALERT_REGION itself exists and is Live::Assertive.
        let region = update
            .nodes
            .iter()
            .find(|(id, _)| *id == ALERT_REGION_ID)
            .expect("ALERT_REGION node must exist");
        assert_eq!(region.1.live(), Some(Live::Assertive));

        // Each Alert node exists.
        let bell_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == alert_node_id(seq_bell))
            .expect("Bell node must exist");
        assert_eq!(bell_node.1.role(), Role::Alert);
        assert_eq!(bell_node.1.label(), Some("Bell"));

        let notify_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == alert_node_id(seq_notify))
            .expect("Notification node must exist");
        assert_eq!(notify_node.1.role(), Role::Alert);
        assert_eq!(notify_node.1.label(), Some("Notification: Build finished"));
        assert_eq!(notify_node.1.description(), Some("exit code 0"));
    }

    /// tree_state_hash changes when an alert is added.
    #[test]
    fn tree_state_hash_detects_alert_added() {
        let mut state = ClientState::new(80, 24, 1000);
        let h0 = compute_tree_state_hash(&state);
        state.add_alert(AlertKind::Bell, 1, "Bell".to_string(), String::new());
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "the hash must change when an alert is added");
        // Even adding the same kind of alert changes the hash because seq differs.
        state.add_alert(AlertKind::Bell, 1, "Bell".to_string(), String::new());
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(
            h1, h2,
            "the hash must change after the second alert is added"
        );
    }

    /// tree_state_hash changes when the alert kind differs.
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
        assert_ne!(h1, h2, "Bell and Notification produce different hashes");
    }

    /// An Alert (Bell) with empty body does not have a description set.
    #[test]
    fn build_tree_alert_without_body_omits_description() {
        let mut state = ClientState::new(80, 24, 1000);
        let seq = state.add_alert(AlertKind::Bell, 1, "Bell".to_string(), String::new());
        let update = build_tree_from_state(&state);
        let bell = update
            .nodes
            .iter()
            .find(|(id, _)| *id == alert_node_id(seq))
            .unwrap();
        assert_eq!(bell.1.description(), None);
    }

    // ===== Phase 5-11-7: PTY input buffer =====

    /// PaneInputBuffer with NodeId(27) decodes to `NodeIdKind::PaneInputBuffer`.
    #[test]
    fn decode_pane_input_buffer() {
        assert_eq!(
            decode_node_id(PANE_INPUT_BUFFER_ID),
            NodeIdKind::PaneInputBuffer
        );
        assert_eq!(decode_node_id(NodeId(27)), NodeIdKind::PaneInputBuffer);
    }

    /// PaneInputBuffer is always present as a PaneArea child and has Role::TextInput.
    #[test]
    fn build_tree_includes_pane_input_buffer() {
        let state = ClientState::new(80, 24, 1000);
        let update = build_tree_from_state(&state);

        let input_buffer = update
            .nodes
            .iter()
            .find(|(id, _)| *id == PANE_INPUT_BUFFER_ID)
            .expect("PaneInputBuffer node must exist");
        assert_eq!(input_buffer.1.role(), Role::TextInput);
        assert_eq!(input_buffer.1.label(), Some("Terminal input buffer"));
        assert_eq!(input_buffer.1.value(), Some(""));
    }

    /// PaneInputBuffer's description includes the focused pane's title.
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
            "description must contain the pane title: {}",
            desc
        );
    }

    /// When no pane is focused, the description shows "No focused pane".
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
            desc.contains("No focused pane"),
            "description must contain the no-focus message: {}",
            desc
        );
    }

    /// PaneInputBuffer is appended as the last child of PaneArea.
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
            "PaneArea's last child must be PaneInputBuffer"
        );
        // Pane body + PaneInputBuffer = 2 children.
        assert_eq!(children.len(), 2);
    }

    /// Changing the focused pane also changes the tree hash (input-buffer description reflects it).
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
        assert_ne!(
            h0, h1,
            "the hash must change when the focused pane's title changes"
        );
    }

    // ===== Phase 5-11-7: SettingsPanel Profiles + Ssh/Keybindings description =====

    /// When the Profiles category is empty: shows the "No profiles defined" guidance.
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
            desc.contains("No profiles defined"),
            "the empty guidance message must be included: {}",
            desc
        );
    }

    /// When the Profiles category has entries: the cycler and each entry are
    /// exposed as widget nodes (UI/UX v3 P1c — previously hand-written
    /// `SettingsProfileItem` ListBoxOptions, and the cycler had no node).
    #[test]
    fn build_settings_panel_profiles_exposes_widget_nodes() {
        use crate::renderer::overlay::widgets::settings_profiles::{PROFILES_CATEGORY, row};
        use crate::renderer::overlay::widgets::spec::WidgetId;
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
        let entry_id = |i: usize| {
            settings_widget_id(WidgetId::new(PROFILES_CATEGORY, row::LIST_BASE + i as u16))
        };

        // The active-profile cycler is reachable now (it had no node before).
        let cycler_id = settings_widget_id(WidgetId::new(PROFILES_CATEGORY, row::ACTIVE));
        let cycler = nodes.iter().find(|(id, _)| *id == cycler_id).unwrap();
        assert_eq!(cycler.1.role(), Role::ComboBox);

        // Each entry is exposed as a ListBoxOption.
        let opt0 = nodes.iter().find(|(id, _)| *id == entry_id(0)).unwrap();
        assert_eq!(opt0.1.role(), Role::ListBoxOption);
        assert!(opt0.1.label().unwrap_or("").contains("bash"));
        assert_eq!(opt0.1.is_selected(), None); // unselected (set_selected is not called)

        let opt1 = nodes.iter().find(|(id, _)| *id == entry_id(1)).unwrap();
        assert_eq!(opt1.1.role(), Role::ListBoxOption);
        assert!(opt1.1.label().unwrap_or("").contains("powershell"));
        // selected_profile = 1 so this one is selected.
        assert_eq!(opt1.1.is_selected(), Some(true));

        // Focus moves to the selected profile entry.
        assert_eq!(focus, entry_id(1));
    }

    /// dispatch_settings_action: Click on a profile entry widget updates
    /// selected_profile (via the shared action router).
    #[test]
    fn dispatch_settings_profile_entry_click() {
        use crate::renderer::overlay::widgets::settings_profiles::{PROFILES_CATEGORY, row};
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
            &NodeIdKind::SettingsWidget {
                category: PROFILES_CATEGORY,
                index: row::LIST_BASE + 1,
            },
            None,
        );
        assert!(handled);
        assert_eq!(panel.selected_profile, 1);
    }

    /// dispatch_settings_action: Focus on a profile entry widget also updates
    /// selected_profile (virtual-cursor traversal = list selection).
    #[test]
    fn dispatch_settings_profile_entry_focus() {
        use crate::renderer::overlay::widgets::settings_profiles::{PROFILES_CATEGORY, row};
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
            &NodeIdKind::SettingsWidget {
                category: PROFILES_CATEGORY,
                index: row::LIST_BASE,
            },
            None,
        );
        assert!(handled);
        assert_eq!(panel.selected_profile, 0);
    }

    /// dispatch_settings_action: an out-of-range profile entry is a no-op and
    /// returns false.
    #[test]
    fn dispatch_settings_profile_entry_out_of_range() {
        use crate::renderer::overlay::widgets::settings_profiles::{PROFILES_CATEGORY, row};
        let mut panel = SettingsPanel::default();
        panel.profiles = vec![];

        let handled = dispatch_settings_action(
            &mut panel,
            accesskit::Action::Click,
            &NodeIdKind::SettingsWidget {
                category: PROFILES_CATEGORY,
                index: row::LIST_BASE + 5,
            },
            None,
        );
        assert!(!handled);
        assert_eq!(panel.selected_profile, 0);
    }

    /// The SSH category (empty list) has a description that prompts adding a new
    /// host via the GUI. In Phase 5-11-8 Step 8-3 Sub-phase D this changed from
    /// TOML-edit guidance to GUI guidance.
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
        // Sub-phase D onward: guidance is to add via the GUI Add button.
        assert!(
            desc.contains("Add") || desc.contains("add"),
            "guidance text: {}",
            desc
        );
        assert!(
            !desc.contains("not implemented yet"),
            "the \"not implemented\" wording must be gone"
        );
    }

    /// Phase 5-11-9 Sub-phase E: the Keybindings category now exposes interactive
    /// GUI nodes and a non-empty description (no longer the TOML-editing guidance).
    #[test]
    fn build_settings_panel_keybindings_has_informative_description() {
        use crate::settings_panel::SettingsCategory;
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Keybindings;
        // Leave the default `keybindings` list intact (built-in defaults are loaded).

        let (nodes, _focus) = build_settings_panel_nodes(&panel);
        let content = nodes
            .iter()
            .find(|(id, _)| *id == SETTINGS_CONTENT_ID)
            .unwrap();
        let desc = content.1.description().unwrap_or("");
        assert!(
            desc.contains("Editing binding") || desc.contains("No keybindings"),
            "guidance text: {}",
            desc
        );
        assert!(
            !desc.contains("not implemented yet"),
            "the \"not implemented\" wording must be gone"
        );
    }

    /// tree_state_hash changes when selected_profile changes.
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
        assert_ne!(h0, h1, "the hash must change when selected_profile changes");
    }

    /// widget_node: a SpinButton is announced with its role and numeric range
    /// (the Ssh port field relies on Increment/Decrement/SetValue working).
    #[test]
    fn widget_node_spin_button_exposes_numeric_range() {
        use crate::renderer::overlay::widgets::spec::{WidgetDesc, WidgetId, WidgetKind};
        let desc = WidgetDesc::new(
            WidgetId::new(4, 3),
            WidgetKind::SpinButton {
                value: 22.0,
                min: 1.0,
                max: 65535.0,
                step: 1.0,
                display: "22".to_string(),
            },
            "Port",
        );
        let node = widget_node(&desc);
        assert_eq!(node.role(), Role::SpinButton);
        assert_eq!(node.numeric_value(), Some(22.0));
        assert_eq!(node.min_numeric_value(), Some(1.0));
        assert_eq!(node.max_numeric_value(), Some(65535.0));
    }

    /// widget_node: a disabled widget is announced as disabled (first needed
    /// by the Ssh Delete button, which is inert while the host list is empty).
    #[test]
    fn widget_node_marks_disabled_widgets() {
        use crate::renderer::overlay::widgets::spec::{WidgetDesc, WidgetId, WidgetKind};
        let mut desc = WidgetDesc::new(
            WidgetId::new(4, 7),
            WidgetKind::Button { destructive: true },
            "Delete",
        );
        desc.enabled = false;
        let node = widget_node(&desc);
        assert!(node.is_disabled());
    }

    /// tree_state_hash changes when active_profile_index changes: the
    /// Profiles cycler's exposed value depends on it (UI/UX v3 P1c), so a
    /// stale hash would leave a screen reader announcing the old profile.
    #[test]
    fn tree_state_hash_detects_active_profile_change() {
        use crate::settings_panel::ProfileEntry;
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.profiles = vec![ProfileEntry {
            name: "a".to_string(),
            icon: String::new(),
            shell_program: String::new(),
            working_dir: String::new(),
        }];
        let h0 = compute_tree_state_hash(&state);

        state.settings_panel.next_active_profile();
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(
            h0, h1,
            "the hash must change when the active profile changes"
        );
    }

    /// tree_state_hash changes when the profiles list changes.
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
        assert_ne!(h0, h1, "the hash must change when profiles are added");
    }

    // ===== Phase 5-11-8 Step 8-1: SSH host ListBox =====

    /// SshHostEntry::label formatting rules.
    #[test]
    fn ssh_host_entry_label_format() {
        use crate::settings_panel::SshHostEntry;

        // Normal: name (user@host:port)
        let h = SshHostEntry {
            name: "myhost".to_string(),
            host: "example.com".to_string(),
            port: 2222,
            username: "alice".to_string(),
            auth_type: "key".to_string(),
        };
        assert_eq!(h.label(), "myhost (alice@example.com:2222)");

        // port = 22 is omitted.
        let h22 = SshHostEntry {
            port: 22,
            ..h.clone()
        };
        assert_eq!(h22.label(), "myhost (alice@example.com)");

        // When name is empty, only the endpoint is shown.
        let h_noname = SshHostEntry {
            name: String::new(),
            ..h.clone()
        };
        assert_eq!(h_noname.label(), "alice@example.com:2222");

        // When username is empty, only the host is shown.
        let h_nouser = SshHostEntry {
            username: String::new(),
            ..h.clone()
        };
        assert_eq!(h_nouser.label(), "myhost (example.com:2222)");
    }

    /// The 700M..800M range now carries `SettingsWidget` ids (UI/UX v3 P1b).
    /// Only the sub-range a `WidgetId` can actually encode decodes to a
    /// widget; the rest of the block stays Unknown, so a stray id in there is
    /// still rejected rather than aliased onto a real control.
    #[test]
    fn settings_widget_range_decodes_only_encodable_ids() {
        use crate::renderer::overlay::widgets::spec::WidgetId;

        for (category, index) in [(0u8, 0u16), (2, 0), (2, 18), (255, 65535)] {
            let id = WidgetId::new(category, index);
            assert_eq!(
                decode_node_id(settings_widget_id(id)),
                NodeIdKind::SettingsWidget { category, index }
            );
        }
        // Above the packed category byte: not producible by `as_u32`.
        assert_eq!(
            decode_node_id(NodeId(SETTINGS_WIDGET_BASE + 0x0100_0000)),
            NodeIdKind::Unknown
        );
        assert_eq!(decode_node_id(NodeId(799_999_999)), NodeIdKind::Unknown);
    }

    /// The empty SSH category includes "No SSH hosts are registered" plus GUI add guidance.
    /// In Phase 5-11-8 Step 8-3 Sub-phase D this changed from TOML-edit guidance to GUI guidance.
    #[test]
    fn build_settings_panel_ssh_empty_has_informative_description() {
        use crate::settings_panel::SettingsCategory;
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Ssh;
        panel.ssh_hosts = vec![];

        let (nodes, _focus) = build_settings_panel_nodes(&panel);
        let content = nodes
            .iter()
            .find(|(id, _)| *id == SETTINGS_CONTENT_ID)
            .unwrap();
        let desc = content.1.description().unwrap_or("");
        assert!(
            desc.contains("No SSH hosts are registered"),
            "the empty-list guidance must be included: {}",
            desc
        );
        // Sub-phase D onward: guidance is to add via the GUI Add button.
        assert!(
            desc.contains("Add") || desc.contains("add"),
            "the GUI guidance must be included: {}",
            desc
        );
    }

    /// Test helper: the widget NodeId of the Ssh host entry at `i`.
    fn ssh_entry_widget_id(i: usize) -> NodeId {
        use crate::renderer::overlay::widgets::settings_ssh::{SSH_CATEGORY, row};
        use crate::renderer::overlay::widgets::spec::WidgetId;
        settings_widget_id(WidgetId::new(SSH_CATEGORY, row::LIST_BASE + i as u16))
    }

    /// Test helper: the widget NodeId of an Ssh field/button at `index`.
    fn ssh_widget_id(index: u16) -> NodeId {
        use crate::renderer::overlay::widgets::settings_ssh::SSH_CATEGORY;
        use crate::renderer::overlay::widgets::spec::WidgetId;
        settings_widget_id(WidgetId::new(SSH_CATEGORY, index))
    }

    /// When the SSH category has hosts: entry widget nodes are exposed
    /// (UI/UX v3 P1c — previously hand-written `SettingsSshHostItem` nodes).
    #[test]
    fn build_settings_panel_ssh_exposes_listbox_options() {
        use crate::settings_panel::{SettingsCategory, SshHostEntry};
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Ssh;
        panel.ssh_hosts = vec![
            SshHostEntry {
                name: "prod".to_string(),
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                auth_type: "key".to_string(),
            },
            SshHostEntry {
                name: "staging".to_string(),
                host: "stg.example.com".to_string(),
                port: 2222,
                username: "alice".to_string(),
                auth_type: "agent".to_string(),
            },
        ];
        panel.selected_host_index = 1;

        let (nodes, focus) = build_settings_panel_nodes(&panel);

        // Each ListBoxOption is exposed.
        let opt0 = nodes
            .iter()
            .find(|(id, _)| *id == ssh_entry_widget_id(0))
            .unwrap();
        assert_eq!(opt0.1.role(), Role::ListBoxOption);
        assert!(opt0.1.label().unwrap_or("").contains("prod"));
        // The description includes the auth method.
        assert!(
            opt0.1.description().unwrap_or("").contains("key"),
            "the auth method must be included in the description"
        );
        assert_eq!(opt0.1.is_selected(), None);

        let opt1 = nodes
            .iter()
            .find(|(id, _)| *id == ssh_entry_widget_id(1))
            .unwrap();
        assert_eq!(opt1.1.role(), Role::ListBoxOption);
        assert!(opt1.1.label().unwrap_or("").contains("staging"));
        assert!(opt1.1.label().unwrap_or("").contains(":2222"));
        // selected_host_index = 1 so this entry is selected.
        assert_eq!(opt1.1.is_selected(), Some(true));

        // Focus moves to the selected host item (ssh_field_focus = 0).
        assert_eq!(focus, ssh_entry_widget_id(1));

        // SETTINGS_CONTENT includes the host count.
        let content = nodes
            .iter()
            .find(|(id, _)| *id == SETTINGS_CONTENT_ID)
            .unwrap();
        let desc = content.1.description().unwrap_or("");
        assert!(
            desc.contains("of 2"),
            "host count must be included: {}",
            desc
        );
    }

    /// dispatch_settings_action: Click on an Ssh entry widget selects it and
    /// hands the focus counter back to the list (via the shared router).
    #[test]
    fn dispatch_settings_ssh_entry_click_selects() {
        use crate::renderer::overlay::widgets::settings_ssh::{SSH_CATEGORY, row};
        let mut panel = make_ssh_panel_with_2_hosts();
        panel.ssh_field_focus = 4;

        let handled = dispatch_settings_action(
            &mut panel,
            accesskit::Action::Click,
            &NodeIdKind::SettingsWidget {
                category: SSH_CATEGORY,
                index: row::LIST_BASE + 1,
            },
            None,
        );
        assert!(handled);
        assert_eq!(panel.selected_host_index, 1);
        assert_eq!(panel.ssh_field_focus, 0);
    }

    /// dispatch_settings_action: Focus on an Ssh field widget moves the
    /// counter, and SetValue on the port writes through the router.
    #[test]
    fn dispatch_settings_ssh_field_focus_and_set_value() {
        use crate::renderer::overlay::widgets::settings_ssh::{SSH_CATEGORY, row};
        let mut panel = make_ssh_panel_with_2_hosts();

        let handled = dispatch_settings_action(
            &mut panel,
            accesskit::Action::Focus,
            &NodeIdKind::SettingsWidget {
                category: SSH_CATEGORY,
                index: row::FIELD_USERNAME,
            },
            None,
        );
        assert!(handled);
        assert_eq!(panel.ssh_field_focus, 4);

        let handled = dispatch_settings_action(
            &mut panel,
            accesskit::Action::SetValue,
            &NodeIdKind::SettingsWidget {
                category: SSH_CATEGORY,
                index: row::FIELD_PORT,
            },
            Some(accesskit::ActionData::NumericValue(8022.0)),
        );
        assert!(handled);
        assert_eq!(panel.ssh_hosts[0].port, 8022);
    }

    /// tree_state_hash changes when selected_host_index changes.
    #[test]
    fn tree_state_hash_detects_selected_host_index_change() {
        use crate::settings_panel::SshHostEntry;
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.ssh_hosts = vec![
            SshHostEntry {
                name: "a".to_string(),
                host: "a.example.com".to_string(),
                port: 22,
                username: "u".to_string(),
                auth_type: "key".to_string(),
            },
            SshHostEntry {
                name: "b".to_string(),
                host: "b.example.com".to_string(),
                port: 22,
                username: "u".to_string(),
                auth_type: "key".to_string(),
            },
        ];
        state.settings_panel.selected_host_index = 0;
        let h0 = compute_tree_state_hash(&state);

        state.settings_panel.selected_host_index = 1;
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(
            h0, h1,
            "the hash must change when selected_host_index changes"
        );
    }

    /// tree_state_hash changes when the ssh_hosts list changes.
    #[test]
    fn tree_state_hash_detects_ssh_hosts_change() {
        use crate::settings_panel::SshHostEntry;
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel.is_open = true;
        state.settings_panel.ssh_hosts = vec![];
        let h0 = compute_tree_state_hash(&state);

        state.settings_panel.ssh_hosts = vec![SshHostEntry {
            name: "added".to_string(),
            host: "new.example.com".to_string(),
            port: 22,
            username: "u".to_string(),
            auth_type: "key".to_string(),
        }];
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(
            h0, h1,
            "the hash must change when an ssh_hosts entry is added"
        );
    }

    // ===== Phase 5-11-8 Step 8-2: SSH host field editing =====

    /// Test helper: build a SettingsPanel with 2 hosts.
    fn make_ssh_panel_with_2_hosts() -> SettingsPanel {
        use crate::settings_panel::{SettingsCategory, SshHostEntry};
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Ssh;
        panel.ssh_hosts = vec![
            SshHostEntry {
                name: "prod".to_string(),
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                auth_type: "key".to_string(),
            },
            SshHostEntry {
                name: "stg".to_string(),
                host: "stg.example.com".to_string(),
                port: 2222,
                username: "alice".to_string(),
                auth_type: "password".to_string(),
            },
        ];
        panel.selected_host_index = 0;
        panel.ssh_field_focus = 0;
        panel
    }

    /// The delete-dialog NodeIds (the Ssh ids that stayed hand-written) and
    /// the title-bar window buttons decode; 40..=46 were retired with the
    /// Ssh widget migration and now decode to Unknown.
    #[test]
    fn settings_ssh_field_node_ids_decode() {
        for retired in 40u64..=46 {
            assert_eq!(
                decode_node_id(NodeId(retired)),
                NodeIdKind::Unknown,
                "retired fixed id {retired} must no longer alias a control"
            );
        }
        assert_eq!(
            decode_node_id(SETTINGS_SSH_DELETE_DIALOG_ID),
            NodeIdKind::SettingsSshDeleteDialog
        );
        assert_eq!(
            decode_node_id(SETTINGS_SSH_DELETE_CONFIRM_BTN_ID),
            NodeIdKind::SettingsSshDeleteConfirmBtn
        );
        assert_eq!(
            decode_node_id(SETTINGS_SSH_DELETE_CANCEL_BTN_ID),
            NodeIdKind::SettingsSshDeleteCancelBtn
        );
        // NodeId(57..=59) belong to the custom title bar window buttons.
        assert_eq!(
            decode_node_id(WINDOW_MINIMIZE_BTN_ID),
            NodeIdKind::WindowMinimizeButton
        );
        assert_eq!(
            decode_node_id(WINDOW_MAXIMIZE_BTN_ID),
            NodeIdKind::WindowMaximizeButton
        );
        assert_eq!(
            decode_node_id(WINDOW_CLOSE_BTN_ID),
            NodeIdKind::WindowCloseButton
        );
    }

    /// build_tree exposes the 5 fields of the selected host.
    #[test]
    fn build_settings_panel_ssh_exposes_5_field_nodes() {
        let panel = make_ssh_panel_with_2_hosts();
        let (nodes, _focus) = build_settings_panel_nodes(&panel);

        let find = |id: NodeId| {
            nodes
                .iter()
                .find(|(node_id, _)| *node_id == id)
                .map(|(_, n)| n)
        };

        use crate::renderer::overlay::widgets::settings_ssh::row;

        // name (TextInput)
        let n = find(ssh_widget_id(row::FIELD_NAME)).expect("name node must exist");
        assert_eq!(n.role(), Role::TextInput);
        assert_eq!(n.value().unwrap_or(""), "prod");

        // host (TextInput)
        let h = find(ssh_widget_id(row::FIELD_HOST)).expect("host node must exist");
        assert_eq!(h.role(), Role::TextInput);
        assert_eq!(h.value().unwrap_or(""), "prod.example.com");

        // port (SpinButton)
        let p = find(ssh_widget_id(row::FIELD_PORT)).expect("port node must exist");
        assert_eq!(p.role(), Role::SpinButton);
        assert_eq!(p.numeric_value(), Some(22.0));
        assert_eq!(p.min_numeric_value(), Some(1.0));
        assert_eq!(p.max_numeric_value(), Some(65535.0));

        // username (TextInput)
        let u = find(ssh_widget_id(row::FIELD_USERNAME)).expect("username node must exist");
        assert_eq!(u.role(), Role::TextInput);
        assert_eq!(u.value().unwrap_or(""), "deploy");

        // auth_type (ComboBox)
        let a = find(ssh_widget_id(row::FIELD_AUTH)).expect("auth_type node must exist");
        assert_eq!(a.role(), Role::ComboBox);
        assert_eq!(a.value().unwrap_or(""), "key");

        // Add / Delete buttons (previously fixed ids 45/46).
        let add = find(ssh_widget_id(row::ADD)).expect("add button must exist");
        assert_eq!(add.role(), Role::Button);
        let del = find(ssh_widget_id(row::DELETE)).expect("delete button must exist");
        assert_eq!(del.role(), Role::Button);
        assert!(!del.is_disabled(), "delete is enabled while hosts exist");
    }

    /// With an empty host list, the field nodes are not exposed (only the
    /// Add/Delete buttons are, Delete reporting disabled).
    #[test]
    fn build_settings_panel_ssh_no_fields_when_empty() {
        use crate::renderer::overlay::widgets::settings_ssh::row;
        use crate::settings_panel::SettingsCategory;
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Ssh;
        panel.ssh_hosts = vec![];

        let (nodes, _focus) = build_settings_panel_nodes(&panel);
        let has_field = nodes.iter().any(|(id, _)| {
            *id == ssh_widget_id(row::FIELD_NAME)
                || *id == ssh_widget_id(row::FIELD_HOST)
                || *id == ssh_widget_id(row::FIELD_PORT)
        });
        assert!(
            !has_field,
            "no field nodes must be exposed for an empty list"
        );
        let del = nodes
            .iter()
            .find(|(id, _)| *id == ssh_widget_id(row::DELETE))
            .expect("delete button stays exposed");
        assert!(del.1.is_disabled(), "delete reports disabled when empty");
    }

    /// When ssh_field_focus is name, focus moves to the name node.
    #[test]
    fn build_settings_panel_ssh_focus_follows_ssh_field_focus() {
        use crate::renderer::overlay::widgets::settings_ssh::row;
        let mut panel = make_ssh_panel_with_2_hosts();
        panel.ssh_field_focus = 1; // name
        let (_nodes, focus) = build_settings_panel_nodes(&panel);
        assert_eq!(focus, ssh_widget_id(row::FIELD_NAME));

        panel.ssh_field_focus = 3; // port
        let (_nodes, focus) = build_settings_panel_nodes(&panel);
        assert_eq!(focus, ssh_widget_id(row::FIELD_PORT));

        panel.ssh_field_focus = 5; // auth_type
        let (_nodes, focus) = build_settings_panel_nodes(&panel);
        assert_eq!(focus, ssh_widget_id(row::FIELD_AUTH));

        panel.ssh_field_focus = 7; // delete button
        let (_nodes, focus) = build_settings_panel_nodes(&panel);
        assert_eq!(focus, ssh_widget_id(row::DELETE));

        panel.ssh_field_focus = 0; // back to list
        let (_nodes, focus) = build_settings_panel_nodes(&panel);
        assert_eq!(focus, ssh_entry_widget_id(0));
    }

    /// SshHostEntry mutation API: set_ssh_host_port_value clamps.
    #[test]
    fn set_ssh_host_port_value_clamps() {
        let mut panel = make_ssh_panel_with_2_hosts();
        panel.set_ssh_host_port_value(8080.4);
        assert_eq!(panel.ssh_hosts[0].port, 8080);

        panel.set_ssh_host_port_value(-100.0);
        assert_eq!(panel.ssh_hosts[0].port, 1);

        panel.set_ssh_host_port_value(99999.0);
        assert_eq!(panel.ssh_hosts[0].port, 65535);
    }

    /// SSH_AUTH_TYPES cycling: recover at the head even from an unknown value.
    #[test]
    fn next_ssh_auth_type_from_unknown() {
        let mut panel = make_ssh_panel_with_2_hosts();
        panel.ssh_hosts[0].auth_type = "unknown".to_string();
        panel.next_ssh_auth_type();
        // "unknown" has position=None, so current=0 (=password) → next is key.
        assert_eq!(panel.ssh_hosts[0].auth_type, "key");
    }

    /// write_ssh_hosts_back: in-place updates preserve unmanaged fields.
    #[test]
    fn write_ssh_hosts_back_preserves_unknown_fields() {
        use crate::settings_panel::{SshHostEntry, write_ssh_hosts_back};

        // Existing TOML has name + key_path.
        let existing = r#"
[[hosts]]
name = "old_name"
host = "old.example.com"
port = 22
username = "olduser"
auth_type = "key"
key_path = "/home/me/.ssh/id_rsa"
forward_local = ["8080:localhost:80"]
"#;
        let mut doc: toml_edit::DocumentMut = existing.parse().unwrap();
        let new_hosts = vec![SshHostEntry {
            name: "new_name".to_string(),
            host: "new.example.com".to_string(),
            port: 2222,
            username: "newuser".to_string(),
            auth_type: "agent".to_string(),
        }];
        write_ssh_hosts_back(&mut doc, &new_hosts);

        let out = doc.to_string();
        // Managed fields are updated.
        assert!(out.contains("name = \"new_name\""), "name updated");
        assert!(out.contains("host = \"new.example.com\""), "host updated");
        assert!(out.contains("port = 2222"), "port updated");
        assert!(out.contains("username = \"newuser\""), "username updated");
        assert!(out.contains("auth_type = \"agent\""), "auth_type updated");
        // Unmanaged fields are preserved.
        assert!(
            out.contains("key_path = \"/home/me/.ssh/id_rsa\""),
            "key_path preserved: {}",
            out
        );
        assert!(
            out.contains("forward_local"),
            "forward_local preserved: {}",
            out
        );
    }

    /// write_ssh_hosts_back: can create a new [[hosts]] array even if it is missing.
    #[test]
    fn write_ssh_hosts_back_creates_new_array() {
        use crate::settings_panel::{SshHostEntry, write_ssh_hosts_back};
        let mut doc: toml_edit::DocumentMut = "".parse().unwrap();
        let hosts = vec![SshHostEntry {
            name: "first".to_string(),
            host: "h.example.com".to_string(),
            port: 22,
            username: "u".to_string(),
            auth_type: "key".to_string(),
        }];
        write_ssh_hosts_back(&mut doc, &hosts);

        let out = doc.to_string();
        assert!(out.contains("name = \"first\""), "newly added: {}", out);
    }

    /// write_ssh_hosts_back: can empty the existing array even when hosts is empty (for Step 8-3).
    #[test]
    fn write_ssh_hosts_back_truncates_existing() {
        use crate::settings_panel::write_ssh_hosts_back;
        let existing = r#"
[[hosts]]
name = "a"
host = "a"
port = 22
username = "u"
auth_type = "key"

[[hosts]]
name = "b"
host = "b"
port = 22
username = "u"
auth_type = "key"
"#;
        let mut doc: toml_edit::DocumentMut = existing.parse().unwrap();
        write_ssh_hosts_back(&mut doc, &[]);
        // The array is empty.
        let arr = doc
            .get("hosts")
            .and_then(|i| i.as_array_of_tables())
            .expect("hosts array still present");
        assert_eq!(arr.len(), 0);
    }

    /// compute_tree_state_hash detects ssh_field_focus changes.
    #[test]
    fn tree_state_hash_detects_ssh_field_focus_change() {
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel = make_ssh_panel_with_2_hosts();
        state.settings_panel.is_open = true;
        state.settings_panel.ssh_field_focus = 0;
        let h0 = compute_tree_state_hash(&state);

        state.settings_panel.ssh_field_focus = 3;
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "the hash must change when ssh_field_focus changes");
    }

    // ============================================================
    // Sprint 5-11-8 Step 8-3 Sub-phase E: Add / Delete + dialog dispatch tests
    // ============================================================

    /// SettingsSshDeleteConfirmBtn Click performs the deletion and closes the dialog.
    #[test]
    fn dispatch_settings_ssh_delete_confirm_btn_click_deletes_host() {
        let mut panel = make_ssh_panel_with_2_hosts();
        panel.open_ssh_delete_dialog();
        assert!(panel.ssh_delete_dialog_open);

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsSshDeleteConfirmBtn,
            None,
        );

        assert!(handled);
        assert_eq!(panel.ssh_hosts.len(), 1, "one host has been deleted");
        assert!(!panel.ssh_delete_dialog_open, "the dialog is closed");
        assert!(panel.dirty);
    }

    /// SettingsSshDeleteCancelBtn Click closes the dialog without deleting.
    #[test]
    fn dispatch_settings_ssh_delete_cancel_btn_click_closes_dialog() {
        let mut panel = make_ssh_panel_with_2_hosts();
        panel.open_ssh_delete_dialog();
        panel.ssh_delete_dialog_confirm_focused = true; // either value is fine

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsSshDeleteCancelBtn,
            None,
        );

        assert!(handled);
        assert!(!panel.ssh_delete_dialog_open);
        assert!(!panel.ssh_delete_dialog_confirm_focused);
        assert_eq!(panel.ssh_hosts.len(), 2, "no deletion happens");
    }

    /// Confirm / Cancel button Focus only toggles the focus flag (no delete).
    #[test]
    fn dispatch_settings_ssh_delete_dialog_focus_toggles_flag() {
        let mut panel = make_ssh_panel_with_2_hosts();
        panel.open_ssh_delete_dialog();
        // Initial state: Cancel focused.
        assert!(!panel.ssh_delete_dialog_confirm_focused);

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Focus,
            &NodeIdKind::SettingsSshDeleteConfirmBtn,
            None,
        );
        assert!(handled);
        assert!(
            panel.ssh_delete_dialog_confirm_focused,
            "Confirm Focus must raise the flag"
        );
        assert!(panel.ssh_delete_dialog_open, "the dialog stays open");

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Focus,
            &NodeIdKind::SettingsSshDeleteCancelBtn,
            None,
        );
        assert!(handled);
        assert!(
            !panel.ssh_delete_dialog_confirm_focused,
            "Cancel Focus must clear the flag"
        );
    }

    /// compute_tree_state_hash detects ssh_delete_dialog_open / confirm_focused changes.
    #[test]
    fn tree_state_hash_detects_ssh_delete_dialog_changes() {
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel = make_ssh_panel_with_2_hosts();
        state.settings_panel.is_open = true;
        let h0 = compute_tree_state_hash(&state);

        // Open the dialog.
        state.settings_panel.open_ssh_delete_dialog();
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1, "the hash must change when the dialog opens");

        // Move focus to Confirm.
        state.settings_panel.ssh_delete_dialog_confirm_focused = true;
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(h1, h2, "the hash must change when confirm_focused changes");
    }

    // ===================================================================
    // Phase 5-11-9 Sub-phase E: Keybindings AccessKit tests
    // ===================================================================

    /// Test factory: SettingsPanel preloaded with 2 keybindings on the Keybindings category.
    fn make_key_panel_with_2_bindings() -> SettingsPanel {
        use crate::settings_panel::{KEYBINDING_ACTIONS, KeyBindingEntry, SettingsCategory};
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Keybindings;
        panel.keybindings = vec![
            KeyBindingEntry {
                key: "ctrl+shift+p".to_string(),
                action: KEYBINDING_ACTIONS[0].to_string(),
            },
            KeyBindingEntry {
                key: "ctrl+b d".to_string(),
                action: KEYBINDING_ACTIONS[1].to_string(),
            },
        ];
        panel.selected_key_index = 0;
        panel.key_field_focus = 0;
        panel
    }

    // ----- decode tests (7) -----

    #[test]
    fn settings_keybinding_dynamic_node_id_decode() {
        // Boundary at offset 0.
        assert_eq!(
            decode_node_id(settings_key_binding_item_id(0)),
            NodeIdKind::SettingsKeyBindingItem { idx: 0 }
        );
        // Mid-range value.
        assert_eq!(
            decode_node_id(settings_key_binding_item_id(42)),
            NodeIdKind::SettingsKeyBindingItem { idx: 42 }
        );
    }

    #[test]
    fn settings_keybinding_dynamic_node_id_roundtrip() {
        for &idx in &[0usize, 1, 9, 99, 999, 12_345] {
            let id = settings_key_binding_item_id(idx);
            assert_eq!(
                decode_node_id(id),
                NodeIdKind::SettingsKeyBindingItem { idx }
            );
        }
    }

    #[test]
    fn settings_key_field_key_id_decodes() {
        assert_eq!(
            decode_node_id(SETTINGS_KEY_FIELD_KEY_ID),
            NodeIdKind::SettingsKeyFieldKey
        );
    }

    #[test]
    fn settings_key_field_action_id_decodes() {
        assert_eq!(
            decode_node_id(SETTINGS_KEY_FIELD_ACTION_ID),
            NodeIdKind::SettingsKeyFieldAction
        );
    }

    #[test]
    fn settings_key_add_btn_id_decodes() {
        assert_eq!(
            decode_node_id(SETTINGS_KEY_ADD_BTN_ID),
            NodeIdKind::SettingsKeyAddBtn
        );
    }

    #[test]
    fn settings_key_delete_btn_id_decodes() {
        assert_eq!(
            decode_node_id(SETTINGS_KEY_DELETE_BTN_ID),
            NodeIdKind::SettingsKeyDeleteBtn
        );
    }

    #[test]
    fn settings_key_delete_dialog_ids_decode() {
        assert_eq!(
            decode_node_id(SETTINGS_KEY_DELETE_DIALOG_ID),
            NodeIdKind::SettingsKeyDeleteDialog
        );
        assert_eq!(
            decode_node_id(SETTINGS_KEY_DELETE_CONFIRM_BTN_ID),
            NodeIdKind::SettingsKeyDeleteConfirmBtn
        );
        assert_eq!(
            decode_node_id(SETTINGS_KEY_DELETE_CANCEL_BTN_ID),
            NodeIdKind::SettingsKeyDeleteCancelBtn
        );
    }

    // ----- dispatch tests (11) -----

    /// ListBoxOption Click updates selected_key_index and resets focus to the list.
    #[test]
    fn dispatch_settings_keybinding_item_click_updates_selection() {
        let mut panel = make_key_panel_with_2_bindings();
        panel.key_field_focus = 2; // pretend Action field was focused

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyBindingItem { idx: 1 },
            None,
        );

        assert!(handled);
        assert_eq!(panel.selected_key_index, 1);
        assert_eq!(panel.key_field_focus, 0, "focus resets to the list");
    }

    /// ListBoxOption Click is rejected when idx is out of range.
    #[test]
    fn dispatch_settings_keybinding_item_click_out_of_range_rejected() {
        let mut panel = make_key_panel_with_2_bindings();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyBindingItem { idx: 99 },
            None,
        );
        assert!(!handled, "out-of-range idx must be rejected");
        assert_eq!(panel.selected_key_index, 0);
    }

    /// Key field Click enters Record mode (Q1 = (c) branch a).
    #[test]
    fn dispatch_settings_key_field_key_click_enters_record_mode() {
        let mut panel = make_key_panel_with_2_bindings();
        assert!(!panel.is_key_recording());

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyFieldKey,
            None,
        );

        assert!(handled);
        assert_eq!(panel.key_field_focus, 1);
        assert!(panel.is_key_recording(), "Click must start Record mode");
    }

    /// Key field SetValue directly overwrites without entering edit mode (Q1 = (c) branch b).
    #[test]
    fn dispatch_settings_key_field_key_set_value_writes_directly() {
        let mut panel = make_key_panel_with_2_bindings();

        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsKeyFieldKey,
            Some(ActionData::Value("ctrl+alt+x".into())),
        );

        assert!(handled);
        assert_eq!(panel.keybindings[0].key, "ctrl+alt+x");
        assert!(
            !panel.is_key_recording(),
            "SetValue must not enter Record mode"
        );
        assert_eq!(panel.key_field_focus, 1);
        assert!(panel.dirty);
    }

    /// Key field SetValue with non-Value data is rejected.
    #[test]
    fn dispatch_settings_key_field_key_set_value_rejects_non_value_data() {
        let mut panel = make_key_panel_with_2_bindings();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsKeyFieldKey,
            None,
        );
        assert!(!handled);
    }

    /// Action field Click cycles forward.
    #[test]
    fn dispatch_settings_key_field_action_click_cycles_forward() {
        use crate::settings_panel::KEYBINDING_ACTIONS;
        let mut panel = make_key_panel_with_2_bindings();
        let original = panel.keybindings[0].action.clone();
        let pos = KEYBINDING_ACTIONS
            .iter()
            .position(|&a| a == original)
            .expect("seed action is in the list");
        let expected = KEYBINDING_ACTIONS[(pos + 1) % KEYBINDING_ACTIONS.len()];

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyFieldAction,
            None,
        );

        assert!(handled);
        assert_eq!(panel.keybindings[0].action, expected);
        assert_eq!(panel.key_field_focus, 2);
    }

    /// Action field Decrement cycles backward.
    #[test]
    fn dispatch_settings_key_field_action_decrement_cycles_backward() {
        use crate::settings_panel::KEYBINDING_ACTIONS;
        let mut panel = make_key_panel_with_2_bindings();
        let original = panel.keybindings[0].action.clone();
        let pos = KEYBINDING_ACTIONS
            .iter()
            .position(|&a| a == original)
            .expect("seed action is in the list");
        let expected =
            KEYBINDING_ACTIONS[(pos + KEYBINDING_ACTIONS.len() - 1) % KEYBINDING_ACTIONS.len()];

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Decrement,
            &NodeIdKind::SettingsKeyFieldAction,
            None,
        );

        assert!(handled);
        assert_eq!(panel.keybindings[0].action, expected);
    }

    /// Action field SetValue with a known string writes directly.
    #[test]
    fn dispatch_settings_key_field_action_set_value_writes_known_string() {
        use crate::settings_panel::KEYBINDING_ACTIONS;
        let mut panel = make_key_panel_with_2_bindings();
        let target = KEYBINDING_ACTIONS[3];

        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsKeyFieldAction,
            Some(ActionData::Value(target.into())),
        );

        assert!(handled);
        assert_eq!(panel.keybindings[0].action, target);
        assert_eq!(panel.key_field_focus, 2);
    }

    /// Action field SetValue with an unknown value returns handled=false (helper rejects).
    #[test]
    fn dispatch_settings_key_field_action_set_value_rejects_unknown() {
        let mut panel = make_key_panel_with_2_bindings();
        let original = panel.keybindings[0].action.clone();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsKeyFieldAction,
            Some(ActionData::Value("not_a_real_action".into())),
        );
        assert!(!handled, "unknown action strings must be rejected");
        assert_eq!(
            panel.keybindings[0].action, original,
            "value must be untouched"
        );
    }

    /// Add button Click appends a new binding and starts Record mode.
    #[test]
    fn dispatch_settings_key_add_btn_click_appends_and_records() {
        let mut panel = make_key_panel_with_2_bindings();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyAddBtn,
            None,
        );

        assert!(handled);
        assert_eq!(panel.keybindings.len(), 3);
        assert_eq!(panel.selected_key_index, 2, "tail is selected");
        assert_eq!(panel.key_field_focus, 1, "key field is focused");
        assert!(panel.is_key_recording(), "Record mode is active");
        assert!(panel.dirty);
    }

    /// Delete button Click opens the confirmation dialog (Cancel default).
    #[test]
    fn dispatch_settings_key_delete_btn_click_opens_dialog() {
        let mut panel = make_key_panel_with_2_bindings();
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyDeleteBtn,
            None,
        );
        assert!(handled);
        assert!(panel.key_delete_dialog_open);
        assert!(
            !panel.key_delete_dialog_confirm_focused,
            "Cancel is focused by default"
        );
        assert_eq!(panel.keybindings.len(), 2, "no deletion happens yet");
    }

    /// Confirm Click deletes and closes the dialog.
    #[test]
    fn dispatch_settings_key_delete_confirm_click_deletes() {
        let mut panel = make_key_panel_with_2_bindings();
        panel.open_key_delete_dialog();

        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyDeleteConfirmBtn,
            None,
        );

        assert!(handled);
        assert_eq!(panel.keybindings.len(), 1);
        assert!(!panel.key_delete_dialog_open);
        assert!(panel.dirty);
    }

    /// Cancel Click closes the dialog without deleting; dialog focus toggles work.
    #[test]
    fn dispatch_settings_key_delete_cancel_and_focus_toggle() {
        let mut panel = make_key_panel_with_2_bindings();
        panel.open_key_delete_dialog();

        // Confirm focus.
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Focus,
            &NodeIdKind::SettingsKeyDeleteConfirmBtn,
            None,
        );
        assert!(handled);
        assert!(panel.key_delete_dialog_confirm_focused);

        // Cancel focus.
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Focus,
            &NodeIdKind::SettingsKeyDeleteCancelBtn,
            None,
        );
        assert!(handled);
        assert!(!panel.key_delete_dialog_confirm_focused);

        // Cancel click closes the dialog.
        let handled = dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyDeleteCancelBtn,
            None,
        );
        assert!(handled);
        assert!(!panel.key_delete_dialog_open);
        assert_eq!(panel.keybindings.len(), 2, "no deletion happens");
    }

    // ----- build_tree tests (4) -----

    /// build_settings_panel_nodes exposes the keybinding list as ListBox children
    /// plus the key/action fields and Add/Delete buttons.
    #[test]
    fn build_settings_panel_nodes_exposes_keybindings_nodes() {
        let mut panel = make_key_panel_with_2_bindings();
        panel.is_open = true;
        let (nodes, _) = build_settings_panel_nodes(&panel);

        // Each binding must have a ListBoxOption.
        for idx in 0..2 {
            let id = settings_key_binding_item_id(idx);
            assert!(
                nodes.iter().any(|(nid, _)| *nid == id),
                "expected ListBoxOption for idx {}",
                idx
            );
        }
        // Field nodes + buttons must exist.
        for id in [
            SETTINGS_KEY_FIELD_KEY_ID,
            SETTINGS_KEY_FIELD_ACTION_ID,
            SETTINGS_KEY_ADD_BTN_ID,
            SETTINGS_KEY_DELETE_BTN_ID,
        ] {
            assert!(
                nodes.iter().any(|(nid, _)| *nid == id),
                "expected node {:?}",
                id
            );
        }
    }

    /// Empty list still exposes Add (and Delete in disabled label form).
    #[test]
    fn build_settings_panel_nodes_empty_keybindings_still_show_add() {
        use crate::settings_panel::SettingsCategory;
        let mut panel = SettingsPanel::default();
        panel.category = SettingsCategory::Keybindings;
        panel.is_open = true;
        // Clear the built-in defaults to simulate an empty list.
        panel.keybindings.clear();
        panel.selected_key_index = 0;
        panel.key_field_focus = 0;

        let (nodes, focus) = build_settings_panel_nodes(&panel);
        assert!(
            nodes.iter().any(|(nid, _)| *nid == SETTINGS_KEY_ADD_BTN_ID),
            "Add button must always be exposed"
        );
        assert!(
            nodes
                .iter()
                .any(|(nid, _)| *nid == SETTINGS_KEY_DELETE_BTN_ID),
            "Delete button must be exposed in disabled form"
        );
        // No ListBoxOption is created when the list is empty.
        assert!(
            !nodes
                .iter()
                .any(|(nid, _)| *nid == settings_key_binding_item_id(0))
        );
        // Default focus when nothing is set falls back to the category tab.
        assert!(matches!(
            decode_node_id(focus),
            NodeIdKind::SettingsTab { .. }
        ));
    }

    /// While the delete dialog is open, focus moves to the Cancel button by default.
    #[test]
    fn build_settings_panel_nodes_dialog_focus_defaults_cancel() {
        let mut panel = make_key_panel_with_2_bindings();
        panel.is_open = true;
        panel.open_key_delete_dialog();
        let (nodes, focus) = build_settings_panel_nodes(&panel);

        // The dialog body + buttons must be in the tree.
        for id in [
            SETTINGS_KEY_DELETE_DIALOG_ID,
            SETTINGS_KEY_DELETE_CONFIRM_BTN_ID,
            SETTINGS_KEY_DELETE_CANCEL_BTN_ID,
        ] {
            assert!(
                nodes.iter().any(|(nid, _)| *nid == id),
                "expected dialog node {:?}",
                id
            );
        }
        // Focus is on Cancel by default.
        assert_eq!(focus, SETTINGS_KEY_DELETE_CANCEL_BTN_ID);

        // After toggling confirm focus, the focus moves to Confirm.
        panel.key_delete_dialog_confirm_focused = true;
        let (_, focus2) = build_settings_panel_nodes(&panel);
        assert_eq!(focus2, SETTINGS_KEY_DELETE_CONFIRM_BTN_ID);
    }

    /// key_field_focus chooses the right focus target.
    #[test]
    fn build_settings_panel_nodes_focus_follows_key_field_focus() {
        let mut panel = make_key_panel_with_2_bindings();
        panel.is_open = true;

        for (focus_val, expected) in [
            (0u8, settings_key_binding_item_id(0)),
            (1, SETTINGS_KEY_FIELD_KEY_ID),
            (2, SETTINGS_KEY_FIELD_ACTION_ID),
            (3, SETTINGS_KEY_ADD_BTN_ID),
            (4, SETTINGS_KEY_DELETE_BTN_ID),
        ] {
            panel.key_field_focus = focus_val;
            let (_, focus) = build_settings_panel_nodes(&panel);
            assert_eq!(
                focus, expected,
                "key_field_focus={} should focus {:?}",
                focus_val, expected
            );
        }
    }

    // ----- hash test (1) -----

    /// compute_tree_state_hash reflects changes across the Keybindings category fields.
    #[test]
    fn tree_state_hash_detects_keybindings_changes() {
        use crate::settings_panel::KEYBINDING_ACTIONS;
        let mut state = ClientState::new(80, 24, 1000);
        state.settings_panel = make_key_panel_with_2_bindings();
        state.settings_panel.is_open = true;

        let h0 = compute_tree_state_hash(&state);

        // 1. Change key_field_focus.
        state.settings_panel.key_field_focus = 2;
        let h1 = compute_tree_state_hash(&state);
        assert_ne!(h0, h1);

        // 2. Change selected_key_index.
        state.settings_panel.selected_key_index = 1;
        let h2 = compute_tree_state_hash(&state);
        assert_ne!(h1, h2);

        // 3. Rewrite a binding's key.
        state.settings_panel.keybindings[1].key = "f5".to_string();
        let h3 = compute_tree_state_hash(&state);
        assert_ne!(h2, h3);

        // 4. Rewrite a binding's action.
        state.settings_panel.keybindings[1].action = KEYBINDING_ACTIONS[5].to_string();
        let h4 = compute_tree_state_hash(&state);
        assert_ne!(h3, h4);

        // 5. Open the delete dialog.
        state.settings_panel.open_key_delete_dialog();
        let h5 = compute_tree_state_hash(&state);
        assert_ne!(h4, h5);

        // 6. Toggle confirm focus inside the dialog.
        state.settings_panel.key_delete_dialog_confirm_focused = true;
        let h6 = compute_tree_state_hash(&state);
        assert_ne!(h5, h6);

        // 7. Record mode change.
        state.settings_panel.cancel_key_delete_dialog();
        let h7 = compute_tree_state_hash(&state);
        state.settings_panel.begin_key_record();
        let h8 = compute_tree_state_hash(&state);
        assert_ne!(h7, h8);
    }

    // ----- sanity (2) -----

    /// The Keybinding dynamic offset must not collide with the widget range
    /// or other adjacent overlay ranges.
    #[test]
    fn settings_keybinding_offset_does_not_overlap_neighbors() {
        // Keybinding range is 900M..1G. Earlier live ranges end at 800M (the
        // widget slot); 800M..900M is retired since the Ssh widget migration.
        const _: () =
            assert!(NODE_ID_SETTINGS_KEY_BINDING_OFFSET >= SETTINGS_WIDGET_BASE + 200_000_000);
        // Tab/pane offsets are at 1e9 and 1e10 -> still safely above the key binding range
        // (the key binding offset spans 100M, matching the local DYN_RANGE in decode_dynamic).
        const _: () =
            assert!(NODE_ID_TAB_OFFSET >= NODE_ID_SETTINGS_KEY_BINDING_OFFSET + 100_000_000);
        // No collision with the fixed ID range (1..=99).
        assert_ne!(
            settings_key_binding_item_id(0),
            NodeId(SETTINGS_KEY_FIELD_KEY_ID.0)
        );
    }

    /// Q1 = (c) regression: Click followed by SetValue still ends in a clean
    /// non-recording state with the SetValue payload applied.
    #[test]
    fn keybinding_key_click_then_set_value_lands_clean() {
        let mut panel = make_key_panel_with_2_bindings();

        // 1. Click enters Record mode.
        dispatch_settings_action(
            &mut panel,
            Action::Click,
            &NodeIdKind::SettingsKeyFieldKey,
            None,
        );
        assert!(panel.is_key_recording());

        // 2. SetValue overrides directly; Record mode is cancelled by the helper.
        dispatch_settings_action(
            &mut panel,
            Action::SetValue,
            &NodeIdKind::SettingsKeyFieldKey,
            Some(ActionData::Value("f12".into())),
        );

        assert!(
            !panel.is_key_recording(),
            "SetValue helper must reset edit mode"
        );
        assert_eq!(panel.keybindings[0].key, "f12");
        assert_eq!(panel.key_field_focus, 1);
    }
}
