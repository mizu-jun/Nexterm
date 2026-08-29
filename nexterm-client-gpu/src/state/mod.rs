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
use std::time::Instant;

use nexterm_proto::PaneLayout;
use winit::window::WindowId;

use crate::host_manager::HostManager;
use crate::macro_picker::MacroPicker;
use crate::palette::CommandPalette;
use crate::renderer::overlay::infobar::{InfoBar, InfoBarKind, InfoBarSlot};
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
    QuickSelectState, split_message_for_profile,
};
pub use pane::{
    FloatRect, PaneColorOverrides, PaneState, PlacedImage, pointer_shape_to_cursor_icon,
};
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
    /// Entrance animation for the context menu (UI/UX v3 P3b).
    pub context_menu_opening: Option<crate::animations::Timed>,
    /// Exit animation for the context menu (UI/UX v3 P3b) — **render-only**.
    ///
    /// The `Option` above *is* the menu's openness, so dismissing it
    /// destroys the content the exit animation still needs to draw. The
    /// ghost owns a clone; `context_menu` goes `None` at once, so nothing
    /// can be hovered or clicked while it fades.
    pub context_menu_closing: Option<(ContextMenu, crate::animations::Timed)>,
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
    /// Click range (x_start, x_end) of the `▾` new-tab dropdown button on the
    /// tab bar (Windows-Terminal-like profile dropdown, P1). Populated each
    /// frame by `build_tab_bar_verts` only when
    /// `TabBarConfig.show_new_tab_button` is on.
    pub new_tab_dropdown_hit_rect: Option<(f32, f32)>,
    /// Click ranges (x_start, x_end) of the minimize / maximize / close
    /// window buttons drawn at the right edge of the tab bar when
    /// `window.decorations = "notitle"` (custom title bar). Populated each
    /// frame by `build_tab_bar_verts`; `None` while the buttons are hidden.
    pub window_minimize_hit_rect: Option<(f32, f32)>,
    /// See [`Self::window_minimize_hit_rect`].
    pub window_maximize_hit_rect: Option<(f32, f32)>,
    /// See [`Self::window_minimize_hit_rect`].
    pub window_close_hit_rect: Option<(f32, f32)>,
    /// Window button the cursor is currently over (custom title bar only).
    /// Updated on mouse-move; the renderer highlights the hovered button
    /// (the close button gets the semantic error colour, WT-style).
    pub hovered_window_button: Option<WindowButton>,
    /// Hover cross-fade over the custom title bar's window buttons
    /// (UI/UX v3 P3b2b). `hovered_window_button` above stays the truth for
    /// hit-testing; this is render-only.
    pub window_button_hover: crate::animations::HoverTransition<WindowButton>,
    /// Press pulse for the custom title bar's window buttons (UI/UX v3 P3b3).
    ///
    /// Wired for all three even though Minimize and Close tear the window
    /// down before the pulse can be seen: excluding them would leave an
    /// untestable exception in the press chain for no visible gain.
    pub window_button_press: crate::animations::PressPulse<WindowButton>,
    /// WSL distros detected at startup (`nexterm_config::wsl::detect_distros`),
    /// shown in the new-tab dropdown after the configured profiles. Cached
    /// once because detection shells out to `wsl.exe` on Windows.
    pub wsl_profiles: Vec<nexterm_config::Profile>,
    /// `pane_id` of the tab the mouse is currently hovering (Sprint 5-7 / UI-1-1).
    /// Updated by `renderer/event_handler/mouse.rs` on mouse-move; the tab-bar
    /// renderer brightens the background for the hovered tab.
    pub hovered_tab_id: Option<u32>,
    /// Hover cross-fade over the tab bar (UI/UX v3 P3b2b).
    ///
    /// `hovered_tab_id` above stays the truth for hit-testing and for
    /// whether the tear-out and close buttons are drawn; this is render-only
    /// and outlives it by one fade so the tab the pointer left can dim back
    /// down.
    pub tab_hover: crate::animations::HoverTransition<u32>,
    /// Press pulse for the tab bar (UI/UX v3 P3b3). A tab click commits on
    /// mouse-down, so this decays on its own rather than waiting for the
    /// button to come up.
    pub tab_press: crate::animations::PressPulse<u32>,
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
    /// Non-blocking status messages, newest last (UI/UX v3 P6).
    ///
    /// Replaces the three `Option` banner fields this used to be — update,
    /// offline and server error — each of which had its own builder and its
    /// own stacking arithmetic. Queue through [`ClientState::push_info_bar`]
    /// and clear through [`ClientState::remove_info_bar`]; where a bar sits is
    /// decided in one place, `overlay::infobar::bar_rects`.
    pub info_bars: std::collections::VecDeque<InfoBar>,
    /// Consent dialog for sensitive operations (Sprint 4-1).
    /// While `Some`, the dialog consumes every key input.
    pub pending_consent: Option<ConsentDialog>,
    /// Entrance animation for the consent dialog (UI/UX v3 P3b).
    pub pending_consent_opening: Option<crate::animations::Timed>,
    /// Exit animation for the consent dialog (UI/UX v3 P3b) — **render-only**.
    ///
    /// `pending_consent` is the security-relevant field: it goes `None` the
    /// instant the dialog is answered or cancelled, since every input path
    /// (keyboard, accessibility, the consent decision itself) consults it to
    /// decide whether a prompt is still answerable. The ghost owns a clone
    /// purely so the renderer has something to fade out; no input path may
    /// read it. See `context_menu_closing` for the general rationale.
    pub pending_consent_closing: Option<(ConsentDialog, crate::animations::Timed)>,
    /// "Always allow" decisions for the current session (reset on next launch)
    pub session_consent_overrides: SessionConsentOverrides,
    /// Name of the currently active workspace (Sprint 5-7 / Phase 2-1).
    /// Updated whenever `WorkspaceList` / `WorkspaceSwitched` arrives from the server.
    /// Read by the `workspace` built-in widget in the status bar.
    pub current_workspace: String,
    /// Full workspace set as `(name, is_active)` pairs (roadmap Phase 3).
    /// Kept in sync with `WorkspaceList` / `WorkspaceSwitched` and used to
    /// build the palette's dynamic switch/create actions.
    pub workspaces: Vec<(String, bool)>,
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
    /// Last theme default colors reported to the server via `SetThemeColors`
    /// (roadmap #10b). Compared each redraw against the committed scheme so a
    /// report goes out only when the theme actually changes. `None` = not
    /// reported yet (e.g. the connection was not established).
    pub last_reported_theme: Option<([u8; 3], [u8; 3])>,
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
    /// Entrance animation for the close-window dialog (UI/UX v3 P3b).
    pub close_window_dialog_opening: Option<crate::animations::Timed>,
    /// Exit animation for the close-window dialog (UI/UX v3 P3b) —
    /// **render-only**. See `context_menu_closing` for the rationale.
    pub close_window_dialog_closing: Option<(CloseWindowDialog, crate::animations::Timed)>,
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

