# nexterm Architecture Design Document

## Overview

nexterm is a daemonless Rust terminal multiplexer.
The server process keeps PTY sessions alive while clients can connect and disconnect at any time.
It provides two types of frontends: a GPU client (wgpu) and a TUI client (ratatui).

---

## Crate Dependency Graph

```
nexterm-client-gpu
  └── nexterm-proto
  └── nexterm-config

nexterm-client-tui
  └── nexterm-proto

nexterm-server
  └── nexterm-proto
  └── nexterm-vt

nexterm-proto   (shared types and message definitions)
nexterm-vt      (VT100 parser and virtual screen)
nexterm-config  (TOML + Lua configuration)
```

There are no circular dependencies. `nexterm-proto` is the sole shared crate and defines all IPC types.

---

## Process Architecture

```
┌───────────────────────────────────────┐
│       nexterm-client-gpu / tui         │
│   winit event loop / crossterm         │
│   wgpu renderer / ratatui renderer     │
└──────────────────┬────────────────────┘
                   │ IPC (postcard / Named Pipe / Unix Socket)
┌──────────────────▼────────────────────┐
│          nexterm-server                │
│   SessionManager                       │
│     └── Session                        │
│           └── Window (BSP layout)      │
│                 └── Pane (PTY mgmt)    │
└──────────────────┬────────────────────┘
                   │ portable-pty
┌──────────────────▼────────────────────┐
│     OS PTY (ConPTY / Unix PTY)         │
│     Shell / Application                │
└───────────────────────────────────────┘
```

---

## Server-Side Architecture

### Session Hierarchy

```
SessionManager
  └── HashMap<String, Session>   (session name → Session)
        └── Session
              ├── name: String
              ├── cols, rows: u16   (terminal overall size)
              ├── client_tx: Option<Sender<ServerToClient>>
              └── HashMap<u32, Window>  (Window ID → Window)
                    └── Window
                          ├── id, name: String
                          ├── focused_pane_id: u32
                          ├── layout: SplitNode        (BSP tree)
                          └── HashMap<u32, Pane>       (Pane ID → Pane)
                                └── Pane
                                      ├── id: u32
                                      ├── cols, rows: u16
                                      ├── shared_tx: Arc<Mutex<Sender<ServerToClient>>>
                                      ├── master: Box<dyn MasterPty>
                                      └── writer: Mutex<Box<dyn Write>>
```

### PTY Reader Thread

Each `Pane` spawns a reader thread via `tokio::task::spawn_blocking` at creation time.

```
PTY reader thread (blocking)
  loop {
    reader.read(&mut buf)
    log_writer.write_all(&buf)     ← only when recording is active
    VtParser::advance(buf)
    Screen::take_dirty_rows()      → send GridDiff message
    Screen::take_pending_images()  → send ImagePlaced message
    Screen::take_pending_bell()    → send Bell message
  }
```

The recording log writer is held as `Arc<Mutex<Option<BufWriter<File>>>>` and swapped by the main thread via `start_recording()` / `stop_recording()`.

The PTY output channel is held as `Arc<Mutex<Sender<ServerToClient>>>` and swapped via `update_tx()` on client reconnect. This is what enables the daemonless design.

### Client Reconnect Flow

```
Client connects
  → IPC::Attach { session_name }
  → get_or_create_and_attach()
      → Session::attach(new_tx)
          → window.update_tx_for_all(&tx)   (replace Sender for all panes)
  → send FullRefresh
  → send LayoutChanged
  → send SessionList
```

---

## BSP Layout Engine

### Data Structure

```rust
enum SplitNode {
    Pane { pane_id: u32 },
    Split {
        dir: SplitDir,   // Vertical (left/right) | Horizontal (top/bottom)
        ratio: f32,      // fraction occupied by the left/top child (0.0–1.0)
        left: Box<SplitNode>,
        right: Box<SplitNode>,
    },
}
```

### Pane Split Procedure (Resolving the Chicken-and-Egg Problem)

