//! Client state — manages the grid, scrollback, palette, and search together
//!
//! Layout established when the file was split in Sprint 5-6:
//! - `pane` — `PaneState` / `PlacedImage` / `FloatRect`
//! - `search` — `SearchState` and the incremental search methods on `ClientState`
//! - `selection` — `DetectedUrl` / `MouseSelection` / `CopyModeState`
//! - `menus` — `ContextMenu*` / `FileTransferDialog` / `QuickSelect*`
//! - `consent` — `ConsentDialog` / `ConsentKind` / `SessionConsentOverrides`
//! - `server_message` — `apply_server_message` and the scroll / jump-to-prompt methods + tests
//!
//! All types previously exposed by the old `state.rs` are re-exported from this
//! module via `pub use`, so external references of the form `crate::state::Foo`
//! do not need to change.

use std::collections::HashMap;

use nexterm_proto::PaneLayout;
use winit::window::WindowId;

use crate::host_manager::HostManager;
use crate::macro_picker::MacroPicker;
use crate::palette::CommandPalette;
use crate::settings_panel::SettingsPanel;

mod blocks;
mod consent;
mod menus;
mod pane;
mod search;
mod selection;
mod server_message;

pub use consent::{ConsentDialog, ConsentKind, SessionConsentOverrides};
// `ContextMenuItem` / `QuickSelectMatch` / `DetectedUrl` are not currently
// referenced directly from elsewhere in the crate, but they form part of the
// public API as the return types of `ContextMenu` / `QuickSelectState` /
// `detect_urls_in_row`, so we keep them re-exported.
#[allow(unused_imports)]
pub use menus::{
    ContextMenu, ContextMenuAction, ContextMenuItem, FileTransferDialog, QuickSelectMatch,
    QuickSelectState,
};
pub use pane::{FloatRect, PaneState, PlacedImage};
pub use search::SearchState;
#[allow(unused_imports)]
pub use selection::{CopyModeState, DetectedUrl, MouseSelection, ViMode, detect_urls_in_row};

/// Alert entry surfaced to screen readers (Sprint 5-11-5 / Phase 5-11-5).
///
/// Data holder that exposes `Bell` (VT BEL `0x07`) / `OSC 9` (iTerm2-compatible
/// notifications) / `OSC 777` (urxvt-compatible notifications) as AccessKit
/// `Role::Alert` nodes.
///
/// **Lifecycle**:
/// - Pushed to the `alerts` queue by `ClientState::add_alert` when the server
///   sends `ServerToClient::Bell` / `ServerToClient::DesktopNotification`.
/// - Stale entries are removed at the top of
///   `update_accesskit_tree_if_needed` via `expire_alerts`.
/// - Once the queue exceeds `ALERTS_MAX_LEN`, the oldest entries are dropped.
///
/// **NodeId**: `accessibility::alert_node_id(seq) = NODE_ID_ALERT_OFFSET + seq`.
/// `seq` is a monotonically increasing counter (`u64`) per client process and
/// therefore collision-free.
#[derive(Debug, Clone)]
pub struct AlertEntry {
    /// Monotonically increasing sequence number (used to compute the NodeId)
    pub seq: u64,
    /// Alert kind
    pub kind: AlertKind,
    /// Originating pane ID (kept for future "notification from pane X" labels and source filtering)
    #[allow(dead_code)]
    pub pane_id: u32,
    /// Title (OSC 9 arrives from the server as "Nexterm", OSC 777 uses the server-provided title, Bell is localized)
    pub title: String,
    /// Body (empty for Bell; for Notification the body decided by the VT parser)
    pub body: String,
    /// Time of insertion (used for TTL)
    pub created_at: std::time::Instant,
}

/// Alert kind (Sprint 5-11-5).
///
/// OSC 9 / OSC 777 are unified server-side into `ServerToClient::DesktopNotification`,
/// so they cannot be distinguished in the client layer (both are folded into
/// `set_pending_notification` by the VT parser). From the SR perspective it is
/// also fine to treat them as a single "notification" kind, so we use a single
/// `Notification` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// Received a VT BEL `0x07`
    Bell,
    /// OSC 9 (iTerm2-compatible) / OSC 777 (urxvt-compatible) desktop notification
    Notification,
}

/// Maximum length of the alert queue (Sprint 5-11-5).
///
/// On overflow, entries are dropped oldest-first. Since the SR only announces
/// new alerts, retaining older ones provides little value.
pub const ALERTS_MAX_LEN: usize = 16;

