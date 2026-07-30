# Overlay Draw-Order Fix + WT-style Custom Title Bar

Status: approved 2026-07-30. Execution order: PR1 → PR2 → PR3 → PR4.

Goal: (a) stop terminal text from bleeding through the settings panel and
every other floating overlay, and (b) retire the native title bar in favour
of a Windows Terminal-style tab bar that doubles as the title bar — within
the existing wgpu overlay architecture (no DOM/web UI).

Background: the maintainer's screenshots (2026-07-30) show terminal glyphs
drawn on top of the settings panel, and a two-storey chrome (native title
bar + tab bar) that looks dated next to Windows Terminal. Investigation
confirmed the bleed-through is a draw-order defect (single bg pass followed
by a single text pass lets grid glyphs paint over overlay backgrounds), not
an alpha problem, and that `WindowDecorations::NoTitle` exists in config/UI
but is never applied (behaves like `Full`).

Decisions (maintainer, 2026-07-30): scope = PR1–PR4 below (macOS custom
chrome deferred); default `decorations` stays `Full` (opt-in `notitle`);
include the small tab-bar polish items; keep the settings scrim at 0.72.

## Phases

### PR1 — Overlay draw-order fix (bug, ships alone) — DONE (2026-07-30)

Record `overlay_bg_start` / `overlay_text_start` after the Quick Select
build in `render_frame.rs::render()`, then draw the main pass as
"grid bg → grid text → overlay bg → overlay text". The Phase B1 settings
scroll scissor becomes a sub-range of the overlay layer via the pure
function `split_scissored_range` (unit-tested). `SCRIM_ALPHA` unchanged.

- [x] Layer boundary markers + 4-stage main pass
- [x] `split_scissored_range` + unit tests
- [x] CHANGELOG entry

### PR2 — Custom title bar, phase 1 (`notitle` comes alive; Windows-verified)

Redefine the dead `NoTitle` variant as "borderless + tab bar acts as the
title bar". Add `wants_os_chrome()` / `wants_custom_titlebar()` to
`WindowDecorations` and use them at the three duplicated decision sites
(`lifecycle.rs`, `event_handler/mod.rs` spawn + hot-reload). Secondary OS
windows keep native decorations (mouse handling is not window-id aware).

- [x] `WindowDecorations` helper methods + exhaustive tests
- [x] Window buttons `─` / `□`(`❐` when maximized) / `×` in
      `build_tab_bar_verts` (flat, hover-filled via SDF pill; widths are
      `cell_w`-relative), hit rects in `ClientState`, dispatch in `mouse.rs`
- [x] Double-click on tab-bar blank space toggles maximize
      (`last_chrome_click`, disabled while Quake is visible)
- [x] Edge resize: `chrome_resize.rs` (pure `resize_edge_at` +
      `resize_cursor`, unit-tested) → cursor shape + `drag_resize_window()`
- [x] Right-click on the tab bar: `show_window_menu()` (`#[cfg(windows)]`)
- [x] Fix `quake.rs` hard-coded `decorations: true` (existing bug; now
      captures `window.is_decorated()`)
- [x] Fix IME candidate position missing `grid_offset` (existing bug)
- [x] Fix `loader.rs` template comment `decorations = "default"` → `"full"`
- [x] AccessKit `Role::Button` nodes for the three buttons (labels stay
      English like the rest of the tree; localizing the a11y tree is a
      separate task — the planned i18n keys were dropped with it)
- [~] Maximized overhang compensation: intentionally NOT implemented.
      winit 0.30.13 does not clamp undecorated maximize
      (WM_GETMINMAXINFO only honours user min/max sizes), but whether the
      DWM overhang actually clips the grid is only observable on real
      Windows. Verify during manual QA; add the padding compensation as a
      follow-up if the edges clip.

### PR3 — Custom title bar, phase 2 (non-Windows menu; Linux-verified)

- [x] `ContextMenu::new_window_system_menu` (maximize-or-restore /
      minimize / close; move/size intentionally omitted) +
      item-composition test. Reuses the existing right-click sizing and
      clamping logic in `on_mouse_right_pressed`.
