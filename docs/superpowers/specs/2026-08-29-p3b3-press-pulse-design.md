# P3b3: Press Pulse

Status: proposed
Related plan: `docs/plans/ui-ux-modernization-v3.md`, section "P3 — Motion language (M–L)", checklist item "P3b widget and overlay motion"
Parent design: `docs/superpowers/specs/2026-08-28-p3b-motion-application-design.md`
Prior work: P3b1 (#79) — `SurfaceMotion`, render-only ghosts; P3b2 (#80, #81) — `HoverTransition<Id>` across all four pointer-hover models; P3a (#77, #78) — `Timed`, `Curve`, `duration`

P3b1 gave overlay surfaces an entrance and an exit. P3b2 gave the pointer's
hover a cross-fade. P3b3 closes the other half of the parent design's "widget
hover/press": the moment the user commits to a control.

## What exists today (measured, not assumed)

**There is no press feedback anywhere in this client.** No draw site reads a
pressed flag; no state field records one. `duration::FASTER = 100` in
`animations/curve.rs` is documented as "Button press feedback" and is
currently unused — P3a transcribed the whole Fluent table ahead of its
consumers.

The fact that shapes this entire design: **three of the four models commit
the action on mouse-*down*, not on mouse-up.**

| Model | Commit point | Surface after the click |
|---|---|---|
| Settings widget row | `on_mouse_left_pressed` (`mouse.rs`, the `hit_test_settings_panel` match) | stays |
| Tab | `on_mouse_left_pressed`, tab-bar branch | stays |
| Window button | `on_mouse_left_pressed`, tab-bar branch | Minimize/Close: gone. Maximize: stays |
| Context-menu item | `on_mouse_left_released` | dismissed on release |

A Fluent-style held "pressed" state — the control sinks while the button is
down and recovers on release — therefore has **no visible window** on three of
the four models: the action, and often the whole surface, changes on the frame
the button goes down. Moving those commits to mouse-up to create that window
would change click semantics across the chrome and is out of scope.

So P3b3 ships a **press pulse**: a one-shot decay fired at the instant of
press, independent of when (or whether) the button comes up.

The maintainer chose this over a held state, and chose colour over geometry,
on 2026-08-29.

## Scope

All four models — the same four `HoverTransition` closed in P3b2, wired at the
same draw sites and (with one exception, below) in the same handler.

Non-goals:

- **No elevation change.** The plan's elevation scale
  (`ui-ux-modernization-v3.md`, "Control 2 (pressed sinks to 1)") has no
  implementation to hook: `add_px_soft_shadow_sdf` draws shadows for whole
  surfaces, and there is no per-control elevation in this renderer. Giving
  every row and tab its own shadow to then animate it is a much larger change
  than P3b3. Recorded here so the plan's line is not read as unimplemented
  scope silently dropped.
- **No geometric scale-down.** Rejected: it moves the fill and the text
  independently and doubles the phase.
- **No foreground colour change.** Fluent drops pressed text to a secondary
  ramp; this client draws over nine builtin schemes and an arbitrary terminal
  background, so the contrast floor is not knowable at design time. Only the
  background moves.
- **No press on the Windows snap-layout path.** `snap_layout.rs` swallows
  `WM_NCLBUTTONDOWN` for `HTMAXBUTTON` and completes the click on
  `WM_NCLBUTTONUP`; the press never reaches client state. Unlike hover — which
  genuinely has two writers there and needed both — press has one.

## The shared type

`animations/press.rs`, sitting beside `hover.rs`:

```rust
pub struct PressPulse<Id> { id: Option<Id>, anim: Timed }

impl<Id: Copy + PartialEq> PressPulse<Id> {
    pub fn press(&mut self, id: Id, now: Instant, anim: &AnimationsConfig);
    pub fn weight(&self, id: Id, now: Instant) -> f32;
    pub fn is_active(&self, now: Instant) -> bool;
}
```

One `Timed`, not the two `HoverTransition` needs. The two-timer form exists
because a hover hand-off must decay the outgoing item from the weight it
actually held; a press is a discrete event with no hand-off — pressing a
second control while the first is still decaying simply replaces it. Losing
the first pulse mid-decay is correct, not a limitation: the user's attention
has moved.

- `press` always restarts: `Timed::new(now, anim.scaled_duration_ms(FASTER), Curve::EasyEase)`.
  Unlike `HoverTransition::retarget` it is **not** idempotent on an unchanged
  id, because the caller is a press handler that fires once per click, not a
  pointer-motion handler that fires per frame. Double-clicking a tab should
  pulse twice.
- `weight` returns `1.0 - anim.progress(now)` for the stored id and `0.0` for
  every other id: full at the press frame, zero 100 ms later.
- `Curve::EasyEase` matches what P3b2 used. Fluent publishes the 100 ms
  duration for press feedback but documents its curves only qualitatively
  (see the note at the top of `curve.rs`), so there is no press-specific curve
  to transcribe; matching hover is the honest default rather than a guess.
- `AnimationsConfig` gating comes free through `scaled_duration_ms`, exactly
  as in `HoverTransition::retarget`: with animations disabled the duration is
  0, `Timed` short-circuits, and `weight` is 0 from the first read. Nothing
  ever renders a stuck pressed appearance.
- **Reduced motion (P3c)** therefore means "no press feedback", not "instant
  pressed state". That is the correct reading here: with the action committed
  on press, a static pressed appearance would have nothing to end it.

## Colour composition

Each draw site keeps computing its own appearance, as with hover. Two lines
change at each site:

```rust
let w = hover_weight.max(press_weight);   // press forces the fill on
// ... the site builds its fill from `w`, unchanged ...
let fill = apply_hsb_animated_rgba(fill, 1.0, 1.0, PRESS_DIM, press_weight);
```

**Why `max` and not just the dim.** Only the tab composes its hover as an
opaque `lerp_rgba(inactive_bg, hovered_bg, w)`. The other three sites draw
the hover as an *additive layer* gated on `w > 0.0` — the window buttons
(`ui_verts.rs`), the context-menu row and its accent (`overlay/dialog.rs`),
and the settings row (`widgets/draw/mod.rs`) all emit no vertices at all
when the weight is zero. Dimming a layer that is not being drawn is
invisible, so press has to raise the weight before it dims it. Pressing
implies the pointer is on the control, so `hover_weight` is normally already
near 1 and the `max` only matters for a click that lands inside the hover
fade's first 100 ms — but without it, that click renders nothing.

**And the layer has to get stronger, not only darker.** The `max` above fixes
the unhovered click, but the *normal* click lands on a control that is already
fully hovered, where `max` changes nothing and only the dim is left to signal
the press. A brightness multiplier scales HSV `v`, so on a scheme whose chrome
is already near-black the absolute step is tiny and pressed would look
identical to hovered. So the three additive sites also raise their layer's
alpha while the pulse is live:

```rust
let a = base_alpha * (1.0 + press_weight * (PRESS_ALPHA_BOOST - 1.0));
```

`PRESS_ALPHA_BOOST` is one shared constant, provisionally 1.7, and the result
is clamped to 1.0. The tab needs no boost: its hover is an opaque lerp with
no alpha to raise, and the dim moves it directly.

This was found by reading the four draw sites while planning; the maintainer
approved the `max` amendment on 2026-08-29, and the alpha boost is its
mechanical completion for the additive sites.

`apply_hsb_animated_rgba` already lerps its multipliers toward identity by
`t`, so `press_weight = 0` returns `base` byte-identical and no new
interpolation helper is needed.

`PRESS_DIM` is one shared constant in `color_util.rs`, provisionally `0.85`.

**Open question, to be resolved in implementation, not now:** a brightness
multiplier scales HSV `v`, so on a scheme whose resting chrome is already very
dark the absolute step is small and the pulse may be imperceptible. The
implementation must measure the pressed-vs-unpressed difference across all
nine builtin schemes and pin it with a test. If some scheme falls below perceptibility,
the fallback is to compose the other way — pull the fill back *down the hover
lerp* (`hover_weight * (1 - press_weight * k)`) so pressed reads as "less
hovered", which is what Fluent's own subtle-button ramp does (rest <
pressed < hover). That fallback needs no new state, only a different
expression at the draw sites.