/// Alert TTL (Sprint 5-11-5).
///
/// After the SR has read the alert, it is auto-removed from the tree to keep
/// it from bloating. Five seconds balances a typical SR announcement duration
/// with human cognitive timing.
pub const ALERT_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// Entire state of the GPU client
pub struct ClientState {
    pub panes: HashMap<u32, PaneState>,
    pub focused_pane_id: Option<u32>,
    /// Pane layout info received from the server (used for split rendering)
    pub pane_layouts: HashMap<u32, PaneLayout>,
    pub cols: u16,
    pub rows: u16,
    pub palette: CommandPalette,
    pub search: SearchState,
    /// Scrollback length specified in the config
    pub scrollback_capacity: usize,
    /// Latest evaluated text for the left status-bar widget (cached)
    pub status_bar_text: String,
    /// Latest evaluated text for the right status-bar widget (cached)
    pub status_bar_right_text: String,
    /// BEL-received flag (the next `about_to_wait` triggers the OS notification)
    pub pending_bell: bool,
    /// Copy mode (Vim-style text selection)
    pub copy_mode: CopyModeState,
    /// Mouse drag selection
    pub mouse_sel: MouseSelection,
    /// IME composing text (preedit)
    pub ime_preedit: Option<String>,
    /// Whether broadcast mode is active
    pub broadcast_mode: bool,
    /// Whether the pane-number overlay is being displayed
    pub display_panes_mode: bool,
    /// Context menu opened via right click (None = hidden)
    pub context_menu: Option<ContextMenu>,
    /// Whether pane zoom is enabled
    pub is_zoomed: bool,
    /// Quick Select mode
    pub quick_select: QuickSelectState,
    /// Host manager UI
    pub host_manager: HostManager,
    /// Lua macro picker UI
    pub macro_picker: MacroPicker,
    /// SFTP file transfer dialog
    pub file_transfer: FileTransferDialog,
    /// Settings panel (Ctrl+,)
    pub settings_panel: SettingsPanel,
    /// Mouse reporting mode reported by the server (0=disabled, 1=X11, 2=SGR)
    #[allow(dead_code)]
    pub mouse_reporting_mode: u8,
    /// Cached geometry for floating panes
    pub floating_pane_rects: HashMap<u32, FloatRect>,
    /// Click ranges of each tab in the tab bar (pane_id → (x_start, x_end)).
    /// The renderer updates this every frame; the mouse handler reads it.
    pub tab_hit_rects: HashMap<u32, (f32, f32)>,
    /// Click range of the `[↗]` detach button shown on tab hover (Sprint 5-9 Phase 4-6).
    ///
    /// `pane_id → (x_start, x_end)`. The renderer registers this every frame for
    /// the hovered tab only, and `event_handler/mouse.rs::on_mouse_left_pressed`
    /// detects it with priority over plain tab hit-testing. On click it fires the
    /// `DetachToNewWindow` path and detaches the target pane into a new OS Window.
    /// Works without depending on global coordinates, including on Wayland.
    pub tab_tearout_hit_rects: HashMap<u32, (f32, f32)>,
    /// Phase 2 (UI/UX modernization): hover-only close `×` button hit regions
    /// per tab `pane_id`. Populated each frame by `build_tab_bar_verts` and
    /// consumed by mouse-click handling to fire the `ClosePane` IPC path.
    pub tab_close_hit_rects: HashMap<u32, (f32, f32)>,
    /// Click range (x_start, x_end) of the settings button on the tab bar
    pub settings_tab_rect: Option<(f32, f32)>,
    /// Click range (x_start, x_end) of the `+` new-tab button on the tab bar
    /// (Sprint 5-15 / UI/UX Modernization v2 Phase 2b). Populated each frame
    /// by `build_tab_bar_verts` only when `TabBarConfig.show_new_tab_button`
    /// is on. `None` means the button is hidden or off-screen.
    pub new_tab_hit_rect: Option<(f32, f32)>,
    /// `pane_id` of the tab the mouse is currently hovering (Sprint 5-7 / UI-1-1).
    /// Updated by `renderer/event_handler/mouse.rs` on mouse-move; the tab-bar
    /// renderer brightens the background for the hovered tab.
    pub hovered_tab_id: Option<u32>,
    /// OS-reported light/dark preference (Sprint 5-15 / Phase 3).
    /// `Some(true)` = dark, `Some(false)` = light, `None` = unknown.
    /// Updated by `WindowEvent::ThemeChanged` and at window creation.
    /// Consumed via [`nexterm_config::Config::effective_color_scheme`].
    pub os_dark_mode: Option<bool>,
    /// Time when the key-hint overlay should disappear (Sprint 5-7 / UI-1-4).
    /// On a lone Leader press this is set to two seconds in the future; the
    /// `lifecycle` clears it back to `None` once that time passes. While `Some`,
    /// prefix-style bindings from `config.keys` are shown semi-transparent at the
    /// bottom of the screen.
    pub key_hint_visible_until: Option<std::time::Instant>,
    /// End time of the tmux-style prefix mode entered right after Leader is
    /// pressed (Sprint 5-7 / UI-1-4 bug fix).
    /// Set together with `key_hint_visible_until` on a lone Leader press.
    /// While `Some`, incoming key input is matched only against `<leader> X`
    /// style bindings; otherwise it falls through as a normal input. Reset to
    /// `None` on expiry or on a successful match.
    pub prefix_pending_until: Option<std::time::Instant>,
    /// Update notification banner (Some(version) = visible, None = hidden)
    pub update_banner: Option<String>,
    /// Offline-mode banner timestamp (Sprint 5-14 / v1.7.8 — P2-1).
    ///
    /// `Some(Instant)` indicates "offline since this time". The renderer
    /// formats the banner with the elapsed seconds so the user can see
    /// progress while the embedded server is starting up. Set by
    /// `try_connect` once the connect-failure streak exceeds the threshold
    /// (currently 1 s = ~5 attempts at the 200 ms cadence) and reset to
    /// `None` on a successful connect.
    pub offline_banner_since: Option<std::time::Instant>,
    /// Consent dialog for sensitive operations (Sprint 4-1).
    /// While `Some`, the dialog consumes every key input.
    pub pending_consent: Option<ConsentDialog>,
    /// "Always allow" decisions for the current session (reset on next launch)
    pub session_consent_overrides: SessionConsentOverrides,
    /// Name of the currently active workspace (Sprint 5-7 / Phase 2-1).
    /// Updated whenever `WorkspaceList` / `WorkspaceSwitched` arrives from the server.
    /// Read by the `workspace` built-in widget in the status bar.
    pub current_workspace: String,
    /// Pending queue for Quake-mode toggle requests (Sprint 5-7 / Phase 2-2).
    ///
    /// `apply_server_message` populates this on `QuakeToggleRequest` and the
    /// lifecycle picks it up on the next frame to actually drive the window
    /// (we keep mutable access to the winit Window outside `ClientState`).
    /// The value is one of `"toggle"` / `"show"` / `"hide"`.
    pub pending_quake_action: Option<String>,
    /// Tab display order (Sprint 5-7 / Phase 2-3).
    ///
    /// Mirrors the order of the `LayoutChanged.panes` array received from the
    /// server (the logical tab order, sorted by `Window.pane_order`). The
    /// tab-bar render loop follows this order.
    pub tab_order: Vec<u32>,
    /// Tab-drag state (Sprint 5-7 / Phase 2-3).
    /// While `Some`, a ghost tab is rendered and the drop reorders on release.
    pub tab_drag: Option<TabDragState>,
    /// Phase 4 (UI/UX v2): mouse drag on a pane split border. `Some` while the
    /// left button is held after pressing inside the border hit-tolerance band.
    pub pane_resize_drag: Option<PaneResizeDrag>,
    /// Phase 4 (UI/UX v2): last cursor icon we asked winit to display. Avoids
    /// thrashing the OS cursor by re-issuing identical `set_cursor` calls.
    /// `winit::window::CursorIcon::Default` mirrors the platform default.
    pub last_cursor_icon: winit::window::CursorIcon,
    /// Animation manager (Sprint 5-7 / Phase 3-2).
    ///
    /// Records timestamps for tab-switch / pane-add and lets the renderer query
    /// progress in [0,1]. With `AnimationsConfig.enabled = false` or
    /// `intensity = "off"`, `scaled_duration_ms` returns 0, so progress is
    /// always 1.0 and animations are effectively disabled.
    pub animations: crate::animations::AnimationManager,
    /// Server Window ID currently shown in the primary OS Window (Sprint 5-8 Phase 4-4).
    ///
    /// On `WindowListChanged`, the Window with `is_focused = true` is recorded
    /// here. When a tab is dropped onto the tab bar of the primary Window during
    /// tab tearing, this field is used to resolve `MovePaneToWindow.target_window_id`.
    pub focused_server_window_id: u32,
    /// Latest response to `QueryForegroundProcess` (Sprint 5-8 Phase 4-5).
    ///
    /// Populated by `apply_server_message` on `ForegroundProcessStatus`.
    /// After `event_handler` matches it against `pending_close_request` and
    /// decides between showing the confirmation dialog and detaching
    /// immediately, it `take()`s the value to clear the slot.
    pub foreground_process_status: Option<ForegroundProcessStatus>,
    /// Pending OS Window close request (Sprint 5-8 Phase 4-5).
    ///
    /// With `close_action = "prompt"`, when the user fires an OS Window close
    /// action we send `QueryForegroundProcess` and record it here. Depending on
    /// the response (or the choice in the confirmation dialog) we then run
    /// detach / kill / cancel.
    pub pending_close_request: Option<PendingCloseRequest>,
    /// Visibility state of the "Close this window?" confirmation dialog (Sprint 5-8 Phase 4-5).
    ///
    /// While `Some`, the renderer paints a modal dialog. `Enter` confirms,
    /// `Esc` cancels. On Wayland, the `[↗]` path reuses the same dialog.
    pub close_window_dialog: Option<CloseWindowDialog>,
    /// SR-facing alert queue (Sprint 5-11-5).
    ///
    /// FIFO that exposes Bell / OSC 9 / OSC 777 as `Role::Alert` nodes. Capped
    /// at `ALERTS_MAX_LEN`; entries past `ALERT_TTL` are auto-removed at the
    /// top of `update_accesskit_tree_if_needed` via `expire_alerts`.
    pub alerts: std::collections::VecDeque<AlertEntry>,
    /// Next `AlertEntry.seq` value to issue (Sprint 5-11-5).
    ///
    /// Monotonic counter. Exhausting a u64 in a single client run is
    /// effectively impossible (around 5.84 hundred million years at 1000
    /// alerts/sec). This is the rationale for collision-free NodeIds.
    pub next_alert_seq: u64,
    /// Banner used to surface non-fatal errors received from the server (Sprint 5-12 Phase 1).
    ///
    /// On `ServerToClient::Error` the message is stored here, and the renderer
    /// paints a red banner at the bottom. `Esc` restores it to `None`. This is
    /// a single slot overwritten by the latest error (never stacks). Coexists
    /// independently with `update_banner`.
    pub error_banner: Option<String>,
    /// Currently-selected command block, used by the block UI (Phase 2a).
    ///
    /// Lookup is `state.panes[pane_id].blocks` keyed by `BlockId`. `None` means
    /// nothing is selected (the renderer draws no highlight). Cleared when the
    /// referenced block leaves the scrollback or the pane is closed.
    #[allow(dead_code)] // consumed by the renderer / keybinding wiring in Phase 2b
    pub selected_block: Option<crate::command_blocks::BlockId>,
    /// Persisted store of user-assigned block names (Phase 2a).
    ///
    /// Loaded once on `ClientState::new` from
    /// `~/.local/state/nexterm/named_blocks.json` (or `%APPDATA%\nexterm\…` on
    /// Windows). Mutations write back atomically through `NamedBlockStore::save`.
    #[allow(dead_code)] // consumed by the palette / name-modal wiring in Phase 2b
    pub named_blocks: crate::named_blocks::NamedBlockStore,
    /// Block-name input modal (Phase 2c-4).
    ///
    /// Opened with `Ctrl+Shift+L` while a block is selected and dismissed via
    /// `Esc` / `Enter`. While `is_open` is true the input-handler routes most
    /// key events through the modal so the focused pane does not receive them.
    pub block_name_modal: blocks::BlockNameModal,
}

