# P3a: Motion Foundation — `Timed` Animations and Animation-Driven Redraw

Status: proposed
Related plan: `docs/plans/ui-ux-modernization-v3.md`, section "P3 — Motion language (M–L)" (lines 189-206), checklist item "P3 motion language + reduced-motion detection" (line 518)
Related prior work: P2a soft shadows (#63), P2b in-app acrylic (#74), P2c window backdrop (#75)

P3 is split into three PRs, mirroring how P2 shipped:

| PR | Scope |
|---|---|
| **P3a (this spec)** | `Timed` animations, the Fluent curve/duration tables, animation-driven redraw, settings-panel open/close as the first consumer |
| P3b | Apply the language to widget hover/press, dialogs, flyouts, tooltips |
| P3c | OS reduced-motion detection (Windows `SPI_GETCLIENTAREAANIMATION`, macOS `accessibilityDisplayShouldReduceMotion`) |

## Goal

Give Nexterm a motion foundation that is time-based, interruptible, and
measurable — and prove it by replacing the one ad-hoc animation the codebase
already has. Today the client cannot animate anything that is not driven by
some other source of frames, because nothing requests a redraw on an
animation's behalf.

## What exists today (measured, not assumed)

- `nexterm-client-gpu/src/animations.rs` (660 lines) holds `AnimationManager`:
  spring physics for the tab accent and the per-pane dim overlay, plus
  time-based helpers (`compute_progress`, `ease_out_cubic`, `linear`) for the
  pane fade-in.
- **`AnimationManager::has_active_animation` (`animations.rs:328`) is dead
  code.** It carries `#[allow(dead_code)]` and is referenced only from its own
  tests. Nothing in the client asks "is an animation running?", so nothing
  requests a frame on an animation's behalf.
- Consequently the springs advance only when a redraw happens for an unrelated
  reason. `render_frame.rs:179` ticks them; if the terminal is idle and no PTY
  output arrives, a spring mid-flight simply stops mid-value until the next
  unrelated redraw.
- `renderer/event_handler/lifecycle.rs:29` sets
  `ControlFlow::WaitUntil(now + 16ms)`, so the event loop already wakes at
  ~60 Hz to poll the PTY. Waking is not the missing piece; requesting a redraw
  is.
- The **one** UI animation that does run is the settings-panel open, and it
  works by pumping its own redraws:
  ```rust
  // lifecycle.rs:668-674
  let sp = &mut self.app.state.settings_panel;
  if sp.is_open && sp.open_progress < 1.0 {
      sp.open_progress = (sp.open_progress + 0.15).min(1.0);
      if let Some(w) = &self.window { w.request_redraw(); }
  }
  ```
  This is frame-count-based (the comment says "assumes 60 fps"), so its real
  duration drifts with frame rate; it ignores `animations.intensity` entirely,
  so "subtle" and "energetic" do nothing to it; and it has **no close
  animation** — `close()` (`settings/mod.rs:443`) resets `open_progress` to 0
  and the panel vanishes on the next frame.
- `SettingsPanel.open_progress: f32` (`settings/mod.rs:80`) has exactly three
  readers: the pump above, `settings/drag.rs:45` (`eased_progress`, a
  hand-rolled `1-(1-t)³`), and the two assignments in `open()` / `close()`.
- `SettingsPanel::open()` is called from `input_handler/action.rs:142`,
  `action.rs:342` and `input_handler/mod.rs:204`; `close()` from
  `input_handler/mod.rs:202`, `mod.rs:383`, `mod.rs:551` and
  `event_handler/mouse.rs:871`.
- `is_open` is the truth for three independent consumers: the AccessKit tree
  (`accessibility.rs:845`, `accessibility.rs:2156`), input routing, and
  rendering. It is also read by P2b's acrylic capture through
  `count_open_overlays` (`render_frame.rs:105`).
- Widget hover/press is binary: `draw/mod.rs:152` paints a hover row fill when
  `spec.hovered && spec.enabled() && spec.kind().is_interactive()`, with no
  transition. `settings/hover.rs` already stores `HoverDwell { category,
  index, since }` — the *entry* timestamp a hover fade-in would need. It does
  not record the widget the pointer left, which a fade-out would need. (P3b
  scope; noted here so P3a's shape does not preclude it.)

### The acceptance criterion names a function that does not exist

The plan's P3 acceptance criterion is "idle `build_pane_vertices` call count
does not regress", citing `plans/audit-round3-2026h2.md` P3. There is no
`build_pane_vertices` in the workspace. The measurement it describes maps onto
today's code as:

- `render_frame.rs:370` — `cache_valid`, the C4 per-pane vertex cache check.
- `render_frame.rs:409` — `build_grid_verts_in_rect`, run **only on a cache
  miss**.
- The cache key includes `cursor_visible` (`render_frame.rs:382`), so a cursor
  blink alone invalidates it — which is exactly the audit-round3 P3 debt,
  still present.

So the quantity to hold is **pane-cache misses per second while idle**, and
this spec uses that wording. The plan's line is stale and P3a corrects it.

## Scope

**In scope:**

- A `Curve` enum carrying the nine Fluent 2 curve tokens, with a pure
  cubic-bezier solver.
- A `duration` constant table carrying the eight Fluent 2 duration tokens.
- A `Timed { start, duration_ms, curve }` value type with pure progress
  queries and interruption support.
- Splitting `animations.rs` into an `animations/` module directory. The file is
  660 lines today; adding the above with its tests would push a single file
  past the 800-line ceiling in the coding conventions.
- Wiring an animation-driven redraw request into `about_to_wait`, backed by an
  aggregate `ClientState::has_active_animation`.
- Migrating the settings-panel open animation to `Timed` and adding a close
  animation, via a render-only "ghost" field.
- A pane-cache-miss counter so the acceptance criterion is measurable at all.

**Out of scope:**

- Widget hover/press, dialog, flyout and tooltip motion — P3b.
- OS reduced-motion detection — P3c. `animations.enabled = false` and
  `intensity = "off"` remain the reduced-motion controls for P3a, and they
  already work through `scaled_duration_ms`.
- Fixing the audit-round3 P3 debt itself (splitting the cursor into its own
  vertex buffer). P3a makes it measurable; the fix stays tracked in its own
  plan.
- Migrating the cursor blink to `Timed`. Blink is a square wave, not an eased
  transition, and touching it would entangle P3a with the cache-invalidation
  debt above.
- Any new config key or user-facing string. P3a adds neither.

## Sourcing the Fluent values

The Fluent 2 design site documents motion qualitatively and publishes no token
values ([fluent2.microsoft.design/motion](https://fluent2.microsoft.design/motion)).
The values below are taken from the implementation repository, which is the
authoritative source for the tokens:

- `microsoft/fluentui`, `packages/tokens/src/global/curves.ts`
- `microsoft/fluentui`, `packages/tokens/src/global/durations.ts`

Curves (CSS control points, `P0 = (0,0)`, `P3 = (1,1)`):

| Token | Control points |
|---|---|
| `curveAccelerateMax` | 0.9, 0.1, 1, 0.2 |
| `curveAccelerateMid` | 1, 0, 1, 1 |
| `curveAccelerateMin` | 0.8, 0, 0.78, 1 |
| `curveDecelerateMax` | 0.1, 0.9, 0.2, 1 |
| `curveDecelerateMid` | 0, 0, 0, 1 |
| `curveDecelerateMin` | 0.33, 0, 0.1, 1 |
| `curveEasyEaseMax` | 0.8, 0, 0.2, 1 |
| `curveEasyEase` | 0.33, 0, 0.67, 1 |
| `curveLinear` | 0, 0, 1, 1 |

Durations: `ultraFast 50`, `faster 100`, `fast 150`, `normal 200`,
`gentle 250`, `slow 300`, `slower 400`, `ultraSlow 500` (ms).

All nine curves are defined even though P3a consumes two, because the table is
a transcription of an external spec: a partial copy invites a future PR to
re-derive a missing constant by eye. They are `const fn` data with no runtime
cost, and P3b consumes most of the rest.

## Architecture

### 1. Module split

`nexterm-client-gpu/src/animations.rs` becomes `animations/`:

| File | Contents |
|---|---|
| `animations/mod.rs` | `AnimationManager`, `SpringState`, `MAX_DIM_ALPHA`; re-exports the submodules |
| `animations/easing.rs` | `ease_out_cubic`, `linear`, `compute_progress` (moved verbatim, tests included) |
| `animations/curve.rs` | `Curve`, the bezier solver, the `duration` constants |
| `animations/timed.rs` | `Timed` |

The split is mechanical: no behaviour change, and the existing tests move with
the code they cover. `mod.rs` re-exports so no call site outside the module
changes.

### 2. `Curve` and the bezier solver

```rust
pub enum Curve {
    Linear,
    AccelerateMax, AccelerateMid, AccelerateMin,
    DecelerateMax, DecelerateMid, DecelerateMin,
    EasyEaseMax, EasyEase,
}

impl Curve {
    pub const fn control_points(self) -> (f32, f32, f32, f32);
    pub fn eval(self, t: f32) -> f32;
}
```

`eval` follows the CSS `cubic-bezier` definition: given `t` as the fraction of
elapsed time, solve `X(s) = t` for the curve parameter `s`, then return `Y(s)`.

- `X(s) = 3(1-s)²s·x1 + 3(1-s)s²·x2 + s³`, and likewise for `Y` with `y1, y2`.
- Solve with Newton-Raphson seeded at `s = t`, up to 8 iterations, falling back
  to bisection over `[0, 1]` when the derivative is near zero (which
  `AccelerateMid` and `DecelerateMid` can produce at the endpoints).
- `Linear` short-circuits to `t`; `t` is clamped to `[0, 1]` on entry.

The solver is pure and self-contained, so it is fully covered by unit tests
without a GPU.

### 3. `Timed`

```rust
pub struct Timed { start: Instant, duration_ms: u32, curve: Curve }

impl Timed {
    pub fn new(start: Instant, duration_ms: u32, curve: Curve) -> Self;
    /// Eased progress in [0, 1].
    pub fn progress(&self, now: Instant) -> f32;
    /// Linear progress in [0, 1], before easing.
    pub fn raw_progress(&self, now: Instant) -> f32;
    pub fn is_done(&self, now: Instant) -> bool;
    /// Build an animation that already holds `value` at `now`, so an
    /// interrupted transition resumes instead of popping.
    pub fn resuming_at(now: Instant, value: f32, duration_ms: u32, curve: Curve) -> Self;
}
```

`progress` delegates to the existing `compute_progress`, so
`duration_ms == 0` yields `1.0` immediately. That is the whole reduced-motion
path: `AnimationsConfig::scaled_duration_ms` already returns 0 when
`enabled = false` or `intensity = "off"`, so every `Timed` constructed through
it is instant, and `is_done` is true on the first query.

`resuming_at` is how an interruption is expressed: the caller reads whatever
value is on screen and asks for an animation that starts there. It finds the
curve parameter `t` with `curve.eval(t) ≈ value` by bisection (the curves are
monotone, so 20 iterations reach f32 precision), then back-dates
`start = now - duration_ms · t`. When `duration_ms == 0` it degenerates to a
finished animation, like every other constructor. The guarantee is continuity
of *value*, not of derivative — the interrupted motion may change speed
abruptly, which is the standard behaviour for a reversed transition.

### 4. Animation-driven redraw

A single aggregate on `ClientState`:

```rust
impl ClientState {
    pub fn has_active_animation(&self, now: Instant) -> bool
}
```

It ORs the existing `AnimationManager::has_active_animation` (which stops being
dead code) with each surface that owns a `Timed`. In P3a that is the settings
panel's open and closing animations; P3b extends the same function.

`about_to_wait` (`lifecycle.rs`, after the existing per-tick work) gains:

```rust
if self.app.state.has_active_animation(Instant::now())
    && let Some(w) = &self.window
{
    w.request_redraw();
}
```

and the frame-count pump at `lifecycle.rs:668-674` is deleted.

**Why this satisfies the acceptance criterion.** When nothing is animating the
aggregate returns `false`, so P3a requests zero redraws that the current code
would not already have requested. Pane-cache misses per second while idle are
therefore unchanged by construction, and a unit test pins the aggregate to
`false` for a freshly built, non-animating state.

### 5. Pane-cache-miss counter

`render_frame.rs` gains a `static PANE_CACHE_MISSES: AtomicU64`, incremented on
the cache-miss branch (`render_frame.rs:402`) and logged at `trace` level once
per second alongside the elapsed interval. Enabled by the existing
`NEXTERM_LOG=trace`; on any other level the cost is one relaxed atomic
increment per miss.

This is the instrument the plan asks for. It is also the first way to observe
the audit-round3 P3 debt, which has been "needs measurement" since that plan
was written.

### 6. Settings panel

`open_progress: f32` is replaced by two fields:

```rust
pub open_anim: Option<Timed>,   // running or settled open transition
pub closing: Option<Timed>,     // render-only ghost; None once finished
```

- `open(now, &animations_cfg)` — sets `is_open = true`, clears `closing`, and
  starts a `Timed` over `duration::NORMAL` (200 ms) on `Curve::DecelerateMax`
  (Fluent's Direct Entrance). If a `closing` animation is in flight, the new
  open is built with `Timed::resuming_at(now, open_progress(now), …)` so it
  picks up from the value already on screen.
- `close(now, &animations_cfg)` — sets `is_open = false` **immediately** and
  starts `closing` over `duration::FAST` (150 ms) on `Curve::AccelerateMax`
  (Gentle Exit). The existing state teardown in `close()` is unchanged.
- `open_progress(now) -> f32` replaces the field for readers: the open
  animation's progress while open, `1.0 - closing.progress(now)` while closing,
  `0.0` otherwise. `drag.rs`'s hand-rolled `eased_progress` is deleted — the
  curve now comes from the `Timed`.
- The renderer draws the panel when `is_open || closing.is_some()`.
- `closing` is cleared when `is_done`, checked in the same `about_to_wait` pass
  that requests the frame.

**The `is_open` contract is unchanged.** Input routing, the AccessKit tree and
`count_open_overlays` continue to read `is_open` alone and see the panel as
closed the instant the user dismisses it — which is what a user pressing `Esc`
means. Only the renderer knows about `closing`. This keeps the blast radius off
`accessibility.rs` and the input path entirely.

**Known artefact, accepted.** `close()` tears down in-flight edit state
(`font_family_editing`, `ssh_field_editing`, the delete dialogs, …) at the
moment it is called, so the 150 ms ghost renders the panel with those edits
already cancelled — a text caret disappears as the panel fades. The edit *was*
cancelled, so this is arguably the honest depiction, and deferring the teardown
would mean a reopen within 150 ms had to decide whether to resurrect it. Not
worth the state machine.

Because `open()` and `close()` now need the clock and the animation config,
their signatures change; the seven call sites listed above pass
`Instant::now()` and `&config.animations`. Taking `now` as a parameter rather
than reading the clock inside keeps both functions testable.

## Testing

CI-verifiable, no GPU required:

**`curve.rs`**
- `eval(0.0) == 0.0` and `eval(1.0) == 1.0` for all nine curves.
- Monotonicity: sampling `t` across `[0, 1]` never decreases (all nine curves
  have `y1, y2 ∈ [0, 1]`, so they are monotone by construction).
- `Linear::eval(t) == t`.
- Out-of-range inputs clamp.
- `AccelerateMid` (1, 0, 1, 1) and `DecelerateMid` (0, 0, 0, 1) specifically —
  they are the degenerate cases that exercise the bisection fallback.
- Known-value spot checks against the CSS reference for `EasyEaseMax`.
- Direction: an accelerate curve is below linear at `t = 0.5`; a decelerate
  curve is above it.

**`timed.rs`**
- Progress is 0 at the start, 1 at and past the duration.
- `duration_ms == 0` yields `1.0` and `is_done == true` immediately — the
  reduced-motion path.
- `progress` differs from `raw_progress` for a non-linear curve.
- `resuming_at(now, v, …).progress(now) ≈ v` across a sweep of `v` and every
  curve (continuity), and `resuming_at` with `duration_ms == 0` is done.

**`ClientState::has_active_animation`**
- A default state with nothing animating returns `false`. This is the
  acceptance criterion in test form.
- Returns `true` while the panel's open `Timed` is running, `false` once done.
- Returns `false` for a running `Timed` built with `intensity = "off"`, because
  such a `Timed` is already done.

**Settings panel**
- `open()` starts progress at 0 and reaches 1 after the duration.
- `close()` leaves `is_open == false` but `closing.is_some()`, and
  `open_progress` decreases from 1 toward 0.
- `close()` then `open()` inside the close duration produces a continuous
  value (no jump to 0).
- With `animations.enabled = false`, `open()` gives progress 1.0 at once and
  `close()` leaves `closing` already done.

**Regression**
- The existing `AnimationManager` and `panel_drag_tests` suites must stay green
  through the module split; they move, they do not change.

### Not verifiable here — stated plainly

Whether the 200 ms entrance and 150 ms exit *feel* right, and whether the
Fluent curves read as intended at Nexterm's panel size, cannot be judged from
CI or from this container. That needs a hand-run on real hardware, and this
spec does not claim otherwise. It joins the on-device verification backlog that
P2a–P2c already contribute to.

The pane-cache-miss counter's idle reading likewise needs a real session to
observe; the CI test proves only that P3a requests no additional redraws.

## Risks

- **Module split noise.** Moving 660 lines makes the diff large and can hide a
  real change inside a rename. Mitigation: the split lands as its own commit
  with no edits beyond `mod` declarations and import paths, so review can
  confirm it by diffing with whitespace and move detection on.
- **Signature change on `open()` / `close()`.** Seven call sites, all in input
  handlers that already have access to the config. Low risk, but it is the
  widest edit in the PR.
- **Interruption continuity.** Back-dating `start` is easy to get subtly wrong
  in a way tests pass but the eye catches. The continuity test above pins the
  value; the derivative is explicitly not guaranteed.
- **Newton-Raphson on degenerate curves.** `AccelerateMid` and `DecelerateMid`
  have zero derivative at an endpoint. The bisection fallback exists for this
  and is tested directly rather than left to chance.
- **The counter is a permanent atomic in a hot path.** One relaxed increment on
  the cache-miss branch only — the branch that is already doing vertex
  construction. Negligible next to its neighbour.

## Delivery

One PR to `master`, English commit messages and description, `cargo clippy
-- -D warnings` and `cargo fmt --check` green, full workspace test suite green.
No `Cargo.lock` change is expected, so no flatpak sources regeneration; if one
appears, `bash scripts/regenerate-flatpak-sources.sh` runs before the PR.

`docs/CONFIGURATION.md` is untouched because P3a adds no config key. The
`animations.rs` entry in `nexterm-client-gpu/CLAUDE.md` is updated to describe
the module directory and the `Timed`/`Curve` types, and
`docs/plans/ui-ux-modernization-v3.md` gains the P3a/P3b/P3c split plus the
correction to the stale `build_pane_vertices` acceptance criterion.
