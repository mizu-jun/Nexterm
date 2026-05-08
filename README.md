# nexterm

A terminal multiplexer written in Rust, inspired by tmux/zellij, featuring GPU rendering via wgpu and a Lua configuration system.

> **日本語ドキュメント:** [README.ja.md](README.ja.md)

[![CI](https://github.com/mizu-jun/nexterm/actions/workflows/ci.yml/badge.svg)](https://github.com/mizu-jun/nexterm/actions/workflows/ci.yml)

## What's New in v0.9.0

**GPU client: major redesign and competitive feature additions**

- **Tokyo Night theme**: Changed the default color scheme from dark to Tokyo Night. Refreshed the tab bar with an accent line and pane borders with 1px thin lines.
- **Settings panel redesign**: New 7-category settings screen with a left sidebar (Startup / Font / Theme / Window / SSH / Keybindings / Profiles).
- **Tab rename UI**: Double-clicking a tab (within 300 ms) shows an inline edit field; pressing Enter sends a RenameWindow command to the server.
- **Context menu enhancements**: Added profile list, inline search, and open-settings menu entries with keyboard shortcut hints.
- **App icon update**: Changed to a blue-gradient "N" logo (updated nexterm-source.png / nexterm.ico).
- **Black margin fix**: Fixed a black window border appearing before the pane layout is received; the background color is now used to fill the gap.

---

For full release history, see [CHANGELOG.md](CHANGELOG.md).

## Features

### SSH & Connectivity
- **SSH client** — Built-in SSH via russh; password and public-key auth; host registry in TOML
- **SSH Host Manager** — `Ctrl+Shift+H` opens a fuzzy-searchable host list; each host opens in a new tab
- **X11 forwarding** — `x11_forward = true` (`ssh -X`) and `x11_trusted = true` (`ssh -Y`) per host
- **SSH agent authentication** — SSH_AUTH_SOCK support for agent-based auth
- **Known hosts verification** — Host key verification against ~/.ssh/known_hosts (replaces accept-all)
- **Local port forwarding** — Forward local ports through SSH tunnels
- **Remote port forwarding** — `-R` style remote-side port forwarding over SSH
- **ProxyJump support** — Multi-hop SSH connections
- **SOCKS5 proxy** — Route connections through SOCKS5 proxies
- **OS keychain** — SSH passwords saved to macOS Keychain / Windows Credential Store / Linux Secret Service
- **SFTP transfer** — `Ctrl+Shift+U/D` opens Upload / Download dialogs with live progress
- **Serial port** — Connect to serial devices via command palette (`ConnectSerial`)

### GPU Rendering & UI
- **GPU rendering** — High-performance font rendering with wgpu + cosmic-text
- **Alternate screen buffer** — Full support for SMCUP/RMCUP and DEC modes 47/1047/1049
- **IME input** — Japanese, Chinese, Korean input method support
- **Right-click context menu** — Copy/Paste/Split/ClosePane operations
- **Pane number overlay** — Display-panes mode for pane navigation
- **Mouse selection copy** — Drag to select text, auto-copy to clipboard (blue highlight)
- **CJK width** — Full-width characters (CJK, emoji) correctly occupy 2 columns

### Core Features
- **Daemonless design** — Server process holds PTYs; sessions survive client disconnects
- **Multi-client session sharing** — tmux-style session attach with synchronized clients
- **Broadcast input mode** — Send keystrokes to all panes simultaneously
- **BSP split layout** — Binary Space Partition for arbitrarily deep pane splitting
- **Window management** — Create, close, rename, and switch windows via IPC
- **Pane operations** — Close panes, resize splits, navigate with keyboard or mouse
- **Tab bar** — WezTerm-style tab bar with pane labels and `❯` separators
- **Copy mode** — Vim-style text selection (Ctrl+[, hjkl, v, y)

### Configuration & Automation
- **Lua + TOML config** — TOML for defaults, Lua for dynamic overrides; hot-reload on save
- **Keybinding customization** — Execute custom actions on key combinations
- **Lua status bar** — Evaluate Lua expressions (e.g. `os.date()`) on the status line every second
- **Lua event hooks** — `on_session_start`, `on_attach`, `on_pane_open` callbacks in `nexterm.lua`
- **Lua Macro Picker** — Define `[[macros]]` in TOML; `Ctrl+Shift+M` opens fuzzy picker; output piped to pane
- **Settings GUI** — `Ctrl+,` opens in-app Font / Colors / Window settings panel; writes back to `nexterm.toml`
- **Font size runtime** — Ctrl+= / Ctrl+- / Ctrl+0 to change font size at runtime
- **Color scheme import** — `nexterm-ctl theme import` supports iTerm2 .itermcolors, Alacritty YAML, base16 TOML
- **Custom color scheme** — Define a 16-color palette in TOML via `[colors.custom]`

### Logging & Recording
- **Asciicast v2 recording** — `nexterm-ctl record start-cast/stop-cast` for asciinema format
- **Log rotation** — Size-based automatic log file rotation
- **Timestamp logging** — Per-line `[HH:MM:SS]` timestamps with optional ANSI strip
- **Session recording** — `nexterm-ctl record start/stop` saves raw PTY output to file

### Terminal & Display
- **OSC 0/1/2 window title** — Dynamic window/tab title via escape sequences
- **OSC 9 desktop notifications** — System notifications from terminal
- **Bell notification** — VT BEL (\x07) triggers an OS window attention request
- **URL detection** — URLs in the grid are underlined; Ctrl+Click opens them in the browser
- **Image protocol** — Sixel and Kitty image display
- **Window transparency** — Configurable opacity, borderless mode, and macOS blur
- **Font fallback chain** — `font_fallbacks` config lists fonts tried when a glyph is missing

### Platform & Accessibility
- **Mouse support** — Click to focus panes, scroll wheel for scrollback; Ctrl+Click to open URLs
- **Clipboard integration** — Ctrl+Shift+C to copy, Ctrl+Shift+V to paste (arboard)
- **TUI fallback** — ratatui-based TUI client for environments without GPU support
- **Web terminal** — Browser-accessible terminal via embedded WebSocket server + xterm.js (`[web] enabled = true`)
- **Cross-platform** — Linux / macOS / Windows (ConPTY + Named Pipe on Windows)
- **Localization** — UI in English, French, German, Spanish, Italian, Simplified Chinese, Japanese, Korean
- **macOS session restore** — CWD preserved on reconnect via `lsof`

### CLI Tool
- **nexterm-ctl** — Session management CLI (list, create, attach, kill, record, import themes)

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

### Phase 8: Advanced UX (complete)

| Step | Description | Status |
|------|-------------|--------|
| 8-1 | Pane zoom toggle (toggle-zoom, `Ctrl+B Z`) | ✅ |
| 8-2 | Quick Select mode — URL / path / IP / hash highlight (`Ctrl+Shift+Space`) | ✅ |
| 8-3 | SSH Host Manager UI — fuzzy search, one-key connect (`Ctrl+Shift+H`) | ✅ |
| 8-4 | Lua Macro execution engine — `[[macros]]` in TOML, picker UI (`Ctrl+Shift+M`) | ✅ |

### Phase 9: Pane management (complete)

| Step | Description | Status |
|------|-------------|--------|
| 9-1 | Swap pane with next / previous sibling (`Ctrl+B {` / `Ctrl+B }`) | ✅ |
| 9-2 | Break pane to new window / join pane (`Ctrl+B !`) | ✅ |

### Phase 10: Connectivity (complete)

| Step | Description | Status |
|------|-------------|--------|
| 10-1 | Remote port forwarding (`-R`) over SSH sessions | ✅ |
| 10-3 | SFTP Upload / Download dialogs with live progress (`Ctrl+Shift+U/D`) | ✅ |
| 10-4 | Serial port connections via command palette (`ConnectSerial`) | ✅ |

### Phase 11: Logging & hooks (complete)

| Step | Description | Status |
|------|-------------|--------|
| 11-1 | Lua event hooks — `on_session_start`, `on_attach`, `on_pane_open` | ✅ |
| 11-3 | Log filename templates (`{session}`, `{date}`, `{time}` placeholders) | ✅ |
| 11-4 | Binary PTY log mode | ✅ |

### Windows (complete)

| Item | Description | Status |
|------|-------------|--------|
| W-1 | MSI installer (WiX Toolset v3, CI-automated) | ✅ |
| W-2 | Code signing workflow (`signtool.exe`, CI secrets) | ✅ |
| W-3 | `nexterm-launcher` — single `nexterm.exe` entry point | ✅ |
| W-4 | Windows Service install / uninstall scripts | ✅ |
| W-5 | PowerShell default args (`-NoLogo`) + cmd.exe fallback | ✅ |
| W-7 | Windows Quick Start documentation | ✅ |
| W-10 | Snapshot save path unified to `%APPDATA%\nexterm` | ✅ |

### Phase 12: Distribution & extensibility (complete)

| Item | Description | Status |
|------|-------------|--------|
| 12-1 | SSH multi-tab connections (HostManager → NewWindow + ConnectSsh) | ✅ |
| 12-2 | X11 forwarding per host (`x11_forward`, `x11_trusted` in `[[hosts]]`) | ✅ |
| 12-3 | In-app settings GUI panel (`Ctrl+,`, writes nexterm.toml via toml_edit) | ✅ |
| 12-4 | Embedded web terminal (axum WebSocket + xterm.js, token auth) | ✅ |
| 12-5 | Homebrew tap formula | ✅ |
| 12-6 | Scoop bucket manifest | ✅ |
| 12-7 | winget manifest | ✅ |
| 12-8 | GitHub Pages documentation site (mdBook, CI auto-deploy) | ✅ |

**Tests**: 178+ passing

## Crate structure

```
nexterm/
├── nexterm-proto        # IPC message types and serialization
├── nexterm-vt           # VT100 parser, virtual screen, image decode
├── nexterm-server       # PTY server (IPC + session management)
├── nexterm-config       # Config loader (TOML + Lua) + StatusBarEvaluator
├── nexterm-client-tui   # TUI client
├── nexterm-client-gpu   # GPU client (wgpu + winit)
├── nexterm-launcher     # nexterm.exe — auto-starts server + opens GPU client
├── nexterm-ctl          # Session management CLI
├── nexterm-i18n         # Localization support (8 languages)
└── nexterm-ssh          # SSH client (russh) — connection, auth, PTY channel
```

## macOS Quick Start

> **TL;DR** — run `nexterm`. That's it. You do not need to start any other executable manually.

### Install with Homebrew (recommended)

```sh
brew install mizu-jun/nexterm/nexterm
nexterm
```

### Install from tarball

1. Download `nexterm-vX.Y.Z-macos-arm64.tar.gz` (Apple Silicon) or
   `nexterm-vX.Y.Z-macos-x86_64.tar.gz` (Intel) from the [Releases](https://github.com/mizu-jun/Nexterm/releases) page.

2. Extract and remove the quarantine flag:
   ```sh
   tar xzf nexterm-vX.Y.Z-macos-arm64.tar.gz
   xattr -dr com.apple.quarantine Nexterm.app
   ```

3. **Option A — GUI (Finder):** Move `Nexterm.app` to `/Applications` and double-click it.

4. **Option B — Terminal:**
   ```sh
   # Copy all binaries to a directory on your PATH
   sudo cp nexterm nexterm-server nexterm-client-gpu nexterm-client-tui nexterm-ctl /usr/local/bin/
   nexterm
   ```

`nexterm` auto-starts `nexterm-server` in the background if it is not already running,
then opens `nexterm-client-gpu`. The other binaries (`nexterm-server`, `nexterm-client-gpu`, etc.)
are for advanced use only.

---

## Linux Quick Start

> **TL;DR** — run `nexterm`. That's it. You do not need to start any other executable manually.

### Install from tarball

1. Download `nexterm-vX.Y.Z-linux-x86_64.tar.gz` from the [Releases](https://github.com/mizu-jun/Nexterm/releases) page.

2. Extract and run the install script:
   ```sh
   tar xzf nexterm-vX.Y.Z-linux-x86_64.tar.gz
   ./install.sh
   ```
   This copies all binaries to `~/.local/bin/` and installs a `.desktop` entry so Nexterm
   appears in your application launcher. Use `sudo ./install.sh` to install system-wide to
   `/usr/local/bin/`.

3. Launch:
   ```sh
   nexterm
   ```

`nexterm` auto-starts `nexterm-server` in the background if it is not already running,
then opens `nexterm-client-gpu`.

### Uninstall

```sh
./install.sh --uninstall       # user install
sudo ./install.sh --uninstall  # system-wide install
```

---

## Windows Quick Start

### System Requirements

| Requirement | Minimum |
|-------------|---------|
| Windows version | **Windows 10 1809 (October 2018 Update) or later** |
| Architecture | x86-64 |
| ConPTY | Built into Windows 10 1809+ |
| GPU | DirectX 11 compatible (wgpu requirement) |

> **Why Windows 10 1809+?**
> Nexterm uses the **ConPTY** (Pseudo Console) API introduced in Windows 10 1809 to provide
> proper terminal emulation for PowerShell and cmd.exe. Earlier versions of Windows are not supported.

### Install with MSI (recommended)

1. Download `nexterm-vX.Y.Z-windows-x86_64.msi` from the [Releases](https://github.com/mizu-jun/nexterm/releases) page.
2. Double-click the MSI to launch the installer wizard.
3. Follow the prompts — Nexterm will be installed to `C:\Program Files\Nexterm` and added to `PATH`.
4. Launch from the **Start menu** or by typing `nexterm` in any terminal.

> **SmartScreen warning**: Because Nexterm is an open-source project without an EV code-signing
> certificate, Windows Defender SmartScreen may display an "Unknown publisher" warning.
> Click **"More info" → "Run anyway"** to proceed. To verify the binary, check the SHA-256
> checksum published on the Releases page.

### Install from ZIP (portable)

1. Download `nexterm-vX.Y.Z-windows-x86_64.zip` from the Releases page.
2. Extract to any directory (e.g. `C:\Nexterm`).
3. Add the directory to your `PATH` (optional):
   ```powershell
   $env:Path += ";C:\Nexterm"
   [System.Environment]::SetEnvironmentVariable("Path", $env:Path, "Machine")
   ```

### Launch

```powershell
# Single command — auto-starts the server and opens the GPU client
nexterm.exe
```

`nexterm.exe` is the **launcher** — it detects whether `nexterm-server` is already running,
starts it in the background if needed, then opens `nexterm-client-gpu`.

To start the server separately (e.g. for debugging):

```powershell
# Start server in a background window
Start-Process -NoNewWindow nexterm-server.exe

# Start GPU client
nexterm-client-gpu.exe

# Or start TUI client (no GPU required)
nexterm-client-tui.exe
```

### Run as a Windows Service (optional)

Register `nexterm-server` as a Windows Service so it starts automatically at boot, without needing a user session:

```powershell
# Run as Administrator
.\install-service.ps1

# Stop / start / uninstall
Stop-Service NextermServer
Start-Service NextermServer
.\uninstall-service.ps1
```

### Default shell

On Windows, Nexterm selects the default shell in this order:

| Priority | Shell | Path |
|----------|-------|------|
| 1 | PowerShell 7 | `C:\Program Files\PowerShell\7\pwsh.exe` |
| 2 | PowerShell 5 | `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe` |
| 3 | cmd.exe | `C:\Windows\System32\cmd.exe` |

To override, set `shell.program` in your config:

```toml
# %APPDATA%\nexterm\nexterm.toml
[shell]
program = "C:\\Windows\\System32\\cmd.exe"
```

### Code signing

Nexterm's binaries are not commercially code-signed by default. To suppress SmartScreen warnings in
a corporate environment, sign the binaries yourself with `signtool.exe`:

```powershell
# Using a self-signed certificate (development / internal use)
$cert = New-SelfSignedCertificate -Subject "CN=Nexterm" -Type CodeSigning -CertStoreLocation Cert:\CurrentUser\My
Set-AuthenticodeSignature -FilePath nexterm.exe -Certificate $cert
```

For production deployments, set the following GitHub Actions secrets in your fork to enable
automatic signing in CI:

| Secret | Description |
|--------|-------------|
| `WINDOWS_CERTIFICATE` | Base64-encoded `.pfx` certificate |
| `WINDOWS_CERTIFICATE_PASSWORD` | Certificate password |

---

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

Quick reference — common shortcuts:

| Key | Action |
|-----|--------|
| `Ctrl+,` | Open settings panel |
| `Ctrl+Shift+P` | Open / close command palette |
| `Ctrl+F` | Start scrollback search |
| `Ctrl+[` | Enter copy mode (Vim-style) |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | Copy / Paste |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Increase / decrease / reset font size |
| `Ctrl+G` | Display-panes mode (show pane numbers) |
| `Ctrl+Shift+H` | SSH Host Manager |
| `Ctrl+B Z` | Toggle zoom on focused pane |

For the complete key binding reference, see [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md).

## nexterm-ctl

Session and configuration management CLI (requires server to be running for most commands).

```bash
# Session management
nexterm-ctl list                           # List all sessions
nexterm-ctl new work                       # Create a new session named 'work'
nexterm-ctl attach work                    # Show how to attach to session 'work'
nexterm-ctl kill work                      # Kill session 'work'

# Recording (raw PTY output)
nexterm-ctl record start work output.log   # Start recording to file
nexterm-ctl record stop work               # Stop recording

# Recording (asciinema v2 format)
nexterm-ctl record start-cast work cast.cast   # Start recording in asciicast v2 format
nexterm-ctl record stop-cast work             # Stop asciicast recording

# Theme import
nexterm-ctl theme import ~/.iTerm2/colorscheme.itermcolors
nexterm-ctl theme import ~/.config/alacritty/color.yaml
nexterm-ctl theme import ~/.config/base16.toml
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
x11_forward = false   # true = ssh -X (untrusted), x11_trusted = true = ssh -Y

[[macros]]
name = "top"
description = "Run top on focused pane"
lua_fn = "macro_top"

[[macros]]
name = "git status"
description = "Show git status"
lua_fn = "macro_git_status"

[web]
enabled = false       # set true to enable browser terminal at ws://localhost:7681
port = 7681
# token = "change-me"  # optional; if set, ?token= param is required

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
