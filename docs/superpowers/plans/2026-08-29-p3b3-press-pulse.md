# P3b3 Press Pulse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every pointer-clickable chrome control a 100 ms press pulse — the fill strengthens and dims at the instant of the press, then decays — across all four models P3b2 closed for hover.

**Architecture:** One shared `PressPulse<Id>` (a single `Timed`, weight `1 → 0`) sits beside `HoverTransition<Id>` in `animations/`. Each of the four models owns an instance next to its hover transition, fires it from the handler that already commits the click, and passes the weight to its existing draw site. Draw sites change by two lines: they take `hover.max(press)` as the fill weight, then run the fill through one shared `press_fill()` helper.

**Tech Stack:** Rust, wgpu/cosmic-text renderer, `nexterm-config` design tokens. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-29-p3b3-press-pulse-design.md`

## Global Constraints

- Base branch is `p3b3-press-pulse`, which sits on top of P3b2b (PR #81). Do not rebase or force-push it.
- Comments and doc-strings in **English**. Conversation and commit messages in Japanese.
- No `unwrap()`. Use `?` or `expect("reason")`.
- `cargo clippy -- -D warnings` and `cargo fmt --check` must pass before every commit.
- `cargo test -p nexterm-client-gpu` must stay green (1013 tests at the base commit).
- Do not touch `nexterm-i18n` (no new user-facing strings), `accessibility.rs` (press is render-only, exactly as P3b1's `closing` field was), or `settings_panel_hit.rs` geometry.
- `duration::FASTER = 100` and `Curve::EasyEase` are the fixed timing for every pulse.
- Two shared constants, both in `color_util.rs`: `PRESS_DIM = 0.85`, `PRESS_ALPHA_BOOST = 2.3` (raised from 1.7 by measurement: at 1.7 the pulse was imperceptible on eight of the nine builtin schemes).

---

### Task 1: `PressPulse<Id>`

**Files:**
- Create: `nexterm-client-gpu/src/animations/press.rs`
- Modify: `nexterm-client-gpu/src/animations/mod.rs` (module declaration + re-export, beside `hover`)
- Test: `nexterm-client-gpu/src/animations/press.rs` (inline `#[cfg(test)] mod tests`, matching `hover.rs`)

**Interfaces:**
- Consumes: `super::{Curve, Timed, duration}`, `nexterm_config::AnimationsConfig`.
- Produces: `crate::animations::PressPulse<Id>` with
  `fn press(&mut self, id: Id, now: Instant, anim: &AnimationsConfig)`,
  `fn weight(&self, id: Id, now: Instant) -> f32`,
  `fn is_active(&self, now: Instant) -> bool`, and `Default`.
  Tasks 3–6 use exactly these three methods.

- [ ] **Step 1: Write the failing tests**