/// Response payload for `QueryForegroundProcess` (Sprint 5-8 Phase 4-5)
#[derive(Debug, Clone, Copy)]
pub struct ForegroundProcessStatus {
    /// Server Window ID being queried
    pub window_id: u32,
    /// `true` if a foreground process is running
    pub has_foreground: bool,
}

/// Pending OS Window close request (Sprint 5-8 Phase 4-5).
///
/// The `close_action` field is retained for future expansion so `Detach`/`Kill`
/// can also go through the pending path. Today only `Prompt` enters the pending
/// state, so the renderer side does not read it yet (`dead_code` suppression).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct PendingCloseRequest {
    /// Server Window ID being shown in the OS Window that triggered the close
    pub server_window_id: u32,
    /// Value of the `window.close_action` setting
    pub close_action: CloseActionKind,
}

/// Client-side mirror of `WindowConfig.close_action` (Sprint 5-8 Phase 4-5).
///
/// Semantically equivalent to the server-side `nexterm_config::CloseAction`.
/// We keep a separate enum on the client to drive `pending_close_request`
/// decisions without growing the inter-crate dependency.
///
/// `Detach` / `Kill` are not assigned to `pending_close_request.close_action`
/// today (only `Prompt` enters the pending state) but are reserved for a
/// future setting that also shows a confirmation dialog on `Detach`, or for
/// a per-window close path that keeps `Kill` pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CloseActionKind {
    /// Show the confirmation dialog only when a foreground process is detected (default)
    Prompt,
    /// Detach without confirmation (the server-side session is kept)
    Detach,
    /// Kill without confirmation (legacy behaviour)
    Kill,
}

