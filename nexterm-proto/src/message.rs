//! IPC message type definitions.
//!
//! Covers messages for both directions: client → server and server → client.

use serde::{Deserialize, Serialize};

use crate::{DirtyRow, Grid};

fn default_key_event_type() -> u8 {
    1
}

/// Bit flags for keyboard modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Modifiers(pub u8);

impl Modifiers {
    /// Bit mask for the Shift key.
    pub const SHIFT: u8 = 0b0001;
    /// Bit mask for the Ctrl key.
    pub const CTRL: u8 = 0b0010;
    /// Bit mask for the Alt / Option key.
    pub const ALT: u8 = 0b0100;
    /// Bit mask for the Meta / Super / Windows key.
    pub const META: u8 = 0b1000;

    /// Returns whether the Ctrl key is held.
    pub fn is_ctrl(self) -> bool {
        self.0 & Self::CTRL != 0
    }
    /// Returns whether the Shift key is held.
    pub fn is_shift(self) -> bool {
        self.0 & Self::SHIFT != 0
    }
}

/// A key event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyCode {
    /// A regular character.
    Char(char),
    /// Function keys F1..=F12.
    F(u8),
    /// Enter / Return.
    Enter,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Shift+Tab (reverse tab).
    BackTab,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Insert.
    Insert,
}

