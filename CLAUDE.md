# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **日本語版:** [CLAUDE.ja.md](CLAUDE.ja.md)

## Language Policy

Nexterm is an open-source project distributed worldwide, but the primary maintainer works in Japanese. The split is therefore:

**Japanese (interactive surface — what the maintainer reads in real time):**
- **Claude Code CLI conversation**: all chat replies, status updates, end-of-turn summaries, option blocks, and clarifying questions are in Japanese. This overrides the global "respond in Japanese" rule only in the sense of making it explicit for this repo — there is no English-conversation mode.
- **Local commit messages on personal branches**: Japanese is acceptable while iterating.

**English (artefacts that ship to the world — what external contributors read):**
- **Source code comments** (`//`, `///`, doc-comments, `expect("...")` messages, `panic!` messages, `log::*!` strings, `anyhow!`/`bail!` literals): English only.
- **Repository documentation** (`README.md`, `docs/**`, `CHANGELOG.md`, ADRs, `examples/**/README.md`, `nexterm-vt/fuzz/README.md`, etc.): English as the canonical/primary file. A Japanese translation, when provided, lives next to the English file as `*.ja.md` (e.g. `README.md` + `README.ja.md`). Currently only the top-level `README` is kept bilingual.
- **Commit messages on PRs that target `master`** and **PR descriptions / titles**: English.
- **Git tags and GitHub Release notes**: English (a Japanese supplement is welcome but not the primary text).
- **Claude Code instruction files** (this `CLAUDE.md`): English. The companion `CLAUDE.ja.md` is a translation kept for local reference and is **not** authoritative — when the two disagree, this file wins.

**Application-facing strings (separate concern):**
- **User-facing strings in the running app**: managed by `nexterm-i18n` (Fluent + JSON locales). Add new strings to **all 8 locale files** under `nexterm-i18n/locales/`. Do not hard-code natural language in the renderer.

When adding a new document, default to English and only create a `*.ja.md` companion if Japanese readability is required for that specific document.

**Rule of thumb:** if a human will read it inside a terminal session with Claude, write Japanese; if it will land in the repo or on GitHub for the world to see, write English.

## Build Commands

```bash
# Linux build dependencies (Ubuntu/Debian)
sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev

# Build all crates
cargo build

# Release build
cargo build --release

# Build a specific crate
cargo build -p nexterm-server
cargo build -p nexterm-client-gpu
cargo build -p nexterm-ctl

# Run tests
cargo test
cargo test -p nexterm-vt                      # one crate
cargo test bsp_split                          # filter by test name
cargo test --test ipc_integration             # nexterm-server integration tests
cargo test --test snapshot_roundtrip          # snapshot round-trip test

# Lint
cargo clippy -- -D warnings        # required for PR merge
cargo fmt --check                  # required for PR merge
cargo fmt                          # apply formatting
cargo audit                        # vulnerability scan (cargo install cargo-audit)

# Debug runs
NEXTERM_LOG=debug nexterm-server
NEXTERM_LOG=trace nexterm-client-gpu   # dump every IPC message
```

## Architecture

### Process Layout

```
nexterm (= nexterm-client-gpu's bin name "nexterm" — single binary)
  ├─ nexterm_server::run_server()   internal tokio task (owns PTY sessions)
  └─ wgpu renderer + winit          (GUI client)
```

Auxiliary binaries:
- `nexterm-client-tui` — TUI fallback (ratatui + crossterm).
- `nexterm-server` — standalone server process (e.g. systemd).
- `nexterm-ctl` — CLI tool (list/new/attach/kill/record).

IPC uses a Unix socket (`$XDG_RUNTIME_DIR/nexterm.sock`) or a Windows named pipe (`\\.\pipe\nexterm-<USERNAME>`). Messages are postcard-serialized with a 4-byte little-endian length prefix (migrated from bincode 1.x in Sprint 5-1 / ADR-0006; see `nexterm-proto/src/codec.rs`). When `nexterm` runs as a single binary, the GUI and the embedded server task communicate through the same IPC channel, so `nexterm-ctl` and other clients connect identically.

The legacy `nexterm-launcher` crate was removed in v1.4.0. Single-binary mode (the `nexterm` bin in `nexterm-client-gpu` spawns the server task internally) shipped in v0.9.3 and the launcher had been redundant ever since; leaving it around caused bin-name collisions. See the v1.4.0 release notes for details.

### Crate Dependencies