/// Visibility state of the close-window confirmation dialog (Sprint 5-8 Phase 4-5).
///
/// Renderer-side dialog drawing is wired up in a follow-up; today
/// `server_window_id` / `message` / `kill_label` / `cancel_label` are unread
/// (`dead_code` suppression). Only the state flow is consumed via
/// `poll_pending_close_request`, with the signal values on `selected_button`
/// (`0xFE` = Kill confirmed, `0xFF` = Cancel confirmed) driving the decision.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CloseWindowDialog {
    /// Server Window ID the confirmation targets
    pub server_window_id: u32,
    /// Message to display (i18n-ready)
    pub message: String,
    /// Label of the "Close (Kill)" button (i18n-ready)
    pub kill_label: String,
    /// Label of the "Cancel" button (i18n-ready)
    pub cancel_label: String,
    /// Currently highlighted button (0 = Kill, 1 = Cancel; 0xFE = Kill confirmed, 0xFF = Cancel confirmed)
    pub selected_button: u8,
}

/// Tab-drag state (Sprint 5-7 / Phase 2-3)
#[derive(Debug, Clone)]
pub struct TabDragState {
    /// pane ID at drag start (the tab being moved)
    pub pane_id: u32,
    /// Mouse X at drag start (used for the click-threshold check)
    pub start_x: f32,
    /// Current mouse X (used to position the ghost)
    pub current_x: f32,
    /// pane ID of the current hover insertion target (drop moves to that slot).
    /// `None` means no insertion target yet (outside the tab bar or hovering itself).
    pub hover_target: Option<u32>,
    /// Whether the gesture has actually been promoted to a drag (X movement
    /// exceeded the threshold). Released while still `false` counts as a click.
    pub committed: bool,
    /// OS Window ID at drag start (Sprint 5-8 Phase 4-2).
    ///
    /// Used in Phase 4-2 to identify the source for the tab-out-of-bar drop
    /// path. `Option` guards against the primary Window not yet being
    /// initialized (in practice always `Some`).
    #[allow(dead_code)]
    pub source_os_window_id: Option<WindowId>,
    /// Screen coordinates at drag start (Sprint 5-8 Phase 4-2).
    ///
    /// Captured via the platform helper (added in Step 2.3) from
    /// `event_handler::mouse::on_mouse_left_pressed`. `None` on platforms where
    /// global coordinates cannot be obtained (Wayland).
    #[allow(dead_code)]
    pub start_screen_pos: Option<(i32, i32)>,
    /// Current screen coordinates (Sprint 5-8 Phase 4-2).
    ///
    /// Updated from `event_handler::mouse::on_cursor_moved` (Step 2.4 wiring).
    /// On drop (Step 2.5) it is passed to `compute_drop_target`. When `None`,
    /// the "spawn a new OS Window" decision is skipped (preserving existing
    /// behaviour).
    #[allow(dead_code)]
    pub current_screen_pos: Option<(i32, i32)>,
}

