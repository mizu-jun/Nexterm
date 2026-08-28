# P3b2b: Hover Cross-Fade for the Tab Bar and the Window Buttons — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the last two pointer-hover models — a tab in the tab bar, and a window button in the custom title bar — the same cross-fade the settings rows and context menu got in P3b2a, reusing `HoverTransition<Id>` unchanged.

**Architecture:** Both models live in one builder, `build_tab_bar_verts` in `renderer/ui_verts.rs`. Each gets a `HoverTransition` on `ClientState`, retargeted from the same handler that writes its logical hover id, and consumed where the builder picks a colour. No new primitive: P3b2a's `HoverTransition` and `lerp_rgba` cover everything here.

**Tech Stack:** Rust 2024, `nexterm-client-gpu` (wgpu + winit + cosmic-text). No new dependency.

**Spec:** `docs/superpowers/specs/2026-08-29-p3b2-hover-crossfade-design.md` — this plan implements its **P3b2b** row. P3b2a (the settings rows and the context menu) shipped as PR #80.

## Global Constraints

- No `unwrap()`. Use `?` or `expect("reason")` with a concrete message.
- Comments, doc-comments and commit messages in this repo are **English**.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` must be green before any commit.
- No new config key, no new user-facing string, no locale change, no `Cargo.lock` change. `tab_bar.hover_highlight` already exists and keeps its exact current meaning.
- Every duration reaching a `Timed` must go through `AnimationsConfig::scaled_duration_ms`; `HoverTransition::retarget` does that internally, so pass no duration.
- **Durations are fixed by the spec and must not be re-tuned:** 100 ms `EasyEase`, in and out, same as P3b2a. Hover is one gesture — a tab fading at a different speed from a settings row would read as inconsistency, not hierarchy.
- **The logical hover id stays the truth.** `ClientState.hovered_tab_id` and `ClientState.hovered_window_button` keep their current meaning and all their write sites. The transitions are *additional* state.
- Paths are relative to `nexterm-client-gpu/src/`.

## Two traps this plan exists to steer around

Both were found by measuring, and neither is in the design document.

### Trap 1: `is_hovered` has a non-colour use

In `build_tab_bar_verts`, `let is_hovered = state.hovered_tab_id == Some(pane_id);` (`ui_verts.rs:295`) is read **twice**:

- `ui_verts.rs:363` — picks the tab's background colour. **This is what P3b2b animates.**
- `ui_verts.rs:474` — `if is_hovered && state.tab_drag.is_none() && label_w >= hover_btn_min_width` gates whether the tear-out `[↗]` and close `[×]` buttons are **drawn at all**.

The second is *behavioural*, not cosmetic. Driving it from the weight would make the buttons linger through the fade-out and appear the instant a fade-in starts — and, worse, they are click targets hit-tested in `mouse.rs`, so a button that is visible at weight 0.05 is a button the user can hit while it is disappearing.

**So `:474` keeps reading the boolean.** Only `:363` consults the weight. A single `is_hovered` local feeding both is exactly why this is easy to get wrong; leave the local in place and add the weight beside it rather than replacing it.

### Trap 2: `hovered_window_button` has two write sites, and the second is easy to miss

- `renderer/event_handler/mouse.rs:465-482` — the pointer-motion hit test. The obvious one.
- `renderer/event_handler/mod.rs:505-511` — `UserEvent::SnapMaximizeHover`, the **Windows snap-layout** path, where the OS tells us the maximize button is being hovered.

Retargeting only the first leaves the snap-layout path snapping while the mouse path fades, and nothing fails to compile. This is the same class of defect P3b1's whole-branch review caught (an AccessKit path bypassing `show_password_modal`), so treat the grep as mandatory rather than as a formality:

```
grep -rn "hovered_window_button\s*=\|hovered_tab_id\s*=" nexterm-client-gpu/src
```

## Which mechanism applies where — do not mix them

P3b2a established two kinds of interpolation, and this phase needs **both, in one builder**:

| Site | Shape | Mechanism |
|---|---|---|
| Tab background (`ui_verts.rs:363`) | The quad is **always drawn**, in one of four colours; hover is the third branch | `lerp_rgba(inactive_bg, brightened, w)` — the colour itself moves |
| Window-button fill (`ui_verts.rs:779-790`) | **Additive** — no quad at all when not hovered | scale the fill's alpha by `w`, and emit nothing at `w == 0.0` |
| Window-button glyph (`ui_verts.rs:803`) | Opaque swap between two tokens | `lerp_rgba(text_secondary, text_primary, w)` |

Using `lerp_rgba` on the additive fill would assume what sits behind the button and would keep painting a quad at weight 0. Scaling alpha on the tab background would fade the tab out of the bar rather than into its hover colour. The plan's task steps use the right one at each site; do not unify them.

## What already exists, measured

- **`build_tab_bar_verts` already takes `_animations_cfg: &nexterm_config::AnimationsConfig`** (`ui_verts.rs:172`) and never uses it. The config is plumbed and waiting; only `now` needs adding, and `frame_now` exists at the call site (`render_frame.rs:197`, already passed to nine other builders).
- It takes `state: &mut ClientState`, so it can read the transitions without a signature change beyond `now`.
- Both write sites for each model already do a change-only `request_redraw` (`mouse.rs:457`, `:482`). Those become redundant once the aggregate covers the transitions — **leave them.** A redraw on the frame the target changes is still correct, and removing them is a separate simplification.
- `ui_verts.rs` has 5 unit tests, all on pure helpers (`progress_indicator_style` and friends). The vertex builders themselves are not unit-testable without a GPU, so this phase's tests are state-level, exactly as P3b2a's were.
- `WindowButton` (`state/mod.rs:443`) derives `Copy, PartialEq, Eq`; `hovered_tab_id` is `Option<u32>`. Both satisfy `HoverTransition<Id>`'s bounds with no change.

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `state/mod.rs` | `ClientState.tab_hover`, `ClientState.window_button_hover`, two `has_active_animation` clauses, the tests | 1, 2 |
| `renderer/event_handler/mouse.rs` | retarget both models from the pointer-motion handler | 1, 2 |
| `renderer/event_handler/mod.rs` | retarget the window button from the snap-layout event | 2 |
| `renderer/ui_verts.rs` | `now` parameter, the un-prefixed `animations_cfg`, three colour sites | 1, 2 |
| `renderer/render_frame.rs` | pass `frame_now` to `build_tab_bar_verts` | 1 |
| `animations/hover.rs` | resolve `target()`'s dead-code allow | 3 |
| `nexterm-client-gpu/CLAUDE.md`, `docs/plans/ui-ux-modernization-v3.md` | the hover rule and the phase's status | 3 |

---

### Task 1: The tab bar

**Files:**
- Modify: `state/mod.rs` (field beside `hovered_tab_id:215`, initialiser near `:897`, aggregate clause, one test)
- Modify: `renderer/event_handler/mouse.rs:444-458` (retarget beside the existing write)
- Modify: `renderer/ui_verts.rs:168-189` (signature), `:355-370` (the colour branch)
- Modify: `renderer/render_frame.rs` (pass `frame_now` at the `build_tab_bar_verts` call)

**Interfaces:**
- Consumes: `crate::animations::HoverTransition` and `crate::color_util::lerp_rgba`, both from P3b2a.
- Produces: `ClientState.tab_hover: HoverTransition<u32>` (public field); `build_tab_bar_verts` gains `now: std::time::Instant`. **Leave `_animations_cfg` exactly as it is, underscore included** — nothing in this plan makes the builder read it (`retarget` takes the config at the handler, not in the builder), so un-prefixing it would trip `-D warnings`.

- [ ] **Step 1: Write the failing test**

Append to the test module in `state/mod.rs`:

```rust
    /// Hovering a tab must ask for frames until the cross-fade finishes, and
    /// stop afterwards.
    #[test]
    fn a_hovered_tab_wants_animation_frames_until_it_settles() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();

        state.tab_hover.retarget(Some(7), t0, &anim);
        assert!(state.has_active_animation(t0, 200));
        assert!(
            state.tab_hover.weight(7, t0).abs() < 1e-3,
            "the fade starts from nothing"
        );

        let done = t0 + Duration::from_millis(100);
        assert!((state.tab_hover.weight(7, done) - 1.0).abs() < 1e-3);
        assert!(!state.has_active_animation(done, 200));
    }

    /// Leaving the tab bar must fade the last tab out rather than snapping —
    /// the same property `HoverTransition` guarantees for the other models.
    #[test]
    fn leaving_the_tab_bar_fades_the_last_tab_out() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        state.tab_hover.retarget(Some(7), t0, &anim);
        let settled = t0 + Duration::from_millis(100);

        state.tab_hover.retarget(None, settled, &anim);
        let mid = settled + Duration::from_millis(50);
        let w = state.tab_hover.weight(7, mid);
        assert!(w > 0.1 && w < 0.9, "must still be tinted while fading: {w}");
        assert!(state.has_active_animation(mid, 200));

        let done = settled + Duration::from_millis(100);
        assert!(state.tab_hover.weight(7, done).abs() < 1e-3);
        assert!(!state.has_active_animation(done, 200));
    }
