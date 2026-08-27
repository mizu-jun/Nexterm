# P2b: In-App Acrylic (Offscreen Capture + Kawase Blur)

Status: proposed
Related plan: `docs/plans/ui-ux-modernization-v3.md`, section "P2 — Depth & materials (XL)" (lines 167-187), checklist item "P2b in-app acrylic (offscreen + Kawase blur)" (line 456)
Related prior work: P2a soft shadows + stroke attributes (#63, #64)

## Goal

Give overlay panels (dialogs, flyouts, tooltips) a translucent "acrylic" material —
blurred terminal content behind the panel, tinted by the current color scheme, with
a subtle procedural noise grain — matching the Fluent Acrylic material described in
the plan's design references. Ships as an opt-in, config-only feature in this phase;
the OS-native backdrop materials (Mica, `window-vibrancy`) and the `window.backdrop`
config enum remain out of scope, deferred to P2c.

## Scope

**In scope:**
- Offscreen `scene_color` capture of the terminal grid, once per overlay-open
  transition (not per-frame).
- A 4-5 tap Kawase downsample/upsample blur chain over the captured texture.
- Sampling blur + scheme tint + procedural noise in overlay panel fills, across all
  three elevation tiers already established by P2a: dialog, flyout, tooltip.
- A minimal, P2b-scoped config surface: `window.in_app_blur_enabled` (bool, default
  `false`) and `window.in_app_blur_strength` (f32, `0.0..=1.0`, default `0.6`).
- A settings panel control for both fields on the existing Window tab
  (`settings_window.rs`), plus the required locale strings.
- Contrast tests for panel labels against the tinted/blurred fill, across all nine
  built-in schemes, at both strength extremes.

**Out of scope (deferred to P2c):**
- The `window.backdrop` enum (`auto | mica | mica-alt | acrylic | none`).
- Windows `DWMWA_SYSTEMBACKDROP_TYPE` integration.
- macOS native vibrancy (`window-vibrancy` / objc2) resolving the dead
  `macos_window_background_blur` config.
- Linux's "falls back to `none` + in-app blur" routing logic (P2b's blur engine
  exists independently of that routing; P2c decides when it's used).

## Non-goals

- Continuous, live re-blurring of terminal content while an overlay stays open. The
  capture is a frozen snapshot taken at overlay-open time; this is an intentional
  performance trade-off recorded in the plan (line 176-178), not a defect to fix
  here.
- Configurable blur radius / tap count. The Kawase chain shape is fixed by the plan;
  only overall strength (tint/blur blend) is user-configurable.
- Resolving the P2a focus-ring double-blend risk (see Risks). It is logged to the
  on-device verification backlog, not blocked on.

## Architecture

### New state: `AcrylicState`

A new module (`renderer/acrylic.rs`) owns:
- `scene_color`: an offscreen texture + view, sized to match the swapchain.
- `blur_chain`: a small series of progressively downsampled textures used as
  ping-pong targets for the Kawase passes.
- `blurred_result`: the final blurred texture that overlay panels sample.
- `capture_generation: u64`: bumped on resize/DPI change.
- `last_captured_generation: Option<u64>`: compared against `capture_generation` to
  decide staleness.
- `overlay_open_count` tracking (read from existing overlay state) to detect the
  0→1 transition that triggers a fresh capture.

Resources are lazily created on first use (first frame where an overlay opens with
`in_app_blur_enabled = true`), not eagerly at startup, so the feature costs nothing
when disabled or unused.

### Render flow (`render_frame.rs`)

Today: `clear_pass → background_image_pass → main_render_pass (grid + overlay
layers, scissored, single pass) → image_render_pass`. There is no offscreen render
target anywhere in the current pipeline (confirmed by inspection: `WgpuState` holds
only swapchain-backed views; `background`/`image_textures` are sample sources, never
render targets).

This design **adds** steps without altering the existing swapchain path, so the
no-overlay-open case is unchanged:

1. `clear_pass`, `background_image_pass` — unchanged.
2. Grid layer renders to the swapchain view — unchanged.
3. **New**: if an overlay is open, `in_app_blur_enabled` is true, and the capture is
   stale (per the generation/open-count check above), re-render the same grid
   vertex buffers a second time into `scene_color` instead of the swapchain.
4. **New**: only on that same stale-capture frame, run the Kawase blur chain over
   `scene_color`, producing `blurred_result`.
5. Overlay layer fill: when `in_app_blur_enabled`, sample `blurred_result` at the
   panel's own screen position, then mix it toward the scheme tint by
   `in_app_blur_strength` (0.0 = fully the existing opaque token fill, 1.0 = fully
   blur+tint), and add a fixed-intensity procedural noise on top regardless of
   strength. Noise intensity is not user-configurable — a single knob
   (`in_app_blur_strength`) is kept deliberately, per the plan's minimal-config
   intent. When disabled, fall back to the existing opaque token fill unchanged
   (`draw_overlay_panel`'s current shadow → border → fill).
6. `image_render_pass` — unchanged, still drawn last.

### Panel-fill sampling: chosen approach

Two approaches were considered for how the overlay fill shader gets access to the
blurred texture and switches behavior:

- **Chosen: extend `BgVertex` with one more attribute** (e.g.
  `@location(7) acrylic_mix: f32`), following the same additive pattern P2a used to
  go from 5 to 7 attributes. The single existing `bg_pipeline` branches in its
  fragment shader on this per-vertex value. This preserves the `custom_bg_shader`
  "subset of attributes" compatibility contract exactly as P2a did, and avoids a
  second pipeline/bind-group to maintain.
- **Rejected: a dedicated second pipeline** for overlay-panel fills only. Cleaner
  separation of concerns, but adds pipeline and bind-group bookkeeping for the
  overlay-only code path, and departs from the attribute-extension idiom the
  codebase already established in P2a.

### Nested overlays

Only one `scene_color` capture exists at a time, and it always holds the terminal
grid — never a previously-opened overlay panel. A flyout opened on top of a dialog
(e.g. a context menu from within the settings panel) blurs the terminal, the same
as the dialog beneath it does. This was confirmed with the maintainer as the
intended behavior; it keeps the implementation to a single shared capture with no
stacking/generation-per-overlay bookkeeping.

### Tint and noise

- Tint reuses the existing blend-strength idiom (`danger_fill(tokens, strength)` /
  `semantic_fill`, established in the G11 follow-up PRs #70/#71): mix the sampled
  blur color toward `surface_0`/`surface_1` by `in_app_blur_strength`.
- Noise is generated procedurally in the fragment shader via a hash function — no
  binary asset, no licensing question, consistent with the "no new dependency
  unless necessary" default. Noise intensity is a fixed constant, not tied to
  `in_app_blur_strength`; only the tint/blur blend is user-configurable.

## Config schema

```toml
[window]
in_app_blur_enabled = false   # default off — opt-in, unverified on real GPU hardware
in_app_blur_strength = 0.6    # 0.0-1.0, tint/blur blend ratio
```

- Added to `WindowConfig` in `nexterm-config/src/schema/window.rs`, next to the
  existing `background_opacity` / `macos_window_background_blur` fields.
- `docs/CONFIGURATION.md` gets both keys documented; `nexterm-config/tests/doc_matches_schema.rs`
  (added in #73) enforces this mechanically — a missing or stale entry fails CI.
- Out-of-range `in_app_blur_strength` values follow whatever clamp/validation
  convention the existing numeric fields (e.g. `background_opacity`) use; this
  implementation detail is confirmed against the actual code at implementation
  time rather than specified here.

## Settings panel UI

Added to the existing Window tab (`nexterm-client-gpu/src/renderer/overlay/widgets/settings_window.rs`)
only, following the tab-extension contract in the project's `CLAUDE.md`:

- `row::IN_APP_BLUR_ENABLED` → `WidgetKind::Toggle`, mirroring `row::CURSOR_BLINK`.
- `row::IN_APP_BLUR_STRENGTH` → `WidgetKind::Slider { min: 0.0, max: 1.0 }`,
  mirroring `row::OPACITY`.
- New locale keys, all 8 locales under `nexterm-i18n/locales/`:
  - `settings-window-in-app-blur-label`
  - `settings-window-in-app-blur-strength-label`
- No changes to `settings_panel_hit.rs` or `accessibility.rs` — the existing
  tab-extension mechanism (row constant + `WidgetDesc` arm + layout entry +
  `apply_window_action` arm) covers hit-testing and the AccessKit tree
  automatically.

## Testing strategy

Machine-verifiable in CI (no GPU required):

- Config default values and TOML round-trip for the two new fields.
- `doc_matches_schema.rs` catches any `CONFIGURATION.md` drift automatically.
- The existing locale key-parity test catches any missing translation across the
  8 locale files.
- WGSL validation of the new blur/composite shader code through the
  `wgpu::naga` re-export, the same GPU-less shader check P2a introduced.
- Pure-function unit tests for:
  - Kawase tap-offset generation (given texture size and chain level).
  - Capture-invalidation state transitions: `overlay_open_count` 0→1 marks dirty,
    1→2 does not, a resize while an overlay is open marks dirty, a resize while
    closed does not.
  - The tint/strength blend math (e.g. `strength = 0.0` reduces to the existing
    opaque fallback color; `strength = 1.0` reaches the full blur/tint mix) —
    mirroring the `danger_fill_gets_redder_with_strength` test shape from the G11
    follow-up.
- Contrast tests: panel labels over the tinted/blurred fill must clear
  `MIN_TEXT_CONTRAST` (4.5:1) across all nine built-in schemes, at both
  `in_app_blur_strength` extremes (0.0 and 1.0) — mirrors the PR #71 danger-button
  contrast test pattern, and is the one machine-checkable proxy for "the tint
  compensates for whatever the blurred background's luminance turns out to be."

## Risks / on-device verification backlog

This environment has no GPU; per the 2026-08-21 decision recorded in
`docs/plans/ui-ux-modernization-v3.md`, visual verification for GPU-rendered
changes is accepted as deferred rather than blocking. The following items are
explicitly **not measured** by this design and should be added to that backlog
when this phase ships:

- Actual perceived blur quality and Kawase tap radius correctness.
- **Carried over from P2a (PR #64)**: `draw_focus_ring`'s stroke-only outline
  stopped repainting the ring's interior, which the PR's own comment flags as
  "harmless while every surface is opaque, a double blend once P2b's acrylic is
  not." Once panel fills become translucent, the stroke/fill anti-aliasing
  boundary may double-blend. This design does not resolve it; it is inherited as
  a named risk.
- Frame-time cost of the extra offscreen render + blur chain on real
  (particularly integrated) GPUs.
- Correctness of recapture behavior across a real multi-monitor / DPI-change
  transition.
- Whether the procedural noise reads as subtle grain or visible banding at
  various strengths and on various panel colors.

## Rollout

- Ship with `in_app_blur_enabled = false` by default; this PR's completion bar is
  build + clippy + fmt + the machine-verifiable tests above, all green. Visual
  confirmation is explicitly deferred, not blocking, consistent with the existing
  project pattern for GPU-rendered changes.
- `docs/plans/ui-ux-modernization-v3.md` gets its P2b checklist line checked off,
  and the on-device verification backlog section gains the new entries above.
- P2c (the `window.backdrop` enum and OS-native backdrop materials) is tracked as
  a separate, later phase and is not blocked by this PR beyond depending on the
  blur engine this PR ships existing.
