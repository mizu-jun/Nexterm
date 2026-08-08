# nexterm-server

Guidance for working inside the PTY server crate. The repo-wide rules — language policy, coding conventions, and the `Key Implementation Patterns` section (PTY reader thread, BSP layout, Lua worker) — live in the root `CLAUDE.md` and still apply here.

## Server Internals (`nexterm-server/src/`)

- `session.rs` — `SessionManager`, `Session`, BSP layout engine.
- `window/` — `Window` implementation, modularized: the BSP tree and pane management in `mod.rs`, the split algorithm in `bsp.rs` (exposes `PaneRect` / `SplitDir`), plus `tiling.rs`, `floating.rs` (exposes `FloatRect`) and `tests.rs`.
- `pane.rs` — `Pane` (PTY + PTY reader thread + recording log writer).
- `ipc/` — IPC module:
  - `platform.rs` — Unix/Windows listeners; UID validation (SO_PEERCRED / getpeereid).
  - `dispatch.rs` — Dispatch logic for 40+ IPC commands.
  - `key.rs` — Key code → VT escape sequence conversion (with 8 unit tests).
  - `plugin_dispatch.rs` — Handlers for plugin IPC commands (`ListPlugins`/`LoadPlugin`/`UnloadPlugin`/`ReloadPlugin`).
- `persist.rs` / `snapshot.rs` — Session persistence (JSON at `~/.local/state/nexterm/snapshot.json`). Schema v3 (`SNAPSHOT_VERSION = 3`, minimum supported v1; `workspace_name` added in Sprint 5-7 / Phase 2-1). Older v1/v2 snapshots are auto-migrated in `load_snapshot()`.
- `web/` — Web terminal feature (axum WebSocket + xterm.js), split into routing (`mod.rs`), token auth, OAuth, TOTP, TLS and access logging.
