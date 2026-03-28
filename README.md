# nexterm

A terminal multiplexer written in Rust, inspired by tmux/zellij, featuring GPU rendering via wgpu and a Lua configuration system.

> **日本語ドキュメント:** [README.ja.md](README.ja.md)

[![CI](https://github.com/kusanagi-jn/nexterm/actions/workflows/ci.yml/badge.svg)](https://github.com/kusanagi-jn/nexterm/actions/workflows/ci.yml)

## Features

- **GPU rendering** — High-performance font rendering with wgpu + cosmic-text
- **Daemonless design** — Server process holds PTYs; sessions survive client disconnects
- **BSP split layout** — Binary Space Partition for arbitrarily deep pane splitting
- **Lua + TOML config** — TOML for defaults, Lua for dynamic overrides; hot-reload on save
- **Mouse support** — Click to focus panes, scroll wheel for scrollback; Ctrl+Click to open URLs
- **Clipboard integration** — Ctrl+Shift+C to copy, Ctrl+Shift+V to paste (arboard)
- **Copy mode** — Vim-style text selection (Ctrl+[, hjkl, v, y)
- **Lua status bar** — Evaluate Lua expressions (e.g. `os.date()`) on the status line every second
- **Tab bar** — WezTerm-style tab bar with pane labels and `❯` separators
- **Session recording** — `nexterm-ctl record start/stop` saves raw PTY output to file
- **Bell notification** — VT BEL (\x07) triggers an OS window attention request
- **Font size** — Ctrl+= / Ctrl+- / Ctrl+0 to change font size at runtime
- **Window transparency** — Configurable opacity, borderless mode, and macOS blur
- **URL detection** — URLs in the grid are underlined; Ctrl+Click opens them in the browser
- **nexterm-ctl** — CLI tool for listing, creating, killing, and recording sessions
- **Image protocol** — Sixel and Kitty image display
- **TUI fallback** — ratatui-based TUI client for environments without GPU support
- **Cross-platform** — Linux / macOS / Windows (ConPTY + Named Pipe on Windows)
- **Localization** — UI in English, French, German, Spanish, Italian, Simplified Chinese, Japanese, Korean
- **SSH client** — Built-in SSH via russh; password and public-key auth; host registry in TOML
- **OS keychain** — SSH passwords saved to macOS Keychain / Windows Credential Store / Linux Secret Service
- **Mouse selection copy** — Drag to select text, auto-copy to clipboard (blue highlight)
- **Pane close** — Close the focused pane; sibling promoted in BSP tree
- **Pane resize** — Keyboard-adjustable split ratio
- **Window management** — Create, close, rename, and switch windows via IPC
- **Broadcast input** — Send keystrokes to all panes simultaneously (like tmux's synchronize-panes)
- **Timestamp logging** — Per-line `[HH:MM:SS]` timestamps with optional ANSI strip
- **OSC notifications** — OSC 0/2 window title change; OSC 9 desktop notifications
- **Custom color scheme** — Define a 16-color palette in TOML via `[colors.custom]`
- **CJK width** — Full-width characters (CJK, emoji) correctly occupy 2 columns
- **Font fallback chain** — `font_fallbacks` config lists fonts tried when a glyph is missing
- **macOS session restore** — CWD preserved on reconnect via `lsof`

## Implementation status

### Phase 1: Core foundation (complete)

| Crate | Description | Status |
|-------|-------------|--------|
| `nexterm-proto` | IPC protocol types (bincode) | ✅ |
| `nexterm-vt` | VT100/ANSI parser + Sixel/Kitty decode | ✅ |
| `nexterm-server` | PTY server (session / window / pane management) | ✅ |
| `nexterm-client-tui` | TUI client (ratatui + crossterm) | ✅ |

### Phase 2: GPU client & configuration (complete)

| Step | Description | Status |
|------|-------------|--------|
| 2-1 | nexterm-config — TOML + Lua config + hot-reload | ✅ |
| 2-2 | nexterm-client-gpu — wgpu renderer foundation | ✅ |
| 2-3 | Sixel / Kitty image protocol support | ✅ |
| 2-4 | Scrollback & incremental search | ✅ |
| 2-5 | Command palette (fuzzy match) | ✅ |

### Phase 3: Multi-pane & extensions (complete)

| Step | Description | Status |
|------|-------------|--------|
| 3-1 | Server-side BSP layout model | ✅ |
| 3-2 | Protocol extensions (LayoutChanged / FocusPane / PasteText) | ✅ |
| 3-3 | GPU client multi-pane split rendering | ✅ |
| 3-4 | Mouse support (click focus / wheel scroll) | ✅ |
| 3-5 | Clipboard integration (Ctrl+Shift+C/V) | ✅ |
| 3-6 | nexterm-ctl CLI (list / new / attach / kill) | ✅ |
| 3-7 | Config hot-reload → GPU client live update | ✅ |
| 3-8 | Lua status bar widget | ✅ |

### Phase 4: Localization & project structure (complete)

| Step | Description | Status |
|------|-------------|--------|
| 4-1 | Standard OSS directory structure (.github, examples, tests) | ✅ |
| 4-2 | nexterm-i18n crate (embedded JSON locales, sys-locale detection) | ✅ |
| 4-3 | UI string localization (8 languages) | ✅ |
| 4-4 | English README / documentation | ✅ |

### Phase 5: UX enhancements (complete)

| Step | Description | Status |
|------|-------------|--------|
| 5-A | Session recording (`nexterm-ctl record start/stop`) | ✅ |
| 5-B | WezTerm-style tab bar with `❯` separators | ✅ |
| 5-C | Window transparency, blur, and borderless mode | ✅ |
| 5-D | Vim-style copy mode (Ctrl+[, hjkl, v to select, y to yank) | ✅ |
| 5-E | Runtime font size change (Ctrl+= / Ctrl+- / Ctrl+0) | ✅ |
| 5-F | URL detection + Ctrl+Click to open in browser | ✅ |
| 5-G | VT BEL notification → OS window attention request | ✅ |

### Phase 6: Security & reliability (complete)

| Step | Description | Status |
|------|-------------|--------|
| 6-1 | IPC peer UID verification (Linux: SO_PEERCRED, macOS: getpeereid) | ✅ |
| 6-2 | Path traversal prevention for `StartRecording` | ✅ |
| 6-3 | Windows Named Pipe: reject remote clients | ✅ |
| 6-4 | LuaWorker background thread (no main-thread blocking) | ✅ |
| 6-5 | Session snapshot persistence (JSON, auto-save/restore) | ✅ |

### Phase 7: Competitive feature parity (complete)

Based on comparison with rlogin, Tera Term, WezTerm, and tmux.

| Step | Description | Status |
|------|-------------|--------|
| 7-1 | Pane close + resize (BSP node removal, ratio adjustment) | ✅ |
| 7-2 | Full window operations (new / close / focus / rename) | ✅ |
| 7-3 | Mouse drag text selection → clipboard | ✅ |
| 7-4 | macOS CWD preservation via lsof | ✅ |
| 7-5 | SSH client (nexterm-ssh, russh 0.58, password + pubkey) | ✅ |
| 7-6 | SSH host registry (`[[hosts]]` in TOML) | ✅ |
| 7-7 | OS keychain integration (keyring crate) | ✅ |
| 7-8 | Timestamp logging with ANSI strip | ✅ |
| 7-9 | Broadcast input to all panes | ✅ |
| 7-10 | OSC 0/2 title + OSC 9 desktop notifications | ✅ |
| 7-11 | Custom 16-color palette in TOML | ✅ |
| 7-12 | CJK / wide character width (unicode-width) | ✅ |
| 7-13 | Font fallback chain (font_fallbacks config) | ✅ |

**Tests**: 86+ passing

## Crate structure

```
nexterm/
├── nexterm-proto        # IPC message types and serialization
├── nexterm-vt           # VT100 parser, virtual screen, image decode
├── nexterm-server       # PTY server (IPC + session management)
├── nexterm-config       # Config loader (TOML + Lua) + StatusBarEvaluator
├── nexterm-client-tui   # TUI client
├── nexterm-client-gpu   # GPU client (wgpu + winit)
├── nexterm-ctl          # Session management CLI
├── nexterm-i18n         # Localization support (8 languages)
└── nexterm-ssh          # SSH client (russh) — connection, auth, PTY channel
```

## Build

### Prerequisites

- Rust 1.80 or later
- **Windows**: Visual Studio Build Tools (C++ components)
- **Linux**: `libx11-dev libxkbcommon-dev libwayland-dev`
- **macOS**: Xcode Command Line Tools (`xcode-select --install`)

### Build commands

```bash
# Build all crates
cargo build --release

# Server only
cargo build --release -p nexterm-server

# GPU client only
cargo build --release -p nexterm-client-gpu

# CLI tool only
cargo build --release -p nexterm-ctl
```

### Test

```bash
cargo test
```

## Usage

### Start the server

```bash
# With debug logging
NEXTERM_LOG=info nexterm-server

# Windows
set NEXTERM_LOG=info && nexterm-server.exe
```

The server listens on the following socket:

| OS | Path |
|----|------|
| Linux / macOS | `$XDG_RUNTIME_DIR/nexterm.sock` |
| Windows | `\\.\pipe\nexterm-<USERNAME>` |

### Start the GPU client

```bash
nexterm-client-gpu
```

Connects to the server automatically on launch and attaches to the `main` session. Starts in offline mode if the server is not running.

### Start the TUI client

```bash
nexterm-client-tui
```

## Key bindings (GPU client)

### General

| Key | Action |
|-----|--------|
| `Ctrl+Shift+P` | Open / close command palette |
| `Ctrl+F` | Start scrollback search |
| `PageUp` | Scroll up in scrollback |
| `PageDown` | Scroll down in scrollback |
| `Escape` | Close search / palette |
| `Enter` (in search) | Jump to next match |
| Regular key input | Forward to focused pane PTY |

### Font size

| Key | Action |
|-----|--------|
| `Ctrl+=` | Increase font size by 1 pt |
| `Ctrl+-` | Decrease font size by 1 pt |
| `Ctrl+0` | Reset font size to config value |

### Clipboard

| Key | Action |
|-----|--------|
| `Ctrl+Shift+C` | Copy visible grid of focused pane to clipboard |
| `Ctrl+Shift+V` | Paste clipboard content into focused pane |

### Copy mode (Vim-style)

| Key | Action |
|-----|--------|
| `Ctrl+[` | Enter copy mode |
| `h` / `j` / `k` / `l` | Move cursor left / down / up / right |
| `v` | Toggle selection start |
| `y` | Yank (copy) selection to clipboard and exit |
| `q` / `Escape` | Exit copy mode |

### Mouse

| Action | Effect |
|--------|--------|
| Left click | Move focus to clicked pane |
| Left drag | Select text (blue highlight), auto-copy to clipboard on release |
| `Ctrl` + Left click | Open URL under cursor in browser |
| Wheel up | Scroll up in scrollback (3 lines) |
| Wheel down | Scroll down in scrollback (3 lines) |

### Pane operations (via server protocol)

| Message | Action |
|---------|--------|
| `SplitVertical` | Split focused pane left/right |
| `SplitHorizontal` | Split focused pane top/bottom |
| `FocusNextPane` | Move focus to next pane |
| `FocusPrevPane` | Move focus to previous pane |
| `ClosePane` | Close focused pane (sibling promoted) |
| `ResizeSplit { delta: f32 }` | Adjust focused split ratio |
| `NewWindow` | Create a new window (tab) |
| `CloseWindow { window_id }` | Close specified window |
| `FocusWindow { window_id }` | Switch to specified window |
| `RenameWindow { window_id, name }` | Rename specified window |
| `SetBroadcast { enabled: bool }` | Toggle broadcast input mode |
| `ConnectSsh { host, port, username, auth_type, ... }` | Open SSH connection in new pane |

## nexterm-ctl

Session management CLI (requires server to be running).

```bash
# List all sessions
nexterm-ctl list

# Create a new session named 'work'
nexterm-ctl new work

# Show how to attach to session 'work'
nexterm-ctl attach work

# Kill session 'work'
nexterm-ctl kill work

# Start recording PTY output to a file
nexterm-ctl record start work output.log

# Stop recording
nexterm-ctl record stop work
```

### Language override

```bash
# Force a specific UI language
NEXTERM_LANG=ja nexterm-ctl list
```

Supported values: `en`, `fr`, `de`, `es`, `it`, `zh-CN`, `ja`, `ko`

## Configuration

Config files are searched in order:

| OS | Path |
|----|------|
| Linux / macOS | `~/.config/nexterm/config.toml` |
| Windows | `%APPDATA%\nexterm\config.toml` |

### nexterm.toml example

```toml
scrollback_lines = 50000

[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji"]

[colors]
scheme = "tokyonight"

[shell]
program = "/usr/bin/fish"

[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']

[window]
background_opacity = 0.95
decorations = "full"   # "full" | "none" | "notitle"

[tab_bar]
enabled = true
height = 28
active_tab_bg = "#ae8b2d"
inactive_tab_bg = "#5c6d74"
separator = "❯"

[[hosts]]
name = "my-server"
host = "192.168.1.100"
port = 22
username = "deploy"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

[log]
auto_log = false
timestamp = true
strip_ansi = true
log_dir = "~/nexterm-logs"

[colors.custom]
foreground = "#cdd6f4"
background = "#1e1e2e"
cursor = "#f5e0dc"
ansi = [
  "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
  "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
  "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af",
  "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
]
```

### nexterm.lua override example

```lua
-- ~/.config/nexterm/nexterm.lua
local cfg = require("nexterm")

-- Change font size at runtime
cfg.font.size = 16.0

-- Show time and session name in status bar
cfg.status_bar.enabled = true
cfg.status_bar.widgets = { 'os.date("%H:%M")', '"main"' }

return cfg
```

> Configuration changes are applied immediately via hot-reload.

## Architecture overview

```
┌──────────────────────────────────────┐
│         nexterm-client-gpu           │
│   wgpu renderer / winit event loop   │
└───────────────┬──────────────────────┘
                │ IPC (bincode / Named Pipe / Unix Socket)
┌───────────────▼──────────────────────┐
│         nexterm-server               │
│  Session → Window → Pane (PTY)       │
│  BSP layout engine                   │
└───────────────┬──────────────────────┘
                │ portable-pty
┌───────────────▼──────────────────────┐
│       OS PTY (ConPTY / Unix)         │
│       Shell / application            │
└──────────────────────────────────────┘
```

For details, see the documentation:

| Document | Contents |
|----------|----------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, data flow, rendering pipeline |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | IPC protocol spec (message types, framing, sequence diagrams) |
| [docs/DESIGN.md](docs/DESIGN.md) | Design document and ADRs |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | Full TOML / Lua configuration reference |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Build instructions, coding conventions, PR guidelines |

## License

MIT OR Apache-2.0