/// Window buttons drawn on the tab bar when the custom title bar is active
/// (`window.decorations = "notitle"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowButton {
    Minimize,
    /// Toggles between maximize and restore depending on the window state.
    Maximize,
    Close,
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
    /// Whether anything on screen still needs another frame (UI/UX v3 P3a).
    ///
    /// The event loop calls this once per tick and requests a redraw only
    /// when it is true, so an idle terminal keeps costing exactly what it
    /// cost before P3a — no extra frames, and therefore no extra
    /// pane-vertex-cache misses.
    ///
    /// `fade_duration_ms` is the pane fade-in duration the caller is using,
    /// already scaled by `AnimationsConfig::scaled_duration_ms`; 0 means
    /// animations are off and nothing is running.
    ///
    /// Overlay surfaces that own a `Timed` are ORed in here as they are
    /// migrated. Adding a surface means adding a clause; there is no
    /// registry to keep in sync.
    pub fn has_active_animation(&self, now: Instant, fade_duration_ms: u32) -> bool {
        if self.animations.has_active_animation(now, fade_duration_ms) {
            return true;
        }
        if self.settings_panel.motion.is_active(now) {
            return true;
        }
        // UI/UX v3 P6d: a bar fading in or out. Bars simply sitting there —
        // including one counting down to its auto-dismissal — must not answer
        // true, or the loop would redraw for the whole 20 s (G-idle).
        if self.info_bars.iter().any(|bar| bar.is_animating(now)) {
            return true;
        }
        if self.palette.motion.is_active(now)
            || self.macro_picker.motion.is_active(now)
            || self.host_manager.motion.is_active(now)
        {
            return true;
        }
        if self.block_name_modal.motion.is_active(now) || self.file_transfer.motion.is_active(now) {
            return true;
        }
        if self
            .context_menu_closing
            .as_ref()
            .is_some_and(|(_, t)| !t.is_done(now))
            || self
                .close_window_dialog_closing
                .as_ref()
                .is_some_and(|(_, t)| !t.is_done(now))
            || self
                .context_menu_opening
                .is_some_and(|t| self.context_menu.is_some() && !t.is_done(now))
            || self
                .close_window_dialog_opening
                .is_some_and(|t| self.close_window_dialog.is_some() && !t.is_done(now))
            || self
                .pending_consent_closing
                .as_ref()
                .is_some_and(|(_, t)| !t.is_done(now))
            || self
                .pending_consent_opening
                .is_some_and(|t| self.pending_consent.is_some() && !t.is_done(now))
        {
            return true;
        }
        if self.host_manager.password_modal_is_active(now) {
            return true;
        }
        if self.settings_panel.tooltip_motion.is_active(now) {
            return true;
        }
        if self.settings_panel.hover_transition.is_active(now) {
            return true;
        }
        if self.settings_panel.press_pulse.is_active(now) {
            return true;
        }
        if self
            .context_menu
            .as_ref()
            .is_some_and(|m| m.hover_transition.is_active(now))
            || self
                .context_menu_closing
                .as_ref()
                .is_some_and(|(m, _)| m.hover_transition.is_active(now))
        {
            return true;
        }
        if self
            .context_menu
            .as_ref()
            .is_some_and(|m| m.press_pulse.is_active(now))
        {
            return true;
        }
        if self.tab_hover.is_active(now) {
            return true;
        }
        if self.tab_press.is_active(now) {
            return true;
        }
        if self.window_button_hover.is_active(now) {
            return true;
        }
        if self.window_button_press.is_active(now) {
            return true;
        }
        false
    }

    /// Show `menu`, starting its entrance (UI/UX v3 P3b).
    pub fn show_context_menu(
        &mut self,
        menu: ContextMenu,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::FAST);
        self.context_menu_closing = None;
        self.context_menu_opening = Some(Timed::new(now, ms, Curve::DecelerateMax));
        self.context_menu = Some(menu);
    }

    /// Dismiss the context menu, leaving a ghost to fade out (UI/UX v3 P3b).
    pub fn dismiss_context_menu(&mut self, now: Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, Timed, duration};

        if let Some(menu) = self.context_menu.take() {
            let ms = anim.scaled_duration_ms(duration::FASTER);
            self.context_menu_closing = Some((menu, Timed::new(now, ms, Curve::AccelerateMax)));
        }
    }

    /// Show `dialog`, starting its entrance (UI/UX v3 P3b).
    pub fn show_close_window_dialog(
        &mut self,
        dialog: CloseWindowDialog,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::SLOW);
        self.close_window_dialog_closing = None;
        self.close_window_dialog_opening = Some(Timed::new(now, ms, Curve::DecelerateMax));
        self.close_window_dialog = Some(dialog);
    }

    /// Dismiss the close-window dialog, leaving a ghost (UI/UX v3 P3b).
    pub fn dismiss_close_window_dialog(
        &mut self,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        if let Some(dialog) = self.close_window_dialog.take() {
            let ms = anim.scaled_duration_ms(duration::FAST);
            self.close_window_dialog_closing =
                Some((dialog, Timed::new(now, ms, Curve::AccelerateMax)));
        }
    }

    /// Show a consent dialog, starting its entrance (UI/UX v3 P3b).
    pub fn show_consent_dialog(
        &mut self,
        dialog: ConsentDialog,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::SLOW);
        self.pending_consent_closing = None;
        self.pending_consent_opening = Some(Timed::new(now, ms, Curve::DecelerateMax));
        self.pending_consent = Some(dialog);
    }

    /// Dismiss the consent dialog, leaving a render-only ghost.
    ///
    /// `pending_consent` goes `None` here, not when the fade ends: a
    /// security prompt stops accepting input the moment it is answered.
    pub fn dismiss_consent_dialog(
        &mut self,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        if let Some(dialog) = self.pending_consent.take() {
            let ms = anim.scaled_duration_ms(duration::FAST);
            self.pending_consent_closing =
                Some((dialog, Timed::new(now, ms, Curve::AccelerateMax)));
        }
    }

    /// Drop every finished ghost (UI/UX v3 P3b). Called once per frame.
    pub fn retire_ghosts(&mut self, now: Instant) {
        if self
            .context_menu_closing
            .as_ref()
            .is_some_and(|(_, t)| t.is_done(now))
        {
            self.context_menu_closing = None;
        }
        if self
            .close_window_dialog_closing
            .as_ref()
            .is_some_and(|(_, t)| t.is_done(now))
        {
            self.close_window_dialog_closing = None;
        }
        if self
            .pending_consent_closing
            .as_ref()
            .is_some_and(|(_, t)| t.is_done(now))
        {
            self.pending_consent_closing = None;
        }
    }

    /// Visibility in `[0, 1]` of an `Option`-shaped surface (UI/UX v3 P3b):
    /// the entrance while it is live, the inverted exit while it is a ghost.
    pub(crate) fn option_surface_progress(
        live: bool,
        opening: Option<crate::animations::Timed>,
        ghost: Option<&crate::animations::Timed>,
        now: Instant,
    ) -> f32 {
        if live {
            return opening.map_or(1.0, |t| t.progress(now));
        }
        ghost.map_or(0.0, |t| 1.0 - t.progress(now))
    }

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
            context_menu_opening: None,
            context_menu_closing: None,
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
            new_tab_dropdown_hit_rect: None,
            window_minimize_hit_rect: None,
            window_maximize_hit_rect: None,
            window_close_hit_rect: None,
            hovered_window_button: None,
            window_button_hover: Default::default(),
            window_button_press: Default::default(),
            wsl_profiles: Vec::new(),
            hovered_tab_id: None,
            tab_hover: Default::default(),
            tab_press: Default::default(),
            os_dark_mode: None,
            key_hint_visible_until: None,
            prefix_pending_until: None,
            info_bars: std::collections::VecDeque::new(),
            pending_consent: None,
            pending_consent_opening: None,
            pending_consent_closing: None,
            session_consent_overrides: SessionConsentOverrides::default(),
            current_workspace: "default".to_string(),
            workspaces: Vec::new(),
            pending_quake_action: None,
            tab_order: Vec::new(),
            tab_drag: None,
            pane_resize_drag: None,
            last_cursor_icon: winit::window::CursorIcon::Default,
            last_reported_theme: None,
            animations: crate::animations::AnimationManager::new(),
            // Phase 4-4: reflect the focused Window ID on WindowListChanged
            focused_server_window_id: 0,
            // Phase 4-5: for the Window-close confirmation dialog
            foreground_process_status: None,
            pending_close_request: None,
            close_window_dialog: None,
            close_window_dialog_opening: None,
            close_window_dialog_closing: None,
            // Sprint 5-11-5: AccessKit Role::Alert notification queue
            alerts: std::collections::VecDeque::new(),
            next_alert_seq: 0,
            // Command-blocks Phase 2a: per-session block UI state.
            selected_block: None,
            named_blocks: crate::named_blocks::NamedBlockStore::load(),
            // Command-blocks Phase 2c-4: block-name input modal.
            block_name_modal: blocks::BlockNameModal::default(),
        }
    }

    /// Queue an InfoBar, replacing any bar already in that slot (UI/UX v3 P6).
    ///
    /// Replacement rather than stacking is the behaviour the three `Option`
    /// fields had: a second server error overwrote the first, and the update
    /// checker only ever raised its banner while none was up. Returns whether
    /// the stack changed, so the caller can decide to redraw.
    pub fn push_info_bar(
        &mut self,
        kind: InfoBarKind,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) -> bool {
        use crate::animations::{Curve, Timed, duration};

        // A bar on its way out does not hold its message against a repeat:
        // the user has already stopped seeing it, so the same error arriving
        // again is news rather than a duplicate.
        if self
            .info_bars
            .iter()
            .any(|bar| bar.kind == kind && !bar.is_dismissed())
        {
            return false;
        }
        self.remove_info_bar(kind.slot());
        let ms = anim.scaled_duration_ms(duration::FAST);
        let entrance = Timed::new(now, ms, Curve::DecelerateMax);
        self.info_bars.push_back(InfoBar::new(kind, now, entrance));
        true
    }

    /// Drop the bar occupying `slot` outright, if any (UI/UX v3 P6).
    ///
    /// No exit animation: this is slot replacement, where the bar that
    /// replaces it is drawn in the same place on the same frame, and fading
    /// one out under the other would only smear the two messages together.
    /// The user-visible path is [`ClientState::dismiss_info_bar`].
    /// Returns whether one was removed.
    pub fn remove_info_bar(&mut self, slot: InfoBarSlot) -> bool {
        let before = self.info_bars.len();
        self.info_bars.retain(|bar| bar.kind.slot() != slot);
        self.info_bars.len() != before
    }

    /// Start the exit of the bar occupying `slot` (UI/UX v3 P6d).
    ///
    /// The bar keeps its place in the stack while it draws out — it is the
    /// renderer's only record that it was ever there — and
    /// [`ClientState::retire_info_bars`] drops it once the exit finishes.
    /// Everything else treats it as gone from this moment (`is_dismissed`).
    /// Returns whether a bar started its exit.
    pub fn dismiss_info_bar(
        &mut self,
        slot: InfoBarSlot,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) -> bool {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::FASTER);
        let Some(bar) = self
            .info_bars
            .iter_mut()
            .find(|bar| bar.kind.slot() == slot && !bar.is_dismissed())
        else {
            return false;
        };
        // Resume from what is on screen, so a bar dismissed mid-entrance
        // fades out from where it got to rather than snapping to opaque.
        let visibility = bar.visibility(now);
        bar.exit = Some(Timed::resuming_at(
            now,
            1.0 - visibility,
            ms,
            Curve::AccelerateMax,
        ));
        true
    }

    /// Dismiss every bar whose auto-dismiss deadline has passed (D3).
    ///
    /// Only the info severity has a deadline at all, so in practice this is
    /// the update notice retiring itself after `INFO_BAR_TTL`. Returns
    /// whether anything started its exit, so the caller can ask for a frame.
    pub fn expire_info_bars(
        &mut self,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) -> bool {
        let expired: Vec<InfoBarSlot> = self
            .info_bars
            .iter()
            .filter(|bar| !bar.is_dismissed() && bar.is_expired(now))
            .map(|bar| bar.kind.slot())
            .collect();
        // A plain loop rather than `any`, which short-circuits: every expired
        // bar has to be dismissed, not just the first one found.
        let mut changed = false;
        for slot in expired {
            changed |= self.dismiss_info_bar(slot, now, anim);
        }
        changed
    }

    /// Drop the bars whose exit has finished drawing (UI/UX v3 P6d).
    ///
    /// The counterpart of the `retire` calls the other overlay surfaces make
    /// in `lifecycle.rs`, and the reason `has_active_animation` goes quiet
    /// again once a bar is gone (G-idle).
    pub fn retire_info_bars(&mut self, now: Instant) -> bool {
        let before = self.info_bars.len();
        self.info_bars.retain(|bar| !bar.is_retired(now));
        self.info_bars.len() != before
    }

    /// Whether a bar the user can still see-and-act-on occupies `slot`.
    ///
    /// A dismissed bar does not count: the callers use this to decide whether
    /// to raise a bar, and one that is fading out should not suppress the
    /// next one.
    pub fn has_info_bar(&self, slot: InfoBarSlot) -> bool {
        self.info_bars
            .iter()
            .any(|bar| bar.kind.slot() == slot && !bar.is_dismissed())
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

    /// Rebuild the palette's dynamic workspace actions from
    /// [`workspaces`](Self::workspaces) (roadmap Phase 3). Called whenever
    /// the workspace set or the active workspace changes.
    pub fn refresh_workspace_palette_actions(&mut self) {
        let actions = crate::palette::build_workspace_actions(&self.workspaces);
        self.palette.set_workspace_actions(actions);
    }

    /// Cursor icon requested by the focused pane via OSC 22, or the platform
    /// default when no pane is focused / no override is active. Applied by
    /// the mouse handler while hovering the grid area.
    pub fn focused_pane_pointer_icon(&self) -> winit::window::CursorIcon {
        self.focused_pane()
            .and_then(|p| p.pointer_shape.as_deref())
            .map(pointer_shape_to_cursor_icon)
            .unwrap_or(winit::window::CursorIcon::Default)
    }

    /// Toggle the command palette
    pub fn toggle_palette(&mut self, now: Instant, anim: &nexterm_config::AnimationsConfig) {
        if self.palette.is_open {
            self.palette.close(now, anim);
        } else {
            self.palette.open(now, anim);
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

#[cfg(test)]
mod animation_frame_tests {
    use super::*;
    use std::time::Duration;

    /// The UI/UX v3 P3a acceptance criterion in test form: a state with
    /// nothing animating must not ask for a frame. If this ever returns
    /// true at rest, the event loop spins at 60 fps and every pane-vertex
    /// cache miss that follows is a regression P3a introduced.
    #[test]
    fn an_idle_state_wants_no_animation_frames() {
        let state = ClientState::new(80, 24, 1000);
        assert!(!state.has_active_animation(std::time::Instant::now(), 250));
    }

    /// Hovering a settings row must ask for frames until the cross-fade
    /// finishes, and stop afterwards.
    #[test]
    fn a_hovered_settings_row_wants_animation_frames_until_it_settles() {
        use crate::renderer::overlay::widgets::spec::WidgetId;

        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        let id = WidgetId::new(2, 0);

        state
            .settings_panel
            .hover_transition
            .retarget(Some(id), t0, &anim);
        assert!(state.has_active_animation(t0, 200));
        assert!(
            state.settings_panel.hover_transition.weight(id, t0).abs() < 1e-3,
            "the fade starts from nothing"
        );

        let done = t0 + Duration::from_millis(100);
        assert!((state.settings_panel.hover_transition.weight(id, done) - 1.0).abs() < 1e-3);
        assert!(!state.has_active_animation(done, 200));
    }

    /// The wiring's own contract: a decaying pulse must keep the frame loop
    /// awake, or a settings row would freeze mid-press until some other
    /// event happened to request a redraw. The pulse's own timing is covered
    /// by `animations::press`; this pins that `ClientState` consults it.
    #[test]
    fn a_settings_row_press_keeps_the_frame_loop_awake() {
        use crate::renderer::overlay::widgets::spec::WidgetId;

        let mut state = ClientState::new(80, 24, 1000);
        let t0 = Instant::now();
        let id = WidgetId::new(2, 0);
        state.settings_panel.press_pulse.press(
            id,
            t0,
            &nexterm_config::AnimationsConfig::default(),
        );
        assert!(state.has_active_animation(t0, 200));
        assert!(!state.has_active_animation(t0 + Duration::from_millis(100), 200));
    }

    /// Dismissing the settings panel (e.g. Esc) while a row is hovered must
    /// retarget the cross-fade to `None`, not just clear `hover_widget`.
    /// Otherwise the fade-out that should start immediately would instead
    /// wait for the next pointer move, and `has_active_animation` would stay
    /// true for a panel that is already closed and drawing nothing. This
    /// pins the state-level property the closed-panel branch in
    /// `renderer/event_handler/mouse.rs` relies on: retargeting to `None`
    /// settles the transition on the normal schedule rather than leaving it
    /// running forever.
    #[test]
    fn retargeting_settings_hover_to_none_settles_the_transition() {
        use crate::renderer::overlay::widgets::spec::WidgetId;

        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        let id = WidgetId::new(2, 0);

        state
            .settings_panel
            .hover_transition
            .retarget(Some(id), t0, &anim);
        assert!(state.has_active_animation(t0, 200));

        // Simulate the panel-closed retarget: point the transition at
        // `None` while the previous item is still fading in.
        let closed_at = t0 + Duration::from_millis(20);
        state
            .settings_panel
            .hover_transition
            .retarget(None, closed_at, &anim);
        assert!(
            state.has_active_animation(closed_at, 200),
            "the outgoing item must still fade out, not vanish instantly"
        );

        let done = closed_at + Duration::from_millis(100);
        assert!(state.settings_panel.hover_transition.weight(id, done).abs() < 1e-3);
        assert!(
            !state.has_active_animation(done, 200),
            "a transition retargeted to None must settle, not run forever"
        );
    }

    /// The menu's hover cross-fade is independent of the widget layer's:
    /// moving the pointer from a settings row into a context menu runs both
    /// at once, which is why each model owns its own transition.
    #[test]
    fn a_hovered_context_menu_item_wants_animation_frames_until_it_settles() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.context_menu = Some(ContextMenu::new_default(10.0, 10.0, &[]));

        let menu = state
            .context_menu
            .as_mut()
            .expect("the menu was just assigned");
        menu.hovered = Some(1);
        menu.hover_transition.retarget(Some(1), t0, &anim);
        assert!(state.has_active_animation(t0, 200));

        let done = t0 + Duration::from_millis(100);
        let menu = state.context_menu.as_ref().expect("still open");
        assert!((menu.hover_transition.weight(1, done) - 1.0).abs() < 1e-3);
        assert!(menu.hover_transition.weight(0, done).abs() < 1e-4);
        assert!(!state.has_active_animation(done, 200));
    }

    /// The context menu is the one model whose click commits on release, so its
    /// pulse is what the user sees for as long as the button is held. It lives
    /// inside `ContextMenu` and dies with it — P3b1's closing ghost is a
    /// separate clone and deliberately does not carry it.
    #[test]
    fn a_context_menu_press_keeps_the_frame_loop_awake() {
        let mut state = ClientState::new(80, 24, 1000);
        let t0 = Instant::now();
        // Assigned directly rather than through `show_context_menu`, which would
        // also start the open animation and make the assertion below pass for
        // the wrong reason. This is how the hover test beside it builds a menu.
        state.context_menu = Some(ContextMenu::new_default(10.0, 10.0, &[]));
        let menu = state
            .context_menu
            .as_mut()
            .expect("the menu was just assigned");
        menu.press_pulse
            .press(1, t0, &nexterm_config::AnimationsConfig::default());
        assert!(state.has_active_animation(t0, 200));
        assert!(!state.has_active_animation(t0 + Duration::from_millis(100), 200));
    }

    /// Hovering a tab must ask for frames until the cross-fade finishes, and
    /// stop afterwards.
    #[test]
    fn a_hovered_tab_wants_animation_frames_until_it_settles() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();

        state.tab_hover.retarget(Some(7), t0, &anim);
        assert!(state.has_active_animation(t0, 200));
        assert!(
            state.tab_hover.weight(7, t0).abs() < 1e-3,
            "the fade starts from nothing"
        );

        let done = t0 + Duration::from_millis(100);
        assert!((state.tab_hover.weight(7, done) - 1.0).abs() < 1e-3);
        assert!(!state.has_active_animation(done, 200));
    }

    /// The wiring's own contract: a decaying pulse must keep the frame loop
    /// awake, or the tab would freeze mid-press until some other event
    /// happened to request a redraw. The pulse's own timing is covered by
    /// `animations::press`; this pins that `ClientState` consults it.
    #[test]
    fn a_tab_press_keeps_the_frame_loop_awake() {
        let mut state = ClientState::new(80, 24, 1000);
        let t0 = Instant::now();
        assert!(!state.has_active_animation(t0, 200));
        state
            .tab_press
            .press(7, t0, &nexterm_config::AnimationsConfig::default());
        assert!(state.has_active_animation(t0, 200));
        let done = t0 + Duration::from_millis(100);
        assert!(!state.has_active_animation(done, 200));
    }

    /// Leaving the tab bar must fade the last tab out rather than snapping —
    /// the same property `HoverTransition` guarantees for the other models.
    #[test]
    fn leaving_the_tab_bar_fades_the_last_tab_out() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.tab_hover.retarget(Some(7), t0, &anim);
        let settled = t0 + Duration::from_millis(100);

        state.tab_hover.retarget(None, settled, &anim);
        let mid = settled + Duration::from_millis(50);
        let w = state.tab_hover.weight(7, mid);
        assert!(w > 0.1 && w < 0.9, "must still be tinted while fading: {w}");
        assert!(state.has_active_animation(mid, 200));

        let done = settled + Duration::from_millis(100);
        assert!(state.tab_hover.weight(7, done).abs() < 1e-3);
        assert!(!state.has_active_animation(done, 200));
    }

    /// The window buttons are the fourth and last hover model. Their fade is
    /// driven from two places — the pointer-motion handler and the Windows
    /// snap-layout event — so the state-level property is what the test can
    /// pin; the two call sites are checked by review.
    #[test]
    fn a_hovered_window_button_wants_animation_frames_until_it_settles() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        let close = crate::state::WindowButton::Close;

        state.window_button_hover.retarget(Some(close), t0, &anim);
        assert!(state.has_active_animation(t0, 200));
        assert!(state.window_button_hover.weight(close, t0).abs() < 1e-3);
        assert!(
            state
                .window_button_hover
                .weight(crate::state::WindowButton::Minimize, t0)
                .abs()
                < 1e-4,
            "an unhovered button weighs nothing"
        );

        let done = t0 + Duration::from_millis(100);
        assert!((state.window_button_hover.weight(close, done) - 1.0).abs() < 1e-3);
        assert!(!state.has_active_animation(done, 200));
    }

    /// Maximize is the only one of the three whose pulse is ever seen —
    /// Minimize and Close remove the window first — but all three are wired,
    /// so all three must keep the frame loop awake while they decay.
    #[test]
    fn a_window_button_press_keeps_the_frame_loop_awake() {
        let mut state = ClientState::new(80, 24, 1000);
        let t0 = Instant::now();
        state.window_button_press.press(
            crate::state::WindowButton::Maximize,
            t0,
            &nexterm_config::AnimationsConfig::default(),
        );
        assert!(state.has_active_animation(t0, 200));
        assert!(!state.has_active_animation(t0 + Duration::from_millis(100), 200));
    }

    /// Moving between two buttons cross-fades them rather than snapping.
    #[test]
    fn moving_between_window_buttons_cross_fades_them() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        let (min, max) = (
            crate::state::WindowButton::Minimize,
            crate::state::WindowButton::Maximize,
        );

        state.window_button_hover.retarget(Some(min), t0, &anim);
        let settled = t0 + Duration::from_millis(100);
        state
            .window_button_hover
            .retarget(Some(max), settled, &anim);

        let mid = settled + Duration::from_millis(50);
        let (w_min, w_max) = (
            state.window_button_hover.weight(min, mid),
            state.window_button_hover.weight(max, mid),
        );
        assert!(w_min > 0.1 && w_min < 0.9, "outgoing mid-fade: {w_min}");
        assert!(w_max > 0.1 && w_max < 0.9, "incoming mid-fade: {w_max}");
    }

    /// P3b's acceptance criterion: a state with nothing animating must not
    /// ask for frames. Eleven surfaces now have a clause in the aggregate,
    /// and each is a way for this to regress.
    #[test]
    fn a_fully_idle_state_wants_no_animation_frames() {
        let state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        assert!(!state.has_active_animation(now, 200));
        assert!(!state.has_active_animation(now, 0));
    }

    #[test]
    fn a_running_spring_wants_animation_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        state.animations.record_tab_switch(7, now);
        assert!(state.has_active_animation(now, 250));
    }

    #[test]
    fn a_settled_spring_wants_no_more_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        state.animations.record_tab_switch(7, now);
        for _ in 0..600 {
            state.animations.tick_by_dt(0.016);
        }
        assert!(!state.has_active_animation(now, 250));
    }

    /// With animations disabled the scaled duration is 0, and nothing may
    /// ask for a frame on their behalf.
    #[test]
    fn disabled_animations_want_no_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        state.animations.record_pane_added(1, now);
        assert!(!state.has_active_animation(now, 0));
    }

    #[test]
    fn an_opening_settings_panel_wants_animation_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        state
            .settings_panel
            .open(now, &nexterm_config::AnimationsConfig::default());
        assert!(state.has_active_animation(now, 250));
        let done = now + std::time::Duration::from_millis(200);
        assert!(!state.has_active_animation(done, 250));
    }

    #[test]
    fn a_closing_settings_panel_wants_animation_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let t0 = std::time::Instant::now();
        let anim = nexterm_config::AnimationsConfig::default();
        state.settings_panel.open(t0, &anim);
        // Close only after the entrance has finished — closing at 0
        // visibility yields an exit that is born done and proves nothing.
        let opened = t0 + std::time::Duration::from_millis(200);
        state.settings_panel.close(opened, &anim);
        assert!(state.has_active_animation(opened, 250));
        let done = opened + std::time::Duration::from_millis(150);
        assert!(!state.has_active_animation(done, 250));
    }

    /// The three large panels share one shape: the logical flag closes at
    /// once, the surface stays visible while it fades, and the frame loop
    /// wants frames for the whole transition.
    #[test]
    fn a_closing_command_palette_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.palette.open(t0, &anim);
        assert!(state.palette.is_open);
        assert!(state.has_active_animation(t0, 200));

        let opened = t0 + Duration::from_millis(300);
        state.palette.close(opened, &anim);
        assert!(!state.palette.is_open, "input must see it as closed");
        assert!(state.palette.motion.is_visible(), "renderer keeps drawing");
        assert!(state.has_active_animation(opened, 200));

        let done = opened + Duration::from_millis(150);
        state.palette.motion.retire(done);
        assert!(!state.palette.motion.is_visible());
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_closing_macro_picker_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.macro_picker.open(t0, &anim);
        let opened = t0 + Duration::from_millis(300);
        state.macro_picker.close(opened, &anim);
        assert!(!state.macro_picker.is_open);
        assert!(state.macro_picker.motion.is_visible());
        assert!(state.has_active_animation(opened, 200));
        let done = opened + Duration::from_millis(150);
        state.macro_picker.motion.retire(done);
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_closing_host_manager_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.host_manager.open(t0, &anim);
        let opened = t0 + Duration::from_millis(300);
        state.host_manager.close(opened, &anim);
        assert!(!state.host_manager.is_open);
        assert!(state.host_manager.motion.is_visible());
        assert!(state.has_active_animation(opened, 200));
        let done = opened + Duration::from_millis(150);
        state.host_manager.motion.retire(done);
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_closing_block_name_modal_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.block_name_modal.open_for(1, Some("build"), t0, &anim);
        let opened = t0 + Duration::from_millis(300);
        state.block_name_modal.close(opened, &anim);
        assert!(!state.block_name_modal.is_open);
        assert!(state.block_name_modal.motion.is_visible());
        assert!(state.has_active_animation(opened, 200));
        let done = opened + Duration::from_millis(150);
        state.block_name_modal.motion.retire(done);
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_closing_file_transfer_dialog_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.file_transfer.open(t0, &anim);
        assert!(state.file_transfer.is_open);
        let opened = t0 + Duration::from_millis(300);
        state.file_transfer.close(opened, &anim);
        assert!(!state.file_transfer.is_open);
        assert!(state.file_transfer.motion.is_visible());
        assert!(state.has_active_animation(opened, 200));
        let done = opened + Duration::from_millis(150);
        state.file_transfer.motion.retire(done);
        assert!(!state.has_active_animation(done, 200));
    }

    /// An `Option`-shaped surface must leave the live field `None` the
    /// instant it is dismissed — nothing can be clicked during the fade —
    /// while the ghost keeps the renderer supplied with content.
    #[test]
    fn a_dismissed_context_menu_leaves_a_ghost_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.context_menu = Some(ContextMenu::new_default(10.0, 10.0, &[]));

        state.dismiss_context_menu(t0, &anim);
        assert!(state.context_menu.is_none(), "input must see it as gone");
        assert!(
            state.context_menu_closing.is_some(),
            "renderer keeps drawing"
        );
        assert!(state.has_active_animation(t0, 200));

        let done = t0 + Duration::from_millis(100);
        state.retire_ghosts(done);
        assert!(state.context_menu_closing.is_none());
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_dismissed_close_window_dialog_leaves_a_ghost_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.close_window_dialog = Some(CloseWindowDialog {
            server_window_id: 1,
            message: "close?".to_string(),
            kill_label: "Close".to_string(),
            cancel_label: "Cancel".to_string(),
            selected_button: 0,
        });

        state.dismiss_close_window_dialog(t0, &anim);
        assert!(state.close_window_dialog.is_none());
        assert!(state.close_window_dialog_closing.is_some());
        assert!(state.has_active_animation(t0, 200));

        let done = t0 + Duration::from_millis(150);
        state.retire_ghosts(done);
        assert!(state.close_window_dialog_closing.is_none());
        assert!(!state.has_active_animation(done, 200));
    }

    /// Dismissing twice in a row must not resurrect the first ghost.
    #[test]
    fn dismissing_an_absent_context_menu_is_a_no_op() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.dismiss_context_menu(t0, &anim);
        assert!(state.context_menu_closing.is_none());
        assert!(!state.has_active_animation(t0, 200));
    }

    /// The security-relevant property: a consent dialog that is fading out
    /// is no longer answerable. `pending_consent` is what every input path
    /// consults, and it is `None` from the instant the user answered.
    #[test]
    fn a_fading_consent_dialog_cannot_be_answered() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.show_consent_dialog(
            ConsentDialog::new(ConsentKind::OpenUrl("https://example.invalid".to_string())),
            t0,
            &anim,
        );
        assert!(state.pending_consent.is_some());

        state.dismiss_consent_dialog(t0, &anim);
        assert!(
            state.pending_consent.is_none(),
            "no input path may see an answerable dialog during the fade"
        );
        assert!(state.pending_consent_closing.is_some());
        assert!(state.has_active_animation(t0, 200));

        let done = t0 + Duration::from_millis(150);
        state.retire_ghosts(done);
        assert!(state.pending_consent_closing.is_none());
        assert!(!state.has_active_animation(done, 200));
    }
}

