# nexterm Configuration Reference

## Configuration File Locations

| OS | TOML path | Lua path |
|----|-----------|---------|
| Linux | `~/.config/nexterm/nexterm.toml` | `~/.config/nexterm/nexterm.lua` |
| macOS | `~/Library/Application Support/nexterm/nexterm.toml` | `~/Library/Application Support/nexterm/nexterm.lua` |
| Windows | `%APPDATA%\nexterm\nexterm.toml` | `%APPDATA%\nexterm\nexterm.lua` |

If the `XDG_CONFIG_HOME` environment variable is set, `$XDG_CONFIG_HOME/nexterm/` takes precedence (Linux only).

---

## Load Order

```
1. Built-in default values
2. nexterm.toml  (if present)
3. nexterm.lua   (if present)
```

Later-loaded values take precedence. Values set in TOML can be overridden by Lua.

---

## nexterm.toml Reference

### `[font]` — Font Settings

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `family` | String | `"monospace"` | Font family name |
| `size` | float | `14.0` | Font size (pt) |
| `ligatures` | bool | `true` | Enable programming ligatures |
| `font_fallbacks` | String[] | `[]` | List of fallback fonts to try in order when a glyph is not found |

```toml
[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji", "Symbols Nerd Font"]
```

### `[colors]` — Color Scheme

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `scheme` | String | `"dark"` | Name of the color scheme to use |

#### Built-in Schemes

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

### `[shell]` — Shell Settings

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `program` | String | OS-dependent | Full path to the shell program |
| `args` | String[] | `[]` | Arguments to pass to the shell |

Default values by OS:
- **Windows**: `C:\Program Files\PowerShell\7\pwsh.exe` (falls back to `powershell.exe`)
- **Linux / macOS**: `$SHELL` environment variable (falls back to `/bin/sh`)

```toml
[shell]
program = "/usr/bin/fish"
args = []
```

### `scrollback_lines` — Scrollback Buffer Size

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `scrollback_lines` | usize | `50000` | Maximum number of lines in the scrollback buffer |

`scrollback_lines` is a top-level key (no section header required).

```toml
scrollback_lines = 10000
```

### `[status_bar]` — Status Bar

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Show the status bar |
| `widgets` | String[] | `[]` | List of Lua expressions to display in the status bar |

Each element in `widgets` is evaluated as a **Lua expression string**. The results are cast to `String`, joined with two spaces, and displayed on the right side.

#### Widget Expression Examples

| Lua expression | Example output | Description |
|----------------|---------------|-------------|
| `'os.date("%H:%M:%S")'` | `14:23:01` | Current time (with seconds) |
| `'os.date("%Y-%m-%d")'` | `2026-03-26` | Current date |
| `'"nexterm"'` | `nexterm` | Fixed string (outer quotes are TOML string, inner quotes are a Lua string literal) |
| `'tostring(math.pi):sub(1,6)'` | `3.1415` | Any arbitrary Lua expression |

> **Note**: When writing Lua string literals inside TOML, double quotes conflict between TOML and Lua. It is recommended to use single-quoted TOML strings for widget expressions.

```toml
[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']
```

