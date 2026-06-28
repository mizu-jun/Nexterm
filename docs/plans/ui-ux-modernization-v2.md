# UI/UX Modernization v2

Status: planning → in progress (Sprint 5-15 / 2026-06-28)
Target: catch up to (and in places surpass) the visual polish of WezTerm,
Ghostty, Warp and Windows Terminal without compromising the GPU-renderer
architecture or daemonless design.

## Motivation

Nexterm already ships a wide feature surface (DesignTokens-driven theming,
Command Blocks, Quake mode, Animations, Acrylic blur, Quick Select, etc.).
The remaining visual gap with Ghostty / Warp comes down to a small number of
deep, foundational differences — most importantly that **every UI surface in
Nexterm is a flat axis-aligned rectangle** because the bg pipeline does not
implement a rounded-rect SDF (see the explicit note in
`renderer/ui_verts.rs:344`).

This plan addresses that gap and a curated set of other modernization items
identified during the 2026-06-28 research round (Zenn WezTerm customization
article, Luminoid 2026 terminal comparison, Windows Terminal 1.25 release
notes, Vibehackers Mac terminal review).

## Out of scope

Items already implemented (do **not** re-implement):

- WezTerm-style `TabBarConfig` (active/inactive colours, accent line, hover
  highlight, tab number, separator, OSC title) — `nexterm-config::schema::window`.
- Tab close `×`, tear-out `↗`, drag-reorder with ghost — `ui_verts.rs`.
- Background image with cover/contain/stretch/center/tile — `background_pass.rs`.
- macOS background blur + Windows Acrylic — `platform.rs`.
- Quake mode, Command Palette, Command Blocks, Vi-mode copy, Quick Select.
- Animations (intensity, reduced-motion, spring physics).
- Inactive-pane dim (alpha overlay, spring-animated).
- Profiles, Lua/TOML hot reload, AccessKit, OSC 8 hyperlinks, Kitty/Sixel.

## Identified gaps

| ID  | Gap | Evidence | Impact |
| --- | --- | --- | --- |
| G1  | No rounded-rect / SDF shader; all chrome is axis-aligned rects | `ui_verts.rs:344` comment explicitly says rounded corners "would need a custom shader" | ★★★ |
| G2  | No gradient background | grep `gradient`: 0 hits | ★★ |
| G3  | Settings panel has no search/filter | grep: 0 hits; Windows Terminal 1.25 added one | ★★ |
| G4  | No OS light/dark follow, no live theme preview | `winit::WindowEvent::ThemeChanged` is not subscribed | ★★ |
| G5  | No `hide_tab_bar_if_only_one_tab` equivalent | `TabBarConfig` lacks the field | ★ |
| G6  | No `+` new-tab button (× and ↗ exist) | `ui_verts.rs:248-252` registers only close/tearout rects | ★★ |
| G7  | Pane borders cannot be dragged to resize with the mouse | `resize.*drag` grep: only server-side hits | ★★ |
| G8  | No cursor blink interval / smooth cursor motion config | `CursorStyle` enum has only block/beam/underline | ★ |
| G9  | No HSB shift for inactive panes (WezTerm `inactive_pane_hsb`) | Current dim is a flat black overlay alpha | ★ |
| ~~G10~~ | ~~DEC 2026 Synchronized Output support is unverified~~ | **already implemented** — nexterm-vt has `synchronized_output_*` tests (verified 2026-06-28) | n/a |
| G11 | Tabs do not show a process / shell icon | label is OSC title verbatim | ★ |

## Phases

Phases are ordered by dependency. Phase 1 is the foundation that unlocks
Phases 2 (pill tabs) and parts of 4/5. Phases 3-6 are largely independent.
Each phase ships as one PR with its own tests and CHANGELOG entry.

### Phase 1 — SDF rounded-rect chrome foundation (★★★)

