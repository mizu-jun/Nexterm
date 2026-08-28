# P3b1: `SurfaceMotion` and Open/Close Motion for Eleven Surfaces — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every stored overlay surface plus the settings tooltip an entrance and an exit animation, driven by one shared `SurfaceMotion` type that replaces P3a's hand-written settings-panel fields.

**Architecture:** `SurfaceMotion` owns a pair of `Timed` values (entrance, exit) and answers `progress` / `is_visible` / `is_active` / `retire` — P3a's settings-panel logic lifted verbatim. Each surface stores one next to its existing openness, which stays the single truth for input routing and AccessKit. The renderer is the only consumer: `render_frame` gates each surface on `is_visible()`, records the vertex-buffer length before the surface's builder runs, and scales the alpha of the vertices that builder appended by the motion's progress. That keeps ten of the eleven builders completely untouched.

**Tech Stack:** Rust 2024, `nexterm-client-gpu` (wgpu + winit + cosmic-text). No new dependency.

**Spec:** `docs/superpowers/specs/2026-08-28-p3b-motion-application-design.md` (this plan implements only the **P3b1** row of that spec's Delivery table; P3b2 hover and P3b3 press are separate plans)

## Global Constraints

- No `unwrap()`. Use `?` or `expect("reason")` with a concrete message.
- Comments, doc-comments and commit messages in this repo are **English**.
- `cargo clippy -- -D warnings` and `cargo fmt --check` must be green before any commit.
- No new config key, no new user-facing string, no locale change, no `Cargo.lock` change. If any of those becomes necessary, stop and raise it — `docs/CONFIGURATION.md` key-parity and `doc_matches_schema` are the guards, and a `Cargo.lock` change would additionally require `bash scripts/regenerate-flatpak-sources.sh`.
- Every duration passed to `Timed` must have gone through `AnimationsConfig::scaled_duration_ms`, so `animations.enabled = false` / `intensity = "off"` yields 0 and the animation is born finished. `SurfaceMotion::open` / `close` do this internally; never construct a `Timed` for a surface directly.
- Durations and curves come from the spec's table and are not to be re-tuned by eye:

  | Surface class | In | Out |
  |---|---|---|
  | Context menu, tooltip | `duration::FAST` (150) `DecelerateMax` | `duration::FASTER` (100) `AccelerateMax` |
  | Dialogs and large panels | `duration::SLOW` (300) `DecelerateMax` | `duration::FAST` (150) `AccelerateMax` |
  | Settings panel (unchanged from P3a) | `duration::NORMAL` (200) `DecelerateMax` | `duration::FAST` (150) `AccelerateMax` |

- Paths in this plan are relative to `nexterm-client-gpu/src/`.

## Decisions this plan makes that the spec left open

The spec specifies *that* the eleven surfaces animate and with which durations, not *what* moves. Two choices are locked in here:

1. **The visual is a whole-surface alpha fade**, applied by scaling `color[3]` on the vertices a builder appended. Both the background and the text shader take straight alpha in the vertex color and premultiply in the fragment stage (`shaders.rs` `fs_main`: `return vec4(base_color.rgb * base_color.a, base_color.a)`), so scaling the alpha component is correct for every overlay pipeline, including acrylic panel fills (`acrylic_mix` blends `rgb` only).

   The alternative — giving each surface the settings panel's 16 px slide — would mean editing ten builders' independently written layout math for no shared abstraction. Travel can be added per surface later; it is not needed for the phase's goal.

2. **The settings panel keeps its own visual** (scrim-only fade plus 16 px slide, panel opaque). Only its *timer plumbing* moves onto `SurfaceMotion`. Changing a shipped feel is a separate decision, exactly as the spec says of its durations.

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `animations/surface.rs` **(new)** | `SurfaceMotion`: the open/close timer pair and its tests | 1 |
| `animations/mod.rs` | `mod surface; pub use surface::SurfaceMotion;` | 1 |
| `renderer/overlay/fade.rs` **(new)** | `apply_surface_fade`: scale alpha over a vertex range, and its tests | 2 |
| `renderer/overlay/mod.rs` | `mod fade;` | 2 |
| `settings/mod.rs` | `SettingsPanel`: `open_anim`/`closing` → `motion: SurfaceMotion` | 3 |
| `settings/drag.rs` | `eased_progress` delegates to `motion.progress` | 3 |
| `settings/open_close_animation_tests.rs` | P3a's tests, updated only where they poke the removed fields | 3 |
| `palette.rs`, `macro_picker.rs`, `host_manager.rs` | `motion` field + `open`/`close` wiring | 4 |
| `state/blocks.rs`, `state/menus.rs` | `BlockNameModal` / `FileTransferDialog` motion | 5 |
| `state/mod.rs` | ghost fields for the `Option`-shaped surfaces; `has_active_animation` clauses | 6, 7, 8 |
| `renderer/overlay/dialog.rs` | consent / close-window / password builders take their content by reference | 6, 7, 8 |
| `host_manager.rs` | `PasswordModalView` + `PasswordModalGhost` | 8 |
| `renderer/render_frame.rs` | every surface's gate, fade and ghost fallback | 4–9 |
| `renderer/event_handler/lifecycle.rs` | the per-frame `retire` sweep | 3, 9 |
| `nexterm-client-gpu/CLAUDE.md` | one line: adding a surface means adding an aggregate clause and a retire call | 9 |

---

### Task 1: `SurfaceMotion`

**Files:**
- Create: `animations/surface.rs`
- Modify: `animations/mod.rs:26-31` (the `mod` / `pub use` block)

**Interfaces:**
- Consumes: `animations::Timed` (`new`, `resuming_at`, `progress`, `is_done`), `animations::Curve`, `nexterm_config::AnimationsConfig::scaled_duration_ms`.
- Produces: `crate::animations::SurfaceMotion` with
  `open(&mut self, now: Instant, anim: &AnimationsConfig, base_ms: u32, curve: Curve)`,
  `close(&mut self, now: Instant, anim: &AnimationsConfig, base_ms: u32, curve: Curve)`,
  `progress(&self, now: Instant) -> f32`,
  `is_visible(&self) -> bool`,
  `is_active(&self, now: Instant) -> bool`,
  `retire(&mut self, now: Instant)`,
  and `Default`.

- [ ] **Step 1: Write the failing tests**

Create `animations/surface.rs` with the test module only (no `SurfaceMotion` yet), so the compile failure is the red state:

```rust
//! The shared open/close timer pair for overlay surfaces (UI/UX v3 P3b).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animations::Curve;
    use nexterm_config::AnimationsConfig;
    use std::time::{Duration, Instant};

    fn on() -> AnimationsConfig {
        AnimationsConfig::default()
    }

    fn off() -> AnimationsConfig {
        AnimationsConfig {
            enabled: false,
            ..AnimationsConfig::default()
        }
    }

    fn open_it(m: &mut SurfaceMotion, now: Instant, anim: &AnimationsConfig) {
        m.open(now, anim, 300, Curve::DecelerateMax);
    }

    fn close_it(m: &mut SurfaceMotion, now: Instant, anim: &AnimationsConfig) {
        m.close(now, anim, 150, Curve::AccelerateMax);
    }

    #[test]
    fn a_fresh_motion_is_invisible() {
        let m = SurfaceMotion::default();
        let now = Instant::now();
        assert!(!m.is_visible());
        assert!(!m.is_active(now));
        assert!(m.progress(now).abs() < 1e-4);
    }

    #[test]
    fn open_runs_from_0_to_1_over_the_entrance_duration() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        assert!(m.is_visible());
        assert!(m.progress(t0).abs() < 1e-3);
        assert!((m.progress(t0 + Duration::from_millis(300)) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn close_keeps_the_surface_visible_while_it_fades() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        let opened = t0 + Duration::from_millis(300);
        close_it(&mut m, opened, &on());
        assert!(m.is_visible(), "the renderer must keep drawing it");
        assert!(m.progress(opened) > 0.9);
        let done = opened + Duration::from_millis(150);
        assert!(m.progress(done).abs() < 1e-3);
    }

    #[test]
    fn reopening_mid_fade_is_continuous() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        let opened = t0 + Duration::from_millis(300);
        close_it(&mut m, opened, &on());
        let mid = opened + Duration::from_millis(75);
        let before = m.progress(mid);
        open_it(&mut m, mid, &on());
        let after = m.progress(mid);
        assert!(
            (after - before).abs() < 5e-2,
            "value jumped on reopen: {before} -> {after}"
        );
    }

    #[test]
    fn retire_drops_a_finished_exit_and_hides_the_surface() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        let opened = t0 + Duration::from_millis(300);
        close_it(&mut m, opened, &on());
        let mid = opened + Duration::from_millis(75);
        m.retire(mid);
        assert!(m.is_visible(), "an unfinished exit must survive retire");
        let done = opened + Duration::from_millis(150);
        m.retire(done);
        assert!(!m.is_visible());
        assert!(!m.is_active(done));
    }

    #[test]
    fn is_active_is_false_once_the_entrance_has_finished() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        assert!(m.is_active(t0));
        assert!(!m.is_active(t0 + Duration::from_millis(300)));
        assert!(m.is_visible(), "finished entrance still means shown");
    }

    /// The reduced-motion path: `scaled_duration_ms` returns 0, so both
    /// transitions are finished the moment they start.
    #[test]
    fn disabled_animations_open_and_close_instantly() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &off());
        assert!((m.progress(t0) - 1.0).abs() < 1e-4);
        assert!(!m.is_active(t0));
        close_it(&mut m, t0, &off());
        assert!(m.progress(t0).abs() < 1e-4);
        assert!(!m.is_active(t0));
        m.retire(t0);
        assert!(!m.is_visible());
    }
}
```

Add the module to `animations/mod.rs`, next to the existing `mod curve; mod easing; mod timed;`:

```rust
mod surface;
```

and extend the existing re-export block:

```rust
pub use surface::SurfaceMotion;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu animations::surface`
Expected: FAIL — `cannot find type SurfaceMotion in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `animations/surface.rs`:

```rust
//! The shared open/close timer pair for overlay surfaces (UI/UX v3 P3b).
//!
//! P3a proved this state machine on the settings panel by hand. P3b needs it
//! on eleven surfaces, so the logic is lifted here verbatim — including its
//! two ordering rules:
//!
//! 1. Read the value already on screen **before** overwriting either field;
//!    `progress` derives it from them, so touching one first loses it.
//! 2. Starting a close passes `1.0 - visibility`, because `closing` counts
//!    up while visibility counts down.
//!
//! The surface's own openness (`is_open`, or a live `Option`) remains the
//! truth for input routing and the AccessKit tree and still flips the
//! instant the user acts. This type is the renderer's permission to keep
//! drawing for a few more frames.

use std::time::Instant;

use nexterm_config::AnimationsConfig;

use super::{Curve, Timed};

/// One surface's entrance and exit animations.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceMotion {
    /// Entrance. `Some` from the moment the surface opens; its progress is
    /// the surface's visibility until it closes.
    open_anim: Option<Timed>,
    /// Exit — **render-only**. Retired by [`SurfaceMotion::retire`] once done.
    closing: Option<Timed>,
}

