# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.9.0] - 2026-06-06

MINOR release adding a complete Vim-style copy mode (D2). Users can now navigate, visually select, search, and yank terminal output using standard Vi key bindings without leaving the keyboard. PROTOCOL_VERSION = 8 and SNAPSHOT_VERSION = 4 are unchanged.

### Added

- **Vi mode in copy mode (D2)**: copy mode now behaves like a full Vim Normal / Visual / Visual-Line modal editor.
  - **Navigation**: `h`/`j`/`k`/`l` (character/line), `w`/`b` (word forward/back), `e` (word end), `^` (first non-blank of line), `G` (last row), `gg` (first row), `Ctrl-u`/`Ctrl-d` (half-page up/down).
  - **Visual selection**: `v` enters character-wise Visual mode; `V` enters line-wise Visual-Line mode. Both toggle off on a second press. `y` yanks the selection to the clipboard and exits copy mode.
  - **Search**: `/` opens a forward incremental search; `?` opens a backward search. `n`/`N` repeat the last search in the same/opposite direction. The search query is committed on `Enter`; `Escape` cancels without navigation.
  - **Rendering**: the selected region is drawn as a blue semi-transparent overlay (alpha 0.45) directly on top of pane content, bypassing the C4 vertex cache so it is always up-to-date. Visual-Line highlights full rows; Visual highlights character ranges. The cursor position is marked with a yellow block (alpha 0.60).
  - **Status bar indicator**: the right zone of the status bar shows `COPY`, `VISUAL`, or `V-LINE` in `accent_primary` colour while copy mode is active.

### Fixed

- **Pre-existing clippy lints from C4**: `append_pane_verts` and `PaneRenderCache::key_matches` were flagged for `too_many_arguments`; both now carry `#[allow(clippy::too_many_arguments)]`. The `map_or(false, …)` pattern in the cache-validity check was replaced with `is_some_and(…)`. A redundant `layout.cols as u16` cast was removed.

## [1.8.1] - 2026-06-06

PATCH release completing the DesignTokens tokenization pass started in v1.8.0. All remaining hardcoded `[f32; 4]` RGBA literals in the GPU renderer overlay modules are now replaced with `DesignTokens` field references. PROTOCOL_VERSION = 8 and SNAPSHOT_VERSION = 4 are unchanged.

### Changed

- **Key-hint overlay fully tokenized (Phase 12)**: `build_key_hint_verts` now accepts a `&DesignTokens` parameter. The banner background is derived from `surface_0` at 0.92 alpha, the accent stripe from `accent_muted`, the header and key-column text from `text_primary`, and the action-column text from `text_secondary`.
- **Command palette and picker overlays tokenized (Phase 13)**: the selected-row highlight in the command palette now uses `surface_2`, selected foreground uses `text_primary`, unselected items use `text_muted`, and the empty-list hints in the macro picker and host manager use `text_muted`. Intentional purple macro branding and green SSH-host branding are preserved.
- **Dialog overlays fully tokenized (Phase 14)**: the password-prompt input text uses `text_primary`, the "remember password" checkbox enabled/disabled states use `semantic_success` / `text_muted`, the "prefilled from keychain" label uses `semantic_info`, the context-menu separator uses `border_subtle` at 0.70 alpha, and the key-hint text in context menus uses `text_muted` at 0.80 alpha. Black semi-transparent backdrops, dark-text-on-bright-button, and Kill/Cancel semantic colours are preserved as intentional.
- **Tab bar settings button tokenized (Phase 15)**: the inactive-state colour of the `⚙` button in the tab bar now uses `text_secondary` instead of a hardcoded `[0.80, 0.80, 0.80, 1.0]`.

## [1.8.0] - 2026-06-06

MINOR release introducing a comprehensive UI/UX overhaul based on a centralised DesignTokens system. All visual chrome — overlays, tabs, pane borders, status bar, banners, and dialogs — now derives colours and radii from a single source of truth, making the renderer theme-aware and eliminating ~500 hardcoded RGBA literals. PROTOCOL_VERSION = 8 and SNAPSHOT_VERSION = 4 are retained; the wire format is unchanged from 1.7.8.

### Added

- **DesignTokens system (`nexterm-client-gpu/src/design_tokens.rs`, Phase 1)**: a centralised struct of colour and geometry constants (`surface`, `overlay`, `accent`, `border`, `text_*`, `pane_border_*`, `tab_*`, `status_bar_*`, corner radii, animation durations). All renderer code now imports `DesignTokens::default()` rather than scattering literal `[f32; 4]` arrays throughout shaders and vertex builders. Dark-mode and future light-mode themes can be swapped by returning a different `DesignTokens` instance.
- **Pill-style tab bar with hover-only close buttons (Phase 1–2)**: tabs are now drawn as rounded rectangles with a configurable pill radius. The close `×` button is invisible at rest and fades in only on tab hover, reducing visual noise when many tabs are open. The active-tab accent line expands from the centre of the pill on focus change.
- **Pane focus visualisation (Phase 3)**: the focused pane receives a coloured border drawn with `DesignTokens.pane_border_active`; inactive panes are subtly dimmed with a translucent overlay quad (`DesignTokens.pane_dim_overlay`). Both effects update instantly when focus moves.
- **Spring-physics animations (Phase 4)**: the tab accent line and the pane dim overlay now use an `ease_out_cubic` spring instead of a hard cut. Duration is driven by `DesignTokens.tab_accent_anim_ms` and `DesignTokens.pane_dim_anim_ms` so `config.animations.intensity = "off"` continues to produce instant transitions.
- **`draw_overlay_panel` unified overlay chrome helper (Phase 5)**: a single function in `vertex_util.rs` that draws the rounded background rect, drop shadow quad, and border stroke shared by every overlay (command palette, host manager, macro picker, SFTP dialog, settings panel, close-window dialog, context menu). Removing per-overlay duplication cut ~300 lines of vertex-builder code.
- **Status bar zone redesign (Phase 6)**: the status bar is divided into left (connection / session info), centre (clock / workspace name), and right (battery / CPU sample, if enabled) zones using DesignTokens spacing. Zone boundaries are drawn with a subtle separator rather than a full-height divider.
- **Update banner and incremental-search chrome tokenized (Phases 7–8)**: the amber update-available banner and the `/` incremental-search overlay bar now use DesignTokens colours and radii instead of hardcoded values, so they inherit future theme changes automatically.
- **Quick Select overlay fully tokenized (Phase 9)**: the Quick Select label overlays that appear during `Ctrl+B Q` copy mode now use DesignTokens for background, text, and highlight colours.
- **SFTP file-transfer dialog tokenized (Phase 10)**: progress bar fill, track, and border colours in the SFTP upload/download dialog replaced with DesignTokens references.

### Changed

- All remaining hardcoded `[f32; 4]` RGBA literals in `nexterm-client-gpu/src/renderer/overlay/settings.rs` replaced with DesignTokens field references (final cleanup commit after Phases 1–10).

## [1.7.8] - 2026-06-06

PATCH release that addresses the remaining P2 items from the Windows-launch investigation memo. PROTOCOL_VERSION = 8 and SNAPSHOT_VERSION = 4 are retained; the wire format is unchanged from 1.7.7.

### Added

- **Offline-mode banner (P2-1)**: while the GPU client is repeatedly failing to connect to the embedded server, a one-line amber bar at the top of the window shows "Connecting to the embedded server… ({seconds}s)" once the offline streak exceeds 1 s. Previously this state was a silent blank window — particularly visible on Windows where the `\\.\pipe\nexterm-<user>` named pipe can take >1 s to come up. The banner auto-clears as soon as the connection succeeds (no key dismissal). New i18n key `offline-banner-connecting` is translated in all 8 locales.

### Fixed

- **Restored panes silently disappearing when their saved cwd was deleted (P2-2)**: `Pane::spawn_with_cwd` now checks that the requested working directory still exists and is a directory before handing it to `portable_pty::spawn_command`. When the directory is missing (e.g. a `cargo clean` removed a `target/` subdir, or a scratch directory was deleted while the session was offline), the spawn now falls back to `$HOME` / `%USERPROFILE%` instead of surfacing `HRESULT -2147024809 (E_INVALIDARG)` on Windows ConPTY and letting the pane be dropped by the snapshot self-heal pass. A `WARN` log line is emitted so the cwd loss is still visible in diagnostics.

## [1.7.7] - 2026-06-06

PATCH release that addresses two of the three problems surfaced by the v1.7.5 diagnostic logs in `nexterm-client.log.2026-06-05`. PROTOCOL_VERSION = 8 and SNAPSHOT_VERSION = 4 are retained; the wire format is unchanged from 1.7.6. The standalone `nexterm-server` binary path is unchanged — only the single-binary GPU client uses the new entry point.

### Fixed

- **Duplicate `Watching the configuration directory` (problem 2)**: the GPU client and the embedded `nexterm-server` task each installed their own `notify::Watcher` over the same TOML directory, keeping two file-system handles open for the entire process lifetime. The client now owns the only `SharedRuntimeConfig` and the only watcher, and forwards each reload to the server's dispatch layer via `ArcSwap::store`. The embedded server skips `runtime_config::spawn_watcher` when given an external runtime config.
- **~1.6 s server starvation between `restored sessions` and `ipc::serve` (problem 3)**: the embedded server previously ran as a `tokio::task` on the same runtime that drove winit, so winit's main-thread occupation could block server-side progress for seconds at a time on lower-core machines. The server now runs on a dedicated OS thread (`std::thread::Builder::name("nexterm-server")`) with its own multi-thread Tokio runtime, fully isolated from winit's scheduling.

### Added

- **`nexterm_server::run_server_with_config_and_runtime(cfg, runtime_cfg, shutdown_rx)`**: new public entry point for embedders that own the `SharedRuntimeConfig` and want explicit shutdown control instead of relying on `tokio::task::JoinHandle::abort()`. The existing `run_server()` (standalone binary) and `run_server_with_config(cfg)` (v1.7.6 single-binary entry) are preserved.
- **`nexterm_server::{SharedRuntimeConfig, RuntimeConfig, build_shared_runtime_config}`**: re-exports of the runtime-config types so external embedders can construct and update the shared handle without depending on internal modules.

### Changed

- Single-binary `nexterm` client: the embedded server task became a dedicated OS thread with its own Tokio runtime; `server_handle.abort()` was replaced by an explicit `tokio::sync::oneshot` shutdown channel. On window close the client now sends `()` on that channel and joins the server thread so its snapshot save completes before the process exits.

## [1.7.6] - 2026-06-06

PATCH release that eliminates the duplicate TOML read on startup of the single-binary GPU client. No behavior change beyond fewer file reads and cleaner logs; PROTOCOL_VERSION = 8 and SNAPSHOT_VERSION = 4 are retained, and the wire format is unchanged from 1.7.5.

### Fixed

- **Duplicate `Loaded the TOML configuration` log (`nexterm-client.log.2026-06-05`)**: the GPU client and the embedded `nexterm-server` task each called `ConfigLoader::load()` independently, so the same TOML file was parsed twice within microseconds of each other on every startup. The client now loads the config once and hands the parsed `Config` to the embedded server via the new `run_server_with_config` entry point. The standalone `nexterm-server` binary still uses `run_server` and continues to load the file itself, so the systemd / standalone path is unchanged.

### Added

- **`nexterm_server::run_server_with_config(cfg)`**: new public entry point for embedders that have already parsed the config. The existing `run_server()` is preserved for the standalone binary and now simply loads the file and delegates to the shared inner routine.

## [1.7.5] - 2026-06-05

Diagnostic-only PATCH release. No behavior change beyond logging; PROTOCOL_VERSION = 8 and SNAPSHOT_VERSION = 4 are retained, and the wire format is unchanged from 1.7.4.

This release adds breadcrumb logging across the server startup sequence and the client reconnect loop so we can pinpoint the silent stall reported in `nexterm-client.log.2026-06-03`, where the server task vanished for ~38 s between `restored sessions` and the IPC accept loop without emitting any log line.

### Added

- **Server startup checkpoints (`nexterm-server/src/lib.rs`)**: six new `INFO` log lines covering snapshot load, self-heal check, runtime-config build, WASM plugin load, config-watcher spawn, optional web-terminal launch, and the decisive `entering ipc::serve` line right before the named-pipe / Unix-socket accept loop. The last line is what distinguishes "stalled before `ipc::serve`" from "stalled inside the accept loop".
- **Named-pipe create failure context (`nexterm-server/src/ipc/platform.rs`, Windows)**: `ServerOptions::create` failures are now logged at `error!` with the raw OS error code and the loop iteration number, so collisions on `\\.\pipe\nexterm-<USERNAME>` are no longer swallowed by `?`.
- **Client reconnect diagnostics (`nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs`)**: `try_connect` now reports the first failure of an offline streak at `INFO` with the underlying error (was `debug`, invisible in production), emits a `WARN` summary every ~5 s while still offline (attempt count + elapsed seconds + last error), and logs the total offline duration at `INFO` once the connection succeeds. The 200 ms retry cadence and overall behavior are unchanged.

### Why

P1-A of the Windows-launch investigation. The next on-device reproduction run will identify exactly which startup step is stalling, which is the prerequisite for designing P1-B (single-instance pipe coordination) against concrete evidence rather than speculation. See `memory/project_windows_powershell_startup_investigation.md` for the full failure analysis.

## [1.7.4] - 2026-06-04

PATCH release that re-syncs the Flatpak vendored-sources manifest with
`Cargo.lock`. No code changes since 1.7.3 (`PROTOCOL_VERSION = 8`,
`SNAPSHOT_VERSION = 4` retained); 1.7.3 shipped with every asset except the
Flatpak bundle, which this release restores.

### Fixed

- **Flatpak build (`pkg/flatpak/cargo-sources.json` out of sync)**: a previous
  dependency bump (`chacha20` 0.9.1 → 0.10.0, `cipher` 0.4.4 → 0.5.1) updated
  `Cargo.lock` without regenerating the vendored-sources manifest, leaving
  stale crate entries that no longer matched `Cargo.lock`. The Flatpak CI's
  sync check failed at release time (so 1.7.3 has no Flatpak asset). The
  manifest is regenerated and back in sync.

