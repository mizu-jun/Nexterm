# nexterm IPC Protocol Specification

## Overview

Communication between nexterm's client and server is defined using **bincode** serialization with a **4-byte little-endian length prefix** framing.
The transport layer differs by OS, but the framing and message format are common across all platforms.

---

## Transport Layer

| OS | Transport | Path |
|----|-----------|------|
| Linux / macOS | Unix Domain Socket | `$XDG_RUNTIME_DIR/nexterm.sock` (mode 0600) |
| Windows | Named Pipe | `\\.\pipe\nexterm-<USERNAME>` |

---

## Framing

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
├─────────────────────────────────────────────────────────────────┤
│              Payload Length (u32, little-endian)                │
├─────────────────────────────────────────────────────────────────┤
│              Payload (bincode-encoded message)                  │
│                        (variable length)                        │
└─────────────────────────────────────────────────────────────────┘
```

- `Payload Length` is the number of bytes in the payload (not including the header itself)
- The payload is an enum encoded with `bincode` default settings

---

## Client → Server Messages (`ClientToServer`)

### `Ping`

Connection health check. The server responds with `Pong`.

```
{ Ping }
```

### `Attach`

Attaches to a session. Creates a new session if one does not exist.

```
{ Attach { session_name: String } }
```

**Response**: `FullRefresh` → `LayoutChanged` → `SessionList` (sent in order)

### `Detach`

Detaches from the session (intentional client disconnect).

```
{ Detach }
```

### `KeyEvent`

Key input event. The server writes it to the focused pane's PTY.

```
{ KeyEvent { code: KeyCode, modifiers: Modifiers } }
```

#### `KeyCode` Values

| Value | Description |
|-------|-------------|
| `Char(char)` | Regular character |
| `F(u8)` | Function key F1–F12 |
| `Enter` | Enter key |
| `Backspace` | Backspace key |
| `Delete` | Delete key |
| `Escape` | Escape key |
| `Tab` | Tab key |
| `BackTab` | Shift+Tab |
| `Up` / `Down` / `Left` / `Right` | Arrow keys |
| `Home` / `End` | Home / End keys |
| `PageUp` / `PageDown` | Page scroll keys |
| `Insert` | Insert key |

#### `Modifiers` Bit Flags

| Bit | Modifier Key |
|-----|-------------|
| `0b0001` | Shift |
| `0b0010` | Ctrl |
| `0b0100` | Alt |
| `0b1000` | Meta (Super) |

### `Resize`

Terminal resize notification. The server recalculates and repositions all panes using BSP layout.

```
{ Resize { cols: u16, rows: u16 } }
```

**Response**: `LayoutChanged`

### `SplitVertical`

Splits the focused pane vertically into left and right halves (equal split with a 1-column divider).

```
{ SplitVertical }
```

**Response**: `FullRefresh` → `LayoutChanged`

### `SplitHorizontal`

Splits the focused pane horizontally into top and bottom halves (equal split with a 1-row divider).

```
{ SplitHorizontal }
```

**Response**: `FullRefresh` → `LayoutChanged`

### `FocusNextPane`

Moves focus to the next pane in ascending pane ID order.

```
{ FocusNextPane }
```

**Response**: `LayoutChanged`

### `FocusPrevPane`

Moves focus to the previous pane in ascending pane ID order.

```
{ FocusPrevPane }
```

**Response**: `LayoutChanged`

### `FocusPane`

Moves focus to the pane with the specified ID (e.g., from a mouse click).

```
{ FocusPane { pane_id: u32 } }
```

**Response**: `LayoutChanged`

### `PasteText`

Writes text directly to the focused pane's PTY (used for clipboard paste).

```
{ PasteText { text: String } }
```

**Response**: None (write to PTY only)

### `ListSessions`

Retrieves the session list without attaching (used by `nexterm-ctl list`).

```
{ ListSessions }
```

**Response**: `SessionList`

### `KillSession`

Force-terminates the specified session (drops the PTY process).

```
{ KillSession { name: String } }
```

**Response**: `SessionList` (updated list after termination) or `Error`

### `StartRecording`

Starts recording the PTY output of the focused pane to a file.

```
{ StartRecording { session_name: String, output_path: String } }
```

**Response**: `RecordingStarted` or `Error`

### `StopRecording`

Stops the recording.

```
{ StopRecording { session_name: String } }
```

**Response**: `RecordingStopped` or `Error`

### `ClosePane`

Closes the focused pane (removes the BSP node and promotes the sibling). Returns `Error` if only one pane exists.

```
{ ClosePane }
```

**Response**: `PaneClosed` → `LayoutChanged` or `Error`

---

### `ResizeSplit`

Adjusts the split ratio of the focused pane.

```
{ ResizeSplit { delta: f32 } }
```

- `delta > 0`: Expands the focused pane
- `delta < 0`: Shrinks the focused pane
- The ratio is clamped to `[0.1, 0.9]`

**Response**: `LayoutChanged`

---

### `NewWindow`

Creates a new window (tab) and moves focus to it.

```
{ NewWindow }
```

**Response**: `WindowListChanged` → `FullRefresh` → `LayoutChanged`

---

### `CloseWindow`

Closes the specified window. The last remaining window cannot be closed.

```
{ CloseWindow { window_id: u32 } }
```

**Response**: `WindowListChanged` or `Error`

---

### `FocusWindow`

Moves focus to the specified window.

```
{ FocusWindow { window_id: u32 } }
```

**Response**: `WindowListChanged` → `LayoutChanged` → `FullRefresh`

---

### `RenameWindow`

Renames the specified window.

```
{ RenameWindow { window_id: u32, name: String } }
```

**Response**: `WindowListChanged` or `Error`

---

### `SetBroadcast`

Toggles broadcast input mode. When enabled, input is simultaneously written to all panes in the focused window.

```
{ SetBroadcast { enabled: bool } }
```

**Response**: None

---

### `ConnectSsh`

Connects to the specified host via SSH and opens a PTY channel in a new pane.

```
{
    ConnectSsh {
        host: String,
        port: u16,
        username: String,
        auth_type: String,   // "password" | "key" | "agent"
        // Sprint 5-1 / G1: 旧 `password: Option<String>` を廃止。
        // クライアントが OS keyring (Service="nexterm-ssh") に
        // Account=`<username>@<host_name>` で保存し、サーバーがそれを取得する。
        password_keyring_account: Option<String>,
        // true の場合、サーバーは認証完了 (成功・失敗のいずれも) 後に
        // keyring エントリを削除する (PasswordModal.remember=false 用)。
        ephemeral_password: bool,
        key_path: Option<String>,
        remote_forwards: Vec<String>,
        x11_forward: bool,
        x11_trusted: bool,
    }
}
```

**Response**: `FullRefresh` → `LayoutChanged` or `Error`

**互換性**: PROTOCOL_VERSION 2 で導入された非互換変更。v1 クライアントは
Hello ハンドシェイク段階で拒否される。

---

## Server → Client Messages (`ServerToClient`)

### `Pong`

Response to `Ping`.

```
{ Pong }
```

### `FullRefresh`

Full grid snapshot of a pane. Sent on attach and when a pane is created.

```
{ FullRefresh { pane_id: u32, grid: Grid } }
```

#### `Grid` Struct

```
Grid {
    width: u16,
    height: u16,
    rows: Vec<Vec<Cell>>,   // rows[y][x]
    cursor_col: u16,
    cursor_row: u16,
}
```

#### `Cell` Struct

```
Cell {
    ch: char,                  // Display character (default: ' ')
    fg: Color,                 // Foreground color
    bg: Color,                 // Background color
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}
```

#### `Color` Type

```
Color = Reset | Indexed(u8) | Rgb(u8, u8, u8)
```

### `GridDiff`

Differential grid update. Sends only the dirty (changed) rows.

```
{
    GridDiff {
        pane_id: u32,
        dirty_rows: Vec<DirtyRow>,
        cursor_col: u16,
        cursor_row: u16,
    }
}
```

#### `DirtyRow` Struct

```
DirtyRow {
    row: u16,              // Row index (0-based)
    cells: Vec<Cell>,      // Full cell array for the row
}
```

### `LayoutChanged`

Notification of a pane layout change. Sent on split, focus change, or resize.

```
{
    LayoutChanged {
        panes: Vec<PaneLayout>,
        focused_pane_id: u32,
    }
}
```

#### `PaneLayout` Struct

```
PaneLayout {
    pane_id: u32,
    col_offset: u16,   // Column offset within the window (0-based)
    row_offset: u16,   // Row offset within the window (0-based)
    cols: u16,         // Pane width
    rows: u16,         // Pane height
    is_focused: bool,
}
```

### `SessionList`

Session list. Sent after a successful `Attach`.

```
{ SessionList { sessions: Vec<SessionInfo> } }
```

#### `SessionInfo` Struct

```
SessionInfo {
    name: String,
    window_count: u32,
    attached: bool,
}
```

### `ImagePlaced`

Image placement notification for Sixel / Kitty protocol images.

```
{
    ImagePlaced {
        pane_id: u32,
        image_id: u32,
        col: u16,        // Placement column on the grid
        row: u16,        // Placement row on the grid
        width: u32,      // Image width in pixels
        height: u32,     // Image height in pixels
        rgba: Vec<u8>,   // RGBA pixel data (width * height * 4 bytes)
    }
}
```

### `Bell`

Sent when the focused pane receives a VT BEL (`\x07`). The client triggers an OS window attention request (`request_user_attention`).

```
{ Bell { pane_id: u32 } }
```

### `RecordingStarted`

Confirmation that recording has started.

```
{ RecordingStarted { pane_id: u32, path: String } }
```

### `RecordingStopped`

Confirmation that recording has stopped.

```
{ RecordingStopped { pane_id: u32 } }
```

### `Error`

Error notification. Sent when an operation fails.

```
{ Error { message: String } }
```

### `PaneClosed`

Notifies that a pane has been closed. The client removes the pane from rendering.

```
{ PaneClosed { pane_id: u32 } }
```

---

### `WindowListChanged`

Notification of a window list change (on create, close, rename, or focus change).

```
{ WindowListChanged { windows: Vec<WindowInfo> } }
```

#### `WindowInfo` Struct

```
WindowInfo {
    window_id: u32,
    name: String,
    pane_count: u32,
    is_focused: bool,
}
```

---

### `TitleChanged`

Terminal title change notification via OSC 0/2. The client updates the window title or tab label.

```
{ TitleChanged { pane_id: u32, title: String } }
```

---

### `DesktopNotification`

Notification via OSC 9. The client displays an OS desktop notification.

```
{ DesktopNotification { pane_id: u32, title: String, body: String } }
```

---

## Connection Sequence Diagrams

### Standard Client (GPU / TUI)

```
Client                          Server
  │                               │
  │──── Attach { "main" } ───────>│
  │                               │ get_or_create_and_attach()
  │<─── FullRefresh { pane_id=1 }─│
  │<─── LayoutChanged { panes }───│
  │<─── SessionList { sessions }──│
  │                               │
  │──── Resize { 220, 60 } ──────>│
  │<─── LayoutChanged { panes }───│
  │                               │
  │──── KeyEvent { 'l', CTRL } ──>│
  │                    write to PTY│
  │<─── GridDiff { pane_id=1 } ───│
  │                               │
  │──── SplitVertical ───────────>│
  │<─── FullRefresh { pane_id=2 }─│
  │<─── LayoutChanged { panes }───│
  │                               │
  │──── Detach ──────────────────>│
  │  (close connection)           │ session.detach()