/// Phase 4 (UI/UX v2): in-flight mouse drag on a pane split border.
///
/// Captured on `on_mouse_left_pressed` when the cursor falls inside the
/// hit-tolerance band of an internal pane border, updated in
/// `on_cursor_moved`, and cleared on `on_mouse_left_released`. Sends
/// `ClientToServer::ResizeSplit { delta }` deltas while dragging — the server
/// already supports adjusting the ratio of the Split closest to the focused
/// pane (`window/bsp.rs::adjust_ratio_for`), so the client focuses one of the
/// two adjacent panes at drag start and just streams pixel-delta-converted
/// ratio adjustments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneResizeDrag {
    /// Pane that received focus at drag start (one of the two panes on either
    /// side of the border). Snapshot only — the server's idea of focus may
    /// drift if another event fires mid-drag, but the resize math does not
    /// need to chase it.
    pub focused_pane_id: u32,
    /// Border axis: `Horizontal` means we drag along X (the split is
    /// vertical / panes are side-by-side); `Vertical` is the opposite.
    pub axis: PaneResizeAxis,
    /// Total length of the parent split in pixels at drag start. Used to
    /// convert pixel motion into a ratio delta in [-1.0, 1.0].
    pub span_px: f32,
    /// Cursor position at the previous `on_cursor_moved` callback. Used to
    /// compute incremental deltas (so each emitted `ResizeSplit` reflects a
    /// single mouse move, not the cumulative motion).
    pub last_cursor: (f32, f32),
}

/// Axis along which a pane border is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneResizeAxis {
    /// Border runs top-to-bottom (vertical line); dragging moves it left/right.
    Horizontal,
    /// Border runs left-to-right (horizontal line); dragging moves it up/down.
    Vertical,
}

/// Result of hit-testing the cursor against the internal borders of the
/// tiled pane layout (Phase 4 / UI-UX v2). Encodes which border was hit so
/// the renderer can show the correct resize cursor (column / row) and the
/// drag handler can pick the right axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneBorderHit {
    /// One of the two panes adjacent to the border. Conventionally the pane
    /// whose right (or bottom) edge is the border.
    pub adjacent_pane_id: u32,
    /// Border axis.
    pub axis: PaneResizeAxis,
}

/// Pixel half-width of the border hit-tolerance band. Picked to be a couple
/// of pixels wider than the border line itself so the affordance is not
/// pixel-perfect.
pub const PANE_BORDER_HIT_TOLERANCE: f32 = 4.0;

