# Architecture Overview

See [`ARCHITECTURE.md`](https://github.com/mizu-jun/Nexterm/blob/master/ARCHITECTURE.md) in the repository root for the full design document.

## Component Overview

Nexterm follows a client-server architecture where the server process owns all terminal state and clients connect over a local transport.

| Binary | Role |
|--------|------|
| `nexterm-server` | Daemon that owns sessions, PTYs, and SSH connections |
| `nexterm-client-gpu` | GPU-rendered desktop client (wgpu) |
| `nexterm-client-tui` | Fallback TUI client (crossterm) |
| `nexterm-ctl` | CLI control tool for scripting and automation |
| `nexterm` | Launcher — starts server if not running, then attaches a client |

## Communication

Clients and the server exchange messages via a binary framed protocol over a Unix domain socket (Linux/macOS) or a named pipe (Windows). See [`PROTOCOL.md`](https://github.com/mizu-jun/Nexterm/blob/master/PROTOCOL.md) for the full message schema.

## Key Crates

- **nexterm-vt** — VT100/xterm parser and screen model
- **nexterm-ssh** — SSH client built on `russh`
- **nexterm-proto** — Protocol buffer definitions and codec
- **nexterm-config** — TOML + Lua configuration loading
- **nexterm-i18n** — 8-language UI string table

## Rendering Pipeline

The GPU client uses `wgpu` with a custom glyph atlas and a single render pass per frame. Font rasterization is handled by `swash`. Frames are submitted at the display refresh rate; dirty-region tracking minimizes GPU work when the terminal content is unchanged.