```

### nexterm-ctl list

```
nexterm-ctl                     Server
  │                               │
  │──── ListSessions ────────────>│
  │<─── SessionList { sessions }──│
  │  (close connection)           │
```

### nexterm-ctl kill

```
nexterm-ctl                     Server
  │                               │
  │──── KillSession { "main" } ──>│
  │                               │ sessions.remove("main")
  │<─── SessionList { sessions }──│
  │  (close connection)           │
```

### Clipboard Paste (Ctrl+Shift+V)

```
GPU Client                      Server
  │  arboard::get_text()          │
  │──── PasteText { text } ──────>│
  │                    write to PTY│
  │<─── GridDiff { pane_id=N } ───│
```

### Session Recording (nexterm-ctl record)

```
nexterm-ctl                     Server
  │                               │
  │── StartRecording { "main",    │
  │       "output.log" } ────────>│  create BufWriter<File>
  │<── RecordingStarted { pane=1, │
  │        path="output.log" } ───│
  │  (close connection)           │
  │                               │  PTY → parser → BufWriter (background)
  │                               │
  │── StopRecording { "main" } ──>│  BufWriter::flush + drop
  │<── RecordingStopped { pane=1 }│
  │  (close connection)           │
```

### VT BEL Notification

```
PTY (Shell)         Server                GPU Client
  │                   │                       │
  │── \x07 ──────────>│                       │
  │              take_pending_bell()           │
  │<── Bell { pane_id=1 } ───────────────────>│
  │                   │          request_user_attention()
  │                   │               (OS window flash)
```

---

## Versioning

The current protocol version is **1.0**.
Uses bincode default settings (little-endian, fixed-width integers).
When changing the protocol, avoid backward-incompatible changes; extend by adding new message variants instead.