impl SurfaceMotion {
    /// Start the entrance, resuming from whatever is on screen.
    ///
    /// `base_ms` is an unscaled `duration::*` constant; the reduced-motion
    /// scaling happens here so no caller can forget it.
    pub fn open(&mut self, now: Instant, anim: &AnimationsConfig, base_ms: u32, curve: Curve) {
        let ms = anim.scaled_duration_ms(base_ms);
        let resume_from = self.closing.is_some().then(|| self.progress(now));
        self.closing = None;
        self.open_anim = Some(match resume_from {
            Some(v) => Timed::resuming_at(now, v, ms, curve),
            None => Timed::new(now, ms, curve),
        });
    }

    /// Start the exit from whatever is on screen.
    pub fn close(&mut self, now: Instant, anim: &AnimationsConfig, base_ms: u32, curve: Curve) {
        let ms = anim.scaled_duration_ms(base_ms);
        let visibility = self.progress(now);
        self.open_anim = None;
        self.closing = Some(Timed::resuming_at(now, 1.0 - visibility, ms, curve));
    }

    /// Visibility in `[0, 1]`: 0 hidden, 1 fully shown.
    pub fn progress(&self, now: Instant) -> f32 {
        if let Some(closing) = self.closing {
            return 1.0 - closing.progress(now);
        }
        self.open_anim.map_or(0.0, |a| a.progress(now))
    }

    /// Whether the renderer should draw the surface at all.
    pub fn is_visible(&self) -> bool {
        self.open_anim.is_some() || self.closing.is_some()
    }

    /// Whether another frame is needed.
    pub fn is_active(&self, now: Instant) -> bool {
        self.closing.is_some_and(|c| !c.is_done(now))
            || self.open_anim.is_some_and(|a| !a.is_done(now))
    }