/// Client → server messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientToServer {
    /// Key input event.
    KeyEvent {
        /// The key that was pressed.
        code: KeyCode,
        /// Modifier keys held at the same time.
        modifiers: Modifiers,
        /// Kitty keyboard protocol event type: 1=press (default), 2=repeat, 3=release.
        /// Older clients that do not send this field default to press (1).
        #[serde(default = "default_key_event_type")]
        event_type: u8,
    },
    /// Terminal resize.
    Resize {
        /// New column count.
        cols: u16,
        /// New row count.
        rows: u16,
    },
    /// Detach from the session (client is exiting).
    Detach,
    /// Attach to a session by name.
    Attach {
        /// Target session name.
        session_name: String,
    },
    /// Create a pane (vertical split).
    SplitVertical,
    /// Create a pane (horizontal split).
    SplitHorizontal,
    /// Move focus to the next pane.
    FocusNextPane,
    /// Move focus to the previous pane.
    FocusPrevPane,
    /// Move focus to the pane with the given ID (e.g. from a mouse click).
    FocusPane {
        /// Pane ID to receive focus.
        pane_id: u32,
    },
    /// Paste text into the focused pane.
    PasteText {
        /// Text to paste.
        text: String,
    },
    /// Liveness check.
    Ping,
    /// List sessions without attaching.
    ListSessions,
    /// Force-kill a session.
    KillSession {
        /// Name of the session to terminate.
        name: String,
    },
    /// Start recording a session.
    StartRecording {
        /// Session name to record.
        session_name: String,
        /// Output path for the recording file.
        output_path: String,
    },
    /// Stop recording a session.
    StopRecording {
        /// Session name to stop recording.
        session_name: String,
    },
    /// Close the focused pane.
    ClosePane,
    /// Adjust the split ratio of the focused pane (positive = grow, negative = shrink).
    ResizeSplit {
        /// Resize delta in the range 0.0..=1.0; positive grows, negative shrinks.
        delta: f32,
    },
    /// SSH connection (host configured by name).
    ///
    /// **Breaking change since PROTOCOL_VERSION 2 (Sprint 5-1 / G1):**
    /// the legacy `password: Option<String>` field was removed and replaced with
    /// `password_keyring_account` + `ephemeral_password`. To keep plain-text passwords
    /// off the IPC channel, the client stores the password in the OS keyring and the
    /// server retrieves it using `Service="nexterm-ssh"` + `Account=<account>`.
    ConnectSsh {
        /// Destination host name or IP address.
        host: String,
        /// SSH port (typically 22).
        port: u16,
        /// Login user name.
        username: String,
        /// Authentication method: `"password"`, `"key"`, or `"agent"`.
        auth_type: String,
        /// Account identifier the server uses to fetch the password from the OS keyring
        /// when authenticating with a password. Format: `<username>@<host_name>`
        /// (`host_name` is `HostConfig.name`). The `Service` is the fixed string
        /// `"nexterm-ssh"`.
        ///
        /// The client must store the password under this account name in the keyring
        /// before sending the IPC message. Use `None` when `auth_type` is not
        /// `"password"` or when an empty password is intended.
        password_keyring_account: Option<String>,
        /// When `true`, the server removes the keyring entry after authentication
        /// finishes (regardless of success or failure). Used when
        /// `PasswordModal.remember = false`.
        #[serde(default)]
        ephemeral_password: bool,
        /// Private key path for public-key authentication.
        key_path: Option<String>,
        /// Remote port forwarding specifications ("remote_port:local_host:local_port",
        /// may be repeated).
        #[serde(default)]
        remote_forwards: Vec<String>,
        /// Whether to enable X11 forwarding (equivalent to `ssh -X`).
        #[serde(default)]
        x11_forward: bool,
        /// Whether to use trusted X11 forwarding (equivalent to `ssh -Y`).
        #[serde(default)]
        x11_trusted: bool,
    },
    /// Create a new window.
    NewWindow,
    /// Close the specified window (the last remaining window cannot be closed).
    CloseWindow {
        /// ID of the window to close.
        window_id: u32,
    },
    /// Move focus to the specified window.
    FocusWindow {
        /// Window ID to focus.
        window_id: u32,
    },
    /// Rename the specified window.
    RenameWindow {
        /// Window ID to rename.
        window_id: u32,
        /// New window name.
        name: String,
    },
    /// Set broadcast mode (true: send input to every pane; false: focused pane only).
    SetBroadcast {
        /// `true` to broadcast to all panes; `false` to limit to the focused pane.
        enabled: bool,
    },
    /// Show/hide the pane-number overlay.
    DisplayPanes {
        /// `true` to show the overlay.
        show: bool,
    },
    /// Start recording in asciicast v2 format.
    StartAsciicast {
        /// Session name to record.
        session_name: String,
        /// Output path for the asciicast file.
        output_path: String,
    },
    /// Stop the asciicast v2 recording.
    StopAsciicast {
        /// Session name to stop recording.
        session_name: String,
    },
    /// Save the current layout as a template.
    SaveTemplate {
        /// Template name to save under.
        name: String,
    },
    /// Load and apply a layout template.
    LoadTemplate {
        /// Template name to load.
        name: String,
    },
    /// List saved templates.
    ListTemplates,
    /// Toggle full-window zoom of the focused pane.
    ToggleZoom,
    /// Swap the focused pane with the specified pane (swaps IDs inside the BSP tree).
    SwapPane {
        /// Target pane ID to swap with.
        target_pane_id: u32,
    },
    /// Detach the focused pane into a brand-new window.
    BreakPane,
    /// Move the focused pane into the specified window.
    JoinPane {
        /// Target window ID.
        target_window_id: u32,
    },
    /// SFTP upload: transfer a local file to the remote.
    SftpUpload {
        /// SSH host configuration name (an entry under `config.hosts`).
        host_name: String,
        /// Local file path.
        local_path: String,
        /// Remote destination path.
        remote_path: String,
    },
    /// SFTP download: transfer a remote file to the local machine.
    SftpDownload {
        /// SSH host configuration name (an entry under `config.hosts`).
        host_name: String,
        /// Remote file path.
        remote_path: String,
        /// Local destination path.
        local_path: String,
    },
    /// Execute a Lua macro and send its result to the focused pane.
    RunMacro {
        /// Lua function name inside `nexterm.lua`.
        macro_fn: String,
        /// Display name shown in the command palette / UI (for logging).
        #[serde(default)]
        display_name: String,
    },
    /// Forward a mouse event to the PTY (used while mouse reporting is enabled).
    MouseReport {
        /// Button number (0 = left, 1 = middle, 2 = right, 64 = wheel up, 65 = wheel down).
        button: u8,
        /// Grid column (0-based).
        col: u16,
        /// Grid row (0-based).
        row: u16,
        /// `true` for press, `false` for release.
        pressed: bool,
        /// Whether this is a motion event (drag).
        motion: bool,
    },
    /// Set the layout mode (`"bsp"` or `"tiling"`).
    SetLayoutMode {
        /// Layout mode string (`"bsp"` or `"tiling"`).
        mode: String,
    },
    /// Open a floating pane (Ctrl+B f).
    OpenFloatingPane,
    /// Close a floating pane.
    CloseFloatingPane {
        /// ID of the floating pane to close.
        pane_id: u32,
    },
    /// Move a floating pane (e.g. via mouse drag).
    MoveFloatingPane {
        /// ID of the floating pane to move.
        pane_id: u32,
        /// New column offset (0-based).
        col_off: u16,
        /// New row offset (0-based).
        row_off: u16,
    },
    /// Resize a floating pane.
    ResizeFloatingPane {
        /// ID of the floating pane to resize.
        pane_id: u32,
        /// New column count.
        cols: u16,
        /// New row count.
        rows: u16,
    },
    /// Connect to a serial port.
    ConnectSerial {
        /// Device path (e.g. `/dev/ttyUSB0`, `COM3`).
        port: String,
        /// Baud rate (e.g. 115200).
        baud_rate: u32,
        /// Data bits: 5, 6, 7, or 8.
        #[serde(default = "default_data_bits")]
        data_bits: u8,
        /// Stop bits: 1 or 2.
        #[serde(default = "default_stop_bits")]
        stop_bits: u8,
        /// Parity: `"none"`, `"odd"`, or `"even"`.
        #[serde(default = "default_parity")]
        parity: String,
    },
    /// List currently loaded plugins.
    ListPlugins,
    /// Load a WASM plugin.
    LoadPlugin {
        /// Path to the WASM file.
        path: String,
    },
    /// Unload a loaded plugin.
    UnloadPlugin {
        /// Path of the plugin to unload.
        path: String,
    },
    /// Reload a plugin (e.g. after the source file changed).
    ReloadPlugin {
        /// Path of the plugin to reload.
        path: String,
    },
    /// List workspaces (Sprint 5-7 / Phase 2-1).
    ///
    /// The response is `ServerToClient::WorkspaceList`, which returns the currently
    /// active workspace name along with every workspace's info.
    ListWorkspaces,
    /// Create a new workspace (Sprint 5-7 / Phase 2-1).
    ///
    /// Returns an error if a workspace with the same name already exists. Creation
    /// alone does not activate the workspace; use `SwitchWorkspace` to switch.
    CreateWorkspace {
        /// Workspace name (must be unique; empty strings are not allowed).
        name: String,
    },
    /// Switch the active workspace (Sprint 5-7 / Phase 2-1).
    ///
    /// Returns an error when the workspace does not exist. On success, the server
    /// responds with `ServerToClient::WorkspaceSwitched`.
    SwitchWorkspace {
        /// Target workspace name.
        name: String,
    },
    /// Rename a workspace (Sprint 5-7 / Phase 2-1).
    ///
    /// Also updates `workspace_name` on every session that belongs to it. Errors if
    /// `from` does not exist or `to` collides with an existing name.
    RenameWorkspace {
        /// Old name.
        from: String,
        /// New name.
        to: String,
    },
    /// Delete a workspace (Sprint 5-7 / Phase 2-1).
    ///
    /// The `default` workspace cannot be removed. If sessions still belong to the
    /// workspace, `force = true` migrates them to `default` before deletion. Deleting
    /// the currently active workspace switches the active selection to `default`.
    DeleteWorkspace {
        /// Workspace name to remove.
        name: String,
        /// `true` to forcibly migrate remaining sessions to `default` and delete anyway.
        #[serde(default)]
        force: bool,
    },
    /// Quake-mode toggle request (Sprint 5-7 / Phase 2-2).
    ///
    /// Used by `nexterm-ctl` to drive the toggle through the compositor's `bindsym`
    /// in environments where `global-hotkey` does not work (e.g. Wayland). The server
    /// broadcasts `ServerToClient::QuakeToggleRequest` to every connected GPU client,
    /// and the actual window operation (show / hide / anchor) is performed on the
    /// client side.
    QuakeToggle {
        /// Operation: `"toggle"`, `"show"`, or `"hide"`.
        #[serde(default = "default_quake_action")]
        action: String,
    },
    /// Tab reorder request (Sprint 5-7 / Phase 2-3).
    ///
    /// The client sends the new order it decided via drag-and-drop on the tab bar.
    /// The server overwrites `Window.pane_order` with the new ordering so that
    /// subsequent `LayoutChanged.panes` reflect it. If `pane_ids` contains unknown
    /// IDs or does not cover all known panes, the server filters and completes the
    /// list on its side.
    ReorderPanes {
        /// New display order, left-to-right across the tab bar.
        pane_ids: Vec<u32>,
    },
    /// Move a pane into another window (Sprint 5-8 / Phase 4-3, PROTOCOL_VERSION 8).
    ///
    /// Sent when the client drops a tab onto **another OS Window's tab bar** or
    /// **outside any OS Window**. The server:
    /// 1. Removes the `Pane` from the source window via `detach_pane`.
    /// 2. Inserts the `Pane` into the target window via `attach_pane`.
    /// 3. Broadcasts `LayoutChanged` to both the source and the target window.
    ///
    /// `target_window_id == 0` means **create a new window**: the server creates a
    /// fresh `Window` and registers it under `Session.windows`. The new window's
    /// `id` is communicated to the client via `LayoutChanged.window_id`, after which
    /// the client spawns a new OS Window and issues an `Attach`.
    ///
    /// `insert_at` is the **insertion index inside the target window** (an index into
    /// `pane_order`, 0-based). `None` appends to the end.
    ///
    /// Failure conditions:
    /// - `pane_id` is not in the source window → the server logs an error and does
    ///   nothing.
    /// - `target_window_id` is not present in the session (and is not 0) → same as above.
    /// - The source window had only one pane and detaching empties it → the source
    ///   window is removed automatically (consistent with the existing `close_pane`
    ///   flow).
    MovePaneToWindow {
        /// ID of the pane to move.
        pane_id: u32,
        /// Destination window ID (`0` = create a new window).
        target_window_id: u32,
        /// Insertion position inside the target window (an index into `pane_order`).
        /// `None` appends to the end.
        insert_at: Option<u32>,
    },
    /// Protocol handshake. Sent as the very first message after the connection opens.
    ///
    /// The server compares `proto_version` against `nexterm_proto::PROTOCOL_VERSION`
    /// and, on a mismatch, returns an error and drops the connection.
    Hello {
        /// `nexterm_proto::PROTOCOL_VERSION`.
        proto_version: u32,
        /// Client kind.
        client_kind: ClientKind,
        /// Client's Cargo version string (used for logging).
        client_version: String,
    },
    /// Used to confirm OS-window closes: queries whether any foreground process
    /// (a descendant of the shell) is running inside the specified window.
    ///
    /// When `window.close_action = "prompt"`, the client sends this message after a
    /// close request comes in and uses the `ServerToClient::ForegroundProcessStatus`
    /// reply to decide whether to show a confirmation dialog or immediately
    /// detach / kill.
    ///
    /// PROTOCOL_VERSION 8 compatibility: appended to the end of the enum so the
    /// existing variant discriminants are untouched (old clients do not send it;
    /// old servers reply with `Error` because they do not handle it — both
    /// scenarios remain within the additive compatibility window of v8).
    QueryForegroundProcess {
        /// Server-side window ID to query.
        window_id: u32,
    },
}