/// Pure helper that hit-tests `(cursor_x, cursor_y)` against the borders
/// implied by `layouts`. The cell metrics (`cell_w`, `cell_h`) and the grid
/// origin (`origin_x`, `origin_y`) are passed in explicitly so the function
/// stays renderer-agnostic and easy to unit-test.
///
/// Strategy:
/// - For every pair of panes that share a column edge (one pane's right edge
///   ≈ another pane's left edge, with overlapping row ranges), the gap is a
///   vertical border → dragging it on the horizontal axis resizes.
/// - Symmetric for shared row edges → vertical-axis drag.
///
/// Returns the first border whose tolerance band contains the cursor, biased
/// toward vertical borders to make narrow-column resizes easier to grab.
pub fn hit_test_pane_border(
    layouts: &HashMap<u32, PaneLayout>,
    cursor_x: f32,
    cursor_y: f32,
    cell_w: f32,
    cell_h: f32,
    origin_x: f32,
    origin_y: f32,
) -> Option<PaneBorderHit> {
    if cell_w <= 0.0 || cell_h <= 0.0 {
        return None;
    }
    let tol = PANE_BORDER_HIT_TOLERANCE;
    // Snapshot layouts for stable iteration order; HashMap order is not
    // deterministic so we sort by pane_id to keep hit-test results stable
    // across frames (avoids cursor flicker when two borders coincide).
    let mut panes: Vec<&PaneLayout> = layouts.values().collect();
    panes.sort_by_key(|p| p.pane_id);

    // Vertical borders (shared column edge).
    for a in &panes {
        let a_right_col = a.col_offset as f32 + a.cols as f32;
        let border_x = origin_x + a_right_col * cell_w;
        if (cursor_x - border_x).abs() > tol {
            continue;
        }
        // Find any pane whose left edge sits at `a_right_col` and whose row
        // range overlaps `a`'s row range (i.e. they share this border).
        for b in &panes {
            if b.pane_id == a.pane_id {
                continue;
            }
            if b.col_offset as f32 != a_right_col {
                continue;
            }
            let a_top = a.row_offset;
            let a_bot = a.row_offset + a.rows;
            let b_top = b.row_offset;
            let b_bot = b.row_offset + b.rows;
            let row_overlap = a_top.max(b_top) < a_bot.min(b_bot);
            if !row_overlap {
                continue;
            }
            // Convert cursor y to pane-row coordinates; require it to lie
            // within the overlapping row range.
            let pane_y_top = origin_y + a_top.max(b_top) as f32 * cell_h;
            let pane_y_bot = origin_y + a_bot.min(b_bot) as f32 * cell_h;
            if cursor_y >= pane_y_top && cursor_y < pane_y_bot {
                return Some(PaneBorderHit {
                    adjacent_pane_id: a.pane_id,
                    axis: PaneResizeAxis::Horizontal,
                });
            }
        }
    }

    // Horizontal borders (shared row edge).
    for a in &panes {
        let a_bot_row = a.row_offset as f32 + a.rows as f32;
        let border_y = origin_y + a_bot_row * cell_h;
        if (cursor_y - border_y).abs() > tol {
            continue;
        }
        for b in &panes {
            if b.pane_id == a.pane_id {
                continue;
            }
            if b.row_offset as f32 != a_bot_row {
                continue;
            }
            let a_left = a.col_offset;
            let a_right = a.col_offset + a.cols;
            let b_left = b.col_offset;
            let b_right = b.col_offset + b.cols;
            let col_overlap = a_left.max(b_left) < a_right.min(b_right);
            if !col_overlap {
                continue;
            }
            let pane_x_left = origin_x + a_left.max(b_left) as f32 * cell_w;
            let pane_x_right = origin_x + a_right.min(b_right) as f32 * cell_w;
            if cursor_x >= pane_x_left && cursor_x < pane_x_right {
                return Some(PaneBorderHit {
                    adjacent_pane_id: a.pane_id,
                    axis: PaneResizeAxis::Vertical,
                });
            }
        }
    }

    None
}