    /// Drop a finished exit animation, so the surface stops being drawn.
    ///
    /// A finished *entrance* is deliberately kept: it is the surface's
    /// visibility for as long as it stays open.
    pub fn retire(&mut self, now: Instant) {
        if self.closing.is_some_and(|c| c.is_done(now)) {
            self.closing = None;
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu animations::surface`
Expected: PASS, 7 tests.

- [ ] **Step 5: Check the gates**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`
Expected: clean. If clippy reports `SurfaceMotion` methods as never used, add `#[allow(dead_code)]` to the `impl` block with the comment `// Consumed by the surfaces migrated in the following tasks.` and remove it in Task 3.

- [ ] **Step 6: Commit**

```bash
git add nexterm-client-gpu/src/animations/surface.rs nexterm-client-gpu/src/animations/mod.rs
git commit -m "feat(client): add SurfaceMotion, the shared overlay open/close timer pair"
```

---

### Task 2: `apply_surface_fade`

**Files:**
- Create: `renderer/overlay/fade.rs`
- Modify: `renderer/overlay/mod.rs` (add `mod fade;` beside the existing submodule declarations)

**Interfaces:**
- Consumes: `crate::glyph_atlas::{BgVertex, TextVertex}`.
- Produces: `pub(in crate::renderer) fn apply_surface_fade(bg: &mut [BgVertex], text: &mut [TextVertex], alpha: f32)`.

- [ ] **Step 1: Write the failing tests**

Create `renderer/overlay/fade.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn bg(alpha: f32) -> BgVertex {
        BgVertex {
            position: [0.0, 0.0],
            color: [0.2, 0.4, 0.6, alpha],
            rect_center: [0.0, 0.0],
            rect_half_size: [0.0, 0.0],
            corner_radius: 0.0,
            shadow_softness: 0.0,
            stroke_width: 0.0,
            acrylic_mix: 0.0,
        }
    }

    fn text(alpha: f32) -> TextVertex {
        TextVertex {
            position: [0.0, 0.0],
            uv: [0.0, 0.0],
            color: [1.0, 1.0, 1.0, alpha],
        }
    }

    #[test]
    fn a_half_fade_halves_every_alpha_and_leaves_rgb_alone() {
        let mut b = [bg(1.0), bg(0.5)];
        let mut t = [text(1.0)];
        apply_surface_fade(&mut b, &mut t, 0.5);
        assert!((b[0].color[3] - 0.5).abs() < 1e-6);
        assert!((b[1].color[3] - 0.25).abs() < 1e-6);
        assert!((t[0].color[3] - 0.5).abs() < 1e-6);
        assert!((b[0].color[0] - 0.2).abs() < 1e-6, "rgb must not change");
    }

    #[test]
    fn a_full_fade_changes_nothing() {
        let mut b = [bg(0.75)];
        let mut t = [text(0.75)];
        apply_surface_fade(&mut b, &mut t, 1.0);
        assert!((b[0].color[3] - 0.75).abs() < 1e-6);
        assert!((t[0].color[3] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_alphas_are_clamped() {
        let mut b = [bg(1.0)];
        let mut t = [];
        apply_surface_fade(&mut b, &mut t, 1.7);
        assert!((b[0].color[3] - 1.0).abs() < 1e-6);
        apply_surface_fade(&mut b, &mut t, -0.3);
        assert!(b[0].color[3].abs() < 1e-6);
    }

    #[test]
    fn empty_ranges_are_fine() {
        let mut b: [BgVertex; 0] = [];
        let mut t: [TextVertex; 0] = [];
        apply_surface_fade(&mut b, &mut t, 0.5);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu overlay::fade`
Expected: FAIL — `cannot find function apply_surface_fade`.

- [ ] **Step 3: Write the implementation**

Insert above the test module:

```rust
//! Fading a whole overlay surface (UI/UX v3 P3b).
//!
//! An overlay's entrance and exit are applied *after* its builder has run,
//! by scaling the alpha of the vertices that builder appended. Both overlay
//! shaders take straight alpha in the vertex color and premultiply in the
//! fragment stage (`shaders.rs`: `return vec4(c.rgb * c.a, c.a)`), so
//! scaling `color[3]` is the correct and only edit — and it is correct for
//! acrylic panel fills too, since `acrylic_mix` blends `rgb` only.
//!
//! Doing it here rather than inside each builder is what lets ten surfaces
//! gain motion without ten independently written layout diffs.

use crate::glyph_atlas::{BgVertex, TextVertex};

/// Scale the alpha of `bg` and `text` by `alpha`, clamped to `[0, 1]`.
///
/// Pass the sub-slices a single surface appended, e.g.
///
/// ```ignore
/// let (bg_start, text_start) = (bg_verts.len(), text_verts.len());
/// self.build_macro_picker_verts(/* ... */);
/// apply_surface_fade(
///     &mut bg_verts[bg_start..],
///     &mut text_verts[text_start..],
///     progress,
/// );
/// ```
pub(in crate::renderer) fn apply_surface_fade(
    bg: &mut [BgVertex],
    text: &mut [TextVertex],
    alpha: f32,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha >= 1.0 {
        return;
    }
    for v in bg {
        v.color[3] *= alpha;
    }
    for v in text {
        v.color[3] *= alpha;
    }
}
```

Add to `renderer/overlay/mod.rs` — `pub(in crate::renderer)`, not a plain `mod`, because `render_frame.rs` is the caller and a private `mod` here would not be reachable from it:

```rust
pub(in crate::renderer) mod fade;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu overlay::fade`
Expected: PASS, 4 tests.

- [ ] **Step 5: Check the gates**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`
Expected: clean. If `apply_surface_fade` is reported unused, add `#[allow(dead_code)] // First consumer lands in Task 4.` above the function and remove it in Task 4.

- [ ] **Step 6: Commit**

```bash
git add nexterm-client-gpu/src/renderer/overlay/fade.rs nexterm-client-gpu/src/renderer/overlay/mod.rs
git commit -m "feat(client): add apply_surface_fade for whole-overlay entrance and exit fades"
```

---

### Task 3: Retrofit the settings panel onto `SurfaceMotion`

The panel works today; this task must not change how it looks or feels. Its P3a tests move with it and stay green — that is the acceptance criterion.

**Files:**
- Modify: `settings/mod.rs:77-86` (the two fields), `settings/mod.rs:362-364` (the `Default` body), `settings/mod.rs:445-490` (`open` / `close`), `settings/mod.rs:510-516` (`is_visible`)
- Modify: `settings/drag.rs:44-53` (`eased_progress`)
- Modify: `state/mod.rs:641-650` (`has_active_animation`)
- Modify: `renderer/event_handler/lifecycle.rs:669-675` (the retire block)
- Test: `settings/open_close_animation_tests.rs` (existing; two assertions reference the removed field)

**Interfaces:**
- Consumes: `SurfaceMotion` from Task 1.
- Produces: `SettingsPanel.motion: SurfaceMotion` (public field). `SettingsPanel::is_visible` and `eased_progress` keep their current signatures, so no call site outside these files changes.

- [ ] **Step 1: Replace the fields**

In `settings/mod.rs`, replace the `open_anim` and `closing` fields with:

```rust
    /// Open/close animation (UI/UX v3 P3b: was a hand-written `Timed` pair
    /// in P3a). `is_open` above remains the truth for input routing and the
    /// AccessKit tree; this is the renderer's permission to keep drawing
    /// the panel while it fades out.
    pub motion: crate::animations::SurfaceMotion,
```

and in the `Default` body replace `open_anim: None, closing: None,` with:

```rust
            motion: crate::animations::SurfaceMotion::default(),
```

- [ ] **Step 2: Rewrite `open` / `close` / `is_visible` / `eased_progress`**

`settings/mod.rs` — `open` becomes:

```rust
    pub fn open(&mut self, now: std::time::Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, duration};

        // P3a's 200 ms Direct Entrance is kept deliberately: the spec's
        // 300 ms dialog row applies to the surfaces getting motion for the
        // first time, and changing a shipped feel is a separate decision.
        self.motion
            .open(now, anim, duration::NORMAL, Curve::DecelerateMax);
        self.is_open = true;
    }
```

`close` keeps its whole body of state resets; only the animation lines change. Replace the `use`, the `ms` binding, the `visibility` binding and the `open_anim` / `closing` writes with:

```rust
        use crate::animations::{Curve, duration};

        self.motion
            .close(now, anim, duration::FAST, Curve::AccelerateMax);
        self.is_open = false;
```

`is_visible` becomes:

```rust
    pub fn is_visible(&self) -> bool {
        self.is_open || self.motion.is_visible()
    }
```

`settings/drag.rs` — `eased_progress` becomes:

```rust
    pub fn eased_progress(&self, now: std::time::Instant) -> f32 {
        self.motion.progress(now)
    }
```

- [ ] **Step 3: Update the aggregate and the retire sweep**

`state/mod.rs`, in `has_active_animation`, replace the two settings-panel clauses with:

```rust
        if self.settings_panel.motion.is_active(now) {
            return true;
        }
```

Note the dropped `sp.is_open &&` guard: `SurfaceMotion::close` clears the entrance, so a closed panel has no entrance to report as active.

`renderer/event_handler/lifecycle.rs`, replace the P3a retire block with:

```rust
        // UI/UX v3 P3a/P3b: drop finished exit animations so the renderer
        // stops drawing those surfaces. Finished entrances are left in
        // place; they are a surface's visibility while it is open.
        let now = Instant::now();
        self.app.state.settings_panel.motion.retire(now);
```

- [ ] **Step 4: Update the two tests that poke the removed field**

In `settings/open_close_animation_tests.rs`, `sp.closing.is_some_and(|c| c.is_done(done))` has no replacement expression (`closing` is now private to `SurfaceMotion`). Assert the observable consequence instead — in `the_close_animation_fades_to_0_and_then_stops_being_visible`:

```rust
    let done = opened + Duration::from_millis(150);
    assert!(sp.eased_progress(done).abs() < 1e-3);
    assert!(sp.is_visible(), "still drawn until the frame loop retires it");
    sp.motion.retire(done);
    assert!(!sp.is_visible());
```

in `reopening_during_the_fade_out_is_continuous`, replace the `sp.closing.is_none()` assertion with:

```rust
    assert!(sp.is_open, "reopening must cancel the fade-out");
```

and in `disabled_animations_open_and_close_instantly`:

```rust
    sp.close(t0, &off());
    assert!(sp.eased_progress(t0).abs() < 1e-4);
    sp.motion.retire(t0);
    assert!(!sp.is_visible());
```

- [ ] **Step 5: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS. Every P3a settings-panel test still passes, including `a_closing_settings_panel_wants_animation_frames` in `state/mod.rs`.

- [ ] **Step 6: Check the gates and commit**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "refactor(client): move the settings panel onto SurfaceMotion"
```

---

### Task 4: The three large `bool` panels — palette, macro picker, host manager

Three surfaces with identical mechanics. Doing them in one task means one review of one pattern rather than three of the same.

**Files:**
- Modify: `palette.rs:47-102` (field, `Default`/`new` body, `open`, `close`)
- Modify: `macro_picker.rs:16-44` (same four places)
- Modify: `host_manager.rs:424-478` (same four places)
- Modify: `state/mod.rs` `has_active_animation` (three clauses)
- Modify: `renderer/render_frame.rs:993-1080` (three gates plus three fades)
- Modify: `renderer/event_handler/lifecycle.rs` (three retire calls)
- Test: `state/mod.rs` test module (three new tests)

**Interfaces:**
- Consumes: `SurfaceMotion` (Task 1), `apply_surface_fade` (Task 2).
- Produces: `CommandPalette.motion`, `MacroPicker.motion`, `HostManager.motion`, all `pub motion: SurfaceMotion`. `open` / `close` on all three gain two parameters: `(&mut self, now: Instant, anim: &AnimationsConfig)` — `HostManager::open` and `close` take no other arguments today, and neither do the other two.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `state/mod.rs`:

```rust
    /// The three large panels share one shape: the logical flag closes at
    /// once, the surface stays visible while it fades, and the frame loop
    /// wants frames for the whole transition.
    #[test]
    fn a_closing_command_palette_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.palette.open(t0, &anim);
        assert!(state.palette.is_open);
        assert!(state.has_active_animation(t0, 200));

        let opened = t0 + Duration::from_millis(300);
        state.palette.close(opened, &anim);
        assert!(!state.palette.is_open, "input must see it as closed");
        assert!(state.palette.motion.is_visible(), "renderer keeps drawing");
        assert!(state.has_active_animation(opened, 200));

        let done = opened + Duration::from_millis(150);
        state.palette.motion.retire(done);
        assert!(!state.palette.motion.is_visible());
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_closing_macro_picker_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.macro_picker.open(t0, &anim);
        let opened = t0 + Duration::from_millis(300);
        state.macro_picker.close(opened, &anim);
        assert!(!state.macro_picker.is_open);
        assert!(state.macro_picker.motion.is_visible());
        assert!(state.has_active_animation(opened, 200));
        let done = opened + Duration::from_millis(150);
        state.macro_picker.motion.retire(done);
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_closing_host_manager_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.host_manager.open(t0, &anim);
        let opened = t0 + Duration::from_millis(300);
        state.host_manager.close(opened, &anim);
        assert!(!state.host_manager.is_open);
        assert!(state.host_manager.motion.is_visible());
        assert!(state.has_active_animation(opened, 200));
        let done = opened + Duration::from_millis(150);
        state.host_manager.motion.retire(done);
        assert!(!state.has_active_animation(done, 200));
    }
```

If the test module lacks them, add `use std::time::Duration;` to its imports.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu closing_command_palette`
Expected: FAIL — `this method takes 0 arguments but 2 arguments were supplied` / `no field motion`.

- [ ] **Step 3: Add the field and the two parameters, in all three types**

The edit is the same in `palette.rs`, `macro_picker.rs` and `host_manager.rs`. Add next to `pub is_open: bool`:

```rust
    /// Open/close animation (UI/UX v3 P3b). `is_open` above stays the truth
    /// for input routing and the AccessKit tree; this is render-only.
    pub motion: crate::animations::SurfaceMotion,
```

add `motion: crate::animations::SurfaceMotion::default(),` to the constructor body, and give `open` / `close` the timer calls. For `CommandPalette`:

```rust
    pub fn open(&mut self, now: std::time::Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, duration};

        self.motion
            .open(now, anim, duration::SLOW, Curve::DecelerateMax);
        self.is_open = true;
        // ... existing body (query reset, selection reset) unchanged
    }

    pub fn close(&mut self, now: std::time::Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, duration};

        self.motion
            .close(now, anim, duration::FAST, Curve::AccelerateMax);
        self.is_open = false;
        // ... existing body unchanged
    }
```

`MacroPicker::open` / `close` and `HostManager::open` / `close` take the identical two lines with the identical constants.

- [ ] **Step 4: Fix every call site**

Run: `cargo build -p nexterm-client-gpu 2>&1 | grep -E "^error" -A 4`

Each error is a call needing `now` and the config. Inside `EventHandler` / `InputHandler` methods the idiom already used elsewhere is:

```rust
self.app.state.palette.open(std::time::Instant::now(), &self.app.config.animations);
```

Call sites to expect (confirm with `grep -rn "palette.open()\|palette.close()\|macro_picker.open()\|macro_picker.close()\|host_manager.open()\|host_manager.close()" nexterm-client-gpu/src`): `renderer/input_handler/mod.rs`, `renderer/event_handler/keyboard.rs`, `renderer/event_handler/mouse.rs`, `renderer/event_handler/accessibility.rs`. Tests in `accessibility.rs` that set `state.palette.is_open = true` directly are left alone — they exercise the AccessKit tree, which reads the flag, not the motion.

- [ ] **Step 5: Gate, fade and retire in the renderer**

In `renderer/render_frame.rs`, each of the three blocks changes from `if state.X.is_open {` to the recorded-range form. For the macro picker:

```rust
        // ---- Lua macro picker (while visible) ----
        if state.macro_picker.motion.is_visible() {
            let (bg_start, text_start) = (bg_verts.len(), text_verts.len());
            self.build_macro_picker_verts(
                state,
                &tokens,
                sw,
                sh,
                cell_w,
                cell_h,
                panel_acrylic_mix,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
            super::overlay::fade::apply_surface_fade(
                &mut bg_verts[bg_start..],
                &mut text_verts[text_start..],
                state.macro_picker.motion.progress(frame_now),
            );
        }
```

Apply the same shape to the host-manager and palette blocks. Two details:

- The builders read `state.X.is_open` in places (`build_host_manager_verts` and friends gate sub-parts on it). Grep each builder for `is_open` and replace those reads with `motion.is_visible()` only where the read decides *whether to draw*; leave reads that decide *behaviour* alone. If a builder returns early on `!is_open`, the fade would be applied to zero vertices and the surface would never draw its exit — that early return must become `motion.is_visible()`.
- `overlay_open_count` (`render_frame.rs:950-958`) keeps reading `is_open`. The acrylic capture should follow the logical state, not the fade: a surface that is closing does not need a fresh capture.

In `renderer/event_handler/lifecycle.rs`, extend the retire sweep:

```rust
        self.app.state.palette.motion.retire(now);
        self.app.state.macro_picker.motion.retire(now);
        self.app.state.host_manager.motion.retire(now);
```

And in `state/mod.rs` `has_active_animation`, add:

```rust
        if self.palette.motion.is_active(now)
            || self.macro_picker.motion.is_active(now)
            || self.host_manager.motion.is_active(now)
        {
            return true;
        }
```

- [ ] **Step 6: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS, including the three new tests.

- [ ] **Step 7: Check the gates and commit**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): animate the command palette, macro picker and host manager open/close"
```

---

### Task 5: The two `bool` modals — block name, file transfer

**Files:**
- Modify: `state/blocks.rs:42-78` (`BlockNameModal`: field, `open_for`, `close`)
- Modify: `state/menus.rs:353-375` (`FileTransferDialog`: field, `new`, and its open/close call sites)
- Modify: `state/mod.rs` `has_active_animation`, plus two new tests
- Modify: `renderer/render_frame.rs` (two gates plus two fades)
- Modify: `renderer/event_handler/lifecycle.rs` (two retire calls)

**Interfaces:**
- Consumes: `SurfaceMotion`, `apply_surface_fade`.
- Produces: `BlockNameModal.motion`, `FileTransferDialog.motion`. `BlockNameModal::open_for` gains two trailing parameters `(now: Instant, anim: &AnimationsConfig)`; `BlockNameModal::close` likewise. `FileTransferDialog` has no `open`/`close` methods today — add them:
  `pub fn open(&mut self, now: Instant, anim: &AnimationsConfig)` and
  `pub fn close(&mut self, now: Instant, anim: &AnimationsConfig)`, each setting `is_open` and driving `motion`.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `state/mod.rs`:

```rust
    #[test]
    fn a_closing_block_name_modal_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state
            .block_name_modal
            .open_for(crate::state::BlockId(1), Some("build"), t0, &anim);
        let opened = t0 + Duration::from_millis(300);
        state.block_name_modal.close(opened, &anim);
        assert!(!state.block_name_modal.is_open);
        assert!(state.block_name_modal.motion.is_visible());
        assert!(state.has_active_animation(opened, 200));
        let done = opened + Duration::from_millis(150);
        state.block_name_modal.motion.retire(done);
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_closing_file_transfer_dialog_stays_visible_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.file_transfer.open(t0, &anim);
        assert!(state.file_transfer.is_open);
        let opened = t0 + Duration::from_millis(300);
        state.file_transfer.close(opened, &anim);
        assert!(!state.file_transfer.is_open);
        assert!(state.file_transfer.motion.is_visible());
        assert!(state.has_active_animation(opened, 200));
        let done = opened + Duration::from_millis(150);
        state.file_transfer.motion.retire(done);
        assert!(!state.has_active_animation(done, 200));
    }