Goal: extend the existing `bg_pipeline` so every UI surface can opt into a
configurable corner radius without changing the call sites that pass
`radius = 0`.

Touchpoints:

- `nexterm-client-gpu/src/shaders.rs`: extend the bg shader WGSL with an SDF
  for rounded rectangles. Output `alpha = smoothstep(...)` so AA is free.
- `nexterm-client-gpu/src/glyph_atlas.rs`: extend `BgVertex` with
  `rect_center: [f32; 2]`, `rect_half_size: [f32; 2]`, `corner_radius: f32`.
  Legacy rects pass `corner_radius = 0.0` and stay pixel-identical.
- `nexterm-client-gpu/src/vertex_util.rs`: introduce
  `add_px_rounded_rect(x, y, w, h, r, color, ...)`. Keep `add_px_rect` as a
  thin wrapper that calls the new helper with `r = 0`.
- `nexterm-config/src/schema/window.rs`: add `UiConfig { corner_radius: f32 }`
  (default `6.0`). Live under `[ui]`. `0.0` disables radius globally for
  users on low-end GPUs.
- Migrate exactly two call sites in Phase 1: the focused-pane border halo
  and the notification banners. Other surfaces are migrated in later phases
  as their own polish work.

Tests:

- `add_px_rounded_rect` with `r = 0` produces byte-identical vertex data to
  `add_px_rect` (regression guard).
- WGSL SDF math reproduced as a pure Rust helper `signed_rect_distance` with
  unit tests covering corners, edges, and the centre.

Risk: vertex layout change touches every bg drawcall. Mitigated by keeping
`add_px_rect` as a zero-radius shim — no migration is forced.

### Phase 2 — Tab bar polish (★★)

Depends on Phase 1.

- Switch active/inactive tab bgs to use `add_px_rounded_rect` with a
  non-uniform radius (top 6 px, bottom 0) for a "pill on a shelf" look.
- New `TabBarConfig` fields:
  - `hide_when_single: bool` (default `false`, matches current behaviour).
    Skips the entire tab-bar drawcall and zeroes `tab_bar_h` when only one
    pane/tab exists.
  - `show_process_icon: bool` (default `false`). Prepends a Nerd Font glyph
    inferred from the active process name; falls back to text when the glyph
    is missing from the resolved font.
- New `+` button immediately left of the Settings button. Hit-rect tracked in
  `state.new_tab_hit_rect`, dispatched via existing `NewPane` IPC.
- Tests: glyph fallback selection, `hide_when_single` toggling, `+` hit rect.

### Phase 3 — OS theme follow + live preview (★★)

- Subscribe to `WindowEvent::ThemeChanged` in
  `renderer/event_handler/window.rs`; cache the current OS theme in
  `ClientState.os_theme`.
- Extend `ColorScheme` with `follow_system: bool`, `light_scheme:
  Option<String>`, `dark_scheme: Option<String>`. When `follow_system` is on,
  the effective palette is chosen at frame time from the current OS theme.
- Settings panel: in the scheme list, hover applies the corresponding
  DesignTokens as a transient preview; selection commits it via the existing
  toml_edit write path.
- Tests: theme switching keeps `DesignTokens` derivation stable, hover
  preview reverts on mouse-leave.

### Phase 4 — Settings search + mouse pane resize (★★)

- Settings panel: add a search field at the top using `SkimMatcherV2` (reuse
  the matcher already wired up by `palette.rs`). Filter renders both category
  names and field labels; matched substrings can be highlighted in a follow-up.
- Mouse pane resize: hit-test the 2/3 px adjacent borders in
  `event_handler/mouse.rs`, set `state.pane_resize_drag = Some(BorderHandle
  { row_or_col, axis, ratio_start })`, and on drag emit a new
  `ClientToServer::ResizePane` IPC (server already supports ratio changes via
  `window/bsp.rs`).
- Tests: settings filter ranks, border hit detection at exact pixel
  boundaries, mouse-up cleanup.