- `nexterm-proto` — All IPC type definitions. Central crate every other crate depends on; changes ripple project-wide.
- `nexterm-vt` — Wrapper around the `vte` crate. VT100/ANSI parser + virtual screen (`Grid`) + Sixel/Kitty image decoding.
- `nexterm-server` — PTY server. Hierarchy: `SessionManager → Session → Window (BSP) → Pane`.
- `nexterm-config` — TOML + Lua config. Load order: defaults → `config.toml` → `config.lua`. Hot reload via the `notify` crate.
- `nexterm-client-gpu` — wgpu renderer (winit 0.30 `ApplicationHandler`). Three-pass rendering: background quads → text → images. See the GPU client section below for update checking (`update_checker.rs`).
- `nexterm-client-tui` — TUI fallback using ratatui + crossterm.
- `nexterm-ssh` — SSH client built on russh 0.60 (upgraded for GHSA-f5v4-2wr6-hqmg pre-auth DoS; uses the `ring` backend to avoid the NASM dependency).
- `nexterm-plugin` — WASM plugin runtime on wasmi. `PLUGIN_API_VERSION = 1` identifies the stable ABI. `PluginManager::unload(path)` / `reload(path)` provide runtime unload/reload. Plugins may export `nexterm_meta` to publish name and version. The server holds it as `Arc<Mutex<Option<PluginManager>>>` on `SessionManager.plugin_manager`, and IPC commands (`ListPlugins`/`LoadPlugin`/`UnloadPlugin`/`ReloadPlugin`) operate on it.
- `nexterm-i18n` — 8-language support (en/ja/zh-CN/ko/de/fr/es/it). User-facing strings must use the `fl!` macro.
- `nexterm-ctl` — CLI tool (list/new/attach/kill/record).

### Server Internals (`nexterm-server/src/`)

- `session.rs` — `SessionManager`, `Session`, BSP layout engine.
- `window/` — `Window` implementation (modularized):
  - `mod.rs` — `Window` itself (BSP tree + pane management).
  - `bsp.rs` — BSP split algorithm (exposes `PaneRect` / `SplitDir`).
  - `tiling.rs` — Tiling layout logic.
  - `floating.rs` — Floating windows (exposes `FloatRect`).
  - `tests.rs` — Layout unit tests including `bsp_split`.
- `pane.rs` — `Pane` (PTY + PTY reader thread + recording log writer).
- `ipc/` — IPC module:
  - `platform.rs` — Unix/Windows listeners; UID validation (SO_PEERCRED / getpeereid).
  - `handler.rs` — Per-client read/write loop.
  - `dispatch.rs` — Dispatch logic for 40+ IPC commands.
  - `key.rs` — Key code → VT escape sequence conversion (with 8 unit tests).
  - `sftp.rs` — SFTP upload/download helpers.
  - `plugin_dispatch.rs` — Handlers for plugin IPC commands (`ListPlugins`/`LoadPlugin`/`UnloadPlugin`/`ReloadPlugin`).
- `persist.rs` / `snapshot.rs` — Session persistence (JSON at `~/.local/state/nexterm/snapshot.json`). Schema v3 (`SNAPSHOT_VERSION = 3`, minimum supported v1; `workspace_name` added in Sprint 5-7 / Phase 2-1). Older v1/v2 snapshots are auto-migrated in `load_snapshot()`.
- `hooks.rs` — Lua hook event handling.
- `serial.rs` — Serial port connections.
- `template.rs` — Session templates.
- `web/` — Web terminal feature (axum WebSocket + xterm.js):
  - `mod.rs` — Endpoints and routing.
  - `auth.rs` — Token authentication.
  - `oauth.rs` — OAuth flow.
  - `otp.rs` — TOTP (time-based one-time password).
  - `tls.rs` — TLS config and certificate loading.
  - `access_log.rs` — Access logging.
- `test_utils.rs` — In-crate test helpers.

### Integration Tests (`nexterm-server/tests/`)

- `ipc_integration.rs` — Round-trip tests covering the full IPC command surface.
- `snapshot_roundtrip.rs` — Save → load round-trip for snapshots.

### GPU Client Internals (`nexterm-client-gpu/src/`)