impl ClientState {
    pub fn new(cols: u16, rows: u16, scrollback_capacity: usize) -> Self {
        Self {
            panes: HashMap::new(),
            focused_pane_id: None,
            pane_layouts: HashMap::new(),
            cols,
            rows,
            // Sprint 5-7 / Phase 3-3: load the persisted usage history
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
            tab_close_hit_rects: HashMap::new(),
            settings_tab_rect: None,
            new_tab_hit_rect: None,
            hovered_tab_id: None,
            os_dark_mode: None,
            key_hint_visible_until: None,
            prefix_pending_until: None,
            update_banner: None,
            offline_banner_since: None,
            pending_consent: None,
            session_consent_overrides: SessionConsentOverrides::default(),
            current_workspace: "default".to_string(),
            pending_quake_action: None,
            tab_order: Vec::new(),
            tab_drag: None,
            pane_resize_drag: None,
            last_cursor_icon: winit::window::CursorIcon::Default,
            animations: crate::animations::AnimationManager::new(),
            // Phase 4-4: reflect the focused Window ID on WindowListChanged
            focused_server_window_id: 0,
            // Phase 4-5: for the Window-close confirmation dialog
            foreground_process_status: None,
            pending_close_request: None,
            close_window_dialog: None,
            // Sprint 5-11-5: AccessKit Role::Alert notification queue
            alerts: std::collections::VecDeque::new(),
            next_alert_seq: 0,
            // Sprint 5-12 Phase 1: banner for server-error display
            error_banner: None,
            // Command-blocks Phase 2a: per-session block UI state.
            selected_block: None,
            named_blocks: crate::named_blocks::NamedBlockStore::load(),
            // Command-blocks Phase 2c-4: block-name input modal.
            block_name_modal: blocks::BlockNameModal::default(),
        }
    }

    /// Push an SR-facing alert onto the queue (Sprint 5-11-5).
    ///
    /// `seq` is assigned automatically. When the queue exceeds `ALERTS_MAX_LEN`,
    /// entries are dropped oldest-first (`pop_front`). This method takes
    /// ownership of `title` / `body`.
    ///
    /// Returns the assigned `seq`. Callers can log it if useful.
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
        // Drop the oldest entries when the cap is exceeded
        while self.alerts.len() > ALERTS_MAX_LEN {
            self.alerts.pop_front();
        }
        seq
    }

    /// Remove alerts whose TTL has expired (Sprint 5-11-5).
    ///
    /// The caller computes `now` via `Instant::now()` and passes it in (for
    /// testability). Entries where `created_at + ALERT_TTL < now` are removed
    /// front-to-back via `pop_front`. Since alerts are inserted in time order,
    /// the scan stops as soon as a still-fresh entry appears at the front.
    ///
    /// Returns the number of removed entries.
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

    /// Immediately dismiss the alert with the given `seq` (Phase 5-11-6 #4).
    ///
    /// Used on the SR `Action::Click` path to remove an alert without waiting
    /// for the 5-second TTL. No-op if the seq is not present (e.g. already
    /// removed by `expire_alerts`).
    ///
    /// Returns `true` if a matching seq was removed, `false` otherwise.
    pub fn dismiss_alert(&mut self, seq: u64) -> bool {
        let before = self.alerts.len();
        self.alerts.retain(|a| a.seq != seq);
        before != self.alerts.len()
    }

    /// Switch the focused pane and clear its activity flag.
    ///
    /// Sprint 5-7 / Phase 3-2: also records the tab-switch animation
    /// (no re-trigger if the same pane is refocused).
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
        // Phase 4 (UI/UX modernization): always sync pane-dim spring targets on focus change.
        let all_ids: Vec<u32> = self.pane_layouts.keys().copied().collect();
        self.animations.record_focus_changed(pane_id, &all_ids);
    }

    /// Return the list of pane IDs with background activity
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

    /// Toggle the command palette
    pub fn toggle_palette(&mut self) {
        if self.palette.is_open {
            self.palette.close();
        } else {
            self.palette.open();
        }
    }
}

#[cfg(test)]
mod pane_border_hit_tests {
    //! Phase 4 (UI/UX v2): pure hit-test against pane split borders.
    use super::*;

    fn layout(id: u32, col: u16, row: u16, cols: u16, rows: u16) -> PaneLayout {
        PaneLayout {
            pane_id: id,
            col_offset: col,
            row_offset: row,
            cols,
            rows,
            is_focused: false,
        }
    }

    /// A horizontal 50/50 split: two side-by-side panes. The shared vertical
    /// border is at the right edge of pane 1 / left edge of pane 2. Clicking
    /// dead-center on that border with the standard tolerance must hit it.
    #[test]
    fn detects_vertical_border_between_side_by_side_panes() {
        let mut layouts = HashMap::new();
        layouts.insert(1, layout(1, 0, 0, 40, 24));
        layouts.insert(2, layout(2, 40, 0, 40, 24));
        // cell_w=10, cell_h=10; origin at (0,0); border at x=400.
        let hit = hit_test_pane_border(&layouts, 400.0, 100.0, 10.0, 10.0, 0.0, 0.0)
            .expect("border at x=400 should be hit");
        assert_eq!(hit.axis, PaneResizeAxis::Horizontal);
    }

    /// Same setup, but click far away from the border: must miss.
    #[test]
    fn misses_when_far_from_border() {
        let mut layouts = HashMap::new();
        layouts.insert(1, layout(1, 0, 0, 40, 24));
        layouts.insert(2, layout(2, 40, 0, 40, 24));
        assert!(hit_test_pane_border(&layouts, 200.0, 100.0, 10.0, 10.0, 0.0, 0.0).is_none());
    }