#[cfg(test)]
mod info_bar_tests {
    //! UI/UX v3 P6b: the three banner fields became one stack, so the
    //! single-slot behaviour they each had by construction is now a property
    //! of `push_info_bar` and has to be asserted.
    use super::*;
    use std::time::Duration;

    /// Motion at its configured default, so a bar is born mid-entrance —
    /// the state the wiring has to survive. Tests that need the
    /// reduced-motion path build their own `AnimationsConfig`.
    fn anim() -> nexterm_config::AnimationsConfig {
        nexterm_config::AnimationsConfig::default()
    }

    fn error(message: &str) -> InfoBarKind {
        InfoBarKind::ServerError {
            message: message.to_string(),
        }
    }

    #[test]
    fn a_second_error_replaces_the_first_rather_than_stacking() {
        let now = Instant::now();
        let mut state = ClientState::new(80, 24, 1000);

        assert!(state.push_info_bar(error("pty launch failed"), now, &anim(),));
        assert!(state.push_info_bar(
            error("config load failed"),
            now + Duration::from_secs(1),
            &anim(),
        ));

        assert_eq!(state.info_bars.len(), 1);
        assert_eq!(state.info_bars[0].kind, error("config load failed"));
    }

    /// Re-pushing an identical bar is what the update poller does on every
    /// tick; it must not restart the bar's clock or report a change.
    #[test]
    fn re_pushing_the_same_bar_is_a_no_op() {
        let now = Instant::now();
        let mut state = ClientState::new(80, 24, 1000);
        assert!(state.push_info_bar(error("boom"), now, &anim(),));

        assert!(!state.push_info_bar(error("boom"), now + Duration::from_secs(5), &anim(),));
        assert_eq!(state.info_bars.len(), 1);
        assert_eq!(state.info_bars[0].created_at, now);
    }