```

The spec also requires the config gate to be pinned, so add a third test. It exercises the *handler's* decision, which a state-level test cannot reach — so it tests the property the handler is responsible for, and the report must say so:

```rust
    /// The spec's gate requirement: with `tab_bar.hover_highlight = false`
    /// there is no transition at all, not a transition toward a zero-weight
    /// target, so the config key keeps meaning exactly what it means today.
    ///
    /// The gate itself lives in the pointer-motion handler, which needs an
    /// `EventHandler`, a window and a config to drive. What is checkable here
    /// is the invariant that decision must preserve: a transition never
    /// retargeted stays quiet and weighs nothing.
    #[test]
    fn a_tab_transition_that_is_never_retargeted_stays_quiet() {
        let state = ClientState::new(80, 24, 1000);
        let now = Instant::now();
        assert!(state.tab_hover.weight(7, now).abs() < 1e-4);
        assert!(!state.has_active_animation(now, 200));
        assert!(!state.has_active_animation(now, 0));
    }
```

If you can reach the handler from a test without building new harness scaffolding, prefer that — it would pin the gate directly rather than by proxy. Do not build the harness for it; say in your report which you did and why.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu hovered_tab_wants_animation_frames`
Expected: FAIL — `no field tab_hover`. (The third test compiles only once the field exists too.)