## [1.7.3] - 2026-06-04

Follow-up PATCH on top of 1.7.2. Replaces the blocking startup-connect retry
introduced in 1.7.2 with a non-blocking attempt plus background reconnect,
which addresses the *root cause* of the offline-mode race, and trims redundant
startup work and log noise observed in real session logs. No breaking changes
(`PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4` retained).

### Fixed

- **Startup connect race, fixed at the root** (`nexterm-client-gpu`): the
  1.7.2 fix retried the IPC connect for ~3 s by **blocking the winit main
  thread** (`block_in_place` + `block_on`). In the single-binary build the
  client and the embedded server share one Tokio runtime, so that block
  *starved the server task* and delayed the very named pipe the client was
  waiting for. Real logs showed the client exhausting its 3 s retry budget at
  T+3.4 s while the server only bound the pipe at T+4.3 s — falling into
  offline mode even though the server came up healthy ~0.9 s later.
  `on_resumed` now makes a single non-blocking connect attempt and, on failure,
  `on_about_to_wait` reconnects on a 200 ms cadence until the server is
  listening. The main thread stays responsive and the server gets the CPU it
  needs to bind the pipe promptly.

### Changed

- **Server loads its config once at startup** (`nexterm-server`): `run_server`
  called `ConfigLoader::load()` twice (once for the shell, again for hooks /
  web / plugins), doubling startup file IO and emitting duplicate "Loaded the
  TOML configuration" log lines. It now loads once and reuses the result.
- **Font system is no longer scanned twice at startup** (`nexterm-client-gpu`):
  the font manager was built once at scale 1.0 in `NextermApp::new` and then
  fully rebuilt in `on_resumed` to apply the real DPI scale, triggering a
  second ~30–50 MB system-font scan (and a duplicate "malformed font" warning
  for fonts like Windows' `mstmc.ttf`). A new `FontManager::set_scale_factor`
  reuses the existing font system and only recomputes scale-dependent metrics.
- **Quieter shutdown / font logs**: the server's "config watcher channel
  closed" message is now `debug!` (it fires on every clean shutdown, so a
  `warn!` was misleading), and the default client log filter adds
  `fontdb=error` to silence unactionable malformed-font warnings from
  third-party system fonts.

## [1.7.2] - 2026-06-01

Follow-up PATCH on top of 1.7.1, fixing a startup race that could leave
the client in offline mode and silencing a second per-frame WARN flood.
No breaking changes (`PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4`
retained).

### Fixed

- **IPC connect race on startup** (`nexterm-client-gpu`): in the
  single-binary build the GPU client and the embedded server task race at
  launch. If the server's snapshot-load + IPC-listen took longer than the
  client's first `connect` attempt (observed: ~943 ms on a real session),
  the connect returned `os error 2` (Windows: file not found — the named
  pipe did not exist yet) and the client fell into offline mode
  **permanently**, with no further reconnect attempts. The window stayed
  blank and unresponsive until the user force-closed it.
  The connect path now retries up to 15 times on a 200 ms cadence (≈3 s
  total budget). The retried path emits a `debug!` line per attempt and
  only escalates to `warn!` if all attempts fail.

### Changed

- **Default log filter targets `wgpu_hal::vulkan::conv=error`**
  (`nexterm-client-gpu`): newer NVIDIA Vulkan drivers advertise
  `VK_PRESENT_MODE_FIFO_LATEST_READY_EXT` (id `1000361000`), which
  current wgpu does not recognize and which it emits as a WARN every
  single frame (≈30 Hz). The directive added in 1.7.1 only suppressed
  INFO from `wgpu_hal`, so this WARN flood survived and now drowned out
  the cleaner log. The new targeted `=error` directive silences only the
  `wgpu_hal::vulkan::conv` module; other `wgpu_hal` WARNs continue to
  surface. Setting `NEXTERM_LOG` explicitly continues to override the
  default exactly as before.

## [1.7.1] - 2026-05-31

Diagnostic and resilience PATCH release. No breaking changes
(`PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4` retained).

### Fixed

- **Snapshot self-heal on startup** (`nexterm-server`): when one or more
  windows or sessions fail to restore from the persisted snapshot (e.g.
  ConPTY returns `E_INVALIDARG` because the saved cwd no longer exists),
  the snapshot file is now rewritten immediately during startup so the
  broken entries are not retried on every subsequent launch. Previously a
  short-lived session that ended before the 30-second auto-save tick fired
  could leave a broken entry stuck in the snapshot indefinitely.
- **Detailed context on ConPTY failures** (`nexterm-server`): `openpty` and
  `spawn_command` errors now carry `cols` / `rows` / `shell` / `args` /
  `cwd` in the error chain, and `Session::restore_from_snapshot` prints
  the full chain via `{:#}`. A previously opaque
  `failed to create psuedo console: HRESULT -2147024809` now reads e.g.
  `openpty failed (cols=0, rows=0, shell="powershell.exe"); ConPTY on
  Windows rejects size 0 with E_INVALIDARG (HRESULT 0x80070057)`.

### Changed

- **Default logging directives** (`nexterm-client-gpu`): when `NEXTERM_LOG`
  is unset, the GPU client now applies
  `info,wgpu_core=warn,wgpu_hal=warn,naga=warn` instead of plain `info`.
  This silences the per-frame
  `Device::maintain: waiting for submission index N` INFO that
  `wgpu_core::device::resource` emits at roughly 60 Hz, which previously
  bloated `nexterm-client.log` past 1 MB for a 4-minute session and drowned
  out useful diagnostics. Setting `NEXTERM_LOG` explicitly continues to
  override the default exactly as before.

## [1.7.0] - 2026-05-31

Sprint 5-11-9 (Keybindings interactive editor + screen-reader support) and
Sprint 5-12 (Windows shell-launch visibility fixes) shipped together. No
breaking changes (`PROTOCOL_VERSION = 8` and `SNAPSHOT_VERSION = 4`
retained).

### Added (Sprint 5-11-9 — Keybindings interactive editor)

- **Sub-phase A — keybinding entry data + display**: introduced
  `KeyBindingEntry { key, action }` plus the `Keybindings` settings category.
  The settings panel renders the loaded bindings (and any built-in defaults)
  as a scrollable list, with `→` separating the key spelling from the action
  name. `KEYBINDING_ACTIONS` enumerates the 27 supported actions
  (`Quit` / `CommandPalette` / `CloseOsWindow` / …) — anything outside the
  list is flagged as invalid.
- **Sub-phase B — edit logic**: `KeyEditMode::Record` (capture the next key
  press) and `KeyEditMode::Text(TextInputState)` (free-form spelling) drive
  in-place editing. `begin_key_record` / `begin_key_text_edit` /
  `capture_key_record` / `commit_key_edit` / `cancel_key_edit` form the
  state machine. `cycle_keybinding_action_forward/backward` cycles through
  `KEYBINDING_ACTIONS`.
- **Sub-phase C — Add / Delete buttons**: `add_key_binding` appends a fresh
  entry and immediately enters Record mode. `open_key_delete_dialog` /
  `cancel_key_delete_dialog` / `confirm_key_delete_dialog` /
  `toggle_key_delete_dialog_focus` drive the delete confirmation dialog
  (Cancel is focused by default to prevent accidental deletion).
- **Sub-phase D — settings-panel UI**: 5-row Keybindings section in
  `renderer/overlay/settings.rs` showing the binding list, the selected key
  field (with Record indicator), the action ComboBox, and Add / Delete
  buttons. Navigation: `↑/↓` cycles `key_field_focus` (0=List, 1=Key,
  2=Action, 3=Add, 4=Delete); `←/→` cycles the action ComboBox or moves
  between dialog buttons; `Enter` activates the current focus; `Esc`
  cancels in-flight edits and closes the delete dialog.
- **Sub-phase E — AccessKit nodes + dispatch + tests**: screen-reader
  exposure of the entire Keybindings editor.
  - **NodeId allocation**: fixed `50..=56` for Key field / Action field /
    Add / Delete buttons / delete-dialog (`AlertDialog` + Confirm + Cancel);
    dynamic offset `900_000_000` for `SettingsKeyBindingItem` (one
    `ListBoxOption` per binding).
  - **`build_settings_panel_nodes` Keybindings branch**: surfaces each
    binding as a `ListBoxOption`, the selected binding's key as a
    `TextInput` (description swaps to a "Recording…" hint while in Record
    mode and exposes the live edit buffer while in Text mode), and the
    action as a `ComboBox`. Add / Delete are exposed as `Button`s; Delete
    becomes a labelled "(disabled)" button when the list is empty so SR
    navigation stays consistent.
  - **`dispatch_settings_action`** gains 12 arms. Per the design decision
    "Q1 = (c) both", `Action::Click` on the key field starts Record mode
    **and** `Action::SetValue` writes the spelling directly via
    `set_keybinding_key_direct`. The action `ComboBox` accepts
    `Click` / `Increment` / `Decrement` cycling plus `SetValue` (rejected
    for strings outside `KEYBINDING_ACTIONS`).
  - **`compute_tree_state_hash`** now reflects the keybinding list, the
    selected index, `key_field_focus`, the delete-dialog state, and the
    `KeyEditMode` (including the in-flight `TextInputState.buffer` /
    `cursor` / `preedit`) so the SR sees every change live.
  - **25 new unit tests**: 7 decode tests (fixed + dynamic offsets), 11
    dispatch tests (each Action × node combination, including Q1=(c)
    Click→Record + SetValue→direct write regression), 4 build-tree tests
    (focus selection, empty-list behaviour, delete-dialog body, focus
    follows `key_field_focus`), 1 hash detection test, 2 sanity tests
    (offset isolation, Click+SetValue lands clean).

### Added (Sprint 5-12 — Shell-launch visibility)

- **Server error banner UI (Sprint 5-12 Phase 1)** — `ServerToClient::Error` used to
  be log-only. It is now stored in `ClientState.error_banner: Option<String>` and
  rendered at the top of the screen as a red banner (deep-red background with a
  bright-red accent). Failures like PTY spawn errors (PowerShell not found, etc.),
  pane-split failures, and config-load errors are now visible directly on screen,
  so users no longer need `NEXTERM_LOG=debug` to diagnose them. Dismissed with
  `Esc` (handled before `update_banner`). Stacks vertically alongside
  `update_banner` without conflict.
- **i18n key `error-banner-prefix`** — added the "Error:" prefix string to all 8
  languages (en/ja/zh-CN/ko/de/fr/es/it).
- **Startup warning queue (Sprint 5-12 Phase 4)** — added
  `startup_warnings: Arc<Mutex<Vec<String>>>` plus `set_startup_warnings` /
  `take_startup_warnings` methods to `SessionManager`. When `run_server` fails
  `ConfigLoader::load()`, instead of silently using defaults, it queues warning
  messages and emits them as `ServerToClient::Error` (semicolon-joined) on the
  first attach. Surfaces through the Phase 1 error-banner machinery.
- **Lua `shell.args` merge support (Sprint 5-12 Phase 3)** — extended
  `apply_lua_table_to_config` so `shell.args` can be overridden from a Lua table.
  Example: `shell = { program = "pwsh.exe", args = {"-NoLogo", "-NonInteractive"} }`.
  Omitting `shell.args` preserves the existing value (from TOML or
  `ShellConfig::default()`); an explicitly empty table is also preserved, so users
  cannot accidentally wipe all args.

### Fixed

- **Windows version-comparison bug that hid PowerShell 10
  (Sprint 5-12 Phase 2)** — when `ShellConfig::default()` scanned
  `%ProgramFiles%\PowerShell\*` on Windows, it compared `PathBuf` values with `>`,
  which falls back to lexicographic order. That made `"7" > "10"`, so even if
  PowerShell 10 was installed, version 7 was chosen. Introduced a new
  `pwsh_version_number()` helper that parses the directory name as `u32` and
  compares numerically. Preview directories (`7-preview`, etc.) fall back to 0
  on parse failure, ensuring numeric versions always win.
- **Lua config `shell.args` was being silently dropped
  (Sprint 5-12 Phase 3)** — the previous `apply_lua_table_to_config`
  overrode only `shell.program` and discarded `shell.args`. Users who wrote
  `shell = { program = "...", args = {...} }` in Lua kept the default args
  (`["-NoLogo"]` for PowerShell), contrary to expectation.
- **Config-load failures were being swallowed (Sprint 5-12 Phase 4)** —
  `nexterm_config::ConfigLoader::load().unwrap_or_default()` discarded `Err`
  completely, so even a corrupted `nexterm.toml` started the server on defaults
  without telling the user. Replaced with a `match`: errors are now surfaced in
  three places — `tracing::error!`, the startup-warning queue, and the client
  error banner.

### Changed

- `nexterm-client-gpu/src/state/server_message.rs::ServerToClient::Error`
  handler changed from "log only" to "log + set `state.error_banner`".
- `nexterm-client-gpu/src/renderer/render_frame.rs` now draws the error banner
  immediately after the `update_banner` block. The two banners stack vertically.
- `nexterm-client-gpu/src/renderer/input_handler/mod.rs::on_key` now handles
  `Esc` while `error_banner.is_some()` before the `update_banner` early-return,
  so the most recent (error) banner is closed first when both overlap.

### Tests Added

- New `#[cfg(all(test, windows))] mod tests` in
  `nexterm-config/src/schema/shell.rs` with 4 regression tests for the version
  comparison bug (v7 < v10 / preview is 0 / no parent is 0 / numeric beats
  non-numeric).
- 3 tests in `nexterm-config/src/loader.rs` for Lua `shell.args` merging
  (override succeeds / preserved on omit / preserved on empty table).

### Compatibility

- No functional breaking changes (`PROTOCOL_VERSION` 8 / `SNAPSHOT_VERSION` 4
  retained).
- `SessionManager::new` signature unchanged (the 12 existing call sites are
  unaffected).
- No config-schema changes (the `[shell]` section is fully compatible).
- No changes to the `ServerToClient` IPC type (we only added a receiver-side UI
  for the existing `Error` variant; the server-side contract is unchanged).
