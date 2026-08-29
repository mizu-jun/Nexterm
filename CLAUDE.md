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

## Documentation Map & Roles

Each persistent document has one job. Write new content in the file that owns that job; do not duplicate it elsewhere.

| Document | Role | When it changes |
|---|---|---|
| `CLAUDE.md` | Working rules for Claude Code (*how to work here*) | When a rule changes; delete stale entries |
| `docs/PRODUCT.md` | Product requirements, vision, non-goals (*what & why to build*) | Reviewed at each minor/major release |
| `docs/ARCHITECTURE.md` | System design (*how it is built*) | When the structure changes |
| `docs/adr/` | Individual design decisions (*why we decided*) | Append-only; never rewrite accepted ADRs |
| `docs/plans/` | Phased work plans with status/progress (steering files for units of work) | Continuously while the work is active; move to `plans/archive/` when done |
| `CHANGELOG.md` | Release history | At each release |

## Build Commands

```bash
# Linux build dependencies (Ubuntu/Debian)
sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev

# Required for PR merge
cargo clippy -- -D warnings
cargo fmt --check

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
- `nexterm-client-core` — Shared client-side IPC implementation (Sprint 3-6). Consolidates the UDS / Windows named-pipe framing, handshake, and send/recv task management that was duplicated in `nexterm-client-gpu` / `nexterm-client-tui` `connection.rs`. Exposes `Connection`; both the GPU and TUI clients depend on it.
- `nexterm-vt` — Wrapper around the `vte` crate. VT100/ANSI parser + virtual screen (`Grid`) + Sixel/Kitty image decoding.
- `nexterm-server` — PTY server. Hierarchy: `SessionManager → Session → Window (BSP) → Pane`.
- `nexterm-config` — TOML + Lua config. Load order: defaults → `config.toml` → `config.lua`. Hot reload via the `notify` crate.
- `nexterm-client-gpu` — wgpu renderer (winit 0.30 `ApplicationHandler`). Three-pass rendering: background quads → text → images.
- `nexterm-client-tui` — TUI fallback using ratatui + crossterm.
- `nexterm-ssh` — SSH client built on russh 0.60 (upgraded for GHSA-f5v4-2wr6-hqmg pre-auth DoS; uses the `ring` backend to avoid the NASM dependency).
- `nexterm-plugin` — WASM plugin runtime on wasmi. `PLUGIN_API_VERSION = 1` identifies the stable ABI. `PluginManager::unload(path)` / `reload(path)` provide runtime unload/reload. Plugins may export `nexterm_meta` to publish name and version. The server holds it as `Arc<Mutex<Option<PluginManager>>>` on `SessionManager.plugin_manager`, and IPC commands (`ListPlugins`/`LoadPlugin`/`UnloadPlugin`/`ReloadPlugin`) operate on it.
- `nexterm-i18n` — 8-language support (en/ja/zh-CN/ko/de/fr/es/it). User-facing strings must use the `fl!` macro.

### Per-crate guidance

Crate-level internals load lazily from the crate directory you are working in:

- `nexterm-server/CLAUDE.md` — server internals (session/window/ipc/persist/web).
- `nexterm-client-gpu/CLAUDE.md` — GPU client internals (renderer, widget layer, palette, animations).

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
  - **Text colour**: never pick one without naming its ground. `DesignTokens` has no flat `text_primary` / `text_secondary` / `text_muted`; ask `tokens.text_on(SurfaceLevel::S0..S3)` for the set corrected against the surface you are drawing on (UI/UX v3 P5). Where the ground moves with state — a row that gains a `surface_3` hover fill — take the *deepest* level it can reach rather than swapping colour mid-interaction. `semantic_*` and `accent_primary` stay raw: those are fill-role tokens, and their floor is WCAG's 3:1 for non-text — if you want one of those hues *as text*, take `text_on(level).accent` / `.error` / `.success` / `.warning`, never the flat token. Where the ground is not a surface token at all (a blended danger fill, a badge), correct at draw time with `color_util::readable_on` (UI/UX v3 P5d); the old `settings/row.rs::ensure_readable` is gone.
  - **Animations**: frame-driven. There is no `prefers-reduced-motion` media query; intensity is controlled by `config.toml` instead.
  - **Strings**: every user-facing string must be added to all 8 languages via `nexterm_i18n::fl!`.
  - **Accessibility**: contrast ratio ≥ 4.5:1, keyboard-only operation must work, respect IME composition (reuse the existing `ime_preedit` path).
- Primary areas for UI/UX work: `renderer/overlay/widgets/` (any migrated settings control), `host_manager.rs`, `palette.rs`, `macro_picker.rs`, `renderer/overlay/`, `state/menus.rs`.
- **Adding a control to a migrated settings tab**: edit that tab's `settings_<tab>.rs` only — add a `row::` constant, a `WidgetDesc` arm, a layout entry, and an `apply_<tab>_action` arm. The renderer, the hit-test and the AccessKit tree pick it up automatically. Do not add geometry to `settings_panel_hit.rs` or a node to `accessibility.rs`; that is the duplication this layer exists to prevent.

## Release Flow

Release, CI and packaging mechanics (version tagging, the WiX v3 MSI, the Flatpak build, russh feature flags) live in the `release-flow` skill — invoke it when cutting a release or debugging those pipelines.

One rule fires outside releases and stays here: **whenever `Cargo.lock` changes, run `bash scripts/regenerate-flatpak-sources.sh` to regenerate `pkg/flatpak/cargo-sources.json` and commit it.** The flatpak CI diffs against that file and fails the job on a mismatch.