- [ ] **Step 3: Add the state and the plumbing**

`state/mod.rs`, beside `pub hovered_tab_id: Option<u32>`:

```rust
    /// Hover cross-fade over the tab bar (UI/UX v3 P3b2b).
    ///
    /// `hovered_tab_id` above stays the truth for hit-testing and for
    /// whether the tear-out and close buttons are drawn; this is render-only
    /// and outlives it by one fade so the tab the pointer left can dim back
    /// down.
    pub tab_hover: crate::animations::HoverTransition<u32>,
```

plus `tab_hover: Default::default(),` in `ClientState::new`.

`renderer/event_handler/mouse.rs` — inside the existing `if prev_hovered != new_hovered { … }` block, beside `self.app.state.hovered_tab_id = new_hovered;`:

```rust
            // UI/UX v3 P3b2b: the config gate stays a gate. With
            // `hover_highlight = false` there is no transition at all, not a
            // transition toward a zero-weight target, so the key keeps
            // meaning exactly what it means today.
            if self.app.config.tab_bar.hover_highlight {
                let anim = self.app.config.animations.clone();
                self.app.state.tab_hover.retarget(
                    new_hovered,
                    std::time::Instant::now(),
                    &anim,
                );
            }
```

The `clone()` is P3b2a's idiom for the `&config` / `&mut state` borrow collision; use a narrower borrow if one is available without contortion.

`renderer/ui_verts.rs` — add `now: std::time::Instant` to `build_tab_bar_verts`'s parameter list (place it beside `cell_h`, before `font`, so the call site stays readable). **Leave `_animations_cfg` named as it is in this task** — Task 2 is what stops it being unused, and un-prefixing it now would trip `-D warnings`.

`renderer/render_frame.rs` — pass `frame_now` at the call.

- [ ] **Step 4: Animate the colour, and leave the behavioural use alone**

`renderer/ui_verts.rs`, the four-branch colour choice around `:355-370`:

