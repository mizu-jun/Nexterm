# Quick Start

This guide walks you from installation to a working multi-pane SSH session in under five minutes.

## 1. Install

Choose the method for your platform:

=== macOS (Homebrew)
```sh
brew install mizu-jun/nexterm/nexterm
```

=== Windows (winget)
```sh
winget install mizu-jun.Nexterm
```

=== Windows (Scoop)
```sh
scoop bucket add mizu-jun https://github.com/mizu-jun/scoop-mizu-jun
scoop install nexterm
```

=== Linux (AppImage)
```sh
curl -L https://github.com/mizu-jun/nexterm/releases/latest/download/nexterm-x86_64.AppImage -o nexterm
chmod +x nexterm && ./nexterm
```

See [Installation](install.md) for full platform details.

---

## 2. Launch

```sh
nexterm
```

This starts the server and attaches the GPU client in one step.
On first launch you will see an empty terminal with a status bar at the bottom.

To start the server and client separately:

```sh
nexterm-server &          # バックグラウンドでサーバーを起動
nexterm-client-gpu        # GPU クライアントを接続
```

---

## 3. Split the Terminal

| Key | Action |
|-----|--------|
| `Ctrl+Shift+\` | Split right (vertical) |
| `Ctrl+Shift+-` | Split below (horizontal) |
| `Ctrl+G` | Show pane numbers then navigate |
| `Ctrl+Shift+P` | Open command palette |

Right-click inside any pane for a context menu with split and close actions.

---

## 4. Connect to an SSH Host

**Option A — Command palette**

1. Press `Ctrl+Shift+P`
2. Type `host` and select **Show Host Manager**
3. Start typing the hostname; press `Enter` to connect

**Option B — Add a host to `nexterm.toml`**

```toml
[[hosts]]
name     = "web-prod"
host     = "192.0.2.1"
port     = 22
username = "deploy"
auth_type = "agent"        # "agent", "key", or "password"
```

Save the file — Nexterm reloads configuration without restart.

**Option C — Use your existing `~/.ssh/config`**

Nexterm reads `~/.ssh/config` automatically.
All non-wildcard hosts appear in the Host Manager.

---

## 5. Detach and Reattach

Sessions survive client disconnection.

```sh
# Detach (keep session running)
Ctrl+D

# List running sessions
nexterm-ctl list

# Reattach
nexterm-ctl attach main
```

---

## 6. Customize Appearance

Open the settings panel with `Ctrl+,` and adjust:

| Tab | What you can change |
|-----|---------------------|
| **Font** | Family name, size |
| **Colors** | One of 9 built-in schemes |
| **Window** | Background opacity |

Changes are saved to `nexterm.toml` automatically.

---

## Next Steps

- [Key Bindings](config/keybindings.md) — full keyboard reference
- [TOML Reference](config/toml.md) — all configuration options
- [Lua Scripting](config/lua.md) — automate with macros and hooks
- [Lua Macro Recipes](config/lua-recipes.md) — copy-and-paste examples
- [SSH & Connectivity](features/ssh.md) — port forwarding, SFTP, X11