- Existing server binaries inter-operate with the new client (older servers
  don't populate `startup_warnings`, but the client-side error banner still
  surfaces PTY spawn failures and similar).

### Verification (live on Windows)

```powershell
# 1. Enable debug logging at startup
$env:NEXTERM_LOG = "debug"
nexterm 2> $env:USERPROFILE\nexterm-debug.log

# 2. Confirm PowerShell 10 is detected in the log
Select-String -Path $env:USERPROFILE\nexterm-debug.log -Pattern "pwsh|powershell"

# 3. Verify a PTY spawn failure shows the red banner at the top of the screen
```

## [1.6.1] - 2026-05-24

Hotfix release for Flatpak distribution. As of v1.6.0, the new dependencies
added in Sprint 5-11-1 (AccessKit PoC) — `accesskit` / `accesskit_winit` and
friends — had not been propagated to `pkg/flatpak/cargo-sources.json`. The
Flatpak workflow failed its Cargo.lock consistency check almost immediately
(17 s). v1.6.1 regenerates `cargo-sources.json` (+234 lines) and ships the
full asset set including Flatpak.

### Fixed

- **Flatpak build failure resolved**: re-aligned
  `pkg/flatpak/cargo-sources.json` with `Cargo.lock` and added the AccessKit
  family of dependencies (`accesskit-0.24.0`, `accesskit_atspi_common-0.18.1`,
  `accesskit_consumer-0.36.0`, `accesskit_ios-0.1.0`, `accesskit_macos-0.26.1`,
  and many more) to the vendored sources. The CI consistency check (diff
  against the generator output) passes again and the Flatpak bundle ships.

### Changed

- Bumped version `1.6.0` → `1.6.1` (workspace.package.version).

### Compatibility

- No functional breaking changes (`PROTOCOL_VERSION` 8 / `SNAPSHOT_VERSION` 4
  retained).
- macOS / Linux / Windows binary distributions are identical to v1.6.0. Only
  Flatpak catches up.

### Known Constraints

- Functionally identical to v1.6.0. Users who already obtained the macOS /
  Linux / Windows binaries for v1.6.0 do not need to upgrade. Pick v1.6.1 only
  if you install via Flatpak.

## [1.6.0] - 2026-05-24

Completes every phase of Sprint 5-11 (5-11-1 through 5-11-8). Delivers the last
remaining HIGH item from audit round 2 — **screen-reader support (H1)** — fully
implemented with AccessKit 0.24 + accesskit_winit 0.33, alongside terminal grid
diff notifications, cursor TextSelection, Bell / OSC notifications as
`Role::Alert`, ActionRequest write-response, and SSH host GUI editing, all
landing together.

### Added — Sprint 5-11-1 / 5-11-2 (Foundation + Tree Construction)

- **Expose the accessibility tree via AccessKit** — integrated
  `accesskit_winit::Adapter` with the primary OS window and each OS window
  spawned by Phase 4. Publishes the tree to screen readers via Windows UI
  Automation, macOS NSAccessibility, and Linux AT-SPI.
- **Dynamic tree generation** —
  `accessibility::build_tree_from_state(&ClientState)` rebuilds the
  `TreeUpdate` from `ClientState` every frame (reflecting tabs, panes, titles,
  and cwd). Honors `tab_order` and finalises the NodeId scheme:
  - Fixed NodeId 1–15 (Root / TabBar / PaneArea / 6 overlays / 4 dialog buttons)
  - Dynamic offsets: Palette item 100M / Host item 200M / Macro item 300M /
    Context item 400M
  - Tab `1_000_000_000 + pane_id` / Pane `10_000_000_000 + pane_id`
    (collision-free; covered by unit tests)
- **6 overlays converted to nodes** — CommandPalette (Dialog + SearchInput +
  ListBox) / ContextMenu (Menu + MenuItem) / CloseWindowDialog (AlertDialog +
  Kill / Cancel buttons) / HostManager / MacroPicker / SettingsPanel
  (minimal; field expansion lands in Phase 5-11-6). Priority-based modal
  control (CloseDialog > ContextMenu > Palette > others).
- **Live tree updates (100 ms throttle)** —
  `compute_tree_state_hash(&ClientState)` hashes the content and
  `on_about_to_wait` compares against the previous hash. When it changes,
  `Adapter::update_if_active` is called under a 100 ms throttle to push the
  diff to the screen reader.
- **ActionRequested handling (read-side operations)** — implements the path
  where a screen reader's Focus / Click / SetValue request actually makes
  Nexterm move. `decode_node_id(NodeId) -> NodeIdKind` performs the reverse
  lookup; `handle_accesskit_action` dispatches:
  - Tab / Pane Focus or Click → `FocusPane` IPC + update of
    `state.focused_pane_id`
  - CloseDialog Kill / Cancel button → reuses the existing
    `selected_button` half-open contract (0xFE = Kill confirm /
    0xFF = Cancel confirm / 0,1 = focus only)
  - ContextMenu / Palette item Click → reuses existing handlers
    (`execute_action` / `execute_context_menu_action`)
  - PaletteSearch SetValue → updates query string and resets `selected = 0`

### Added — Sprint 5-11-3 (Terminal Grid Diff Notification, the heart of H1)

- **Screen-reader support for the terminal proper** — each grid row is
  published as an `accesskit::Role::TextRun` node. NodeId scheme extended:
  PaneRow = `20G + pane_id*1000 + row` (per-pane partitioning, collision-free).
- **Row-text API**: `pane_row_text(grid, row)` strips SGR escapes,
  `trim_end_matches(' ')` removes trailing whitespace, and fully blank rows
  become `" "` so the screen reader preserves a row boundary.
- **Only the focused pane runs at `Live::Polite`** — other panes and the
  scrollback are kept at `Live::Off` to avoid over-announcement.
- **Per-row hash diffing** — `compute_grid_row_hashes` hashes each row with
  `DefaultHasher`; `update_accesskit_tree_if_needed` is widened to detect
  "tree-shape change OR grid change" independently.

### Added — Sprint 5-11-4 (Cursor TextSelection + Scrollback)

- **Row nodes become `Role::TextRun` with `set_character_lengths`** — the
  UTF-8 byte-length array lets CJK cells carry correct boundaries.
- **TextSelection integration** — the cursor row of the focused pane is
  represented as a caret via `TextPosition { node, character_index = cursor_col }`
  (anchor == focus).
- **`Live::Polite` narrowed** — from every visible row to only the cursor row.
- **Scrollback exposed** — `SCROLLBACK_WINDOW_RADIUS = 100` rows around
  `scroll_offset` slide through a window (`Live::Off`). NodeIds are
  continuous via `pane_scrollback_row_node_id`: viewport (0–999) /
  scrollback (1000–9999).
- **New pure functions**: `pane_row_text_with_lengths` /
  `scrollback_row_text_with_lengths` / `cursor_character_index`.

### Added — Sprint 5-11-5 (Bell / OSC Notifications → Role::Alert)

- **`ClientState.alerts: VecDeque<AlertEntry>`** — TTL 5 s, max 16 entries,
  managed by `add_alert` / `expire_alerts`.
- **`Live::Assertive` region container** — each `Role::Alert` sits under
  `ALERT_REGION_ID = NodeId(26)` (separated from the pane-row range via
  `NODE_ID_ALERT_OFFSET = 50T`).
- **Bell (`\x07`) and OSC 9 / OSC 777 notifications** — two kinds,
  `AlertKind::Bell` and `AlertKind::Notification`, that the screen reader can
  announce immediately. Added to the SR region first regardless of consent
  settings (to prevent false suppression).

### Added — Sprint 5-11-6 (ActionRequest Write-Response + Window Category Complete)

- **Four core write-responses**:
  - HostItem Click → `connect_ssh_host_new_tab` (SSH connect in a new tab)
  - MacroItem Click → `RunMacro` IPC (run macro immediately)
  - Alert Click → `dismiss_alert(seq)` (immediate dismissal, bypassing TTL)
  - PaneArea ScrollUp / ScrollDown → `scroll_up/down_focused_pane`
- **SettingsPanel Window category — 4 fields completed**: `cursor_style` /
  `padding_x` / `padding_y` / `present_mode` write back to `[window]` /
  `[gpu]` / top-level via `toml_edit`. NodeId 36–39 assigned to
  `SettingsCursorStyle/PaddingX/PaddingY/PresentMode` (SR actions on
  SpinButton / ComboBox / Slider are all wired up).
- **Window-category UI expansion** — 5-row inline edit (↑/↓ selects a field,
  ←/→ changes the value, with a highlight rect + a mini-slider).

### Added — Sprint 5-11-7 (PTY Input Buffer + Profiles ListBox)

- **PTY input buffer (NodeId 27, fixed)** — a single `Role::TextInput` node
  exposed under `PANE_AREA_ID`. `Action::SetValue` routes through
  `ClientToServer::PasteText` IPC into the PTY. This is a workaround because
  AccessKit 0.24 does not standardise `Role::Terminal` SetValue; line feeds
  (`\n`) are transferred as-is.
- **Profiles category** — `SettingsPanel.profiles` exposed as
  `Role::ListBoxOption`. Click or Focus updates `selected_profile`.
  NodeId offset `600_000_000`.
- **Better descriptions on the Ssh / Keybindings categories** — replaced the
  "not implemented" placeholder with guidance to edit TOML directly
  (`[[hosts]]` / `[[keys]]`).

### Added — Sprint 5-11-8 Step 8-1 / 8-2 / 8-3 (SSH Host GUI Editing + a11y)

- **Step 8-1 (read-only ListBox)** — Settings panel → SSH category exposes
  `[[hosts]]` as a ListBox. `SshHostEntry.label()` produces a single readable
  row label.
- **Step 8-2 (Field SR editing)** — name / host / port / username / auth_type
  exposed as `Role::TextInput` / `Role::SpinButton` / `Role::ComboBox`. They
  accept direct edits from the screen reader via `set_value`.
- **Step 8-3 Sub-phase A (Inline GUI editing)** — `TextInputState` struct
  (cursor + UTF-8 boundary aware + preedit support). Pressing Enter on name /
  host / username enters edit mode. Key bindings: character input /
  Backspace / ←→ / Home / End / Delete.
- **Step 8-3 Sub-phase B (IME preedit routing)** — routes CJK IME composition
  text correctly into SSH fields. `set_ime_cursor_area` also tracks the IME
  window position.
- **Step 8-3 Sub-phase C (SpinButton / ComboBox visual editing)**:
  - port: `←` / `→` step by 1 (clamped to 1–65535)
  - auth_type: `←` / `→` cycle `password` / `key` / `agent`
- **Step 8-3 Sub-phase D (Add / Delete + delete confirmation dialog)**:
  - NodeId 45 = SettingsSshAddBtn / 46 = SettingsSshDeleteBtn
  - NodeId 47 = SettingsSshDeleteDialog (`Role::AlertDialog`, modal)
  - NodeId 48 = SettingsSshDeleteConfirmBtn / 49 =
    SettingsSshDeleteCancelBtn (Cancel is the default focus to prevent
    accidental deletion)
  - The post-deletion selection clamps to **n** (tail → n-1 / middle → same
    index / empty → focus = 0)
  - `ssh_field_focus` extended to 0..=7. While the dialog is open, Enter /
    Esc / ←→ / Tab are handled first (input_handler patch).
- **Step 8-3 Sub-phase E** — added 25 unit tests (6 for TextInputState
  UTF-8 boundaries + 3 for `ssh_field_edit` lifecycle + 7 for
  Add/Delete/dialog + 6 for dispatch + 1 for `tree_state_hash` detection +
  Sub-phase C smoke).

### Changed

- **Promoted README.md's `Screen reader support` from "experimental" to a
  full implementation notice.**

### Compatibility

- **No breaking changes**. `PROTOCOL_VERSION` 8 / `SNAPSHOT_VERSION` 4
  retained. Existing config files and snapshots keep working.
- New dependencies: `accesskit = "0.24"` / `accesskit_winit = "0.33"`
  (workspace dependencies).
- Added a `UserEvent::Accessibility(accesskit_winit::Event)` variant. Because
  `accesskit_winit::Event` is not `Clone`, removed `#[derive(Clone)]` from
  `UserEvent` as a whole (a grep confirmed no callers needed cloning).
- AccessKit 0.24 does not provide `Action::Default`, so every write-side
  response is implemented through `Action::Click` only.

### Known Constraints

- **Real-device SR verification** — verification with NVDA / VoiceOver / Orca
  on actual hardware is still pending. The Action paths cannot be validated
  with local unit tests alone.
- **The TUI client is out of scope** — `nexterm-client-tui` has no GUI
  editing or screen-reader support; edit `[[hosts]]` in `config.toml`
  directly as before.

### Verification

- `cargo test --workspace`: all **880+ tests pass** (339 in the `nexterm` bin
  plus the rest of the workspace)
- `cargo clippy --workspace --all-targets -- -D warnings`: green
- `cargo fmt --check`: clean

## [1.5.1] - 2026-05-19

Completes Sprint 5-10 Phase 4-7. A PATCH release that lands the
**Windows `has_foreground_process` implementation** that v1.5.0 had to leave
as a "Known Issue". With this in place, the `close_action = "prompt"`
confirmation dialog now behaves symmetrically across all three OSes
(Linux / macOS / Windows).

### Added — Sprint 5-10 Phase 4-7

- **Windows `has_foreground_process` implementation** — enumerates processes
  via `CreateToolhelp32Snapshot` + `Process32FirstW/NextW` and decides a
  foreground process is running if any process has the shell PID
  (`Pane.pid`) as its parent. As a result, Windows now also fires the
  confirmation dialog with `close_action = "prompt"` whenever ssh, vim, or
  any long-running job is active.
  - `HandleGuard` auto-calls `CloseHandle` to prevent handle leaks
  - All 4 `unsafe` blocks carry SAFETY comments
  - The detection logic is logically equivalent to the macOS implementation
    (built on `ps -A`), so the false-positive pattern (a shell with
    background jobs still returns `true`) is the same — and safe.

### Compatibility

