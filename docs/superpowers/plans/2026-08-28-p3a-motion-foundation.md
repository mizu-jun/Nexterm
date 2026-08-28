# P3a Motion Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the GPU client a time-based, interruptible motion foundation (Fluent curves + durations + a `Timed` value type), make animations drive their own redraws, and prove it by replacing the settings panel's frame-count animation hack with a real open/close transition.

**Architecture:** `animations.rs` becomes an `animations/` module directory holding the existing spring/easing code plus two new pure units — `Curve` (nine Fluent cubic-bezier tokens with a Newton-Raphson/bisection solver) and `Timed` (start + duration + curve). A single aggregate `ClientState::has_active_animation` is queried once per event-loop tick; when it is true, and only then, the loop requests a redraw. The settings panel keeps `is_open` as the sole truth for input and accessibility, and gains a render-only `closing: Option<Timed>` ghost so it can fade out after it is logically closed.

**Tech Stack:** Rust 2024 (workspace edition), wgpu, winit 0.30 `ApplicationHandler`, `tracing`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-28-p3a-motion-foundation-design.md`

## Global Constraints

- Branch: `p3a-motion-foundation` (already created, spec already committed as `aaf4a3b`).
- **No new dependency.** `Cargo.lock` must not change; if it does, run `bash scripts/regenerate-flatpak-sources.sh` and commit `pkg/flatpak/cargo-sources.json`.
- **No new config key and no new user-facing string.** P3a adds neither, so `docs/CONFIGURATION.md` and `nexterm-i18n/locales/*.json` are untouched.
- **English only** for code comments, doc-comments, commit messages and the PR description. Japanese stays in the CLI conversation.
- **No `unwrap()`.** Use `?` or `expect("concrete reason")`.
- `cargo clippy -- -D warnings` and `cargo fmt --check` must pass before the PR.
- Formatting: never run bare `rustfmt <file>`. Use `cargo fmt`, or `rustfmt --edition 2024 <file>`. Bare `rustfmt` defaults to style edition 2015 and reorders imports the opposite way from CI.
- Curve control points and duration values are transcriptions of `microsoft/fluentui` `packages/tokens/src/global/{curves,durations}.ts`. Copy them exactly; do not round or re-derive.
- Every task ends with `cargo test -p nexterm-client-gpu` green before its commit.

---

## File Structure

| File | Responsibility |
|---|---|
| `nexterm-client-gpu/src/animations/mod.rs` (create) | `AnimationManager`, `SpringState`, `MAX_DIM_ALPHA`; declares and re-exports the submodules |
| `nexterm-client-gpu/src/animations/easing.rs` (create) | `ease_out_cubic`, `linear`, `compute_progress` — moved verbatim |
| `nexterm-client-gpu/src/animations/curve.rs` (create) | `Curve`, the cubic-bezier solver, the `duration` constants |
| `nexterm-client-gpu/src/animations/timed.rs` (create) | `Timed` |
| `nexterm-client-gpu/src/animations.rs` (delete) | Replaced by the directory above |
| `nexterm-client-gpu/src/renderer/render_frame.rs` (modify) | Pane-cache-miss counter; panel drawn while closing |
| `nexterm-client-gpu/src/renderer/event_handler/mod.rs` (modify) | New `last_cache_miss_report` field |
| `nexterm-client-gpu/src/renderer/app.rs` (modify) | Initialise that field |
| `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs` (modify) | Counter reporting; animation-driven redraw; delete the frame-count pump |
| `nexterm-client-gpu/src/state/mod.rs` (modify) | `ClientState::has_active_animation` aggregate |
| `nexterm-client-gpu/src/settings/mod.rs` (modify) | `open_anim` / `closing` fields; `open()` / `close()` / `is_visible()` |
| `nexterm-client-gpu/src/settings/drag.rs` (modify) | `eased_progress(now)` reads the `Timed` |
| `nexterm-client-gpu/src/renderer/overlay/settings/mod.rs` (modify) | Draw while closing; take `now` |
| `nexterm-client-gpu/src/renderer/event_handler/settings_panel_hit.rs` (modify) | Pass `now` to `eased_progress` |
| `nexterm-client-gpu/src/renderer/input_handler/action.rs`, `input_handler/mod.rs`, `event_handler/mouse.rs` (modify) | Updated `open()` / `close()` call sites |
| `nexterm-client-gpu/CLAUDE.md`, `docs/plans/ui-ux-modernization-v3.md` (modify) | Documentation |

---

### Task 1: Split `animations.rs` into an `animations/` directory

Pure code movement, no behaviour change. It lands first and alone so the later diffs are readable.

**Files:**
- Create: `nexterm-client-gpu/src/animations/mod.rs`
- Create: `nexterm-client-gpu/src/animations/easing.rs`
- Delete: `nexterm-client-gpu/src/animations.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: the module path `crate::animations` continues to export `AnimationManager`, `SpringState`, `MAX_DIM_ALPHA`, `ease_out_cubic`, `linear`, `compute_progress` exactly as before. No caller outside the module changes.

- [ ] **Step 1: Record the current test count**

Run: `cargo test -p nexterm-client-gpu animations 2>&1 | tail -5`

Write down the number of tests that ran. It must be identical at the end of this task — this is the whole safety net for a move.

- [ ] **Step 2: Create `animations/easing.rs` with the easing code moved verbatim**

Move these items out of `animations.rs` unchanged: the `ease_out_cubic`, `linear` and `compute_progress` functions, and the tests named `ease_out_cubic_is_0_and_1_at_endpoints`, `ease_out_cubic_is_monotonically_increasing`, `ease_out_cubic_exceeds_linear_near_middle`, `ease_out_cubic_clamps_out_of_range_inputs`, `linear_is_identity`, `compute_progress_with_duration_0_is_always_1`, `compute_progress_with_zero_elapsed_is_0`, `compute_progress_at_half_duration_is_0_5`, `compute_progress_beyond_duration_clamps_to_1`.

The file header:

```rust
//! Easing helpers and time-based progress (Sprint 5-7 / Phase 3-2).
//!
//! Split out of `animations.rs` in UI/UX v3 P3a so the module stays within
//! the file-size guidance while `Curve` and `Timed` join it.

use std::time::Instant;
```

The moved test module needs `use super::*;` and `use std::time::Duration;`.

Do not change a single line of the moved bodies. If a body looks improvable, leave it — this task is a move.

- [ ] **Step 3: Create `animations/mod.rs` with the remainder**

`animations/mod.rs` keeps the original file's `//!` header (amended to mention the split), `MAX_DIM_ALPHA`, `SpringState`, `TabSwitchState`, `AnimationManager`, and every test not moved in Step 2. Add near the top:

```rust
mod easing;

pub use easing::{compute_progress, ease_out_cubic, linear};
```

Keep `use std::collections::HashMap;` and `use std::time::{Duration, Instant};`. Keep `#[allow(dead_code)]` on `linear`, `cleanup_expired`, `current_tab_switch_target`, `tab_switch_progress` and `has_active_animation` exactly as they are today — Task 5 removes the one on `has_active_animation` and no other.

- [ ] **Step 4: Delete the old file**

```bash
git rm nexterm-client-gpu/src/animations.rs
```

`mod animations;` at `nexterm-client-gpu/src/main.rs:13` is unchanged — Rust resolves it to the directory.

- [ ] **Step 5: Verify nothing else changed**

```bash
cargo test -p nexterm-client-gpu animations 2>&1 | tail -5
cargo clippy -p nexterm-client-gpu -- -D warnings
cargo fmt --check
```

Expected: the same test count as Step 1, all passing; clippy and fmt clean. If clippy now reports an unused import in either file, remove only that import.

- [ ] **Step 6: Commit**

```bash
git add -A nexterm-client-gpu/src/animations nexterm-client-gpu/src/animations.rs
git commit -m "refactor(client): split animations.rs into an animations/ module

Pure code movement ahead of UI/UX v3 P3a, which adds Curve and Timed.
No behaviour change; the same tests run and pass."
```

---

### Task 2: `Curve` — the Fluent cubic-bezier table and solver

**Files:**
- Create: `nexterm-client-gpu/src/animations/curve.rs`
- Modify: `nexterm-client-gpu/src/animations/mod.rs` (declare and re-export)

**Interfaces:**
- Consumes: nothing from Task 1 beyond the module existing.
- Produces:
  - `pub enum Curve { Linear, AccelerateMax, AccelerateMid, AccelerateMin, DecelerateMax, DecelerateMid, DecelerateMin, EasyEaseMax, EasyEase }`, deriving `Debug, Clone, Copy, PartialEq, Eq`.
  - `pub const fn Curve::control_points(self) -> (f32, f32, f32, f32)`
  - `pub fn Curve::eval(self, t: f32) -> f32`
  - `pub fn Curve::invert(self, value: f32) -> f32`
  - `pub mod duration` with `ULTRA_FAST: u32 = 50`, `FASTER = 100`, `FAST = 150`, `NORMAL = 200`, `GENTLE = 250`, `SLOW = 300`, `SLOWER = 400`, `ULTRA_SLOW = 500`.
  - Re-exported from `crate::animations` as `Curve` and `duration`.

- [ ] **Step 1: Write the failing tests**

Create `nexterm-client-gpu/src/animations/curve.rs` containing **only** the test module below, so the task starts red:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Every curve this project uses.
    const ALL: [Curve; 9] = [
        Curve::Linear,
        Curve::AccelerateMax,
        Curve::AccelerateMid,
        Curve::AccelerateMin,
        Curve::DecelerateMax,
        Curve::DecelerateMid,
        Curve::DecelerateMin,
        Curve::EasyEaseMax,
        Curve::EasyEase,
    ];

    #[test]
    fn every_curve_starts_at_0_and_ends_at_1() {
        for c in ALL {
            assert!(c.eval(0.0).abs() < 1e-3, "{c:?} at 0");
            assert!((c.eval(1.0) - 1.0).abs() < 1e-3, "{c:?} at 1");
        }
    }

    #[test]
    fn every_curve_is_monotonically_increasing() {
        for c in ALL {
            let mut prev = -1.0;
            for i in 0..=100 {
                let v = c.eval(i as f32 / 100.0);
                assert!(v >= prev - 1e-4, "{c:?} dipped at t={}", i as f32 / 100.0);
                prev = v;
            }
        }
    }

    #[test]
    fn linear_is_the_identity() {
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            assert!((Curve::Linear.eval(t) - t).abs() < 1e-6);
        }
    }

    #[test]
    fn out_of_range_inputs_clamp() {
        for c in ALL {
            assert!(c.eval(-1.0).abs() < 1e-3, "{c:?} below 0");
            assert!((c.eval(2.0) - 1.0).abs() < 1e-3, "{c:?} above 1");
        }
    }

    /// `EasyEaseMax` (0.8, 0, 0.2, 1) and `EasyEase` (0.33, 0, 0.67, 1) are
    /// both point-symmetric about (0.5, 0.5) — x2 = 1-x1 and y2 = 1-y1 — so
    /// their midpoint is exactly 0.5. This is the one closed-form value the
    /// solver can be checked against without a reference implementation.
    #[test]
    fn symmetric_curves_pass_through_their_midpoint() {
        assert!((Curve::EasyEaseMax.eval(0.5) - 0.5).abs() < 1e-3);
        assert!((Curve::EasyEase.eval(0.5) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn accelerate_lags_linear_and_decelerate_leads_it() {
        assert!(Curve::AccelerateMax.eval(0.5) < 0.5);
        assert!(Curve::DecelerateMax.eval(0.5) > 0.5);
    }

    /// `AccelerateMid` (1, 0, 1, 1) has a zero X-derivative at t=1 and
    /// `DecelerateMid` (0, 0, 0, 1) has one at t=0. Newton-Raphson stalls
    /// there; these two exist to exercise the bisection fallback directly.
    #[test]
    fn degenerate_curves_still_solve() {
        for c in [Curve::AccelerateMid, Curve::DecelerateMid] {
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                let v = c.eval(t);
                assert!(v.is_finite(), "{c:?} not finite at {t}");
                assert!((0.0..=1.0).contains(&v), "{c:?} out of range at {t}: {v}");
            }
        }
    }

    #[test]
    fn invert_round_trips_through_eval() {
        for c in ALL {
            for i in 0..=10 {
                let v = i as f32 / 10.0;
                let t = c.invert(v);
                assert!(
                    (c.eval(t) - v).abs() < 1e-2,
                    "{c:?}: invert({v}) = {t}, eval back = {}",
                    c.eval(t)
                );
            }
        }
    }

    #[test]
    fn durations_match_the_fluent_table() {
        assert_eq!(duration::ULTRA_FAST, 50);
        assert_eq!(duration::FASTER, 100);
        assert_eq!(duration::FAST, 150);
        assert_eq!(duration::NORMAL, 200);
        assert_eq!(duration::GENTLE, 250);
        assert_eq!(duration::SLOW, 300);
        assert_eq!(duration::SLOWER, 400);
        assert_eq!(duration::ULTRA_SLOW, 500);
    }
}
```

Add `mod curve;` and `pub use curve::{Curve, duration};` to `animations/mod.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu animations::curve 2>&1 | tail -20`

Expected: compilation failure — `cannot find type Curve in this scope`, `failed to resolve: use of undeclared crate or module duration`.

- [ ] **Step 3: Write the implementation**

Prepend to `nexterm-client-gpu/src/animations/curve.rs`, above the test module:

```rust
//! Fluent 2 motion curves and durations (UI/UX v3 P3a).
//!
//! Values are transcribed from the Fluent UI implementation repository —
//! `microsoft/fluentui`, `packages/tokens/src/global/curves.ts` and
//! `durations.ts`. The Fluent 2 design site documents motion qualitatively
//! and publishes no token values, so the implementation repo is the source
//! of truth here. Do not re-derive these by eye.
//!
//! All nine curves are defined even though P3a uses two: a partial copy of
//! an external table invites a later change to guess at a missing constant.
//! They are `const fn` data with no runtime cost.

/// Animation durations from the Fluent 2 token set, in milliseconds.
///
/// `dead_code` is allowed for the module as a whole: this is a verbatim
/// transcription of an external table, and the steps P3a does not consume
/// yet are consumed by P3b. Silencing them individually as they are picked
/// up would churn this file for no gain.
#[allow(dead_code)]
pub mod duration {
    /// Checkbox tick, toggle snap.
    pub const ULTRA_FAST: u32 = 50;
    /// Button press feedback.
    pub const FASTER: u32 = 100;
    /// Small control state changes.
    pub const FAST: u32 = 150;
    /// Panel slide, card expand.
    pub const NORMAL: u32 = 200;
    /// Slightly softer than `NORMAL`.
    pub const GENTLE: u32 = 250;
    /// Dialog entrance, page transition.
    pub const SLOW: u32 = 300;
    /// Large-surface movement.
    pub const SLOWER: u32 = 400;
    /// Full-screen morph.
    pub const ULTRA_SLOW: u32 = 500;
}

/// A Fluent 2 easing curve, expressed as a CSS-style cubic bezier with
/// `P0 = (0, 0)` and `P3 = (1, 1)`.
///
/// Accelerate curves start slow and leave quickly (use them for exits);
/// decelerate curves arrive quickly and settle (use them for entrances).
///
/// `dead_code` is allowed for the same reason as `duration` above: the
/// table is transcribed whole, and the variants P3a does not construct are
/// P3b's to construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Curve {
    /// No easing. Progress bars and spinners.
    Linear,
    AccelerateMax,
    AccelerateMid,
    AccelerateMin,
    DecelerateMax,
    DecelerateMid,
    DecelerateMin,
    EasyEaseMax,
    EasyEase,
}

impl Curve {
    /// The `(x1, y1, x2, y2)` control points, matching the CSS
    /// `cubic-bezier()` argument order.
    pub const fn control_points(self) -> (f32, f32, f32, f32) {
        match self {
            Curve::Linear => (0.0, 0.0, 1.0, 1.0),
            Curve::AccelerateMax => (0.9, 0.1, 1.0, 0.2),
            Curve::AccelerateMid => (1.0, 0.0, 1.0, 1.0),
            Curve::AccelerateMin => (0.8, 0.0, 0.78, 1.0),
            Curve::DecelerateMax => (0.1, 0.9, 0.2, 1.0),
            Curve::DecelerateMid => (0.0, 0.0, 0.0, 1.0),
            Curve::DecelerateMin => (0.33, 0.0, 0.1, 1.0),
            Curve::EasyEaseMax => (0.8, 0.0, 0.2, 1.0),
            Curve::EasyEase => (0.33, 0.0, 0.67, 1.0),
        }
    }

    /// Map elapsed-time fraction `t` to eased progress, both in `[0, 1]`.
    pub fn eval(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if self == Curve::Linear {
            return t;
        }
        let (x1, y1, x2, y2) = self.control_points();
        let s = solve_for_x(x1, x2, t);
        axis(y1, y2, s).clamp(0.0, 1.0)
    }

    /// The `t` whose eased value is `value` — the inverse of [`Curve::eval`].
    ///
    /// Used to resume an interrupted animation from the value already on
    /// screen. `eval` is monotone for every curve in this table, so a plain
    /// bisection is exact enough and cannot diverge.
    pub fn invert(self, value: f32) -> f32 {
        let value = value.clamp(0.0, 1.0);
        if self == Curve::Linear {
            return value;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if self.eval(mid) < value {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }
}

/// One axis of a cubic bezier with the endpoints pinned to 0 and 1.
fn axis(p1: f32, p2: f32, s: f32) -> f32 {
    let u = 1.0 - s;
    3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s
}

/// Derivative of [`axis`] with respect to `s`.
fn axis_derivative(p1: f32, p2: f32, s: f32) -> f32 {
    let u = 1.0 - s;
    3.0 * u * u * p1 + 6.0 * u * s * (p2 - p1) + 3.0 * s * s * (1.0 - p2)
}

/// Find the curve parameter `s` with `X(s) = t`.
///
/// Newton-Raphson seeded at `s = t` converges in a couple of iterations for
/// the well-conditioned curves. `AccelerateMid` and `DecelerateMid` have a
/// zero X-derivative at an endpoint, where Newton stalls or steps outside
/// `[0, 1]`; bisection then finishes the job. X is monotone in `s` for every
/// curve in the table (all control points lie in `[0, 1]`), so bisection
/// always converges.
fn solve_for_x(x1: f32, x2: f32, t: f32) -> f32 {
    const EPSILON: f32 = 1e-6;

    let mut s = t;
    for _ in 0..8 {
        let err = axis(x1, x2, s) - t;
        if err.abs() < EPSILON {
            return s;
        }
        let d = axis_derivative(x1, x2, s);
        if d.abs() < EPSILON {
            break;
        }
        s -= err / d;
        if !(0.0..=1.0).contains(&s) {
            break;
        }
    }

    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if axis(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu animations::curve 2>&1 | tail -20`

Expected: 9 tests pass.

- [ ] **Step 5: Lint and format**

```bash
cargo clippy -p nexterm-client-gpu -- -D warnings
cargo fmt --check
```

The `#[allow(dead_code)]` on `Curve` and on `duration` is deliberate and already in the code above: this crate is a binary, so an unconstructed enum variant or an unread constant is a `dead_code` warning, and `-D warnings` would fail the build over a table that is intentionally transcribed whole. Do not remove either attribute.

- [ ] **Step 6: Commit**

```bash
git add nexterm-client-gpu/src/animations/curve.rs nexterm-client-gpu/src/animations/mod.rs
git commit -m "feat(client): add the Fluent 2 motion curve and duration tables

Nine cubic-bezier curves and eight duration steps, transcribed from
microsoft/fluentui packages/tokens. The solver mirrors CSS cubic-bezier:
Newton-Raphson on X with a bisection fallback for the two curves whose
X-derivative vanishes at an endpoint."
```

---

### Task 3: `Timed`

**Files:**
- Create: `nexterm-client-gpu/src/animations/timed.rs`
- Modify: `nexterm-client-gpu/src/animations/mod.rs` (declare and re-export)

**Interfaces:**
- Consumes: `Curve` and `compute_progress` from Tasks 1–2.
- Produces: `pub struct Timed` (`Debug, Clone, Copy`) with
  - `pub fn new(start: Instant, duration_ms: u32, curve: Curve) -> Self`
  - `pub fn resuming_at(now: Instant, value: f32, duration_ms: u32, curve: Curve) -> Self`
  - `pub fn raw_progress(&self, now: Instant) -> f32`
  - `pub fn progress(&self, now: Instant) -> f32`
  - `pub fn is_done(&self, now: Instant) -> bool`
  - Re-exported from `crate::animations` as `Timed`.

- [ ] **Step 1: Write the failing tests**

Create `nexterm-client-gpu/src/animations/timed.rs` containing **only**:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn progress_runs_from_0_to_1_over_the_duration() {
        let t0 = Instant::now();
        let a = Timed::new(t0, 200, Curve::Linear);
        assert!(a.progress(t0).abs() < 1e-4);
        assert!((a.progress(t0 + Duration::from_millis(100)) - 0.5).abs() < 1e-3);
        assert!((a.progress(t0 + Duration::from_millis(200)) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn progress_stays_at_1_past_the_duration() {
        let t0 = Instant::now();
        let a = Timed::new(t0, 200, Curve::DecelerateMax);
        assert!((a.progress(t0 + Duration::from_secs(60)) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn is_done_flips_at_the_duration() {
        let t0 = Instant::now();
        let a = Timed::new(t0, 200, Curve::DecelerateMax);
        assert!(!a.is_done(t0));
        assert!(!a.is_done(t0 + Duration::from_millis(199)));
        assert!(a.is_done(t0 + Duration::from_millis(200)));
    }

    /// The reduced-motion path: `AnimationsConfig::scaled_duration_ms`
    /// returns 0 when animations are disabled or the intensity is "off", and
    /// a zero-duration `Timed` must be finished before it is ever queried.
    #[test]
    fn a_zero_duration_animation_is_finished_immediately() {
        let t0 = Instant::now();
        let a = Timed::new(t0, 0, Curve::DecelerateMax);
        assert!((a.progress(t0) - 1.0).abs() < 1e-4);
        assert!(a.is_done(t0));
    }

    #[test]
    fn easing_makes_progress_differ_from_raw_progress() {
        let t0 = Instant::now();
        let mid = t0 + Duration::from_millis(100);
        let a = Timed::new(t0, 200, Curve::DecelerateMax);
        assert!((a.raw_progress(mid) - 0.5).abs() < 1e-3);
        assert!(a.progress(mid) > a.raw_progress(mid) + 0.05);
    }

    #[test]
    fn resuming_at_starts_from_the_requested_value() {
        let now = Instant::now();
        for curve in [
            Curve::Linear,
            Curve::DecelerateMax,
            Curve::AccelerateMax,
            Curve::EasyEase,
        ] {
            for i in 0..=10 {
                let v = i as f32 / 10.0;
                let a = Timed::resuming_at(now, v, 200, curve);
                assert!(
                    (a.progress(now) - v).abs() < 2e-2,
                    "{curve:?}: asked for {v}, got {}",
                    a.progress(now)
                );
            }
        }
    }

    #[test]
    fn resuming_at_finishes_within_the_remaining_duration() {
        let now = Instant::now();
        let a = Timed::resuming_at(now, 0.5, 200, Curve::Linear);
        assert!(!a.is_done(now));
        assert!(a.is_done(now + Duration::from_millis(200)));
    }

    #[test]
    fn resuming_a_zero_duration_animation_is_finished_immediately() {
        let now = Instant::now();
        let a = Timed::resuming_at(now, 0.3, 0, Curve::DecelerateMax);
        assert!(a.is_done(now));
        assert!((a.progress(now) - 1.0).abs() < 1e-4);
    }
}
```

Add `mod timed;` and `pub use timed::Timed;` to `animations/mod.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu animations::timed 2>&1 | tail -20`

Expected: compilation failure — `cannot find struct Timed in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `nexterm-client-gpu/src/animations/timed.rs`, above the test module:

```rust
//! Time-based, curve-eased animations (UI/UX v3 P3a).
//!
//! A `Timed` is a value, not a running object: it stores when it started,
//! how long it lasts and which curve shapes it, and answers questions about
//! any instant you hand it. Nothing ticks it. That keeps every consumer
//! testable without a clock and makes an animation cheap to copy around.
//!
//! Springs (`SpringState` in the parent module) stay the tool for motion
//! that must be interruptible mid-flight by a new target, such as the tab
//! accent. `Timed` is for transitions with a known start, end and duration.

use std::time::{Duration, Instant};

use super::curve::Curve;
use super::easing::compute_progress;

/// One time-based animation.
#[derive(Debug, Clone, Copy)]
pub struct Timed {
    start: Instant,
    duration_ms: u32,
    curve: Curve,
}

impl Timed {
    /// Start an animation at `start`.
    ///
    /// Pass a `duration_ms` that already went through
    /// `AnimationsConfig::scaled_duration_ms`, so a user who turned
    /// animations off gets 0 here and the animation is born finished.
    pub fn new(start: Instant, duration_ms: u32, curve: Curve) -> Self {
        Self {
            start,
            duration_ms,
            curve,
        }
    }

    /// Build an animation that already holds `value` at `now`.
    ///
    /// This is how an interruption is expressed: read whatever value is on
    /// screen, then ask for an animation that continues from there instead
    /// of snapping back to 0. Continuity of *value* is guaranteed; the
    /// speed may change abruptly, which is the normal look of a reversed
    /// transition.
    pub fn resuming_at(now: Instant, value: f32, duration_ms: u32, curve: Curve) -> Self {
        if duration_ms == 0 {
            return Self::new(now, 0, curve);
        }
        let elapsed_ms = (duration_ms as f32 * curve.invert(value)).round() as u64;
        let start = now
            .checked_sub(Duration::from_millis(elapsed_ms))
            .unwrap_or(now);
        Self::new(start, duration_ms, curve)
    }

    /// Elapsed fraction in `[0, 1]`, before easing.
    pub fn raw_progress(&self, now: Instant) -> f32 {
        compute_progress(self.start, now, self.duration_ms)
    }

    /// Eased progress in `[0, 1]` — the value a consumer should animate on.
    pub fn progress(&self, now: Instant) -> f32 {
        self.curve.eval(self.raw_progress(now))
    }

    /// Whether the animation has reached its end and needs no more frames.
    pub fn is_done(&self, now: Instant) -> bool {
        self.raw_progress(now) >= 1.0
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu animations::timed 2>&1 | tail -20`

Expected: 8 tests pass.

- [ ] **Step 5: Run the whole crate's tests, lint and format**

```bash
cargo test -p nexterm-client-gpu 2>&1 | tail -10
cargo clippy -p nexterm-client-gpu -- -D warnings
cargo fmt --check
```

- [ ] **Step 6: Commit**

```bash
git add nexterm-client-gpu/src/animations/timed.rs nexterm-client-gpu/src/animations/mod.rs
git commit -m "feat(client): add Timed, a curve-eased time-based animation value

A Timed stores start, duration and curve and answers about any instant,
so consumers stay testable without a clock. A zero duration — what
AnimationsConfig yields when animations are off — is born finished, which
is the whole reduced-motion path. resuming_at expresses an interruption as
\"continue from the value already on screen\"."
```

---

### Task 4: Pane-vertex-cache-miss counter

The P3 acceptance criterion needs something to measure. This is independent of the motion work and lands on its own.

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/render_frame.rs` (imports; new static + accessor; increment on the cache-miss branch near line 402)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/mod.rs:128` area (new field)
- Modify: `nexterm-client-gpu/src/renderer/app.rs:84` area (initialise it)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs` (report once per second)

**Interfaces:**
- Consumes: nothing from Tasks 1–3.
- Produces: `pub(super) fn take_pane_cache_misses() -> u64` in `renderer::render_frame`, returning the count since the previous call and resetting it to zero.

- [ ] **Step 1: Write the failing test**

Add this as a **new** top-level module at the very end of `nexterm-client-gpu/src/renderer/render_frame.rs`. The file already has `#[cfg(test)]` blocks at lines 120 and 1777; do not put this inside either of them.

```rust
#[cfg(test)]
mod pane_cache_counter_tests {
    use super::*;

    /// The counter is a process-global, so this test both reads and resets
    /// it. It runs in the same process as other tests, which never render,
    /// so nothing else touches it.
    #[test]
    fn take_pane_cache_misses_drains_the_counter() {
        take_pane_cache_misses(); // start from a known state
        PANE_CACHE_MISSES.fetch_add(3, Ordering::Relaxed);
        assert_eq!(take_pane_cache_misses(), 3);
        assert_eq!(take_pane_cache_misses(), 0);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu pane_cache_counter 2>&1 | tail -20`

Expected: compilation failure — `cannot find function take_pane_cache_misses`, `cannot find value PANE_CACHE_MISSES`.

- [ ] **Step 3: Add the counter**

In `nexterm-client-gpu/src/renderer/render_frame.rs`, extend the import at line 8:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
```

Then add, just below the imports and above `impl WgpuState`:

```rust
/// Per-pane vertex-cache misses since the last drain (UI/UX v3 P3a).
///
/// The UI/UX v3 plan states the P3 acceptance criterion as "idle
/// `build_pane_vertices` call count does not regress". No function by that
/// name exists. The equivalent is a miss on the C4 pane cache below, which
/// is the only path that rebuilds a pane's cell vertices, so misses per
/// second is the quantity that criterion is really about.
///
/// It is also the first instrument for the cursor-blink invalidation debt
/// tracked as P3 in `docs/plans/audit-round3-2026h2.md`: the cache key
/// includes `cursor_visible`, so a blink alone can force a rebuild. That
/// entry has been marked "needs measurement" ever since it was written.
static PANE_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

/// Read the miss count and reset it to zero.
pub(super) fn take_pane_cache_misses() -> u64 {
    PANE_CACHE_MISSES.swap(0, Ordering::Relaxed)
}
```

In the cache-miss branch — the `} else {` at `render_frame.rs:402`, immediately before the `// Cache miss: rebuild into local vecs` comment — add:

```rust
                            PANE_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p nexterm-client-gpu pane_cache_counter 2>&1 | tail -20`

Expected: 1 test passes.

- [ ] **Step 5: Report the counter once per second at trace level**

In `nexterm-client-gpu/src/renderer/event_handler/mod.rs`, next to `last_status_eval` (line 128), add:

```rust
    /// Last time the pane-cache-miss counter was reported (UI/UX v3 P3a).
    pub(super) last_cache_miss_report: Instant,
```

In `nexterm-client-gpu/src/renderer/app.rs`, next to `last_status_eval: Instant::now(),` (line 84), add:

```rust
            last_cache_miss_report: Instant::now(),
```

In `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs`, widen the tracing import at line 12:

```rust
use tracing::{info, trace, warn};
```

and add this at the end of `on_about_to_wait`, after the `self.update_accesskit_tree_if_needed();` call:

```rust
        // UI/UX v3 P3a: report the pane-vertex-cache miss rate. Idle should
        // read 0; anything above that on a still screen is the cursor-blink
        // invalidation debt (audit-round3 P3) showing itself.
        let since_report = self.last_cache_miss_report.elapsed();
        if since_report >= Duration::from_secs(1) {
            self.last_cache_miss_report = Instant::now();
            let misses = crate::renderer::render_frame::take_pane_cache_misses();
            trace!(
                "pane vertex cache: {misses} misses in {:.2}s",
                since_report.as_secs_f32()
            );
        }
```

`Duration` and `Instant` are already imported in `lifecycle.rs`; if the compiler disagrees, add `use std::time::{Duration, Instant};`.

- [ ] **Step 6: Verify the whole crate builds and passes**

```bash
cargo test -p nexterm-client-gpu 2>&1 | tail -10
cargo clippy -p nexterm-client-gpu -- -D warnings
cargo fmt --check
```

- [ ] **Step 7: Commit**

```bash
git add nexterm-client-gpu/src/renderer/render_frame.rs \
        nexterm-client-gpu/src/renderer/event_handler/mod.rs \
        nexterm-client-gpu/src/renderer/app.rs \
        nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs
git commit -m "feat(client): count pane-vertex-cache misses under NEXTERM_LOG=trace

The UI/UX v3 P3 acceptance criterion names build_pane_vertices, which does
not exist; the measurable equivalent is a miss on the C4 pane cache. This
adds the counter and a once-per-second trace line, which is also the first
instrument for the cursor-blink invalidation debt in audit-round3 P3."
```

---

### Task 5: Let animations drive their own redraws

`AnimationManager::has_active_animation` has been dead code since it was written, so a spring mid-flight advances only when an unrelated redraw happens. This wires it up.

**Files:**
- Modify: `nexterm-client-gpu/src/animations/mod.rs` (remove one `#[allow(dead_code)]`)
- Modify: `nexterm-client-gpu/src/state/mod.rs` (new aggregate on `ClientState`, near `impl ClientState` at line 625)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs` (request the frame)
- Modify: `nexterm-client-gpu/src/renderer/render_frame.rs:795` (use the named duration constant)

**Interfaces:**
- Consumes: `duration::GENTLE` from Task 2; `AnimationManager::has_active_animation(now, duration_ms)` from Task 1.
- Produces: `pub fn ClientState::has_active_animation(&self, now: Instant, fade_duration_ms: u32) -> bool`. Task 6 extends its body; the signature does not change.

- [ ] **Step 1: Write the failing tests**

Add a **new** test module at the end of `nexterm-client-gpu/src/state/mod.rs`. The existing `#[cfg(test)]` block there is `mod pane_border_hit_tests`, which is about split-border hit-testing; these tests do not belong in it.

```rust
#[cfg(test)]
mod animation_frame_tests {
    use super::*;

    /// The UI/UX v3 P3a acceptance criterion in test form: a state with
    /// nothing animating must not ask for a frame. If this ever returns
    /// true at rest, the event loop spins at 60 fps and every pane-vertex
    /// cache miss that follows is a regression P3a introduced.
    #[test]
    fn an_idle_state_wants_no_animation_frames() {
        let state = ClientState::new(80, 24, 1000);
        assert!(!state.has_active_animation(std::time::Instant::now(), 250));
    }

    #[test]
    fn a_running_spring_wants_animation_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        state.animations.record_tab_switch(7, now);
        assert!(state.has_active_animation(now, 250));
    }

    #[test]
    fn a_settled_spring_wants_no_more_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        state.animations.record_tab_switch(7, now);
        for _ in 0..600 {
            state.animations.tick_by_dt(0.016);
        }
        assert!(!state.has_active_animation(now, 250));
    }

    /// With animations disabled the scaled duration is 0, and nothing may
    /// ask for a frame on their behalf.
    #[test]
    fn disabled_animations_want_no_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        state.animations.record_pane_added(1, now);
        assert!(!state.has_active_animation(now, 0));
    }
}
```

Note: `tick_by_dt` is `#[cfg(test)]`-only on `AnimationManager` and is available here because the whole crate compiles as one test binary.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu state:: 2>&1 | tail -20`

Expected: compilation failure — `no method named has_active_animation found for struct ClientState`.

- [ ] **Step 3: Add the aggregate and un-dead the manager method**

In `nexterm-client-gpu/src/animations/mod.rs`, delete the `#[allow(dead_code)]` immediately above `pub fn has_active_animation`. Leave every other `#[allow(dead_code)]` in that file alone.

In `nexterm-client-gpu/src/state/mod.rs`, inside `impl ClientState`, add:

```rust
    /// Whether anything on screen still needs another frame (UI/UX v3 P3a).
    ///
    /// The event loop calls this once per tick and requests a redraw only
    /// when it is true, so an idle terminal keeps costing exactly what it
    /// cost before P3a — no extra frames, and therefore no extra
    /// pane-vertex-cache misses.
    ///
    /// `fade_duration_ms` is the pane fade-in duration the caller is using,
    /// already scaled by `AnimationsConfig::scaled_duration_ms`; 0 means
    /// animations are off and nothing is running.
    ///
    /// Overlay surfaces that own a `Timed` are ORed in here as they are
    /// migrated. Adding a surface means adding a clause; there is no
    /// registry to keep in sync.
    pub fn has_active_animation(&self, now: Instant, fade_duration_ms: u32) -> bool {
        self.animations.has_active_animation(now, fade_duration_ms)
    }
```

If `Instant` is not already imported in `state/mod.rs`, add `use std::time::Instant;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu state:: 2>&1 | tail -20`

Expected: the four new tests pass.

- [ ] **Step 5: Request the frame from the event loop**

In `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs`, at the very end of `on_about_to_wait` (after the pane-cache-miss reporting block added in Task 4), add:

```rust
        // UI/UX v3 P3a: an animation is the only thing that knows it needs
        // another frame. `on_new_events` already wakes the loop every 16 ms,
        // so this costs one predicate per tick and requests a redraw only
        // while something is actually moving.
        let fade_ms = self.app.config.animations.scaled_duration_ms(GENTLE);
        if self
            .app
            .state
            .has_active_animation(Instant::now(), fade_ms)
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
```

Add `use crate::animations::duration::GENTLE;` to the imports at the top of `lifecycle.rs`.

- [ ] **Step 6: Name the magic duration at its other use site**

`render_frame.rs:795` reads `let fade_duration = config.animations.scaled_duration_ms(250);`. That 250 and the `GENTLE` above must stay equal — one is the duration the fade runs for, the other the duration the frame-request predicate assumes. Replace the literal:

```rust
            let fade_duration = config.animations.scaled_duration_ms(crate::animations::duration::GENTLE);
```

- [ ] **Step 7: Verify**

```bash
cargo test -p nexterm-client-gpu 2>&1 | tail -10
cargo clippy -p nexterm-client-gpu -- -D warnings
cargo fmt --check
```

- [ ] **Step 8: Commit**

```bash
git add nexterm-client-gpu/src/animations/mod.rs \
        nexterm-client-gpu/src/state/mod.rs \
        nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs \
        nexterm-client-gpu/src/renderer/render_frame.rs
git commit -m "feat(client): let running animations request their own frames

AnimationManager::has_active_animation had been dead code since it was
written, so a spring mid-flight only advanced when an unrelated redraw
happened. ClientState::has_active_animation aggregates it, and the event
loop requests a redraw only while it is true — an idle terminal asks for
exactly the frames it asked for before."
```

---

### Task 6: Migrate the settings panel to `Timed` and add a close animation

**Files:**
- Modify: `nexterm-client-gpu/src/settings/mod.rs` (replace `open_progress` at line 80; `open()` at 436; `close()` at 443; the initialiser at 358)
- Modify: `nexterm-client-gpu/src/settings/drag.rs:43-48` (`eased_progress`)
- Modify: `nexterm-client-gpu/src/state/mod.rs` (extend the Task 5 aggregate)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs:666-674` (delete the pump; retire finished close animations)
- Modify: `nexterm-client-gpu/src/renderer/render_frame.rs:1069` (draw while closing)
- Modify: `nexterm-client-gpu/src/renderer/overlay/settings/mod.rs:77-101` (accept `now`; draw while closing)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/settings_panel_hit.rs:105`
- Modify call sites: `renderer/input_handler/action.rs:142`, `action.rs:342`, `renderer/input_handler/mod.rs:202`, `mod.rs:204`, `mod.rs:383`, `mod.rs:551`, `renderer/event_handler/mouse.rs:871`

**Interfaces:**
- Consumes: `Timed`, `Curve`, `duration::{NORMAL, FAST}` from Tasks 2–3; `ClientState::has_active_animation` from Task 5.
- Produces:
  - `SettingsPanel { pub open_anim: Option<Timed>, pub closing: Option<Timed> }` replacing `pub open_progress: f32`
  - `pub fn SettingsPanel::open(&mut self, now: Instant, anim: &nexterm_config::AnimationsConfig)`
  - `pub fn SettingsPanel::close(&mut self, now: Instant, anim: &nexterm_config::AnimationsConfig)`
  - `pub fn SettingsPanel::is_visible(&self) -> bool`
  - `pub fn SettingsPanel::eased_progress(&self, now: Instant) -> f32`

- [ ] **Step 1: Write the failing tests**

Add a new test module at the end of `nexterm-client-gpu/src/settings/mod.rs`:

```rust
#[cfg(test)]
mod open_close_animation_tests {
    use super::*;
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

    #[test]
    fn a_fresh_panel_is_closed_and_invisible() {
        let sp = SettingsPanel::default();
        assert!(!sp.is_open);
        assert!(!sp.is_visible());
        assert!(sp.eased_progress(Instant::now()).abs() < 1e-4);
    }

    #[test]
    fn open_runs_from_0_to_1_over_the_entrance_duration() {
        let mut sp = SettingsPanel::default();
        let t0 = Instant::now();
        sp.open(t0, &on());
        assert!(sp.is_open);
        assert!(sp.is_visible());
        assert!(sp.eased_progress(t0).abs() < 1e-3);
        assert!((sp.eased_progress(t0 + Duration::from_millis(200)) - 1.0).abs() < 1e-3);
    }

    /// `is_open` is the truth for input routing and the AccessKit tree, so
    /// dismissing the panel must close it at once. Only the renderer knows
    /// about the fade-out.
    #[test]
    fn close_closes_logically_but_keeps_the_panel_visible() {
        let mut sp = SettingsPanel::default();
        let t0 = Instant::now();
        sp.open(t0, &on());
        let t1 = t0 + Duration::from_millis(200);
        sp.close(t1, &on());
        assert!(!sp.is_open);
        assert!(sp.is_visible());
        assert!(sp.eased_progress(t1) > 0.9);
    }

    #[test]
    fn the_close_animation_fades_to_0_and_then_stops_being_visible() {
        let mut sp = SettingsPanel::default();
        let t0 = Instant::now();
        sp.open(t0, &on());
        // Let the entrance finish first: closing a panel that is still at 0
        // produces an exit animation that is born finished, which would make
        // this test pass without exercising the fade at all.
        let opened = t0 + Duration::from_millis(200);
        sp.close(opened, &on());
        assert!(sp.eased_progress(opened) > 0.9);
        let done = opened + Duration::from_millis(150);
        assert!(sp.eased_progress(done).abs() < 1e-3);
        assert!(sp.closing.is_some_and(|c| c.is_done(done)));
    }

    /// Reopening mid-fade must pick up the value already on screen, not
    /// snap to 0 and replay the entrance.
    #[test]
    fn reopening_during_the_fade_out_is_continuous() {
        let mut sp = SettingsPanel::default();
        let t0 = Instant::now();
        sp.open(t0, &on());
        let opened = t0 + Duration::from_millis(200);
        sp.close(opened, &on());
        let mid = opened + Duration::from_millis(75);
        let before = sp.eased_progress(mid);
        sp.open(mid, &on());
        let after = sp.eased_progress(mid);
        assert!(sp.closing.is_none(), "reopening must cancel the fade-out");
        assert!(
            (after - before).abs() < 5e-2,
            "value jumped on reopen: {before} -> {after}"
        );
    }

    /// The reduced-motion path. `scaled_duration_ms` returns 0 when
    /// animations are disabled, so both transitions are finished the moment
    /// they start.
    #[test]
    fn disabled_animations_open_and_close_instantly() {
        let mut sp = SettingsPanel::default();
        let t0 = Instant::now();
        sp.open(t0, &off());
        assert!((sp.eased_progress(t0) - 1.0).abs() < 1e-4);
        sp.close(t0, &off());
        assert!(sp.eased_progress(t0).abs() < 1e-4);
        assert!(sp.closing.is_some_and(|c| c.is_done(t0)));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu open_close_animation 2>&1 | tail -20`

Expected: compilation failure — `no method named is_visible`, wrong argument count for `open` / `close`.

- [ ] **Step 3: Replace the field and rewrite `open` / `close`**

In `nexterm-client-gpu/src/settings/mod.rs`, replace the `open_progress` field declaration (line 78-80) with:

```rust
    /// Entrance animation. `Some` from the moment the panel opens; its
    /// progress is the panel's visibility while `is_open`.
    pub open_anim: Option<crate::animations::Timed>,
    /// Exit animation — **render-only**. `is_open` goes false the instant
    /// the user dismisses the panel, so input routing and the AccessKit
    /// tree see it as closed immediately; this field is the renderer's
    /// permission to keep drawing it for another few frames while it fades.
    pub closing: Option<crate::animations::Timed>,
```

Replace `open_progress: 0.0,` in the initialiser (line 358) with:

```rust
            open_anim: None,
            closing: None,
```

Replace `open()` and `close()` (lines 436-444 and onward) — keep every existing line of `close()`'s state teardown, only the first two lines change:

```rust
    /// Open the panel and start its entrance animation.
    ///
    /// Fluent calls this a Direct Entrance: arrive quickly, settle gently.
    /// Reopening while the exit animation is still running resumes from the
    /// value already on screen rather than replaying from 0.
    pub fn open(&mut self, now: std::time::Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::NORMAL);
        // Read the value on screen *before* touching either field —
        // `eased_progress` derives it from them.
        let resume_from = self.closing.is_some().then(|| self.eased_progress(now));
        self.closing = None;
        self.open_anim = Some(match resume_from {
            Some(v) => Timed::resuming_at(now, v, ms, Curve::DecelerateMax),
            None => Timed::new(now, ms, Curve::DecelerateMax),
        });
        self.is_open = true;
    }

    /// Close the panel and start its exit animation.
    ///
    /// Fluent calls this a Gentle Exit: linger, then leave quickly. `is_open`
    /// goes false here, not when the animation ends — pressing Esc means the
    /// panel is closed, whatever is still on screen.
    pub fn close(&mut self, now: std::time::Instant, anim: &nexterm_config::AnimationsConfig) {
        use crate::animations::{Curve, Timed, duration};

        let ms = anim.scaled_duration_ms(duration::FAST);
        // As in `open`: read the on-screen value first. `closing` counts up
        // while visibility counts down, hence the inversion.
        let visibility = self.eased_progress(now);
        self.open_anim = None;
        self.closing = Some(Timed::resuming_at(
            now,
            1.0 - visibility,
            ms,
            Curve::AccelerateMax,
        ));
        self.is_open = false;

        // ---- existing teardown below, unchanged ----
        self.drag_slider = None;
        // ...
    }
```

**Do not retype the teardown.** In the real file, `close()` continues from
`self.drag_slider = None;` through the end of the function (`dirty`,
`font_family_editing`, `tab_rename_editing`, `theme_hover_preview`,
`ssh_field_editing`, the SSH and key delete-dialog flags, and anything else
already there). Leave every one of those lines exactly as it is; the only
edits to `close()` are the new signature, the three animation lines, and
deleting the old `self.open_progress = 0.0;`.

Add, next to them:

```rust
    /// Whether the renderer should draw the panel at all.
    ///
    /// True while open, and while an exit animation is still running.
    pub fn is_visible(&self) -> bool {
        self.is_open || self.closing.is_some()
    }
```

- [ ] **Step 4: Rewrite `eased_progress` in `drag.rs`**

Replace `SettingsPanel::eased_progress` (`nexterm-client-gpu/src/settings/drag.rs:43-48`) with:

```rust
    /// Panel visibility in `[0, 1]`: 0 fully hidden, 1 fully shown.
    ///
    /// The curve now comes from the `Timed` (UI/UX v3 P3a) rather than the
    /// hand-rolled ease-out this used to apply.
    pub fn eased_progress(&self, now: std::time::Instant) -> f32 {
        if let Some(closing) = self.closing {
            return 1.0 - closing.progress(now);
        }
        self.open_anim.map_or(0.0, |a| a.progress(now))
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu open_close_animation 2>&1 | tail -20`

Expected: the 6 new tests pass. The crate as a whole will not build yet — the call sites come next.

- [ ] **Step 6: Update the seven `open()` / `close()` call sites**

Each site gains two arguments. The pattern:

```rust
// before
self.app.state.settings_panel.open();
// after
let now = std::time::Instant::now();
let anim = self.app.config.animations.clone();
self.app.state.settings_panel.open(now, &anim);
```

Cloning `AnimationsConfig` (two small fields, `Clone`) sidesteps any borrow-checker complaint about holding `&self.app.config` while `&mut self.app.state` is live. If the direct form `self.app.state.settings_panel.open(now, &self.app.config.animations)` compiles at a given site, prefer it — disjoint field borrows usually allow it.

Sites: `renderer/input_handler/action.rs:142` (open), `action.rs:342` (open), `renderer/input_handler/mod.rs:202` (close), `mod.rs:204` (open), `mod.rs:383` (close), `mod.rs:551` (close), `renderer/event_handler/mouse.rs:871` (close).

- [ ] **Step 7: Draw the panel while it is fading out**

`nexterm-client-gpu/src/renderer/render_frame.rs:1069` — change the gate:

```rust
        if state.settings_panel.is_visible() {
```

`nexterm-client-gpu/src/renderer/overlay/settings/mod.rs` — `build_settings_panel_verts` needs the frame clock. Add a parameter after `cell_h: f32`:

```rust
        now: std::time::Instant,
```

and change lines 95-101:

```rust
        let sp = &state.settings_panel;
        if !sp.is_visible() {
            return None;
        }

        // Open/close animation (UI/UX v3 P3a): Fluent Direct Entrance in,
        // Gentle Exit out. 0 = hidden, 1 = fully shown.
        let eased = sp.eased_progress(now);
```

At the call site in `render_frame.rs:1070`, pass `frame_now` (the `Instant` already bound at `render_frame.rs:178`) in the new position.

`nexterm-client-gpu/src/renderer/event_handler/settings_panel_hit.rs:105`:

```rust
        let eased = sp.eased_progress(std::time::Instant::now());
```

- [ ] **Step 8: Teach the aggregate about the panel, and retire finished fades**

In `nexterm-client-gpu/src/state/mod.rs`, extend `ClientState::has_active_animation`:

```rust
    pub fn has_active_animation(&self, now: Instant, fade_duration_ms: u32) -> bool {
        if self.animations.has_active_animation(now, fade_duration_ms) {
            return true;
        }
        let sp = &self.settings_panel;
        if sp.closing.is_some_and(|c| !c.is_done(now)) {
            return true;
        }
        sp.is_open && sp.open_anim.is_some_and(|a| !a.is_done(now))
    }
```

In `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs`, **delete** lines 666-674 — the whole `// Advance the settings-panel open/close animation` block including its comment — and put in its place:

```rust
        // UI/UX v3 P3a: drop a finished exit animation so the renderer stops
        // drawing the panel. The entrance animation is left in place; it is
        // the panel's visibility while it is open.
        let now = Instant::now();
        let sp = &mut self.app.state.settings_panel;
        if sp.closing.is_some_and(|c| c.is_done(now)) {
            sp.closing = None;
        }
```

The redraw request added in Task 5 already covers both animations, because the aggregate now reports them.

- [ ] **Step 9: Add the aggregate tests for the panel**

Add to the `animation_frame_tests` module in `state/mod.rs` that Task 5 created:

```rust
    #[test]
    fn an_opening_settings_panel_wants_animation_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let now = std::time::Instant::now();
        state
            .settings_panel
            .open(now, &nexterm_config::AnimationsConfig::default());
        assert!(state.has_active_animation(now, 250));
        let done = now + std::time::Duration::from_millis(200);
        assert!(!state.has_active_animation(done, 250));
    }

    #[test]
    fn a_closing_settings_panel_wants_animation_frames() {
        let mut state = ClientState::new(80, 24, 1000);
        let t0 = std::time::Instant::now();
        let anim = nexterm_config::AnimationsConfig::default();
        state.settings_panel.open(t0, &anim);
        // Close only after the entrance has finished — closing at 0
        // visibility yields an exit that is born done and proves nothing.
        let opened = t0 + std::time::Duration::from_millis(200);
        state.settings_panel.close(opened, &anim);
        assert!(state.has_active_animation(opened, 250));
        let done = opened + std::time::Duration::from_millis(150);
        assert!(!state.has_active_animation(done, 250));
    }
```

- [ ] **Step 10: Verify the whole crate**

```bash
cargo test -p nexterm-client-gpu 2>&1 | tail -15
cargo clippy -p nexterm-client-gpu -- -D warnings
cargo fmt --check
```

Expected: everything green. If `panel_drag_tests` in `settings/drag.rs` fails to compile because it constructed a panel with `open_progress`, update those constructions to set `open_anim` / `closing` — do not change what the tests assert.

- [ ] **Step 11: Commit**

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): animate the settings panel open and close with Timed

The panel's entrance was a frame-count hack (open_progress += 0.15,
'assumes 60 fps'), so its real duration drifted with frame rate and it
ignored animations.intensity entirely; there was no exit animation at all.
It now runs on Timed: a 200 ms Fluent Direct Entrance in, a 150 ms Gentle
Exit out, both scaled by the configured intensity.

is_open stays the single truth for input routing and the AccessKit tree
and still goes false the instant the user dismisses the panel. The new
closing field is render-only — the renderer's permission to keep drawing
the panel while it fades."
```

---

### Task 7: Documentation

**Files:**
- Modify: `nexterm-client-gpu/CLAUDE.md` (the `animations.rs` bullet)
- Modify: `docs/plans/ui-ux-modernization-v3.md` (P3 section, progress checklist)

**Interfaces:**
- Consumes: everything above.
- Produces: nothing code-facing.

- [ ] **Step 1: Update the crate guide**

In `nexterm-client-gpu/CLAUDE.md`, replace the `animations.rs` bullet with:

```markdown
- `animations/` — UI animation foundation. `mod.rs` holds `AnimationManager` and the spring physics (`SpringState`) for the tab accent and per-pane dim; `easing.rs` the time-based helpers (`ease_out_cubic`, `compute_progress`); `curve.rs` the nine Fluent 2 cubic-bezier curves and eight duration steps, transcribed from `microsoft/fluentui` `packages/tokens` (do not re-derive them by eye); `timed.rs` the `Timed { start, duration_ms, curve }` value type. Springs are for motion interrupted by a new target; `Timed` is for transitions with a known start, end and duration. A zero duration — what `AnimationsConfig::scaled_duration_ms` returns when `enabled = false` or `intensity = "off"` — makes a `Timed` finished on creation, which is the whole reduced-motion path. `ClientState::has_active_animation` is the one place that decides whether the event loop requests another frame; a surface that gains a `Timed` adds a clause there or it will simply never animate.
```

- [ ] **Step 2: Record the P3 split and correct the stale acceptance criterion**

In `docs/plans/ui-ux-modernization-v3.md`, in the "### P3 — Motion language (M–L)" section, replace the acceptance bullet (lines 203-206) with:

```markdown
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
```

In the "## Progress" section, replace the `- [ ] P3 motion language + reduced-motion detection` line with:

```markdown
- [ ] P3 motion language + reduced-motion detection
  - [ ] P3a motion foundation (`Timed`, Fluent curves, animation-driven redraw, settings panel)
  - [ ] P3b widget and overlay motion
  - [ ] P3c OS reduced-motion detection
```

- [ ] **Step 3: Verify nothing else drifted**

```bash
cargo test -p nexterm-config doc_matches_schema 2>&1 | tail -5
```

Expected: pass. P3a adds no config key, so this guard should be unaffected; running it confirms that.

- [ ] **Step 4: Commit**

```bash
git add nexterm-client-gpu/CLAUDE.md docs/plans/ui-ux-modernization-v3.md
git commit -m "docs: record the P3 split and the animations/ module layout

Also corrects the P3 acceptance criterion, which named a build_pane_vertices
function that does not exist. The measurable equivalent is the pane-vertex
cache miss rate, which P3a now counts."
```

---

## Final Verification

- [ ] **Full workspace suite**

```bash
cargo test 2>&1 | tail -20
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

- [ ] **Confirm no dependency drift**

```bash
git status --short Cargo.lock
```

Expected: empty. If `Cargo.lock` changed, run `bash scripts/regenerate-flatpak-sources.sh` and commit `pkg/flatpak/cargo-sources.json` alongside it.

- [ ] **Hand-run check (cannot be done in CI or in the dev container)**

On real hardware: open the settings panel with `Ctrl+,` and close it with `Esc`; the entrance should arrive quickly and settle, the exit should linger then leave. Set `animations.intensity = "off"` and confirm both are instant. Run with `NEXTERM_LOG=trace` and confirm the `pane vertex cache: N misses in 1.00s` line reads 0 on a still screen with the cursor blink disabled, and reads a nonzero steady rate with it enabled — the latter is the audit-round3 P3 debt, expected and not a P3a regression.

This step is **not** a blocker for the PR; like P2a–P2c it joins the on-device verification backlog. State plainly in the PR description that it has not been run.
