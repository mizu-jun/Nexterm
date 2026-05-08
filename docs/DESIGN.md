# nexterm Design Document

## Design Goals

1. **Daemonless reconnect** — Sessions remain alive even when the client disconnects
2. **Fast rendering** — Near-zero-copy grid rendering via GPU (wgpu)
3. **Simple state management** — The client holds a copy of the grid and receives only diffs
4. **Cross-platform** — Supports Linux / macOS / Windows from a single codebase

---

## ADR-001: Daemonless Architecture

### Background

tmux keeps a persistent server process (daemon) to manage sessions.
nexterm similarly has a server process that holds PTYs, but rather than calling it a "daemon," the design allows users to explicitly start and stop it.

### Decision

- `nexterm-server` operates as an independent server process
- Clients can connect and disconnect at any time
- PTY processes continue running while clients are disconnected

### Implementation

The PTY read thread of each `Pane` holds an `Arc<Mutex<Sender<ServerToClient>>>`.
On client reconnect, it is sufficient to swap the channel via `update_tx()`.

---

## ADR-002: BSP Layout Engine

### Background

tmux splits panes with `%` / `"` commands.
nexterm adopts a BSP (Binary Space Partition) tree to handle arbitrary-depth splits uniformly.

### Decision

```rust
enum SplitNode {
    Pane { pane_id: u32 },
    Split { dir, ratio, left, right },
}
```

- Splits always bisect the focused pane
- Default ratio is 50:50 (drag-to-resize planned for the future)
- Borders are fixed at 1 cell width

### Trade-offs

- **Pro**: Recursive computation can represent arbitrarily complex layouts
- **Con**: An "equal 3-way split" as in tmux cannot be expressed directly (workaround: adjust intermediate node ratios)

---

## ADR-003: wgpu for the GPU Client

### Background

CPU-based text rendering for a terminal emulator requires heavy string processing.
Using the GPU enables fast rendering by UV-sampling from a glyph atlas.

### Decision

- wgpu: cross-platform GPU API (Vulkan / Metal / DX12 / WebGPU)
- cosmic-text: glyph rasterization with Unicode and CJK support

### Render Pass Layout

```
Pass 1: Background rectangles (cell background colors, cursor)
Pass 2: Text (sampling from the glyph atlas)
Pass 3: Images (Sixel/Kitty RGBA textures)
```

---

## ADR-004: bincode for IPC Format

### Background

JSON is human-readable but slow. Protobuf requires a schema definition and has poor ergonomics with Rust.

### Decision

Adopt the `bincode` crate.

- **Pro**: Fully integrated with Rust's `serde`; only type definitions are needed
- **Pro**: Extremely fast with minimal overhead
- **Con**: Not human-readable (binary must be dumped for debugging)
- **Con**: Cross-language interoperability is difficult (not a concern at this time)

---

## ADR-005: Two-Layer Config with TOML + Lua

### Background

TOML is ideal for static configuration. However, dynamic processing such as status bar widgets requires a scripting language.

### Decision

- `config.toml`: static settings such as defaults, fonts, and color schemes
- `config.lua`: dynamic overrides (Lua 5.4 embedded via the `mlua` crate)

### Load Order

```
Defaults → config.toml → config.lua
```

Later-loaded values take precedence (override).

---

## ADR-006: Bundled TUI Fallback Client

### Background

Users may need to run nexterm in environments where wgpu is unavailable (e.g., inside containers, over SSH).

### Decision

Implement `nexterm-client-tui` with ratatui + crossterm.
The protocol with the server is shared with the GPU client (`nexterm-proto`).

### Feature Limitations

- Image protocols (Sixel / Kitty) are not supported (`ImagePlaced` is ignored)
- Scrollback and command palette are not implemented (planned for Phase 3)
- Multi-pane display is limited to showing only the focused pane

---

## Grid Diff Protocol Design

### Problem

Transmitting the full grid every frame wastes bandwidth.

### Solution

The server-side `Screen` maintains a dirty flag (`dirty: Vec<bool>`).
Changed rows are marked each time PTY output is processed, and diffs are extracted with `take_dirty_rows()`.

```
Client                Server
  │                     │
  │                  PTY output "hello\r\n"
  │                  → Screen.dirty[0] = true
  │<── GridDiff ────────│
  │    dirty_rows=[row0]│
```

The client only needs to merge the diff into its local grid.

---

## Security Design

### Unix Domain Socket

- `chmod 0600` restricts access to the owner only
- After accepting a connection, the client's UID is obtained via `SO_PEERCRED` (Linux) / `getpeereid()` (macOS/BSD) and compared against the server's UID. Connections with a UID mismatch are immediately dropped.

### Windows Named Pipe

- `ServerOptions::reject_remote_clients(true)` rejects connections from outside the local machine
- The default DACL allows access only to the creator