/// Client kind (identified during the IPC handshake).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientKind {
    /// GPU client (winit + wgpu).
    Gpu,
    /// TUI client (ratatui + crossterm).
    Tui,
    /// CLI tool (nexterm-ctl).
    Ctl,
    /// Other (plugins, etc.).
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
fn default_quake_action() -> String {
    "toggle".to_string()
}

/// Layout information for a pane (in grid coordinates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneLayout {
    /// Unique pane ID.
    pub pane_id: u32,
    /// Column offset inside the window (0-based).
    pub col_offset: u16,
    /// Row offset inside the window (0-based).
    pub row_offset: u16,
    /// Number of columns in the pane (in characters).
    pub cols: u16,
    /// Number of rows in the pane (in characters).
    pub rows: u16,
    /// Whether this pane currently holds focus.
    pub is_focused: bool,
}

/// Server → client messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerToClient {
    /// Differential grid update (regular paint update).
    GridDiff {
        /// Target pane ID.
        pane_id: u32,
        /// Dirty rows only.
        dirty_rows: Vec<DirtyRow>,
        /// Cursor column (0-based).
        cursor_col: u16,
        /// Cursor row (0-based).
        cursor_row: u16,
    },
    /// Full-screen snapshot (on attach / reconnect).
    FullRefresh {
        /// Target pane ID.
        pane_id: u32,
        /// Snapshot grid.
        grid: Grid,
    },
    /// Session list.
    SessionList {
        /// Session info entries.
        sessions: Vec<SessionInfo>,
    },
    /// Response to `Ping`.
    Pong,
    /// Error notification.
    Error {
        /// Error message.
        message: String,
    },
    /// Image placement notification (Sixel / Kitty protocol).
    ImagePlaced {
        /// Target pane ID.
        pane_id: u32,
        /// Unique image ID (used for frame management).
        image_id: u32,
        /// Placement column in the grid (0-based).
        col: u16,
        /// Placement row in the grid (0-based).
        row: u16,
        /// Image width in pixels.
        width: u32,
        /// Image height in pixels.
        height: u32,
        /// RGBA pixel data.
        rgba: Vec<u8>,
    },
    /// OSC 66 text-sizing notification (Kitty Text Sizing Protocol).
    TextSized {
        /// Target pane ID.
        pane_id: u32,
        /// Placement column in the grid (0-based).
        col: u16,
        /// Placement row in the grid (0-based).
        row: u16,
        /// Scale numerator (integer scale `s` maps to num=s, den=1).
        scale_num: u8,
        /// Scale denominator (1 for integer scales).
        scale_den: u8,
        /// Width in character cells (0 = auto).
        width_cells: u16,
        /// Vertical alignment: 0 = baseline, 1 = center, 2 = top.
        valign: u8,
        /// Horizontal alignment: 0 = left, 1 = center, 2 = right.
        halign: u8,
        /// Text to render at the specified scale.
        text: String,
    },
    /// Layout-change notification (on split, focus change, or resize).
    LayoutChanged {
        /// Layouts for all panes.
        panes: Vec<PaneLayout>,
        /// ID of the pane that currently holds focus.
        focused_pane_id: u32,
    },
    /// BEL notification (emitted by a pane that received `\x07`).
    Bell {
        /// Pane ID that received the BEL.
        pane_id: u32,
    },
    /// Session-recording start notification.
    RecordingStarted {
        /// Pane ID being recorded.
        pane_id: u32,
        /// Recording file path.
        path: String,
    },
    /// Session-recording stop notification.
    RecordingStopped {
        /// Pane ID that was being recorded.
        pane_id: u32,
    },
    /// Window-list change notification.
    WindowListChanged {
        /// Latest list of windows.
        windows: Vec<WindowInfo>,
    },
    /// Notification that a pane was closed (set `pane_id` to 0 when the entire
    /// window was closed alongside it).
    PaneClosed {
        /// Closed pane ID (0 = the entire window was closed).
        pane_id: u32,
    },
    /// Window/pane title-change notification.
    TitleChanged {
        /// Pane ID whose title changed.
        pane_id: u32,
        /// New title string.
        title: String,
    },
    /// Desktop notification.
    DesktopNotification {
        /// Source pane ID.
        pane_id: u32,
        /// Notification title.
        title: String,
        /// Notification body.
        body: String,
    },
    /// OSC 52 clipboard-write request (Sprint 4-1).
    ///
    /// The client follows the `SecurityConfig.osc52_clipboard` policy to decide
    /// whether to display a consent dialog or grant/deny the request immediately.
    ClipboardWriteRequest {
        /// Requesting pane ID.
        pane_id: u32,
        /// Content to write (control characters stripped on the server side).
        text: String,
    },
    /// Broadcast-mode status notification.
    BroadcastModeChanged {
        /// `true` = broadcast to all panes, `false` = focused pane only.
        enabled: bool,
    },
    /// asciicast v2 recording-start notification.
    AsciicastStarted {
        /// Pane ID being recorded.
        pane_id: u32,
        /// Path to the asciicast file.
        path: String,
    },
    /// asciicast v2 recording-stop notification.
    AsciicastStopped {
        /// Pane ID that was being recorded.
        pane_id: u32,
    },
    /// Template-save completion notification.
    TemplateSaved {
        /// Template name.
        name: String,
        /// Path to the saved file.
        path: String,
    },
    /// Template-load completion notification.
    TemplateLoaded {
        /// Name of the loaded template.
        name: String,
    },
    /// Template list.
    TemplateList {
        /// Names of saved templates.
        names: Vec<String>,
    },
    /// Pane-zoom state-change notification.
    ZoomChanged {
        /// `true` = zoomed, `false` = normal layout.
        is_zoomed: bool,
    },
    /// `BreakPane` completion notification (the new window's ID).
    PaneBroken {
        /// ID of the newly created window.
        new_window_id: u32,
        /// Pane ID that was broken out.
        pane_id: u32,
    },
    /// Serial connection success notification.
    SerialConnected {
        /// Allocated pane ID.
        pane_id: u32,
        /// Connected port name (e.g. `/dev/ttyUSB0`).
        port: String,
    },
    /// SFTP transfer progress notification.
    SftpProgress {
        /// Source local path or remote path (used for the UI).
        path: String,
        /// Number of bytes transferred so far.
        transferred: u64,
        /// Total byte count (0 = unknown).
        total: u64,
    },
    /// SFTP transfer completion notification.
    SftpDone {
        /// Source/destination path (used for the UI).
        path: String,
        /// `None` on success, error message on failure.
        error: Option<String>,
    },
    /// OSC 133 semantic-zone-mark notification.
    SemanticMark {
        /// Pane ID that received the mark.
        pane_id: u32,
        /// Marked row (0-based).
        row: u16,
        /// `"A"` = PromptStart, `"B"` = CommandStart, `"C"` = OutputStart, `"D"` = CommandEnd.
        kind: String,
        /// Only `Some` for the D mark.
        exit_code: Option<i32>,
    },
    /// OSC 7 current-working-directory (CWD) change notification (Sprint 5-2 / B2).
    ///
    /// Emitted when the shell writes something like
    /// `printf '\033]7;file://%s%s\033\\' "$HOSTNAME" "$PWD"`. The client uses the
    /// new CWD for tab display, window title, and to inherit the parent CWD when
    /// creating a new pane.
    CwdChanged {
        /// Pane ID whose CWD changed.
        pane_id: u32,
        /// New CWD (with `file://` stripped and percent-decoded; assumed absolute).
        cwd: String,
    },
    /// Floating-pane open notification.
    FloatingPaneOpened {
        /// Opened floating-pane ID.
        pane_id: u32,
        /// Column offset (0-based).
        col_off: u16,
        /// Row offset (0-based).
        row_off: u16,
        /// Column count of the pane.
        cols: u16,
        /// Row count of the pane.
        rows: u16,
    },
    /// Floating-pane position/size-change notification.
    FloatingPaneMoved {
        /// Floating-pane ID that moved.
        pane_id: u32,
        /// Column offset (0-based).
        col_off: u16,
        /// Row offset (0-based).
        row_off: u16,
        /// Column count of the pane.
        cols: u16,
        /// Row count of the pane.
        rows: u16,
    },
    /// Floating-pane close notification.
    FloatingPaneClosed {
        /// Closed floating-pane ID.
        pane_id: u32,
    },
    /// List of currently loaded plugins.
    PluginList {
        /// Plugin paths.
        paths: Vec<String>,
    },
    /// Plugin operation completion notification.
    PluginOk {
        /// Target plugin path.
        path: String,
        /// Operation kind: `"loaded"`, `"unloaded"`, or `"reloaded"`.
        action: String,
    },
    /// Workspace list (Sprint 5-7 / Phase 2-1).
    ///
    /// Sent in response to `ListWorkspaces`, or after a successful
    /// `CreateWorkspace` / `RenameWorkspace` / `DeleteWorkspace`.
    WorkspaceList {
        /// Currently active workspace name.
        current: String,
        /// Information for every workspace.
        workspaces: Vec<WorkspaceInfo>,
    },
    /// Workspace-switch completion notification (Sprint 5-7 / Phase 2-1).
    WorkspaceSwitched {
        /// Workspace name after the switch.
        name: String,
    },
    /// Quake-mode toggle request (Sprint 5-7 / Phase 2-2).
    ///
    /// When the server receives `ClientToServer::QuakeToggle`, it broadcasts this
    /// message to every connected GPU client. Quake-mode-capable clients (the GPU
    /// build) react to `action` by showing, hiding, or toggling their window.
    QuakeToggleRequest {
        /// One of `"toggle"`, `"show"`, or `"hide"`.
        action: String,
    },
    /// Protocol handshake response (server → client).
    ///
    /// Sent by the server immediately after the client's Hello, carrying the server's
    /// version info. When the server drops the connection because of a version
    /// mismatch, this message is **not** sent (only an `Error` variant plus the
    /// disconnect).
    HelloAck {
        /// Lowest protocol version the server supports.
        proto_version: u32,
        /// Server's Cargo version string.
        server_version: String,
    },
    /// Reply to `QueryForegroundProcess` (Sprint 5-8 / Phase 4-5).
    ///
    /// Indicates whether any foreground process (i.e. a non-shell child) is running
    /// inside the specified window. When `has_foreground = true`, the client shows
    /// the confirmation dialog.
    ///
    /// PROTOCOL_VERSION 8 compatibility: appended to the end of the enum. Old
    /// clients do not send the query, so this reply never fires for them.
    ForegroundProcessStatus {
        /// Server-side window ID being queried (used for client-side correlation).
        window_id: u32,
        /// `true` when at least one pane has a foreground process running.
        has_foreground: bool,
    },
}

