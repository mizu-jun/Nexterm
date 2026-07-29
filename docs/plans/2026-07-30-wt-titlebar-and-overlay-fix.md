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

- [ ] `WindowDecorations` helper methods + exhaustive tests
- [ ] Window buttons `─` / `□`(`❐` when maximized) / `×` in
      `build_tab_bar_verts` (SDF pill pattern, `cell_w`-relative widths),
      hit rects in `ClientState`, dispatch in `mouse.rs`
- [ ] Double-click on tab-bar blank space toggles maximize
      (`last_chrome_click`, disabled while Quake is visible)
- [ ] Edge resize: `chrome_resize.rs` (`ResizeEdge` + `resize_edge_at`,
      pure + tested) → cursor shape + `drag_resize_window()`
- [ ] Right-click on blank chrome: `show_window_menu()` (`#[cfg(windows)]`)
- [ ] Maximized: +8 px padding compensation (borderless overhang)
- [ ] Fix `quake.rs` hard-coded `decorations: true` (existing bug)
- [ ] Fix IME candidate position missing `grid_offset` (existing bug)
- [ ] Fix `loader.rs` template comment `decorations = "default"` → `"full"`
- [ ] AccessKit `Role::Button` nodes for the three buttons
- [ ] i18n: `titlebar-button-{minimize,maximize,restore,close}` ×8 locales

### PR3 — Custom title bar, phase 2 (non-Windows menu; Linux-verified)

- [ ] `ContextMenu::new_window_system_menu` (restore/minimize/maximize/
      close; move/size intentionally omitted) + item-composition test
- [ ] `ContextMenuAction::{RestoreWindow, MinimizeWindow,
      ToggleMaximizeWindow}` (close reuses `CloseOsWindow`)
- [ ] `warn!` on `drag_window` / `drag_resize_window` errors (Wayland)
- [ ] i18n: `context-menu-window-{restore,minimize,maximize}` ×8 locales
- [ ] Manual smoke: X11 + Wayland (GNOME/KDE)

### PR4 — Tab bar polish

- [ ] `corner_radius_chrome` default 6.0 → 10.0
- [ ] `draw_overlay_panel`: legacy `add_rounded_px_rect` →
      `add_px_rounded_rect_sdf`, fixed radius → `ui_cfg.chrome_radius()`
      (visual regression pass over all overlays)

## Out of scope

Windows 11 snap-layout flyout (`WM_NCHITTEST` hook), `WM_NCCALCSIZE`-based
overhang fix, macOS custom chrome (future phase 3: transparent-titlebar +
traffic lights via `WindowAttributesExtMacOS`), `notitle` for secondary OS
windows, move/size items in the system menu, per-button colour config.