1. Pre-allocate an ID with `new_pane_id()`
2. Insert into the tree with `layout.insert_after(focused_id, new_id, dir)`
3. Recursively compute all pane rectangles with `compute_layouts(cols, rows)`
4. Spawn the pane with `Pane::spawn_with_id(new_id, rect.cols, rect.rows, ...)`
5. Resize existing panes to their new sizes

### Layout Calculation (Recursive)

```
compute(col_off, row_off, cols, rows, out):
  Pane → out.push(PaneRect { pane_id, col_off, row_off, cols, rows })
  Split(Vertical):
    left_cols = floor(cols * ratio)
    right_cols = cols - left_cols - 1   // 1-column border
    compute(left, col_off, ...)
    compute(right, col_off + left_cols + 1, ...)
  Split(Horizontal):
    top_rows = floor(rows * ratio)
    bot_rows = rows - top_rows - 1      // 1-row border
    compute(left, row_off, ...)
    compute(right, row_off + top_rows + 1, ...)
```

---

## IPC Layer

### Transport

| OS | Transport | Path |
|----|-----------|------|
| Linux / macOS | Unix Domain Socket | `$XDG_RUNTIME_DIR/nexterm.sock` |
| Windows | Named Pipe | `\\.\pipe\nexterm-<USERNAME>` |

### Framing

All messages are sent and received as a 4-byte LE length prefix followed by a postcard payload.

```
┌────────────────┬─────────────────────────┐
│ 4B (LE u32)    │ N bytes (postcard)       │
│ payload length │ message body             │
└────────────────┴─────────────────────────┘
```

### Security

**Unix Domain Socket — UID Peer Verification**

After accepting a connection, the kernel API is used to retrieve the client UID and compare it against the server UID. A mismatch results in immediate disconnection.

| OS | API |
|----|-----|
| Linux | `getsockopt(SO_PEERCRED)` → `ucred.uid` |
| macOS / BSD | `libc::getpeereid(fd, &uid, &gid)` |
| Other Unix | UID verification skipped (warning log only) |

**Windows Named Pipe**

`ServerOptions::reject_remote_clients(true)` rejects connections from outside the local machine.

**Path Traversal Prevention**

The `StartRecording { path }` handler calls `validate_recording_path()` to reject `..` components and empty paths upfront.

### Thread Model (Server Side)

```
tokio::spawn(handle_client)
  ├── tokio::spawn (send loop: rx → write_half)
  └── recv loop:  read_half → dispatch()
```

Two async tasks are spawned per client connection (send and receive are separated).

---

## VT Parser

The `nexterm-vt` crate wraps the `vte` crate and manages a virtual screen.

```
VtParser
  ├── vte::Parser     (byte stream → callbacks)
  └── Screen
        ├── Grid (Cell[][] : virtual grid)
        ├── dirty: Vec<bool>     (per-row dirty flags)
        ├── cursor: (u16, u16)
        └── pending_images: Vec<PendingImage>
```

### Dirty Diff Delivery

- `Screen::take_dirty_rows()` extracts dirty rows as `Vec<DirtyRow>`
- `DirtyRow { row: u16, cells: Vec<Cell> }` is sent to the client as a `GridDiff` message
- The client merges the received diff into its local grid

### Image Protocol

| Protocol | Decoder | Sent Message |
|----------|---------|--------------|
| Sixel | Decode DCS `q` sequence | `ImagePlaced { rgba, width, height, col, row }` |
| Kitty | Decode APC `G` sequence | Same as above |

---

## GPU Client (nexterm-client-gpu)

### Rendering Pipeline

A 3-pass pipeline combining wgpu custom shaders with a cosmic-text glyph atlas.

```
Render Pass
  ├── Pass 1: Background rectangles (bg_verts)
  │     └── Background color of each grid cell + cursor rectangle
  ├── Pass 2: Text (text_verts)
  │     └── UV sampling from cosmic-text glyph atlas
  └── Pass 3: Images (img_verts)
        └── ImagePlaced RGBA textures
```

### Multi-Pane Rendering

When `pane_layouts` is non-empty (server is connected), each `PaneLayout`'s offset is used to draw each pane at its correct position.