    #[test]
    fn bars_of_different_slots_coexist() {
        let now = Instant::now();
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(error("boom"), now, &anim());
        state.push_info_bar(InfoBarKind::Offline { since: now }, now, &anim());
        state.push_info_bar(
            InfoBarKind::UpdateAvailable {
                version: "1.9.0".to_string(),
            },
            now,
            &anim(),
        );

        assert_eq!(state.info_bars.len(), 3);
        assert!(state.has_info_bar(InfoBarSlot::Update));
        assert!(state.has_info_bar(InfoBarSlot::Offline));
        assert!(state.has_info_bar(InfoBarSlot::ServerError));
    }

    /// The offline bar clears on a successful connect and must take nothing
    /// else with it — the old code cleared a field, this clears a slot.
    #[test]
    fn removing_one_slot_leaves_the_others_standing() {
        let now = Instant::now();
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(InfoBarKind::Offline { since: now }, now, &anim());
        state.push_info_bar(error("boom"), now, &anim());

        assert!(state.remove_info_bar(InfoBarSlot::Offline));
        assert!(!state.remove_info_bar(InfoBarSlot::Offline));
        assert!(!state.has_info_bar(InfoBarSlot::Offline));
        assert!(state.has_info_bar(InfoBarSlot::ServerError));
    }