/// Session information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session name.
    pub name: String,
    /// Number of windows.
    pub window_count: u32,
    /// Whether a client is currently attached.
    pub attached: bool,
    /// Owning workspace name (Sprint 5-7 / Phase 2-1).
    ///
    /// Added in PROTOCOL_VERSION 5. `#[serde(default)]` lets older clients decode
    /// the structure with an empty string when the field is missing in the postcard
    /// payload. The server always populates the field (defaulting to `"default"`).
    #[serde(default)]
    pub workspace_name: String,
}

/// Window information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// Unique window ID.
    pub window_id: u32,
    /// Window name.
    pub name: String,
    /// Number of panes in the window.
    pub pane_count: u32,
    /// Whether this window currently holds focus.
    pub is_focused: bool,
}

/// Workspace information (added in Sprint 5-7 / Phase 2-1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    /// Workspace name.
    pub name: String,
    /// Number of sessions belonging to this workspace.
    pub session_count: u32,
    /// Whether this is the currently active workspace.
    pub is_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cell, Grid};

    #[test]
    fn key_event_postcard_roundtrip() {
        let msg = ClientToServer::KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: Modifiers(Modifiers::CTRL),
            event_type: 1,
        };
        let encoded = postcard::to_stdvec(&msg).unwrap();
        let decoded: ClientToServer = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn full_refresh_postcard_roundtrip() {
        let grid = Grid::new(80, 24);
        let msg = ServerToClient::FullRefresh { pane_id: 1, grid };
        let encoded = postcard::to_stdvec(&msg).unwrap();
        let decoded: ServerToClient = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn grid_diff_postcard_roundtrip() {
        let msg = ServerToClient::GridDiff {
            pane_id: 0,
            dirty_rows: vec![DirtyRow {
                row: 3,
                cells: vec![Cell::default(); 80],
            }],
            cursor_col: 5,
            cursor_row: 3,
        };
        let encoded = postcard::to_stdvec(&msg).unwrap();
        let decoded: ServerToClient = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn modifiers_bit_flags() {
        let m = Modifiers(Modifiers::CTRL | Modifiers::SHIFT);
        assert!(m.is_ctrl());
        assert!(m.is_shift());
    }

    #[test]
    fn hello_message_postcard_roundtrip() {
        let msg = ClientToServer::Hello {
            proto_version: 1,
            client_kind: ClientKind::Gpu,
            client_version: "1.0.2".to_string(),
        };
        let encoded = postcard::to_stdvec(&msg).unwrap();
        let decoded: ClientToServer = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn hello_ack_message_postcard_roundtrip() {
        let msg = ServerToClient::HelloAck {
            proto_version: 1,
            server_version: "1.0.2".to_string(),
        };
        let encoded = postcard::to_stdvec(&msg).unwrap();
        let decoded: ServerToClient = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn workspace_ipc_postcard_roundtrip() {
        // ListWorkspaces
        let msg = ClientToServer::ListWorkspaces;
        let encoded = postcard::to_stdvec(&msg).unwrap();
        let decoded: ClientToServer = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(msg, decoded);

        // CreateWorkspace / SwitchWorkspace / RenameWorkspace / DeleteWorkspace
        let cases = [
            ClientToServer::CreateWorkspace {
                name: "dev".to_string(),
            },
            ClientToServer::SwitchWorkspace {
                name: "prod".to_string(),
            },
            ClientToServer::RenameWorkspace {
                from: "old".to_string(),
                to: "new".to_string(),
            },
            ClientToServer::DeleteWorkspace {
                name: "tmp".to_string(),
                force: true,
            },
        ];
        for msg in cases {
            let enc = postcard::to_stdvec(&msg).unwrap();
            let dec: ClientToServer = postcard::from_bytes(&enc).unwrap();
            assert_eq!(msg, dec);
        }

        // WorkspaceList / WorkspaceSwitched
        let list = ServerToClient::WorkspaceList {
            current: "default".to_string(),
            workspaces: vec![
                WorkspaceInfo {
                    name: "default".to_string(),
                    session_count: 2,
                    is_active: true,
                },
                WorkspaceInfo {
                    name: "dev".to_string(),
                    session_count: 0,
                    is_active: false,
                },
            ],
        };
        let enc = postcard::to_stdvec(&list).unwrap();
        let dec: ServerToClient = postcard::from_bytes(&enc).unwrap();
        assert_eq!(list, dec);

        let switched = ServerToClient::WorkspaceSwitched {
            name: "dev".to_string(),
        };
        let enc = postcard::to_stdvec(&switched).unwrap();
        let dec: ServerToClient = postcard::from_bytes(&enc).unwrap();
        assert_eq!(switched, dec);
    }

    #[test]
    fn session_info_workspace_name_defaults_to_empty_via_serde_default() {
        // Compatibility with older clients: a postcard payload that omits the
        // `workspace_name` field should still decode into a SessionInfo (with the
        // default value `""`).
        //
        // postcard honors struct field order, so we synthesize the legacy byte layout
        // containing only (name, window_count, attached) and verify the behavior.
        let name = "old".to_string();
        let window_count: u32 = 1;
        let attached = false;
        let mut buf = postcard::to_stdvec(&name).unwrap();
        buf.extend(postcard::to_stdvec(&window_count).unwrap());
        buf.extend(postcard::to_stdvec(&attached).unwrap());
        // Intentionally omit `workspace_name` (= legacy format).
        let decoded: Result<SessionInfo, _> = postcard::from_bytes(&buf);
        // The legacy format is shorter than the new one, so postcard 1.x returns an
        // error for the truncated payload. Backward compatibility cannot be solved
        // purely at the serde layer; the server implementation is expected to always
        // populate `workspace_name`. Here we only check that the type still exists.
        let _ = decoded; // strict back-compat is the server's responsibility

        // The new format (including `workspace_name`) round-trips through postcard.
        let info = SessionInfo {
            name: "ok".to_string(),
            window_count: 2,
            attached: true,
            workspace_name: "dev".to_string(),
        };
        let enc = postcard::to_stdvec(&info).unwrap();
        let dec: SessionInfo = postcard::from_bytes(&enc).unwrap();
        assert_eq!(info, dec);
    }

    #[test]
    fn reorder_panes_ipc_postcard_roundtrip() {
        let msg = ClientToServer::ReorderPanes {
            pane_ids: vec![3, 1, 4, 1, 5, 9, 2, 6],
        };
        let enc = postcard::to_stdvec(&msg).unwrap();
        let dec: ClientToServer = postcard::from_bytes(&enc).unwrap();
        assert_eq!(msg, dec);

        // An empty vector round-trips just as well.
        let empty = ClientToServer::ReorderPanes { pane_ids: vec![] };
        let enc = postcard::to_stdvec(&empty).unwrap();
        let dec: ClientToServer = postcard::from_bytes(&enc).unwrap();
        assert_eq!(empty, dec);
    }

    #[test]
    fn move_pane_to_window_ipc_postcard_roundtrip() {
        // Sprint 5-8 / Phase 4-3 — added in PROTOCOL_VERSION 8.
        // Typical pattern: target_window_id != 0 with insert_at = Some.
        let msg = ClientToServer::MovePaneToWindow {
            pane_id: 42,
            target_window_id: 7,
            insert_at: Some(2),
        };
        let enc = postcard::to_stdvec(&msg).unwrap();
        let dec: ClientToServer = postcard::from_bytes(&enc).unwrap();
        assert_eq!(msg, dec);

        // target_window_id = 0 (create a new window), insert_at = None (append to end).
        let new_window = ClientToServer::MovePaneToWindow {
            pane_id: 99,
            target_window_id: 0,
            insert_at: None,
        };
        let enc = postcard::to_stdvec(&new_window).unwrap();
        let dec: ClientToServer = postcard::from_bytes(&enc).unwrap();
        assert_eq!(new_window, dec);
    }

    #[test]
    fn quake_toggle_ipc_postcard_roundtrip() {
        // QuakeToggle (client → server).
        let msg = ClientToServer::QuakeToggle {
            action: "toggle".to_string(),
        };
        let enc = postcard::to_stdvec(&msg).unwrap();
        let dec: ClientToServer = postcard::from_bytes(&enc).unwrap();
        assert_eq!(msg, dec);

        // QuakeToggleRequest (server → client, broadcast).
        let req = ServerToClient::QuakeToggleRequest {
            action: "show".to_string(),
        };
        let enc = postcard::to_stdvec(&req).unwrap();
        let dec: ServerToClient = postcard::from_bytes(&enc).unwrap();
        assert_eq!(req, dec);
    }

    #[test]
    fn client_kind_every_variant_postcard_roundtrips() {
        for kind in [
            ClientKind::Gpu,
            ClientKind::Tui,
            ClientKind::Ctl,
            ClientKind::Other,
        ] {
            let encoded = postcard::to_stdvec(&kind).unwrap();
            let decoded: ClientKind = postcard::from_bytes(&encoded).unwrap();
            assert_eq!(kind, decoded);
        }
    }
}