## Wiring

Every call is one line at a site that already exists.

| Model | Where `press` is called | Id type | Where the field lives |
|---|---|---|---|
| Settings widget row | the `hit_test_settings_panel` match arms in `on_mouse_left_pressed` | `WidgetId` | `settings/mod.rs`, beside the existing hover transition |
| Tab | tab-bar branch of `on_mouse_left_pressed` | `u32` (pane id) | `state/mod.rs`, beside `tab_hover` |
| Window button | same branch, the `hit_minimize` / `hit_maximize` / `hit_close` arms | `WindowButton` | `state/mod.rs`, beside `window_button_hover` |
| Context-menu item | **a new branch** — see below | `usize` | `state/menus.rs`, beside `ContextMenu::hover_transition` |

Two traps:

1. **The context menu has no press branch today.** Its click is resolved in
   `on_mouse_left_released`; `on_mouse_left_pressed` ignores the menu
   entirely. P3b3 adds a hit test there whose only effect is to fire the
   pulse. It must sit **before** the settings-panel branch and before the
   tab-bar branch, because the menu draws above both — placing it later would
   let a menu overlapping the tab bar pulse the tab underneath.
2. **The context menu's pulse dies with the menu.** The field lives inside
   `ContextMenu`, which becomes `None` on release, and P3b1's closing ghost is
   a separate clone that will not carry the pulse. In practice the pulse is
   visible from press until the button comes up — the one model where the
   feedback behaves like a held state. That is acceptable and intended; the
   alternative (hoisting the pulse to `ClientState` so it outlives the menu)
   would animate a control the user can no longer see.