```rust
            //   1. Active -> active_bg
            //   2. Inactive but has activity -> activity_bg (from config)
            //   3. Hovered -> brightened inactive_bg, cross-faded (P3b2b)
            //   4. Normal -> inactive_bg
            let tab_bg = if is_active {
                active_bg
            } else if has_activity {
                activity_bg
            } else if cfg.hover_highlight {
                // The quad is always drawn, so the *colour* moves — alpha
                // scaling would fade the tab out of the bar instead of into
                // its hover tint.
                let hovered_bg = [
                    (inactive_bg[0] + 0.06).min(1.0),
                    (inactive_bg[1] + 0.06).min(1.0),
                    (inactive_bg[2] + 0.08).min(1.0),
                    inactive_bg[3],
                ];
                crate::color_util::lerp_rgba(
                    inactive_bg,
                    hovered_bg,
                    state.tab_hover.weight(pane_id, now),
                )
            } else {
                inactive_bg
            };
```

Note the branch condition changed from `is_hovered && cfg.hover_highlight` to `cfg.hover_highlight` alone: with the weight driving the colour, a non-hovered tab lerps at `w = 0` and yields `inactive_bg` exactly — `lerp_rgba` returns `a` unchanged at `t <= 0.0`. That is why its endpoint early-returns matter here.

**Do not touch `ui_verts.rs:474`.** `if is_hovered && state.tab_drag.is_none() && label_w >= hover_btn_min_width` keeps reading the boolean. The tear-out and close buttons are click targets hit-tested in `mouse.rs`; a button drawn at weight 0.05 is a button the user can hit while it is vanishing. Leave the `is_hovered` local in place for it.

- [ ] **Step 5: Add the aggregate clause**

`state/mod.rs`, in `has_active_animation`:

```rust
        if self.tab_hover.is_active(now) {
            return true;
        }
```

- [ ] **Step 6: Run the suite**

Run: `cargo test -p nexterm-client-gpu`
Expected: PASS, including the two new tests and `a_fully_idle_state_wants_no_animation_frames`.

- [ ] **Step 7: Check the gates and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): cross-fade the tab bar's hover tint"
```

---

### Task 2: The window buttons

Two properties here, and they need **different** mechanisms — the fill is additive, the glyph is an opaque swap. This is also the model with two write sites.

**Files:**
- Modify: `state/mod.rs` (field beside `hovered_window_button:207`, initialiser near `:895`, aggregate clause, one test)
- Modify: `renderer/event_handler/mouse.rs:465-482` (retarget)
- Modify: `renderer/event_handler/mod.rs:505-511` (retarget from the snap-layout event)
- Modify: `renderer/ui_verts.rs:775-810` (the fill and the glyph), and un-prefix `animations_cfg` if it is now used — see Step 4

**Interfaces:**
- Consumes: `HoverTransition`, `lerp_rgba`, and Task 1's `now` parameter on `build_tab_bar_verts` (already threaded).
- Produces: `ClientState.window_button_hover: HoverTransition<WindowButton>` (public field).

- [ ] **Step 1: Write the failing test**

```rust
    /// The window buttons are the fourth and last hover model. Their fade is
    /// driven from two places — the pointer-motion handler and the Windows
    /// snap-layout event — so the state-level property is what the test can
    /// pin; the two call sites are checked by review.
    #[test]
    fn a_hovered_window_button_wants_animation_frames_until_it_settles() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        let close = crate::state::WindowButton::Close;

        state.window_button_hover.retarget(Some(close), t0, &anim);
        assert!(state.has_active_animation(t0, 200));
        assert!(state.window_button_hover.weight(close, t0).abs() < 1e-3);
        assert!(
            state
                .window_button_hover
                .weight(crate::state::WindowButton::Minimize, t0)
                .abs()
                < 1e-4,
            "an unhovered button weighs nothing"
        );

        let done = t0 + Duration::from_millis(100);
        assert!((state.window_button_hover.weight(close, done) - 1.0).abs() < 1e-3);
        assert!(!state.has_active_animation(done, 200));
    }

    /// Moving between two buttons cross-fades them rather than snapping.
    #[test]
    fn moving_between_window_buttons_cross_fades_them() {
        let mut state = ClientState::new(80, 24, 1000);
        let anim = nexterm_config::AnimationsConfig::default();
        let t0 = Instant::now();
        let (min, max) = (
            crate::state::WindowButton::Minimize,
            crate::state::WindowButton::Maximize,
        );

        state.window_button_hover.retarget(Some(min), t0, &anim);
        let settled = t0 + Duration::from_millis(100);
        state.window_button_hover.retarget(Some(max), settled, &anim);

        let mid = settled + Duration::from_millis(50);
        let (w_min, w_max) = (
            state.window_button_hover.weight(min, mid),
            state.window_button_hover.weight(max, mid),
        );
        assert!(w_min > 0.1 && w_min < 0.9, "outgoing mid-fade: {w_min}");
        assert!(w_max > 0.1 && w_max < 0.9, "incoming mid-fade: {w_max}");
    }