### Path Traversal Prevention

The `StartRecording { path }` handler pre-validates paths with `validate_recording_path()`, rejecting any path containing `..` components or empty paths.

### Authentication

Only UID verification is performed; there is no password-based authentication. Local-only communication is assumed. For network access, configuring an SSH tunnel or similar is recommended.

---

## Performance Design

### PTY Read Thread

Dispatched to the OS thread pool via `tokio::task::spawn_blocking`.
Does not block the tokio async runtime.

### Grid Diffs

By sending only diffs, idle panes generate zero traffic.

### GPU Rendering

- Glyph atlas: rasterized glyphs are cached into a texture on first render
- Vertex buffer: only diffs are updated per frame (efficient GPU memory usage)
- Poll interval: PTY output is checked and redrawn at 16ms intervals (~60 fps)

---

---

## ADR-007: Mouse Support Design

### Background

In winit 0.30, the `ApplicationHandler` trait requires implementing `window_event()`.
`CursorMoved`, `MouseInput`, and `MouseWheel` events are received there.

### Decision

- **Click-to-focus**: Cache cursor position on `CursorMoved`, then on `MouseInput { Left, Released }` convert to cell coordinates and send `FocusPane { pane_id }`
- **Wheel scroll**: On `MouseWheel` events, call `scroll_up` / `scroll_down` (in 3-line increments)
- Cell coordinate conversion: `pane_id = layout.iter().find(|p| point_in_pane(cursor, p))`

### Trade-offs

- **Pro**: Uses pane rectangle info received from the server via `LayoutChanged`, so no synchronization between server and client is needed
- **Con**: Behavior when clicking on border pixels is undefined (currently ignored)

---

## ADR-008: arboard for Clipboard Integration

### Background

Two crates provide cross-platform clipboard access: `arboard` (Arboard Clipboard) and `clipboard` (older crate).

### Decision

Adopt `arboard = "3"`.

- **Pro**: Supports all 3 OSes — Windows (OLE), macOS (NSPasteboard), Linux (X11/Wayland)
- **Pro**: Supports both images (RGBA) and text
- **Con**: `arboard::Clipboard::new()` must be called on the main thread on some OSes; instantiate it on each use

### Implementation

- `Ctrl+Shift+V`: `arboard::Clipboard::new()?.get_text()` → send `ClientToServer::PasteText { text }`
- `Ctrl+Shift+C`: convert the focused pane's grid to plain text via `grid_to_text()` → `arboard::Clipboard::new()?.set_text()`

---

## ADR-009: nexterm-ctl as a Standalone Crate

### Background

A session management CLI equivalent to tmux's `tmux list-sessions` / `tmux kill-session` is needed.
The requirement is to be operable without launching the GPU client or TUI client.

### Decision

Implement `nexterm-ctl` as a standalone `[[bin]]` crate.

- IPC connection reuses types from `nexterm-proto`
- Transport is the same Named Pipe / Unix Socket used by GPU/TUI clients
- Subcommands: `list` / `new` / `attach` / `kill` (implemented with `clap derive`)

### Note on the `attach` Subcommand

Since `nexterm-ctl` itself does not perform interactive terminal I/O, the `attach` subcommand only prints a guidance message about how to attach. Actual attachment is handled by `nexterm-client-gpu` or `nexterm-client-tui`.

---

## ADR-010: notify Crate for Config Hot Reload

### Background

Polling to detect config file changes introduces high latency and wastes CPU.

### Decision

Adopt `notify = "6"` to use OS-native file watching APIs.

- Linux: `inotify`
- macOS: `kqueue` / FSEvents
- Windows: `ReadDirectoryChangesW`

### Implementation

The `watch_config(tx: Sender<Config>)` function creates a `RecommendedWatcher` and sends a new `Config` whenever a config file change is detected. The GPU client polls `config_rx.try_recv()` in its `about_to_wait` hook and applies the new config when received.

The glyph atlas is regenerated only when the font family or size changes (a diff check is required because this is an expensive operation).

---

## ADR-011: Lua Status Bar Evaluation via LuaWorker Background Thread

### Background

Defining status bar widgets as Lua expressions allows writing expressions like `os.date()` that return the current time. Because `mlua::Lua` is `!Send + !Sync`, instances cannot be moved across threads. The initial design evaluated Lua synchronously on the main thread, but heavy Lua processing risked blocking the winit event loop and degrading frame rate.

### Decision

Implement `LuaWorker` in the `nexterm-config` crate. The `Lua` instance is created and owned inside a dedicated OS thread (`std::thread::spawn`); the main thread communicates with it via channels.

```
Main thread (winit)
  └── LuaWorker::eval_widgets(&widgets) → returns cached value immediately from Arc<Mutex<String>>
          │ try_send (SyncChannel)
          ▼
Lua worker thread (nexterm-lua-worker)
  └── loop { recv() → Lua::eval() → Arc<Mutex<String>>.lock().write() }
```