- `renderer.rs` — wgpu initialization + three-pass render pipeline + cosmic-text glyph atlas + winit event loop.
- `state.rs` — `ClientState` (panes, pane layouts, copy mode, search, context menu, etc.).
- `font.rs` — `FontManager` (cosmic-text wrapper, CJK width calculation).
- `glyph_atlas.rs` — GPU glyph atlas management. Uses `LruCache` to cache glyphs (stale entries are evicted on font change). `new_with_config(device, atlas_size)` initializes using the configured size as the upper bound.
- `shaders.rs` — WGSL shader constants.
- `vertex_util.rs` — Vertex buffer utilities.
- `color_util.rs` — Color palette conversions.
- `key_map.rs` — Key input mapping.
- `connection.rs` — IPC connection to the server.
- `settings_panel.rs` — Settings panel UI opened with `Ctrl+,` (7 categories, writes back via `toml_edit`, language picker uses `LANGUAGE_OPTIONS`).
- `palette.rs` — Command palette (`Ctrl+Shift+P`). Fuzzy search via `SkimMatcherV2`. Sprint 5-7 / Phase 3-3 covers all 25 actions in `execute_action` (Quit, ClosePane, NewWindow, QuickSelect, SetBroadcastOn/Off, …) and persists usage history at `~/.local/state/nexterm/palette_history.json` (atomic write, mode 0600). The pure `rank_actions` function orders by history when the query is empty (last_used desc → use_count desc) and combines fuzzy score with a `history_bonus` (use_count×10 capped at 100, +100 within 1 day, +50 within 1 week) when a query is present. `record_use` records the selection.
- `scrollback.rs` — Scrollback management + incremental search.
- `host_manager.rs` — SSH host manager UI. `load_history()` / `save_history()` persist connection frequency to `host_history.json`. The `PasswordModal` struct handles the password prompt for `auth_type="password"` hosts.
- `macro_picker.rs` — Lua macro picker UI.
- `update_checker.rs` — Polls the GitHub Releases API five seconds after startup. Disabled by `auto_check_update = false`. Results land in `ClientState.update_banner`; `Esc` dismisses, `Enter` opens the release page.
- `platform.rs` — Platform-specific utilities. `apply_acrylic_blur` enables the Windows 11 Acrylic effect via `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE=4)` (no-op on Windows 10 and earlier). `open_releases_url` opens the release page in the default browser.
- `renderer/background_pass.rs` — Background image rendering (Sprint 5-7 / Phase 3-1). When `WindowConfig.background_image` is set, the image is loaded at startup and each frame draws clear → background image → cell backgrounds → text. NDC + UV computation for each fit mode (cover/contain/stretch/center/tile) lives in the pure function `compute_background_quad`, with 11 unit tests. Images larger than 4096×4096 are downscaled with Lanczos3. Tile mode falls back to stretch when the tile count exceeds 256 (defensive). Reuses the existing `image_pipeline` (used for Sixel/Kitty) instead of introducing a separate one. Supported formats: PNG / JPEG (whichever features are enabled in the workspace `image` crate).
- `animations.rs` — UI animation foundation (Sprint 5-7 / Phase 3-2). Easing helpers (`ease_out_cubic`, `linear`, …) and `AnimationManager` (timestamps for tab switches and pane insertions). The renderer queries progress in [0,1] via `tab_switch_progress(now, duration)` / `pane_fade_in_progress(id, now, duration)`. When `Config.animations.enabled = false` or `intensity = "off"`, `scaled_duration_ms` returns 0 and all animations apply instantly (reduced-motion support). `intensity` has four levels: `off`, `subtle` (×0.5), `normal` (×1.0), `energetic` (×1.5). Tab switching is a 200 ms ease-out (accent line expands from the center, with fade-in); new pane insertion is a 250 ms white overlay fading from alpha 0.35 to 0.

## Key Implementation Patterns

### PTY Reader Thread (the daemonless design)

Each pane spawns a reader thread via `tokio::task::spawn_blocking`. On client connect/disconnect the `Arc<Mutex<Sender<ServerToClient>>>` is swapped atomically, which lets the session outlive any individual client.

### BSP Layout (pane splits)

A recursive tree of the `SplitNode` enum. Pane creation order matters: reserve the pane ID first → insert into the tree → recompute all pane sizes → spawn the PTY → resize the existing panes. This sequence avoids the chicken-and-egg problem.

### Lua Worker

The `mlua::Lua` instance lives on its own dedicated OS thread (`nexterm-lua-worker`) and communicates with the main thread over channels. `StatusBarEvaluator` requests a re-evaluation every second; it returns the cached value immediately and refreshes in the background.

### TOML Write-back from the Settings Panel

Use the `toml_edit` crate so existing comments and structure are preserved when values are updated. Do not rewrite the file wholesale via the `toml` crate.

### Language Selection

`LANGUAGE_OPTIONS: &[(&str, &str)]` (display name, language code) in `settings_panel.rs` manages the picker. Changing it from the settings panel writes the `language` key back to `config.toml`, and `nexterm-i18n` applies it on next launch. When adding a new display string, add it to **all 8 JSON locale files** under `nexterm-i18n/locales/`.

### Context Menu Width

`build_context_menu_verts` in `renderer.rs` computes the menu width dynamically from the text length. Do not hard-code a fixed width (translations in some languages overflow).

### Cursor Style, Window Padding, Present Mode

- `CursorStyle` in `nexterm-config` (block/beam/underline) is selected via `config.cursor_style`. `vertex_util::draw_cursor()` switches the shape.
- `WindowConfig.padding_x` / `padding_y` (pixels) are applied as the grid origin offset: `grid_offset_y = tab_bar_h + padding_y`.
- `GpuConfig.present_mode` (fifo/mailbox/auto) is converted to `wgpu::PresentMode` inside `WgpuState::new` and set on `SurfaceConfiguration`.