```

Adjust the `WindowButton` import path to whatever the crate exposes — `state/mod.rs`'s own test module may already have it in scope.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu hovered_window_button_wants_animation_frames`
Expected: FAIL — `no field window_button_hover`.

- [ ] **Step 3: Add the state and retarget it from BOTH sites**

`state/mod.rs`, beside `pub hovered_window_button: Option<WindowButton>`:

```rust
    /// Hover cross-fade over the custom title bar's window buttons
    /// (UI/UX v3 P3b2b). `hovered_window_button` above stays the truth for
    /// hit-testing; this is render-only.
    pub window_button_hover: crate::animations::HoverTransition<WindowButton>,
```

plus the initialiser.

`renderer/event_handler/mouse.rs`, inside the existing `if prev_button != new_button { … }`:

```rust
            let anim = self.app.config.animations.clone();
            self.app.state.window_button_hover.retarget(
                new_button,
                std::time::Instant::now(),
                &anim,
            );
```

`renderer/event_handler/mod.rs`, in the `UserEvent::SnapMaximizeHover` arm, beside `self.app.state.hovered_window_button = new_hover;` — pass the same `new_hover`. This is the Windows snap-layout path; missing it would leave that route snapping while the mouse route fades, with nothing failing to compile.

Then run the mandatory grep and confirm you found no third site:

```
grep -rn "hovered_window_button\s*=\|hovered_tab_id\s*=" nexterm-client-gpu/src
```

- [ ] **Step 4: Animate both properties, with the right mechanism each**

`renderer/ui_verts.rs`, around `:775-810`:

```rust
                let w = state.window_button_hover.weight(button, now);
                // The fill is an additive layer — absent when not hovered —
                // so its alpha carries the fade and nothing is emitted at 0.
                if w > 0.0 {
                    let bg = if button == WindowButton::Close {
                        tokens.semantic_error
                    } else {
                        [
                            (inactive_bg[0] + 0.08).min(1.0),
                            (inactive_bg[1] + 0.08).min(1.0),
                            (inactive_bg[2] + 0.10).min(1.0),
                            1.0,
                        ]
                    };
                    add_px_rounded_rect_sdf(
                        bx,
                        bar_y,
                        window_button_w,
                        bar_h,
                        radius,
                        [bg[0], bg[1], bg[2], bg[3] * w],
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                // The glyph is an opaque swap between two tokens, so the
                // colour itself moves.
                let fg = crate::color_util::lerp_rgba(
                    tokens.text_secondary,
                    tokens.text_primary,
                    w,
                );
```

The `let hovered = state.hovered_window_button == Some(button);` local at `:778` has no reader left once both uses take the weight — delete it, and check nothing else in the block used it.

If `animations_cfg` is still unused after this task, leave its underscore; if some site now needs it, un-prefix it. It is more likely to stay unused: `retarget` takes the config at the *handler*, not in the builder.

- [ ] **Step 5: Add the aggregate clause**

```rust
        if self.window_button_hover.is_active(now) {
            return true;
        }
```

- [ ] **Step 6: Run the suite**

Run: `cargo test --workspace`
Expected: PASS. Confirm `a_fully_idle_state_wants_no_animation_frames` is still green — it now has four more clauses than at the start of P3b2.