```

Adjust the `BlockId` construction to whatever `state/blocks.rs` actually exposes (check with `grep -n "pub struct BlockId" -A 3 nexterm-client-gpu/src/state/blocks.rs`); if it is not a tuple struct, build it the way the existing block tests do.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu block_name_modal_stays_visible`
Expected: FAIL — argument count / `no field motion`.

- [ ] **Step 3: Implement**

`state/blocks.rs` — add the field next to `pub is_open: bool`:

```rust
    /// Open/close animation (UI/UX v3 P3b); render-only, see `is_open`.
    pub motion: crate::animations::SurfaceMotion,
```

`BlockNameModal` derives `Default`, and `SurfaceMotion` derives `Default` too, so no constructor edit is needed. Add the timer calls at the top of `open_for` and `close`:

```rust
    pub fn open_for(
        &mut self,
        block_id: BlockId,
        current_name: Option<&str>,
        now: std::time::Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, duration};

        self.motion
            .open(now, anim, duration::SLOW, Curve::DecelerateMax);
        self.is_open = true;
        // ... existing body unchanged
    }

    pub fn close(&mut self, now: std::time::Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, duration};

        self.motion
            .close(now, anim, duration::FAST, Curve::AccelerateMax);
        self.is_open = false;
        // ... existing body unchanged
    }
```

`state/menus.rs` — add the same field to `FileTransferDialog`, `motion: crate::animations::SurfaceMotion::default(),` to `FileTransferDialog::new`, and the two new methods:

```rust
    /// Open the dialog and start its entrance (UI/UX v3 P3b).
    pub fn open(&mut self, now: std::time::Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, duration};

        self.motion
            .open(now, anim, duration::SLOW, Curve::DecelerateMax);
        self.is_open = true;
    }

    /// Close the dialog and start its exit (UI/UX v3 P3b).
    pub fn close(&mut self, now: std::time::Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, duration};

        self.motion
            .close(now, anim, duration::FAST, Curve::AccelerateMax);
        self.is_open = false;
    }
```

Then replace every direct `state.file_transfer.is_open = true/false` assignment with the new methods:

Run: `grep -rn "file_transfer.is_open = " nexterm-client-gpu/src`

- [ ] **Step 4: Gate, fade, retire, aggregate**

Same five edits as Task 4, for `block_name_modal` and `file_transfer`: the two `render_frame.rs` gates become `motion.is_visible()` with a recorded range and an `apply_surface_fade` call using `frame_now`; two `retire` lines join the lifecycle sweep; two `is_active` clauses join `has_active_animation`. Leave `overlay_open_count`'s `state.file_transfer.is_open` read as it is.

- [ ] **Step 5: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS.

