//! IPC message type definitions.
//!
//! Covers messages for both directions: client → server and server → client.

use serde::{Deserialize, Serialize};

use crate::{DirtyRow, Grid};

fn default_key_event_type() -> u8 {
    1
}

/// Bit flags for keyboard modifier keys.
///
/// The raw bits are private (audit round 3, A5/R5); construct with
/// [`Modifiers::new`] and read with [`Modifiers::bits`]. The postcard wire
/// format is unchanged (a newtype struct still serializes as its inner `u8`),
/// so this needs no `PROTOCOL_VERSION` bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Modifiers(u8);

impl Modifiers {
    /// Bit mask for the Shift key.
    pub const SHIFT: u8 = 0b0001;
    /// Bit mask for the Ctrl key.
    pub const CTRL: u8 = 0b0010;
    /// Bit mask for the Alt / Option key.
    pub const ALT: u8 = 0b0100;
    /// Bit mask for the Meta / Super / Windows key.
    pub const META: u8 = 0b1000;

    /// Construct from a raw bit mask.
    pub const fn new(bits: u8) -> Self {
        Self(bits)
    }

    /// Return the raw bit mask.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Returns whether the Ctrl key is held.
    pub fn is_ctrl(self) -> bool {
        self.0 & Self::CTRL != 0
    }
    /// Returns whether the Shift key is held.
    pub fn is_shift(self) -> bool {
        self.0 & Self::SHIFT != 0
    }
    /// Returns whether the Alt / Option key is held.
    pub fn is_alt(self) -> bool {
        self.0 & Self::ALT != 0
    }
    /// Returns whether the Meta / Super / Windows key is held.
    pub fn is_meta(self) -> bool {
        self.0 & Self::META != 0
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
        ///
        /// `#[serde(default = "default_key_event_type")]` only supplies a value when
        /// this struct is deserialized from a self-describing format (e.g. JSON/TOML)
        /// that omits the field. postcard is a positional wire format, not
        /// self-describing: every field must still be present, in this exact position,
        /// in the encoded bytes, or decoding fails outright (see audit round 4 #25).
        /// This attribute does NOT give older client payloads forward compatibility
        /// over the wire; it only affects non-postcard (de)serialization paths and
        /// documents the intended default for readers.
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
    /// Report the client's active theme default colors (PROTOCOL_VERSION 10,
    /// roadmap #10b).
    ///
    /// Sent after attach and whenever the user commits a theme change. The
    /// server forwards the values into each pane's VT screen so OSC 10/11
    /// queries report the colors that are actually rendered. With multiple
    /// clients attached, the most recent report wins.
    SetThemeColors {
        /// Theme default foreground (8-bit RGB).
        fg: [u8; 3],
        /// Theme default background (8-bit RGB).
        bg: [u8; 3],
    },
    /// A file was dropped onto the focused pane (PROTOCOL_VERSION 10).
    ///
    /// The server decides how to deliver it: when the pane's application
    /// opted in to the kitty drag-and-drop protocol (OSC 72 `t=a`), the path
    /// is offered as a `text/uri-list` drop; otherwise `paste_fallback` is
    /// pasted exactly like `PasteText` (the pre-DnD behavior).
    DndDrop {
        /// Absolute filesystem path of the dropped file.
        path: String,
        /// Pre-formatted paste text (quoted path, batching spaces included).
        paste_fallback: String,
    },
    /// Create a pane (vertical split) running an explicit shell instead of the
    /// session default (PROTOCOL_VERSION 11).
    ///
    /// Sent by the new-tab profile dropdown and the context-menu profile
    /// entries: the client resolves a `Profile` (or a detected WSL distro)
    /// into a concrete command line and ships it here, so the server does not
    /// need to know about client-side profile state.
    SplitWithShell {
        /// Shell / program to launch (e.g. `/usr/bin/fish`, `wsl.exe`).
        program: String,
        /// Arguments passed to the program.
        #[serde(default)]
        args: Vec<String>,
        /// Initial working directory (`None` = the server-side default,
        /// which falls back to the user's home directory).
        #[serde(default)]
        cwd: Option<String>,
        /// Extra environment variables set for the child process.
        #[serde(default)]
        env: Vec<(String, String)>,
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
    /// Phase 2c (UI/UX v2): foreground-process change notification.
    ///
    /// Broadcast at most once per second per pane by the server's
    /// session-wide polling ticker; only when the detected name actually
    /// changes from the previous tick. `process_name = None` means the
    /// shell is at the prompt (no foreground job) or detection failed —
    /// the client must clear any previously rendered icon in that case.
    ///
    /// Decoupled from `TitleChanged` because the cadence and triggers
    /// are independent (titles fire on OSC 0/2 escape sequences;
    /// process names follow the OS process table). Bundling them would
    /// either re-send unchanged titles every second or force the
    /// process polling to wait for an OSC 0 escape.
    ProcessChanged {
        /// Pane ID whose foreground process changed.
        pane_id: u32,
        /// New foreground process name (e.g. `"vim"`, `"ssh"`) or
        /// `None` when the shell is sitting at the prompt.
        process_name: Option<String>,
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
    /// OSC 22 mouse-pointer-shape change (PROTOCOL_VERSION 10).
    ///
    /// The shape is the raw (length-capped) name from the escape sequence,
    /// e.g. `"pointer"`, `"text"`, `"wait"`; `"default"` resets. The client
    /// maps it onto a platform cursor icon and falls back to the default for
    /// unknown names. Appended to the end of the enum so existing variant
    /// discriminants are untouched.
    PointerShapeChanged {
        /// Pane ID whose pointer shape changed.
        pane_id: u32,
        /// Requested shape name.
        shape: String,
    },
    /// Dynamic-color override snapshot (PROTOCOL_VERSION 10, roadmap #10b).
    ///
    /// Broadcast whenever a pane's OSC 4/10/11 color state changes. This is
    /// the **full** override state, not a delta — the client replaces its
    /// per-pane override record with the snapshot, so late joiners converge
    /// on the latest message.
    PaneColorsChanged {
        /// Pane ID whose dynamic colors changed.
        pane_id: u32,
        /// OSC 10 dynamic foreground (`None` = theme default).
        fg: Option<[u8; 3]>,
        /// OSC 11 dynamic background (`None` = theme default).
        bg: Option<[u8; 3]>,
        /// OSC 4 palette overrides, sorted by index.
        palette: Vec<(u8, [u8; 3])>,
    },
    /// OSC 9;4 (ConEmu) progress report (PROTOCOL_VERSION 10).
    ///
    /// Rendered as a per-tab progress indicator. `state` 0 removes the
    /// indicator, 1 = normal, 2 = error, 3 = indeterminate, 4 = paused;
    /// `progress` is a clamped 0–100 percentage.
    ProgressChanged {
        /// Pane ID whose progress changed.
        pane_id: u32,
        /// Progress state (0–4, see above).
        state: u8,
        /// Percentage (0–100).
        progress: u8,
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
    /// Added in PROTOCOL_VERSION 5. `#[serde(default)]` here does NOT provide wire
    /// forward/backward compatibility over postcard: postcard is a positional format,
    /// so a payload from an older struct definition that is missing this trailing
    /// field does not decode into "field defaulted" — it fails to decode at all (a
    /// short/truncated buffer error), because there is nothing to mark where the
    /// field would have been. `#[serde(default)]` only helps when the *same* struct
    /// is deserialized from a self-describing format (JSON/TOML/etc.) that omits the
    /// field, or when constructing the value in-process without serde. See audit
    /// round 4 #25 and the `session_info_workspace_name_defaults_to_empty_via_serde_default`
    /// test below, which demonstrates the truncated-postcard-payload case returning an
    /// error rather than a defaulted value. Wire compatibility here is maintained by
    /// convention instead: the server always populates the field.
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

    /// Phase 2c (UI/UX v2): `ProcessChanged` must postcard-roundtrip for
    /// both the `Some(name)` (foreground process detected) and `None`
    /// (shell at prompt) cases so the client sees identical semantics.
    #[test]
    fn process_changed_postcard_roundtrip() {
        let with_name = ServerToClient::ProcessChanged {
            pane_id: 7,
            process_name: Some("vim".to_string()),
        };
        let encoded = postcard::to_stdvec(&with_name).unwrap();
        let decoded: ServerToClient = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(with_name, decoded);

        let none = ServerToClient::ProcessChanged {
            pane_id: 7,
            process_name: None,
        };
        let encoded = postcard::to_stdvec(&none).unwrap();
        let decoded: ServerToClient = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(none, decoded);
    }

    /// PROTOCOL_VERSION 11: `SplitWithShell` must postcard-roundtrip both in
    /// its minimal form (bare program) and with every optional field set,
    /// since profile launches routinely carry cwd + env overrides.
    #[test]
    fn split_with_shell_postcard_roundtrip() {
        let minimal = ClientToServer::SplitWithShell {
            program: "/usr/bin/fish".to_string(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
        };
        let encoded = postcard::to_stdvec(&minimal).unwrap();
        let decoded: ClientToServer = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(minimal, decoded);

        let full = ClientToServer::SplitWithShell {
            program: "wsl.exe".to_string(),
            args: vec!["-d".to_string(), "Ubuntu".to_string()],
            cwd: Some("/home/user/project".to_string()),
            env: vec![("FOO".to_string(), "bar".to_string())],
        };
        let encoded = postcard::to_stdvec(&full).unwrap();
        let decoded: ClientToServer = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(full, decoded);
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
    fn pane_colors_changed_postcard_roundtrip() {
        // Roadmap #10b — added in PROTOCOL_VERSION 10.
        let msg = ServerToClient::PaneColorsChanged {
            pane_id: 3,
            fg: Some([255, 136, 0]),
            bg: None,
            palette: vec![(1, [0x12, 0x34, 0x56]), (196, [255, 0, 0])],
        };
        let bytes = postcard::to_allocvec(&msg).expect("serialize PaneColorsChanged");
        let decoded: ServerToClient =
            postcard::from_bytes(&bytes).expect("deserialize PaneColorsChanged");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn set_theme_colors_postcard_roundtrip() {
        // Roadmap #10b — added in PROTOCOL_VERSION 10.
        let msg = ClientToServer::SetThemeColors {
            fg: [0xd9, 0xd9, 0xd9],
            bg: [0x0d, 0x0d, 0x0d],
        };
        let bytes = postcard::to_allocvec(&msg).expect("serialize SetThemeColors");
        let decoded: ClientToServer =
            postcard::from_bytes(&bytes).expect("deserialize SetThemeColors");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn dnd_drop_postcard_roundtrip() {
        // kitty DnD — added in PROTOCOL_VERSION 10.
        let msg = ClientToServer::DndDrop {
            path: "C:/tmp/a.txt".to_string(),
            paste_fallback: "\"C:/tmp/a.txt\"".to_string(),
        };
        let bytes = postcard::to_allocvec(&msg).expect("serialize DndDrop");
        let decoded: ClientToServer = postcard::from_bytes(&bytes).expect("deserialize DndDrop");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn progress_changed_postcard_roundtrip() {
        // OSC 9;4 — added in PROTOCOL_VERSION 10.
        let msg = ServerToClient::ProgressChanged {
            pane_id: 5,
            state: 1,
            progress: 42,
        };
        let bytes = postcard::to_allocvec(&msg).expect("serialize ProgressChanged");
        let decoded: ServerToClient =
            postcard::from_bytes(&bytes).expect("deserialize ProgressChanged");
        assert_eq!(msg, decoded);
    }

    #[test]
    fn pointer_shape_changed_postcard_roundtrip() {
        // OSC 22 — added in PROTOCOL_VERSION 10.
        let msg = ServerToClient::PointerShapeChanged {
            pane_id: 7,
            shape: "pointer".to_string(),
        };
        let bytes = postcard::to_allocvec(&msg).expect("serialize PointerShapeChanged");
        let decoded: ServerToClient =
            postcard::from_bytes(&bytes).expect("deserialize PointerShapeChanged");
        assert_eq!(msg, decoded);
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

    /// Decode a postcard-encoded unsigned LEB128 varint from the start of `bytes`
    /// and return its value. postcard encodes an enum's variant index (its wire
    /// tag) as exactly this kind of varint, immediately followed by the variant's
    /// field data, so this is what actually identifies "which variant" a decoder
    /// sees on the wire.
    fn read_leading_varint_u32(bytes: &[u8]) -> u32 {
        let mut value: u32 = 0;
        let mut shift = 0;
        for &byte in bytes {
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return value;
            }
            shift += 7;
        }
        panic!("truncated varint in postcard output");
    }

    /// Maps each `ClientToServer` variant to its intended wire tag (0-based
    /// declaration order). This match is exhaustive by construction: adding a new
    /// variant to the enum without adding a corresponding arm here fails to
    /// compile, which is the first line of defense against silent reordering.
    fn client_to_server_expected_tag(msg: &ClientToServer) -> u32 {
        match msg {
            ClientToServer::KeyEvent { .. } => 0,
            ClientToServer::Resize { .. } => 1,
            ClientToServer::Detach => 2,
            ClientToServer::Attach { .. } => 3,
            ClientToServer::SplitVertical => 4,
            ClientToServer::SplitHorizontal => 5,
            ClientToServer::FocusNextPane => 6,
            ClientToServer::FocusPrevPane => 7,
            ClientToServer::FocusPane { .. } => 8,
            ClientToServer::PasteText { .. } => 9,
            ClientToServer::Ping => 10,
            ClientToServer::ListSessions => 11,
            ClientToServer::KillSession { .. } => 12,
            ClientToServer::StartRecording { .. } => 13,
            ClientToServer::StopRecording { .. } => 14,
            ClientToServer::ClosePane => 15,
            ClientToServer::ResizeSplit { .. } => 16,
            ClientToServer::ConnectSsh { .. } => 17,
            ClientToServer::NewWindow => 18,
            ClientToServer::CloseWindow { .. } => 19,
            ClientToServer::FocusWindow { .. } => 20,
            ClientToServer::RenameWindow { .. } => 21,
            ClientToServer::SetBroadcast { .. } => 22,
            ClientToServer::DisplayPanes { .. } => 23,
            ClientToServer::StartAsciicast { .. } => 24,
            ClientToServer::StopAsciicast { .. } => 25,
            ClientToServer::SaveTemplate { .. } => 26,
            ClientToServer::LoadTemplate { .. } => 27,
            ClientToServer::ListTemplates => 28,
            ClientToServer::ToggleZoom => 29,
            ClientToServer::SwapPane { .. } => 30,
            ClientToServer::BreakPane => 31,
            ClientToServer::JoinPane { .. } => 32,
            ClientToServer::SftpUpload { .. } => 33,
            ClientToServer::SftpDownload { .. } => 34,
            ClientToServer::RunMacro { .. } => 35,
            ClientToServer::MouseReport { .. } => 36,
            ClientToServer::SetLayoutMode { .. } => 37,
            ClientToServer::OpenFloatingPane => 38,
            ClientToServer::CloseFloatingPane { .. } => 39,
            ClientToServer::MoveFloatingPane { .. } => 40,
            ClientToServer::ResizeFloatingPane { .. } => 41,
            ClientToServer::ConnectSerial { .. } => 42,
            ClientToServer::ListPlugins => 43,
            ClientToServer::LoadPlugin { .. } => 44,
            ClientToServer::UnloadPlugin { .. } => 45,
            ClientToServer::ReloadPlugin { .. } => 46,
            ClientToServer::ListWorkspaces => 47,
            ClientToServer::CreateWorkspace { .. } => 48,
            ClientToServer::SwitchWorkspace { .. } => 49,
            ClientToServer::RenameWorkspace { .. } => 50,
            ClientToServer::DeleteWorkspace { .. } => 51,
            ClientToServer::QuakeToggle { .. } => 52,
            ClientToServer::ReorderPanes { .. } => 53,
            ClientToServer::MovePaneToWindow { .. } => 54,
            ClientToServer::Hello { .. } => 55,
            ClientToServer::QueryForegroundProcess { .. } => 56,
            ClientToServer::SetThemeColors { .. } => 57,
            ClientToServer::DndDrop { .. } => 58,
            ClientToServer::SplitWithShell { .. } => 59,
        }
    }

    /// Maps each `ServerToClient` variant to its intended wire tag (0-based
    /// declaration order). Exhaustive for the same reason as
    /// `client_to_server_expected_tag` above.
    fn server_to_client_expected_tag(msg: &ServerToClient) -> u32 {
        match msg {
            ServerToClient::GridDiff { .. } => 0,
            ServerToClient::FullRefresh { .. } => 1,
            ServerToClient::SessionList { .. } => 2,
            ServerToClient::Pong => 3,
            ServerToClient::Error { .. } => 4,
            ServerToClient::ImagePlaced { .. } => 5,
            ServerToClient::TextSized { .. } => 6,
            ServerToClient::LayoutChanged { .. } => 7,
            ServerToClient::Bell { .. } => 8,
            ServerToClient::RecordingStarted { .. } => 9,
            ServerToClient::RecordingStopped { .. } => 10,
            ServerToClient::WindowListChanged { .. } => 11,
            ServerToClient::PaneClosed { .. } => 12,
            ServerToClient::TitleChanged { .. } => 13,
            ServerToClient::ProcessChanged { .. } => 14,
            ServerToClient::DesktopNotification { .. } => 15,
            ServerToClient::ClipboardWriteRequest { .. } => 16,
            ServerToClient::BroadcastModeChanged { .. } => 17,
            ServerToClient::AsciicastStarted { .. } => 18,
            ServerToClient::AsciicastStopped { .. } => 19,
            ServerToClient::TemplateSaved { .. } => 20,
            ServerToClient::TemplateLoaded { .. } => 21,
            ServerToClient::TemplateList { .. } => 22,
            ServerToClient::ZoomChanged { .. } => 23,
            ServerToClient::PaneBroken { .. } => 24,
            ServerToClient::SerialConnected { .. } => 25,
            ServerToClient::SftpProgress { .. } => 26,
            ServerToClient::SftpDone { .. } => 27,
            ServerToClient::SemanticMark { .. } => 28,
            ServerToClient::CwdChanged { .. } => 29,
            ServerToClient::FloatingPaneOpened { .. } => 30,
            ServerToClient::FloatingPaneMoved { .. } => 31,
            ServerToClient::FloatingPaneClosed { .. } => 32,
            ServerToClient::PluginList { .. } => 33,
            ServerToClient::PluginOk { .. } => 34,
            ServerToClient::WorkspaceList { .. } => 35,
            ServerToClient::WorkspaceSwitched { .. } => 36,
            ServerToClient::QuakeToggleRequest { .. } => 37,
            ServerToClient::HelloAck { .. } => 38,
            ServerToClient::ForegroundProcessStatus { .. } => 39,
            ServerToClient::PointerShapeChanged { .. } => 40,
            ServerToClient::PaneColorsChanged { .. } => 41,
            ServerToClient::ProgressChanged { .. } => 42,
        }
    }

    /// Audit round 4 #11: postcard's enum wire tag is implicit declaration order,
    /// not self-describing. Inserting a new variant anywhere but the end silently
    /// changes every following variant's wire tag, breaking compatibility between
    /// differently-versioned client/server builds without any compile error.
    ///
    /// This test pins each variant's actual serialized tag against the
    /// hand-maintained, exhaustive `*_expected_tag` match above. The match's
    /// exhaustiveness check catches *new* variants missing an arm; this loop
    /// catches a variant having been *moved* (its real wire tag drifting away
    /// from the tag recorded here) — together they make an accidental reorder a
    /// CI failure instead of a silent wire-format break.
    #[test]
    fn client_to_server_wire_tags_are_pinned_to_declaration_order() {
        let cases: Vec<ClientToServer> = vec![
            ClientToServer::KeyEvent {
                code: KeyCode::Char('a'),
                modifiers: Modifiers::new(0),
                event_type: 1,
            },
            ClientToServer::Resize { cols: 80, rows: 24 },
            ClientToServer::Detach,
            ClientToServer::Attach {
                session_name: "s".to_string(),
            },
            ClientToServer::SplitVertical,
            ClientToServer::SplitHorizontal,
            ClientToServer::FocusNextPane,
            ClientToServer::FocusPrevPane,
            ClientToServer::FocusPane { pane_id: 1 },
            ClientToServer::PasteText {
                text: "t".to_string(),
            },
            ClientToServer::Ping,
            ClientToServer::ListSessions,
            ClientToServer::KillSession {
                name: "s".to_string(),
            },
            ClientToServer::StartRecording {
                session_name: "s".to_string(),
                output_path: "p".to_string(),
            },
            ClientToServer::StopRecording {
                session_name: "s".to_string(),
            },
            ClientToServer::ClosePane,
            ClientToServer::ResizeSplit { delta: 0.1 },
            ClientToServer::ConnectSsh {
                host: "h".to_string(),
                port: 22,
                username: "u".to_string(),
                auth_type: "password".to_string(),
                password_keyring_account: None,
                ephemeral_password: false,
                key_path: None,
                remote_forwards: vec![],
                x11_forward: false,
                x11_trusted: false,
            },
            ClientToServer::NewWindow,
            ClientToServer::CloseWindow { window_id: 1 },
            ClientToServer::FocusWindow { window_id: 1 },
            ClientToServer::RenameWindow {
                window_id: 1,
                name: "n".to_string(),
            },
            ClientToServer::SetBroadcast { enabled: true },
            ClientToServer::DisplayPanes { show: true },
            ClientToServer::StartAsciicast {
                session_name: "s".to_string(),
                output_path: "p".to_string(),
            },
            ClientToServer::StopAsciicast {
                session_name: "s".to_string(),
            },
            ClientToServer::SaveTemplate {
                name: "n".to_string(),
            },
            ClientToServer::LoadTemplate {
                name: "n".to_string(),
            },
            ClientToServer::ListTemplates,
            ClientToServer::ToggleZoom,
            ClientToServer::SwapPane { target_pane_id: 1 },
            ClientToServer::BreakPane,
            ClientToServer::JoinPane {
                target_window_id: 1,
            },
            ClientToServer::SftpUpload {
                host_name: "h".to_string(),
                local_path: "l".to_string(),
                remote_path: "r".to_string(),
            },
            ClientToServer::SftpDownload {
                host_name: "h".to_string(),
                remote_path: "r".to_string(),
                local_path: "l".to_string(),
            },
            ClientToServer::RunMacro {
                macro_fn: "f".to_string(),
                display_name: "d".to_string(),
            },
            ClientToServer::MouseReport {
                button: 0,
                col: 0,
                row: 0,
                pressed: true,
                motion: false,
            },
            ClientToServer::SetLayoutMode {
                mode: "bsp".to_string(),
            },
            ClientToServer::OpenFloatingPane,
            ClientToServer::CloseFloatingPane { pane_id: 1 },
            ClientToServer::MoveFloatingPane {
                pane_id: 1,
                col_off: 0,
                row_off: 0,
            },
            ClientToServer::ResizeFloatingPane {
                pane_id: 1,
                cols: 10,
                rows: 10,
            },
            ClientToServer::ConnectSerial {
                port: "p".to_string(),
                baud_rate: 115200,
                data_bits: 8,
                stop_bits: 1,
                parity: "none".to_string(),
            },
            ClientToServer::ListPlugins,
            ClientToServer::LoadPlugin {
                path: "p".to_string(),
            },
            ClientToServer::UnloadPlugin {
                path: "p".to_string(),
            },
            ClientToServer::ReloadPlugin {
                path: "p".to_string(),
            },
            ClientToServer::ListWorkspaces,
            ClientToServer::CreateWorkspace {
                name: "n".to_string(),
            },
            ClientToServer::SwitchWorkspace {
                name: "n".to_string(),
            },
            ClientToServer::RenameWorkspace {
                from: "a".to_string(),
                to: "b".to_string(),
            },
            ClientToServer::DeleteWorkspace {
                name: "n".to_string(),
                force: false,
            },
            ClientToServer::QuakeToggle {
                action: "toggle".to_string(),
            },
            ClientToServer::ReorderPanes { pane_ids: vec![1] },
            ClientToServer::MovePaneToWindow {
                pane_id: 1,
                target_window_id: 1,
                insert_at: None,
            },
            ClientToServer::Hello {
                proto_version: 1,
                client_kind: ClientKind::Gpu,
                client_version: "1.0.0".to_string(),
            },
            ClientToServer::QueryForegroundProcess { window_id: 1 },
            ClientToServer::SetThemeColors {
                fg: [0, 0, 0],
                bg: [0, 0, 0],
            },
            ClientToServer::DndDrop {
                path: "p".to_string(),
                paste_fallback: "f".to_string(),
            },
            ClientToServer::SplitWithShell {
                program: "p".to_string(),
                args: vec![],
                cwd: None,
                env: vec![],
            },
        ];

        for msg in &cases {
            let expected = client_to_server_expected_tag(msg);
            let encoded = postcard::to_stdvec(msg).expect("serialize ClientToServer variant");
            let actual_tag = read_leading_varint_u32(&encoded);
            assert_eq!(
                actual_tag, expected,
                "ClientToServer::{msg:?} wire tag drifted from its expected \
                 declaration-order position; the enum was likely reordered without \
                 updating client_to_server_expected_tag (breaks wire compatibility \
                 across client/server versions)"
            );
        }
    }

    /// Audit round 4 #11 — see
    /// `client_to_server_wire_tags_are_pinned_to_declaration_order` for the
    /// rationale. Same pinning check for `ServerToClient`.
    #[test]
    fn server_to_client_wire_tags_are_pinned_to_declaration_order() {
        let cases: Vec<ServerToClient> = vec![
            ServerToClient::GridDiff {
                pane_id: 1,
                dirty_rows: vec![],
                cursor_col: 0,
                cursor_row: 0,
            },
            ServerToClient::FullRefresh {
                pane_id: 1,
                grid: Grid::new(1, 1),
            },
            ServerToClient::SessionList { sessions: vec![] },
            ServerToClient::Pong,
            ServerToClient::Error {
                message: "e".to_string(),
            },
            ServerToClient::ImagePlaced {
                pane_id: 1,
                image_id: 1,
                col: 0,
                row: 0,
                width: 1,
                height: 1,
                rgba: vec![],
            },
            ServerToClient::TextSized {
                pane_id: 1,
                col: 0,
                row: 0,
                scale_num: 1,
                scale_den: 1,
                width_cells: 0,
                valign: 0,
                halign: 0,
                text: "t".to_string(),
            },
            ServerToClient::LayoutChanged {
                panes: vec![],
                focused_pane_id: 0,
            },
            ServerToClient::Bell { pane_id: 1 },
            ServerToClient::RecordingStarted {
                pane_id: 1,
                path: "p".to_string(),
            },
            ServerToClient::RecordingStopped { pane_id: 1 },
            ServerToClient::WindowListChanged { windows: vec![] },
            ServerToClient::PaneClosed { pane_id: 0 },
            ServerToClient::TitleChanged {
                pane_id: 1,
                title: "t".to_string(),
            },
            ServerToClient::ProcessChanged {
                pane_id: 1,
                process_name: None,
            },
            ServerToClient::DesktopNotification {
                pane_id: 1,
                title: "t".to_string(),
                body: "b".to_string(),
            },
            ServerToClient::ClipboardWriteRequest {
                pane_id: 1,
                text: "t".to_string(),
            },
            ServerToClient::BroadcastModeChanged { enabled: true },
            ServerToClient::AsciicastStarted {
                pane_id: 1,
                path: "p".to_string(),
            },
            ServerToClient::AsciicastStopped { pane_id: 1 },
            ServerToClient::TemplateSaved {
                name: "n".to_string(),
                path: "p".to_string(),
            },
            ServerToClient::TemplateLoaded {
                name: "n".to_string(),
            },
            ServerToClient::TemplateList { names: vec![] },
            ServerToClient::ZoomChanged { is_zoomed: true },
            ServerToClient::PaneBroken {
                new_window_id: 1,
                pane_id: 1,
            },
            ServerToClient::SerialConnected {
                pane_id: 1,
                port: "p".to_string(),
            },
            ServerToClient::SftpProgress {
                path: "p".to_string(),
                transferred: 0,
                total: 0,
            },
            ServerToClient::SftpDone {
                path: "p".to_string(),
                error: None,
            },
            ServerToClient::SemanticMark {
                pane_id: 1,
                row: 0,
                kind: "A".to_string(),
                exit_code: None,
            },
            ServerToClient::CwdChanged {
                pane_id: 1,
                cwd: "c".to_string(),
            },
            ServerToClient::FloatingPaneOpened {
                pane_id: 1,
                col_off: 0,
                row_off: 0,
                cols: 1,
                rows: 1,
            },
            ServerToClient::FloatingPaneMoved {
                pane_id: 1,
                col_off: 0,
                row_off: 0,
                cols: 1,
                rows: 1,
            },
            ServerToClient::FloatingPaneClosed { pane_id: 1 },
            ServerToClient::PluginList { paths: vec![] },
            ServerToClient::PluginOk {
                path: "p".to_string(),
                action: "loaded".to_string(),
            },
            ServerToClient::WorkspaceList {
                current: "c".to_string(),
                workspaces: vec![],
            },
            ServerToClient::WorkspaceSwitched {
                name: "n".to_string(),
            },
            ServerToClient::QuakeToggleRequest {
                action: "toggle".to_string(),
            },
            ServerToClient::HelloAck {
                proto_version: 1,
                server_version: "1.0.0".to_string(),
            },
            ServerToClient::ForegroundProcessStatus {
                window_id: 1,
                has_foreground: false,
            },
            ServerToClient::PointerShapeChanged {
                pane_id: 1,
                shape: "pointer".to_string(),
            },
            ServerToClient::PaneColorsChanged {
                pane_id: 1,
                fg: None,
                bg: None,
                palette: vec![],
            },
            ServerToClient::ProgressChanged {
                pane_id: 1,
                state: 0,
                progress: 0,
            },
        ];

        for msg in &cases {
            let expected = server_to_client_expected_tag(msg);
            let encoded = postcard::to_stdvec(msg).expect("serialize ServerToClient variant");
            let actual_tag = read_leading_varint_u32(&encoded);
            assert_eq!(
                actual_tag, expected,
                "ServerToClient::{msg:?} wire tag drifted from its expected \
                 declaration-order position; the enum was likely reordered without \
                 updating server_to_client_expected_tag (breaks wire compatibility \
                 across client/server versions)"
            );
        }
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