## Coding Conventions

- No `unwrap()`. Use `?` or `expect("reason")` with a concrete message.
- Propagate errors with `anyhow::Result`.
- Async: `tokio::spawn`; for blocking work use `tokio::task::spawn_blocking`.
- IPC mutex: `tokio::sync::Mutex`; PTY reader thread mutex: `std::sync::Mutex`.
- User-facing strings must go through the `nexterm_i18n::fl!` macro and be added to all 8 locales under `nexterm-i18n/locales/`.
- When adding a protocol message, check both `nexterm-proto/src/message.rs` and `nexterm-proto/src/grid.rs`.
- **Comments and doc-strings must be in English** (see "Documentation Language Policy" above).

## UI/UX Guidelines (important)

This project renders its own GUI with Rust + wgpu + cosmic-text. There is no web frontend (no HTML, CSS, React, Vue, or DOM).

- **The global `frontend-design` skill does not apply here.** That skill assumes a web UI (HTML/CSS/JS, React, CSS variables, CSS animations, browser font pairs, etc.) and its output does not fit Nexterm's wgpu renderer.
- For UI proposals, follow these existing patterns:
  - **Rendering**: draw through `renderer/overlay/` (tab bar, status bar, dialogs) and the vertex builders in `vertex_util.rs`. Do not emit CSS or DOM.
  - **Fonts**: go through `FontManager` (the cosmic-text wrapper) in `font.rs`. Do not pull in Google Fonts or web fonts.
  - **Colors**: use the palette helpers in `color_util.rs` and `ColorScheme` (theme switching lives in the settings panel).
  - **Animations**: frame-driven. There is no `prefers-reduced-motion` media query; intensity is controlled by `config.toml` instead.
  - **Strings**: every user-facing string must be added to all 8 languages via `nexterm_i18n::fl!`.
  - **Accessibility**: contrast ratio ≥ 4.5:1, keyboard-only operation must work, respect IME composition (reuse the existing `ime_preedit` path).
- Primary areas for UI/UX work: `settings_panel.rs`, `host_manager.rs`, `palette.rs`, `macro_picker.rs`, `renderer/overlay/`, `state/menus.rs`.

## Release Flow

Releases are automated by `.github/workflows/release.yml` and triggered by pushing a version tag (`v*.*.*`). The Windows installer (`.msi`) is built with WiX v3; components are managed in `wix/main.wxs` (`nexterm-client-gpu.exe` is intentionally excluded).

CI is configured at `.github/workflows/ci.yml` and runs on push/PR against `master`. The 3-OS matrix (Linux / macOS / Windows) runs `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check`.

Bump the version in `Cargo.toml` under `[workspace.package] version` only (not in individual crate `Cargo.toml` files). The workspace uses Rust 2024 edition (`edition = "2024"`), so Rust 1.85+ is required.

The Flatpak build (`.github/workflows/flatpak.yml`) runs on `ubuntu-latest`. Do not use a `container:` block — it disables `apt-get`. `flatpak remote-add`, `flatpak install`, and `flatpak-builder` all require the `--user` flag (CI has no system-level privileges).

The flatpak-builder sandbox is network-isolated, so cargo dependencies are vendored ahead of time into `pkg/flatpak/cargo-sources.json` and referenced from the manifest's `sources`. **Whenever `Cargo.lock` changes, run `bash scripts/regenerate-flatpak-sources.sh` to regenerate `cargo-sources.json` and commit it.** The flatpak CI runs `flatpak-cargo-generator.py` as its first step and diffs against `cargo-sources.json`; mismatches fail the job, catching missed regenerations. The build forces offline mode with `CARGO_NET_OFFLINE=true` + `cargo --offline build`.

For SSH agent authentication on russh 0.59 / 0.60, the loop variable from `request_identities()` is `&AgentIdentity`. `authenticate_publickey_with` takes an `ssh_key::PublicKey`, so call `identity.public_key().into_owned()` (russh 0.58 returned a `PublicKey` directly from `identity.clone()`, but the type changed in 0.59). There were no breaking API changes between 0.59 and 0.60 for our code. In `Cargo.toml`, set `default-features = false, features = ["ring", "rsa", "flate2"]` to avoid the `aws-lc-rs` backend so the project builds on platforms without NASM (e.g. Windows).

When passing preprocessor variables to WiX v3's `candle.exe`, use the `-dName=Value` form (no space). Calling from PowerShell as `-d "Name=Value"` splits into two arguments and yields `CNDL0289`. The correct form is `"-dVersion=$version"`.