- [ ] **Step 6: Check the gates and commit**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): animate the block-name modal and file-transfer dialog open/close"
```

---

### Task 6: `Option`-shaped ghosts — context menu and close-window dialog

These surfaces *are* their content: setting the field to `None` destroys what the renderer would draw. The exit animation therefore needs a ghost that owns a clone, so the live field can go `None` at once.

**Files:**
- Modify: `state/mod.rs` (two ghost fields on `ClientState`, two helper methods, `has_active_animation`, two new tests)
- Modify: `renderer/overlay/dialog.rs:566` (`build_close_window_dialog_verts` takes its dialog by reference)
- Modify: `renderer/render_frame.rs:1117-1230` (two gates with ghost fallback plus two fades)
- Modify: `renderer/event_handler/mouse.rs:645,748`, `renderer/event_handler/accessibility.rs:248`, `renderer/event_handler/window.rs:167,210` (open/close through the helpers)
- Modify: `renderer/event_handler/lifecycle.rs` (two retire calls)

**Interfaces:**
- Consumes: `Timed`, `Curve`, `duration`, `apply_surface_fade`.
- Produces on `ClientState`:
  `pub context_menu_closing: Option<(ContextMenu, Timed)>`,
  `pub close_window_dialog_closing: Option<(CloseWindowDialog, Timed)>`,
  `pub fn dismiss_context_menu(&mut self, now: Instant, anim: &AnimationsConfig)`,
  `pub fn dismiss_close_window_dialog(&mut self, now: Instant, anim: &AnimationsConfig)`,
  `pub fn retire_ghosts(&mut self, now: Instant)`.
  `build_close_window_dialog_verts` changes its first parameter from `state: &ClientState` to `dialog: &CloseWindowDialog` (matching `build_context_menu_verts`, which already takes `menu: &ContextMenu`).

Both `ContextMenu` and `CloseWindowDialog` already `derive(Clone)`, so the ghost costs one clone of a small struct at dismiss time.

- [ ] **Step 1: Write the failing tests**

Append to the test module in `state/mod.rs`:

```rust
    /// An `Option`-shaped surface must leave the live field `None` the
    /// instant it is dismissed — nothing can be clicked during the fade —
    /// while the ghost keeps the renderer supplied with content.
    #[test]
    fn a_dismissed_context_menu_leaves_a_ghost_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.context_menu = Some(ContextMenu::new_default(10.0, 10.0, &[]));

        state.dismiss_context_menu(t0, &anim);
        assert!(state.context_menu.is_none(), "input must see it as gone");
        assert!(state.context_menu_closing.is_some(), "renderer keeps drawing");
        assert!(state.has_active_animation(t0, 200));

        let done = t0 + Duration::from_millis(100);
        state.retire_ghosts(done);
        assert!(state.context_menu_closing.is_none());
        assert!(!state.has_active_animation(done, 200));
    }

    #[test]
    fn a_dismissed_close_window_dialog_leaves_a_ghost_and_wants_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.close_window_dialog = Some(CloseWindowDialog {
            server_window_id: 1,
            message: "close?".to_string(),
            kill_label: "Close".to_string(),
            cancel_label: "Cancel".to_string(),
            selected_button: 0,
        });

        state.dismiss_close_window_dialog(t0, &anim);
        assert!(state.close_window_dialog.is_none());
        assert!(state.close_window_dialog_closing.is_some());
        assert!(state.has_active_animation(t0, 200));

        let done = t0 + Duration::from_millis(150);
        state.retire_ghosts(done);
        assert!(state.close_window_dialog_closing.is_none());
        assert!(!state.has_active_animation(done, 200));
    }

    /// Dismissing twice in a row must not resurrect the first ghost.
    #[test]
    fn dismissing_an_absent_context_menu_is_a_no_op() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.dismiss_context_menu(t0, &anim);
        assert!(state.context_menu_closing.is_none());
        assert!(!state.has_active_animation(t0, 200));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu dismissed_context_menu`
Expected: FAIL — `no method named dismiss_context_menu`.

- [ ] **Step 3: Implement the ghosts**

In `state/mod.rs`, next to `pub context_menu: Option<ContextMenu>`:

```rust
    /// Exit animation for the context menu (UI/UX v3 P3b) — **render-only**.
    ///
    /// The `Option` above *is* the menu's openness, so dismissing it
    /// destroys the content the exit animation still needs to draw. The
    /// ghost owns a clone; `context_menu` goes `None` at once, so nothing
    /// can be hovered or clicked while it fades.
    pub context_menu_closing: Option<(ContextMenu, crate::animations::Timed)>,
```

and next to `pub close_window_dialog: Option<CloseWindowDialog>` the same field with `CloseWindowDialog`, plus `context_menu_closing: None,` / `close_window_dialog_closing: None,` in `ClientState::new`.

Add to `impl ClientState`:

```rust
    /// Dismiss the context menu, leaving a ghost to fade out (UI/UX v3 P3b).
    pub fn dismiss_context_menu(
        &mut self,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        if let Some(menu) = self.context_menu.take() {
            let ms = anim.scaled_duration_ms(duration::FASTER);
            self.context_menu_closing = Some((menu, Timed::new(now, ms, Curve::AccelerateMax)));
        }
    }

    /// Dismiss the close-window dialog, leaving a ghost (UI/UX v3 P3b).
    pub fn dismiss_close_window_dialog(
        &mut self,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        if let Some(dialog) = self.close_window_dialog.take() {
            let ms = anim.scaled_duration_ms(duration::FAST);
            self.close_window_dialog_closing =
                Some((dialog, Timed::new(now, ms, Curve::AccelerateMax)));
        }
    }

    /// Drop every finished ghost (UI/UX v3 P3b). Called once per frame.
    pub fn retire_ghosts(&mut self, now: Instant) {
        if self
            .context_menu_closing
            .as_ref()
            .is_some_and(|(_, t)| t.is_done(now))
        {
            self.context_menu_closing = None;
        }
        if self
            .close_window_dialog_closing
            .as_ref()
            .is_some_and(|(_, t)| t.is_done(now))
        {
            self.close_window_dialog_closing = None;
        }
    }
```

There is no entrance ghost problem: an opening `Option` surface has its content, so the entrance is a plain `Timed` alongside it. For these two, opening is instantaneous today and the spec's table lists an "In" duration, so give the entrance a `Timed` as well — store it in the same ghost slot pattern would conflate the two, so instead track entrances with a `SurfaceMotion`-free `Option<Timed>`:

```rust
    /// Entrance animation for the context menu (UI/UX v3 P3b).
    pub context_menu_opening: Option<crate::animations::Timed>,
```

set in the two `mouse.rs` construction sites via a helper:

```rust
    /// Show `menu`, starting its entrance (UI/UX v3 P3b).
    pub fn show_context_menu(
        &mut self,
        menu: ContextMenu,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::FAST);
        self.context_menu_closing = None;
        self.context_menu_opening = Some(Timed::new(now, ms, Curve::DecelerateMax));
        self.context_menu = Some(menu);
    }
```

Add the mirrored `close_window_dialog_opening` field and `show_close_window_dialog` method. Progress for the renderer is then: `opening.map_or(1.0, |t| t.progress(now))` while the live field is `Some`, and `1.0 - t.progress(now)` from the ghost.

Give that expression a name rather than repeating it — add to `impl ClientState`:

```rust
    /// Visibility in `[0, 1]` of an `Option`-shaped surface (UI/UX v3 P3b):
    /// the entrance while it is live, the inverted exit while it is a ghost.
    pub(crate) fn option_surface_progress(
        live: bool,
        opening: Option<crate::animations::Timed>,
        ghost: Option<&crate::animations::Timed>,
        now: Instant,
    ) -> f32 {
        if live {
            return opening.map_or(1.0, |t| t.progress(now));
        }
        ghost.map_or(0.0, |t| 1.0 - t.progress(now))
    }
```

- [ ] **Step 4: Add the aggregate clauses**

In `has_active_animation`:

```rust
        if self
            .context_menu_closing
            .as_ref()
            .is_some_and(|(_, t)| !t.is_done(now))
            || self
                .close_window_dialog_closing
                .as_ref()
                .is_some_and(|(_, t)| !t.is_done(now))
            || self
                .context_menu_opening
                .is_some_and(|t| self.context_menu.is_some() && !t.is_done(now))
            || self
                .close_window_dialog_opening
                .is_some_and(|t| self.close_window_dialog.is_some() && !t.is_done(now))
        {
            return true;
        }
