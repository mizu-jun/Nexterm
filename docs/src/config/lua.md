# Lua Scripting

Nexterm supports a `nexterm.lua` file that is loaded after `nexterm.toml` and can override any configuration value dynamically. It also provides event hooks and macro definitions.

## File Location

| OS | Path |
|----|------|
| Linux | `~/.config/nexterm/nexterm.lua` |
| macOS | `~/Library/Application Support/nexterm/nexterm.lua` |
| Windows | `%APPDATA%\nexterm\nexterm.lua` |

## Basic Usage

The script receives the current configuration table (after TOML is applied) via `require("nexterm")`, modifies it, and returns it.

```lua
-- ~/.config/nexterm/nexterm.lua
local cfg = require("nexterm")

cfg.font.size    = 16.0
cfg.font.family  = "Fira Code"
cfg.scrollback_lines = 100000
cfg.colors = "gruvbox"

return cfg
```

## Configuration Table Structure

```lua
{
  font = {
    family    = "string",
    size      = 14.0,     -- float
    ligatures = true,     -- bool
  },
  colors = "string",      -- scheme name (flat string)
  shell = {
    program = "string",
  },
  scrollback_lines = 50000,
}
```

## Event Hooks

Register callbacks in the global `hooks` table to react to Nexterm lifecycle events.

| Hook | Signature | Fires when |
|------|-----------|------------|
| `hooks.on_session_start` | `function(session: string)` | A new session is first created |
| `hooks.on_attach` | `function(session: string)` | A client attaches to a session |
| `hooks.on_detach` | `function(session: string)` | A client detaches from a session |
| `hooks.on_pane_open` | `function(session: string, pane_id: number)` | A new pane is created |
| `hooks.on_pane_close` | `function(session: string, pane_id: number)` | A pane is closed |

```lua
hooks.on_session_start = function(session)
    io.write("[nexterm] session started: " .. session .. "\n")
end

hooks.on_attach = function(session)
    os.execute('notify-send "nexterm" "attached to ' .. session .. '"')
end

hooks.on_pane_open = function(session, pane_id)
    io.write(string.format("[nexterm] pane %d opened in %s\n", pane_id, session))
end
```

Hooks run on a dedicated `nexterm-lua-hooks` thread and do not block the main thread. Exceptions are logged and the next event is processed normally.

## Macro Functions

Macros defined in `[[macros]]` (TOML) must have a corresponding Lua function. The function receives the session name and pane ID and returns a string that is sent to the PTY.

```lua
-- Signature: function(session: string, pane_id: number) -> string
function macro_git_status(session, pane_id)
    return "git status\n"
end

function macro_docker_ps(session, pane_id)
    return "docker ps\n"
end

function macro_top(session, pane_id)
    return "top\n"
end
```

> Macro functions have a 500 ms timeout. If exceeded, execution is cancelled and `nil` is returned.

## Status Bar Widgets

Status bar widgets configured in `[status_bar] widgets` are Lua expressions evaluated every second on the GPU client. Any valid Lua expression returning a string is accepted.

```toml
# nexterm.toml
[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']
```

## `require("nexterm")` Pattern

The `nexterm` module is registered in `package.preload` and can be loaded with `require`. This is the standard way to access and modify the current configuration table.

```lua
local cfg = require("nexterm")
-- modify cfg...
return cfg
```