- **No breaking changes**. `PROTOCOL_VERSION` 8 / `SNAPSHOT_VERSION` 4 retained.
- New dependency: `windows-sys = "0.59"` added to the Windows target of
  `nexterm-server`. Already used at the same version in `nexterm-client-gpu`,
  so Cargo.lock gets no new package and `pkg/flatpak/cargo-sources.json` is
  unchanged.

### Verification

- `cargo test --workspace`: **689 pass + 5 ignored**
- `cargo clippy --workspace --all-targets -- -D warnings`: green
- `cargo fmt --check`: clean

## [1.5.0] - 2026-05-19

The stable release of Sprint 5-8 / 5-9 Phase 4 — "tab tearing" (drag-out tab).
Contains the v1.5.0-beta.1 content plus the three items deferred to
Phase 4-6.

### Breaking Changes (CRITICAL — must read)

- **`PROTOCOL_VERSION` 7 → 8**: with the addition of
  `ClientToServer::MovePaneToWindow`, older clients (up to v1.4.0) and the
  new server are incompatible at the Hello handshake. Client and server must
  be upgraded together.
- **`SNAPSHOT_VERSION` 3 → 4**: added
  `ServerSnapshot.client_os_windows: Vec<OsWindowSnapshot>`. v3 → v4 is
  auto-migrated through `#[serde(default)]`, so existing users need to do
  nothing.
- See the
  [v1.4.0 → v1.5.0 section in `docs/MIGRATION.md`](docs/MIGRATION.md)
  for details.

### Added — Tab tearing (Sprint 5-8 Phase 4-1 through 4-4)

- **Drag-and-drop a tab to another OS window** to split it off into a new
  window (X11 / macOS / Windows)
  - Design holds multiple winit native windows in a single process
    (`EventLoopProxy` + `UserEvent`)
  - `Window::insert_pane_at` / `Window::into_single_pane` /
    `Session::move_pane` implemented
  - If the source window becomes empty it is removed automatically
- **Merge into another OS window** — drop the dragged tab onto another
  window's tab bar to merge them
- **`ClientToServer::MovePaneToWindow { pane_id, target_window_id, insert_at }`**
  IPC added
- **`window.close_action` setting added** — three values
  (`prompt` / `detach` / `kill`) control what happens on OS-window close

### Added — Sprint 5-9 Phase 4-5

- **`QueryForegroundProcess` IPC** + **`ForegroundProcessStatus` response**
  (compatible PROTOCOL v8 additions; no discriminant impact thanks to
  appending at the end of the enum)
- **State management for the OS-window close dialog** — with
  `close_action = "prompt"` (default), the dialog fires whenever a non-shell
  foreground process is running.
- **Three Wayland-fallback UX paths** — Wayland's security model hides global
  coordinates, so drag-out detection cannot work. Alternative paths:
  - Context menu: right-click on a pane → "Detach to new window"
  - Hotkey: `Ctrl+B D` (leader + D) splits the current tab into a new OS window
  - Command palette: `Ctrl+Shift+P` → "Detach to New Window"
- **`Ctrl+B W`** (leader + W) closes only the current OS window
- **`OsWindowSnapshot`** — persists multi-OS-window layout (position, size,
  and the set of server window IDs that belong to it)
- **i18n in all 8 languages** — 9 tab-tearing strings added to
  en / ja / zh-CN / ko / de / fr / es / it

### Added — Sprint 5-9 Phase 4-6 (v1.5.0-beta.1 → v1.5.0 delta)

- **Tab hover `[↗]` button** — hovering a tab shows an `↗` icon at the right
  edge; clicking it detaches the tab into a new OS window. Lets Wayland
  users perform tab tearing entirely through the GUI (ui_verts drawing +
  hit-testing + the DetachToNewWindow route)
- **Renderer drawing + keyboard handling for the confirmation dialog** —
  `build_close_window_dialog_verts` paints a red-accent warning dialog.
  `Enter` / `Y` confirm the focused button; `Esc` / `N` cancel;
  `←` / `→` / `Tab` change focus.
- **macOS `has_foreground_process` implementation** — uses
  `ps -A -o pid=,ppid=` to find children whose parent is the shell PID.
  Reliably detects ssh, vim, and other long-running jobs so the confirm
  dialog fires.

### Changed

- The `Prompt` branch of `on_close_requested` is now real (up through
  Phase 4-4 it had been degraded to behave like Kill).
- The TUI client doesn't support tab tearing; it ignores the new IPC
  variants as no-ops.

### Known Issues / Items Deferred to Phase 4-7

- **Windows `has_foreground_process` implementation**: currently hard-coded
  to false (the practical effect is the same as the Kill path). Planned for
  v1.5.1 PATCH or v1.6.0, using the `windows-sys` crate and
  `Toolhelp32Snapshot`.

### Verification

- `cargo test --workspace`: **689 tests pass**
- `cargo clippy --workspace --all-targets -- -D warnings`: green
- `cargo fmt --check`: clean
- 3-OS matrix CI (Linux / macOS / Windows): equivalent to v1.5.0-beta.1

## [1.5.0-beta.1] - 2026-05-19

A prerelease of Sprint 5-8 / 5-9 Phase 4 — "tab tearing" (drag-out tab).
Shipped to validate the IPC / SNAPSHOT breaking-change impact ahead of the
v1.5.0 stable release. **Not recommended for production use.**

### Breaking Changes (CRITICAL — must read)

- **`PROTOCOL_VERSION` 7 → 8**: with the addition of
  `ClientToServer::MovePaneToWindow`, older clients (up to v1.4.0) and the
  new server are incompatible at the Hello handshake. Client and server must
  be upgraded together.
- **`SNAPSHOT_VERSION` 3 → 4**: added
  `ServerSnapshot.client_os_windows: Vec<OsWindowSnapshot>`. v3 → v4 is
  auto-migrated through `#[serde(default)]`, so existing users need to do
  nothing.
- See the
  [v1.4.0 → v1.5.0 section in `docs/MIGRATION.md`](docs/MIGRATION.md)
  for details.

### Added — Tab tearing (Sprint 5-8 Phase 4-1 through 4-4)

- **Drag-and-drop a tab to another OS window** to split it off into a new
  window (X11 / macOS / Windows)
  - Design holds multiple winit native windows in a single process
    (`EventLoopProxy` + `UserEvent`)
  - `Window::insert_pane_at` / `Window::into_single_pane` /
    `Session::move_pane` implemented
  - If the source window becomes empty it is removed automatically
- **Merge into another OS window** — drop the dragged tab onto another
  window's tab bar to merge them
- **`ClientToServer::MovePaneToWindow { pane_id, target_window_id, insert_at }`**
  IPC added
- **`window.close_action` setting added** — three values
  (`prompt` / `detach` / `kill`) control what happens on OS-window close

### Added — Phase 4-5 (first appearance in this beta)

- **`QueryForegroundProcess` IPC** + **`ForegroundProcessStatus` response**
  (compatible PROTOCOL v8 additions)
- **OS-window close confirmation dialog** — when `close_action = "prompt"`
  (default) and a non-shell foreground process is running, a confirmation
  dialog appears (state management only at this point; the visual polish
  arrives in Phase 4-6).
- **Three Wayland-fallback UX paths** — Wayland's security model hides global
  coordinates, so drag-out detection cannot work. Alternative paths:
  - Context menu: right-click on a pane → "Detach to new window"
  - Hotkey: `Ctrl+B D` (leader + D) splits the current tab into a new OS window
  - Command palette: `Ctrl+Shift+P` → "Detach to New Window"
  - **The tab-hover `[↗]` button is not shipped in this beta (deferred to
    Phase 4-6).**
- **`Ctrl+B W`** (leader + W) closes only the current OS window
- **`OsWindowSnapshot`** — persists multi-OS-window layout (position, size,
  and the set of server window IDs that belong to it)
- **i18n in all 8 languages** — 9 tab-tearing strings added to
  en / ja / zh-CN / ko / de / fr / es / it

### Changed

- The `Prompt` branch of `on_close_requested` is now real (up through
  Phase 4-4 it had been degraded to behave like Kill).
- The TUI client doesn't support tab tearing; it ignores the new IPC
  variants as no-ops.

### Fixed

None (this release is new features only).

### Known Issues / Items Deferred to Phase 4-6

- **Tab hover `[↗]` button**: the hover vertex-add in `ui_verts.rs` on the
  renderer side isn't implemented yet. Wayland users should rely on the
  three alternative paths above.
- **Renderer drawing of the confirmation dialog**: state management only;
  Phase 4-6 fills in the visual button rendering.
- **macOS / Windows `has_foreground_process` implementation**: Linux only
  for now (`/proc/{pid}/stat` tpgid comparison). macOS / Windows return
  false (the practical effect is the same as the Kill path).

### Verification

- `cargo test --workspace`: **689 tests pass** (+3 over v1.4.0's 686 thanks
  to snapshot v4)
- `cargo clippy --workspace --all-targets -- -D warnings`: green
- `cargo fmt --check`: clean
- 3-OS matrix CI (Linux / macOS / Windows): equivalent to v1.4.0

### CI Workflow Changes

- `release.yml`: added `v[0-9]+.[0-9]+.[0-9]+-*` (prerelease) to the tag
  pattern
- The `prerelease` flag is now derived automatically based on whether the
  tag contains `-`
- The Windows MSI build strips the `-beta.1` suffix from the WiX Version
  (WiX v3 constraint)

## [1.4.0] - 2026-05-17

The minor release following the v1.3.1 hotfix. **No user-visible breaking
changes** (the `nexterm` command behaviour, the distribution layout, MSI
shortcut, etc. are all preserved). The internal cleanup that removed the
`nexterm-launcher` crate (workspace shrinks from 12 to 11 crates) is the
reason this is a MINOR rather than a PATCH. Also bundles a fix for a prefix
keybinding mis-fire bug (real user impact).

### Fixed

- **Fixed prefix-keybinding mis-fires** (`config_key_matches`): the old
  implementation evaluated only the last token via `split_whitespace().last()`,
  so configurations like
  `keys = [{ key = "<leader> d", action = "ClosePane" }]` matched a bare
  `d` press and fired the action immediately (`"<leader> %"` similarly
  fired on bare `5`). Changed `config_key_matches` to return false whenever
  the key string contains spaces, making it strictly for single-key bindings.
  Introduced `ClientState.prefix_pending_until: Option<Instant>` to track
  prefix mode: pressing the leader alone only enters prefix mode (and
  suppresses PTY transmission) when at least one `<leader> X` binding
  exists. Split `check_config_keybindings` into prefix and single-key paths,
  with automatic 2-second timeout. Added 13 unit tests in key_map (4
  regression tests for the bug + happy-path single keys + 9 edge cases).

### Removed

- **`nexterm-launcher` crate removed**: ever since v0.9.3 made `nexterm-client-gpu`
  start the server as an internal tokio task (**single-binary design**,
  `bin name = "nexterm"`), the launcher had no purpose — but it was never
  deleted. Up through v1.3.1 both `nexterm-launcher` and
  `nexterm-client-gpu` declared `bin name = "nexterm"`, so `cargo build`
  would overwrite `target/release/nexterm` depending on compilation order
  (in practice the client-gpu variant won). This release deletes the
  launcher crate outright, resolving the bin-name collision at the root.
  Cleaned up the corresponding `if [ -f ]` guards in WiX / Flatpak /
  `release.yml` (they were for the client-gpu variant). The result matches
  the layout of mainstream terminal emulators (Alacritty / kitty /
  Ghostty / WezTerm): a single main binary (`nexterm`) plus a few
  auxiliary CLIs (`nexterm-ctl` / `nexterm-client-tui` / `nexterm-server`).
  No user impact (the `nexterm` command behaviour, distribution layout, MSI
  shortcut, etc. are unchanged). Workspace consolidated from 12 → 11
  crates.

### Documentation

- **Fixed gaps between public docs and reality**: a batch update of stale
  text that misled new users.
  - The `bincode` → `postcard` migration (done on 2026-05-12 in
    Sprint 5-1 / ADR-0006) was not propagated. Updated README.md /
    README.ja.md / docs/ARCHITECTURE.md / docs/THREAT_MODEL.md /
    docs/PROTOCOL.md (body text and diagrams). docs/DESIGN.md's ADR-004
    is now explicitly noted as superseded by ADR-0006.
  - `PROTOCOL_VERSION = 1` → `7` (README and README.ja v1.1.0 sections;
    annotated "see nexterm-proto/src/lib.rs for the current value").
  - Rust MSRV `1.78` / `1.80` → `1.85` (required because the workspace
    uses `edition = "2024"`). Updated README.md / README.ja.md /
    docs/src/install.md.
  - `SNAPSHOT_VERSION = 2` → `3` (workspace_name was added in
    Sprint 5-7 / Phase 2-1). Updated CLAUDE.md / docs/THREAT_MODEL.md /
    docs/adr/0007-snapshot-v1-deprecation.md.
  - CONTRIBUTING.md / CONTRIBUTING.ja.md dependency list:
    bincode → postcard.
  - nexterm-client-core/src/lib.rs framing comments switched to postcard.
  - README.md test count "240+ tests" → measured **660+** (with an
    annotation in the Test Strategy table of docs/ARCHITECTURE.md).
  - README.ja.md wording around the "daemonless design" updated to "an
    internal tokio task owns the PTY", matching the single-binary
    implementation.

### Build

- Bumped workspace version `1.3.1` → `1.4.0` (cleanup release, no
  breaking changes).

## [1.3.0] - 2026-05-17

Minor version corresponding to Sprint 5-6 (GPU client large-file split) and
Sprint 5-7 (UI/UX modernization Phases 1 + 2 + 3). Contains breaking changes
to the IPC protocol and snapshot format — consult
[docs/MIGRATION.md](docs/MIGRATION.md) before upgrading.

### Breaking-change Summary (v1.2.0 → v1.3.0)

- **`PROTOCOL_VERSION` bumped `4` → `7`**
  - `5`: workspace IPC (`ListWorkspaces` / `CreateWorkspace` /
    `SwitchWorkspace` / `RenameWorkspace` / `DeleteWorkspace` +
    `WorkspaceList` / `WorkspaceSwitched`) (Phase 2-1)
  - `6`: Quake-mode IPC (`QuakeToggle` + `QuakeToggleRequest`) (Phase 2-2)
  - `7`: tab reordering IPC (`ReorderPanes`) (Phase 2-3)