```

- [ ] **Step 5: Route every call site through the helpers**

Run: `cargo build -p nexterm-client-gpu 2>&1 | grep -E "^error" -A 4`

- `renderer/event_handler/mouse.rs:645` and `:748` — construction — become `self.app.state.show_context_menu(menu, now, &self.app.config.animations);`. At `:645` the current code assigns the result of an expression that may be `None`; keep that shape by calling `dismiss_context_menu` in the `None` arm.
- `renderer/event_handler/accessibility.rs:248` — `context_menu = None` — becomes `dismiss_context_menu(...)`.
- `renderer/event_handler/window.rs:210` — construction — becomes `show_close_window_dialog(...)`; `:167` — `= None` — becomes `dismiss_close_window_dialog(...)`.
- Any other `context_menu = None` the build turns up (there are dismiss paths in the keyboard and mouse handlers) gets the same treatment. Grep to be sure: `grep -rn "context_menu = None\|close_window_dialog = None" nexterm-client-gpu/src`.

- [ ] **Step 6: Draw the live surface or the ghost**

`renderer/overlay/dialog.rs` — change `build_close_window_dialog_verts`'s first parameter from `state: &ClientState` to `dialog: &CloseWindowDialog` and delete its internal `let Some(dialog) = &state.close_window_dialog else { return; };`. If its body reads anything else off `state`, pass that explicitly rather than keeping the whole state around.

`renderer/render_frame.rs` — the context-menu block becomes:

```rust
        // ---- Context menu (on right-click; ghost while it fades) ----
        let menu_to_draw = state
            .context_menu
            .as_ref()
            .or(state.context_menu_closing.as_ref().map(|(m, _)| m));
        if let Some(menu) = menu_to_draw {
            let progress = ClientState::option_surface_progress(
                state.context_menu.is_some(),
                state.context_menu_opening,
                state.context_menu_closing.as_ref().map(|(_, t)| t),
                frame_now,
            );
            let (bg_start, text_start) = (bg_verts.len(), text_verts.len());
            self.build_context_menu_verts(
                menu,
                &tokens,
                sw,
                sh,
                cell_w,
                cell_h,
                panel_acrylic_mix,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
            super::overlay::fade::apply_surface_fade(
                &mut bg_verts[bg_start..],
                &mut text_verts[text_start..],
                progress,
            );
        }
```

Apply the same shape to the close-window block, passing `dialog` to the builder. `option_surface_progress` must be `pub(crate)` for this call site.

`renderer/event_handler/lifecycle.rs` — add `self.app.state.retire_ghosts(now);` to the sweep.

- [ ] **Step 7: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS, including the three new tests.

- [ ] **Step 8: Check the gates and commit**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): animate the context menu and close-window dialog with render-only ghosts"
```

---

### Task 7: The consent dialog

Separated from Task 6 deliberately. The spec flags it: a security prompt that is still on screen after it stopped accepting input deserves its own review. `pending_consent` goes `None` at dismiss time, so no answer can be given during the fade — the test below is what pins that.

**Files:**
- Modify: `state/mod.rs` (`pending_consent_opening` / `_closing`, the dismiss helper, `retire_ghosts`, `has_active_animation`, one new test)
- Modify: `renderer/overlay/dialog.rs:351` (`build_consent_dialog_verts` takes `dialog: &ConsentDialog`)
- Modify: `renderer/event_handler/consent.rs:34,81,123` (construction through a helper), plus every `pending_consent = None` site
- Modify: `renderer/render_frame.rs:1195-1214`

**Interfaces:**
- Consumes: everything Task 6 produced, including `option_surface_progress` and `retire_ghosts`.
- Produces: `ClientState.pending_consent_opening: Option<Timed>`, `ClientState.pending_consent_closing: Option<(ConsentDialog, Timed)>`, `show_consent_dialog`, `dismiss_consent_dialog`.

`ConsentDialog` is `{ kind: ConsentKind, selected: usize }`; confirm it derives `Clone` (`grep -n "derive" -A 1 nexterm-client-gpu/src/state/consent.rs`) and add `Clone` to the derive if it does not — a two-field struct of a plain enum and a `usize` is `Copy`-cheap either way.

- [ ] **Step 1: Write the failing test**

```rust
    /// The security-relevant property: a consent dialog that is fading out
    /// is no longer answerable. `pending_consent` is what every input path
    /// consults, and it is `None` from the instant the user answered.
    #[test]
    fn a_fading_consent_dialog_cannot_be_answered() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.show_consent_dialog(
            ConsentDialog::new(ConsentKind::ClipboardWrite),
            t0,
            &anim,
        );
        assert!(state.pending_consent.is_some());

        state.dismiss_consent_dialog(t0, &anim);
        assert!(
            state.pending_consent.is_none(),
            "no input path may see an answerable dialog during the fade"
        );
        assert!(state.pending_consent_closing.is_some());
        assert!(state.has_active_animation(t0, 200));

        let done = t0 + Duration::from_millis(150);
        state.retire_ghosts(done);
        assert!(state.pending_consent_closing.is_none());
        assert!(!state.has_active_animation(done, 200));
    }
```

Use whichever `ConsentKind` variant exists — check with `grep -n "pub enum ConsentKind" -A 12 nexterm-client-gpu/src/state/consent.rs` and pick the simplest variant, constructing any payload it requires.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu fading_consent_dialog`
Expected: FAIL — `no method named show_consent_dialog`.

- [ ] **Step 3: Implement**

Add the two fields, and to `impl ClientState`:

```rust
    /// Show a consent dialog, starting its entrance (UI/UX v3 P3b).
    pub fn show_consent_dialog(
        &mut self,
        dialog: ConsentDialog,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::SLOW);
        self.pending_consent_closing = None;
        self.pending_consent_opening = Some(Timed::new(now, ms, Curve::DecelerateMax));
        self.pending_consent = Some(dialog);
    }

    /// Dismiss the consent dialog, leaving a render-only ghost.
    ///
    /// `pending_consent` goes `None` here, not when the fade ends: a
    /// security prompt stops accepting input the moment it is answered.
    pub fn dismiss_consent_dialog(
        &mut self,
        now: Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        if let Some(dialog) = self.pending_consent.take() {
            let ms = anim.scaled_duration_ms(duration::FAST);
            self.pending_consent_closing = Some((dialog, Timed::new(now, ms, Curve::AccelerateMax)));
        }
    }
```

Extend `retire_ghosts` with the third slot and `has_active_animation` with the third pair of clauses, in the same shape as Task 6.

- [ ] **Step 4: Route the call sites and the renderer**

`renderer/event_handler/consent.rs:34,81,123` become `self.app.state.show_consent_dialog(crate::state::ConsentDialog::new(...), now, &self.app.config.animations);`. Every `pending_consent = None` becomes `dismiss_consent_dialog(...)` — grep for them, including the paths that answer the prompt.

`renderer/render_frame.rs`: the consent block takes the Task 6 live-or-ghost shape, passing `dialog` to `build_consent_dialog_verts`, whose first parameter changes from `state` to `dialog: &ConsentDialog`.

- [ ] **Step 5: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS.

- [ ] **Step 6: Check the gates and commit**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): animate the consent dialog, keeping it unanswerable while it fades"
```

---

### Task 8: The password modal's redacted ghost

The one surface whose ghost may not be a clone. `PasswordModal.input` is a private `zeroize::Zeroizing<String>` whose whole purpose is to keep the secret's lifetime short; a ghost holding a clone would extend it for the length of an animation, for a cosmetic effect.

**Files:**
- Modify: `host_manager.rs:328-350` (add `PasswordModalView` + `PasswordModalGhost` + `PasswordModal::view`)
- Modify: `host_manager.rs:424-440` (`password_modal_opening` / `password_modal_closing` on `HostManager`, and the show/dismiss helpers)
- Modify: `renderer/overlay/dialog.rs:19-40` (`build_password_modal_verts` takes `view: PasswordModalView`)
- Modify: `renderer/render_frame.rs:1024-1040`
- Modify: `state/mod.rs` `has_active_animation`; `renderer/event_handler/lifecycle.rs` retire sweep
- Modify: `renderer/input_handler/mod.rs:1043,1063` (dismiss through the helper)
- Test: new `#[cfg(test)] mod` tests in `host_manager.rs`

**Interfaces:**
- Consumes: `Timed`, `Curve`, `duration`, `apply_surface_fade`.
- Produces:
  ```rust
  pub struct PasswordModalGhost { /* no secret; see below */ }
  pub struct PasswordModalView<'a> {
      pub username: &'a str,
      pub host: &'a str,
      pub port: u16,
      pub input_len: usize,
      pub error: Option<&'a str>,
      pub remember: bool,
      pub prefilled: bool,
  }
  impl PasswordModal { pub fn view(&self) -> PasswordModalView<'_>; }
  impl PasswordModalGhost { pub fn view(&self) -> PasswordModalView<'_>; }
  ```
  `HostManager` gains `password_modal_opening: Option<Timed>` and `password_modal_closing: Option<(PasswordModalGhost, Timed)>`, plus `show_password_modal(&mut self, modal: PasswordModal, now, anim)`, `dismiss_password_modal(&mut self, now, anim)` and `retire_password_modal(&mut self, now)`.

A `View` rather than a direct ghost read is what makes the security boundary structural: the builder can only see the seven fields the view exposes, whether it is drawing the live modal or the ghost, so there is no path by which it could reach `input`.

- [ ] **Step 1: Write the failing tests**

Add to `host_manager.rs`:

```rust
#[cfg(test)]
mod password_ghost_tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn host() -> HostConfig {
        let mut h = HostConfig::default();
        h.name = "prod".to_string();
        h.username = "deploy".to_string();
        h.host = "example.invalid".to_string();
        h.port = 22;
        h
    }

    /// The security property, stated as a test so a future "just clone the
    /// modal" refactor fails CI instead of passing review. `PasswordModal`
    /// keeps `input` private precisely so it cannot be copied; the ghost
    /// must not carry it in any form, and `size_of` is the coarse but
    /// unambiguous witness: adding a `String`/`Zeroizing<String>` field
    /// would grow the struct.
    #[test]
    fn the_ghost_carries_no_string_the_secret_could_live_in() {
        use std::mem::size_of;
        // username + host + error are the only owned strings the ghost may
        // hold, plus a usize, a u16 and two bools.
        let max = 3 * size_of::<String>() + size_of::<usize>() + 8;
        assert!(
            size_of::<PasswordModalGhost>() <= max,
            "PasswordModalGhost grew to {} bytes (max {max}); did a secret \
             field get added?",
            size_of::<PasswordModalGhost>()
        );
    }

    /// What the renderer sees must be identical before and after the
    /// modal becomes a ghost — except that it is derived from a count, so
    /// the mask keeps its length and the characters are never available.
    #[test]
    fn the_ghost_view_matches_the_live_view() {
        let mut hm = HostManager::new(vec![]);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        let mut modal = PasswordModal::new(host());
        modal.push_char('a');
        modal.push_char('b');
        let live_len = modal.input_len();

        hm.show_password_modal(modal, t0, &anim);
        let live = hm
            .password_modal
            .as_ref()
            .expect("modal must be live after show")
            .view();
        let (live_user, live_host, live_port) = (live.username.to_string(), live.host.to_string(), live.port);

        hm.dismiss_password_modal(t0, &anim);
        assert!(hm.password_modal.is_none(), "the secret must be dropped now");
        let (ghost, _) = hm
            .password_modal_closing
            .as_ref()
            .expect("a ghost must remain");
        let g = ghost.view();
        assert_eq!(g.input_len, live_len, "the mask must keep its length");
        assert_eq!(g.username, live_user);
        assert_eq!(g.host, live_host);
        assert_eq!(g.port, live_port);
    }

    #[test]
    fn a_dismissed_password_modal_retires_after_its_exit() {
        let mut hm = HostManager::new(vec![]);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        hm.show_password_modal(PasswordModal::new(host()), t0, &anim);
        hm.dismiss_password_modal(t0, &anim);
        let mid = t0 + Duration::from_millis(50);
        hm.retire_password_modal(mid);
        assert!(hm.password_modal_closing.is_some());
        let done = t0 + Duration::from_millis(150);
        hm.retire_password_modal(done);
        assert!(hm.password_modal_closing.is_none());
    }
}
```