- [ ] **Step 7: Check the gates and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): cross-fade the window buttons' hover fill and glyph"
```

---

### Task 3: Resolve `target()`, and record what the phase learned

**Files:**
- Modify: `animations/hover.rs` (`target()` and its `#[allow(dead_code)]`)
- Modify: `nexterm-client-gpu/CLAUDE.md` (the hover rule)
- Modify: `docs/plans/ui-ux-modernization-v3.md` (status and the verification backlog)

**Interfaces:** none produced; this task consumes the finished state of Tasks 1 and 2.

- [ ] **Step 1: Decide `target()` by measuring**

P3b2a's whole-branch review deferred this with an explicit instruction: `HoverTransition::target()` has no production caller and carries a method-level `#[allow(dead_code)]`; revisit once the tab bar and window buttons land, and delete it if neither needs it either.

Run: `grep -rn "\.target()" nexterm-client-gpu/src`

- **If the only hits are in `#[cfg(test)]` code:** delete `target()` and its `#[allow(dead_code)]`. Then fix the tests that used it — they should assert the observable consequence (a weight) rather than the internal target. If a test becomes unable to express what it was checking, **stop and report** rather than weakening it; that would mean `target()` is load-bearing for testing and should stay with a comment saying so.
- **If a production caller appeared:** delete only the `#[allow(dead_code)]` and note in your report which site consumes it.

Either way, say in your report which branch you took and what the grep showed.

- [ ] **Step 2: Run the suite and the gates**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check`

- [ ] **Step 3: Extend the crate guide**

`nexterm-client-gpu/CLAUDE.md` already carries the surface rule from P3b1 ("Adding an overlay surface"). Add the hover rule beside it:

```markdown
- **Adding a hover model**: give it its own `HoverTransition<Id>` on the
  state that owns the hovered id, retarget it from the same handler that
  writes that id (**every** such handler — the window buttons have two, one
  of them the Windows snap-layout event), and add an `is_active` clause to
  `has_active_animation`. Then pick the interpolation by the shape of the
  hovered appearance, not by habit: an **additive** layer (a fill that is
  absent when not hovered) scales its alpha by the weight and emits no
  geometry at 0; a colour that is **always drawn** in one of several variants
  lerps with `color_util::lerp_rgba`. `apply_surface_fade` is for whole
  surfaces opening and closing and does not apply to hover. Finally, check
  whether the hovered flag has a *behavioural* reader as well as a colour
  one — the tab bar's tear-out button is gated on hover and must keep the
  boolean, because a button drawn at weight 0.05 is still clickable.
```

- [ ] **Step 4: Update the phase plan**

In `docs/plans/ui-ux-modernization-v3.md`, mark P3b2 complete (both halves) and add to the on-device verification backlog the two items only hardware can settle:

- Whether the tab bar's `+0.06/+0.06/+0.08` brightening is perceptible *as a fade* at all, or whether the delta is too small for the transition to register. The design flags this as the open question of P3b2 and assumes "leave it"; if it proves invisible, the options are to increase the delta (a visual change beyond motion) or to drop the tab model.
- Whether the Close button **fading** to `semantic_error` rather than snapping to it weakens the "this is destructive" signal. This is the only place in P3b2 where the hovered appearance is a warning rather than an affordance.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src nexterm-client-gpu/CLAUDE.md docs/plans/ui-ux-modernization-v3.md
git commit -m "docs(client): record the hover-model rule, and resolve HoverTransition::target"
```

---

## Closing out P3b2b

- [ ] Open the PR against `master` with an English title and body. Suggested title: `feat(client): hover cross-fade for the tab bar and window buttons (UI/UX v3 P3b2b)`.
- [ ] In the PR body, name both traps explicitly — the tear-out button keeping the boolean, and the snap-layout write site — since both are invisible in the diff's shape and are exactly what a reviewer should confirm.
- [ ] State plainly what was not verified: no motion in P3b2 has been seen on hardware, and the tab-brightening perceptibility question is open.
- [ ] Confirm CI is green before merging, including the flatpak job (no `Cargo.lock` change is expected, so it should diff clean).
- [ ] **P3b2 is then complete.** The remaining sub-phase is P3b3 (the `pressed` widget state), which the P3b design notes is behaviour rather than motion — it adds a state that does not exist, touches the hit-test and AccessKit paths, and should land in its own PR.