```
for layout in pane_layouts:
  off_x = layout.col_offset * cell_w
  off_y = layout.row_offset * cell_h
  rect  = (off_x, off_y, layout.cols * cell_w, layout.rows * cell_h)

  if scroll_active:
    build_scrollback_verts_in_rect(pane, layout, rect)
  else:
    build_grid_verts_in_rect(pane, layout, rect)
    build_border_verts(pane, layout)   // border between adjacent panes
```

Text color in non-focused panes is dimmed to 70%.
Focused pane borders are blue `[0.30, 0.55, 0.90]`; non-focused panes use gray `[0.35, 0.35, 0.42]`.

### Event Loop (winit 0.30 ApplicationHandler)

```
ApplicationHandler
  ├── new_events()          — application startup
  ├── resumed()             — window creation (transparency/decorations from config) · wgpu init
  ├── window_event()
  │     ├── KeyboardInput   → ClientToServer::KeyEvent
  │     │     ├── Ctrl+=/-/0          → change_font_size / reset_font_size
  │     │     ├── Ctrl+[              → copy_mode.enter()
  │     │     ├── Keys in copy mode   → handle_copy_mode_key()
  │     │     └── Other               → forward to PTY
  │     ├── MouseInput (Left, Ctrl)   → find_url_at() → open_url()
  │     ├── Resized         → ClientToServer::Resize
  │     └── CloseRequested  → exit
  └── about_to_wait()
        ├── Poll PTY output (16ms interval) → apply server messages
        ├── Check pending_bell → request_user_attention()
        ├── Evaluate status bar (every 1 second)
        └── Redraw
```

### Client State Management (ClientState)

```
ClientState
  ├── panes: HashMap<u32, PaneState>      (received grids)
  ├── focused_pane_id: Option<u32>
  ├── pane_layouts: HashMap<u32, PaneLayout>
  ├── palette: CommandPalette             (command palette)
  ├── search: SearchState                 (incremental search)
  ├── pending_bell: bool                  (BEL flag)
  └── copy_mode: CopyModeState            (Vim-style copy mode)

PaneState
  ├── grid: Grid
  ├── cursor_col, cursor_row: u16
  ├── scrollback: Scrollback
  ├── scroll_offset: usize
  ├── images: HashMap<u32, PlacedImage>
  └── has_activity: bool                  (output-while-unfocused flag)

CopyModeState
  ├── is_active: bool
  ├── cursor_col, cursor_row: u16
  └── selection_start: Option<(u16, u16)>
```

URL detection is performed by `detect_urls_in_row()`, which scans for URLs starting with `https://` or `http://` and returns them as `DetectedUrl { row, col_start, col_end, url }`.
On Ctrl+Click, `find_url_at(col, row)` locates any URL at the click position and opens it in the OS default browser.

---

## TUI Client (nexterm-client-tui)

A lightweight fallback client built with ratatui + crossterm.
Intended for environments where a GPU is unavailable (e.g., over SSH).

- Single-pane display (uses only the `is_focused` field from BSP layout information)
- Converts `crossterm` key events to `ClientToServer::KeyEvent` and sends them
- Renders the grid using a `ratatui` `Paragraph` widget

---

## Configuration System (nexterm-config)

### Load Order

```
1. Default values (Rust Default trait)
2. Load config.toml (TOML deserialization)
3. Execute config.lua (Lua overrides)
4. Merge result into Config struct
```

### Hot Reload

File system events are monitored using the `notify` crate.
When a configuration file changes, `ConfigWatcher` reloads it and updates the `Config`.

### Configuration Schema

| Field | Type | Default |
|-------|------|---------|
| `font.family` | String | `"monospace"` |
| `font.size` | f32 | `14.0` |
| `font.ligatures` | bool | `true` |
| `colors` | ColorScheme | `dark` |
| `shell.program` | String | OS-dependent |
| `scrollback_lines` | usize | `50000` |
| `status_bar.enabled` | bool | `false` |
| `window.background_opacity` | f32 | `1.0` |
| `window.decorations` | WindowDecorations | `Full` |
| `window.macos_window_background_blur` | u32 | `0` |
| `tab_bar.enabled` | bool | `true` |
| `tab_bar.height` | u32 | `28` |
| `tab_bar.active_tab_bg` | String | `"#ae8b2d"` |
| `tab_bar.inactive_tab_bg` | String | `"#5c6d74"` |
| `tab_bar.separator` | String | `"❯"` |