- [x] Actions: `MinimizeWindow` + `ToggleMaximizeWindow` (one toggle
      variant instead of the planned separate Restore/Maximize — the
      menu shows whichever label applies) + `RequestCloseWindow`
      (routes through `UserEvent::RequestClose` so `close_action`
      applies; the planned `CloseOsWindow` reuse would have bypassed it)
- [x] `warn!` on `drag_window` / `drag_resize_window` errors (Wayland)
- [x] i18n: `context-menu-window-{restore,minimize,maximize}` ×8 locales
- [ ] Manual smoke: X11 + Wayland (GNOME/KDE)

### PR4 — Tab bar polish

- [x] `corner_radius_chrome` default 6.0 → 10.0
- [x] `draw_overlay_panel`: legacy `add_rounded_px_rect` →
      `add_px_rounded_rect_sdf` (the legacy helper had no callers left
      and was removed). Visual regression pass over all overlays is part
      of the manual QA round.
- [~] Wiring the hard-coded panel radius (6.0 at every call site) to the
      existing `[ui] corner_radius_overlay` setting was descoped: it
      needs a `ui_cfg` parameter through ~10 overlay builder signatures.
      Tracked as a separate cleanup — today the setting exists but only
      the default value is honoured visually.

## Out of scope

`WM_NCCALCSIZE`-based overhang fix, macOS custom chrome (future phase 3:
transparent-titlebar + traffic lights via `WindowAttributesExtMacOS`),
`notitle` for secondary OS windows, move/size items in the system menu,
per-button colour config.

## Follow-up (requested 2026-07-30, after PR1–PR4 shipped)

Two items originally descoped were requested as follow-up work.

### PR5 — Windows 11 snap layouts (`WM_NCHITTEST` subclass)

DWM shows the snap-layout flyout only where `WM_NCHITTEST` answers
`HTMAXBUTTON`; a borderless winit window answers `HTCLIENT` everywhere.
winit 0.30 passes `WM_NCHITTEST` straight to `DefWindowProc` and treats
`WM_NCLBUTTONDOWN` specially only for `HTCAPTION` (verified in the vendored
source), so a `SetWindowSubclass` hook composes cleanly.

- [x] `snap_layout.rs` (cfg(windows)): `SetWindowSubclass` on the primary
      window; `WM_NCHITTEST` → `HTMAXBUTTON` over the maximize button's
      rectangle. The button becomes non-client, so hover/click are
      reconstructed from `WM_NCMOUSEMOVE`/`WM_NCMOUSELEAVE` (with
      `TrackMouseEvent` TME_NONCLIENT) and `WM_NCLBUTTONDOWN/UP`
      (swallowed; UP toggles) and forwarded as UserEvents.
- [x] `UserEvent::SnapMaximizeToggle` (Quake-guarded, same as the
      client-area path) + `SnapMaximizeHover` (drives
      `hovered_window_button` for the hover fill).
- [x] Renderer publishes the button rect (physical px, tab-bar band)
      after every frame; `None` keeps the hook dormant, so `full`/`none`
      decorations and hidden tab bars are unaffected.
- [x] windows-sys features: `Win32_Graphics_Gdi`, `Win32_UI_Shell`,
      `Win32_UI_Input_KeyboardAndMouse`. All call signatures type-checked
      against the vendored crate for `x86_64-pc-windows-msvc` (a full
      workspace cross-check is blocked by aws-lc-sys's C build).
- [x] Unit tests: lparam → signed screen coords (negative-monitor case),
      half-open rect containment.
- [ ] Manual (Windows 11): hover maximize button → flyout appears; zone
      pick snaps the window; click still toggles; hover fill tracks.

### PR6 — `decorations = "notitle"` as the default

- [x] `WindowDecorations::default()` → `NoTitle` on Windows/Linux;
      macOS keeps `Full` (winit's `drag_resize_window` is unimplemented
      there — a borderless window would not be resizable). Implemented
      as a hand-written platform-aware `impl Default` (the derive can't
      branch on cfg); `WindowConfig::default()` follows it.
- [x] Template comment (`loader.rs`), `docs/CONFIGURATION.md`,
      `docs/ARCHITECTURE.md`, CHANGELOG breaking-change note (set
      `decorations = "full"` to restore the native title bar).
- [x] Tests: platform-default assertion (enum + omitted TOML key); the
      settings-panel cycle test now starts from an explicit variant
      instead of the platform-dependent default.