    #[test]
    fn a_fresh_state_has_no_bars() {
        let state = ClientState::new(80, 24, 1000);
        assert!(state.info_bars.is_empty());
        assert!(!state.has_info_bar(InfoBarSlot::Update));
    }

    /// UI/UX v3 P6d. A dismissed bar stays in the stack so the renderer can
    /// draw it leaving, but it is out of everything else at once — which is
    /// what lets the same message be raised again while the old one is still
    /// fading, instead of being swallowed as a duplicate.
    #[test]
    fn a_dismissed_bar_is_drawn_out_but_counts_as_gone() {
        let now = Instant::now();
        let shown = now + Duration::from_secs(1);
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(error("boom"), now, &anim());

        assert!(state.dismiss_info_bar(InfoBarSlot::ServerError, shown, &anim()));
        assert!(!state.dismiss_info_bar(InfoBarSlot::ServerError, shown, &anim()));

        assert_eq!(state.info_bars.len(), 1, "still drawn while it fades");
        assert!(state.info_bars[0].is_dismissed());
        assert!(!state.has_info_bar(InfoBarSlot::ServerError));

        // The repeat is news, not a duplicate — and it takes the slot from
        // the ghost rather than stacking on top of it.
        assert!(state.push_info_bar(error("boom"), shown, &anim()));
        assert_eq!(state.info_bars.len(), 1);
        assert!(!state.info_bars[0].is_dismissed());
    }