- **`SNAPSHOT_VERSION` bumped `2` → `3`**: added
  `SessionSnapshot.workspace_name` and `ServerSnapshot.current_workspace`.
  v2 JSON auto-migrates through `serde(default)`.
- **A new server rejects an old client at the Hello handshake, and an old
  server rejects a new client.** Always upgrade client and server together.

### Sprint 5-7 Phase 3 — Visual polish

- **Background image support** (Phase 3-1): `[window.background_image]`
  displays a wallpaper-style background. Five fit modes:
  `cover` / `contain` / `stretch` / `center` / `tile`, with adjustable
  opacity. Images larger than 4096×4096 are auto-downscaled with Lanczos3.
  Reuses the existing `image_pipeline` (used for Sixel/Kitty).
- **UI animations** (Phase 3-2): tab switching 200 ms (accent line
  extending + fade-in) and pane insertion 250 ms (white-overlay fade-out),
  both ease-out. `[animations] enabled = false` or `intensity = "off"`
  flips animations to instant (reduced-motion support). Four intensity
  steps: `off` / `subtle` (×0.5) / `normal` (×1.0) / `energetic` (×1.5).
- **Command palette: full coverage + history persistence** (Phase 3-3):
  added the 6 missing actions (Quit / ClosePane / NewWindow / QuickSelect /
  SetBroadcastOn / SetBroadcastOff) for a complete set of 25 actions.
  Usage history persisted to
  `~/.local/state/nexterm/palette_history.json` (atomic write + Unix
  0600). Ranking: by history when the query is empty; with a query, fuzzy
  score + history_bonus (use_count × 10 capped at 100, +100 within 24 h,
  +50 within a week).

### Sprint 5-7 Phase 2 — Headline features

- **Workspaces** (Phase 2-1): introduced a "workspace" concept that groups
  sessions. Added `nexterm-ctl workspace list / create / switch / rename /
  delete [--force]` subcommands. The status bar exposes a built-in
  `workspace` widget that shows the current workspace. The `default`
  workspace cannot be renamed or deleted.
- **Quake mode** (Phase 2-2): a global hotkey (default: `Ctrl+\``) makes the
  window slide in from a screen edge (top/bottom/left/right). Uses the
  `global-hotkey` 0.8 crate. Wayland has no global-hotkey API; as a
  workaround, `nexterm-ctl quake toggle/show/hide` can be invoked from a
  compositor `bindsym`.
- **Tab reordering** (Phase 2-3): drag tabs left or right on the tab bar to
  reorder. A 6 px threshold confirms the drag; below that it's treated as a
  click. While dragging, a ghost tab and an insertion-position indicator
  are drawn. `pane_order: Vec<u32>` is managed separately from the
  physical layout.

### Sprint 5-7 Phase 1 — Polish batch

- **Dynamic tab colors + hover highlight** (UI-1-1): added
  `activity_tab_bg` / `active_accent_color` / `show_tab_number` /
  `inactive_text_brightness` / `hover_highlight` to `TabBarConfig`. Tab
  backgrounds brighten on mouse hover.
- **Right-side status-bar widgets** (UI-1-2): added built-in widgets
  `cwd` / `cwd_short` / `git_branch` / `workspace`. Extended
  `WidgetContext` to propagate the focused pane's cwd.
- **Leader key support** (UI-1-3): `Config.leader_key` allows configuring
  the `<leader>` placeholder, making WezTerm-style prefix keybindings
  concise to express.
- **Key-hint overlay** (UI-1-4): pressing the leader alone shows
  prefix-style bindings as a semi-transparent overlay at the bottom of the
  screen for 2 seconds. New module `renderer/overlay/key_hint.rs`.

### Sprint 5-6 — GPU client large-file split (refactor)

No behaviour changes. Split the four largest GPU client files into
submodules to improve maintainability.

- `event_handler.rs` (1,318 lines) → 7 submodules (consent /
  settings_panel_hit / lifecycle / window / mouse / keyboard)
- `input_handler.rs` (1,377 lines) → 6 submodules
- `renderer/mod.rs` (1,579 lines) → 6 files (wgpu_init / render_frame /
  event_handler / etc.)
- `state.rs` (1,319 lines) → 7 files (pane / search / selection / menus /
  consent / server_message + state/mod.rs)

### i18n Entries Added

Sprint 5-7-related UI strings added to all 8 languages
(en / ja / zh-CN / ko / de / fr / es / it). 6 keys × 8 languages = 48 new
entries for the command palette, plus workspace / Quake / key-hint
related strings.

## [1.2.0] - 2026-05-13

The release tied to completing Sprints 5-1 through 5-5 of audit round 2
(70 tasks). Contains breaking changes — consult
[docs/MIGRATION.md](docs/MIGRATION.md) before upgrading.

### Breaking-change Summary (v1.1.0 → v1.2.0)

- **`PROTOCOL_VERSION` bumped `1` → `4`**
  - `2`: removed plaintext SSH password from the IPC (Sprint 5-1 / G1)
  - `3`: migrated IPC wire format from bincode to postcard (Sprint 5-1 / G3)
  - `4`: added OSC 7 CWD reporting and the `CwdChanged` event
    (Sprint 5-2 / B2)
- **IPC wire format**: replaced bincode with postcard (old v1.1.0 clients
  cannot connect to a v1.2.0 server)
- **GPU present mode default**: `fifo` → `mailbox` (accept tearing in
  exchange for a one-frame latency reduction; explicitly set
  `present_mode = "fifo"` to restore the old behaviour)

### Sprint 5-5 — Tests, observability, and documentation (I1/I2/A6/A9/J1/J2)

- **15 new unit tests in `nexterm-ssh`** (I1): `parse_jump_spec` /
  `parse_socks5_credentials` / `parse_forward_spec` / `SshConfig`
  construction / fast-fail on unreachable port. A full mock SSH server is
  future work.
- **5 new smoke tests in `nexterm-launcher`** (I2): per-OS extension paths
  for `server_exe` / `client_exe` / `tui_exe`, `exe_dir`, and the
  `wait_for_server` timeout path.
- **`tracing::instrument` on key async paths** (A6):
  `SshSession::connect/authenticate/open_shell`,
  `persist::save_snapshot/load_snapshot`, and IPC `dispatch_inner`.
  `dispatch_inner` uses `skip_all` because of sensitive payloads.
- **Snapshot v1 removal timeline pinned in ADR-0007** (A9): we plan to bump
  `SNAPSHOT_VERSION_MIN` from `1` to `2` in v2.0.0. The `nexterm-plugin`
  v1 removal timeline is referenced through ADR-0003 for consistency.
- **mdBook skeleton** (J1): added new `docs/src/troubleshooting.md` /
  `docs/src/adr-index.md`. `SUMMARY.md` gained a Reference section.
  `README.md` updated to a v1.2.0 baseline.
- **rustdoc warnings 9 → 0** (J2): backticked `[[macros]]` / `vec2<f32>` /
  `https://...` / `rows[y][x]` so they are not interpreted as links.
  `cargo doc --no-deps --lib --workspace` now finishes with zero warnings.

### Sprint 5-4 — Architecture cleanup + UX + ADRs (A1/A2/A3/D1/D4/D8/E1/F1/J3)

- **Split `overlay_verts.rs` (1,958 lines) into 5 files** (A2):
  `renderer/overlay/{picker, dialog, settings, util, mod}.rs`. The largest
  is settings.rs at 795 lines.
- **Split `nexterm-ctl/main.rs` (1,757 lines)** (A1): main.rs 343 lines +
  ipc.rs 96 lines +
  `cmd/{session, record, template, service, ghostty, theme, plugin, wsl, util, mod}.rs`.
- **Split `nexterm-server/web/mod.rs` (1,088 lines)** (A3): mod.rs 247
  lines + router.rs 129 lines + middleware.rs 144 lines +
  `handlers/{page, login, oauth, ws, assets, mod}.rs`.
- **Migrated examples/plugins (4) to Plugin API v2** (F1): added
  `nexterm_api_version() -> 2`, bumped plugin versions to `0.2.0`, and
  added a v1→v2 migration guide in `examples/plugins/README.md`.
- **WSL distro auto-detection + Profile import** (E1):
  `nexterm-ctl wsl import-profiles [--dry-run]` auto-detects WSL distros
  like Ubuntu and writes `[[profiles]]` entries to `config.toml`.
- **Quick Select expanded: 5 → 11 patterns** (D1): added Email / UUID /
  file:line / Jira / Windows path / IPv6 and others. Priority-ordered with
  duplicate elimination and 10 unit tests.
- **Theme gallery hidden-bug fix + `nexterm-ctl theme` subcommand** (D4):
  fixed `parse_builtin_scheme` falling back to Dark for Catppuccin /
  Dracula / Nord / OneDark. Added `BuiltinScheme::all() / from_toml_name()`.
- **Added `Ctrl+Shift+Z` as an alternative binding for Pane Zen mode**
  (D8): alongside the tmux-style `Ctrl+B Z`.
- **ADR directory cleanup** (J3): added template, README index, and 5
  retroactive ADRs (0002–0006) under `docs/adr/`.

### Sprint 5-3 — Performance measurement (C5/C1/C2/C3/I5/J4)

- **Introduced criterion benchmarks for `nexterm-vt`** (C5): performance
  regressions on the VT parser, scrolling, and Sixel decoding are now
  detectable. `cargo bench -p nexterm-vt`.
- **Input-latency measurement script added** (C1): scripted VT advance
  1 ms / wgpu present queue size measurements.
- **wgpu upgrade policy captured in ADR-0001** (C2): organised the 22 → 26
  decision with test results and alternatives.
- **Default present_mode changed to `mailbox`** (C3): cuts 1 frame.
  `[gpu] present_mode = "fifo"` restores the old behaviour.
- **Added a coverage job to GitHub Actions** (I5): `cargo-llvm-cov`
  generates `target/coverage`.
- **Published benchmark numbers in `docs/benchmarks.md`** (J4): documented
  reference values and how to re-measure.

### Sprint 5-2 — Terminal compatibility (B1/B2/B5)

- **Full OSC 133 (semantic prompt marks) + jump-to-prompt support** (B1):
  the client records prompt boundaries; `Ctrl+Up` / `Ctrl+Down` jump to
  the previous / next prompt. Also selectable from the command palette as
  "Jump to previous prompt / next prompt". Localised in 8 languages.
- **OSC 7 (CWD reporting) + parent CWD inheritance** (B2): added the
  `CwdChanged` IPC event (`PROTOCOL_VERSION` 4). Splitting a new pane
  now inherits the parent pane's CWD.
- **Synchronized Output (DCS=2026) test coverage** (B5): pinned existing
  behaviour with VT snapshot tests.

### Security — Sprint 5-1 (G3) IPC wire format: bincode → postcard

**Breaking change**: `PROTOCOL_VERSION` bumped `2` → `3`. See
[docs/MIGRATION.md](docs/MIGRATION.md) for details.

- **Removed `bincode = "1"` from every crate** and replaced it with
  `postcard = "1" (use-std)`.
  - Affected crates: `nexterm-proto` / `nexterm-server` /
    `nexterm-client-core` / `nexterm-client-gpu` / `nexterm-client-tui` /
    `nexterm-ctl`
  - Affected calls: `bincode::serialize` → `postcard::to_stdvec`,
    `bincode::deserialize` → `postcard::from_bytes` (3 implementation
    sites + 19 in tests)
- **Removed the `deny.toml` ignore for `RUSTSEC-2025-0141`** (bincode 1.x
  unmaintained). `cargo deny check` now passes the `advisories` section
  with zero ignores.
- **Side benefit**: postcard's varint encoding shrinks IPC messages by
  10–20% on average.
- Effect: removed the lock-in on bincode 1.x and moved to a maintainable
  supply chain.

### Security — Sprint 5-1 (G1) Removed plaintext SSH password from IPC

**Breaking change**: `PROTOCOL_VERSION` bumped `1` → `2`. See
[docs/MIGRATION.md](docs/MIGRATION.md) for details.

- **Removed `password: Option<String>` from `ClientToServer::ConnectSsh`**.
  Replaced with:
  - `password_keyring_account: Option<String>` — account identifier in the
    OS keyring
  - `ephemeral_password: bool` — flag to delete the keyring entry after
    successful authentication
- **Client (nexterm-client-gpu)**: `connect_ssh_host_with_password()`
  first calls `nexterm_config::keyring::store_password()` to save the
  password, then sends only the account name over IPC. When
  `PasswordModal.remember=false`, `ephemeral_password=true` is set.
- **Server (nexterm-server)**: `handle_connect_ssh()` retrieves the
  password via `nexterm_config::keyring::get_password()` and passes it to
  russh wrapped in `Zeroizing<String>`. When `ephemeral_password=true`,
  the entry is deleted after authentication.
- Effect: no plaintext password ever crosses the Unix Domain Socket /
  Named Pipe; the TODO in `input_handler.rs` (HIGH H-6) is cleared.

### Security — Sprint 5-1 (G2) GitHub Actions SHA pinning

- **Pinned every GitHub Action to a git SHA** (an SLSA 2 requirement).
  Mutable-tag references like `actions/checkout@v4` were replaced with
  the corresponding git commit SHA plus a `# v4.3.1` style comment. This
  defends against supply-chain attacks that retag upstream actions.
  - Files updated: `ci.yml` / `release.yml` / `sbom.yml` / `fuzz.yml` /
    `flatpak.yml` / `pages.yml` (36 sites total)
  - 9 actions pinned: `actions/checkout`, `actions/upload-artifact`,
    `actions/upload-pages-artifact`, `actions/deploy-pages`,
    `actions/attest-build-provenance`, `Swatinem/rust-cache`,
    `EmbarkStudios/cargo-deny-action`, `softprops/action-gh-release`,
    `dtolnay/rust-toolchain` (stable / nightly)

## [1.1.0] - 2026-05-10

