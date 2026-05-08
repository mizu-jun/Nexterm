# Lua Macro Recipes

Copy-and-paste examples for common automation tasks.
Add these to `~/.config/nexterm/nexterm.lua` (Linux/macOS) or
`%APPDATA%\nexterm\nexterm.lua` (Windows).

---

## Basics: Sending a Command

Every macro function receives `(session, pane_id)` and returns the string to
send to the PTY.  A trailing `\n` presses Enter.

```lua
-- TOML: [[macros]] name="hello" fn="macro_hello" key="ctrl+alt+h"
function macro_hello(session, pane_id)
    return "echo Hello from nexterm!\n"
end
```

---

## Recipe 1: Git Status at a Glance

```lua
function macro_git_status(session, pane_id)
    return "git status --short && git log --oneline -5\n"
end
```

TOML entry:
```toml
[[macros]]
name = "git status"
fn   = "macro_git_status"
key  = "ctrl+alt+g"
```

---

## Recipe 2: Activate Python Virtual Environment

```lua
function macro_venv(session, pane_id)
    return "source .venv/bin/activate\n"
end
```

TOML entry:
```toml
[[macros]]
name = "venv activate"
fn   = "macro_venv"
key  = "ctrl+alt+v"
```

---

## Recipe 3: Docker Compose Up/Down Toggle

```lua
local _compose_up = false

function macro_compose_toggle(session, pane_id)
    if _compose_up then
        _compose_up = false
        return "docker compose down\n"
    else
        _compose_up = true
        return "docker compose up -d\n"
    end
end
```

---

## Recipe 4: Broadcast Identical Commands to All Panes

Use the hook `on_pane_open` plus the `broadcast` feature.

```lua
function macro_broadcast_enable(session, pane_id)
    -- Enable broadcast mode: every keystroke goes to all panes
    return "\x02*"   -- Ctrl+B then *
end

function macro_broadcast_disable(session, pane_id)
    return "\x02*"
end
```

---

## Recipe 5: Open a Split and CD into the Same Directory

```lua
function macro_split_here(session, pane_id)
    -- Get current directory from the environment (set by your shell prompt)
    local cwd = os.getenv("PWD") or "~"
    -- Split vertically then cd into the same directory
    -- Uses escape sequence to trigger SplitVertical (\x02%) then sends cd
    return "\x02%cd " .. cwd .. "\n"
end
```

---

## Recipe 6: Tail a Log File in a New Pane

```lua
function macro_tail_log(session, pane_id)
    -- Split horizontally and tail the most recent log file
    return "\x02\"tail -f /var/log/syslog\n"
end
```

---

## Recipe 7: Status Bar Widget — Git Branch

Status bar widgets return a string to display in the bar.

```toml
# nexterm.toml
[status_bar]
enabled = true

[[status_bar.widgets]]
kind  = "lua"
fn    = "widget_git_branch"
width = 24
```

```lua
function widget_git_branch()
    local handle = io.popen("git symbolic-ref --short HEAD 2>/dev/null")
    if not handle then return "" end
    local branch = handle:read("*l") or ""
    handle:close()
    if branch == "" then return "" end
    return " \xef\x90\xa0 " .. branch .. " "  -- nerd font git icon
end
```

---

## Recipe 8: Status Bar Widget — CPU Usage

```lua
function widget_cpu()
    local handle = io.popen(
        "top -bn1 2>/dev/null | grep 'Cpu(s)' | awk '{print $2}'"
    )
    if not handle then return "CPU:?" end
    local val = handle:read("*l") or "?"
    handle:close()
    return " CPU:" .. val .. "% "
end
```

---

## Recipe 9: Auto-Set Window Title on Pane Open

```lua
hooks.on_pane_open = function(session, pane_id)
    -- Send OSC 0 title sequence through the PTY isn't possible directly,
    -- but we can log the event for external tools
    io.write(string.format("[nexterm] pane %d opened in '%s'\n", pane_id, session))
end
```

---

## Recipe 10: Notify on Session Start (Linux/macOS)

```lua
hooks.on_session_start = function(session)
    -- Desktop notification via notify-send (Linux) or osascript (macOS)
    local cmd
    if package.config:sub(1,1) == "/" then
        -- Unix-like
        cmd = 'notify-send "nexterm" "Session \'' .. session .. '\' started" 2>/dev/null'
        if not pcall(function() os.execute(cmd) end) then
            -- macOS fallback
            os.execute('osascript -e \'display notification "Session ' ..
                session .. ' started" with title "nexterm"\' 2>/dev/null')
        end
    end
end
```

---

## Tips

**Return `nil` or `""` to send nothing** — useful when a macro only has side effects.

```lua
function macro_log_only(session, pane_id)
    io.write("macro triggered\n")
    return ""   -- nothing sent to the PTY
end
```

**Use `\x02` for Ctrl+B** to trigger built-in prefix commands from a macro:

| Sequence | Action |
|----------|--------|
| `"\x02%"` | Split vertically |
| `"\x02\""` | Split horizontally |
| `"\x02x"` | Close pane |
| `"\x02n"` | Focus next pane |
| `"\x02z"` | Toggle zoom |

See [Lua Scripting](lua.md) for the full API reference.
