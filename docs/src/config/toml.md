# TOML Reference

This page documents all settings available in `nexterm.toml`. See [Configuration Overview](overview.md) for file locations and load order.

## `[font]` — Font settings

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `family` | String | `"monospace"` | Font family name |
| `size` | float | `14.0` | Font size in points |
| `ligatures` | bool | `true` | Enable programming ligatures |
| `font_fallbacks` | String[] | `[]` | Fallback fonts tried in order when a glyph is not found |

```toml
[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji", "Symbols Nerd Font"]
```

## `[colors]` — Color scheme

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `scheme` | String | `"dark"` | Color scheme name |

### Built-in schemes

| Value | Description |
|-------|-------------|
| `"dark"` | Default dark |
| `"light"` | Light |
| `"tokyonight"` | Tokyo Night |
| `"solarized"` | Solarized Dark |
| `"gruvbox"` | Gruvbox Dark |

```toml
[colors]
scheme = "tokyonight"
```

## `[colors.custom]` — Custom color palette

Use `scheme = "custom"` to activate a fully custom 16-color palette.

| Key | Type | Description |
|-----|------|-------------|
| `foreground` | String | Foreground color (`#rrggbb`) |
| `background` | String | Background color (`#rrggbb`) |
| `cursor` | String | Cursor color (`#rrggbb`) |
| `ansi` | String[16] | ANSI 16 colors (normal + bright for each of 8 colors) |

```toml
[colors]
scheme = "custom"

[colors.custom]
foreground = "#cdd6f4"
background = "#1e1e2e"
cursor     = "#f5e0dc"
ansi = [
  "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
  "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
  "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af",
  "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
]
```

## `[shell]` — Shell settings

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `program` | String | OS-dependent | Full path to the shell executable |
| `args` | String[] | `[]` | Arguments passed to the shell |

OS defaults: Windows uses `pwsh.exe` (falls back to `powershell.exe`); Linux/macOS use `$SHELL` (falls back to `/bin/sh`).

```toml
[shell]
program = "/usr/bin/fish"
args = []
```

## `scrollback_lines` — Scrollback buffer size

Top-level key (no section header required).

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `scrollback_lines` | usize | `50000` | Maximum lines in the scrollback buffer |

```toml
scrollback_lines = 10000
```

## `[status_bar]` — Status bar

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Show the status bar |
| `widgets` | String[] | `[]` | List of Lua expressions evaluated and rendered right-aligned |

Each widget element is a Lua expression string. The result is coerced to a string and items are separated by two spaces.

```toml
[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']
```

Widgets are re-evaluated every second.

## `[window]` — Window appearance

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `background_opacity` | float | `1.0` | Background opacity (0.0 = transparent, 1.0 = opaque) |
| `macos_window_background_blur` | u32 | `0` | macOS blur strength (0 = disabled) |
| `decorations` | String | `"full"` | Window decoration style |

### `decorations` values

| Value | Description |
|-------|-------------|
| `"full"` | Standard OS title bar and border |
| `"none"` | No title bar or border (borderless) |
| `"notitle"` | No title bar only |

```toml
[window]
background_opacity = 0.92
macos_window_background_blur = 20
decorations = "notitle"
```

## `[terminal]` — Terminal feature flags

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `alt_screen_buffer` | bool | `true` | Alternate screen buffer (SMCUP/RMCUP) |
| `dec_mode_47_1047_1049` | bool | `true` | DEC Private Mode 47/1047/1049 support |
| `osc_window_title` | bool | `true` | OSC 0/1/2 window title support |
| `osc_notifications` | bool | `true` | OSC 9 desktop notification support |
| `cjk_width` | bool | `true` | Accurate CJK character width calculation |
| `ime_support` | bool | `true` | IME (Input Method Editor) support |

```toml
[terminal]
alt_screen_buffer = true
osc_window_title  = true
osc_notifications = true
cjk_width         = true
ime_support       = true
```

## `[tab_bar]` — Tab bar

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Show the tab bar |
| `height` | u32 | `28` | Tab bar height in pixels |
| `active_tab_bg` | String | `"#ae8b2d"` | Active tab background color (`#rrggbb`) |
| `inactive_tab_bg` | String | `"#5c6d74"` | Inactive tab background color (`#rrggbb`) |
| `separator` | String | `"❯"` | Separator character between tabs |

```toml
[tab_bar]
enabled          = true
height           = 28
active_tab_bg    = "#ae8b2d"
inactive_tab_bg  = "#5c6d74"
separator        = "❯"
```

## `[[keys]]` — Key bindings

Define custom key bindings as an array. These override the defaults.

| Key | Type | Description |
|-----|------|-------------|
| `key` | String | Key string, e.g. `"ctrl+shift+p"` |
| `action` | String | Action name or inline Lua code |
| `command` | String | (Optional) Shell command to execute |

### Available actions

