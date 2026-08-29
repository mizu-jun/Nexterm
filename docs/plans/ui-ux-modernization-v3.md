# UI/UX Modernization v3 — Fluent Design Foundation

Status: approved 2026-07-30; last reconciled 2026-08-21 (P1 complete:
metric tokens, widget layer, all 9 settings tabs migrated, G11 colour
migration and its follow-up — two sites stay out by decision, recorded
below; on-device verification remains accepted-as-unverified).
Target releases: v1.17–v1.21+.
Execution order: P0 → P1 → P2 → {P3, P4, P5 parallelizable} → P6 → P7.
Source: successor to `plans/archive/ui-ux-modernization-v2.md` and
`plans/archive/2026-07-29-windows-terminal-like-ux.md`.

## Motivation

v1.11–v1.15 closed the *feature* gaps versus Windows Terminal (pill tabs,
9-category settings GUI with search, profile dropdown, command palette, Quake
mode). What remains is the *design-language* gap: Nexterm's chrome is drawn
from ad-hoc constants — flat offset-quad shadows, two competing rounded-rect
implementations, no shared widget visuals, no hover/press motion, no icon
system. This plan grounds the visual and interaction language in the official
Windows 11 design guidance (Fluent Design):

- Design principles (Effortless / Calm / Personal / Familiar / Complete +
  Coherent): <https://learn.microsoft.com/windows/apps/design/design-principles>
