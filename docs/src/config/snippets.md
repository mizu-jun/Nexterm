# Common Configuration Snippets

Ready-to-use `nexterm.toml` snippets. Pick what you need and paste into your config file.

Config file locations:

| OS | Path |
|----|------|
| Linux | `~/.config/nexterm/nexterm.toml` |
| macOS | `~/Library/Application Support/nexterm/nexterm.toml` |
| Windows | `%APPDATA%\nexterm\nexterm.toml` |

---

## Appearance

### Font: Nerd Fonts / Ligature Font

```toml
[font]
family    = "JetBrainsMono Nerd Font"
size      = 15.0
ligatures = true
```

### Color Scheme

Available built-in values: `dark` `light` `tokyonight` `solarized`
`gruvbox` `catppuccin` `dracula` `nord` `onedark`

```toml
[colors]
scheme = "catppuccin"
```

### Transparent Background (macOS / Linux compositor)

```toml
[window]
background_opacity = 0.85
blur               = true    # macOS vibrancy / KDE blur
```

### Window Decorations

```toml
[window]
decorations = "full"         # "full" | "none" | "transparent" (macOS)
```

---

## Shell

### Use Fish Shell

```toml
[shell]
program = "/usr/bin/fish"
args    = []
```

### Use Zsh with Custom RC

```toml
[shell]
program = "/bin/zsh"
args    = ["--login"]
```

### Windows: Use PowerShell 7

```toml
[shell]
program = "pwsh.exe"
args    = ["-NoLogo"]
```

---

## SSH Hosts

### Password Authentication

```toml
[[hosts]]
name      = "staging"
host      = "10.0.0.50"
port      = 22
username  = "ubuntu"
auth_type = "password"
# パスワードは OS キーリングに保存される（平文では保存しない）
```

### Key Authentication

```toml
[[hosts]]
name      = "prod-web"
host      = "203.0.113.10"
port      = 22
username  = "deploy"
auth_type = "key"
key_path  = "~/.ssh/id_ed25519_prod"
```

### SSH Agent Authentication (Recommended)

```toml
[[hosts]]
name      = "git-server"
host      = "git.example.com"
port      = 22
username  = "git"
auth_type = "agent"
```

### Non-Standard Port

```toml
[[hosts]]
name      = "router"
host      = "192.168.1.1"
port      = 2222
username  = "admin"
auth_type = "key"
key_path  = "~/.ssh/id_rsa"
```

### Jump Host (ProxyJump)

```toml
[[hosts]]
name        = "internal-db"
host        = "10.10.0.20"
port        = 22
username    = "dbadmin"
auth_type   = "agent"
proxy_jump  = "bastion"     # must match the `name` of another host entry
```

### Local Port Forwarding

```toml
[[hosts]]
name          = "db-tunnel"
host          = "db.example.com"
port          = 22
username      = "ubuntu"
auth_type     = "agent"
forward_local = ["5432:localhost:5432"]   # local:5432 → remote:localhost:5432
```

### X11 Forwarding

```toml
[[hosts]]
name        = "linux-desktop"
host        = "192.168.1.100"
port        = 22
username    = "user"
auth_type   = "agent"
x11_forward = true
x11_trusted = false   # true = ssh -Y (trusted), false = ssh -X (untrusted)
```

---

## Session & Scrollback

### Increase Scrollback Buffer

```toml
scrollback_lines = 100000
```

### Custom Session Name

```toml
default_session = "work"
```

---

## Status Bar

### Minimal Status Bar

```toml
[status_bar]
enabled   = true
separator = "│"

[[status_bar.widgets]]
kind = "session"

[[status_bar.widgets]]
kind = "clock"
```

### Full Status Bar with Lua Widgets

```toml
[status_bar]
enabled   = true
separator = "│"

[[status_bar.widgets]]
kind = "session"

[[status_bar.widgets]]
kind  = "lua"
fn    = "widget_git_branch"
width = 24

[[status_bar.widgets]]
kind  = "lua"
fn    = "widget_cpu"
width = 12

[[status_bar.widgets]]
kind = "clock"
```

See [Lua Macro Recipes](lua-recipes.md) for the `widget_git_branch` and `widget_cpu` implementations.

---

## Key Bindings

### Custom Split Keys (tmux-style)

```toml
[[keys]]
key    = "ctrl+b+%"
action = "SplitVertical"

[[keys]]
key    = "ctrl+b+\""
action = "SplitHorizontal"
```

### Quick Host Connect

```toml
[[keys]]
key    = "ctrl+shift+h"
action = "ShowHostManager"
```

### Custom Shell Command

```toml
[[keys]]
key     = "ctrl+alt+t"
command = "htop\n"
```

---

## Macros

### Define a Macro

```toml
[[macros]]
name = "git log"
fn   = "macro_git_log"
key  = "ctrl+alt+l"
```

Then implement `macro_git_log` in `nexterm.lua`:

```lua
function macro_git_log(session, pane_id)
    return "git log --oneline --graph --all | head -20\n"
end
```

---

## Web Terminal

```toml
[web]
enabled  = true
port     = 8080
bind     = "127.0.0.1"   # "0.0.0.0" to allow LAN access
tls_cert = ""            # path to PEM cert (leave empty for HTTP)
tls_key  = ""
totp     = true          # require TOTP OTP on login
```

---

## GPU / Rendering

### FPS Limit

```toml
[gpu]
fps_limit = 30    # reduce on battery; increase for high-refresh displays
```

### Custom WGSL Shader

```toml
[gpu]
vertex_shader   = "~/.config/nexterm/shaders/my.vert.wgsl"
fragment_shader = "~/.config/nexterm/shaders/my.frag.wgsl"
```

See [Custom Shaders](../advanced/shaders.md) for details.

---

## Logging

### Enable Debug Log

```toml
[log]
file    = "/tmp/nexterm.log"
level   = "debug"    # "error" | "warn" | "info" | "debug" | "trace"
```