| Action | Description |
|--------|-------------|
| `SplitVertical` | Split focused pane left/right |
| `SplitHorizontal` | Split focused pane top/bottom |
| `FocusNextPane` | Move focus to next pane |
| `FocusPrevPane` | Move focus to previous pane |
| `Detach` | Detach from session |
| `SearchScrollback` | Start scrollback search |
| `DisplayPanes` | Show pane number overlay for navigation |
| `ClosePane` | Close focused pane |
| `NewWindow` | Create a new window |
| `ToggleZoom` | Toggle zoom on focused pane |
| `SwapPaneNext` | Swap focused pane with next sibling |
| `SwapPanePrev` | Swap focused pane with previous sibling |
| `BreakPane` | Move focused pane to a new window |
| `ShowHostManager` | Open SSH host manager |
| `ShowMacroPicker` | Open Lua macro picker |
| `SftpUploadDialog` | Open SFTP upload dialog |
| `SftpDownloadDialog` | Open SFTP download dialog |
| `ConnectSerialPrompt` | Open serial port connection dialog |
| `QuickSelect` | Quick Select mode (URLs, paths, IPs, hashes) |

```toml
[[keys]]
key    = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key    = "ctrl+shift+-"
action = "SplitHorizontal"

[[keys]]
key     = "ctrl+alt+t"
command = "echo 'Hello from nexterm' | figlet"
```

## `[[hosts]]` — SSH host entries

Pre-register SSH hosts for quick connection from the command palette.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | String | — | Display name (required) |
| `host` | String | — | Hostname or IP address (required) |
| `port` | u16 | `22` | SSH port |
| `username` | String | — | Username (required) |
| `auth_type` | String | `"key"` | Auth method: `"password"`, `"key"`, or `"agent"` |
| `key_path` | String | — | Private key file path (for `auth_type = "key"`) |
| `proxy_jump` | String | — | ProxyJump hostname for multi-hop connections |
| `socks5_proxy` | String | — | SOCKS5 proxy address (`host:port`) |

```toml
[[hosts]]
name      = "production"
host      = "192.168.1.100"
port      = 22
username  = "deploy"
auth_type = "key"
key_path  = "~/.ssh/id_ed25519"

[[hosts]]
name      = "staging"
host      = "staging.example.com"
username  = "app"
auth_type = "agent"
```

## `[[macros]]` — Lua macros

Define macros callable from the macro picker (`Ctrl+Shift+M`).

| Key | Type | Description |
|-----|------|-------------|
| `name` | String | Display name (fuzzy-searched in picker) |
| `description` | String | Description (optional) |
| `lua_fn` | String | Lua global function name to invoke |

```toml
[[macros]]
name        = "git status"
description = "Show git status in current pane"
lua_fn      = "macro_git_status"
```

## `[web]` — Web terminal

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Enable the web terminal server |
| `port` | u16 | `7681` | TCP port to listen on |
| `token` | String | — | Legacy static token (passed as `?token=` query param) |

```toml
[web]
enabled = true
port    = 7681
```

## `[web.auth]` — TOTP / OTP authentication

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `totp_enabled` | bool | `false` | Require TOTP authentication for all browser access |
| `totp_secret` | String | — | Base32-encoded TOTP secret. Auto-written after first-run setup at `/setup`. |
| `issuer` | String | `"Nexterm"` | Issuer name displayed in authenticator apps |

```toml
[web.auth]
totp_enabled = true
issuer       = "Nexterm"
# totp_secret is written automatically after /setup verification
```

> First-run: leave `totp_secret` unset and open `/setup` in a browser to scan the QR code and register the secret.

## `[web.tls]` — HTTPS / TLS

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Enable HTTPS (WSS for WebSocket) |
| `cert_file` | String | — | Path to PEM certificate. Omit to auto-generate a self-signed cert. |
| `key_file` | String | — | Path to PEM private key (required when `cert_file` is set). |

```toml
# Auto-generated self-signed certificate (stored in ~/.config/nexterm/tls/)
[web.tls]
enabled = true

# Custom certificate (e.g. Let's Encrypt)
[web.tls]
enabled   = true
cert_file = "/etc/nexterm/tls/fullchain.pem"
key_file  = "/etc/nexterm/tls/privkey.pem"
```

## `[log]` — PTY logging

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto_log` | bool | `false` | Start logging automatically at session start |
| `log_dir` | String | — | Directory for log files |
| `timestamp` | bool | `false` | Prepend `[HH:MM:SS]` timestamp to each line |
| `strip_ansi` | bool | `false` | Strip ANSI escape sequences from log files |
| `max_log_size` | u64 | `104857600` | Maximum log file size in bytes (default 100 MB) |
| `log_template` | String | — | Log filename template (`{session}`, `{date}`, `{time}`) |
| `binary` | bool | `false` | Record raw bytes alongside text log |

```toml
[log]
auto_log      = true
log_dir       = "~/nexterm-logs"
timestamp     = true
strip_ansi    = true
max_log_size  = 52428800
log_template  = "{session}_{date}_{time}.log"
```