Rollup release marking the end of Sprints 1–4. Contains breaking changes —
consult [docs/MIGRATION.md](docs/MIGRATION.md) before upgrading.

### Added — Sprint 4-2 Plugin API v2

- **Bumped `PLUGIN_API_VERSION` to 2.** New host contract:
  - **Input sanitization**: ESC, OSC/CSI/DCS/APC sequences, and C0 control
    characters (except `\t\r\n`) are stripped before they reach
    `nexterm_on_output` / `nexterm_on_command`. Plugins see plain text only.
  - **`write_pane` PaneId allow-list**: writes are permitted only to the
    pane IDs allowed for the current call scope. During
    `nexterm_on_output(pane_id, ...)` only that `pane_id` may be written;
    during `nexterm_on_command` no pane may be written.
- **`MIN_SUPPORTED_API_VERSION = 1`** preserves backwards compatibility for
  v1 plugins. They run with the old semantics (no sanitization, no write
  restriction) and a deprecation warning is logged at load.
- **Added `PluginInfo.api_version`** (shown by `nexterm-ctl plugin list`).
- **Made `sanitize_for_plugin(input: &[u8]) -> Vec<u8>` public** (for tests
  and diagnostics).

### Added — Sprint 4-4 Property tests

- **Added `proptest` (1.x) as a workspace dependency**
  (`[workspace.dependencies]`). Referenced from
  `[dev-dependencies]` of `nexterm-vt` / `nexterm-server`.
- **Sixel / Kitty parser property tests**
  (`nexterm-vt/tests/proptest_image.rs`):
  - `decode_sixel` / `decode_kitty` never panic on arbitrary byte
    streams
  - On success, `rgba.len() == width * height * 4` always holds
  - Huge dimensions (≥ 8193×8192) are always rejected with `None`
  - Panic-resistant when invoked through the VtParser path (including APC)
- **BSP / tiling property tests** (`nexterm-server/src/window/tests.rs`):
  - Arbitrary sequences of Insert / Remove operations never make
    `compute()` panic
  - Given enough area, rectangles stay on screen, don't overlap, and IDs
    are unique
  - Snapshot round-trips preserve pane IDs and rectangles
  - Tiling invariants hold (pane count match, in-bounds, ID match)

### Security — Sprint 1–3 hardening

Fixed CRITICAL / HIGH issues surfaced by a comprehensive security audit.
**Contains breaking changes. See [docs/MIGRATION.md](docs/MIGRATION.md) for
details.**

#### Authentication & Authorization (Web Terminal)

- **OAuth GitHub Org-validation bypass fixed** (CRITICAL): the old
  implementation's `get_current_token()` always returned `None`, so Org
  membership was never actually verified. `exchange_code()` now returns the
  `access_token` and propagates it to `is_user_allowed()`.
- **TOTP replay-attack mitigation** (CRITICAL): detects and rejects reuse
  of an OTP code within the ±1 window using `subtle::ConstantTimeEq`
  constant-time comparison + a `HashSet<(window, code)>`.
- **TOTP IP-based rate limiting** (CRITICAL): brute-force defense at 5
  attempts / 60 s, implemented in `web::rate_limit`. Returns 429 with
  `Retry-After: 60`.
- **TLS fallback off by default** (CRITICAL): silent downgrade to HTTP on
  TLS-config failure is gone. Explicitly opt in with
  `[web] allow_http_fallback = true`.
- **OIDC userinfo_endpoint SSRF mitigation** (HIGH): enforce HTTPS, reject
  internal IPs, verify issuer-domain match.
- **`legacy_token` constant-time comparison** (HIGH): `subtle::ConstantTimeEq`
  prevents timing attacks.

#### IPC / Protocol

- **bincode message-size cap** (CRITICAL): `MAX_MSG_LEN = 64 MiB` prevents
  local OOM attacks (applied at server / GPU/TUI clients / ctl — 4 places).
- **Protocol Hello + versioning**: connections must begin with
  `ClientToServer::Hello { proto_version, client_kind, client_version }`.
  `PROTOCOL_VERSION = 1`. Mismatch disconnects.

#### VT Parser / Images

- **VT buffer caps** (CRITICAL): introduced APC 4 MiB / DCS Sixel 16 MiB /
  Kitty chunked-transfer 64 MiB caps. Prevents DoS by a malicious PTY.
- **Image-decode u32 overflow fixed** (CRITICAL): `width * height * 4` is
  computed in u64 and limited by `MAX_IMAGE_BYTES = 256 MiB`.
- **OSC 8 URI allow-list** (CRITICAL): rejects schemes like `javascript:` /
  `file:`. Allowed: `http/https/mailto/ftp/ftps/ssh`. Caps: title 256 /
  notification 1024 / URI 2048 bytes.

#### Sandboxing

- **Lua sandbox** (CRITICAL): disabled `os` / `io` / `package` / `require` /
  `dofile` / `loadfile` / `debug`. Blocks RCE via `config.lua`.
- **WASM sandbox hardening** (CRITICAL): wasmi `consume_fuel(true)` +
  `FUEL_PER_CALL = 10M` provisioned before each call. `MAX_MEMORY_PAGES = 256`
  (16 MiB) caps memory. `nexterm_api_version()` verifies the version at
  load. Mutex poisoning is recoverable.

#### Secrets / Persistence

- **snapshot / host_history use atomic write + 0600** (CRITICAL): write to
  a temp file → fsync → rename, and force `mode(0o600)` on Unix. Prevents
  corruption on crash and leakage of sensitive data.
- **Force 0600 on TLS private keys** (HIGH): the key file produced when
  generating a self-signed cert is made 0600 regardless of umask.
- **GUI `PasswordModal` uses `Zeroizing<String>`** (HIGH): the
  password-input buffer is zeroed on drop.

#### Logging

- **Strip query strings from the access log** (HIGH): prevents OAuth
  `?code=` / `?state=` / `?token=` and friends from leaking into the log.

### Fixed

- **TomlConfig functionality regression fixed**: the old `TomlConfig`
  intermediate struct was missing `window/web/hosts/macros/log/cursor_style/
  auto_check_update/language` and similar fields, so most of what users
  wrote in `config.toml` was being silently ignored. Switched to
  deserializing `Config` directly.
- **DEFAULT_CONFIG_TOML template fixed**: the first-launch template used
  keys that didn't match the implementation, e.g. `[color_scheme] builtin = ...` /
  `[tab_bar] show = ...`. Aligned the key names with the implementation.
- **CI repair**: `cargo fmt --check` had been failing on master; cleaned up.

### Added

- **Tests**: added focused tests for every CRITICAL/HIGH fix (about 60
  tests across the proto / vt / config / server / plugin crates).
- **`docs/MIGRATION.md`**: migration document for the breaking changes
  (Lua sandbox, protocol Hello, TLS fallback default-off).

## [1.0.0] - 2026-04-27

### Added

- **v1.0.0 release**: the 0.9.x line is stabilised and officially released
  as v1.0.0.
  - Plugin API v1 frozen (`PLUGIN_API_VERSION = 1`) for a stable ABI
  - WASM plugin runtime (wasmi) + `nexterm-ctl plugin` CLI
  - SSH host history persistence + password-auth modal
  - Snapshot schema v2 (with auto migration)
  - GPU renderer (wgpu + cosmic-text), 3-pass rendering pipeline
  - 8-language i18n (en/ja/zh-CN/ko/de/fr/es/it)
  - Auto-update notification (polls the GitHub Releases API)
  - Settings panel (7 categories, TOML write-back, hot reload)
  - Web terminal (axum WebSocket + xterm.js)
  - Serial-port connection support

### Changed

- **CI branch fix**: changed the trigger branches in
  `.github/workflows/ci.yml` from `main` / `develop` to `master`. CI now
  runs automatically on push and PR to the default branch.

---

## [0.9.15] - 2026-04-27

### Added

- **MSI auto-update notification**: 5 seconds after startup, polls the
  GitHub Releases API in the background and shows a green banner at the
  top of the screen if a newer release exists.
  - New `update_checker` module
    (`nexterm-client-gpu/src/update_checker.rs`)
  - Uses `tokio::sync::watch` to asynchronously notify the latest version
  - The banner closes with `Esc`; `Enter` opens the release page in the
    default browser
- **`auto_check_update` config field**: added `auto_check_update = true/false`
  to `config.toml`. Default `true`.
- **Settings-panel integration**: added an `auto_check_update` toggle to
  the Startup category (toggle with `Space`, save with `Enter`).
- **i18n in 8 languages**: added the `update-available` / `update-dismiss` /
  `update-open-releases` keys to all 8 languages.

---

## [0.9.14] - 2026-04-27

### Added

- **Plugin API freeze (`PLUGIN_API_VERSION = 1`)**: Stable WASM ABI is now versioned. `nexterm-plugin` exports `PLUGIN_API_VERSION: u32 = 1` and provides `nexterm.api_version() -> i32` as a host import so plugins can verify compatibility at runtime.
- **`nexterm_meta` plugin export**: Plugins can now export `nexterm_meta(name_buf, name_max, ver_buf, ver_max) -> i32` to publish their name and version. Displayed in `nexterm-ctl plugin list`.
- **`unload` / `reload` methods on `PluginManager`**: Plugins can be unloaded (by path) or reloaded (unload + load) at runtime without restarting the server.
- **IPC plugin commands**: Four new `ClientToServer` messages: `ListPlugins`, `LoadPlugin { path }`, `UnloadPlugin { path }`, `ReloadPlugin { path }`. Corresponding `ServerToClient` responses: `PluginList { paths }`, `PluginOk { path, action }`.
- **`nexterm-ctl plugin` subcommands**: `list`, `load <path>`, `unload <path>`, `reload <path>`.
- **`PluginManager` embedded in `SessionManager`**: Plugin manager is now accessible from the IPC dispatch layer via `manager.plugin_manager`.
- **`echo-suppress` sample plugin** (`examples/plugins/echo-suppress/`): Demonstrates `nexterm_meta`, `api_version()` import, and output suppression.
- **`docs/plugin-api.md`**: Full Plugin API reference documenting all host imports, plugin exports, memory layout, and CLI management.

### Changed

- **`PluginInfo`** now includes `name: Option<String>` and `version: Option<String>` fields populated from `nexterm_meta`.
- Existing sample plugins README updated to include `echo-suppress`.

---

## [0.9.13] - 2026-04-26

### Added

- **Host history persistence**: Connection history is now saved to `~/.local/state/nexterm/host_history.json` (Unix) / `%APPDATA%\nexterm\host_history.json` (Windows). Frequently-connected hosts sort to the top across restarts.
- **Password authentication modal**: Selecting a host with `auth_type = "password"` in the SSH Host Manager now opens a password input overlay. Password characters are masked with `*`. Press Enter to connect, Esc to cancel.
- **`record_connection` wired**: Entering a host from the Host Manager now records the connection in history and persists it to disk immediately.

### Changed

- **`HostManager::new`** now calls `load_history()` on startup so previously recorded frequencies are available immediately.
- **`PasswordModal` struct** added to `host_manager` module with `push_char`, `pop_char`, and `take_password` methods.

---

## [0.9.12] - 2026-04-26

### Improved

- **Snapshot schema v2**: Added `session_title` field to `SessionSnapshot` for future display title support. Old v1 snapshots are automatically migrated on load.
- **Snapshot migration**: `persist::load_snapshot` now migrates v1 snapshots to v2 instead of discarding them. Supported version range: v1–v2.
- **Version guard**: `restore_from_snapshot` now accepts snapshots in the supported range (v1–v2) instead of requiring an exact version match.

### Added (tests)

- `test_v1_snapshot_migrates_to_v2`: Verifies that a v1 JSON snapshot deserializes correctly with `session_title` defaulting to `None`.
- `test_session_title_defaults_to_none`: Verifies backward-compat deserialization when `session_title` is absent.

---

## [0.9.11] - 2026-04-26

### Security

- **russh 0.58 → 0.59**: Mitigated pre-authentication DoS vulnerability (keyboard-interactive unbounded allocation). Updated `AgentIdentity::public_key()` call to match the new `authenticate_publickey_with` signature in russh 0.59.
- **lru 0.12 → 0.17**: Resolved `IterMut` stacked-borrows violation in the glyph atlas LRU cache.

---

## [0.9.10] - 2026-04-26

### Added

- **Cursor style**: New `cursor_style` config option (`"block"` / `"beam"` / `"underline"`) to control the cursor shape in the GPU renderer.
- **Window padding**: New `[window] padding_x` / `padding_y` config options to add pixel padding around the terminal grid.
- **Present mode**: New `[gpu] present_mode` config option (`"fifo"` / `"mailbox"` / `"auto"`) to control wgpu vsync behaviour.
- **Default color scheme**: Changed default color scheme to `TokyoNight`.

### Improved

- **Glyph atlas LRU cache**: Replaced the `HashMap`-based glyph cache with an `LruCache` to automatically evict stale entries after font changes, reducing memory waste.
- **Atlas size from config**: `[gpu] atlas_size` is now used as the maximum texture size for the glyph atlas. Initial size starts at half `atlas_size` (minimum 1024) and grows on demand.
- **Broadcast channel capacity**: Increased IPC broadcast channel capacity from 512 → 2048 to reduce dropped messages under heavy output.
- **Pane border visibility**: Increased separator width from 1 px → 2 px and adjusted border colour for better contrast with the Tokyo Night theme.

### Fixed

- **clippy lint**: Resolved `type_complexity` lint in `nexterm-server/src/web/oauth.rs` by introducing a `OAuthClient` type alias. Resolved `collapsible_if` lint in `nexterm-server/src/lib.rs`.

---

## [0.9.9] - 2026-04-25

### Fixed

- **Touchpad scrolling**: Fixed an issue where Windows touchpad scroll events (PixelDelta) were silently ignored. Added an accumulation buffer that triggers a line scroll once enough delta accumulates to equal one cell height.
- **Font ligatures**: Fixed an issue where `[font] ligatures = true` in the config file was not correctly passed through to FontManager.

### Improved

