# Key Bindings

See [TOML Reference — `[[keys]]`](toml.md#keys--key-bindings) for how to define custom bindings in `nexterm.toml`.

## Default Key Bindings

| Key | Action |
|-----|--------|
| `Ctrl+Shift+\` | Split pane vertically |
| `Ctrl+Shift+-` | Split pane horizontally |
| `Ctrl+Shift+P` | Open command palette |
| `Ctrl+Shift+M` | Open Lua macro picker |
| `Ctrl+Shift+F` | Search scrollback |
| `Ctrl+Shift+U` | Open SFTP upload dialog |
| `Ctrl+Shift+D` | Open SFTP download dialog |
| `Ctrl+G` | Display pane numbers (navigate by number or arrow keys) |
| `Ctrl+D` | Detach from session |

## Customizing Key Bindings

Add `[[keys]]` entries to `nexterm.toml` to override or extend defaults. The `action` value can be any built-in action name or a shell command via the `command` key.

```toml
[[keys]]
key    = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key     = "ctrl+alt+g"
command = "git log --oneline -20"
```

## Right-Click Context Menu

Right-clicking inside a GPU client pane shows a context menu with common actions: Copy, Paste, Split Vertical, Split Horizontal, Close Pane, and Display Panes.

## Display Panes Mode

Trigger `Display Panes` (default `Ctrl+G`) to overlay each pane with its number. Type the target pane number or use arrow keys to move focus. Press `Escape` to cancel.