Evaluation occurs **every 1 second** (inside the GPU client's `about_to_wait` hook).

### `[window]` — Window Appearance

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `background_opacity` | float | `1.0` | Window background opacity (0.0 = fully transparent, 1.0 = opaque). A compositor is required for transparency |
| `macos_window_background_blur` | u32 | `0` | macOS window background blur intensity (0 = disabled) |
| `decorations` | String | `"full"` | Window decoration style |

#### `decorations` Values

| Value | Description |
|-------|-------------|
| `"full"` | Show the OS-native title bar and borders |
| `"none"` | Hide title bar and borders (borderless) |
| `"notitle"` | Hide title bar only |

```toml
[window]
background_opacity = 0.92
macos_window_background_blur = 20
decorations = "notitle"
```

### `[terminal]` — Terminal Feature Settings

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `alt_screen_buffer` | bool | `true` | Alternate screen buffer support (SMCUP/RMCUP) |
| `dec_mode_47_1047_1049` | bool | `true` | DEC Private Mode 47/1047/1049 support |
| `osc_window_title` | bool | `true` | OSC 0/1/2 window title support |
| `osc_notifications` | bool | `true` | OSC 9 desktop notification support |
| `cjk_width` | bool | `true` | Accurate CJK character width calculation |
| `ime_support` | bool | `true` | IME (Input Method Editor) support |

```toml
[terminal]
alt_screen_buffer = true
dec_mode_47_1047_1049 = true
osc_window_title = true
osc_notifications = true
cjk_width = true
ime_support = true
```

The alternate screen buffer is used by applications such as `less`, `vim`, and `htop` to clear the display and switch between views.

### `[tab_bar]` — Tab Bar (WezTerm style)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Show the tab bar |
| `height` | u32 | `28` | Tab bar height (pixels) |
| `active_tab_bg` | String | `"#ae8b2d"` | Active tab background color (`#rrggbb` format) |
| `inactive_tab_bg` | String | `"#5c6d74"` | Inactive tab background color (`#rrggbb` format) |
| `separator` | String | `"❯"` | Separator character between tabs |

```toml
[tab_bar]
enabled = true
height = 28
active_tab_bg = "#ae8b2d"
inactive_tab_bg = "#5c6d74"
separator = "❯"
```

### `[[keys]]` — Key Bindings

Define custom key bindings as an array. Use this to override default bindings.

| Key | Type | Description |
|-----|------|-------------|
| `key` | String | Key string (e.g. `"ctrl+shift+p"`) |
| `action` | String | Action name or custom Lua code |
| `command` | String | (Optional) Command to execute |

#### Default Actions

| Action | Description |
|--------|-------------|
| `SplitVertical` | Split the focused pane left/right |
| `SplitHorizontal` | Split the focused pane top/bottom |
| `FocusNextPane` | Move focus to the next pane |
| `FocusPrevPane` | Move focus to the previous pane |
| `Detach` | Detach from the session |
| `SearchScrollback` | Start a scrollback search |
| `DisplayPanes` | Show pane number overlay (for navigation) |
| `ClosePane` | Close the focused pane |
| `NewWindow` | Create a new window |
| `ToggleZoom` | Zoom/unzoom the focused pane |
| `SwapPaneNext` | Swap the focused pane with the next sibling |
| `SwapPanePrev` | Swap the focused pane with the previous sibling |
| `BreakPane` | Break the focused pane into a new window |
| `ShowHostManager` | Open the SSH host manager |
| `ShowMacroPicker` | Open the Lua macro picker |
| `SftpUploadDialog` | Open the SFTP upload dialog |
| `SftpDownloadDialog` | Open the SFTP download dialog |
| `ConnectSerialPrompt` | Open the serial port connection dialog |
| `QuickSelect` | Quick Select mode (URLs, paths, IPs, hashes) |
| `ShowSettings` | Open the settings GUI panel (default: `Ctrl+,`) |

#### Custom Key Binding Examples

```toml
# Standard actions
[[keys]]
key = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key = "ctrl+shift+-"
action = "SplitHorizontal"

[[keys]]
key = "ctrl+shift+p"
action = "CommandPalette"

# Execute a custom command
[[keys]]
key = "ctrl+alt+t"
command = "echo 'Hello from nexterm' | figlet"
```

#### Right-Click Context Menu

Right-clicking inside the GPU client shows a context menu:

- **Copy** — Copy the entire focused pane
- **Paste** — Paste clipboard contents
- **Split Vertical** — Split the pane left/right
- **Split Horizontal** — Split the pane top/bottom
- **Close Pane** — Close the pane
- **Display Panes** — Enter pane number overlay mode

#### Display Panes Mode

`Display Panes` or `Ctrl+G` shows a pane number overlay.
Type the displayed pane number or use the arrow keys to navigate between panes.

### `[[hosts]]` — SSH Host Registration

Pre-register SSH connection targets. Registered hosts can be selected and connected to from the command palette.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | String | — | Display name (required) |
| `host` | String | — | Hostname or IP address (required) |
| `port` | u16 | `22` | SSH port number |
| `username` | String | — | Username (required) |
| `auth_type` | String | `"key"` | Authentication method: `"password"`, `"key"`, `"agent"` |
| `key_path` | String | — | Path to private key file (when `auth_type = "key"`) |
| `proxy_jump` | String | — | ProxyJump hostname (for multi-hop connections) |
| `socks5_proxy` | String | — | SOCKS5 proxy address (`host:port` format) |
| `local_forwards` | Table[] | — | Local port forwarding configuration |
| `forward_remote` | Table[] | — | Remote port forwarding configuration (`-R`) |
| `x11_forward` | bool | `false` | Enable X11 forwarding (equivalent to `ssh -X`) |
| `x11_trusted` | bool | `false` | Trusted X11 forwarding (equivalent to `ssh -Y`, takes precedence over `x11_forward`) |

#### SSH Authentication Methods

- `"password"` — Password authentication (stored securely in the OS keychain)
- `"key"` — Public key authentication (specify a private key file)
- `"agent"` — SSH agent authentication (uses `SSH_AUTH_SOCK`)

#### Local Port Forwarding

Maps a local port to a remote host:port.

```toml
[[hosts.local_forwards]]
local_port = 8080
remote_host = "localhost"
remote_port = 3000
```

#### SSH Host Configuration Examples

```toml
# Public key authentication
[[hosts]]
name = "Production Server"
host = "192.168.1.100"
port = 22
username = "deploy"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

# Password authentication
[[hosts]]
name = "Development Server"
host = "dev.example.com"
port = 2222
username = "ubuntu"
auth_type = "password"
# Password is stored in the OS keychain

# SSH agent authentication
[[hosts]]
name = "Staging"
host = "staging.example.com"
port = 22
username = "app"
auth_type = "agent"

# Connection via ProxyJump
[[hosts]]
name = "Internal Server"
host = "internal.company.local"
port = 22
username = "admin"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"
proxy_jump = "bastion.company.com"

# Connection via SOCKS5 proxy
[[hosts]]
name = "Remote Server"
host = "remote.example.com"
port = 22
username = "user"
auth_type = "key"
key_path = "~/.ssh/id_rsa"
socks5_proxy = "proxy.example.com:1080"

# With local port forwarding
[[hosts]]
name = "DB Server"
host = "db.internal"
port = 22
username = "dbadmin"
auth_type = "key"
key_path = "~/.ssh/db_key"

[[hosts.local_forwards]]
local_port = 5432
remote_host = "localhost"
remote_port = 5432
```

#### Remote Port Forwarding (`-R`)

Forwards a port on the SSH server to a local port (equivalent to `ssh -R`).

```toml
[[hosts]]
name = "Remote Forward Example"
host = "example.com"
port = 22
username = "user"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

[[hosts.forward_remote]]
remote_port = 9090
local_host  = "localhost"
local_port  = 9090
```

#### Known Hosts Verification

Host keys in `~/.ssh/known_hosts` are verified when establishing SSH connections. When connecting to an unknown host, a system prompt will ask for confirmation.

#### SSH Agent Authentication

When `auth_type = "agent"`, nexterm uses the system SSH agent via the socket specified by the `SSH_AUTH_SOCK` environment variable.

---

### `[web]` — Web Terminal

A built-in web terminal accessible from a browser. Disabled by default.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Enable the web terminal |
| `port` | u16 | `7681` | Listening port |
| `token` | String | — | Access token (if omitted, no authentication is required; recommended for LAN use only) |

```toml
[web]
enabled = true
port = 7681
token = "your-secret-token"
```

**How to access:**

```
# Open in browser
http://localhost:7681/?session=main&token=your-secret-token

# Direct WebSocket connection
ws://localhost:7681/ws?session=main&token=your-secret-token
```

> **Security note**: If `token` is not set, all devices on the LAN can access the terminal.
> The default bind address is `0.0.0.0` (all interfaces).
> Always set a `token` if you are using this locally.

---

### `[[macros]]` — Lua Macro Definitions

Define Lua macros that can be invoked from the command palette.
They appear in the macro picker opened with `Ctrl+Shift+M` and are executed by pressing Enter.
The return value (a string) of the macro function is sent to the focused pane's PTY.

| Key | Type | Description |
|-----|------|-------------|
| `name` | String | Display name (required). Used for fuzzy search in the picker |
| `description` | String | Description text (optional; shows `lua_fn` if omitted) |
| `lua_fn` | String | Name of the Lua global function to execute (required) |

```toml
[[macros]]
name = "top"
description = "Run top in the focused pane"
lua_fn = "macro_top"

[[macros]]
name = "git status"
description = "Show git status for the current directory"
lua_fn = "macro_git_status"

[[macros]]
name = "docker ps"
description = "List running containers"
lua_fn = "macro_docker_ps"
```

Define the corresponding Lua functions in `nexterm.lua`:

```lua
-- ~/.config/nexterm/nexterm.lua

-- Signature: function(session: string, pane_id: number) -> string
function macro_top(session, pane_id)
    return "top\n"   -- Text to send to the PTY
end

function macro_git_status(session, pane_id)
    return "git status\n"
end

function macro_docker_ps(session, pane_id)
    return "docker ps\n"
end
```

> Macro functions are executed synchronously on the `nexterm-lua-hooks` thread. A 500ms timeout is enforced; if exceeded, execution is cancelled and `None` is returned.

---

### `[[serial]]` — Serial Port Connection

Serial port settings used by `ConnectSerial` in the command palette can be entered directly in the connection dialog or specified via the protocol.

```
ConnectSerial { path: "/dev/ttyUSB0", baud: 115200 }
```

Selecting `Connect Serial` from the command palette displays an input prompt for the port and baud rate.

---

### `[log]` — Logging Settings

Settings for logging PTY output.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto_log` | bool | `false` | Automatically start logging when a session begins |
| `log_dir` | String | — | Directory to save log files |
| `timestamp` | bool | `false` | Prepend a `[HH:MM:SS]` timestamp to each line |
| `strip_ansi` | bool | `false` | Strip ANSI escape sequences from log files |
| `max_log_size` | u64 | `104857600` | Maximum log file size in bytes (default: 100MB) |
| `log_template` | String | — | Log filename template (supports `{session}`, `{date}`, `{time}`) |
| `binary` | bool | `false` | Binary PTY log mode — records raw bytes alongside text logs |

#### Log Rotation

When a log file reaches the size limit, rotation runs automatically. Existing files are renamed with `.1`, `.2`, ... suffixes, and new log data is written to a new file.

```toml
[log]
auto_log = true
log_dir = "~/nexterm-logs"
timestamp = true
strip_ansi = true
max_log_size = 52428800    # 50MB
log_template = "{session}_{date}_{time}.log"   # e.g. main_2026-03-30_14-23-01.log
binary = false
```

#### Log Filename Template

The following placeholders are available in `log_template`:

| Placeholder | Expanded value | Example |
|-------------|---------------|---------|
| `{session}` | Session name | `main` |
| `{date}` | Date `YYYY-MM-DD` | `2026-03-30` |
| `{time}` | Time `HH-MM-SS` | `14-23-01` |

```toml
# Example: "work_2026-03-30_14-23-01.log"
log_template = "{session}_{date}_{time}.log"
```

#### Recording in asciinema v2 Format

Use `nexterm-ctl record start-cast` / `nexterm-ctl record stop-cast` to record in asciinema-compatible format.

```bash
nexterm-ctl record start-cast <session> <output.cast>
nexterm-ctl record stop-cast <session>
```

Playback with the asciinema tool:

```bash
asciinema play output.cast
```

---

### `[colors.custom]` — Custom Color Palette

A custom 16-color palette used when `scheme = "custom"`.

| Key | Type | Description |
|-----|------|-------------|
| `foreground` | String | Foreground color (`#rrggbb`) |
| `background` | String | Background color (`#rrggbb`) |
| `cursor` | String | Cursor color (`#rrggbb`) |
| `ansi` | String[16] | ANSI 16 colors (black, red, green, yellow, blue, magenta, cyan, white — normal + bright for each) |

```toml
[colors]
scheme = "custom"

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

---

## Complete nexterm.toml Example

```toml
# Scrollback buffer size
scrollback_lines = 10000

[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji"]

[colors]
scheme = "tokyonight"

[shell]
program = "/usr/bin/zsh"
args = []

[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']

[window]
background_opacity = 0.95
macos_window_background_blur = 0
decorations = "full"

[tab_bar]
enabled = true
height = 28
active_tab_bg = "#ae8b2d"
inactive_tab_bg = "#5c6d74"
separator = "❯"

[terminal]
alt_screen_buffer = true
osc_window_title = true
osc_notifications = true
cjk_width = true
ime_support = true

[[keys]]
key = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key = "ctrl+shift+-"
action = "SplitHorizontal"

[[keys]]
key = "ctrl+shift+p"
action = "CommandPalette"

# Public key authentication
[[hosts]]
name = "Production Server"
host = "192.168.1.100"
port = 22
username = "deploy"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

# SSH agent authentication
[[hosts]]
name = "Staging"
host = "staging.example.com"
port = 22
username = "app"
auth_type = "agent"

# Connection via ProxyJump
[[hosts]]
name = "Internal Server"
host = "internal.company.local"
port = 22
username = "admin"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"
proxy_jump = "bastion.company.com"

[log]
auto_log = false
log_dir = "~/nexterm-logs"
timestamp = true
strip_ansi = true
max_log_size = 104857600
```

---

## nexterm.lua Reference

The Lua script acts as a dynamic override applied after TOML.
The script must return a configuration table.

### Global Variables

| Variable | Type | Description |
|----------|------|-------------|
| `nexterm` | table | The current configuration table (values after TOML has been applied) |

### Return Value

Return the configuration table as the last expression in the script. If nothing is returned, the TOML configuration is used as-is.

### Configuration Table Structure

```lua
{
  font = {
    family = "string",
    size   = 14.0,        -- float
    ligatures = true,     -- bool
  },
  colors = "string",      -- scheme name (flat string)
  shell = {
    program = "string",
  },
  scrollback_lines = 50000,
}
```

### Lua Configuration Example

```lua
-- ~/.config/nexterm/nexterm.lua

-- Get the current configuration
local cfg = require("nexterm")

-- Change the font size
cfg.font.size = 16.0

-- Use a larger size on high-DPI displays (future: DPI fetch API)
cfg.font.family = "Fira Code"

-- Increase scrollback
cfg.scrollback_lines = 100000

-- Change the color scheme
cfg.colors = "gruvbox"

return cfg
```

### Lua Event Hooks

Register callback functions in the `hooks` table to run code in response to Nexterm events.

| Hook name | Signature | Fired when |
|-----------|-----------|-----------|
| `hooks.on_session_start` | `function(session: string)` | A new session is created for the first time |
| `hooks.on_attach` | `function(session: string)` | A client attaches to a session |
| `hooks.on_detach` | `function(session: string)` | A client detaches from a session |
| `hooks.on_pane_open` | `function(session: string, pane_id: number)` | A new pane is created |
| `hooks.on_pane_close` | `function(session: string, pane_id: number)` | A pane is closed |

```lua
-- ~/.config/nexterm/nexterm.lua

-- Log when a session starts
hooks.on_session_start = function(session)
    io.write("[nexterm] session started: " .. session .. "\n")
end

-- Show a notification on attach
hooks.on_attach = function(session)
    os.execute('notify-send "nexterm" "attached to ' .. session .. '"')
end

-- Log each time a new pane opens
hooks.on_pane_open = function(session, pane_id)
    io.write(string.format("[nexterm] pane %d opened in %s\n", pane_id, session))
end
```

> **Thread model**: Hooks are executed on a dedicated `nexterm-lua-hooks` thread (does not block the main thread). If a hook throws an exception, an error is logged and the next event is processed.

---

### `require("nexterm")` Pattern

The `nexterm` module is registered in `package.preload` and can be loaded with `require`.
This allows the configuration file to be split into modules.

```lua
-- nexterm.lua
local cfg = require("nexterm")

-- To split into separate files:
-- local theme = require("my_theme")  -- Note: loading external files is not yet implemented
```

---

## Configuration Priority Summary

```
High
 │  Return value from nexterm.lua
 │  Values from nexterm.toml
 │  Built-in default values
Low
```

If only some fields are set, the remaining fields use their default values (per-field merge).

---

## When Configuration Changes Take Effect

When a configuration file is saved, the **GPU client automatically detects the filesystem change** and applies it in real time (hot reload).

| Setting | When it takes effect | Notes |
|---------|---------------------|-------|
| Font settings | Immediately (hot reload) | Changing the font family or size regenerates the glyph atlas |
| Color scheme | Immediately (hot reload) | Applied from the next frame |
| Scrollback buffer size | Immediately (hot reload) | Does not affect the existing buffer |
| Shell settings | At session creation (server side) | Does not affect running sessions |
| Key bindings | Immediately (hot reload) | Applied from the next key event |
| Status bar settings | Immediately (hot reload) | Changes to `enabled` take effect from the next frame |
| Lua widget expressions | Re-evaluated every 1 second | Changes to `nexterm.lua` are reflected in the next evaluation cycle |
| Window transparency / decorations | On restart | `background_opacity` / `decorations` are applied as window attributes at startup |
| Tab bar settings | Immediately (hot reload) | Changes to `enabled`, colors, and separator take effect from the next frame |

> Hot reload is implemented using filesystem watching via the `notify` crate. Changes are typically reflected within 100ms of detection.