    /// Dismissing a bar that is still fading in has nothing to fade out, so
    /// it leaves immediately rather than fading from a visibility it never
    /// reached — the continuity rule `Timed::resuming_at` encodes.
    #[test]
    fn a_bar_dismissed_before_it_appears_leaves_at_once() {
        let now = Instant::now();
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(error("boom"), now, &anim());

        state.dismiss_info_bar(InfoBarSlot::ServerError, now, &anim());
        assert!(state.retire_info_bars(now));
        assert!(state.info_bars.is_empty());
    }

    /// The retire path. A bar that animates but never leaves keeps the event
    /// loop requesting frames forever, which is the P3b1 failure mode
    /// (G-idle).
    #[test]
    fn a_bar_leaves_the_stack_once_its_exit_has_finished() {
        let now = Instant::now();
        let shown = now + Duration::from_secs(1);
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(error("boom"), now, &anim());
        state.dismiss_info_bar(InfoBarSlot::ServerError, shown, &anim());

        assert!(!state.retire_info_bars(shown));
        let settled = shown + Duration::from_secs(1);
        assert!(state.retire_info_bars(settled));
        assert!(state.info_bars.is_empty());
        assert!(!state.retire_info_bars(settled));
    }

    /// G-idle: a bar that is merely sitting there — including one counting
    /// down its 20 s deadline — must not ask for frames, or the update notice
    /// alone would keep the GPU awake for the whole timeout.
    #[test]
    fn only_a_moving_bar_asks_for_frames() {
        let now = Instant::now();
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(
            InfoBarKind::UpdateAvailable {
                version: "1.9.0".to_string(),
            },
            now,
            &anim(),
        );

        assert!(state.has_active_animation(now, 200));
        let settled = now + Duration::from_secs(1);
        assert!(!state.has_active_animation(settled, 200));

        state.dismiss_info_bar(InfoBarSlot::Update, settled, &anim());
        assert!(state.has_active_animation(settled, 200));
        let gone = settled + Duration::from_secs(1);
        assert!(!state.has_active_animation(gone, 200));
    }