`PasswordModal::push_char` is the existing input method — confirm its name with `grep -n "pub fn " nexterm-client-gpu/src/host_manager.rs | sed -n '/PasswordModal/,$p'` and use whatever the modal actually exposes; if it takes a `char` by another name, adjust the two calls. `HostConfig::default()` may not exist — if not, build the host the way the existing host-manager tests do.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu password_ghost_tests`
Expected: FAIL — `cannot find type PasswordModalGhost`.

- [ ] **Step 3: Implement the view and the ghost**

In `host_manager.rs`, after `impl PasswordModal`:

```rust
/// Everything the password modal's renderer is allowed to see.
///
/// `build_password_modal_verts` used to read the modal's public fields
/// directly under a comment explaining that it must touch `input` only via
/// `input_len()` (HIGH H-6). Routing it through this view makes that
/// boundary structural instead of advisory: there is no field here the
/// secret could reach, so the mask is drawn from a count in both the live
/// and the fading case.
pub struct PasswordModalView<'a> {
    pub username: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub input_len: usize,
    pub error: Option<&'a str>,
    pub remember: bool,
    pub prefilled: bool,
}

/// What the password modal's exit animation needs to draw, and nothing more.
///
/// Deliberately **not** a clone of [`PasswordModal`]: its `input` is a
/// private `Zeroizing<String>` whose whole point is to minimise how long
/// the secret exists in memory. Keeping a second copy alive for the length
/// of a fade-out would pay a security cost for a cosmetic effect.
pub struct PasswordModalGhost {
    username: String,
    host: String,
    port: u16,
    input_len: usize,
    error: Option<String>,
    remember: bool,
    prefilled: bool,
}

impl PasswordModal {
    /// Borrow the fields the renderer may see.
    pub fn view(&self) -> PasswordModalView<'_> {
        PasswordModalView {
            username: &self.host.username,
            host: &self.host.host,
            port: self.host.port,
            input_len: self.input_len(),
            error: self.error.as_deref(),
            remember: self.remember,
            prefilled: self.prefilled,
        }
    }

    /// Snapshot the modal for its exit animation, leaving the secret behind.
    fn to_ghost(&self) -> PasswordModalGhost {
        PasswordModalGhost {
            username: self.host.username.clone(),
            host: self.host.host.clone(),
            port: self.host.port,
            input_len: self.input_len(),
            error: self.error.clone(),
            remember: self.remember,
            prefilled: self.prefilled,
        }
    }
}

impl PasswordModalGhost {
    /// Borrow the same view the live modal exposes.
    pub fn view(&self) -> PasswordModalView<'_> {
        PasswordModalView {
            username: &self.username,
            host: &self.host,
            port: self.port,
            input_len: self.input_len,
            error: self.error.as_deref(),
            remember: self.remember,
            prefilled: self.prefilled,
        }
    }
}
```

Add to `HostManager` the two fields (`password_modal_opening: Option<crate::animations::Timed>`, `password_modal_closing: Option<(PasswordModalGhost, crate::animations::Timed)>`), initialise them to `None` in `new`, and add:

```rust
    /// Show `modal`, starting its entrance (UI/UX v3 P3b).
    pub fn show_password_modal(
        &mut self,
        modal: PasswordModal,
        now: std::time::Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::SLOW);
        self.password_modal_closing = None;
        self.password_modal_opening = Some(Timed::new(now, ms, Curve::DecelerateMax));
        self.password_modal = Some(modal);
    }

    /// Dismiss the modal, dropping the secret and leaving a redacted ghost.
    pub fn dismiss_password_modal(
        &mut self,
        now: std::time::Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, Timed, duration};

        if let Some(modal) = self.password_modal.take() {
            let ms = anim.scaled_duration_ms(duration::FAST);
            let ghost = modal.to_ghost();
            // `modal` is dropped here, zeroing `input`.
            self.password_modal_closing = Some((ghost, Timed::new(now, ms, Curve::AccelerateMax)));
        }
    }

    /// Drop a finished exit animation.
    pub fn retire_password_modal(&mut self, now: std::time::Instant) {
        if self
            .password_modal_closing
            .as_ref()
            .is_some_and(|(_, t)| t.is_done(now))
        {
            self.password_modal_closing = None;
        }
    }

    /// Visibility in `[0, 1]` of the password modal (UI/UX v3 P3b).
    pub fn password_modal_progress(&self, now: std::time::Instant) -> f32 {
        if self.password_modal.is_some() {
            return self.password_modal_opening.map_or(1.0, |t| t.progress(now));
        }
        self.password_modal_closing
            .as_ref()
            .map_or(0.0, |(_, t)| 1.0 - t.progress(now))
    }

    /// What the renderer should draw: the live modal, else the ghost.
    pub fn password_modal_view(&self) -> Option<PasswordModalView<'_>> {
        if let Some(modal) = &self.password_modal {
            return Some(modal.view());
        }
        self.password_modal_closing.as_ref().map(|(g, _)| g.view())
    }

    /// Whether the password modal needs another frame.
    pub fn password_modal_is_active(&self, now: std::time::Instant) -> bool {
        if self
            .password_modal_closing
            .as_ref()
            .is_some_and(|(_, t)| !t.is_done(now))
        {
            return true;
        }
        self.password_modal.is_some()
            && self.password_modal_opening.is_some_and(|t| !t.is_done(now))
    }
```

- [ ] **Step 4: Move the builder onto the view**

`renderer/overlay/dialog.rs` — change `build_password_modal_verts`'s `state: &ClientState` parameter to `view: &PasswordModalView<'_>`, delete the `let Some(modal) = &state.host_manager.password_modal else { return; };`, and replace `modal.host.username` → `view.username`, `modal.host.host` → `view.host`, `modal.host.port` → `view.port`, `modal.input_len()` → `view.input_len`, `modal.error` → `view.error`, `modal.remember` → `view.remember`, `modal.prefilled` → `view.prefilled`. Replace the H-6 comment with:

```rust
        // HIGH H-6, made structural in UI/UX v3 P3b: this builder is handed a
        // `PasswordModalView`, which has no field the password could be in.
        // The mask is drawn from `input_len`, never from the characters.
```

If the body reads anything else off `state` (e.g. tokens or scheme data), pass it as an explicit parameter.

`renderer/render_frame.rs`:

```rust
        if let Some(view) = state.host_manager.password_modal_view() {
            let (bg_start, text_start) = (bg_verts.len(), text_verts.len());
            self.build_password_modal_verts(
                &view,
                &tokens,
                sw,
                sh,
                cell_w,
                cell_h,
                panel_acrylic_mix,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
            super::overlay::fade::apply_surface_fade(
                &mut bg_verts[bg_start..],
                &mut text_verts[text_start..],
                state.host_manager.password_modal_progress(frame_now),
            );
        }
```

The borrow of `state` for `view` overlaps the `&self` builder call and the `&mut bg_verts` locals only, so this compiles as written; if the surrounding block holds `state` mutably, bind the progress before the builder call and let `view` end its borrow first.

- [ ] **Step 5: Route the call sites, the aggregate and the sweep**

`renderer/input_handler/mod.rs:1043` and `:1063` (`password_modal = None`) become `dismiss_password_modal(...)`. The construction site (`grep -rn "password_modal = Some" nexterm-client-gpu/src`) becomes `show_password_modal(...)`. `state/mod.rs` `has_active_animation` gains:

```rust
        if self.host_manager.password_modal_is_active(now) {
            return true;
        }
```

and `renderer/event_handler/lifecycle.rs` gains `self.app.state.host_manager.retire_password_modal(now);`.

- [ ] **Step 6: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS, including the three ghost tests.

- [ ] **Step 7: Check the gates and commit**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): animate the password modal with a redacted, secret-free ghost"
```

---

### Task 9: The tooltip, the idle property, and the crate note

The eleventh surface has no stored openness: it is drawn when `HoverDwell::is_ready(now)` is true, from `renderer/overlay/settings/theme_tab.rs:133` only. Its motion therefore hangs off the dwell predicate.

**Files:**
- Modify: `settings/mod.rs` (add `tooltip_motion: SurfaceMotion`)
- Modify: `settings/hover.rs` (no state change; add the driver method on `SettingsPanel` in `settings/mod.rs`)
- Modify: `renderer/event_handler/mouse.rs:396` area (drive the motion where `hover_widget` is set) and the lifecycle tick
- Modify: `renderer/overlay/settings/theme_tab.rs:131-160` (take `now` and a fade factor instead of calling `Instant::now()` inline)
- Modify: `state/mod.rs` (`has_active_animation` clause, and the idle-property test)
- Modify: `nexterm-client-gpu/CLAUDE.md`

**Interfaces:**
- Consumes: `SurfaceMotion`, `apply_surface_fade`, `HoverDwell::is_ready`.
- Produces: `SettingsPanel.tooltip_motion: SurfaceMotion` and
  `pub fn tick_tooltip(&mut self, now: Instant, anim: &AnimationsConfig)` — called once per frame; opens the motion when a dwell is ready and closes it otherwise.

- [ ] **Step 1: Write the failing tests**

Add to `settings/open_close_animation_tests.rs`:

```rust
/// The tooltip has no open flag: `tick_tooltip` turns the dwell predicate
/// into an entrance and an exit.
#[test]
fn the_tooltip_opens_once_the_dwell_is_ready_and_closes_when_it_clears() {
    use crate::settings_panel::hover::{HoverDwell, TOOLTIP_DELAY_MS};

    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.hover_widget = Some(HoverDwell::enter(None, 2, 0, t0));

    sp.tick_tooltip(t0, &on());
    assert!(!sp.tooltip_motion.is_visible(), "not yet — still dwelling");

    let ready = t0 + Duration::from_millis(TOOLTIP_DELAY_MS as u64);
    sp.tick_tooltip(ready, &on());
    assert!(sp.tooltip_motion.is_visible());
    assert!(sp.tooltip_motion.progress(ready).abs() < 1e-3);
    let shown = ready + Duration::from_millis(150);
    assert!((sp.tooltip_motion.progress(shown) - 1.0).abs() < 1e-3);

    // The pointer leaves the panel.
    sp.hover_widget = None;
    sp.tick_tooltip(shown, &on());
    assert!(sp.tooltip_motion.is_visible(), "it must fade, not vanish");
    let gone = shown + Duration::from_millis(100);
    assert!(sp.tooltip_motion.progress(gone).abs() < 1e-3);
    sp.tooltip_motion.retire(gone);
    assert!(!sp.tooltip_motion.is_visible());
}

