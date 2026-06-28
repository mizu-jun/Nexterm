# Key Bindings

The canonical, complete key binding catalogue (every default binding, the `[[keys]]` table schema, action names, modifier syntax, and platform notes) lives in:

→ **[docs/KEYBINDINGS.md](../../KEYBINDINGS.md)**

## Quick orientation

| Need | Where |
|------|-------|
| Full default binding table | [docs/KEYBINDINGS.md](../../KEYBINDINGS.md) |
| How to override / extend bindings in TOML | [docs/KEYBINDINGS.md — Customizing](../../KEYBINDINGS.md) and [docs/CONFIGURATION.md](../../CONFIGURATION.md) |
| Right-click context menu and pane-number overlay | [docs/KEYBINDINGS.md](../../KEYBINDINGS.md) |

## Minimal custom binding example

```toml
[[keys]]
key    = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key     = "ctrl+alt+g"
command = "git log --oneline -20"
```