- **CI quality**: Removed `continue-on-error: true` from the Windows ConPTY integration test so that test failures now cause the build to fail.
- **WiX build stability**: Changed version injection to use `candle.exe -dVersion=X.Y.Z` flag instead of modifying the source file (`wix/main.wxs`) directly.

### Fixed (tests)

- **`window_config_default_value` test**: Fixed a mismatch where the test expected `background_opacity` to be `1.0` even after the default was changed to `0.95`.

---

## [0.9.8] - 2026-04-25

### Fixed

- **PowerShell auto-launch**: Fixed an issue where PowerShell did not start automatically on Windows. The config including the `-NoLogo` argument is now correctly propagated to all pane creation paths.
- **Window transparency**: Fixed an issue where the window background was not transparent on first launch without a config file. Changed the default opacity to 0.95.
- **Freeze on close**: Fixed a hang when closing the window with the × button. The IPC connection is now dropped before the server task is terminated.
- **Context menu text overflow**: Fixed shortcut key labels overflowing outside the menu border. Unified drawing position calculation to use `visual_width()`.

### Changed

- **Dependency update**: Updated `rand` from 0.8.6 to 0.9.4.

---

## [0.9.7] - 2026-04-20

### Added

- **Language selection UI**: Added the ability to select the UI language during installation and from the settings panel (8 languages supported).

### Fixed

- **Context menu width**: Fixed menu overflow for languages with longer translated text.
- **Freeze on window close**: Fixed a hang that occurred when attempting to close the window.
- **PowerShell detection**: Improved accuracy of automatic PowerShell shell detection.

---

## [0.9.6] - 2026-04-19

### Improved

- **nexterm-server ipc.rs module split**: Split `ipc.rs` (1707 lines) into 5 submodules for improved maintainability.
  - `ipc/platform.rs` — Unix Domain Socket / Windows Named Pipe listener and UID verification
  - `ipc/handler.rs` — Read/write loop for connected clients
  - `ipc/dispatch.rs` — Dispatch logic for 40+ IPC commands
  - `ipc/key.rs` — Keycode → VT escape sequence conversion (8 unit tests)
  - `ipc/sftp.rs` — SFTP upload/download helpers

- **Integration tests added**: Added 2 files under `nexterm-server/tests/`.
  - `ipc_integration.rs` — Round-trip tests for bincode serialization + 4-byte LE framing (14 tests)
  - `snapshot_roundtrip.rs` — JSON round-trip and persistence tests for session snapshots (6 tests)

- **`#![warn(missing_docs)]` applied workspace-wide**: Applied to 6 crates (nexterm-vt / nexterm-ssh / nexterm-plugin / nexterm-config / nexterm-server / nexterm-i18n) with missing documentation added in bulk.

### Fixed

- **Reduced `unwrap()` in production code**: Converted unsafe `unwrap()` calls in `web/mod.rs`, `web/auth.rs`, `web/oauth.rs`, `window.rs`, `nexterm-plugin`, and `nexterm-ssh` to `expect("reason")` for improved panic diagnostics.
- **`persist::state_dir()`**: Fixed to prefer the `XDG_STATE_HOME` environment variable (for test isolation and XDG compliance).

---

## [0.9.5] - 2026-04-18

### Added

- **CLAUDE.md**: Added project guide for Claude Code. Documents build commands, architecture overview, and coding conventions.
- **docs/KEYBINDINGS.md**: Extracted the complete key binding reference into a standalone file.

### Changed

- **Dependency updates**: Updated 104 packages to their latest compatible versions, including `vte` 0.13 → 0.15, `cosmic-text` 0.12 → 0.18, and `portable-pty` 0.8 → 0.9.
- **README refactor**: Reduced README.md by 32% (1019 → 690 lines). Replaced the changelog section with a link to CHANGELOG.md and moved key binding details to docs/KEYBINDINGS.md.

### Improved

- **nexterm-client-gpu module split**: Extracted 5 modules from `renderer.rs` (5553 lines) to improve maintainability.
  - `glyph_atlas.rs` — GlyphAtlas, BgVertex, TextVertex, GlyphKey
  - `shaders.rs` — WGSL shader constants
  - `color_util.rs` — ANSI 256-color and hex color conversion utilities
  - `key_map.rs` — winit keycode ↔ proto keycode conversion
  - `vertex_util.rs` — Rectangle, text, URL, and grid → text conversion utilities
- **Rustdoc expansion**: Added documentation comments to all public APIs in `nexterm-proto` (messages, types, enums). Enabled `#![warn(missing_docs)]`.
- **unsafe SAFETY comments**: Documented safety rationale for `SO_PEERCRED`/`getpeereid` in `nexterm-server/ipc.rs` and `libc::kill` in `pane.rs`.
- **Clippy warnings resolved**: Resolved all Clippy warnings across the workspace. Now compliant with CI's `-D warnings` flag.

---

## [0.9.4] - 2026-04-14

### Fixed

- **PowerShell crash fix**: Replaced direct array index accesses in `nexterm-vt`'s `erase_in_line`, `erase_in_display`, and `scroll_up` with the safe `Grid::clear_row()` / `Grid::copy_row()` methods. Prevents IndexError panics caused by complex VT sequences sent by PSReadLine.

### Added

- **Settings panel mouse interaction**: Sidebar categories, font size/opacity sliders, and theme color dots can now be clicked and dragged with the mouse. Sliders auto-save on drag release. Clicking outside the panel closes it.

### Changed

- **Terminal background transparency**: The terminal background is now 95% opaque by default (`background_opacity = 0.95`), giving a subtle see-through effect. The settings panel and context menu always remain fully opaque. Adjustable between 0.1 and 1.0 via `[window] background_opacity` in `nexterm.toml`.
- **Memory usage reduction**: Changed `cosmic-text`'s `FontSystem` initialization from a full system scan to loading only OS-specific font directories (macOS: `/System/Library/Fonts`, Windows: `C:\Windows\Fonts`). Estimated ~30–40 MB memory reduction.

---

## [0.8.0] - 2026-04-06

### Added

**Web Terminal: OAuth2 / SSO authentication**
- OAuth2/OIDC support for GitHub, Google, Azure AD, and any generic OIDC provider.
- Authorization Code Flow with CSRF protection (state parameter, 10-minute TTL).
- Access control via `allowed_emails` and `allowed_orgs` (GitHub only).
- Client secret can be set via `NEXTERM_OAUTH_CLIENT_SECRET` environment variable (recommended over storing in `nexterm.toml`).
- OAuth login button automatically injected into the login page when OAuth is enabled.

**Web Terminal: session management improvements**
- Configurable session TTL via `[web.auth] session_timeout_secs` (default: 86400 s = 24 h).
- Concurrent session limit via `[web] max_sessions` (0 = unlimited); oldest session is evicted when limit is reached.
- Explicit logout endpoint: `POST /auth/logout` revokes the session cookie.

**Web Terminal: HTTPS enforcement**
- New `[web] force_https = true` option; checks `X-Forwarded-Proto` and issues 301 redirects for HTTP requests (useful behind a TLS-terminating reverse proxy).

**Web Terminal: access log**
- New `[web.access_log]` section; logs every request (including WebSocket upgrades and failed auth attempts).
- CSV output to a configurable file path, or to the server log via `tracing` when no file is set.
- Fields: `timestamp`, `remote_addr`, `method`, `path`, `status`, `auth_method`, `user_id`.

**TUI client: multi-pane support**
- Ctrl+B prefix key system for pane management.
- Horizontal/vertical split, focus cycling, pane close, zoom.
- Status bar showing active session and pane count.
- Full help overlay (Ctrl+B ?).

**SSH host manager enhancements**
- Tag-based filtering and group management.
- Connection history with frequency-based sorting.
- Bulk operations (connect all in group, disconnect all).

**WASM plugin examples**
- Three ready-to-build sample plugins: `error-detector`, `command-counter`, `timestamp-injector`.
- Full plugin documentation including C and Rust examples.

**Documentation**
- Quickstart guide improvements, configuration snippet collection, Lua macro recipe collection.
- Full web terminal authentication reference including enterprise GitHub SSO example.

### Changed

- `[web.auth]` now contains `session_timeout_secs` field (previously hardcoded to 24 h).
- `[web]` has new fields: `max_sessions`, `force_https`, `access_log`.
- `nexterm-config`: `OAuthConfig` and `AccessLogConfig` are now publicly exported.

---

## [0.7.6] - 2026-04-06

### Added

**GPU client: TUI-parity tab bar and settings panel**
- Tab bar now displays the OSC 0/2 window title (e.g. current working directory) in each tab label, matching the TUI client behaviour.
- "⚙ Settings" button rendered on the right side of the tab bar; clicking it toggles the settings panel without a keyboard shortcut.
- Mouse click hit-testing on tab bar: clicking a tab switches the active pane; clicking the settings button opens/closes the panel.
- Settings panel font family field is now fully editable: press **F** (on Font tab) to enter edit mode, type the family name, **Backspace** to delete, **Enter** to confirm, **Escape** to cancel. Characters are intercepted before forwarding to the server.
- `PaneState` carries a `title: String` field updated by `ServerToClient::TitleChanged` messages.
- `ClientState` carries `tab_hit_rects` and `settings_tab_rect` populated each frame by `build_tab_bar_verts`.

### Changed

- `render()` and `build_tab_bar_verts()` now take `&mut ClientState` to allow per-frame hit-rect writes.

### Documentation

- All documentation converted to English as the primary language.
- Japanese translations added for user-facing docs: `shaders.ja.md`, `performance.ja.md`, `graphics.ja.md`, `plugins.ja.md`.
- `docs/ARCHITECTURE.md` and `docs/CONFIGURATION.md` fully translated to English.

---

## [0.7.5] - 2026-04-06

### Fixed

**GPU client: rendering quality pass**
- Added status bar height (`cell_h`) to the `visible_rows` calculation in scrollback view, fixing overlap between the last row and the status bar.
- `ScaleFactorChanged` (DPI change) event now recalculates `cols`/`rows` and sends a Resize notification to the server, resolving layout shift when moving to a high-DPI display.
- Applied `tab_bar_h` offset to the right-click context menu y-coordinate, fixing menu position when the tab bar is enabled.
- Added `cleared_this_frame` flag to `GlyphAtlas`; resetting the flag at the start of each frame prevents glyph corruption from stale UV coordinates after an atlas overflow mid-frame.
- Pre-declared `family_owned` for all code paths in `font.rs` to clarify lifetime structure.

---

## [0.7.4] - 2026-04-06

### Fixed

**GPU client (Windows): fix CJK full-width character spacing**
- Added `wide: bool` parameter to `rasterize_char()`; full-width characters (Unicode width ≥ 2) now render into a 2-cell buffer (`display_cols = 2.0`).
- Added `wide` field to `GlyphKey` so full-width and half-width glyphs are cached separately in the atlas.
- Japanese, Chinese, Korean, and other CJK characters are now evenly spaced and correctly rendered.

**GPU client (Windows): fix tab bar / terminal content overlap**
- Fixed tab bar (at y=0) and row-1 terminal content being drawn at the same y-coordinate.
- Added `y_offset: f32` parameter to `build_grid_verts` / `build_scrollback_verts`.
- Multi-pane `_in_rect` functions updated to use `off_y = row_offset * cell_h + tab_bar_h`.
- Pane borders and number badges now account for the tab bar height.

**GPU client (Windows): fix black band on the right side**
- The `rows` calculation was using the full window height, causing overlap with the tab bar and status bar.
- Fixed with `rows = (height - tab_bar_h - status_bar_h) / cell_h` for accurate usable row count.
- Corrected in both the initial window setup and resize event handler.
- Mouse click → cell coordinate conversion now subtracts `tab_bar_h` for accurate row targeting.

---

## [0.7.3] - 2026-04-06

### Fixed

**GPU client (Windows): fix font character spacing**
- `Attrs::new()` defaulted to `Family::SansSerif`, causing fallback to a proportional font (Segoe UI, etc.) on Windows.
- `measure_char_width` and `rasterize_char` now explicitly set `Family::Monospace` or `Family::Name(family)`.
- Config font name `"monospace"` maps to `Family::Monospace` (fontdb selects the system monospace font); specific names (`Consolas`, `JetBrains Mono`, etc.) use `Family::Name` directly.
- Cell width measurement switched from `Buffer::draw()` ink pixels to `layout_runs()` advance width, which includes right bearing for accurate character spacing.
- Eliminates the "Wi ndows PowerShe l l" extra-space rendering bug.

**Shader hot-reload, gallery, and migration tools**
- Added `WgpuState::reload_shader_pipelines()`: hot-reloads WGSL shaders on file change (no restart needed).
- `examples/shaders/`: bundled sample WGSL shaders — CRT, Matrix, Glow (background) / Grayscale, Amber (text).
- `nexterm-ctl import-ghostty`: imports a Ghostty config file and converts it to nexterm config.
- `nexterm-ctl service install/uninstall/status`: manages autostart services via systemd (Linux) / launchd (macOS).

---

## [0.7.2] - 2026-04-05

### Added

**Custom WGSL shader support**
- Added `[gpu]` section to `nexterm-config` (`custom_bg_shader` / `custom_text_shader` / `fps_limit` / `atlas_size`).
- GPU client loads WGSL files from the specified paths at startup (falls back to built-in shaders on failure).
- Enables custom effects such as CRT scanlines and glow.

**Documentation site expansion**
- `docs/src/features/graphics.md`: Sixel / Kitty graphics protocol guide.
- `docs/src/features/plugins.md`: WASM plugin development guide (with Rust sample code).
- `docs/src/advanced/shaders.md`: custom WGSL shader reference and examples.
- `docs/src/advanced/performance.md`: performance tuning guide.

### Performance

**GPU buffer reuse for rendering optimization**
- Added reusable vertex/index buffers to `WgpuState`.
- Replaced per-frame `create_buffer_init` (GPU allocation) with `queue.write_buffer` overwrites.
- Buffers are only reallocated (2× size) when capacity is exceeded; no reallocation in normal operation.
- GPU allocation count for an 80×24 terminal drops from **~4 per frame → 0 per frame**.

**FPS cap**
- `gpu.fps_limit` (default 60 FPS) controls the frame rate.
- Set to 0 for uncapped (vsync only).