    /// Tolerance band must be respected on both sides of the border. A click
    /// at exactly `border_x ± (tol - 0.5)` hits; `border_x ± (tol + 0.5)` misses.
    #[test]
    fn respects_tolerance_band_on_both_sides() {
        let mut layouts = HashMap::new();
        layouts.insert(1, layout(1, 0, 0, 40, 24));
        layouts.insert(2, layout(2, 40, 0, 40, 24));
        let tol = PANE_BORDER_HIT_TOLERANCE;
        // Inside tolerance.
        assert!(
            hit_test_pane_border(&layouts, 400.0 - (tol - 0.5), 100.0, 10.0, 10.0, 0.0, 0.0)
                .is_some()
        );
        assert!(
            hit_test_pane_border(&layouts, 400.0 + (tol - 0.5), 100.0, 10.0, 10.0, 0.0, 0.0)
                .is_some()
        );
        // Outside tolerance.
        assert!(
            hit_test_pane_border(&layouts, 400.0 - (tol + 0.5), 100.0, 10.0, 10.0, 0.0, 0.0)
                .is_none()
        );
        assert!(
            hit_test_pane_border(&layouts, 400.0 + (tol + 0.5), 100.0, 10.0, 10.0, 0.0, 0.0)
                .is_none()
        );
    }

    /// A vertical 50/50 split: two stacked panes. The shared horizontal
    /// border must be detected with `Vertical` axis.
    #[test]
    fn detects_horizontal_border_between_stacked_panes() {
        let mut layouts = HashMap::new();
        layouts.insert(1, layout(1, 0, 0, 80, 12));
        layouts.insert(2, layout(2, 0, 12, 80, 12));
        // border y = 12 * cell_h = 120.
        let hit = hit_test_pane_border(&layouts, 200.0, 120.0, 10.0, 10.0, 0.0, 0.0)
            .expect("border at y=120 should be hit");
        assert_eq!(hit.axis, PaneResizeAxis::Vertical);
    }

    /// Grid-origin offset (tab bar + padding) must shift the border line.
    /// Without applying it, the click would land in the wrong place.
    #[test]
    fn respects_grid_origin_offset() {
        let mut layouts = HashMap::new();
        layouts.insert(1, layout(1, 0, 0, 40, 24));
        layouts.insert(2, layout(2, 40, 0, 40, 24));
        // origin_y=30 (tab bar 24 + pad 6); border still at x=400 but the
        // y range must include the offset.
        let hit = hit_test_pane_border(&layouts, 400.0, 130.0, 10.0, 10.0, 0.0, 30.0)
            .expect("offset border should still hit");
        assert_eq!(hit.axis, PaneResizeAxis::Horizontal);
        // Above the grid (y < origin_y) — should not hit.
        assert!(hit_test_pane_border(&layouts, 400.0, 10.0, 10.0, 10.0, 0.0, 30.0).is_none());
    }

    /// L-shaped layout: one pane splits a column but the row ranges only
    /// partially overlap. The cursor at the overlap region hits; outside it
    /// misses.
    #[test]
    fn requires_row_range_overlap_for_vertical_border() {
        let mut layouts = HashMap::new();
        layouts.insert(1, layout(1, 0, 0, 40, 12)); // top-left
        layouts.insert(2, layout(2, 40, 0, 40, 24)); // right (full height)
        // Overlap: rows 0..=11 (since 1 ends at row 12). Cursor at y=50 (row 5).
        assert!(hit_test_pane_border(&layouts, 400.0, 50.0, 10.0, 10.0, 0.0, 0.0).is_some());
        // Outside overlap: y=180 (row 18) — pane 1 is gone there.
        assert!(hit_test_pane_border(&layouts, 400.0, 180.0, 10.0, 10.0, 0.0, 0.0).is_none());
    }

    /// Empty layouts: nothing to hit.
    #[test]
    fn empty_layouts_never_hit() {
        let layouts = HashMap::new();
        assert!(hit_test_pane_border(&layouts, 100.0, 100.0, 10.0, 10.0, 0.0, 0.0).is_none());
    }

    /// Degenerate cell metrics must not panic and must not report hits.
    #[test]
    fn zero_cell_metrics_return_none() {
        let mut layouts = HashMap::new();
        layouts.insert(1, layout(1, 0, 0, 40, 24));
        assert!(hit_test_pane_border(&layouts, 100.0, 100.0, 0.0, 10.0, 0.0, 0.0).is_none());
        assert!(hit_test_pane_border(&layouts, 100.0, 100.0, 10.0, 0.0, 0.0, 0.0).is_none());
    }
}