### Phase 5 — Gradient background + cursor polish (★)

- New `[window.gradient]` config: `from: String` (`#RRGGBB`), `to: String`,
  `angle: f32` (degrees). Mutually exclusive with `background_image`.
- `background_pass.rs`: add a gradient pipeline that piggybacks on the
  existing screen-space quad. Pure helper `compute_gradient_uv` with unit
  tests for 0°, 90°, 45° and 180°.
- `CursorConfig` (new file `nexterm-config/src/schema/cursor.rs`):
  - `blink_enabled: bool` (default `true`).
  - `blink_interval_ms: u32` (default `530`, the de facto xterm cadence).
  - `smooth_motion: bool` (default `true`). When on, `animations.rs`
    interpolates the previous cursor cell to the new one over 80 ms.
- Tests: blink phase function, interpolation easing endpoint identity.

### Phase 6 — inactive_pane_hsb (★)

DEC 2026 Synchronized Output was already implemented in nexterm-vt before
this plan was written (`synchronized_output_*` tests in `nexterm-vt/src` —
verified 2026-06-28). Phase 6 therefore collapses to the HSB transform:

- Replace the inactive-pane black overlay with a configurable HSB transform:
  `[colors.inactive_pane_hsb] hue = 1.0, saturation = 0.6, brightness =
  0.85`. The current spring-animated alpha still controls the transition.
- Tests: `[colors.inactive_pane_hsb]` round-trips through TOML; HSB
  multiplication on a representative pixel.

## Risks and mitigations

1. **SDF cost on low-end GPUs.** The new bg pipeline does one extra `length()`
   and a `smoothstep` per fragment. Mitigation: `[ui] corner_radius = 0.0`
   short-circuits the rounded path; `[gpu] profile = "low_power"` can pin
   that override.
2. **Per-pane vertex cache regression.** Commit `90c916a` (C4) added a
   per-pane vertex cache; vertex layout changes have to be applied to the
   cached buffers too. Mitigation: bump the cache key whenever the bg vertex
   layout changes.
3. **Backward-compatible config.** All new fields use `#[serde(default)]` so
   existing `config.toml` files continue to load. Document the additions in
   `docs/CONFIGURATION.md`.
4. **CJK width drift.** Nerd Font glyphs inserted in tab labels must respect
   `FontManager`'s unicode-width logic. Mitigation: prepend glyphs only when
   the font *actually* contains them (probe via cosmic-text).
5. **i18n.** Every new user-visible string in the Settings panel and the
   command palette must land in all eight locale files under
   `nexterm-i18n/locales/`.
6. **GPU verification gap.** CI cannot validate the rendered output. Each
   phase ships with a hand-run screenshot saved to `docs/img/uiux-v2/phase-N/`
   so reviewers have a reference.

## Complexity estimates

| Phase | Estimate | Primary files |
| --- | --- | --- |
| 1 | HIGH (10–14 h) | `shaders.rs`, `vertex_util.rs`, `glyph_atlas.rs`, `gpu_buffers.rs`, new WGSL |
| 2 | MEDIUM (5–7 h) | `ui_verts.rs`, `mouse.rs`, `state/mod.rs`, `window.rs` (config) |
| 3 | MEDIUM (5–7 h) | `event_handler/window.rs`, `color.rs`, `overlay/settings.rs` |
| 4 | MEDIUM (6–8 h) | `settings_panel.rs`, `mouse.rs`, `window/bsp.rs`, `pane_dispatch.rs` |
| 5 | MEDIUM (5–7 h) | `background_pass.rs`, `animations.rs`, new `cursor.rs` schema |
| 6 | LOW (2–3 h) | `ui_verts.rs`, `tokens.rs` (DEC 2026 already shipped) |

Total: 33–47 h (≈ one to two-week sprint).

## Implementation status (2026-06-28)

