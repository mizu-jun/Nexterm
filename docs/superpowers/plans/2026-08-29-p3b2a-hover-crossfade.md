# P3b2a: `HoverTransition` and the Two Overlay Hover Models — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the settings widget rows and the context-menu items a hover cross-fade, driven by one generic `HoverTransition<Id>` that the two title-bar models will reuse in P3b2b.

**Architecture:** A hover weight is a scalar in `[0, 1]` per hovered id, not a post-pass over vertices. `HoverTransition<Id>` holds `from`/`to`/`Timed` and answers `weight(id, now)`; each draw site lerps between its resting and hovered appearance using that weight. The logical hover state (`hover_widget`, `ContextMenu.hovered`) stays the truth for tooltips, hit-testing and AccessKit; only colour choice consults the transition.

**Tech Stack:** Rust 2024, `nexterm-client-gpu` (wgpu + winit + cosmic-text). No new dependency.

**Spec:** `docs/superpowers/specs/2026-08-29-p3b2-hover-crossfade-design.md`. This plan implements only its **P3b2a** row (the two overlay models); the tab bar and window buttons are P3b2b.

## Global Constraints

- No `unwrap()`. Use `?` or `expect("reason")` with a concrete message.
- Comments, doc-comments and commit messages in this repo are **English**.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` must be green before any commit.
- No new config key, no new user-facing string, no locale change, no `Cargo.lock` change.
- Every duration passed to `Timed` must have gone through `AnimationsConfig::scaled_duration_ms`, so `animations.enabled = false` / `intensity = "off"` yields 0 and the transition is born finished. `HoverTransition::retarget` does this internally; never construct a `Timed` for hover directly.
- **Durations are fixed by the spec and must not be re-tuned:** both models use `duration::FASTER` (100) with `Curve::EasyEase`, in and out.
- **The logical hover state stays the truth.** `SettingsPanel.hover_widget` and `ContextMenu.hovered` keep their current meaning and their current write sites; the tooltip's `HoverDwell::is_ready` path (P3b1) must be untouched. Only colour choice reads the transition.
- **Focus precedence is unchanged.** `draw_row_background` paints an opaque `surface_2` for a focused row and no hover fill; that stays exactly as it is, whatever the hover weight says.
- `WidgetSpec.hovered: bool` **stays**. It is the hit-test/semantic answer to "is the pointer over this"; the weight is a rendering concern and travels separately. Do not replace one with the other.
- Paths in this plan are relative to `nexterm-client-gpu/src/`.

## What P3b1 established that this plan reuses

- `animations/surface.rs` holds `SurfaceMotion`, the analogous type for surface open/close. `HoverTransition` is its sibling, not its subtype — surfaces fade as a whole, hover lerps colours. **`apply_surface_fade` is not used anywhere in this plan.**
- `ClientState::has_active_animation` is the one place that decides whether the event loop requests another frame. `nexterm-client-gpu/CLAUDE.md` records the rule: a new animated thing adds a clause there or it silently never animates.
- `Timed::resuming_at(now, value, ms, curve)` is how an interrupted transition stays continuous — read the on-screen value first, then build the animation from it.

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `animations/hover.rs` **(new)** | `HoverTransition<Id>` and its tests | 1 |
| `animations/mod.rs` | `mod hover; pub use hover::HoverTransition;` | 1 |
| `color_util.rs` | `lerp_rgba`, beside the existing `with_alpha` | 2 |
| `settings/mod.rs` | `SettingsPanel.hover_transition` | 3 |
| `renderer/event_handler/mouse.rs:395` | retarget the widget transition | 3 |
| `renderer/overlay/widgets/draw/mod.rs` | `WidgetTheme` gains the transition and `now`; `draw_row_background` lerps | 3 |
| `renderer/overlay/settings/*_tab.rs` (10 files) | one `now` parameter, two `WidgetTheme` fields | 3 |
| `renderer/overlay/settings/mod.rs` | pass `now` to each tab renderer | 3 |
| `state/menus.rs` | `ContextMenu.hover_transition` | 4 |
| `renderer/event_handler/mouse.rs:581`, `event_handler/accessibility.rs:265` | retarget the menu transition | 4 |
| `renderer/overlay/dialog.rs:267-299` | lerp the item fill, the accent line and the text | 4 |
| `state/mod.rs` | two `has_active_animation` clauses, and the tests | 3, 4 |

---

### Task 1: `HoverTransition<Id>`

**Files:**
- Create: `animations/hover.rs`
- Modify: `animations/mod.rs` (the `mod` / `pub use` block, beside `mod surface;`)

**Interfaces:**
- Consumes: `animations::{Timed, Curve}`, `nexterm_config::AnimationsConfig::scaled_duration_ms`.
- Produces: `crate::animations::HoverTransition<Id>` with
  `Default`,
  `retarget(&mut self, to: Option<Id>, now: Instant, anim: &AnimationsConfig)`,
  `weight(&self, id: Id, now: Instant) -> f32`,
  `is_active(&self, now: Instant) -> bool`,
  and `target(&self) -> Option<Id>` for the idempotence check.

- [ ] **Step 1: Write the failing tests**

Create `animations/hover.rs` with the test module only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::AnimationsConfig;
    use std::time::Duration;

    fn on() -> AnimationsConfig {
        AnimationsConfig::default()
    }

    fn off() -> AnimationsConfig {
        AnimationsConfig {
            enabled: false,
            ..AnimationsConfig::default()
        }
    }

    /// 100 ms is `duration::FASTER`, the constant both P3b2 models use.
    const MS: u64 = 100;

    #[test]
    fn a_fresh_transition_weighs_nothing() {
        let h: HoverTransition<u32> = HoverTransition::default();
        let now = Instant::now();
        assert!(h.weight(1, now).abs() < 1e-4);
        assert!(!h.is_active(now));
        assert_eq!(h.target(), None);
    }

    #[test]
    fn entering_an_item_fades_it_in() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(7), t0, &on());
        assert_eq!(h.target(), Some(7));
        assert!(h.weight(7, t0).abs() < 1e-3);
        assert!(h.is_active(t0));
        let done = t0 + Duration::from_millis(MS);
        assert!((h.weight(7, done) - 1.0).abs() < 1e-3);
        assert!(!h.is_active(done));
    }

    /// The cross-fade: the item being left and the item being entered are
    /// complementary at every instant, and nothing else weighs anything.
    #[test]
    fn moving_between_items_cross_fades_them() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &on());
        let settled = t0 + Duration::from_millis(MS);
        h.retarget(Some(2), settled, &on());

        let mid = settled + Duration::from_millis(50);
        let (w1, w2) = (h.weight(1, mid), h.weight(2, mid));
        assert!(w1 > 0.1 && w1 < 0.9, "outgoing item should be mid-fade: {w1}");
        assert!(w2 > 0.1 && w2 < 0.9, "incoming item should be mid-fade: {w2}");
        assert!((w1 + w2 - 1.0).abs() < 1e-3, "must be complementary");
        assert!(h.weight(3, mid).abs() < 1e-4, "untouched item weighs 0");

        let done = settled + Duration::from_millis(MS);
        assert!(h.weight(1, done).abs() < 1e-3);
        assert!((h.weight(2, done) - 1.0).abs() < 1e-3);
    }

    /// Leaving the model entirely still fades the last item out — this is why
    /// the transition cannot live on the logical hover state, which goes
    /// `None` the moment the pointer leaves.
    #[test]
    fn leaving_fades_the_last_item_out() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &on());
        let settled = t0 + Duration::from_millis(MS);
        h.retarget(None, settled, &on());
        assert_eq!(h.target(), None);

        let mid = settled + Duration::from_millis(50);
        let w = h.weight(1, mid);
        assert!(w > 0.1 && w < 0.9, "must still be drawn while fading: {w}");
        assert!(h.is_active(mid));

        let done = settled + Duration::from_millis(MS);
        assert!(h.weight(1, done).abs() < 1e-3);
        assert!(!h.is_active(done));
    }

    /// Retargeting to the same id must not restart the fade — `retarget` is
    /// called from a per-motion handler that fires far more often than the
    /// hovered id changes.
    #[test]
    fn retargeting_the_same_item_is_a_no_op() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(7), t0, &on());
        let mid = t0 + Duration::from_millis(50);
        let before = h.weight(7, mid);
        h.retarget(Some(7), mid, &on());
        let after = h.weight(7, mid);
        assert!((after - before).abs() < 1e-4, "fade restarted: {before} -> {after}");
        // And it still finishes on the original schedule.
        assert!((h.weight(7, t0 + Duration::from_millis(MS)) - 1.0).abs() < 1e-3);
    }

    /// The defect the two-timer design exists to prevent. A single `Timed`
    /// with the pair summing to 1 makes the *incoming* item jump to whatever
    /// the outgoing one held — here 0.5 — the instant the pointer crosses
    /// the boundary. Sweeping a list crosses boundaries faster than the
    /// 100 ms fade, so that jump is the common case, not the corner.
    #[test]
    fn interrupting_mid_fade_jumps_neither_item() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &on());
        let mid = t0 + Duration::from_millis(50);
        let out_before = h.weight(1, mid);
        assert!(
            out_before > 0.1 && out_before < 0.9,
            "the test needs item 1 genuinely mid-fade: {out_before}"
        );

        h.retarget(Some(2), mid, &on());

        let out_after = h.weight(1, mid);
        assert!(
            (out_after - out_before).abs() < 1e-3,
            "the outgoing item jumped: {out_before} -> {out_after}"
        );
        let in_after = h.weight(2, mid);
        assert!(
            in_after.abs() < 1e-3,
            "the incoming item must start from nothing, not from the \
             outgoing item's weight: {in_after}"
        );
    }

    /// Coming back to the item that is still fading out must resume it, not
    /// restart it from 0 — the pointer wobbling on a row boundary is the
    /// gesture this covers.
    #[test]
    fn returning_to_a_fading_item_resumes_it() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &on());
        let settled = t0 + Duration::from_millis(100);
        h.retarget(Some(2), settled, &on());

        let mid = settled + Duration::from_millis(50);
        let held = h.weight(1, mid);
        assert!(held > 0.1 && held < 0.9, "item 1 should be mid-decay: {held}");

        h.retarget(Some(1), mid, &on());
        let resumed = h.weight(1, mid);
        assert!(
            (resumed - held).abs() < 5e-2,
            "returning restarted the fade: {held} -> {resumed}"
        );
        // And it climbs back to 1 rather than stalling.
        assert!((h.weight(1, mid + Duration::from_millis(100)) - 1.0).abs() < 1e-2);
    }

    /// The reduced-motion path.
    #[test]
    fn disabled_animations_snap() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &off());
        assert!((h.weight(1, t0) - 1.0).abs() < 1e-4);
        assert!(!h.is_active(t0));
        h.retarget(Some(2), t0, &off());
        assert!(h.weight(1, t0).abs() < 1e-4);
        assert!((h.weight(2, t0) - 1.0).abs() < 1e-4);
        assert!(!h.is_active(t0));
    }
}
```

Add to `animations/mod.rs`, beside `mod surface;`:

```rust
mod hover;
```

and to the re-export block:

```rust
pub use hover::HoverTransition;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu animations::hover`
Expected: FAIL — `cannot find type HoverTransition in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `animations/hover.rs`:

```rust
//! Hover cross-fade between two items of one model (UI/UX v3 P3b2).
//!
//! A hover weight is a scalar, not a layer: three of the four hover models
//! in this client interpolate more than one property (a fill, an accent
//! line, a text colour), and two of them compute the hovered colour by
//! brightening the resting one. So this type answers "how hovered is this
//! id, right now" and each draw site lerps its own appearance — unlike
//! `SurfaceMotion`, whose consumers fade a whole surface's vertices.
//!
//! One pointer means one transition **per model**, not globally: moving from
//! a settings row to a tab starts a tab-bar transition while the widget
//! layer's is still fading out. Each model therefore owns its own
//! `HoverTransition`.
//!
//! The logical hover state (`SettingsPanel.hover_widget`,
//! `ContextMenu.hovered`) stays the truth for tooltips, hit-testing and
//! accessibility. It cannot also carry the transition: it goes `None` the
//! moment the pointer leaves, which is exactly when the fade-out must still
//! be running.

use std::time::Instant;

use nexterm_config::AnimationsConfig;

use super::{Curve, Timed, duration};

/// A cross-fade between the previously hovered item and the current one.
///
/// **Two timers, not one.** The obvious form is a single `Timed` with the
/// outgoing item at `1 - progress` and the incoming one at `progress`, so
/// the pair always sums to 1. That is wrong: the invariant only holds when
/// the outgoing item was already at weight 1. Enter row A and, 50 ms later
/// while A is still at 0.5, move to row B — a single timer makes B *jump* to
/// 0.5 on the frame the pointer crosses the boundary. Sweeping down a list
/// crosses boundaries faster than 100 ms routinely, so that form pops on
/// exactly the gesture hover exists to support.
///
/// With two timers the outgoing item decays from the weight it actually held
/// and the incoming one rises from the weight *it* actually held — 0
/// normally, or its partly-decayed value when the pointer comes back to it.
/// The pair does not sum to 1 mid-handoff, which is correct: at that instant
/// neither row is fully hovered.
///
/// One slot is a real limitation: only one item fades out at a time, so
/// sweeping across five rows drops the three intermediate ones to 0 as each
/// is replaced, leaving a trail that cuts off rather than one that fades. A
/// fixed-capacity `id → Timed` map would fix it behind an unchanged
/// `weight()`; a single slot is bounded and matches what the design chose.
#[derive(Debug, Clone, Copy)]
pub struct HoverTransition<Id> {
    /// The item fading out, and the weight it held when it started to.
    from: Option<(Id, f32)>,
    from_anim: Timed,
    /// The item fading in.
    to: Option<Id>,
    to_anim: Timed,
}

impl<Id> Default for HoverTransition<Id> {
    fn default() -> Self {
        // Both animations are born finished; with `from` and `to` both
        // `None`, every id weighs 0 regardless.
        let zero = Timed::new(Instant::now(), 0, Curve::EasyEase);
        Self {
            from: None,
            from_anim: zero,
            to: None,
            to_anim: zero,
        }
    }
}

impl<Id: Copy + PartialEq> HoverTransition<Id> {
    /// Point the transition at `to`, resuming from whatever is on screen.
    ///
    /// Idempotent while `to` is unchanged, because the caller is a
    /// pointer-motion handler that fires far more often than the hovered id
    /// changes; restarting the fade on every frame of a slow drag across one
    /// row would freeze it near 0.
    pub fn retarget(&mut self, to: Option<Id>, now: Instant, anim: &AnimationsConfig) {
        if self.to == to {
            return;
        }
        let ms = anim.scaled_duration_ms(duration::FASTER);
        // Read both weights off the screen *before* overwriting any field.
        let outgoing = self.to.map(|id| (id, self.weight(id, now)));
        let incoming_from = to.map_or(0.0, |id| self.weight(id, now));

        self.from = outgoing;
        self.from_anim = Timed::new(now, ms, Curve::EasyEase);
        self.to = to;
        self.to_anim = Timed::resuming_at(now, incoming_from, ms, Curve::EasyEase);
    }

    /// Hover weight for `id` in `[0, 1]`.
    pub fn weight(&self, id: Id, now: Instant) -> f32 {
        if self.to == Some(id) {
            return self.to_anim.progress(now);
        }
        if let Some((from_id, held)) = self.from
            && from_id == id
        {
            return held * (1.0 - self.from_anim.progress(now));
        }
        0.0
    }

    /// Whether another frame is needed.
    pub fn is_active(&self, now: Instant) -> bool {
        (self.to.is_some() && !self.to_anim.is_done(now))
            || (self.from.is_some() && !self.from_anim.is_done(now))
    }

    /// The item currently being hovered, as far as the transition knows.
    pub fn target(&self) -> Option<Id> {
        self.to
    }
}
```

The `let Some(..) = .. && ..` let-chain is the idiom this crate already uses (see `AnimationManager::record_pane_removed` in `animations/mod.rs`); it needs no `if let` nesting on this edition.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu animations::hover`
Expected: PASS, 8 tests.

- [ ] **Step 5: Check the gates**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`
Expected: clean. If clippy reports the type or its methods as never used, add `#[allow(dead_code)]` to the `impl` block with the comment `// Consumed by the two overlay models in Tasks 3 and 4.` and remove it in Task 3.

- [ ] **Step 6: Commit**

```bash
git add nexterm-client-gpu/src/animations/hover.rs nexterm-client-gpu/src/animations/mod.rs
git commit -m "feat(client): add HoverTransition, the shared hover cross-fade"
```

---

### Task 2: `lerp_rgba`

**Files:**
- Modify: `color_util.rs` (add the function beside `with_alpha`, and its tests to that file's existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: `pub(crate) fn lerp_rgba(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4]`.

Task 4 lerps the context-menu fill, accent line and text; P3b2b lerps the tab and window-button fills. Three call sites in this plan, two more in the next — a helper, not a premature abstraction.

- [ ] **Step 1: Write the failing tests**

Add to `color_util.rs`'s test module:

```rust
    #[test]
    fn lerp_rgba_hits_both_endpoints_exactly() {
        let a = [0.1, 0.2, 0.3, 1.0];
        let b = [0.9, 0.8, 0.7, 0.5];
        assert_eq!(lerp_rgba(a, b, 0.0), a);
        assert_eq!(lerp_rgba(a, b, 1.0), b);
    }

    #[test]
    fn lerp_rgba_is_linear_at_the_midpoint() {
        let m = lerp_rgba([0.0, 0.0, 0.0, 0.0], [1.0, 0.5, 0.25, 1.0], 0.5);
        assert!((m[0] - 0.5).abs() < 1e-6);
        assert!((m[1] - 0.25).abs() < 1e-6);
        assert!((m[2] - 0.125).abs() < 1e-6);
        assert!((m[3] - 0.5).abs() < 1e-6);
    }

    /// A weight arrives from an eased curve and is already clamped, but a
    /// colour helper that trusts its caller is a colour helper that produces
    /// out-of-range channels the first time someone doesn't.
    #[test]
    fn lerp_rgba_clamps_t() {
        let a = [0.0, 0.0, 0.0, 0.0];
        let b = [1.0, 1.0, 1.0, 1.0];
        assert_eq!(lerp_rgba(a, b, -0.5), a);
        assert_eq!(lerp_rgba(a, b, 1.5), b);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu color_util::tests::lerp_rgba`
Expected: FAIL — `cannot find function lerp_rgba`.

- [ ] **Step 3: Write the implementation**

```rust
/// Linearly interpolate two RGBA colours, `t` clamped to `[0, 1]`.
///
/// Used by the hover cross-fade (UI/UX v3 P3b2), where the hovered
/// appearance is a *different colour* rather than an extra layer — a
/// brightened tab background, an accent-tinted menu row — so alpha scaling
/// cannot express the transition and the colour itself has to move.
///
/// Interpolation is in whatever space the tokens already are (linear-ish
/// sRGB floats, as everywhere else in this renderer); no gamma correction is
/// applied, matching how the palette's existing blends behave.
pub(crate) fn lerp_rgba(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu color_util`
Expected: PASS, including the three new tests and every pre-existing one.

- [ ] **Step 5: Check the gates and commit**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`
If `lerp_rgba` is reported unused, add `#[allow(dead_code)] // First consumer lands in Task 4.` and remove it in Task 4.

```bash
git add nexterm-client-gpu/src/color_util.rs
git commit -m "feat(client): add lerp_rgba for hover colour cross-fades"
```

---

### Task 3: The settings widget row

The widest task, but almost all of it is mechanical. `draw_row_background` receives only a `WidgetSpec` and a `WidgetTheme`, so the weight travels via `WidgetTheme` — two new fields — rather than by threading the transition into the nine per-tab widget builders.

**Files:**
- Modify: `settings/mod.rs` (add `hover_transition`, and its `Default` entry)
- Modify: `renderer/event_handler/mouse.rs:379-400` (retarget beside the existing `hover_widget` write)
- Modify: `renderer/overlay/widgets/draw/mod.rs:37-50` (`WidgetTheme` fields), `:149-172` (`draw_row_background`), and the `test_support` fixture that builds a `WidgetTheme`
- Modify: `renderer/overlay/settings/mod.rs` (pass `now` to each tab renderer)
- Modify: all ten `renderer/overlay/settings/*_tab.rs` (one `now` parameter each; two fields at each `WidgetTheme {` — note `theme_tab.rs` builds one twice, at `:50` and `:165`)
- Modify: `state/mod.rs` (`has_active_animation` clause, plus one test)

**Interfaces:**
- Consumes: `HoverTransition` (Task 1).
- Produces: `SettingsPanel.hover_transition: HoverTransition<WidgetId>` (public field); `WidgetTheme` gains `pub hover: &'a HoverTransition<WidgetId>` and `pub now: std::time::Instant`; every `draw_*_tab` function gains a trailing-or-adjacent `now: std::time::Instant` parameter (place it beside the other scalars, before `font`, so the call sites stay readable).

`WidgetId` is `pub(crate)` in `renderer/overlay/widgets/spec.rs:70` and derives `Copy, PartialEq, Eq, Hash`, so it satisfies `HoverTransition`'s bounds. `SettingsPanel` lives in `settings/mod.rs` and may name it as `crate::renderer::overlay::widgets::WidgetId` — check the existing re-export path before inventing one; if `WidgetId` is not reachable from `settings/mod.rs`, **stop and report it** rather than making the widgets module more public.

- [ ] **Step 1: Write the failing test**

Add to the test module in `state/mod.rs`:

```rust
    /// Hovering a settings row must ask for frames until the cross-fade
    /// finishes, and stop afterwards.
    #[test]
    fn a_hovered_settings_row_wants_animation_frames_until_it_settles() {
        use crate::renderer::overlay::widgets::WidgetId;

        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        let id = WidgetId::new(2, 0);

        state
            .settings_panel
            .hover_transition
            .retarget(Some(id), t0, &anim);
        assert!(state.has_active_animation(t0, 200));
        assert!(
            state.settings_panel.hover_transition.weight(id, t0).abs() < 1e-3,
            "the fade starts from nothing"
        );

        let done = t0 + Duration::from_millis(100);
        assert!(
            (state.settings_panel.hover_transition.weight(id, done) - 1.0).abs() < 1e-3
        );
        assert!(!state.has_active_animation(done, 200));
    }
```

Adjust the `WidgetId` import path to whatever the crate actually exposes.

Also add, to the existing test module in `renderer/overlay/widgets/draw/mod.rs` (which already has a `test_support` fixture and specs built via `spec_at`):

```rust
    /// Focus outranks hover: a focused row paints an opaque `surface_2` and
    /// no hover fill, whatever the hover weight says. This is pre-P3b2
    /// behaviour and the cross-fade must not disturb it.
    #[test]
    fn a_focused_row_ignores_the_hover_weight() {
        let mut hover: crate::animations::HoverTransition<WidgetId> = Default::default();
        let now = Instant::now();
        let spec = spec_at(WidgetKind::Toggle { on: false });
        hover.retarget(Some(spec.id()), now, &nexterm_config::AnimationsConfig::default());
        let settled = now + Duration::from_millis(100);
        assert!((hover.weight(spec.id(), settled) - 1.0).abs() < 1e-3);

        // Build the same spec focused, draw it, and assert the fill is the
        // focus colour rather than a hover-tinted `surface_3`.
        // (Follow whatever assertion style the neighbouring draw tests use —
        // they inspect the appended `bg_verts` colours.)
    }
```

**The commented tail is deliberate:** the assertion must match how the neighbouring draw tests inspect appended vertices, and that style is not visible from this plan. Read one of them (`draw/mod.rs`'s existing tests around `spec_at`) and finish the test in their idiom. If those tests do not inspect colours at all — if they only assert vertex counts — then say so in your report and assert the count difference instead, rather than inventing a new test harness.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu hovered_settings_row_wants_animation_frames`
Expected: FAIL — `no field hover_transition`.

- [ ] **Step 3: Add the state and retarget it**

In `settings/mod.rs`, beside `hover_widget`:

```rust
    /// Hover cross-fade for this panel's widget rows (UI/UX v3 P3b2).
    ///
    /// `hover_widget` above stays the truth for the tooltip dwell timer and
    /// goes `None` the moment the pointer leaves the panel; this outlives it
    /// by exactly one fade so the row the pointer left can still dim out.
    pub hover_transition: crate::animations::HoverTransition<WidgetId>,
```

plus `hover_transition: Default::default(),` in the `Default` body.

In `renderer/event_handler/mouse.rs`, right after the existing `sp.hover_widget = hovered.map(...)` assignment, retarget from the same `hovered` local — note it is an `Option<(u8, u16)>`, so build the id from it:

```rust
        // UI/UX v3 P3b2: the same pointer move drives the cross-fade. This is
        // idempotent while the hovered row is unchanged, so a slow drag
        // across one row does not restart the fade.
        let now = std::time::Instant::now();
        let anim = &self.app.config.animations;
        let sp = &mut self.app.state.settings_panel;
        sp.hover_transition.retarget(
            hovered.map(|(category, index)| WidgetId::new(category, index)),
            now,
            anim,
        );
```

Reuse the `sp` binding that already exists there if the borrow checker allows; the snippet re-binds only to be self-contained. If `&self.app.config.animations` and `&mut self.app.state.settings_panel` conflict, take `let anim = self.app.config.animations.clone();` — `AnimationsConfig` derives `Clone` — rather than restructuring the handler.

- [ ] **Step 4: Thread the weight to the draw site**

`renderer/overlay/widgets/draw/mod.rs` — add to `WidgetTheme`:

```rust
    /// Hover cross-fade for the panel's rows (UI/UX v3 P3b2).
    pub hover: &'a crate::animations::HoverTransition<super::WidgetId>,
    /// Frame time, for the hover weight.
    pub now: std::time::Instant,
```

and rewrite `draw_row_background`:

```rust
/// Hover / focus fill behind the whole row.
fn draw_row_background(spec: &WidgetSpec, theme: &WidgetTheme<'_>, sink: &mut WidgetSink<'_>) {
    // UI/UX v3 P3b2: the hover fill now fades in and out. Focus still wins
    // outright — it paints an opaque `surface_2`, so a hover fade underneath
    // it would be invisible, and one over it would change a shipped
    // appearance for reasons unrelated to motion.
    let fill = if spec.focused() {
        Some(theme.tokens.surface_2)
    } else if spec.enabled() && spec.kind().is_interactive() {
        let w = theme.hover.weight(spec.id(), theme.now);
        (w > 0.0).then(|| {
            let s = theme.tokens.surface_3;
            [s[0], s[1], s[2], s[3] * HOVER_ALPHA * w]
        })
    } else {
        None
    };
    if let Some(color) = fill {
        add_px_rounded_rect_sdf(
            spec.rect.x,
            spec.rect.y,
            spec.rect.w,
            spec.rect.h,
            theme.metrics.radius.control,
            color,
            theme.sw,
            theme.sh,
            sink.bg_verts,
            sink.bg_idx,
        );
    }
}
```

`spec.hovered` is deliberately no longer read here — the weight subsumes it, and the field stays for the hit-test and semantic answer. If clippy now reports `hovered` as never read, **do not delete the field**: report it, and the controller will rule.

Then, mechanically: each of the ten `renderer/overlay/settings/*_tab.rs` render functions gains a `now: std::time::Instant` parameter and passes `hover: &sp.hover_transition, now,` in its `WidgetTheme` literal (`theme_tab.rs` has two literals). `renderer/overlay/settings/mod.rs` passes its own `now` down — it already has one, since P3a gave `build_settings_panel_verts` a `now` and P3b1 uses it for the tooltip. Update the `test_support` `WidgetTheme` fixture in `draw/mod.rs` with a `HoverTransition::default()` and an `Instant::now()`.

- [ ] **Step 5: Add the aggregate clause**

`state/mod.rs`, in `has_active_animation`:

```rust
        if self.settings_panel.hover_transition.is_active(now) {
            return true;
        }
```

- [ ] **Step 6: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS, including the new test and every pre-existing widget-draw test.

- [ ] **Step 7: Check the gates and commit**

Run: `cargo clippy -p nexterm-client-gpu --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): cross-fade the settings panel's hover fill"
```

---

### Task 4: The context menu

Three properties move together here — the item fill, the 3 px left accent line, and the label colour — which is why this model cannot be expressed as a fade of an added layer.

**Files:**
- Modify: `state/menus.rs` (`ContextMenu.hover_transition`, and every constructor — `new_default` and any sibling)
- Modify: `renderer/event_handler/mouse.rs:570-582` (retarget beside `menu.hovered = new_hovered`)
- Modify: `renderer/event_handler/accessibility.rs:265` (retarget beside `menu.hovered = Some(idx)`)
- Modify: `renderer/overlay/dialog.rs:267-299` (lerp the three properties) and its caller in `renderer/render_frame.rs` if `now` is not already a parameter of `build_context_menu_verts`
- Modify: `state/mod.rs` (`has_active_animation` clause, plus one test)

**Interfaces:**
- Consumes: `HoverTransition` (Task 1), `lerp_rgba` (Task 2).
- Produces: `ContextMenu.hover_transition: HoverTransition<usize>` (public field). `build_context_menu_verts` gains `now: std::time::Instant` if it lacks one.

`ContextMenu` derives `Clone` and P3b1 clones it into a render-only exit ghost. `HoverTransition` derives `Clone, Copy`, so the ghost carries a running hover fade and it keeps running while the menu leaves. **That is intended** — the item the pointer left should not snap back at the moment the menu starts fading — and the doc comment on the field must say so, or a future reader will read it as a leak.

- [ ] **Step 1: Write the failing test**

Add to the test module in `state/mod.rs`:

```rust
    /// The menu's hover cross-fade is independent of the widget layer's:
    /// moving the pointer from a settings row into a context menu runs both
    /// at once, which is why each model owns its own transition.
    #[test]
    fn a_hovered_context_menu_item_wants_animation_frames_until_it_settles() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.context_menu = Some(ContextMenu::new_default(10.0, 10.0, &[]));

        let menu = state
            .context_menu
            .as_mut()
            .expect("the menu was just assigned");
        menu.hovered = Some(1);
        menu.hover_transition.retarget(Some(1), t0, &anim);
        assert!(state.has_active_animation(t0, 200));

        let done = t0 + Duration::from_millis(100);
        let menu = state.context_menu.as_ref().expect("still open");
        assert!((menu.hover_transition.weight(1, done) - 1.0).abs() < 1e-3);
        assert!(menu.hover_transition.weight(0, done).abs() < 1e-4);
        assert!(!state.has_active_animation(done, 200));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu hovered_context_menu_item_wants_animation_frames`
Expected: FAIL — `no field hover_transition`.

- [ ] **Step 3: Add the state and retarget it**

`state/menus.rs`, in `ContextMenu`:

```rust
    /// Hover cross-fade over `items` (UI/UX v3 P3b2).
    ///
    /// `hovered` above stays the truth for click dispatch and the AccessKit
    /// focus node. This is render-only, and it is deliberately carried into
    /// the exit ghost P3b1 clones on dismiss: the item the pointer left
    /// should keep fading rather than snap back as the menu leaves.
    pub hover_transition: crate::animations::HoverTransition<usize>,
```

plus `hover_transition: Default::default(),` in every constructor.

At both write sites of `menu.hovered`, retarget from the same value:

```rust
                // UI/UX v3 P3b2: same value, same frame.
                menu.hover_transition.retarget(
                    new_hovered,
                    std::time::Instant::now(),
                    &self.app.config.animations,
                );
```

In `event_handler/accessibility.rs:265` the value is `Some(idx)` rather than a local; pass that. Mirror the surrounding code's idiom for reaching the config; if a borrow conflict appears, clone `AnimationsConfig` rather than restructuring.

- [ ] **Step 4: Lerp the three properties**

`renderer/overlay/dialog.rs` — replace the `if menu.hovered == Some(i)` block and the text-colour choice with weight-driven lerps:

```rust
            // UI/UX v3 P3b2: hover cross-fades rather than snapping. Three
            // properties move together — the row fill, the accent line and
            // the label — so the weight lerps each of them instead of an
            // extra layer being faded in.
            let w = menu.hover_transition.weight(i, now);
            if w > 0.0 {
                let hab = tokens.tab_active_bg;
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    menu_w - 4.0,
                    cell_h - 2.0,
                    [hab[0], hab[1], hab[2], 0.90 * w],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // Left accent line on hover (3px)
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    3.0,
                    cell_h - 2.0,
                    [ap[0], ap[1], ap[2], 0.90 * w],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
```

and, at the label:

```rust
            let text_color = crate::color_util::lerp_rgba(
                tokens.text_secondary,
                tokens.text_primary,
                menu.hover_transition.weight(i, now),
            );
```

The fill and the accent are additive layers, so scaling their alpha by `w` is the correct interpolation for them; the label is a colour swap between two opaque tokens, so it needs `lerp_rgba`. Keep the two mechanisms distinct rather than forcing one — using `lerp_rgba` on the fill would interpolate *from* the panel colour it happens to sit on, which is not what is behind it.

If `build_context_menu_verts` has no `now` parameter, add one and pass `frame_now` from `render_frame.rs` — that binding already exists at `render_frame.rs:197` and is passed to nine other builders.

- [ ] **Step 5: Add the aggregate clause**

`state/mod.rs`, in `has_active_animation` — note it must cover the ghost too, since the ghost's fade keeps running:

```rust
        if self
            .context_menu
            .as_ref()
            .is_some_and(|m| m.hover_transition.is_active(now))
            || self
                .context_menu_closing
                .as_ref()
                .is_some_and(|(m, _)| m.hover_transition.is_active(now))
        {
            return true;
        }
```

- [ ] **Step 6: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS. Confirm P3b1's `a_fully_idle_state_wants_no_animation_frames` is still green — it now has two more ways to break.

- [ ] **Step 7: Check the gates and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): cross-fade the context menu's hover fill, accent and label"
```

---

## Closing out P3b2a

- [ ] Update `docs/plans/ui-ux-modernization-v3.md`: note that P3b2a shipped, and add to the on-device verification backlog the items CI cannot judge — whether 100 ms reads as responsive or as lag on a hover, and whether the context menu's three simultaneous interpolations read as one coherent motion or as three things moving.
- [ ] Extend the "Adding an overlay surface" note in `nexterm-client-gpu/CLAUDE.md` with the hover rule: a new hover model owns its own `HoverTransition`, retargets from the same site that writes the logical hover state, and adds an `is_active` clause to `has_active_animation`. Mention that `apply_surface_fade` is for surfaces and does not apply to hover.
- [ ] Open the PR against `master` with an English title and body. Suggested title: `feat(client): hover cross-fade for the settings rows and the context menu (UI/UX v3 P3b2a)`.
- [ ] State plainly in the PR body what was not verified: no motion in this phase has been seen on hardware, and the repo's screenshot convention cannot capture a transition.
- [ ] **This branch is stacked on P3b1 (PR #79).** Confirm #79 has merged before opening the P3b2a PR against `master`, or target the PR at #79's branch and say so.
