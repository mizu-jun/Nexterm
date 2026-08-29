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

> **Unknown keys are ignored silently.** The parser does not reject keys it
> does not recognise, so a typo — or a key copied from an older revision of
> this document — produces no error and no effect. If a setting appears to do
> nothing, check its spelling against the tables below first.

### Top-level keys

Keys that live at the root of `nexterm.toml`, outside any table.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `api_version` | String | `"1.0"` | Configuration schema version |
| `language` | String | `"auto"` | UI language: `auto` (detect from the OS), `en`, `ja`, `zh-CN`, `ko`, `de`, `fr`, `es`, `it` |
| `leader_key` | String | `"ctrl+b"` | Leader key for prefixed bindings (tmux-style). See [`[[keys]]`](#keys--key-bindings) |
| `scrollback_lines` | usize | `50000` | Scrollback buffer size — [detailed below](#scrollback_lines--scrollback-buffer-size) |
| `cursor_style` | String | `"block"` | Cursor shape: `block`, `beam`, `underline`. Blink and motion live in [`[cursor]`](#cursor--cursor-blink--motion) |
| `auto_check_update` | bool | `true` | Query the GitHub Releases API for a newer version five seconds after startup |
| `colors_follow_system` | bool | `false` | Follow the OS light/dark preference instead of using `[colors]` unchanged |
| `colors_light` | String | — | Built-in scheme to use while the OS reports **light**. Only consulted when `colors_follow_system = true`; unset falls back to `light` |
| `colors_dark` | String | — | Built-in scheme to use while the OS reports **dark**. Only consulted when `colors_follow_system = true`; unset falls back to `tokyonight` |
| `active_profile` | String | — | Name of the profile to apply — see [`[[profiles]]`](#profiles--named-configuration-profiles) |
| `plugin_dir` | String | platform default | WASM plugin directory. Default: `~/.config/nexterm/plugins` (Linux/macOS), `%APPDATA%\nexterm\plugins` (Windows) |
| `plugins_disabled` | bool | `false` | Disable the plugin runtime entirely |

```toml
api_version = "1.0"
language = "ja"
leader_key = "ctrl+b"
cursor_style = "beam"
auto_check_update = true

# Follow the OS theme, choosing between two built-in schemes.
colors_follow_system = true
colors_light = "gruvbox"
colors_dark = "tokyonight"
```

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
| `"catppuccin"` | Catppuccin Mocha |
| `"dracula"` | Dracula |
| `"nord"` | Nord |
| `"onedark"` | One Dark |
| `"highcontrast"` | High Contrast — pure black ground, pure white text, every text role at WCAG AAA (7:1) |

```toml
[colors]
scheme = "tokyonight"
```

### `[inactive_pane_hsb]` — Dimming Unfocused Panes

How an unfocused pane is visually pushed back when a window is split.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `hue` | f32 | `1.0` | Hue multiplier. **Currently a no-op** (see below) |
| `saturation` | f32 | `0.6` | Saturation multiplier, `0.0`–`1.0`. `1.0` = untouched, `0.0` = grayscale |
| `brightness` | f32 | `0.85` | Brightness multiplier, `0.0`–`1.0`. `1.0` = no dimming, `0.0` = black |

```toml
[inactive_pane_hsb]
saturation = 0.6
brightness = 0.85
```

A true HSB transform needs a post-process shader pass. The current
implementation approximates it with a flat overlay: `brightness < 1.0` paints
black at alpha `1.0 - brightness`, and `saturation < 1.0` mixes that overlay
toward mid grey. A real hue shift is not possible this way, so `hue` is
accepted and ignored.

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
| `background_opacity` | float | `0.95` | Window background opacity (0.0 = fully transparent, 1.0 = opaque). A compositor is required for transparency |
| `backdrop` | String | `"auto"` | OS-native window backdrop material. See below — a backdrop is only visible where `background_opacity` is below `1.0` |
| `decorations` | String | `"notitle"` (`"full"` on macOS) | Window decoration style |
| `in_app_blur_enabled` | bool | `false` | Enable the in-app acrylic material (blurred terminal behind overlay panels). Opt-in — unverified on real GPU hardware as of this writing |
| `in_app_blur_strength` | float | `0.6` | Blend ratio between the opaque panel fill (0.0) and the full blur+tint acrylic material (1.0). Only used when `in_app_blur_enabled` is true |

#### `decorations` Values

| Value | Description |
|-------|-------------|
| `"full"` | Show the OS-native title bar and borders (default on macOS) |
| `"none"` | Hide title bar and borders (borderless) |
| `"notitle"` | Windows Terminal-style custom title bar: borderless, the tab bar doubles as the title bar (window buttons, drag-to-move, double-click maximize, edge resize). Default on Windows/Linux |

#### `backdrop` Values

| Value | Windows | macOS | Linux |
|-------|---------|-------|-------|
| `"auto"` | Mica Alt (the material Nexterm has always applied) | None | None |
| `"mica"` | Mica | Vibrancy | None |
| `"mica-alt"` | Mica Alt | Vibrancy | None |
| `"acrylic"` | Acrylic | Vibrancy | None |
| `"none"` | None | None | None |

macOS has a single `NSVisualEffectView` material family, so `mica`,
`mica-alt` and `acrylic` all resolve to the same vibrancy there. Linux has no
cross-compositor equivalent; use `in_app_blur_enabled` instead, which blurs
the terminal behind overlay panels inside the app.

Windows requires Windows 11 build 22621 (22H2) or later —
`DWMWA_SYSTEMBACKDROP_TYPE` does not exist on older builds and the request is
ignored.

> **A backdrop is drawn *behind* the window, so the terminal has to let it
> through.** With `background_opacity = 1.0` the terminal paints over the
> material and nothing is visible; Nexterm logs a warning at startup when that
> combination is configured. Lower `background_opacity` to see the backdrop.
> Nexterm does not override the opacity you configured.

```toml
[window]
background_opacity = 0.92
backdrop = "mica-alt"
decorations = "notitle"
```

### `[cursor]` — Cursor Blink & Motion

The cursor's *shape* is the top-level `cursor_style` key; this table controls
how it blinks and moves.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `blink_enabled` | bool | `true` | Blink the cursor (matches xterm) |
| `blink_interval_ms` | u32 | `530` | Blink half-period in ms — one full on/off cycle is twice this. Values below 50 ms are clamped at render time to avoid flicker |
| `smooth_motion` | bool | `true` | Interpolate the cursor between cells as it moves. `false` snaps it immediately |

```toml
cursor_style = "beam"

[cursor]
blink_enabled = true
blink_interval_ms = 530
smooth_motion = true
```

### `[ui]` — Chrome Rounding

Corner radii, in pixels, for the SDF rounded-rect background pipeline. Setting a
radius to `0.0` produces pixel-identical output to a build without rounding.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `corner_radius_chrome` | f32 | `10.0` | Tab pills, focused-pane outline, banners |
| `corner_radius_overlay` | f32 | `10.0` | Command palette, settings panel, dialogs |

Negative values are clamped to `0.0`.

```toml
[ui]
corner_radius_chrome = 10.0
corner_radius_overlay = 10.0
```

### `[animations]` — Motion

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | `"auto"` \| bool | `"auto"` | Master switch. `"auto"` follows the OS accessibility preference; `true` always animates; `false` never animates |
| `intensity` | String | `"normal"` | `off`, `subtle` (×0.5), `normal` (×1.0), `energetic` (×1.5) |

`enabled = "auto"` reads Windows' "Show animations in Windows" setting
(`SPI_GETCLIENTAREAANIMATION`) and macOS' "Reduce motion" toggle
(`NSWorkspace.accessibilityDisplayShouldReduceMotion`), and animates wherever
there is no such preference to read — today, that means Linux. Detection can
only ever *disable* motion: there is no OS signal that turns animation on, so
`auto` never animates more than `true` would. The OS value is read at startup
and again whenever the window regains focus (there is no native
change-notification to listen for), and it is never written back into this
file — `config.toml` always keeps whatever you set for `enabled`, `"auto"`
included.

`true` animates even when the OS asks for reduced motion; pre-P3c configs
with `enabled = true` / `enabled = false` keep parsing the same as before.

For a reduced-motion preference regardless of platform, set `enabled = false`
or `intensity = "off"` — both make every duration 0 ms.

```toml
[animations]
enabled = "auto"
intensity = "subtle"
```

### `[scrolling]` — Wheel & Touchpad

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `multiplier` | f32 | `3.0` | Rows per discrete wheel notch. Clamped to `1.0..=20.0` |
| `momentum` | bool | `false` | Continue touchpad scrolls with simulated inertia after the fingers lift. Applies to pixel-precision (touchpad) scrolling only — a discrete wheel never gets inertia |

`momentum` is off by default because Windows precision touchpads and macOS
already synthesize inertial events at the OS level; it mainly helps on
Linux/X11.

```toml
[scrolling]
multiplier = 3.0
momentum = false
```

### `[gpu]` — Renderer

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `fps_limit` | u32 | `60` | Frame-rate cap. `0` = unlimited |
| `atlas_size` | u32 | `2048` | Square glyph-atlas size in pixels. `4096` helps on high-DPI displays or with very large fonts |
| `present_mode` | String | `"mailbox"` | `fifo` (vsync, tearing-free, ~16 ms more latency at 60 Hz), `mailbox` (low latency), `auto` (the adapter chooses) |
| `custom_bg_shader` | String | — | Path to a WGSL shader replacing the background-rectangle shader |
| `custom_text_shader` | String | — | Path to a WGSL shader replacing the glyph shader |

`mailbox` falls back to `fifo` automatically where it is unsupported (some
Wayland compositors).

```toml
[gpu]
fps_limit = 60
atlas_size = 2048
present_mode = "mailbox"
```

#### Custom shaders

Both shaders must define `@vertex fn vs_main` and `@fragment fn fs_main`.

- **Background** vertex input (7 attributes): `position: vec2<f32>`,
  `color: vec4<f32>`, `rect_center: vec2<f32>`, `rect_half_size: vec2<f32>`,
  `corner_radius: f32`, `shadow_softness: f32`, `stroke_width: f32`. The last
  two were added for soft shadows and outlines; a shader reading only the first
  five keeps working, because wgpu only requires that shader inputs be a subset
  of the buffer layout.
- **Text** vertex input: `position: vec2<f32>`, `uv: vec2<f32>`,
  `color: vec4<f32>`. Bindings: `@group(0) @binding(0)` is the glyph texture,
  `@binding(1)` its sampler.

> **The fragment output must be premultiplied alpha (`rgb * a`).** Every
> pipeline blends with `PREMULTIPLIED_ALPHA_BLENDING` against a premultiplied
> surface. A shader written for straight alpha will look wrong wherever it is
> not fully opaque.

```toml
[gpu]
custom_bg_shader = "~/.config/nexterm/shaders/crt.wgsl"
```

### `[quake_mode]` — Drop-down Window

Slides the window in from a screen edge on a global hotkey — the "hotkey
window" of Guake, Tilix and iTerm2.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | When `false`, no hotkey is registered |
| `hotkey` | String | `"ctrl+\`"` | Modifiers joined with `+`: `ctrl` / `alt` / `shift` / `super` (or `meta` / `cmd` / `win`), last token is the key |
| `edge` | String | `"top"` | Anchor edge: `top`, `bottom`, `left`, `right` |
| `height_pct` | u8 | `45` | Percentage of screen height (1–100), for a top/bottom edge |
| `width_pct` | u8 | `100` | Percentage of screen width (1–100) |
| `animation_ms` | u32 | `150` | Slide duration. `0` disables the animation |
| `always_on_top` | bool | `true` | Keep the window topmost while visible |
| `minimize_on_hide` | bool | `false` | Minimize when hiding instead of just hiding. On macOS this maps to `Hide`, which also removes the window from the Dock — leaving it `false` is recommended there |

```toml
[quake_mode]
enabled = true
hotkey = "ctrl+`"
edge = "top"
height_pct = 45
animation_ms = 150
```

> **Wayland**: global hotkeys can only be registered through the compositor by
> spec, so the hotkey works on Windows, macOS and Linux/X11 but **not on
> Wayland**. On Wayland, bind `nexterm-ctl quake toggle` in your compositor
> config instead.

### Terminal features (always on — not configurable)

These behaviours are unconditional and have **no configuration keys**:

| Feature | Notes |
|---------|-------|
| Alternate screen buffer (SMCUP/RMCUP) | Used by `less`, `vim`, `htop` and friends to swap the display and restore it on exit |
| DEC Private Mode 47 / 1047 / 1049 | The escape-sequence variants of the same mechanism |
| OSC 0 / 1 / 2 window title | The window and tab titles follow what the application sets |
| OSC 9 / 777 desktop notifications | Gated by consent policy, not by an on/off switch — see [`[security]`](#security--consent-policy) |
| CJK width calculation | East-Asian wide characters occupy two cells |
| IME (Input Method Editor) | Pre-edit composition is drawn inline |

> **Removed from this document:** earlier revisions described a `[terminal]`
> table with a boolean for each row above. **No such table has ever existed in
> the code.** Because unknown keys are ignored silently, a config written
> against that section parsed without complaint and changed nothing. If your
> `nexterm.toml` still carries a `[terminal]` block, it is dead weight and can
> be deleted.

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
| `proxy_jump` | String | — | ProxyJump host — the `name` of another entry in `hosts` |
| `forward_local` | String[] | `[]` | Local port forwards, each `"<local>:<host>:<remote>"` |
| `forward_remote` | String[] | `[]` | Remote port forwards (`-R`), each `"<remote>:<host>:<local>"` |
| `x11_forward` | bool | `false` | Enable X11 forwarding (equivalent to `ssh -X`) |
| `x11_trusted` | bool | `false` | Trusted X11 forwarding (equivalent to `ssh -Y`, takes precedence over `x11_forward`) |
| `group` | String | `""` | Free-form group name used to categorise hosts in the manager |
| `tags` | String[] | `[]` | Labels used for filtering in the host manager |

> Earlier revisions of this document listed a `socks5_proxy` key and described
> the forwards as tables (`[[hosts.local_forwards]]` with `local_port` /
> `remote_host` / `remote_port`). **Neither matches the code**: there is no
> SOCKS5 key, and both forward lists are arrays of strings. Since unknown keys
> are ignored silently, a config written that way connected with no forwarding
> and no warning.

#### SSH Authentication Methods

- `"password"` — Password authentication (stored securely in the OS keychain)
- `"key"` — Public key authentication (specify a private key file)
- `"agent"` — SSH agent authentication (uses `SSH_AUTH_SOCK`)

#### Local Port Forwarding

Maps a local port onto a `host:port` reachable from the SSH server. Each entry
is a single string, in the same `<local>:<host>:<remote>` order `ssh -L` uses.

```toml
[[hosts]]
name = "App Server"
host = "app.internal"
username = "deploy"
forward_local = ["8080:localhost:3000"]
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

# With local port forwarding, a group and tags
[[hosts]]
name = "DB Server"
host = "db.internal"
port = 22
username = "dbadmin"
auth_type = "key"
key_path = "~/.ssh/db_key"
forward_local = ["5432:localhost:5432"]
group = "production"
tags = ["db", "postgres"]
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
forward_remote = ["9090:localhost:9090"]
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

### `[security]` — Consent Policy

Governs operations a remote program can ask the terminal to perform. Each policy
is `allow`, `deny`, or `prompt` (show a modal and let the user decide).

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `external_url` | String | `"prompt"` | Opening a URL via an OSC 8 hyperlink or Ctrl+click |
| `osc52_clipboard` | String | `"prompt"` | An OSC 52 clipboard-write request |
| `osc_notification` | String | `"prompt"` | An OSC 9 / 777 desktop-notification request |
| `osc52_max_bytes` | usize | `1048576` | Hard cap (1 MiB) on an OSC 52 write. Larger requests are rejected outright, whatever the policy says |
| `notification_max_bytes` | usize | `4096` | Cap on notification text. Excess is truncated |
| `plugin_read` | String | `"deny"` | Whether WASM plugins may read terminal contents (`read_pane` / `read_grid` / `read_scrollback`) |
| `plugin_read_max_bytes` | usize | `1048576` | Cap on a single plugin read. Larger results are truncated at a UTF-8 boundary (text) or byte boundary (grid dump) |

```toml
[security]
external_url = "prompt"
osc52_clipboard = "prompt"
osc_notification = "prompt"
plugin_read = "deny"
```

> **`plugin_read` defaults to `deny`, not `prompt`.** The plugin read API is an
> information-egress channel and stays off until an operator opts in. There is
> no synchronous prompt path for a server-side plugin call, so **`prompt` is
> treated as `deny`** rather than silently allowing the read.

### `[hooks]` — Event Hooks

Commands or Lua functions run when a terminal event fires. Every field is
optional.

| Key | Type | Description |
|-----|------|-------------|
| `on_pane_open` | String | Shell command run when a pane opens |
| `on_pane_close` | String | Shell command run when a pane closes |
| `on_session_start` | String | Shell command run when a session starts |
| `on_attach` | String | Shell command run when a client attaches |
| `on_detach` | String | Shell command run when a client detaches |
| `lua_on_pane_open` | String | Name of a Lua function to call on pane open |
| `lua_on_pane_close` | String | Name of a Lua function to call on pane close |
| `lua_on_session_start` | String | Name of a Lua function to call on session start |
| `lua_on_attach` | String | Name of a Lua function to call on attach |
| `lua_on_detach` | String | Name of a Lua function to call on detach |

Shell hooks run through `sh -c` with `$NEXTERM_PANE_ID` and `$NEXTERM_SESSION`
available. Lua hooks take the *name* of a function defined in `nexterm.lua`.

```toml
[hooks]
on_pane_open = "echo pane $NEXTERM_PANE_ID opened >> ~/nexterm.log"
lua_on_session_start = "on_session_start"
```

```lua
-- nexterm.lua
function on_session_start(session)
  print("session started: " .. session)
end
```

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

### `[blocks]` — Command Blocks (Warp-style block UI)

Drives the Warp-style block overlay rendered alongside the grid when the
shell emits OSC 133 prompt sequences. See
[`shell-integration.md`](shell-integration.md) for the bash / zsh / fish
prompt snippets needed to enable the underlying markers; without them
this whole section is a no-op.

```toml
[blocks]
enabled = true            # Master switch; set to false to skip the overlay pass
border_width_px = 2       # Left-border width in pixels (clamped to 1..=8)
show_exit_code_badge = true  # Reserve for the on-screen ✓ / ✗ / ● badge (renderer support lands later)
```

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `enabled` | `bool` | `true` | When `false` the renderer skips the block overlay entirely |
| `border_width_px` | `u8` | `2` | Clamped to the range `1..=8` at draw time |
| `show_exit_code_badge` | `bool` | `true` | Forward-compat flag; the glyph badge ships once on-device verification is available |

Named blocks are stored at `~/.local/state/nexterm/named_blocks.json`
(`%APPDATA%\nexterm\named_blocks.json` on Windows) with atomic write +
mode 0600, capped at 10 000 entries.

---

### `[[serial_ports]]` — Serial Port Presets

Named serial-port presets. Each entry is one connection you can pick without
retyping its parameters.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | String | — | Display name (required) |
| `port` | String | — | Device path — e.g. `/dev/ttyUSB0`, `COM3` (required) |
| `baud_rate` | u32 | `115200` | Baud rate |
| `data_bits` | u8 | `8` | Data bits: 5, 6, 7 or 8 |
| `stop_bits` | u8 | `1` | Stop bits: 1 or 2 |
| `parity` | String | `"none"` | Parity: `"none"`, `"odd"`, `"even"` |

```toml
[[serial_ports]]
name = "Arduino"
port = "/dev/ttyUSB0"
baud_rate = 115200

[[serial_ports]]
name = "Router console"
port = "/dev/ttyUSB1"
baud_rate = 9600
data_bits = 8
stop_bits = 1
parity = "none"
```

> The table name is `serial_ports`, **not `serial`** — earlier revisions of this
> document titled this section `[[serial]]`, which the parser ignores.

You can also connect ad hoc without a preset: `ConnectSerial` in the command
palette prompts for the port and baud rate, and a key binding can pass them
directly.

```
ConnectSerial { path: "/dev/ttyUSB0", baud: 115200 }
```

---

### `[log]` — Logging Settings

Settings for logging PTY output.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `auto_log` | bool | `false` | Automatically start logging when a session begins |
| `log_dir` | String | — | Directory to save log files |
| `timestamp` | bool | `false` | Prepend a `[HH:MM:SS]` timestamp to each line |
| `strip_ansi` | bool | `false` | Strip ANSI escape sequences from log files |
| `file_name_template` | String | — | Log filename template (see below). Unset uses the directory plus a fixed file name |
| `binary_log` | bool | `false` | Also write raw PTY bytes to a `.bin` file next to the text log |

```toml
[log]
auto_log = true
log_dir = "~/nexterm-logs"
timestamp = true
strip_ansi = true
file_name_template = "{session}_{pane}_{datetime}.log"
binary_log = false
```

#### Log Filename Template

Placeholders available in `file_name_template`:

| Placeholder | Expanded value | Example |
|-------------|---------------|---------|
| `{session}` | Session name | `main` |
| `{pane}` | Pane ID | `3` |
| `{datetime}` | Start time, `YYYYMMDD_HHMMSS` | `20260330_142301` |

```toml
# Produces e.g. "work_3_20260330_142301.log"
file_name_template = "{session}_{pane}_{datetime}.log"
```

#### Log size and rotation (current limitation)

**Logs started from `nexterm.toml` are never rotated and grow without bound.**
There is no size-limit key. Plan for the disk usage of a long-running session,
or rotate the files with an external tool such as `logrotate`.

The rotation machinery itself exists in the server (rename to `.1`, `.2`, …,
keeping a bounded number of files), but the config-driven recording path passes
a limit of `0`, which disables it. Wiring a size limit through to
`nexterm.toml` is outstanding work.

> Earlier revisions of this document listed a `max_log_size` key defaulting to
> 100 MB and described rotation as automatic. **Neither was ever true of a
> config-driven log**: the key does not exist, and — since unknown keys are
> ignored silently — setting it produced no error and no rotation.

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

### `[[profiles]]` — Named Configuration Profiles

A profile overrides part of the configuration under a name. Selecting one with
the top-level `active_profile` key applies its overrides on top of everything
else; any field left unset keeps the base value.

| Key | Type | Description |
|-----|------|-------------|
| `name` | String | Profile name, unique (required) |
| `icon` | String | Icon shown in tabs and the context menu (emoji or ASCII) |
| `font` | Table | Overrides `[font]` |
| `colors` | Table | Overrides `[colors]` |
| `shell` | Table | Overrides `[shell]` |
| `scrollback_lines` | usize | Overrides the top-level `scrollback_lines` |
| `tab_bar` | Table | Overrides `[tab_bar]` |
| `working_dir` | String | Initial working directory |
| `env` | Table | Extra environment variables for the launched shell |

```toml
active_profile = "work"

[[profiles]]
name = "work"
icon = "🏢"
working_dir = "~/projects"
scrollback_lines = 100000

[profiles.font]
family = "Hack Nerd Font"
size = 14.0

[profiles.colors]
scheme = "catppuccin"

[profiles.env]
NEXTERM_PROFILE = "work"
```

> `working_dir` and `env` are consumed when a shell is launched; the other
> fields are merged into the effective `Config`.

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
# Top-level keys
api_version = "1.0"
language = "auto"
leader_key = "ctrl+b"
scrollback_lines = 10000
cursor_style = "block"
auto_check_update = true

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
backdrop = "auto"
decorations = "notitle"
in_app_blur_enabled = false
in_app_blur_strength = 0.6

[tab_bar]
enabled = true
height = 28
active_tab_bg = "#ae8b2d"
inactive_tab_bg = "#5c6d74"
separator = "❯"

[cursor]
blink_enabled = true
blink_interval_ms = 530
smooth_motion = true

[ui]
corner_radius_chrome = 10.0
corner_radius_overlay = 10.0

[animations]
enabled = "auto"
intensity = "normal"

[scrolling]
multiplier = 3.0
momentum = false

[gpu]
fps_limit = 60
atlas_size = 2048
present_mode = "mailbox"

[security]
external_url = "prompt"
osc52_clipboard = "prompt"
osc_notification = "prompt"
plugin_read = "deny"

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
file_name_template = "{session}_{pane}_{datetime}.log"
binary_log = false
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
| Window transparency / decorations / backdrop | On restart | `background_opacity` / `decorations` / `backdrop` are applied as window attributes at startup |
| Tab bar settings | Immediately (hot reload) | Changes to `enabled`, colors, and separator take effect from the next frame |

> Hot reload is implemented using filesystem watching via the `notify` crate. Changes are typically reflected within 100ms of detection.