---

## Session Persistence

On shutdown, the server saves the full session state as a JSON snapshot and automatically restores it on the next startup (`nexterm-server/src/persist.rs`, `src/snapshot.rs`).

```
# Save path
Linux / macOS: ~/.local/state/nexterm/snapshot.json
Windows:       %APPDATA%\nexterm\snapshot.json
```

### Snapshot Type Hierarchy

```rust
ServerSnapshot
  ├── version: u32                         // for compatibility checking
  ├── saved_at: u64                        // Unix timestamp
  └── sessions: Vec<SessionSnapshot>
        └── SessionSnapshot
              ├── name, shell, cols, rows
              ├── focused_window_id: u32
              └── windows: Vec<WindowSnapshot>
                    └── WindowSnapshot
                          ├── id, name, focused_pane_id
                          └── layout: SplitNodeSnapshot
                                ├── Pane { pane_id, cwd: Option<PathBuf> }
                                └── Split { dir, ratio, left, right }
```

### Restore Flow on Startup

```
persist::load_snapshot()
  → SessionManager::restore_from_snapshot()
      for each SessionSnapshot:
        Session::restore_from_snapshot()
          → Window::restore_from_snapshot()
              → compute_pane_sizes() to calculate each pane's rectangle
              → Pane::spawn_with_cwd(id, cols, rows, tx, shell, cwd)
  → set_min_pane_id(max_pane_id + 1)     // prevent ID collisions via AtomicU32::fetch_max
  → set_min_window_id(max_window_id + 1)
```

### Working Directory Resolution (Linux only)

```rust
// Pane::read_working_dir() — Linux implementation
std::fs::read_link(format!("/proc/{}/cwd", pid))
```

Returns `None` on non-Linux platforms (the shell starts in its default directory).

---

## Error Handling Policy

- All errors propagate via `anyhow::Result`
- Deserialization errors in the IPC receive loop use `continue` (discard the single message)
- PTY read errors use `break` (terminate the thread; the pane becomes invalid)
- Client disconnection is not an error but a normal condition (`read_exact` Err → `break` → detach)

---

## Test Strategy

> The per-crate breakdown below is the original Phase 3 baseline. As of 2026-05-17 the workspace has grown to **660+ passing tests** across unit / integration / proptest. Re-run `cargo test --workspace` for the latest count.

| Layer | Test Coverage | Count (baseline) |
|-------|--------------|-------|
| nexterm-proto | postcard round-trip serialization | 4 |
| nexterm-vt | VT sequences, dirty flags, resize | 6 |
| nexterm-config | default construction, TOML round-trip, LuaWorker async evaluation | 5 |
| nexterm-server | BSP calculation, session management, IPC path validation, snapshot round-trip | 14 |
| nexterm-client-gpu | ClientState message application, search lifecycle, `hex_to_rgba`, ANSI256 | 21 |
| nexterm-client-tui | ClientState message application | 2 |
| **Total (current, 2026-05-17)** | unit + integration + proptest across the workspace | **660+** |

---

## Phase 3 Completed Tasks

| Step | Description | Status |
|------|-------------|--------|
| 3-4 | Mouse support (click focus / wheel scroll) | ✅ Done |
| 3-5 | Clipboard integration (arboard crate, Ctrl+Shift+C/V) | ✅ Done |
| 3-6 | nexterm-ctl CLI (list / new / attach / kill) | ✅ Done |
| 3-7 | Config hot reload → reflected in GPU client | ✅ Done |
| 3-8 | Lua status bar widget | ✅ Done |

### 3-4: Mouse Support Implementation Details

| Event | Handling |
|-------|---------|
| `CursorMoved` | Store cursor position in `cursor_position: Option<(f64, f64)>` |
| `MouseInput Left Released` | Cell coordinates → search `pane_layouts` → send `FocusPane` |
| `MouseWheel` | `LineDelta` / `PixelDelta` → `scroll_up` / `scroll_down` |