Create `nexterm-client-gpu/src/animations/press.rs` containing only the test module for now:

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

    /// 100 ms is `duration::FASTER`, the constant every press site uses.
    const MS: u64 = 100;

    #[test]
    fn a_fresh_pulse_weighs_nothing() {
        let p: PressPulse<u32> = PressPulse::default();
        let now = Instant::now();
        assert!(p.weight(1, now).abs() < 1e-4);
        assert!(!p.is_active(now));
    }

    #[test]
    fn a_press_starts_full_and_decays_to_nothing() {
        let mut p: PressPulse<u32> = PressPulse::default();
        let t0 = Instant::now();
        p.press(7, t0, &on());
        assert!((p.weight(7, t0) - 1.0).abs() < 1e-3);
        assert!(p.is_active(t0));
        assert!(p.weight(8, t0).abs() < 1e-4, "another id weighs nothing");
        let done = t0 + Duration::from_millis(MS);
        assert!(p.weight(7, done).abs() < 1e-3);
        assert!(!p.is_active(done));
    }

    /// One slot: pressing a second control drops the first immediately.
    /// Unlike hover there is no hand-off to preserve — the user's attention
    /// has moved, and the abandoned pulse must not keep requesting frames.
    #[test]
    fn a_second_press_replaces_the_first() {
        let mut p: PressPulse<u32> = PressPulse::default();
        let t0 = Instant::now();
        p.press(1, t0, &on());
        let mid = t0 + Duration::from_millis(50);
        assert!(p.weight(1, mid) > 0.1, "first pulse is mid-decay");
        p.press(2, mid, &on());
        assert!(p.weight(1, mid).abs() < 1e-4);
        assert!((p.weight(2, mid) - 1.0).abs() < 1e-3);
    }

    /// Deliberately NOT idempotent, unlike `HoverTransition::retarget`: the
    /// caller fires once per click, not once per pointer-motion frame, so a
    /// double-click must pulse twice rather than continue the first decay.
    #[test]
    fn pressing_the_same_id_again_restarts_the_decay() {
        let mut p: PressPulse<u32> = PressPulse::default();
        let t0 = Instant::now();
        p.press(3, t0, &on());
        let mid = t0 + Duration::from_millis(80);
        assert!(p.weight(3, mid) < 0.5, "decayed most of the way");
        p.press(3, mid, &on());
        assert!((p.weight(3, mid) - 1.0).abs() < 1e-3, "restarted at full");
    }

    /// The config gate. With animations off `scaled_duration_ms` returns 0,
    /// so the pulse is already finished on the frame it is fired and no site
    /// ever renders a pressed appearance.
    #[test]
    fn disabled_animations_never_show_a_pulse() {
        let mut p: PressPulse<u32> = PressPulse::default();
        let t0 = Instant::now();
        p.press(5, t0, &off());
        assert!(p.weight(5, t0).abs() < 1e-4);
        assert!(!p.is_active(t0));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu --lib animations::press`
Expected: compile error — `cannot find type PressPulse in this scope` (the module is not declared yet either).

- [ ] **Step 3: Write the implementation**

Prepend to `nexterm-client-gpu/src/animations/press.rs`:

```rust
//! One-shot press feedback (UI/UX v3 P3b3).
//!
//! Three of this client's four pointer models commit their action on
//! mouse-*down*: by the frame the button comes up the tab has switched, the
//! panel row has toggled, or the window is gone. A held "pressed" state has
//! no window to live in, so press is a pulse — full weight at the press
//! instant, zero 100 ms later, independent of the button ever coming up.
//!
//! One `Timed`, where `HoverTransition` needs two. That type's second timer
//! exists so a hand-off decays the outgoing item from the weight it actually
//! held; a press has no hand-off. Pressing a second control simply replaces
//! the first, which is correct: the abandoned control is no longer where the
//! user is looking.

use std::time::Instant;

use nexterm_config::AnimationsConfig;

use super::{Curve, Timed, duration};

/// A decaying press highlight for at most one item of one model.
#[derive(Debug, Clone, Copy)]
pub struct PressPulse<Id> {
    /// The item that was pressed, if any.
    id: Option<Id>,
    /// Runs 0 → 1 while the pulse decays 1 → 0.
    anim: Timed,
}

impl<Id> Default for PressPulse<Id> {
    fn default() -> Self {
        // Born finished. With `id` at `None` every id weighs 0 regardless,
        // so the arbitrary start instant is never read (`Timed` at a zero
        // duration short-circuits before touching it).
        Self {
            id: None,
            anim: Timed::new(Instant::now(), 0, Curve::EasyEase),
        }
    }
}

impl<Id: Copy + PartialEq> PressPulse<Id> {
    /// Fire a pulse on `id`, replacing whatever was decaying before.
    pub fn press(&mut self, id: Id, now: Instant, anim: &AnimationsConfig) {
        self.id = Some(id);
        self.anim = Timed::new(
            now,
            anim.scaled_duration_ms(duration::FASTER),
            Curve::EasyEase,
        );
    }

    /// Press weight for `id` in `[0, 1]`: 1 at the press instant, 0 once the
    /// pulse has run out.
    pub fn weight(&self, id: Id, now: Instant) -> f32 {
        if self.id == Some(id) {
            1.0 - self.anim.progress(now)
        } else {
            0.0
        }
    }

    /// Whether another frame is needed.
    pub fn is_active(&self, now: Instant) -> bool {
        self.id.is_some() && !self.anim.is_done(now)
    }
}
```

In `nexterm-client-gpu/src/animations/mod.rs`, add the module beside `hover` and re-export it beside `HoverTransition`:

```rust
mod press;
```
```rust
pub use press::PressPulse;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu --lib animations::press`
Expected: 5 passed.

Then: `cargo clippy -p nexterm-client-gpu -- -D warnings` and `cargo fmt --check`.
Expected: clean. If clippy reports `PressPulse` as never constructed outside tests, **do not** add `#[allow(dead_code)]` — Task 3 is the first production caller and the warning disappears there. If the build must be clean at this commit, keep the export and land Tasks 1 and 3 together rather than silencing it.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src/animations/press.rs nexterm-client-gpu/src/animations/mod.rs
git commit -m "feat(client): add the shared PressPulse one-shot animation

押下即発火という既存のクリック確定タイミングに合わせ、保持型ではなく
一発減衰の press 表現を共有型として追加する。HoverTransition と違い
二段タイマーは不要で、押下ごとに作り直す（非冪等）。"
```

---

### Task 2: The shared `press_fill()` composition helper

**Files:**
- Modify: `nexterm-client-gpu/src/color_util.rs` (add two constants and one function; tests in the existing inline test module)
- Test: `nexterm-client-gpu/src/color_util.rs`

**Interfaces:**
- Consumes: existing `apply_hsb_animated_rgba`, `composite_over`, `relative_luminance` in the same file.
- Produces: `pub(crate) fn press_fill(color: [f32; 4], press: f32) -> [f32; 4]`. Tasks 3–6 call exactly this, as the last step of building their fill colour.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `nexterm-client-gpu/src/color_util.rs`:

```rust
#[test]
fn press_fill_is_identity_at_zero_weight() {
    let c = [0.4, 0.5, 0.6, 0.35];
    let out = press_fill(c, 0.0);
    for i in 0..4 {
        assert!((out[i] - c[i]).abs() < 1e-6, "channel {i} moved at weight 0");
    }
}

#[test]
fn press_fill_darkens_and_strengthens_at_full_weight() {
    let c = [0.4, 0.5, 0.6, 0.35];
    let out = press_fill(c, 1.0);
    assert!(
        relative_luminance([out[0], out[1], out[2]]) < relative_luminance([c[0], c[1], c[2]]),
        "pressed fill must be darker"
    );
    assert!(out[3] > c[3], "pressed fill must be stronger");
    assert!(out[3] <= 1.0, "alpha must stay in range");
}

#[test]
fn press_fill_never_pushes_an_opaque_fill_past_one() {
    // The tab's hover is an opaque lerp; the boost must clamp rather than
    // produce an out-of-range alpha the shader would clip unpredictably.
    let out = press_fill([0.2, 0.2, 0.25, 1.0], 1.0);
    assert!((out[3] - 1.0).abs() < 1e-6);
}

/// The design's open question, pinned as a gate: on every builtin scheme a
/// pressed control must be visibly different from a merely hovered one.
/// Modelled on the settings row, the weakest of the four sites (a
/// `surface_3` layer at `HOVER_ALPHA` over `surface_1`).
#[test]
fn press_is_perceptible_on_every_builtin_scheme() {
    use nexterm_config::BuiltinScheme;
    const SCHEMES: [BuiltinScheme; 9] = [
        BuiltinScheme::Dark,
        BuiltinScheme::Light,
        BuiltinScheme::TokyoNight,
        BuiltinScheme::Solarized,
        BuiltinScheme::Gruvbox,
        BuiltinScheme::Catppuccin,
        BuiltinScheme::Dracula,
        BuiltinScheme::Nord,
        BuiltinScheme::OneDark,
    ];
    for scheme in SCHEMES {
        let tokens = nexterm_config::DesignTokens::from_palette(&scheme.palette());
        let bg = [
            tokens.surface_1[0],
            tokens.surface_1[1],
            tokens.surface_1[2],
        ];
        let s = tokens.surface_3;
        let hovered = [s[0], s[1], s[2], s[3] * 0.35];
        let rest = relative_luminance(composite_over(hovered, bg));
        let pressed = relative_luminance(composite_over(press_fill(hovered, 1.0), bg));
        assert!(
            (rest - pressed).abs() > 0.004,
            "{scheme:?}: pressed is indistinguishable from hovered (Δ luminance {})",
            (rest - pressed).abs()
        );
    }
}
```

```rust
/// Press must not cost legibility. Deliberately a *relative* check, not a
/// flat 4.5:1 assertion: Solarized and OneDark already have contrast defects
/// in their resting chrome (tracked for P5), and a flat assertion here would
/// fail on those pre-existing defects rather than on anything P3b3 does.
#[test]
fn press_never_worsens_text_contrast_on_any_builtin_scheme() {
    use nexterm_config::BuiltinScheme;
    const SCHEMES: [BuiltinScheme; 9] = [
        BuiltinScheme::Dark,
        BuiltinScheme::Light,
        BuiltinScheme::TokyoNight,
        BuiltinScheme::Solarized,
        BuiltinScheme::Gruvbox,
        BuiltinScheme::Catppuccin,
        BuiltinScheme::Dracula,
        BuiltinScheme::Nord,
        BuiltinScheme::OneDark,
    ];
    for scheme in SCHEMES {
        let tokens = nexterm_config::DesignTokens::from_palette(&scheme.palette());
        let bg = [
            tokens.surface_1[0],
            tokens.surface_1[1],
            tokens.surface_1[2],
        ];
        let s = tokens.surface_3;
        let hovered = [s[0], s[1], s[2], s[3] * 0.35];
        let fg = [
            tokens.text_primary[0],
            tokens.text_primary[1],
            tokens.text_primary[2],
        ];
        let before = contrast_ratio(fg, composite_over(hovered, bg));
        let after = contrast_ratio(fg, composite_over(press_fill(hovered, 1.0), bg));
        assert!(
            after >= before * 0.95,
            "{scheme:?}: press cut text contrast from {before} to {after}"
        );
    }
}
```

If the perceptibility test fails for a scheme, **raise `PRESS_ALPHA_BOOST`** (it moves dark schemes, where the brightness multiplier has almost no absolute room) rather than lowering the threshold. Record the final value and the reason in the commit message.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu --lib color_util`
Expected: compile error — `cannot find function press_fill in this scope`.

- [ ] **Step 3: Write the implementation**

Add to `nexterm-client-gpu/src/color_util.rs`, next to the other colour helpers:

```rust
/// Brightness multiplier applied to a control's fill at full press weight
/// (UI/UX v3 P3b3). Fluent's subtle-button ramp puts pressed below hover;
/// this is that step, expressed as an HSV `v` multiplier so it follows every
/// scheme without a per-scheme table.
pub(crate) const PRESS_DIM: f32 = 0.85;

/// Alpha multiplier applied to a hover layer at full press weight.
///
/// The dim alone is not enough. Three of the four press sites draw their
/// hover as an additive layer, and on a scheme whose chrome is already
/// near-black the brightness step has almost no absolute room — pressed
/// would read as identical to hovered. Strengthening the layer moves on
/// every scheme.
pub(crate) const PRESS_ALPHA_BOOST: f32 = 1.7;

/// Apply press feedback to a control fill already composed for `hover.max(press)`.
///
/// At `press == 0` this returns `color` unchanged, so a site that is never
/// pressed keeps its exact P3b2 appearance.
pub(crate) fn press_fill(color: [f32; 4], press: f32) -> [f32; 4] {
    let press = press.clamp(0.0, 1.0);
    let dimmed = apply_hsb_animated_rgba(color, 1.0, 1.0, PRESS_DIM, press);
    let alpha = (color[3] * (1.0 + press * (PRESS_ALPHA_BOOST - 1.0))).min(1.0);
    [dimmed[0], dimmed[1], dimmed[2], alpha]
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu --lib color_util`
Expected: all pass, including the nine-scheme gate.

Then: `cargo clippy -p nexterm-client-gpu -- -D warnings` and `cargo fmt --check`.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src/color_util.rs
git commit -m "feat(client): add press_fill, the shared press composition

押下時の見た目を 1 箇所に集約する。明度を下げるだけでは暗いスキームで
知覚できないため、加算レイヤの alpha も同時に強める。9 スキーム全部で
hover と press が区別できることをテストで固定する。"
```

---

### Task 3: Tab bar

**Files:**
- Modify: `nexterm-client-gpu/src/state/mod.rs` (field beside `tab_hover` around line 226; initialiser in `ClientState::new`; one clause in `has_active_animation` around line 677)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/mouse.rs` (the tab-click branch of `on_mouse_left_pressed`, at `if let Some(pane_id) = hit_pane {`, around line 1385)
- Modify: `nexterm-client-gpu/src/renderer/ui_verts.rs` (the tab background, around line 374)
- Test: `nexterm-client-gpu/src/state/mod.rs` (inline tests, beside the existing `tab_hover` ones around line 1330)

**Interfaces:**
- Consumes: `crate::animations::PressPulse` (Task 1), `crate::color_util::press_fill` (Task 2).
- Produces: `ClientState.tab_press: crate::animations::PressPulse<u32>` (the id is a pane id). Nothing later depends on it.

- [ ] **Step 1: Write the failing test**

Add to the inline test module of `nexterm-client-gpu/src/state/mod.rs`:

```rust
/// The wiring's own contract: a decaying pulse must keep the frame loop
/// awake, or the tab would freeze mid-press until some other event
/// happened to request a redraw. The pulse's own timing is covered by
/// `animations::press`; this pins that `ClientState` consults it.
#[test]
fn a_tab_press_keeps_the_frame_loop_awake() {
    let mut state = ClientState::new(80, 24, 1000);
    let t0 = Instant::now();
    assert!(!state.has_active_animation(t0, 200));
    state
        .tab_press
        .press(7, t0, &nexterm_config::AnimationsConfig::default());
    assert!(state.has_active_animation(t0, 200));
    let done = t0 + Duration::from_millis(100);
    assert!(!state.has_active_animation(done, 200));
}
```

`ClientState::new(80, 24, 1000)` and the `200` are what the neighbouring
animation tests in this module already use.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu --lib a_tab_press_keeps_the_frame_loop_awake`
Expected: compile error — `no field tab_press on type ClientState`.

- [ ] **Step 3: Write the implementation**

In `nexterm-client-gpu/src/state/mod.rs`, directly below the `tab_hover` field:

```rust
    /// Press pulse for the tab bar (UI/UX v3 P3b3). A tab click commits on
    /// mouse-down, so this decays on its own rather than waiting for the
    /// button to come up.
    pub tab_press: crate::animations::PressPulse<u32>,
```

Add `tab_press: Default::default(),` to the `ClientState::new` initialiser, beside `tab_hover`.

In `has_active_animation`, beside the existing hover clause:

```rust
        if self.tab_press.is_active(now) {
            return true;
        }
```

In `nexterm-client-gpu/src/renderer/event_handler/mouse.rs`, inside `if let Some(pane_id) = hit_pane {`, immediately after `let now = Instant::now();`:

```rust
                        // UI/UX v3 P3b3: pulse before the branch below decides
                        // between focus-switch and rename — both are presses
                        // and both deserve the feedback.
                        self.app
                            .state
                            .tab_press
                            .press(pane_id, now, &self.app.config.animations);
```

In `nexterm-client-gpu/src/renderer/ui_verts.rs`, replace the tab background computation:

```rust
                crate::color_util::lerp_rgba(
                    inactive_bg,
                    hovered_bg,
                    state.tab_hover.weight(pane_id, now),
                )
```

with:

```rust
                // UI/UX v3 P3b3: press raises the weight before dimming it —
                // a click that lands inside the hover fade's first 100 ms
                // would otherwise have almost no fill to dim.
                let press = state.tab_press.weight(pane_id, now);
                let w = state.tab_hover.weight(pane_id, now).max(press);
                crate::color_util::press_fill(
                    crate::color_util::lerp_rgba(inactive_bg, hovered_bg, w),
                    press,
                )
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu --lib` then `cargo clippy -p nexterm-client-gpu -- -D warnings` and `cargo fmt --check`.
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src/state/mod.rs nexterm-client-gpu/src/renderer/event_handler/mouse.rs nexterm-client-gpu/src/renderer/ui_verts.rs
git commit -m "feat(client): pulse the tab under the pointer on press

タブのクリックは押下で確定するため、押下時点で pulse を発火し、
描画側は hover と press の大きい方で fill を作ってから press_fill に通す。"
```

---

### Task 4: Window buttons

**Files:**
- Modify: `nexterm-client-gpu/src/state/mod.rs` (field beside `window_button_hover` around line 211; initialiser; one clause in `has_active_animation`)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/mouse.rs` (the `hit_minimize` / `hit_maximize` / `hit_close` chain in `on_mouse_left_pressed`, around line 1247)
- Modify: `nexterm-client-gpu/src/renderer/ui_verts.rs` (the window-button loop, around line 787)
- Test: `nexterm-client-gpu/src/state/mod.rs`

**Interfaces:**
- Consumes: `crate::animations::PressPulse`, `crate::color_util::press_fill`, `crate::state::WindowButton`.
- Produces: `ClientState.window_button_press: crate::animations::PressPulse<WindowButton>`.

- [ ] **Step 1: Write the failing test**

Add to the inline test module of `nexterm-client-gpu/src/state/mod.rs`:

```rust
/// Maximize is the only one of the three whose pulse is ever seen —
/// Minimize and Close remove the window first — but all three are wired,
/// so all three must keep the frame loop awake while they decay.
#[test]
fn a_window_button_press_keeps_the_frame_loop_awake() {
    let mut state = ClientState::new(80, 24, 1000);
    let t0 = Instant::now();
    state.window_button_press.press(
        crate::state::WindowButton::Maximize,
        t0,
        &nexterm_config::AnimationsConfig::default(),
    );
    assert!(state.has_active_animation(t0, 200));
    assert!(!state.has_active_animation(t0 + Duration::from_millis(100), 200));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu --lib a_window_button_press_keeps_the_frame_loop_awake`
Expected: compile error — `no field window_button_press on type ClientState`.

- [ ] **Step 3: Write the implementation**

In `nexterm-client-gpu/src/state/mod.rs`, below `window_button_hover`:

```rust
    /// Press pulse for the custom title bar's window buttons (UI/UX v3 P3b3).
    ///
    /// Wired for all three even though Minimize and Close tear the window
    /// down before the pulse can be seen: excluding them would leave an
    /// untestable exception in the press chain for no visible gain.
    pub window_button_press: crate::animations::PressPulse<WindowButton>,
```

Add `window_button_press: Default::default(),` to `ClientState::new`, and to `has_active_animation`:

```rust
        if self.window_button_press.is_active(now) {
            return true;
        }
```

In `mouse.rs`, inside the tab-bar branch of `on_mouse_left_pressed`, immediately before `if hit_minimize {`:

```rust
                // UI/UX v3 P3b3: one pulse for whichever button was hit.
                let pressed_button = if hit_minimize {
                    Some(crate::state::WindowButton::Minimize)
                } else if hit_maximize {
                    Some(crate::state::WindowButton::Maximize)
                } else if hit_close {
                    Some(crate::state::WindowButton::Close)
                } else {
                    None
                };
                if let Some(button) = pressed_button {
                    let now = std::time::Instant::now();
                    self.app
                        .state
                        .window_button_press
                        .press(button, now, &self.app.config.animations);
                }
```

In `ui_verts.rs`, replace the weight read and the fill emission in the window-button loop:

```rust
                let w = state.window_button_hover.weight(button, now);
```

with:

```rust
                // UI/UX v3 P3b3: the fill is additive, so press has to raise
                // the weight before `press_fill` dims and strengthens it.
                let press = state.window_button_press.weight(button, now);
                let w = state.window_button_hover.weight(button, now).max(press);
```

and change the colour passed to `add_px_rounded_rect_sdf` from

```rust
                        [bg[0], bg[1], bg[2], bg[3] * w],
```

to

```rust
                        crate::color_util::press_fill([bg[0], bg[1], bg[2], bg[3] * w], press),
```

Leave the glyph colour swap below it untouched — the spec keeps foregrounds out of press.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu --lib`, then clippy and fmt as in Task 3.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src/state/mod.rs nexterm-client-gpu/src/renderer/event_handler/mouse.rs nexterm-client-gpu/src/renderer/ui_verts.rs
git commit -m "feat(client): pulse the window button on press

Minimize と Close は押下直後にウィンドウが消えるため実際には見えないが、
3 つとも配線して press 連鎖に例外を作らない。"
```

---

### Task 5: Settings panel rows

**Files:**
- Modify: `nexterm-client-gpu/src/settings/mod.rs` (field beside `hover_transition`, around line 108)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/settings_panel_hit.rs` (extract the hit → `WidgetId` mapping)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/mouse.rs` (use the extracted mapping in `on_cursor_moved` around line 379, and call it in `on_mouse_left_pressed` around line 917)
- Modify: `nexterm-client-gpu/src/renderer/overlay/widgets/draw/mod.rs` (`WidgetTheme` gains `press`; `draw_row_background`; test support)
- Modify: all 10 `WidgetTheme { .. }` construction sites: `renderer/overlay/settings/{blocks,font,keybindings,profiles,security,startup,ssh,window}_tab.rs` and `theme_tab.rs` (two sites, lines 51 and 168)
- Modify: `nexterm-client-gpu/src/renderer/overlay/settings/mod.rs` (the panel's own active-animation aggregate)
- Test: `nexterm-client-gpu/src/renderer/overlay/widgets/draw/mod.rs`

**Interfaces:**
- Consumes: `crate::animations::PressPulse`, `crate::color_util::press_fill`, `crate::renderer::overlay::widgets::spec::WidgetId`.
- Produces: `SettingsPanel.press_pulse: PressPulse<WidgetId>`; `WidgetTheme.press: &'a PressPulse<WidgetId>`; `pub(super) fn widget_id_of(hit: &SettingsPanelHit) -> Option<WidgetId>`.

- [ ] **Step 1: Write the failing test**

This is the highest-value test in the plan: it pins the `max` amendment, the one thing that makes press visible on a row the pointer has not finished hovering.

Add to the test module in `nexterm-client-gpu/src/renderer/overlay/widgets/draw/mod.rs`:

```rust
/// A press with no hover at all must still paint. The hover fill is an
/// additive layer gated on `w > 0.0`, so without press raising the weight
/// a click landing inside the hover fade's first frames would emit no
/// vertices and show nothing.
#[test]
fn a_pressed_row_paints_even_with_no_hover() {
    let spec = spec_at(WidgetKind::Toggle { on: false });
    let hover: HoverTransition<WidgetId> = Default::default();
    let mut press: PressPulse<WidgetId> = Default::default();
    let now = Instant::now();
    press.press(
        spec.id(),
        now,
        &nexterm_config::AnimationsConfig::default(),
    );
    let verts = bg_vertices_with_states(&hover, &press, now, |t, s| {
        draw_row_background(&spec, t, s)
    });
    assert_eq!(verts.len() / 4, 1);
}

/// And a press on an already-hovered row must look different from hover
/// alone, or the feedback is invisible in the common case.
#[test]
fn a_pressed_row_differs_from_a_merely_hovered_one() {
    let spec = spec_at(WidgetKind::Toggle { on: false });
    let mut hover: HoverTransition<WidgetId> = Default::default();
    let cfg = nexterm_config::AnimationsConfig::default();
    let now = Instant::now();
    hover.retarget(Some(spec.id()), now, &cfg);
    let settled = now + Duration::from_millis(100);

    let idle: PressPulse<WidgetId> = Default::default();
    let hovered = bg_vertices_with_states(&hover, &idle, settled, |t, s| {
        draw_row_background(&spec, t, s)
    });

    let mut press: PressPulse<WidgetId> = Default::default();
    press.press(spec.id(), settled, &cfg);
    let pressed = bg_vertices_with_states(&hover, &press, settled, |t, s| {
        draw_row_background(&spec, t, s)
    });

    assert_eq!(hovered.len(), pressed.len());
    assert!(
        hovered[0].color != pressed[0].color,
        "pressed fill must differ from the hovered fill"
    );
}
```

If `BgVertex`'s colour field is not named `color`, use whatever the neighbouring tests read; the assertion is "the first vertex's colour differs".

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu --lib widgets::draw`
Expected: compile error — `cannot find function bg_vertices_with_states`.

- [ ] **Step 3: Write the implementation**

**(a)** In `nexterm-client-gpu/src/settings/mod.rs`, below `hover_transition`:

```rust
    /// Press pulse for this panel's widget rows (UI/UX v3 P3b3). A row's
    /// action commits on mouse-down, so the pulse decays on its own.
    pub press_pulse: crate::animations::PressPulse<
        crate::renderer::overlay::widgets::spec::WidgetId,
    >,
```

Add `press_pulse: Default::default(),` wherever the panel's other animation fields are initialised.

**(b)** In `renderer/event_handler/settings_panel_hit.rs`, add the mapping that `on_cursor_moved` currently inlines, so the press handler cannot drift from it:

```rust
/// Map a panel hit to the widget it addresses (UI/UX v3 P3b3).
///
/// Extracted from `on_cursor_moved`, which needs the same mapping for the
/// hover cross-fade. Two copies would drift the moment a category is added.
pub(super) fn widget_id_of(hit: &SettingsPanelHit) -> Option<crate::renderer::overlay::widgets::spec::WidgetId> {
    use crate::renderer::overlay::widgets::settings_blocks::BLOCKS_CATEGORY;
    use crate::renderer::overlay::widgets::settings_font::FONT_CATEGORY;
    use crate::renderer::overlay::widgets::settings_keybindings::KEYBINDINGS_CATEGORY;
    use crate::renderer::overlay::widgets::settings_profiles::PROFILES_CATEGORY;
    use crate::renderer::overlay::widgets::settings_security::SECURITY_CATEGORY;
    use crate::renderer::overlay::widgets::settings_ssh::SSH_CATEGORY;
    use crate::renderer::overlay::widgets::settings_startup::STARTUP_CATEGORY;
    use crate::renderer::overlay::widgets::settings_theme::{THEME_CATEGORY, THEME_SWATCH_BASE};
    use crate::renderer::overlay::widgets::settings_window::WINDOW_CATEGORY;
    use crate::renderer::overlay::widgets::spec::WidgetId;
    let (category, index) = match hit {
        SettingsPanelHit::ThemeColor(i) => (THEME_CATEGORY, THEME_SWATCH_BASE + *i as u16),
        SettingsPanelHit::ThemeRow(index) => (THEME_CATEGORY, *index),
        SettingsPanelHit::WindowRow(index) => (WINDOW_CATEGORY, *index),
        SettingsPanelHit::FontRow(index) => (FONT_CATEGORY, *index),
        SettingsPanelHit::StartupRow(index) => (STARTUP_CATEGORY, *index),
        SettingsPanelHit::BlocksRow(index) => (BLOCKS_CATEGORY, *index),
        SettingsPanelHit::SecurityRow(index) => (SECURITY_CATEGORY, *index),
        SettingsPanelHit::ProfilesRow(index) => (PROFILES_CATEGORY, *index),
        SettingsPanelHit::SshRow(index) => (SSH_CATEGORY, *index),
        SettingsPanelHit::KeybindingsRow(index) => (KEYBINDINGS_CATEGORY, *index),
        _ => return None,
    };
    Some(WidgetId::new(category, index))
}
```

Copy the `FONT_CATEGORY` / `BLOCKS_CATEGORY` import paths from the `use` block at the top of the `on_cursor_moved` body in `mouse.rs` (around line 365) — that block is the source of truth for the module names.

In `mouse.rs::on_cursor_moved`, replace the inlined `let hovered = match hit { ... }` with a call to `widget_id_of(&hit)`, and adjust the two consumers below it: `hover_widget` needs `(category, index)`, so read them back off the `WidgetId` (`id.category`, `id.index`), and `hover_transition.retarget` takes the `Option<WidgetId>` directly.

**(c)** In `mouse.rs::on_mouse_left_pressed`, in the settings-panel branch, immediately after `let hit = self.hit_test_settings_panel(px as f32, py as f32);`:

```rust
                // UI/UX v3 P3b3: pulse the row before the match below acts on
                // it. `Outside`, `TitleBar`, `Category` and `Slider` map to no
                // widget id and are skipped.
                if let Some(id) = super::settings_panel_hit::widget_id_of(&hit) {
                    let now = std::time::Instant::now();
                    let anim = self.app.config.animations.clone();
                    self.app.state.settings_panel.press_pulse.press(id, now, &anim);
                }
```

**(d)** In `renderer/overlay/widgets/draw/mod.rs`, add the field to `WidgetTheme` below `hover`:

```rust
    /// Press pulse for the panel's rows (UI/UX v3 P3b3).
    pub press: &'a crate::animations::PressPulse<super::spec::WidgetId>,
```

Rewrite `draw_row_background`'s fill computation:

```rust
    let press = theme.press.weight(spec.id(), theme.now);
    let fill = if spec.focused() {
        Some(theme.tokens.surface_2)
    } else if spec.enabled() && spec.kind().is_interactive() {
        // UI/UX v3 P3b3: press raises the weight so a click on a row the
        // pointer has not finished hovering still paints something to dim.
        let w = theme.hover.weight(spec.id(), theme.now).max(press);
        (w > 0.0).then(|| {
            let s = theme.tokens.surface_3;
            [s[0], s[1], s[2], s[3] * HOVER_ALPHA * w]
        })
    } else {
        None
    };
    // Applied to both branches: pressing an already-focused row must still
    // give feedback, and focus paints an opaque fill with room to dim.
    let fill = fill.map(|c| crate::color_util::press_fill(c, press));
```

**(e)** In the same file's `test_support`, rename `bg_vertices_with_hover` to `bg_vertices_with_states` taking both, and update its three existing callers:

```rust
    pub(in crate::renderer::overlay::widgets::draw) fn bg_vertices_with_states(
        hover: &crate::animations::HoverTransition<WidgetId>,
        press: &crate::animations::PressPulse<WidgetId>,
        now: std::time::Instant,
        f: impl FnOnce(&WidgetTheme<'_>, &mut WidgetSink<'_>),
    ) -> Vec<BgVertex> {
```

with `press,` added to the `WidgetTheme { .. }` literal inside it, and the zero-hover `bg_vertices` helper passing `&PressPulse::default()`.

**(f)** Add `press: &sp.press_pulse,` to all 10 production `WidgetTheme { .. }` sites listed under **Files**, beside their existing `hover: &sp.hover_transition,`. Some tabs bind the panel as something other than `sp` — match the local name used by the neighbouring `hover:` line.

**(g)** In `renderer/overlay/settings/mod.rs`, add the pulse to whatever predicate reports the panel's active animation, beside the existing `hover_transition` check.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu --lib`, then clippy and fmt.
Expected: green, including the two new draw tests and the pre-existing `a_focused_row_ignores_the_hover_weight` (which must still pass — press is zero in it, so `press_fill` is identity).

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): pulse the settings row on press

hit → WidgetId の対応を settings_panel_hit.rs へ切り出し、hover と press
の両方が同じ対応表を使うようにした。focus 済みの行を押した場合も
press_fill を通すため、押下フィードバックが消えない。"
```

---

### Task 6: Context-menu items

**Files:**
- Modify: `nexterm-client-gpu/src/state/menus.rs` (field beside `hover_transition`, around line 124)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/mouse.rs` (a new block at the top of `on_mouse_left_pressed`)
- Modify: `nexterm-client-gpu/src/renderer/overlay/dialog.rs` (the menu row fill and its accent, around line 271)
- Modify: `nexterm-client-gpu/src/state/mod.rs` (one clause in `has_active_animation`)
- Test: `nexterm-client-gpu/src/state/mod.rs`

**Interfaces:**
- Consumes: `crate::animations::PressPulse`, `crate::color_util::press_fill`.
- Produces: `ContextMenu.press_pulse: PressPulse<usize>` (the id is the item index).

- [ ] **Step 1: Write the failing test**

```rust
/// The context menu is the one model whose click commits on release, so its
/// pulse is what the user sees for as long as the button is held. It lives
/// inside `ContextMenu` and dies with it — P3b1's closing ghost is a
/// separate clone and deliberately does not carry it.
#[test]
fn a_context_menu_press_keeps_the_frame_loop_awake() {
    let mut state = ClientState::new(80, 24, 1000);
    let t0 = Instant::now();
    // Assigned directly rather than through `show_context_menu`, which would
    // also start the open animation and make the assertion below pass for
    // the wrong reason. This is how the hover test beside it builds a menu.
    state.context_menu = Some(ContextMenu::new_default(10.0, 10.0, &[]));
    let menu = state
        .context_menu
        .as_mut()
        .expect("the menu was just assigned");
    menu.press_pulse
        .press(1, t0, &nexterm_config::AnimationsConfig::default());
    assert!(state.has_active_animation(t0, 200));
    assert!(!state.has_active_animation(t0 + Duration::from_millis(100), 200));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu --lib a_context_menu_press_keeps_the_frame_loop_awake`
Expected: compile error — `no field press_pulse on type ContextMenu`.

- [ ] **Step 3: Write the implementation**

In `state/menus.rs`, below `hover_transition`:

```rust
    /// Press pulse for this menu's items (UI/UX v3 P3b3).
    ///
    /// Unlike the other three models the menu commits on release, so this is
    /// visible for as long as the button is held — press feedback that
    /// happens to behave like a held state. It dies with the menu: the
    /// closing ghost is a separate clone and does not carry it, because a
    /// dismissed menu has nothing left to give feedback about.
    pub press_pulse: crate::animations::PressPulse<usize>,
```

Add `press_pulse: Default::default(),` to the menu's constructor.

In `state/mod.rs::has_active_animation`, beside the other context-menu clauses:

```rust
        if self
            .context_menu
            .as_ref()
            .is_some_and(|m| m.press_pulse.is_active(now))
        {
            return true;
        }
```

In `mouse.rs::on_mouse_left_pressed`, as the first statement inside `if let Some((px, py)) = self.cursor_position {` — before the resize-edge check, so nothing can preempt it:

```rust
            // UI/UX v3 P3b3: pulse the menu item under the pointer. The menu
            // commits on release, so this is purely additive — it does not
            // consume the press, and every branch below runs exactly as
            // before. `hovered` is already maintained by `on_cursor_moved`,
            // so no second hit test is needed here.
            let menu_anim = self.app.config.animations.clone();
            if let Some(menu) = &mut self.app.state.context_menu
                && let Some(i) = menu.hovered
            {
                menu.press_pulse
                    .press(i, std::time::Instant::now(), &menu_anim);
            }
            let _ = (px, py);
```

Drop the `let _ = (px, py);` line if `px`/`py` are already used further down (they are — it is only there to keep this snippet self-contained if pasted first).

In `renderer/overlay/dialog.rs`, replace the weight read:

```rust
            let w = menu.hover_transition.weight(i, now);
```

with:

```rust
            // UI/UX v3 P3b3: press raises the weight before dimming it; both
            // layers below carry the same treatment so the row and its accent
            // stay one object.
            let press = menu.press_pulse.weight(i, now);
            let w = menu.hover_transition.weight(i, now).max(press);
```

and wrap both layer colours:

```rust
                    crate::color_util::press_fill([hab[0], hab[1], hab[2], 0.90 * w], press),
```
```rust
                    crate::color_util::press_fill([ap[0], ap[1], ap[2], 0.90 * w], press),
```

Leave the label colour below untouched.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu` (the whole crate this time), then `cargo clippy -- -D warnings` and `cargo fmt --check` at the workspace root.
Expected: green, 1013 + the new tests.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): pulse the context-menu item on press

メニューだけは release で確定するため、押下から離すまで pulse が見える。
既存の分岐順は変えず、press ハンドラの先頭で hovered 済みの index に
対して発火するだけの純粋な追加にとどめた。"
```

---

## Known limitations to state in the PR description

Do not present these as bugs found in review; they are decided.

- **A context menu overlapping the tab bar pulses both** the menu item and the tab underneath, because the press falls through to the tab branch. That fall-through is pre-existing — the click already switches tabs today — and P3b3 deliberately does not change control flow to fix it.
- **Minimize and Close never show their pulse.** Wired anyway; see Task 4.
- **Nothing here is verified on a GPU.** The nine-scheme test measures colours, not appearance. This lands on the on-device verification backlog in `docs/plans/ui-ux-modernization-v3.md`.

## Plan updates to `docs/plans/ui-ux-modernization-v3.md`

Add to the P3b checklist, after the P3b2 entry, in the same voice as its neighbours: P3b3 press pulse — the shared `PressPulse<Id>`, why the pulse is one-shot rather than held (three of four models commit on mouse-down), the `hover.max(press)` amendment and the additive-layer alpha boost, and the two non-goals (per-control elevation, foreground colour). Do this as part of the final commit, before opening the PR.
