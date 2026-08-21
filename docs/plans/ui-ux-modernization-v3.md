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
- Acceptance: idle `build_pane_vertices` call count does not regress
  (measured with the tracing counter recommended by
  `plans/audit-round3-2026h2.md` P3); reduced-motion ON renders every
  animation instantly.

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

### P5 — Accessibility & high contrast (M)

Addresses G10 (principle: Complete + Coherent).

- Move `ensure_readable` out of the settings panel into shared color
  utilities; apply to tab bar, dialogs, palette, blocks UI, status bar.
- New built-in high-contrast scheme (target ≥ 7:1).
- Automated contrast tests: every built-in scheme × every token pairing used
  for text must pass 4.5:1.
- AccessKit roles/status flow from `WidgetSpec` (P1) instead of per-control
  re-definition.

### P6 — Notification surfaces (M)

Principles: Calm, Personal. Fluent surface model: Dialog = blocking
confirmation of destructive/irreversible actions; Flyout = light-dismiss
contextual UI; InfoBar = non-blocking status.

- New `overlay/infobar.rs`: non-blocking, auto-dismissing banner (reuses the
  update-banner slot pattern in `ClientState`).
- Reclassify `ConsentDialog` kinds: keep genuinely destructive/security
  confirmations modal; move low-risk notices to InfoBar. Consent-surface
  changes are security-sensitive — review scope carefully before moving any
  prompt out of a modal.
- New strings ×8 locales.

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
- [ ] CONFIGURATION.md inventory PR
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
- [ ] P2b in-app acrylic (offscreen + Kawase blur)
- [ ] P2c `window.backdrop` config (Win/macOS/Linux)
- [ ] P3 motion language + reduced-motion detection
- [ ] P4 icon font + chrome type ramp
- [ ] P5 contrast everywhere + high-contrast scheme
- [ ] P6 InfoBar + consent reclassification
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
  - The cross-cutting rule above still asks for hand-run screenshots under
    `docs/img/uiux-v3/`. That directory does not exist yet.
  - What *is* machine-verified for the colour work, and did not exist before
    it: the contrast floors are now pinned by tests rather than by reasoning
    — every dialog fill/label pair across all nine built-in schemes (#71),
    the danger button in both focus states (#70), and the badge label (#69).
    Two of those tests fail if a hard-coded literal returns, and #71's fails
    on Nord if the adaptive blend is removed. This does not substitute for
    looking at the result, but it does mean the *readability* claims here
    rest on measurement even though the *appearance* claims do not.

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