### 3-5: Clipboard Integration

| Shortcut | Behavior |
|----------|---------|
| `Ctrl+Shift+C` | Convert visible grid of focused pane to text and copy |
| `Ctrl+Shift+V` | Paste from clipboard to PTY via `PasteText` message |

### 3-6: nexterm-ctl Command Reference

```
nexterm-ctl list              List all sessions
nexterm-ctl new <name>        Create a new session
nexterm-ctl attach <name>     Show instructions for attaching
nexterm-ctl kill <name>       Force-terminate a session
```

IPC: Two new protocol messages added — `ListSessions` and `KillSession`.

### 3-7: Config Hot Reload

`nexterm-config::watch_config()` monitors `~/.config/nexterm/`.
On change detection, the updated `Config` is received; if fonts changed, the glyph atlas is also regenerated.

### 3-8: Lua Status Bar Widget

`nexterm-config::StatusBarEvaluator` evaluates Lua expressions via `LuaWorker`.
Re-evaluated every second in `about_to_wait` and displayed on the right side of the status line.

`LuaWorker` holds a `mlua::Lua` instance on a dedicated OS thread (`nexterm-lua-worker`) and communicates with the main thread via channels (preventing the main thread from blocking).

```
about_to_wait (winit, every 1 second)
  └── StatusBarEvaluator::evaluate_widgets(&widgets)
        └── LuaWorker::eval_widgets()
              ├── Return cached value immediately from Arc<Mutex<String>>
              └── SyncChannel::try_send(request) → Lua worker thread
                    └── Lua::eval() → Arc<Mutex<String>>.write()
```

**nexterm.lua configuration example:**

```lua
return {
  status_bar = {
    enabled = true,
    widgets = { 'os.date("%H:%M:%S")', '"nexterm"' },
  }
}
```

---

## Phase 5 Completed Tasks

| Step | Description | Status |
|------|-------------|--------|
| 5-G | VT BEL notification → OS window attention request | ✅ Done |
| 5-E | Runtime font size change (Ctrl+= / Ctrl+- / Ctrl+0) | ✅ Done |
| 5-A | Session recording (`nexterm-ctl record start/stop`) | ✅ Done |
| 5-C | Window transparency, blur, and borderless mode (config-driven) | ✅ Done |
| 5-D | Vim-style copy mode (Ctrl+[, hjkl, v, y) | ✅ Done |
| 5-F | URL detection + Ctrl+Click to open browser | ✅ Done |
| 5-B | WezTerm-style tab bar (`❯` separator) | ✅ Done |

### 5-A: Session Recording Implementation Details

```
Pane
  └── log_writer: Arc<Mutex<Option<BufWriter<File>>>>

start_recording(path)
  → File::create(path) → BufWriter → *guard = Some(writer)

stop_recording()
  → writer.flush() → *guard = None

PTY read loop
  → after reading buf → log_writer.write_all(&buf)
```

### 5-B: Tab Bar Rendering

`build_tab_bar_verts()` sorts `pane_layouts` by ID in ascending order and renders a tab for each pane.
- Active tab: `active_tab_bg` color, bold label
- Inactive tab: `inactive_tab_bg` color, normal label
- Separator: `cfg.separator` (default `❯`) displayed between each tab

### 5-D: Copy Mode State Transitions

```
Normal mode ──[Ctrl+[]──> Copy mode
                          ├── hjkl → move cursor
                          ├── v    → toggle selection_start
                          ├── y    → yank_selection() → clipboard → exit
                          └── q/Esc → exit
```

### 5-F: URL Detection

`detect_urls_in_row(row_idx, cells)` scans a row's text for `https://` / `http://` URLs and returns them as `DetectedUrl { row, col_start, col_end, url }`.

On Ctrl+Click, if `find_url_at(col, row)` finds a hit, the browser is launched using a platform-specific command:
- Windows: `cmd /c start <url>`
- macOS: `open <url>`
- Linux: `xdg-open <url>`