    /// The reduced-motion path: with animations off every `Timed` is born
    /// finished, so a bar never asks for a frame and its dismissal retires on
    /// the same tick.
    #[test]
    fn motion_off_makes_a_bar_appear_and_leave_without_a_single_extra_frame() {
        let now = Instant::now();
        let mut off = nexterm_config::AnimationsConfig::default();
        off.enabled = nexterm_config::AnimationsEnabled::No;
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(error("boom"), now, &off);

        assert!(!state.has_active_animation(now, 0));
        state.dismiss_info_bar(InfoBarSlot::ServerError, now, &off);
        assert!(!state.has_active_animation(now, 0));
        assert!(state.retire_info_bars(now));
        assert!(state.info_bars.is_empty());
    }

    /// D3: the info severity dismisses itself after `INFO_BAR_TTL`; the
    /// warning and error severities never do, however long they sit there.
    #[test]
    fn only_the_informational_bar_dismisses_itself() {
        use crate::renderer::overlay::infobar::INFO_BAR_TTL;

        let now = Instant::now();
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(
            InfoBarKind::UpdateAvailable {
                version: "1.9.0".to_string(),
            },
            now,
            &anim(),
        );
        state.push_info_bar(InfoBarKind::Offline { since: now }, now, &anim());
        state.push_info_bar(error("boom"), now, &anim());

        assert!(!state.expire_info_bars(now + Duration::from_secs(1), &anim()));

        let after_ttl = now + INFO_BAR_TTL + Duration::from_secs(1);
        assert!(state.expire_info_bars(after_ttl, &anim()));
        // A second pass finds nothing new: the bar it dismissed is already on
        // its way out, and the other two have no deadline at all.
        assert!(!state.expire_info_bars(after_ttl, &anim()));

        state.retire_info_bars(after_ttl + Duration::from_secs(1));
        assert!(!state.has_info_bar(InfoBarSlot::Update));
        assert!(state.has_info_bar(InfoBarSlot::Offline));
        assert!(state.has_info_bar(InfoBarSlot::ServerError));
    }

    /// Slot replacement is deliberately abrupt: the replacing bar is drawn in
    /// the same place on the same frame, so fading the old one out under it
    /// would only smear the two messages together.
    #[test]
    fn replacing_a_slot_drops_the_old_bar_outright() {
        let now = Instant::now();
        let mut state = ClientState::new(80, 24, 1000);
        state.push_info_bar(error("first"), now, &anim());
        state.push_info_bar(error("second"), now, &anim());

        assert_eq!(state.info_bars.len(), 1);
        assert!(!state.info_bars[0].is_dismissed());
    }
}
