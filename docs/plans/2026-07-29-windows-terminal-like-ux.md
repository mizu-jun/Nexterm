# Windows Terminal-like UX Improvements

Status: approved 2026-07-29. Execution order: P1 → P4 → P2 → P3 (P5 not scoped).

Goal: close the remaining usability gaps versus Windows Terminal in (a) the
day-to-day screen (tab bar / new-session entry points) and (b) the settings
panel — within the existing wgpu overlay architecture (no DOM/web UI, per
`docs/PRODUCT.md` non-goals).

Most WT-like elements already shipped in v1.11.0 (`plans/archive/ui-ux-modernization-v2.md`):
pill tabs with `+`/`×`/tear-out/drag/process icons, 9-category settings GUI with
sidebar search + hit badges, theme gallery/hover preview/OS theme follow,
command palette, Quake mode, Acrylic. This plan covers only what is left.

## Phases

### P1 — New-tab profile dropdown (impact ★★★, MEDIUM-HIGH) — DONE (2026-07-29)

WT's signature UX: `+` click opens a new tab with the default profile; a `▾`
chevron next to it (or right-click on `+`) opens a menu listing profiles.

- Menu items: default / `Config.profiles` entries / detected WSL distros
  (`nexterm-config/src/wsl.rs`) / separator / open settings.
- Touchpoints: `renderer/event_handler/mouse.rs` (hit handling),
  `renderer/ui_verts.rs` (chevron), `state/menus.rs` (reuse the context-menu
  infrastructure), `nexterm-proto` (new-pane-with-profile IPC if missing —
  possible PROTOCOL_VERSION bump), `nexterm-server/src/ipc/dispatch.rs`.
- Verify first: whether profile `shell_program` / `working_dir` are actually
  wired into PTY spawn (`ProfileEntry` fields carry `#[allow(dead_code)]`).
- Tests: pure menu-item builder unit tests; IPC round-trip in
  `nexterm-server/tests/ipc_integration.rs`.
- i18n: all new strings in all 8 locales.

### P4 — Settings panel convenience pack (impact ★, LOW)

- "Open config.toml in editor" button (reuse the `platform.rs` URL-open
  pattern; `CREATE_NO_WINDOW` on Windows).
- Per-field "restore default".

### P2 — Row-level settings search filtering (impact ★★, HIGH)

Complete the WT 1.25-style settings search: when a query is active, show only
matching rows (with highlight). Prerequisite refactor: split the ~2.1 kLoC
settings vertex builder in `renderer/overlay/settings/` into per-row units,
guarded by a "identical vertex output before/after" regression test. Reuses
the existing `category_fields` / `FieldEntry` catalogue.

### P3 — Keybinding recorder + conflict detection (impact ★★, MEDIUM)

Press-to-record in the keybinding fields (`settings/keybindings_edit.rs`)
plus a warning when the chord collides with an existing binding.

## Out of scope

- Tab-contains-panes model (WT-style). Nexterm keeps one tab per pane.
- First-run wizard (P5) — not approved.

## Shared risks

- GPU output is not CI-verifiable → save hand-run screenshots under `docs/img/`.
- Every new user-facing string lands in all 8 locales with the key-parity test.
- `cargo clippy -- -D warnings`, `cargo fmt --check`, full test suite stay green.

## Progress

- [x] P1 new-tab profile dropdown (`feat/new-tab-profile-dropdown`) —
  `SplitWithShell` IPC (PROTOCOL_VERSION 11), env support in `Pane` spawn,
  `▾` dropdown reusing `ContextMenu`, WSL distros cached at startup, real
  `OpenProfile` launch, `tab-dropdown-new-tab` key ×8 locales. Note: the
  `ProfileEntry` dead-code suspicion was confirmed — `OpenProfile` was a stub.
- [x] P4 settings convenience pack (PR #39) — "Open config.toml" footer link;
  per-field restore-default folded into P2.
- [x] P2-A search highlight + category reset (`feat/settings-search-highlight`)
  — rendered-label fuzzy highlight (localized labels searchable), per-category
  "Reset defaults" footer link (value categories only), i18n ×8, i18n locale
  test race fixed with a test mutex. Row *collapse* (P2-B row model) is next.
- [x] P2-B search collapse (same PR as P2-A) — `settings/row_filter.rs`
  visible-row/slot model; Window (14 rows incl. sliders), Security, and
  Blocks draw + hit-test both derive Y from the same visible list. Font /
  Startup / Theme keep highlight-only (3–4 short rows, nothing to collapse);
  list categories (SSH / Keybindings / Profiles) are out of scope.
- [x] P3 keybinding conflict warning (`feat/keybinding-conflict-warning`,
  stacked on P2) — the press-to-record recorder already existed
  (`KeyEditMode::Record`, Enter → capture → write-back), so P3 reduced to
  duplicate-chord detection: pure `find_key_conflict` (case-insensitive,
  whitespace-normalized) + a non-blocking `semantic_warning` line under the
  keybindings list, `settings-key-conflict` ×8 locales.