`ClientState::has_active_animation` gains a clause per model, matching what
P3b2b did for the hover transitions, so a decaying pulse keeps requesting
frames. The settings panel's own aggregate is the existing one in
`settings/mod.rs`.

Window buttons stay in scope even though Minimize and Close remove the window
before the pulse can be seen; Maximize shows it, and excluding two of three
would leave an untestable exception in the wiring for no user-visible gain.

## Testing

Unit tests on `PressPulse` (`animations/press.rs`):

1. `press` then read at the press instant → weight 1.0; at +100 ms → 0.0;
   another id reads 0.0 throughout.
2. Pressing a second id while the first is mid-decay → the first reads 0.0
   immediately, the second reads 1.0. Pins the single-slot replacement as
   intended behaviour rather than an accident.
3. `press` twice on the same id → the second call restarts the decay. Pins the
   deliberate non-idempotence against a later copy of `retarget`'s early
   return.
4. With `AnimationsConfig { enabled: false, .. }` → weight is 0.0 at the press
   instant. P3b2b shipped without this gate covered and had to add it in
   review; it is in the plan from the start here.

Composition tests in `color_util.rs`, both across all nine builtin schemes:
the pressed fill differs perceptibly from the merely hovered one (the
measurement behind the open question above), and it does not cut the text
contrast over it by more than 5%.

That second one is deliberately relative rather than a flat ≥ 4.5:1
assertion. Solarized and OneDark already carry contrast defects in their
resting chrome — known since P2b and tracked for P5 — so an absolute
assertion here would fail on those pre-existing defects instead of on
anything P3b3 does. Press must not make legibility worse; fixing what it
inherits is P5's job.

Not covered by tests, and not claimed: whether the pulse *looks* right. GPU
output is not CI-verifiable; this lands on the existing on-device verification
backlog in the plan.

## Delivery

One PR, on top of P3b2b (#81). Task order: the shared type and its tests, then
the four wirings, then the composition constant and its contrast test.