- Signature experiences: geometry, layering & elevation, materials, motion,
  typography, iconography, color
  (<https://learn.microsoft.com/windows/apps/design/signature-experiences/geometry> etc.)
- App basics: title bar, commanding, dialogs/flyouts, content spacing
  (<https://learn.microsoft.com/windows/apps/design/basics/>)

Strategic differentiator: Windows Terminal is Windows-only by construction.
Nexterm can carry **one coherent Fluent-informed design language across
Linux / macOS / Windows** — something WT structurally cannot do.

## Non-goals

- A DOM/web-based UI (`docs/PRODUCT.md` non-goals). Everything below is
  rendered natively with wgpu + cosmic-text.
- Tab-contains-panes model (ADR-0005: one tab per pane).
- First-run wizard (still not approved), font presets, OKLCH/wide gamut.
- Editing the `docs/PRODUCT.md` vision now — revisit with a small follow-up PR
  once P1/P2 have shipped.

## What already exists (do not redo)

- WT-parity UX shipped through v1.15.0 (see the two archived plans above).
- Color design tokens: `DesignTokens` — 21 color fields derived from
  `SchemePalette` (`nexterm-config/src/schema/tokens.rs`).
- SDF rounded rectangles: `add_px_rounded_rect_sdf`
  (`nexterm-client-gpu/src/vertex_util.rs:144`) — used by tab pills and the
  settings scrollbar.
- Animation groundwork: `AnimationManager` (`animations.rs`) with spring +
  eased timers (tab accent, inactive-pane dim, pane fade-in) and the
  `off/subtle/normal/energetic` intensity config.
- AccessKit tree (`accessibility.rs`); WCAG contrast helper `ensure_readable`
  (settings panel only, `overlay/settings/row.rs:42`).
- Windows backdrop: `DWMWA_SYSTEMBACKDROP_TYPE = 4` (Mica Alt), applied
  unconditionally (`platform.rs:12`, called from `lifecycle.rs:109`).
- Shipped 2026-07-30/31 while this plan was in review (parallel plan
  `plans/archive/2026-07-30-wt-titlebar-and-overlay-fix.md`), all released
  as **v1.16.0 (2026-07-31)**:
  - Overlay draw order: the frame renders in two layers (grid, then
    overlays), so grid glyphs no longer bleed through panels (#44).
  - `decorations = "notitle"` implemented as a WT-style custom title bar:
    borderless window, tab bar doubling as the title bar, min/max/close
    buttons exposed to AccessKit, double-click maximize, native system
    menu on Windows. Secondary windows keep native decorations (#46).
  - SDF rounded corners for every overlay panel and chrome corner radius
    default 6 → 10 px; the legacy three-rect helper was removed (#47).
  - In-app system menu for the custom title bar on Linux/macOS (8-locale,
    honours `window.close_action`); Wayland `drag_window` /
    `drag_resize_window` failures are logged instead of swallowed (#48).
  - Windows 11 snap layouts on the custom maximize button: the window proc
    is subclassed to answer `WM_NCHITTEST` with `HTMAXBUTTON` — the P7
    spike question, answered in production (#49).
  - `window.decorations` now defaults to `"notitle"` on Windows/Linux;
    macOS keeps `"full"` (winit cannot start an interactive resize
    there) (#50).

## Identified gaps

| ID  | Gap | Evidence | Impact |
|-----|-----|----------|--------|
| G1  | Pipelines blend straight alpha while the surface is `CompositeAlphaMode::PreMultiplied` (Issue #35) — **closed by #45 (2026-07-31)** | `renderer/wgpu_init.rs:172,246,294`; comment at `overlay/settings/mod.rs:98-109` | ★★★ blocks all translucency work |
| G2  | Two rounded-rect systems; the legacy 3-rect one visibly breaks above 8 px radius — **closed by #47 (2026-07-30)** | `vertex_util.rs:302` vs `:144`; `overlay/util.rs:83-114` | ★★ |
| G3  | Shadows are flat single-color offset quads; no softness, no elevation scale | `overlay/util.rs:82-94` | ★★★ |
| G4  | Tokens cover color only; spacing/typography/motion/elevation are ad-hoc constants | `tokens.rs:27-81`; `row.rs:132-135` (`cell_w * 0.6` …) | ★★★ |
| G5  | No shared widgets: 7 per-tab focus counters; draw / input / AccessKit triple-defined per control | `settings/mod.rs`; `input_handler/mod.rs:590-780`; `accessibility.rs:1590-1824` | ★★★ |
| G6  | No hover/press/open-close motion; no OS reduced-motion detection | `animations.rs` | ★★ |
| G7  | Backdrop hard-coded to Mica Alt; `macos_window_background_blur` is dead config; Linux has nothing | `platform.rs:12`; `schema/window.rs:194` | ★★ |
| G8  | Icons are Unicode glyphs (`▶ Aa ◐`) / opt-in Nerd Font | `settings/category.rs:55-65`; `tab_icons.rs` | ★★ |
| G9  | No tooltip component anywhere | repo-wide grep | ★★ |
| G10 | Contrast guard (4.5:1) exists only inside the settings panel | `overlay/settings/row.rs:42` | ★★ |
| G11 | Hard-coded colors bypass tokens: IME preedit, cursor, OSC 9;4 progress, theme-preview palette duplication | `render_frame.rs:1121,1133`; `vertex_util.rs:389-417`; `ui_verts.rs:1482-1485`; `theme_tab.rs:100-108` | ★ |

## Adopted Fluent reference values

All values verified against Microsoft Learn (see References).

| Domain | Values |
|---|---|
| Corner radius | 8 px top-level surfaces (window, dialog, flyout); 4 px in-page controls; 0 px where edges intersect |
| Spacing ramp | 8 / 12 / 16 / 32 / 48 epx |
| Type ramp (chrome only, never the terminal grid) | Caption 12/16, Body 14/20, Body Strong 14/20 SB, Subtitle 20/28, Title 28/36 |
| Motion durations | 83 / 167 / 250 / 333 ms |
| Motion curves | Direct Entrance `cubic-bezier(0,0,0,1)`; Existing Elements `(0.55,0.55,0,1)`; Gentle Exit `(1,0,1,1)`; Bare Minimum: linear 83 ms opacity |
| Elevation scale | Dialog 128, Flyout 32, Tooltip 16, Card 8, Control 2 (pressed sinks to 1) |
| Materials | Mica ≙ long-lived surfaces; Acrylic ≙ transient surfaces (menus, flyouts) |
| Contrast | ≥ 4.5:1 body text, ≥ 3:1 large text; high-contrast themes target ≥ 7:1 |

## Phases

### P0 — Compositing correctness + SDF unification (S–M)

Fixes G1 (Issue #35) and G2 before anything translucent is built on top.

> **Status 2026-07-31: P0 is complete.** The SDF-unification half (G2)
> shipped separately in #47; the compositing contract (G1) shipped in #45.
> The section below is kept as originally scoped for the record.

**Compositing contract** (the rule every later phase relies on): all fragment
shaders output **premultiplied alpha**; all pipelines use
`BlendState::PREMULTIPLIED_ALPHA_BLENDING`; the clear color premultiplies
`background_opacity` into RGB. Custom `gpu.custom_bg_shader` /
`custom_text_shader` must premultiply their output — breaking change, called
out in CHANGELOG and in the config doc comments.

- Touchpoints: `shaders.rs` (3 shaders), `renderer/wgpu_init.rs:172,246,294`,
  `renderer/shader_reload.rs:101,148`, `render_frame.rs` (clear color),
  `nexterm-config/src/schema/gpu.rs` (doc comments).
- SDF unification: replace the legacy `add_rounded_px_rect`
  (`vertex_util.rs:302`) call sites — starting with `draw_overlay_panel`
  (`overlay/util.rs:83,99,114`) — with `add_px_rounded_rect_sdf`, then delete
  the legacy function. Radius > 8 px now renders correctly (deliberate visual
  improvement).
- Tests: full suite; any vertex-identity test that asserts the old 3-rect
  output is updated deliberately with rationale in the commit message.
- Acceptance: with `background_opacity < 1.0` the window composites without
  the washed-out fringe described in #35 (manual check on Windows); no clippy
  warnings; opaque default path visually unchanged.

### P1 — Metric tokens + shared widget layer (XL, 2–3 PRs)

Addresses G4, G5, G9, G11 (principles: Effortless, Familiar, Coherent).

- New `nexterm-config/src/schema/metrics.rs`: `MetricTokens` { spacing ramp,
  radius (control 4 / overlay 8, bridged from the existing
  `UiConfig.corner_radius_*` for backward compatibility), chrome type ramp,
  elevation scale, motion durations + easing table }. Kept separate from the
  palette-derived color `DesignTokens`.
- New `renderer/overlay/widgets/` module: a `WidgetSpec` descriptor
  (immediate-mode, rebuilt per frame) consumed by exactly three readers —
  `draw_widget` (visuals: real toggle pills, chevrons, sliders, focus ring),
  key/mouse routing via a single focused-widget field, and the AccessKit tree
  builder. This collapses the 7 per-tab focus counters and the triple
  definition of every control. (Shipped as `focused_widget_index: u16` rather
  than a full `WidgetId`: the category is already in `SettingsPanel.category`,
  and storing it twice invites the two copies drifting apart.)
- First tooltip component (Tooltip elevation 16, radius 4).
- Migrate Theme and Window tabs first; remaining tabs follow tab-by-tab with
  old and new coexisting.
- Migrate G11 hard-coded colors onto tokens; delete the theme-preview
  duplicate palette.
- Acceptance: every migrated control shares identical hover/press/focus
  visuals; `input_handler` and `accessibility.rs` shrink measurably; a
  tooltip appears on at least the settings sidebar icons.

### P2 — Depth & materials (XL)

Addresses G3, G7 (principles: Calm, Familiar).

- Extend the BG shader with `shadow_softness` and `stroke_width` vertex
  attributes (5 → ~8 attributes; wgpu default max is 16) → soft shadows
  scaled by the elevation table, 1 px focus rings without rect stacking.
- **In-app acrylic**: render the scene into a persistent offscreen
  `scene_color` texture, run a 4–5 tap Kawase downsample blur chain, and let
  overlay panels sample blur + tint + noise. Captured **once per overlay
  open** and reused (no per-frame recapture → independent of the
  audit-round3 P3 vertex-rebuild debt; no per-frame buffer allocation).
- New `window.backdrop` config: `auto | mica | mica-alt | acrylic | none`.
  Windows maps to `DWMWA_SYSTEMBACKDROP_TYPE` (turns today's hard-coded
  Mica Alt into a choice); macOS resolves the dead
  `macos_window_background_blur` config (via a vetted crate such as
  `window-vibrancy` or direct objc2 — license/maintenance check first);
  Linux falls back to `none` + the in-app blur.
- Acceptance: settings panel / palette / dialogs show visibly different
  elevation weights; overlays blur the terminal behind them when
  `backdrop != none`; all three OSes degrade gracefully.

### P3 — Motion language (M–L)

Addresses G6 (principles: Effortless, Calm).

- Add `Timed { start, duration, curve }` animations with the Fluent
  cubic-bezier table to `AnimationManager` (springs stay for interruptible
  motion such as the tab accent).
- Apply to widget hover/press, dialog/flyout/tooltip open-close (Direct
  Entrance in, Gentle Exit out).
- OS reduced-motion detection: Windows
  `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`; macOS
  `accessibilityDisplayShouldReduceMotion` (shares the objc2 dependency
  decision with P2); Linux: manual `animations.enabled` config remains the
  fallback. Detection only ever *disables* motion.
- P3 ships in three PRs: **P3a** motion foundation (`Timed`, the Fluent
  curve/duration tables, animation-driven redraw, settings-panel open/close),
  **P3b** widget hover/press and overlay open-close, **P3c** OS
  reduced-motion detection.
- Acceptance: the idle pane-vertex-cache miss rate does not regress
  (measured with the counter added in P3a — `NEXTERM_LOG=trace`); with
  reduced motion on, every animation renders instantly. The criterion
  previously named `build_pane_vertices`, which does not exist in the
  codebase; the C4 pane cache miss is the equivalent, and the cursor-blink
  invalidation debt behind it stays tracked in
  `plans/audit-round3-2026h2.md` P3.

### P4 — Iconography & chrome typography (M)

Addresses G8 (principles: Familiar, Personal).

- Bundle a subset of `microsoft/fluentui-system-icons` (MIT — add license
  text to third-party notices) via `include_bytes!` +
  `fontdb::Database::load_font_data`, registered under an explicit family
  name. Nerd Font pipeline stays untouched for terminal-content icons.
- Replace the settings sidebar Unicode glyphs and menu glyphs; icon sizes
  follow the 16/20/24 px steps.
- Apply the chrome type ramp (Caption/Body/Subtitle/Title) via
  `FontManager`; the terminal grid keeps the single user-configured font.
- Acceptance: all chrome icons render without any user-installed font.

> **Design spec (2026-08-29):**
> `plans/2026-08-29-p4-iconography-and-chrome-typography.md`. It splits the
> phase into P4a (icon font) and P4b (type ramp) over four PRs, and corrects
> this entry's **M** sizing: the ramp above has never had a reader, and the
> chrome has no variable-size text path at all — every chrome string advances
> by `cell_w`. P4b therefore adds a proportional run primitive and adopts it at
> a bounded list of six surfaces, rather than "applying the ramp".

### P5 — Accessibility & high contrast (M)

Addresses G10 (principle: Complete + Coherent).

- Move `ensure_readable` out of the settings panel into shared color
  utilities; apply to tab bar, dialogs, palette, blocks UI, status bar.
- New built-in high-contrast scheme (target ≥ 7:1).
- Automated contrast tests: every built-in scheme × every token pairing used
  for text must pass 4.5:1.
- AccessKit roles/status flow from `WidgetSpec` (P1) instead of per-control
  re-definition.

> **Design spec (2026-08-29):**
> `plans/2026-08-29-p5-contrast-and-high-contrast.md`. It corrects three of the
> four bullets above. The AccessKit bullet **already shipped** in P1b/P1c
> (`accessibility.rs:1450-1506` derives `Role` and state from `WidgetDesc`), so
> it is dropped. The defect is **systemic rather than per-scheme**: `text_muted`
> fails 4.5:1 on all nine built-ins at every surface level, and Light's semantic
> green/yellow reach 1.18:1. And `ensure_readable` raises alpha only, so
> relocating it — the first bullet — would not move a single number. The fix
> moves into `DesignTokens` derivation (per-surface corrected text tokens; raw
> semantic/accent tokens kept for their dominant fill role), over four PRs
> P5a–P5d.

### P6 — Notification surfaces (M)

Principles: Calm, Personal. Fluent surface model: Dialog = blocking
confirmation of destructive/irreversible actions; Flyout = light-dismiss
contextual UI; InfoBar = non-blocking status.

- New `overlay/infobar.rs`: non-blocking, auto-dismissing banner (reuses the
  update-banner slot pattern in `ClientState`).
- ~~Reclassify `ConsentDialog` kinds: keep genuinely destructive/security
  confirmations modal; move low-risk notices to InfoBar.~~ **Dropped
  2026-08-29** — see the design spec §4. All three consent kinds are pre-action
  authorisations carrying pane-controlled content; moving one to a non-blocking
  surface changes security posture rather than presentation, and
  `ConsentPolicy = "allow"` is already the per-category opt-out.
- New strings ×8 locales.

> **Design spec (2026-08-29):**
> `plans/2026-08-29-p6-notification-surfaces.md`. It corrects two of the three
> bullets above. There is no single "update-banner slot" to reuse: Nexterm
> already ships **three** top-of-screen banners (update / offline / error) with
> three state fields, three builders totalling 232 lines, and hand-written
> stacking arithmetic in each — so P6 is a **consolidation, not an addition**.
> The measurement also found an accessibility defect: `error_banner` appears
> **zero times** in `accessibility.rs`, so shell-launch and config-load failures
> are never announced. The consent-reclassification bullet is **recommended
> dropped** — all three consent kinds are pre-action authorisations, and moving
> one to a non-blocking surface changes security posture rather than
> presentation; `ConsentPolicy = "allow"` is the existing opt-out. **Signed off
> 2026-08-29**, so P6 touches no security-relevant code. "New strings ×8" turns
> out to be one string. Four PRs P6a–P6d.

### P7 — Full custom title bar (spike S, then XL)

Principle: Familiar (WT-style tabs-in-titlebar silhouette). High risk —
gated behind a spike.

> **Status 2026-07-31: P7 has effectively shipped ahead of schedule**
> (v1.16.0). #46 delivered the base `notitle` title bar; #48 the in-app
> system menu for Linux/macOS; #49 answered the spike question in
> production (WndProc subclassing + `WM_NCHITTEST`/`HTMAXBUTTON` snap
> layouts coexist with winit's pump); #50 made `notitle` the
> Windows/Linux default. The spike gate below is **closed**. Remaining
> scope is absorbed into earlier phases: Fluent-spec caption-button
> states/metrics (32 px bar, 5 states) → P1 widget layer + P3 state
> motion; macOS traffic-light coexistence → revisit with P5; Linux CSD
> limitations → the P1-adjacent CONFIGURATION.md inventory PR. The
> section below is kept as originally scoped for the record.

- **Spike first (go/no-go gate)**: winit 0.30 does not expose
  `WM_NCHITTEST`; snap layouts on Windows 11 require subclassing the window
  proc (`SetWindowLongPtrW(GWLP_WNDPROC)`) and coexisting with winit's
  message pump. Prove hit-testing + snap-layout flyout + no event-loop
  regressions in a throwaway branch before committing to the phase.
- Implementation after go: `decorations = none` + custom caption buttons
  (min/max/restore/close, 5 visual states, 32 px bar) + drag regions
  (`w.drag_window()` already proven at `mouse.rs:1147`) + double-click
  maximize/restore + right-click system menu; macOS keeps native traffic
  lights over a hidden title bar; Linux documents CSD limitations.
- Unsafe code needs a safety review; every `unsafe` block carries a
  `// SAFETY:` comment.

## Cross-cutting rules

- Every PR updates `docs/CONFIGURATION.md` for the config fields it touches.
  One inventory PR (S) around P1 backfills the currently undocumented
  fields (`[ui]`, `[animations]`, `[cursor]`, `[quake_mode]`, parts of
  `[window]` / `[tab_bar]`).
- Every new user-facing string lands in all 8 locales with the key-parity
  test green.
- `cargo clippy -- -D warnings`, `cargo fmt --check`, full test suite green.
- GPU output is not CI-verifiable → hand-run screenshots under
  `docs/img/uiux-v3/`.
- Whenever `Cargo.lock` changes, run
  `bash scripts/regenerate-flatpak-sources.sh` and commit the result.
- Breaking changes to `custom_bg_shader` / `custom_text_shader` are called
  out in CHANGELOG.

## Risks and mitigations

- **Offscreen rendering is new to this codebase** (P2): resize/DPI handling
  and capture invalidation are the traps. Mitigate with persistent textures,
  capture-once semantics, and a tracing counter on blur-chain executions.
- **audit-round3 P3 debt** (cursor blink rebuilds pane vertices): P2's
  capture-once design avoids coupling; P3's acceptance criterion guards
  against regression. The debt itself stays tracked in its own plan.
- **macOS backdrop dependency**: adding `window-vibrancy`/objc2 needs a
  license + maintenance check and a flatpak sources regeneration.
- **P7 WndProc subclassing**: unsafe, environment-sensitive; proven in
  production by #49 (spike closed 2026-07-31).
- **Vertex-identity regression tests** will legitimately change in P0/P1;
  update them deliberately, never delete them.
- **Translation quality** beyond en/ja needs native review eventually
  (existing, not new, debt).

## Release mapping

| Phase | Release |
|---|---|
| P0 | v1.17 (v1.16.0 shipped 2026-07-31 carrying #46–#50, before P0 merged) |
| P1 | v1.17–v1.18 |
| P2 | v1.18–v1.19 |
| P3 | v1.19 |
| P4 | v1.19–v1.20 |
| P5 | v1.20 |
| P6 | v1.21 |
| P7 | mostly shipped in v1.16.0 (#46/#48/#49/#50); remainder folds into P1/P3/P5 |

## Progress

- [x] P0 (SDF half) rounded-rect unification — shipped via #47 (2026-07-30)
- [x] P0 (compositing half) premultiplied-alpha contract — shipped via #45
      (2026-07-31; the on-device visual checks listed in that PR were never
      run — see the on-device verification backlog at the end of this section)
- [x] P1a metric tokens (`metrics.rs`) — `MetricTokens` with the spacing,
      radius, type, elevation and motion ramps
- [x] P1b widget layer + tooltip — `renderer/overlay/widgets/`, with
      `WidgetDesc` (semantics) / `WidgetSpec` (semantics + layout) consumed by
      the renderer, the hit-test and the AccessKit tree
- [x] P1c tab migration — **9 of 9 done** (Theme, Window, Font, Startup,
      Blocks, Security, Profiles, Ssh, Keybindings). Follow-ups below:
  - [x] Foundation for the list-shaped tabs — `draw.rs` split into `draw/`
        by control family (it was at 779 of the 800-line guideline);
        `WidgetId.index` widened to `u16` so a category can address one widget
        per user-created list entry; `WidgetKind::{ListItem, Button,
        KeyCapture}` added and `Text` given a real `caret` offset (the old
        renderer appended `_` and ignored `TextInputState.cursor`, which also
        affected the already-migrated Security and Startup fields)
  - [x] Ssh / Keybindings / Profiles — the three share one shape: a list, an
        edit panel for the selected entry, Add/Delete, and a delete dialog.
        Migrated smallest first (Profiles → Ssh → Keybindings). Profiles
        shipped via #56: first `ListItem` consumer, retires the hand-written
        `SettingsProfileItem` AccessKit range, and fixes the latent
        hit-test-vs-scroll offset mismatch for every migrated tab. Ssh
        shipped via #57: adds `WidgetKind::SpinButton` and disabled-widget
        AccessKit support, windows the entry list via `list_window`, retires
        the 800M host-item range and fixed ids 40..=46, and heals the
        event-route gap that made the dialog/button dispatch arms
        unreachable. Keybindings closes the set: first `KeyCapture` consumer,
        adds `WidgetDesc.invalid` (a typo'd action stays red and is announced
        as invalid, which the hand-written renderer did by hand), retires the
        900M binding-item range and fixed ids 50..=53, gives the leader-key
        row its first AccessKit node at all, and feeds `leader_key` into the
        tree-diff hash so an in-flight edit reaches a reader. Each delete
        dialog is a modal over the whole panel, not a settings row, and stays
        on its existing hand-written AccessKit nodes for now
  - [x] Bounded list viewport — shipped via #54: the Ssh and Keybindings
        lists window to `MAX_LIST_ROWS` (8) rows around the selection with a
        range-indicator row (`list_window` in `layout.rs`), anchoring the
        edit panel right below the windowed list. Predates the widget layer
        and was kept separate from the list-tab migration on purpose
  - [x] Collapse the per-tab focus counters into one field — the seven
        `<tab>_field_focus` counters are now a single
        `SettingsPanel.focused_widget_index` (a `WidgetId.index` for the current
        category). Every category change goes through the new
        `set_category`, which resets the index, the scroll offset and any
        in-flight field edit; before this, the keyboard paths reset all seven
        counters by hand and the sidebar click reset none of them, so a click
        during an edit left the buffer live but invisible
  - [x] Derive keyboard navigation from the descriptors — `widgets/navigation.rs`
        walks the current category's descriptors, skipping anything that is not
        a focus stop (`!enabled`, `Label`, and `Swatch`, which is a redundant
        mouse affordance for the cycler row above it). The five identical
        `next_<tab>_field` / `prev_<tab>_field` pairs and four
        `<tab>_FIELD_COUNT` constants are gone; a category now gains keyboard
        navigation by describing its controls. Ssh and Keybindings keep bespoke
        arrow handling: index 0 addresses their entry list as a whole, which a
        plain descriptor walk cannot express — migrating that convention is its
        own change. Profiles and Blocks have no focus ring to preserve
  - [x] Hard-coded colour migration (G11) — shipped via #62. The four
        plan-scoped sites now read `DesignTokens`: the IME preedit
        (`surface_3` backdrop, `accent_primary` composition underline,
        `text_primary` text — previously a dark-only gray-and-yellow set),
        the cursor (`draw_cursor_with_visibility` takes a scheme-derived
        base colour, `text_primary`, keeping the per-shape alpha — the
        hard-coded white cursor was invisible on light schemes; the dead
        legacy `draw_cursor` wrapper is gone), the OSC 9;4 progress bar
        (`semantic_success` / `semantic_error` / `text_muted` /
        `semantic_warning` via the new `color_util::with_alpha`), and the
        theme-preview swatches, now derived from the canonical
        `BuiltinScheme::palette().bg` — the hand-copied table had visibly
        drifted (dark and gruvbox most) and a new test pins the swatches to
        the scheme list
  - [x] G11 follow-up — the out-of-plan sites the #62 sweep surfaced,
        shipped in three PRs split by surface so each stayed reviewable.
        Shadow quads (`tooltip.rs`, `overlay/util.rs`) were and remain G3's
        scope, not G11's
    - [x] Terminal surfaces — #69. The selection highlight was
          `[0.25, 0.55, 1.0, 0.40]` duplicated in both `grid_verts`
          builders and had already drifted into a third value in copy mode
          (`[0.40, 0.65, 1.0, 0.45]`); all three now call
          `color_util::selection_color`, so the values cannot drift again.
          Copy mode's block cursor and the pane-number badge move onto
          `semantic_warning`, and the tab-rename edit text onto
          `text_primary` (it was white over the pale `surface_3` a light
          scheme gives it). The badge needed a second helper,
          `color_util::on_surface_text`: `text_on_accent` is derived from
          `accent_primary`'s luminance, so on a dark-accent scheme it picks
          a *light* label and puts it on a pale yellow badge
    - [x] Modal scrim + destructive-button reds — #70. The scrim was not
          "one literal in four files" but an asymmetry: five surfaces veil
          the screen behind a modal and only the settings panel derived its
          veil from the scheme, so on a light scheme a black veil sat behind
          a light panel. All five now call `util::scrim_color`, keeping
          `surface_0` for the reason the panel already documented in place;
          alpha stays a parameter because the panel fades its scrim in with
          the open animation. The delete-dialog reds had drifted further
          than the plan recorded — not only the focused fill (`[0.498,
          0.196, 0.196]` against `[0.486, 0.180, 0.180]`) but the resting
          treatment (dark red against `surface_1`) and the label rule.
          Both tabs now call `row::danger_button_colors`
    - [x] Modal dialog buttons — #71. The close-window Kill/Cancel fills
          and the consent dialog's selected-button fill, via the new
          `util::danger_fill` / `util::caution_fill`. Kill deliberately does
          *not* reuse `danger_button_colors`: that helper answers "is this
          focused", while Kill must read as destructive *before* selection,
          so it steps between two blend strengths instead. Measuring the
          labels across all nine built-in schemes produced the one finding
          worth carrying forward: **no fixed blend strength works.** The
          error hue at 0.85 lands at a middling luminance on Nord (4.42:1)
          and `semantic_warning` used raw does the same on Solarized
          (4.37:1) — luminances where *neither* a near-black nor a
          near-white label has anything to contrast with. `semantic_fill`
          therefore walks the blend back toward `surface_1` until the label
          clears `MIN_TEXT_CONTRAST`, the same shape as
          `row::ensure_readable`, trading a slightly quieter fill on some
          schemes for a label that is always readable
    - [ ] Remainder — two sites stay out, both deliberately, and neither is
          a mechanical migration:
      - `overlay/picker.rs`'s query/selection colours. The purple/green
        macro & SSH branding is intentional per its code comments, so
        migrating it is **a product decision about whether Nexterm keeps
        per-feature brand hues at all**, not a token substitution
      - `color_util::resolve_color`'s no-palette fallbacks (`[0.85, …]` /
        `[0.05, …]`). `render_frame` builds `scheme_palette` as an
        unconditional `Some`, so the `None` arm is reachable only from
        tests: tokenising it would change nothing that renders. (The same
        line makes the `DesignTokens::default()` branch beside it dead
        too — worth folding into a future cleanup, not into G11)
- [x] CONFIGURATION.md inventory PR — shipped via #73. `Config` has 36 fields; the
      document covered 13 and described ten keys that never existed in the code.
      `nexterm-config/tests/doc_matches_schema.rs` now fails if a documented key
      is not a real field, but `complete_example_uses_only_real_config_keys`
      only walks the example's **top-level** keys, so a key nested under
      `[window]` is invisible to it — it did **not** catch P2c's removal of
      `macos_window_background_blur`. The guard that did is
      `removed_phantom_keys_do_not_return`, which scans the document textually
      at any nesting depth and which Task 2 armed by adding the key to its
      `PHANTOM` list
- [x] P2a soft shadows + stroke attributes — shipped via #63. The BG shader
      gains `shadow_softness` / `stroke_width` vertex attributes (5 → 7;
      additive for `custom_bg_shader` — a 5-attribute custom shader keeps
      validating because wgpu checks that shader inputs are a subset of the
      layout). `spread = max(shadow_softness, 0.5)` reproduces the pre-P2a
      1 px AA exactly when the extensions are off, so plain fills are
      bit-identical. `draw_overlay_panel` and the tooltip now derive their
      shadows from the elevation table via `shadow_params` (offset =
      elevation/16, softness = elevation/8, alpha 0.45 — initial mapping,
      needs on-device tuning): dialogs sit at `dialog` (128), the context
      menu / pickers / settings panel at `flyout` (32), the tooltip keeps
      `tooltip` (16). CI now parses and validates all three built-in WGSL
      shaders through the `wgpu::naga` re-export — the first shader check
      that runs without a GPU
  - [x] P2a follow-up — `stroke_width` has a consumer: `draw_focus_ring`'s
        two bands are real outline quads via the new
        `vertex_util::add_px_stroke_sdf` (tight quad — an outline band hugs
        the *inside* of the rect edge, so unlike the soft shadow it needs no
        growing; `width` clamped to half the shortest side, past which the
        opposite bands fold back and carve a hole out of the centre). The
        geometry is unchanged, so `list::focus_rect`'s inset still holds. Two
        deliberate consequences: the area inside the ring is no longer
        repainted (it was covered again by the row fill and the control —
        harmless while every surface is opaque, a double blend once P2b's
        acrylic is not), and the accent/surface boundary gains a shared
        half-pixel of AA instead of an opaque butt joint (**on-device check**)
  - [ ] P2a follow-up (deferred) — the 1 px border ring in
        `draw_overlay_panel` is the same stacked-fill idiom, but its ring
        colour is `border_default` at 18 % alpha, so a stroke would change
        the visible band's alpha profile (a fill is opaque up to the panel
        edge; a stroke fades on both sides). Unlike the focus ring this is a
        real visual change on a surface that appears behind every overlay, so
        it waits until the P2a shadow constants are tuned on-device rather
        than stacking another unverified change
- [x] P2b in-app acrylic (offscreen + Kawase blur) — shipped via #74
- [x] P2c `window.backdrop` config (Win/macOS/Linux) — shipped via #75.
      **This closes P2.** The Windows material was mislabelled Acrylic
      throughout while applying Mica Alt (`DWMSBT_TABBEDWINDOW` = 4, against
      `DWMSBT_TRANSIENTWINDOW` = 3 for Acrylic); `auto` keeps applying Mica Alt
      so the correction changes no appearance. macOS resolves every non-`none`
      value to one `NSVisualEffectView` material, since AppKit has no
      Mica/Acrylic distinction; Linux resolves everything to `none` and leans
      on P2b's in-app blur. Two latent defects closed alongside: a requested
      backdrop now makes the window transparent on its own (previously only
      `background_opacity < 1.0` did, so a backdrop on an opaque-configured
      window could never show), and `spawn_os_window`'s secondary windows get
      the backdrop at all (they never had one). The dead
      `macos_window_background_blur` field is removed. `WindowBackdrop::resolve`
      takes the target OS as a parameter rather than reading `cfg!`, and
      `dwm_backdrop_value` is a plain `const fn` compiled everywhere, so the
      Windows and macOS routing tables and every DWM constant are asserted on
      the Linux runners — a wrong constant fails there instead of reaching a
      Windows release
- [ ] P3 motion language + reduced-motion detection
  - [x] P3a motion foundation — shipped via #77. `Timed { start, duration_ms,
        curve }` plus the nine Fluent 2 cubic-bezier curves and eight duration
        steps, transcribed from `microsoft/fluentui` `packages/tokens` (the
        Fluent 2 design site documents motion qualitatively and publishes no
        token values). `AnimationManager::has_active_animation` had been dead
        code since it was written, so a spring mid-flight only advanced when an
        unrelated redraw happened; `ClientState::has_active_animation` now
        aggregates it and the event loop requests a frame only while it is true
        — an idle terminal asks for exactly the frames it asked for before.
        The settings panel is the first consumer: its entrance was a
        frame-count hack (`open_progress += 0.15`, "assumes 60 fps") that
        ignored `animations.intensity` and had no exit at all; it is now a
        200 ms Direct Entrance and a 150 ms Gentle Exit, both intensity-scaled.
        `is_open` stays the single truth for input routing and the AccessKit
        tree and still goes false the instant the user dismisses the panel —
        the new `closing` field is render-only, which kept the change out of
        `accessibility.rs` and the input path entirely. Two corrections landed
        with it: the acceptance criterion below named a `build_pane_vertices`
        that does not exist, and the design spec had `Timed` delegate to
        `compute_progress`, whose `Duration::as_millis()` truncation quantises
        the recovered curve parameter enough to move `Curve::AccelerateMax` by
        ~0.1 near its tail — the exact curve and duration the panel exit uses,
        so reopening mid-fade would have visibly jumped
  - [ ] P3b widget and overlay motion
    - [x] P3b1 overlay open/close motion — shipped via #79 (squash
          `5d6e167`), merged to `master`. A shared `SurfaceMotion` open/close
          timer pair drives open/close motion across eleven overlay
          surfaces: six `bool`-shaped ones directly, and four
          `Option`-shaped ones via render-only ghosts (a clone of the
          content that lets the live field go `None` immediately while the
          ghost still animates out), plus the settings tooltip, which has no
          stored openness and is instead driven from a hover-dwell
          predicate. The password modal's ghost is redacted:
          `PasswordModalGhost` carries only `input_len: usize`, no password,
          and a `PasswordModalView` makes that boundary structural rather
          than advisory
    - [ ] P3b2 hover cross-fade — P3b2a (the settings-panel rows and the
          context menu) is pending review in #80; P3b2b (the tab bar and
          the window buttons) is pending review in #81. Together they close
          all four of the client's
          pointer-hover models. `HoverTransition<Id>` (`animations/hover.rs`)
          is the shared two-timer cross-fade every model retargets from the
          handler(s) that write its hovered id — the window buttons have
          two, the pointer-motion handler and the Windows snap-layout
          `UserEvent`, and missing the second would have left that path
          snapping with nothing failing to compile. The tab bar's tear-out
          button keeps its boolean hover reader alongside the fading tint,
          because a button drawn at weight 0.05 must stay clickable.
          `HoverTransition::target()` is `#[cfg(test)]`-gated rather than
          carrying an `#[allow(dead_code)]`, following
          `AnimationManager::tick_by_dt`'s precedent: no production caller
          exists, but the method pins that `retarget` actually moved the
          target, a property the weight assertions alone do not cover
    - [ ] P3b3 press pulse — a shared `PressPulse<Id>` (`animations/press.rs`)
          fires a 100 ms one-shot on mouse-down across the four clickable
          chrome models (window buttons, tab bar, settings rows, context menu
          items) rather than a held state, because three of the four commit
          their action on mouse-down and a held pulse would have nothing to
          hold for. The context menu is the exception — it commits on
          release — so its pulse is visible for as long as the button stays
          down, without needing separate handling: the same one-shot timer
          just keeps reporting non-zero weight while the press is recent.
          Each site amends its existing hover weight read to
          `hover.weight(id, now).max(press.weight(id, now))`, so a press
          before the hover cross-fade has caught up still shows full
          intensity, and threads the same `press` value into
          `color_util::press_fill`, which dims and strengthens an *additive*
          fill layer's alpha — the shape every one of the four sites already
          had, since none of them recolours something that is always drawn.
          Two non-goals: per-control elevation (a shadow or lift on press)
          and any change to a foreground colour — press changes background
          fills only, confirmed at each site by keeping the label/glyph
          colour reading the hover weight alone, never the amended one
  - [x] P3c OS reduced-motion detection — `animations.enabled` is now a
        tri-state (`AnimationsEnabled::Auto` / `Yes` / `No`) instead of a
        bool, with a custom `Deserialize` that keeps parsing pre-P3c
        `enabled = true` / `enabled = false` configs unchanged and adds
        `"auto"` as the new spelling. `Auto` is the default: it defers to the
        OS accessibility preference rather than to the client's own opinion,
        so a user who has already told their OS "no motion" does not have to
        tell Nexterm separately. Detection reads Windows'
        `SPI_GETCLIENTAREAANIMATION` and macOS'
        `NSWorkspace.accessibilityDisplayShouldReduceMotion`; Linux has no
        portal or desktop API wired up, so `auto` animates there and the
        manual `true`/`false` setting is the documented fallback. The OS
        value lives in `AnimationsConfig::os_reduced_motion`, a
        `#[serde(skip)]` field the client's platform layer writes via
        `set_os_reduced_motion` — a structural boundary, not just a
        convention, that keeps a machine's transient OS state out of
        `config.toml`. That boundary has a corollary: hot-reloading
        `config.toml` builds a fresh `Config` whose `os_reduced_motion`
        starts unset, so the client explicitly re-carries the last-sampled
        value across a reload — without it, saving the file would have
        silently re-enabled animation the OS had asked to stop. There is no
        native change-notification for the OS preference, so it is sampled
        at startup and again whenever the window regains focus rather than
        pushed on change; a toggle made while the window is already focused
        is not picked up until focus is lost and regained. The settings
        panel's animations row became a three-state cycler, and its `auto`
        entry shows which way it currently resolves ("Auto (normal)" /
        "Auto (reduced)"), reading `os_reduced_motion()` rather than
        guessing. One correction to this plan's own P3 framing: the entry
        above said macOS detection would "share the objc2 dependency
        decision with P2" — that was stale by the time P3c landed, since P2c
        added `window-vibrancy` for the backdrop material, not `objc2`, and
        `window-vibrancy` does not expose the accessibility preference. P3c
        made its own dependency call instead: `objc2` core only, deliberately
        not the typed `objc2-app-kit` subtree, matching the precedent P2b/P2c
        set of pulling in the smallest crate that answers the question at
        hand
- [x] P4a icon font — the bundled `fluentui-system-icons` subset
      (`assets/fonts/`, 3.4 KB from a 2.8 MB upstream face), its rasterisation
      path, and the eighteen chrome sites now drawing from it. Two structural
      changes carried it: `GlyphKey` gained a `FontRole` and a size, because
      Fluent's PUA codepoints sit inside the Nerd Font range and would
      otherwise share a cache slot with terminal-content icons; and
      `lru_cap_from_cell` stopped assuming every atlas entry is one cell.
      Icon sizes come from the 16/20/24 steps scaled by DPI, never from the
      terminal cell, so icon weight no longer drifts with the user's font
      size — with a clamp so a one-cell slot under a small font shrinks the
      icon instead of letting it bleed. Hit regions are untouched throughout.
      **The acceptance criterion — "renders without any user-installed
      font" — is verified only as far as a CPU test can go** (every bundled
      icon rasterises to non-empty ink through the real path); how any of it
      *looks* is unverified and joins the backlog below
- [x] P4b chrome type ramp — `TypeRamp` finally has readers. The chrome gained
      a text-run primitive (`measure_run` / `truncate_run_to_width` /
      `add_run_verts`) that advances by each glyph's measured width at a ramp
      size instead of by `cell_w` at the cell size, and six surfaces adopted
      it: the settings panel title (Title), section headers (Subtitle), every
      text-bearing control in the widget layer (Body / Body Strong), the
      sidebar labels, the tooltip (Caption) and the dialog titles and body
      text. Measurement and drawing share one per-glyph number by
      construction, so a truncation cannot disagree with what is drawn.
      Weight follows D-2 — SemiBold maps to the existing bold flag, because
      the chrome draws in the *user's* terminal font where a real weight-600
      request resolves differently on every machine. Two claims in the plan
      were corrected while building it: this is not really a "proportional"
      path (a monospace terminal font makes Latin advances equal — what the
      ramp buys is *size*, not proportionality), and the tooltip's placement
      function had to change signature rather than just its draw call. Dialog
      button labels and the footer links stay on the cell path because their
      text widths reach click targets. **Every text size in the settings panel
      changed and none of it has been looked at** — see the backlog below
      - [x] P4c follow-up — the footer's `↗` / `↺` links and the dialog button
      labels (2026-08-29). Measuring corrected the spec: the dialog buttons
      reach **no** click target (both dialogs are keyboard + AccessKit only),
      so they were plain typography; the footer links were the real hit-region
      work and now come from one `footer::footer_links` call that the builder
      draws and the hit-test tests against. See the design spec's §8.1. Noted
      but not fixed: the footer links have no AccessKit node at all
- [x] P5 contrast everywhere + high-contrast scheme (P5a–P5d, 2026-08-29)
- [x] P6 InfoBar consolidation (P6a–P6d, 2026-08-29; consent reclassification
  dropped, spec §4). Appearance stays on the on-device backlog below — the
  spec's §7 asks specifically for three bars stacked in a small window.
  - [x] P6a InfoBar model + pure layout — shipped via #92 (2026-08-29)
  - [x] P6b migrate the three banners onto the stack — the three `Option`
    fields and the three builders are gone; the stack draws below the tab bar,
    capped at two bars, and `Enter` acts on the top bar only (2026-08-29)
  - [x] P6c AccessKit nodes ×kind + tree hash + `infobar-more-count` ×8 locales
    — every kind now has a `Role::Alert` node keyed by its slot, so the server
    error is announceable for the first time (assertive; the other two polite),
    bars past the drawn cap are announced in full, and the cap reports the rest
    as `+{count} more` (2026-08-29)
  - [x] P6d entrance/exit motion + auto-dismissal for the info severity — every
    bar fades in and out on its own `Timed`, so one can be leaving while the one
    under it arrives; `Esc`, `Enter` and a successful connect all dismiss rather
    than delete; the update notice retires itself after 20 s while the warning
    and error severities never do; a dismissed bar leaves the AccessKit tree at
    once but is still drawn until its exit finishes; `has_active_animation` goes
    quiet once every bar settles, so a bar counting down its deadline asks for
    no frames (G-idle) (2026-08-29)
- [x] P7 base `notitle` custom title bar — shipped early via #46 (2026-07-30)
- [x] P7 spike: Windows 11 snap layouts — answered in production via #49 (2026-07-31)
- [x] P7 default-on decision — `notitle` default on Windows/Linux via #50 (2026-07-31)
- [ ] P7 remainder — absorbed into P1 (button widget), P3 (state motion), P5 (macOS coexistence)
- [ ] **On-device verification backlog — accepted as unverified (2026-08-21).**
      GPU output is not CI-verifiable, and the visual checks each PR listed have
      accumulated unrun since #45. The maintainer has decided to carry them
      forward rather than block further phases on them. **There is no
      measurement behind this entry: treat every item below as untested, not as
      passing.** What was never looked at:
  - #45 — translucency under the premultiplied-alpha contract
  - #51 / #52 — the shared widget visuals (`Button` outline, `ListItem`
    selection bar, `KeyCapture` accent frame)
  - #54 — the 8-row bounded list viewport and its range-indicator row past
    20 entries
  - #56 / #57 / #58 — the three migrated list tabs as a whole, in particular
    Keybindings: the `KeyCapture` recording state, the `invalid` red, and
    whether the leader-key row, its hint and the duplicate warning overlap
  - #59 / #60 — keyboard navigation in every category (Tab / ↑ / ↓ / Enter);
    the counter collapse and the descriptor walk were matched against the old
    behaviour by reading, then by tests, but never by hand
  - #62 — the four migrated colour sites in both dark and light schemes:
    cursor visibility, IME preedit, the OSC 9;4 bar in each state, and the
    dark / gruvbox swatches
  - #63 — soft-shadow weight ordering and the alpha 0.45 mapping. The
    `shadow_params` constants shipped as an initial recipe explicitly meant to
    be tuned on a real GPU, so these are *expected* to be wrong, not merely
    unchecked
  - #64 — the focus ring's accent/surface boundary, which now shares a
    half-pixel of AA instead of meeting as an opaque butt joint
  - #69 — the selection highlight's new hue per scheme, whether the copy-mode
    cursor stays distinguishable from the selection now that both derive from
    the same palette, and the pane-number badge's label
  - #70 — **the highest-value item in this list.** The scrim is the one place
    the colour sweep changed what an existing surface *looks like* rather than
    where its value comes from: four modals that veiled in black now veil in
    `surface_0`, so on a light scheme they went from a dark veil to a light
    one. Contrast was reasoned about, never seen
  - #71 — whether the adaptive step-back leaves Kill visibly red enough on the
    schemes that trigger it (Nord in particular), and the Cancel-selected fill
    now that it is a blend rather than a flat yellow
  - #74 — P2b in-app acrylic. Not measured on real hardware:
    perceived blur quality and the Kawase tap radius; the carried-over P2a
    risk that `draw_focus_ring`'s stroke-only interior (#64) may
    double-blend against a now-translucent panel fill; frame-time cost of
    the extra offscreen pass + 4-pass blur chain, particularly on
    integrated GPUs; recapture correctness across a real multi-monitor /
    DPI-change transition; whether the fixed-intensity procedural noise
    reads as grain or banding on various panel colours. Four more surfaced
    while building it, not while planning it. What the capture actually
    contains: the captured `scene_color` is the `bg_pipeline`'s pre-overlay
    range — cell backgrounds plus the gradient, chrome bars, and
    pane/copy-mode overlays — by design; the overlay layer, the background
    image, and terminal text glyphs (drawn by other pipelines) are not in
    it, so a blurred panel shows a frosted composite of that chrome, not a
    frosted terminal.
  - P2c — the backdrop materials themselves, on both OSes that have any. Not
    measured: whether Mica, Mica Alt and Acrylic are distinguishable in Nexterm at
    all, given that the terminal paints `background_opacity` over the surface (at
    the 0.95 default only 5% of the material shows through, which may be why the
    Mica Alt that has shipped since v1.1 has never been remarked on); the macOS
    `NSVisualEffectMaterial::UnderWindowBackground` choice, which is an unmeasured
    initial recipe of exactly the same kind as #63's `shadow_params` and #74's
    `ACRYLIC_TINT_OPACITY`; whether an OS backdrop and P2b's in-app acrylic look
    coherent when both are on. What *is* machine-verified: the five-value routing
    table across three OSes, and the DWM constant for each material — the mapping
    is a plain `const fn`, so a wrong constant fails on a Linux runner rather than
    surviving to a Windows release.
    Nobody has seen whether that reads as intended or as a flat colour
    field, and it is the single most likely way the effect disappoints.
    The tint constant: `ACRYLIC_TINT_OPACITY = 0.85`
    (`nexterm-client-gpu/src/renderer/acrylic.rs`) was chosen to match
    Fluent's in-app acrylic and to keep the strength slider monotonic — an
    unmeasured initial recipe, exactly like #63's `shadow_params`, that
    should be expected to need tuning on real hardware rather than merely
    confirming. Contrast under an adversarial backdrop: the shipped test
    (`panel_body_text_clears_contrast_floor_across_acrylic_strengths`, in
    `nexterm-client-gpu/src/renderer/overlay/util.rs`) asserts the 4.5:1
    floor only against scheme-realistic backdrops (`surface_0`/
    `surface_1`), because that is what the captured bg range paints; it
    deliberately does not assert the pure-black/pure-white extreme, since
    no non-zero blur can satisfy that bound on this palette set (Nord's
    `text_secondary` has 0.02:1 of baseline headroom), so a program
    painting a full-screen high-luminance background behind an overlay
    panel can push panel body text below the floor at mid-to-high
    strengths — measured, documented, accepted for a feature that is off
    by default, but never seen. The grain's effect on readability: the
    contrast model excludes the ±1.5% luma procedural dither on the
    grounds that WCAG contrast is a property of the mean background
    rather than a zero-mean per-pixel excursion — sound on paper and
    unverified in the eye; whether the dither degrades small-glyph
    legibility on a real panel is exactly the kind of thing only looking
    can answer. Ships with `in_app_blur_enabled = false` by default
    specifically because none of this is measured yet.
  - P3a — motion is the one thing on this list that a still screenshot cannot
    capture at all, so it is the item most dependent on someone actually
    watching it. Not measured: whether the 200 ms Direct Entrance and 150 ms
    Gentle Exit read as calm or as sluggish at the settings panel's size;
    whether `DecelerateMax` and `AccelerateMax` are the right two of the nine
    curves for a panel this large, given that Fluent's guidance scales duration
    with the element's size and travel; whether reopening mid-fade looks
    continuous in the eye and not merely in the arithmetic. Also unmeasured is
    the acceptance criterion itself: the shipped test proves only that P3a
    requests no redraws the previous code would not have requested, while the
    number the criterion actually names — the idle pane-vertex-cache miss rate
    — needs a real session with `NEXTERM_LOG=trace`. That reading is worth
    taking early: it is the first observation of the cursor-blink invalidation
    debt (`plans/audit-round3-2026h2.md` P3), which has carried a "needs
    measurement" label since it was written and now finally has an instrument.
    One known artefact, accepted rather than unknown: `close()` tears down
    in-flight edit state immediately, so the 150 ms fade renders the panel with
    those edits already cancelled — a text caret disappears as the panel goes.
  - P3b2 — hover cross-fade for the tab bar and window buttons. Not measured:
    whether the tab bar's `+0.06/+0.06/+0.08` brightening is perceptible *as a
    fade* at all, or whether the delta is too small for the transition to
    register — the design flags this as P3b2's open question and assumes
    "leave it"; if it proves invisible, the options are to increase the delta
    (a visual change beyond motion) or to drop the tab model. Also unmeasured:
    whether the Close button **fading** to `semantic_error` rather than
    snapping to it weakens the "this is destructive" signal — the only place
    in P3b2 where the hovered appearance is a warning rather than an
    affordance. A third gap, not a hardware question but one this phase
    surfaced and that belongs on this list rather than only in a scratch
    ledger: the tab bar's `hover_highlight` gate is verified by code
    inspection only, not by test — covering it needs an `EventHandler` test
    harness the crate does not have, which is out of scope for P3b2 and
    deserves its own decision.
  - P4a — every chrome icon in the window changed shape, and nobody has looked
    at one. The CPU test proves each of the eighteen icons rasterises to
    non-empty ink out of the bundled subset, which is a coverage claim, not an
    appearance one. Not measured: whether the caption buttons read as the
    Windows 11 caption set beside a real Windows shell — the stroke weight and
    optical size are the residual risk decision D-1 accepted rather than
    resolved; whether a 16 px step at scale 1.0 is the right weight next to the
    tab labels, or whether the sidebar wants 20 px; whether the slot-fitting
    clamp ever actually fires in practice, and if it does, whether a shrunken
    icon looks deliberate or broken; whether the icons sit optically centred in
    their slots, since `icon_placement` centres the *ink bounds*, and an icon
    whose artwork is asymmetric inside its em box will centre differently than
    a glyph baseline would have; and how any of it looks on a HiDPI display,
    where `icon_px` multiplies the step by the scale factor — a path with no
    test because `scale_factor` only becomes real in `on_resumed`.
  - P4b — **every text size in the settings panel changed, and nobody has seen
    any of it.** This is the largest unverified appearance change in the whole
    phase: the ramp's Body is 14 epx where the chrome previously drew at the
    terminal font size (≈18.7 px at the 14 pt default), so all panel text gets
    *smaller*, while section headers get *larger* (Subtitle 20) and the panel
    title larger still (Title 28). Whether that reads as a hierarchy or as a
    mismatch is exactly the question a screenshot would answer. Also not
    measured: whether Body Strong is distinguishable from Body when both are
    14 px and the only difference is the bold flag (D-2's accepted cost, and
    the place it is most likely to disappoint); whether rows still look
    vertically centred now that the line box is the ramp's line height rather
    than the cell; whether the smaller text still clears the 4.5:1 contrast
    floor *perceptually* at 14 px, since the contrast tests measure colour
    pairs and say nothing about size; whether the tooltip box still hugs its
    caption text; whether the consent dialog looks coherent with a Title-sized
    heading, Body text and cell-sized *buttons*, which is the one deliberate
    size mismatch this phase leaves behind; and all of it at HiDPI, where the
    ramp is multiplied by the scale factor on a path with no test.
  - The cross-cutting rule above still asks for hand-run screenshots under
    `docs/img/uiux-v3/`. That directory does not exist yet. P3a makes the gap
    wider than "nobody took a screenshot": a screenshot cannot show a
    transition, so the motion work needs a capture format nothing in this
    project has established.
  - What *is* machine-verified for the colour work, and did not exist before
    it: the contrast floors are now pinned by tests rather than by reasoning
    — every dialog fill/label pair across all nine built-in schemes (#71),
    the danger button in both focus states (#70), and the badge label (#69).
    Two of those tests fail if a hard-coded literal returns, and #71's fails
    on Nord if the adaptive blend is removed. This does not substitute for
    looking at the result, but it does mean the *readability* claims here
    rest on measurement even though the *appearance* claims do not. P2b's
    readability claim is pinned the same way: one test sweeps all nine
    schemes at five strengths, including the two (Solarized, OneDark) whose
    tokens already fail the floor with no acrylic involved, so the test
    fires if either is ever "fixed" without updating it.

## References

- Design principles: <https://learn.microsoft.com/windows/apps/design/design-principles>
- Guidelines overview: <https://learn.microsoft.com/windows/apps/design/guidelines-overview>
- Geometry: <https://learn.microsoft.com/windows/apps/design/signature-experiences/geometry>
- Layering & elevation: <https://learn.microsoft.com/windows/apps/design/signature-experiences/layering>
- Materials: <https://learn.microsoft.com/windows/apps/design/style/mica>,
  <https://learn.microsoft.com/windows/apps/design/style/acrylic>
- Motion: <https://learn.microsoft.com/windows/apps/design/signature-experiences/motion>,
  <https://learn.microsoft.com/windows/apps/design/motion/timing-and-easing>
- Typography: <https://learn.microsoft.com/windows/apps/design/signature-experiences/typography>
- Iconography: <https://learn.microsoft.com/windows/apps/design/iconography/segoe-fluent-icons-font>
  (values only; the font itself is not redistributable — bundled icons come
  from MIT-licensed `microsoft/fluentui-system-icons`)
- Title bar: <https://learn.microsoft.com/windows/apps/design/basics/titlebar-design>
- Commanding / dialogs: <https://learn.microsoft.com/windows/apps/design/basics/commanding-basics>
- Content spacing: <https://learn.microsoft.com/windows/apps/design/basics/content-basics>
- Accessibility: <https://learn.microsoft.com/windows/apps/design/accessibility/accessible-text-requirements>