**ASCII glyph pre-warming**
- ASCII printable characters (0x20–0x7E) are pre-loaded into the glyph atlas at startup in both Regular and Bold.
- Eliminates first-keystroke rasterization latency.

**Launcher startup time optimization**
- Changed `wait_for_server` polling to exponential backoff (10 ms, 10 ms, 10 ms, 20 ms, 50 ms, 100 ms).
- Average server-ready detection time reduced from **100 ms → ~30 ms** when the server starts quickly.

---

## [0.7.1] - 2026-04-05

### Fixed

**Fix ad-hoc codesign failure on macOS Intel builds**
- Signing individual binaries before signing the whole app bundle caused a subcomponent error.
- Changed to a single `codesign --force --deep --sign - dist/Nexterm.app` for the full bundle.

---

## [0.7.0] - 2026-04-05

### Added

**Floating panes**
- Added `OpenFloatingPane` / `CloseFloatingPane` / `MoveFloatingPane` / `ResizeFloatingPane` IPC commands.
- Added `FloatRect` cache and `floating_pane_rects` field to the GPU client.

**WASM plugin system**
- New `nexterm-plugin` crate (wasmi 0.38-based sandboxed WASM runtime).
- Built-in plugin API: `nexterm_on_output`, `nexterm_on_command`; host imports: `nexterm.log`, `nexterm.write_pane`.
- Added `plugin_dir` / `plugins_disabled` fields to config.

**Status bar widget enhancements**
- Built-in widgets: `"time"`, `"date"`, `"hostname"`, `"session"`, `"pane_id"`.
- Added `right_widgets` (right-aligned) and `separator` fields to `StatusBarConfig`.
- `WidgetContext` now passes session name and pane ID to widgets.

**Linux packaging**
- `linux/AppRun`: AppImage entry-point script.
- `pkg/flatpak/`: Flatpak manifest + AppStream metadata.
- Added AppImage build and upload step to GitHub Actions.
- `.github/workflows/flatpak.yml`: dedicated Flatpak build workflow.

**Test coverage improvements**
- Total test count: 145 → 178 (+33 tests).
- New tests in: nexterm-proto, nexterm-client-tui, nexterm-vt, nexterm-config, nexterm-plugin.

---

## [0.6.0] - 2026-04-05

### Added

**Four new built-in color schemes (Catppuccin / Dracula / Nord / One Dark)**

- Added `Catppuccin`, `Dracula`, `Nord`, and `OneDark` to `BuiltinScheme` in `nexterm-config`.
- Defined full fg/bg/ANSI[16] color palettes for all 9 schemes; reflected in the GPU renderer's terminal drawing.
- Settings panel (`[Colors]` tab) expanded to show all 9 scheme dots.

**Shell completion script generation**

- Added `nexterm-ctl completions <shell>` command.
  Outputs completion scripts for bash / zsh / fish / powershell / elvish to stdout.

**Man page generation**

- Added `nexterm-ctl man` command.
  Outputs a troff-format man page to stdout (`nexterm-ctl man > nexterm-ctl.1` to save).

**Bracketed paste mode (DEC ?2004)**

- VT parser now interprets `CSI ?2004h` / `CSI ?2004l` to track bracketed paste mode.
- When the mode is active, pasted text is wrapped with `ESC[200~` … `ESC[201~` before sending to the PTY.
  Prevents accidental command execution in zsh, fish, vim, and other shells/editors.

**Auto-load `~/.ssh/config`**

- Host Manager (`Ctrl+Shift+H`) now parses `~/.ssh/config` at startup and merges entries with `[[hosts]]`.
- `Host *` wildcards are excluded. Duplicate entries (same host + port already in `nexterm.toml`) are suppressed.

**Vim-compatible copy mode keys**

- `w` / `b`: word-wise forward / backward movement.
- `$`: jump to end of line.
- `Y`: yank the entire current line and exit copy mode.
- `/`: incremental search mode (Enter to confirm, n for next match, Esc to cancel).

**OSC 8 hyperlink support**

- Added `Grid.hyperlinks: Vec<HyperlinkSpan>` to `nexterm-proto`.
- VT parser interprets `ESC ] 8 ; ; <url> BEL` … `ESC ] 8 ; ; BEL` and records spans in the grid.
- GPU client's URL click (`Ctrl+Click`) now detects OSC 8 links first.

**Tab/pane activity notification**

- When output arrives in an unfocused pane, its tab shows an orange background and a `●` indicator.

**Mouse reporting (SGR ?1006 / X11 ?1000)**

- VT parser interprets `CSI ?1000h` / `CSI ?1006h` to track mouse modes.
- GPU client mouse clicks and drags are sent to the PTY as SGR escape sequences.
- Added `ClientToServer::MouseReport` message to `nexterm-proto`.

**Scrollback search UI completed**

- Added `Scrollback::search_prev()`. `Shift+Enter` or `Shift+N` moves to the previous match.
- Improved search bar UI: cursor `|`, accent line, key hint display.

**OSC 133 semantic zones**

- VT parser interprets `ESC ] 133 ; A/B/C/D BEL` to track prompt / command / output boundaries.
- Exit code of a completed command (D mark) is shown in the status bar (non-zero only).
- Added `ServerToClient::SemanticMark` message to `nexterm-proto`.

**Profiles (named configuration sets)**

- Added `Profile` struct and `Config.profiles` / `Config.active_profile` to `nexterm-config`.
- `Profile` can override font, colors, shell, scrollback, and tab bar from the base config.
- `Config::effective()` returns the config with the active profile applied.
- `Config::activate_profile(name)` / `clear_active_profile()` control profile switching.

### Changed

- `nexterm-client-gpu`: Settings panel scheme selector now supports all 9 schemes.

### Tests

- `nexterm-vt`: added bracketed paste mode enable/disable tests; OSC 8 hyperlink and OSC 133 semantic zone tests (18 tests total).
- `nexterm-server`: added BSP 4-split layout, session management API, and SSH config parser tests.
- `nexterm-config`: added profile application and TOML parse tests (17 tests total).

---

## [0.5.5] - 2026-04-05

### Fixed

**Windows — GPU client font rendering fixed**

- Replaced the `cell_w = font_size * 0.6` fixed-ratio heuristic with actual advance width measurement
  by rasterizing the reference character `'0'` at runtime via `layout_runs()`.
  Eliminates extra spaces between characters ("Wi ndows Power She l l").
- Added `scale_factor: f32` to `FontManager::new()`; passes `window.scale_factor()` from winit
  so the physical font size is correctly computed for high-DPI displays (125 %, 150 % scaling).
- Fixed a negative-coordinate wrap bug (`x as u32`) in the `rasterize_char` closure;
  added `if x < 0 || y < 0 { return; }` guard.
- `WindowEvent::ScaleFactorChanged` is now handled: font and glyph atlas are automatically regenerated on DPI change.

**Windows 11 — Acrylic frosted-glass background**

- Calls `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMWCP_ACRYLIC)` to apply
  a frosted-glass effect to the window background, similar to Windows Terminal.
- wgpu Surface composite alpha mode set to `PreMultiplied` for correct transparent blending.
- No effect on Windows 10 or non-Windows platforms; code is `#[cfg(windows)]`-guarded.

---

## [0.5.4] - 2026-04-05

### Fixed

**Windows — console window no longer appears on launch**

Added `#[windows_subsystem = "windows"]` (release builds only) to `nexterm.exe`,
`nexterm-server`, and `nexterm-client-gpu`. Launching `nexterm.exe` from the MSI installer
or Explorer no longer opens a stray black console window.

- Logs are written to `%LOCALAPPDATA%\nexterm\nexterm-server.log` / `nexterm-client.log`
  with daily rotation (`tracing-appender`).
- Errors are reported via `MessageBoxW` dialogs.

**macOS — binaries are ad-hoc signed + Intel Mac support**

- All macOS release binaries are now signed with `codesign --sign -` (ad-hoc).
  `xattr -dr com.apple.quarantine <file>` is all that's needed to bypass Gatekeeper.
- Built `x86_64-apple-darwin` target on the `macos-13` (Intel) runner;
  `nexterm-vX.Y.Z-macos-x86_64.tar.gz` is now included in release assets.

---

## [0.5.1] - 2026-03-31

### Fixed — Windows build & test (4 bugs)

This patch release fixes compilation and test failures that prevented the
Windows binary from being produced in the v0.5.0 release workflow.

| # | Crate / file | Root cause | Fix |
|---|---|---|---|
| 1 | `nexterm-launcher/Cargo.toml` | `windows-sys 0.59` split `CreateFileW` security descriptor handling into a separate `Win32_Security` feature; the feature was missing from the dependency declaration | Added `"Win32_Security"` to the `windows-sys` features list |
| 2 | `nexterm-launcher/src/main.rs` | `GENERIC_READ` was imported from `Win32::Storage::FileSystem`; in `windows-sys 0.59` it was moved to `Win32::Foundation` | Moved `GENERIC_READ` (and `INVALID_HANDLE_VALUE`) to the `Win32::Foundation` use statement |
| 3 | `nexterm-server/src/pane.rs` | `portable_pty` imports were guarded with `#[cfg(unix)]`, preventing `MasterPty`, `NativePtySystem`, `PtySize`, and `CommandBuilder` from being compiled on Windows even though `portable_pty` supports ConPTY on Windows | Removed the `#[cfg(unix)]` attribute from the `portable_pty` use statement |
| 4 | `nexterm-server/src/ipc.rs` | Path-validation unit tests used Unix-style absolute paths (`/home/user/…`, `/etc/passwd`, `/tmp/…`) which are **not** recognised as absolute by `std::path::Path::is_absolute()` on Windows, causing the "reject forbidden absolute paths" test to pass silently for the wrong reason | Added `#[cfg(unix)]` / `#[cfg(windows)]` guards; Windows tests use `%TEMP%\nexterm\…` and `D:\secret\…` / `C:\Windows\System32\…` style paths |

**All 93 unit tests now pass on `x86_64-pc-windows-msvc`.**

---

## [0.5.0] - 2026-03-27

### Added

**SSH & Connectivity**
- SSH multi-tab connections — SSH Host Manager (`Ctrl+Shift+H`) opens each host in a new tab
- X11 forwarding — `x11_forward = true` / `x11_trusted = true` in `[[hosts]]` (equivalent to `ssh -X` / `ssh -Y`)

**UX**
- In-app Settings GUI — `Ctrl+,` opens a Font / Colors / Window panel; changes write back to `nexterm.toml` instantly
- Settings action added to command palette (now 17 actions)

**Web Terminal**
- Embedded web terminal — `[web] enabled = true`; xterm.js served at `ws://localhost:7681`
- Token-based auth (`token = "..."` in config), disabled by default

**Package Distribution**
- Homebrew tap formula (`pkg/homebrew/nexterm.rb`)
- Scoop bucket manifest (`pkg/scoop/nexterm.json`)
- winget manifest (`pkg/winget/mizu-jun.Nexterm.yaml`)
- GitHub Pages documentation site auto-deployed via CI

---

## [0.4.0] - 2026-01-15

### Added

**SSH & Connectivity**
- SSH Host Manager — fuzzy-searchable host list (`Ctrl+Shift+H`); connects with one keystroke
- SFTP Upload / Download dialogs (`Ctrl+Shift+U` / `Ctrl+Shift+D`) with live progress bar
- Remote port forwarding (`-R`) over SSH sessions
- Serial port connections (`ConnectSerial` via command palette)

**UX & Pane Management**
- Command palette (Ctrl+Shift+P) extended with 16 actions including SFTP and host manager
- Lua Macro Picker — fuzzy-searchable macro list (`Ctrl+Shift+M`); one-key execution
- Quick Select mode (`Ctrl+Shift+Space`) — highlight URLs, paths, IPs, and hashes
- Pane zoom toggle (`Ctrl+B Z`) — focus a single pane full-screen
- Swap pane with next/previous sibling (`Ctrl+B {` / `Ctrl+B }`)
- Break pane to new window (`Ctrl+B !`)

**Automation**
- Lua event hooks: `on_session_start`, `on_attach`, `on_pane_open`
- Lua Macro engine: define `[[macros]]` in TOML, execute via picker

**Logging**
- Log filename templates (`{session}`, `{date}`, `{time}` placeholders)
- Binary PTY log mode

**Windows**
- MSI installer built with WiX Toolset v3 (CI-automated)
- Windows Service install/uninstall scripts
- Automatic code signing via `signtool.exe` when CI secrets are configured
- `nexterm-launcher` — single `nexterm.exe` auto-starts server + opens GPU client

---

## [0.3.0] - 2025-11-20

### Added

**SSH & Security**
- Known-hosts host key verification
- SSH agent authentication via `SSH_AUTH_SOCK`
- Local port forwarding through SSH tunnels
- ProxyJump multi-hop connection support
- SOCKS5 proxy support

**Terminal & Display**
- Full alternate screen buffer support (SMCUP/RMCUP)
- OSC 0/1/2 window title support
- OSC 9 desktop notifications
- CJK wide character rendering fixes

**GPU Client**
- IME input support (Japanese, Chinese, Korean)
- Keybinding customization
- Right-click context menu (Copy/Paste/Split/ClosePane)
- Pane number overlay in display-panes mode
- Mouse selection with automatic clipboard copy

---

## [0.2.0] - 2025-09-10

### Added
- GPU-accelerated renderer using wgpu + cosmic-text
- Command palette (`Ctrl+Shift+P`) with initial 8 actions
- Split pane: horizontal (`Ctrl+B %`) and vertical (`Ctrl+B "`)
- Scrollback buffer with configurable history size
- Basic session save / restore (JSON snapshots)

---

## [0.1.0] - 2025-07-01

### Added
- Initial release
- TUI client (`nexterm-client-tui`) using ratatui + crossterm
- IPC protocol between server and client (`nexterm-proto`)
- VT parser (`nexterm-vt`) with ANSI/xterm sequence support
- SSH client (`nexterm-ssh`) via `russh`
- TOML configuration (`nexterm-config`)
- i18n support for 8 languages (`nexterm-i18n`)
- `nexterm-ctl` CLI for session management