/// Moving to another control restarts the dwell, so the tooltip that was
/// showing must close.
#[test]
fn moving_to_another_control_closes_the_tooltip() {
    use crate::settings_panel::hover::{HoverDwell, TOOLTIP_DELAY_MS};

    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.hover_widget = Some(HoverDwell::enter(None, 2, 0, t0));
    let ready = t0 + Duration::from_millis(TOOLTIP_DELAY_MS as u64);
    sp.tick_tooltip(ready, &on());
    assert!(sp.tooltip_motion.is_visible());

    sp.hover_widget = Some(HoverDwell::enter(sp.hover_widget, 2, 1, ready));
    sp.tick_tooltip(ready, &on());
    assert!(sp.tooltip_motion.progress(ready + Duration::from_millis(100)).abs() < 1e-3);
}
```

Adjust the `use` path if `hover` is re-exported under a different name (`grep -n "mod hover\|pub use hover" nexterm-client-gpu/src/settings/mod.rs`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu tooltip_opens_once_the_dwell`
Expected: FAIL — `no method named tick_tooltip`.

- [ ] **Step 3: Implement the driver**

In `settings/mod.rs`, add the field:

```rust
    /// Tooltip entrance/exit (UI/UX v3 P3b). The tooltip has no stored
    /// openness — it is a predicate over `hover_widget`'s dwell timer — so
    /// `tick_tooltip` translates that predicate into motion once per frame.
    pub tooltip_motion: crate::animations::SurfaceMotion,
```

(plus `tooltip_motion: crate::animations::SurfaceMotion::default(),` in the `Default` body), and the method:

```rust
    /// Open or close the tooltip motion from the dwell predicate.
    ///
    /// Called once per frame. Idempotent while the answer does not change:
    /// `SurfaceMotion::open` on an already-open motion would restart the
    /// entrance, so the current state is checked first.
    pub fn tick_tooltip(
        &mut self,
        now: std::time::Instant,
        anim: &nexterm_config::AnimationsConfig,
    ) {
        use crate::animations::{Curve, duration};

        let dwell = self.hover_widget.filter(|d| d.is_ready(now));
        let ready = dwell.is_some();
        if ready == self.tooltip_shown {
            return;
        }
        self.tooltip_shown = ready;
        if let Some(d) = dwell {
            // Capture the anchor now: the exit animation still needs it
            // after `hover_widget` has cleared.
            self.tooltip_snapshot = Some((d.category, d.index));
            self.tooltip_motion
                .open(now, anim, duration::FAST, Curve::DecelerateMax);
        } else {
            self.tooltip_motion
                .close(now, anim, duration::FASTER, Curve::AccelerateMax);
        }
    }
```

This needs two more fields: `tooltip_snapshot: Option<(u8, u16)>` (default `None`), specified in Step 4 below, and `tooltip_shown: bool` (default `false`), documented as "what `tick_tooltip` last decided; the edge detector that keeps a per-frame call idempotent". Moving to another control resets `HoverDwell.since`, so `is_ready` goes false on the next frame and the edge fires — which is what the second test pins.

Call it from `renderer/event_handler/lifecycle.rs`, in the same sweep, before the retires:

```rust
        let anim = self.app.config.animations.clone();
        self.app.state.settings_panel.tick_tooltip(now, &anim);
        self.app.state.settings_panel.tooltip_motion.retire(now);
```

If `AnimationsConfig` is not `Clone`, borrow it in a narrower scope or read the two fields it needs; do not clone the whole `Config`.

- [ ] **Step 4: Fade the tooltip in the renderer**

`renderer/overlay/settings/theme_tab.rs` — the tooltip function currently calls `std::time::Instant::now()` inline and returns early when the dwell is not ready. Change it to take `now: std::time::Instant` and `fade: f32` as parameters, replace the readiness early-return with `if fade <= 0.0 { return; }` (the motion is now the authority on whether to draw), record the vertex range at the top, and apply `apply_surface_fade` at the end. Pass `now` and `sp.tooltip_motion.progress(now)` from its caller, which already has both `sp` and a frame `Instant`.

Keep the `spec`-lookup early-returns: while the tooltip fades out, `hover_widget` may already be `None`, so the anchor and text must come from a snapshot rather than a fresh lookup. Store that snapshot when the motion opens — add to `SettingsPanel`:

```rust
    /// The tooltip's anchor and text, captured when it opened, so the exit
    /// animation can draw it after `hover_widget` has already cleared.
    pub tooltip_snapshot: Option<(u8, u16)>,
```

set to `Some((d.category, d.index))` in the `ready` branch of `tick_tooltip`, and have the renderer resolve the spec from the snapshot instead of from `hover_widget`. Do not clear it in the close branch: the ghost needs it, and the next open overwrites it.

- [ ] **Step 5: Add the last aggregate clause and the idle property test**

`state/mod.rs`:

```rust
        if self.settings_panel.tooltip_motion.is_active(now) {
            return true;
        }
```

and the phase's acceptance criterion, as its own test:

```rust
    /// P3b's acceptance criterion: a state with nothing animating must not
    /// ask for frames. Eleven surfaces now have a clause in the aggregate,
    /// and each is a way for this to regress.
    #[test]
    fn a_fully_idle_state_wants_no_animation_frames() {
        let state = ClientState::new(80, 24, 1000);
        let now = Instant::now();
        assert!(!state.has_active_animation(now, 200));
        assert!(!state.has_active_animation(now, 0));
    }
```

- [ ] **Step 6: Run the whole workspace suite**

Run: `cargo test --workspace`
Expected: PASS. Then confirm the sweep is complete:

Run: `grep -rn "is_active\|retire" nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs nexterm-client-gpu/src/state/mod.rs | grep -c motion`
Expected: every migrated surface appears in both the lifecycle sweep and the aggregate. Cross-check against the eleven: settings panel, palette, macro picker, host manager, block-name modal, file-transfer dialog, context menu, close-window dialog, consent dialog, password modal, tooltip.

- [ ] **Step 7: Record the rule in the crate guide**

In `nexterm-client-gpu/CLAUDE.md`, under the animations/renderer section, add:

```markdown
- **Adding an overlay surface**: give it a `SurfaceMotion` (or, for an
  `Option`-shaped surface, an `Option<Timed>` entrance plus a
  `(ghost, Timed)` exit), then add it in three places or it will not
  animate: the `has_active_animation` clause in `state/mod.rs`, the retire
  call in `renderer/event_handler/lifecycle.rs`, and the recorded-range
  `apply_surface_fade` call around its builder in `render_frame.rs`. There
  is no registry that catches an omission.
```

- [ ] **Step 8: Check the gates and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src nexterm-client-gpu/CLAUDE.md
git commit -m "feat(client): animate the settings tooltip and pin the idle-frame property"
```

---

## Closing out P3b1

- [ ] Update `docs/plans/ui-ux-modernization-v3.md`: note under "P3 — Motion language" that P3b1 shipped, and add to the on-device verification backlog the three items CI cannot judge: whether 300 ms entrances read as arrival or as lag, whether a fading consent dialog is acceptable while unanswerable, and whether the context menu's 150/100 ms pair feels immediate.
- [ ] Open the PR against `master` with an English title and body. Suggested title: `feat(client): shared SurfaceMotion and open/close motion for eleven overlay surfaces (UI/UX v3 P3b1)`.
- [ ] In the PR body, state plainly what was not verified: motion cannot be captured by the repo's screenshot convention, and nothing in this phase was seen on real hardware.
- [ ] Confirm CI is green before merging, including the flatpak job (no `Cargo.lock` change is expected, so it should diff clean).
- [x] Known-and-accepted: the four `show_*` helpers for the `Option`-shaped surfaces (`show_context_menu`, `show_consent_dialog`, `show_close_window_dialog` in `state/mod.rs`, `show_password_modal` in `host_manager.rs`) discard the ghost and start the entrance with `Timed::new(now, ...)` from 0, while `SurfaceMotion::open` (used by the six `bool`-shaped surfaces) calls `Timed::resuming_at` when an exit is in flight, letting those surfaces reopen seamlessly mid-fade. Reopening one of the four `Option`-shaped surfaces inside its exit window (context menu's ~100 ms, for example) therefore shows a visible alpha jump instead of a seamless resume. This was raised in the whole-branch review and decided, not missed: resuming would mean inverting each ghost's progress into `Timed::resuming_at` across four helpers — new behaviour beyond this phase's plan, wanting its own tests — and the affected windows are only 100-150 ms. Deferred past P3b1; revisit only if it proves noticeable on real hardware.