- Uses `try_send` on `request_tx: SyncSender<LuaRequest>`; requests are dropped if the channel is full (non-blocking)
- The last evaluation result is always held in `cache: Arc<Mutex<String>>`; `eval_widgets()` returns immediately

### Trade-offs

- **Pro**: The main thread is never blocked by Lua evaluation (zero impact on frame rate)
- **Pro**: No need to move the `Lua` instance across threads (naturally avoids the `!Send` constraint)
- **Con**: Evaluation results can lag by at most one frame (acceptable for status bar use cases)
- **Con**: If Lua evaluation is slow, the previous result continues to be displayed

---

## ADR-012: Session Persistence (JSON Snapshots)

### Background

Restarting the server previously caused all sessions, windows, panes, and BSP layouts to be lost. tmux addresses this with the `.tmux_resurrect` / `tmux-continuum` plugins, but nexterm is designed so that the server itself manages snapshots.

### Decision

Save all session state to a JSON file on server shutdown and restore it on the next startup.

**Storage format**: Pretty-printed JSON via `serde_json` (human-readable, easy to debug)

**Storage path**:
- Linux / macOS: `~/.local/state/nexterm/snapshot.json`
- Windows: `%APPDATA%\nexterm\snapshot.json`

**Snapshot types**:

```rust
ServerSnapshot { version: u32, sessions: Vec<SessionSnapshot>, saved_at: u64 }
SessionSnapshot { name, shell, cols, rows, windows, focused_window_id }
WindowSnapshot  { id, name, focused_pane_id, layout: SplitNodeSnapshot }
SplitNodeSnapshot::Pane   { pane_id, cwd: Option<PathBuf> }
SplitNodeSnapshot::Split  { dir, ratio, left, right }
```

**Restore flow**:

```
On startup:
  persist::load_snapshot()
    → SessionManager::restore_from_snapshot()
        → Session::restore_from_snapshot()
            → Window::restore_from_snapshot()
                → Pane::spawn_with_cwd(id, cols, rows, tx, shell, cwd)
  → set_min_pane_id(max_id + 1)   // prevent ID collisions
  → set_min_window_id(max_id + 1)

On shutdown:
  SessionManager::to_snapshot()
    → persist::save_snapshot()
```

**Working directory restoration**:
- Linux: retrieve child process cwd from the `/proc/{pid}/cwd` symlink
- Other OSes: `None` on restore (falls back to the shell's default startup directory)

### Trade-offs

- **Pro**: JSON rather than a binary format makes versioning and debugging straightforward
- **Pro**: The `version` field enables compatibility checks (mismatches are skipped)
- **Con**: The PTY virtual screen contents (grid) are not saved (only the shell is restarted)
- **Con**: Pane working directories are not restored on non-Linux platforms

---

## ADR-013: IPC Security (UID Verification and Path Traversal Prevention)

### Background

Unix domain socket permissions of 0600 and the default DACL on Windows Named Pipes restrict access to the owner, but on shared servers or container environments there is a risk of socket file permission changes or privilege escalation attacks. Additionally, if a path traversal string (e.g., `../etc/passwd`) were passed as the `StartRecording { path }` argument, it could result in writes to arbitrary file paths.

### Decision

**UID peer verification (Unix only)**:

After accepting a connection, the client's UID is obtained from the kernel via `SO_PEERCRED` / `getpeereid()` and compared against the server's `euid`. If they do not match, the connection is immediately dropped.

| OS | Implementation |
|----|------|
| Linux | `getsockopt(SO_PEERCRED)` → `ucred.uid` |
| macOS / BSD | `libc::getpeereid(fd, &uid, &gid)` |
| Other Unix | UID verification skipped (warning log only) |

**Windows Named Pipe**: `.reject_remote_clients(true)` rejects connections from outside the local machine.

**Path traversal prevention**:

`validate_recording_path()` is called first in the `StartRecording { path }` handler. Paths containing `std::path::Component::ParentDir` (`..`) or empty paths are returned as errors.

### Trade-offs

- **Pro**: Access can be restricted to the same user at the OS level, reducing reliance on file permissions
- **Con**: `SO_PEERCRED` is Linux-only and `getpeereid` is macOS/BSD-only, making conditional compilation complex
- **Con**: Connections via `setuid` binaries or `sudo` may be unintentionally rejected

---

## Future Design Issues

| Issue | Priority | Summary |
|------|--------|------|
| Pane border dragging | Low | Change BSP tree ratios via mouse drag |
| TUI client scrollback | Low | Add scrollback support to the ratatui client |
| macOS / Windows cwd restore | Low | Portable working directory retrieval without relying on `/proc` |