| Phase | Status | Notes |
| --- | --- | --- |
| 1 | **shipped** | 5-attribute `BgVertex`, `BG_SHADER` SDF branch, `add_px_rounded_rect_sdf`, `signed_rect_distance`. 9 new unit tests pass. |
| 2a | **shipped** | Tab pills, Settings button, drag ghost tab all render via the SDF helper. |
| 2b | **shipped** | `TabBarConfig.hide_when_single` (default `false`) and `TabBarConfig.show_new_tab_button` (default `true`). `state.new_tab_hit_rect` populated each frame; left-click dispatches `SplitVertical`. |
| 2c | deferred | Per-tab process / shell icon (Nerd Font). Requires a new IPC field for the active process name. |
| 3 | **shipped** | `Config.colors_follow_system`, `colors_light`, `colors_dark`; `ClientState.os_dark_mode` seeded at window creation and refreshed on `WindowEvent::ThemeChanged`; `Config::effective_color_scheme` selects the right palette per frame. 4 new unit tests pass. |
| 3b | deferred | Live theme preview on hover inside the settings panel. |
| 4 | **shipped** | Category-level fuzzy search in the settings panel sidebar (`/` activates, Esc clears then closes, `filter_categories` helper with SkimMatcherV2 + per-category keyword synonyms). Mouse pane resize via border hit-test (`hit_test_pane_border`, 4 px tolerance) → focus adjacent pane → stream `ClientToServer::ResizeSplit` deltas. Cursor switches to `EwResize` / `NsResize` on hover. 14 new unit tests pass. Reuses existing `ResizeSplit` IPC (no protocol bump). Field-level filtering deferred — the panel renderer would need to be split first. |
| 4b | **shipped** | Field-level search via the `category_fields` catalogue (`FieldEntry { label, aliases }`); `filter_categories` now scores against label + field labels + aliases, and the sidebar renders a `(N)` hit-count badge per category when a query is active. The pure `field_hit_count` helper drives the badge. Deeper filtering inside the main panel (collapsing non-matching rows) still requires breaking up the 2.1 kLoC `build_settings_panel_verts` and is left as a future task. 7 new unit tests pass. |
| 5 | **shipped** | Linear-gradient background + cursor blink. `[window.gradient]` (CSS-convention angles; mutually exclusive with `background_image`) draws via per-corner vertex colours on the existing `bg_pipeline` (no new shader). `[cursor]` config with `blink_enabled` / `blink_interval_ms` (xterm-default 530 ms, safe-floor 50 ms) gates `draw_cursor_with_visibility`; visibility is part of the `PaneRenderCache` key so toggles invalidate cached buffers. 15 new unit tests (gradient_t at 0°/45°/90°/180°/270°/wrap/NaN, geometry sanity, cursor visibility schedule, safe-floor, TOML round-trips). |
| 5b | pending | Smooth cursor motion (animations.rs interpolation over 80 ms). Config flag `cursor.smooth_motion` already lives in `CursorConfig` so TOMLs don't break when 5b lands. |
| 6 | pending | `inactive_pane_hsb` (DEC 2026 already in nexterm-vt). |

## References

- Zenn — [mozumasu: WezTerm カスタマイズで快適なターミナル環境を構築する](https://zenn.dev/mozumasu/articles/mozumasu-wezterm-customization)
- Luminoid — [Choosing a terminal emulator in 2026](https://blog.luminoid.dev/Terminal-Emulator-Comparison-2026/)
- 4sysops — [Windows Terminal Preview 1.25](https://4sysops.com/archives/windows-terminal-preview-125-kitty-protocol-settings-search-and-gui-for-key-bindings/)
- DeepWiki — [microsoft/terminal: Window Features and Customization](https://deepwiki.com/microsoft/terminal/6.2-quake-mode-and-window-features)
- Vibehackers — [Best Terminal for Mac in 2026](https://vibehackers.io/blog/best-terminal-for-mac)
