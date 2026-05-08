# Configuration Overview

Nexterm is configured through two files that are loaded in order: `nexterm.toml` for static settings and `nexterm.lua` for dynamic overrides.

## File Locations

| OS | TOML path | Lua path |
|----|-----------|---------|
| Linux | `~/.config/nexterm/nexterm.toml` | `~/.config/nexterm/nexterm.lua` |
| macOS | `~/Library/Application Support/nexterm/nexterm.toml` | `~/Library/Application Support/nexterm/nexterm.lua` |
| Windows | `%APPDATA%\nexterm\nexterm.toml` | `%APPDATA%\nexterm\nexterm.lua` |

On Linux, if `XDG_CONFIG_HOME` is set, `$XDG_CONFIG_HOME/nexterm/` takes priority.

## Load Order

```
1. Built-in defaults
2. nexterm.toml  (if present)
3. nexterm.lua   (if present)
```

Values loaded later take precedence. Lua can override anything set in TOML.

## Hot Reload

The GPU client watches both config files for changes. Most settings (font, colors, key bindings, status bar) take effect immediately without restarting. See [TOML Reference](toml.md) for per-setting reload behavior.
