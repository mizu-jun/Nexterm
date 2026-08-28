# P3b2: Hover Cross-Fade

Status: proposed
Related plan: `docs/plans/ui-ux-modernization-v3.md`, section "P3 — Motion language (M–L)", checklist item "P3b widget and overlay motion"
Parent design: `docs/superpowers/specs/2026-08-28-p3b-motion-application-design.md` — **this document corrects its hover survey; see "Correction" below**
Prior work: P3b1 (PR #79) — `SurfaceMotion`, render-only ghosts, `apply_surface_fade`; P3a (#77, #78) — `Timed`, `Curve`, `duration`

P3b1 gave eleven surfaces an entrance and an exit. P3b2 does the same for the
state a pointer creates: hover, on every chrome control that has it.

## Correction to the parent design

The P3b design document states:

> ### Hover exists in exactly two places

That is wrong, and the error is load-bearing — it set that document's Scope to
two hover models and would have shipped P3b2 with half the chrome still
snapping. **There are four pointer-hover models.** The two the parent document
missed are the tab bar and the custom title bar's window buttons, both of which
are chrome that responds to the pointer and therefore squarely inside the
parent's own stated goal ("controls that respond to the pointer and the keyboard
rather than snapping").

The maintainer reviewed the corrected survey and set P3b2's scope to all four
(2026-08-29).

`snap_layout.rs` also carries a `hovered` flag. It is the Windows snap-layout
overlay, drawn by the OS rather than by this renderer, and it is not a hover
model of ours. Out of scope, permanently.

## What exists today (measured, not assumed)

Four models. None has a transition of any kind; each flips between two
appearances on the frame the pointer crosses a boundary.

| # | Model | State | Drawn by | Resting → hovered |
|---|---|---|---|---|
| 1 | Settings widget row | `SettingsPanel.hover_widget: Option<HoverDwell>` → `WidgetSpec.hovered` | `draw_row_background`, `widgets/draw/mod.rs:149` | nothing → `surface_3` at `alpha * HOVER_ALPHA` (0.35) |
| 2 | Context-menu item | `ContextMenu.hovered: Option<usize>` | `overlay/dialog.rs:267` | nothing → `tab_active_bg` @ 0.90 fill **+** `accent_primary` @ 0.90 3 px left accent **+** text `text_secondary` → `text_primary` |
| 3 | Tab | `ClientState.hovered_tab_id: Option<u32>` | `ui_verts.rs:363` | `inactive_bg` → `inactive_bg` + `(0.06, 0.06, 0.08)`, **gated on `tab_bar.hover_highlight`** |
| 4 | Window button | `ClientState.hovered_window_button: Option<WindowButton>` | `ui_verts.rs:778-805` | nothing → `semantic_error` (Close) or `inactive_bg` + `(0.08, 0.08, 0.10)` fill, **+** text `text_secondary` → `text_primary` |

Facts that shape the design:

- **All four id types are already `Copy + PartialEq`:** `WidgetId`
  (`widgets/spec.rs:69`), `usize`, `u32`, and `WindowButton`
  (`state/mod.rs:443`). One generic transition type can serve all four.
- **`build_tab_bar_verts` already takes `_animations_cfg: &AnimationsConfig`**
  (`ui_verts.rs:172`) and never uses it. The config is plumbed and waiting; only
  `now` needs adding, and `frame_now` already exists at the call site
  (`render_frame.rs:197`).
- **Models 3 and 4 already request a redraw only when the hovered id changes**
  (`mouse.rs:433`, `:459`). That change-only redraw *is* the absence of a
  transition, and it is why they need aggregate clauses (below) once they have
  one.
- **Model 1's hover fill loses to focus.** `draw_row_background` checks
  `focused()` first and paints an opaque `surface_2`; hover is the `else`. A
  focused row shows no hover fill at all today.
- **Selection is not hover and stays untouched.** `palette.rs`,
  `host_manager.rs` and `macro_picker.rs` are keyboard-driven and highlight a
  `selected` index; the consent and close-window dialogs highlight a selected
  *button*. The parent design's reasoning holds: with key-repeat, a selection
  that fades lags the arrow key that moved it.

## The central finding: P3b1's mechanism does not transfer

P3b1's visual was `apply_surface_fade` — scale `color[3]` over the vertex range
a builder appended. That works for a whole surface arriving or leaving, because
"absent" and "present at alpha 1" are the two endpoints and alpha interpolates
between them.

Hover is not that shape. **Three of the four models interpolate more than one
property**, and two of them compute the hovered colour by *brightening a
resting colour* rather than by adding a layer:

- Model 2 changes a fill, an accent line, *and* a text colour.
- Model 3 has no extra layer at all — the same quad is painted a brighter
  colour.
- Model 4 changes a fill *and* a text colour, and the Close button's hovered
  fill is an unrelated hue (`semantic_error`), not a brightening.

Scaling alpha over a vertex range cannot express any of those. Fading model 3's
tab quad would fade the tab out of the bar, not into its hover colour.

**So P3b2's mechanism is a scalar, consumed at colour-choice time.** Each
builder asks for a hover weight in `[0, 1]` for the id it is about to draw, and
lerps between the resting and hovered appearance. Nothing is applied after the
fact; there is no post-pass. This is a different mechanism from P3b1's, not an
extension of it, and the two coexist without interacting.

## Architecture

### 1. `HoverTransition<Id>` — one generic, four instantiations

`nexterm-client-gpu/src/animations/hover.rs`:

```rust
/// A cross-fade between the previously hovered item and the current one.
///
/// One pointer means one transition per model — but *per model*, not
/// globally: moving the pointer from a settings row to a tab starts a
/// tab-bar transition while the widget layer's is still fading out.
pub struct HoverTransition<Id> {
    /// The item fading out, and the weight it held when it started to.
    from: Option<(Id, f32)>,
    from_anim: Timed,
    /// The item fading in.
    to: Option<Id>,
    to_anim: Timed,
}

impl<Id: Copy + PartialEq> HoverTransition<Id> {
    /// Point the transition at `to`, resuming from whatever is on screen.
    pub fn retarget(&mut self, to: Option<Id>, now: Instant, anim: &AnimationsConfig);
    /// Hover weight for `id` in `[0, 1]`.
    pub fn weight(&self, id: Id, now: Instant) -> f32;
    /// Whether another frame is needed.
    pub fn is_active(&self, now: Instant) -> bool;
}
```

**Two timers, not one.** The obvious form of this type is a single `Timed`
with the outgoing item at `1 - progress` and the incoming one at `progress`,
so the pair always sums to 1. That is wrong, and wrong in a way that shows:
the sum-to-1 invariant only holds when the outgoing item was already at
weight 1. Enter row A, and 50 ms later — while A is still at 0.5 — move to
row B: a single timer makes B *jump* to 0.5 on the frame the pointer crosses
the boundary. Sweeping a pointer down a list crosses boundaries faster than
100 ms routinely, so the naive form pops on exactly the gesture hover exists
to support.

With two timers the outgoing item decays from the weight it actually held
and the incoming one rises from the weight *it* actually held (0 normally,
or its partly-decayed value if the pointer came back to it). Neither jumps,
and the pair simply does not sum to 1 mid-handoff — which is correct, because
at that instant neither row is fully hovered.

**One slot remains a real limitation, stated plainly.** Only one item can be
fading out at a time, so sweeping across five rows in 200 ms drops the three
intermediate rows to 0 the moment each is replaced, leaving a trail that cuts
off rather than one that fades. A fixed-capacity map of `id → Timed` would
fix it; a single slot is bounded, trivially reasoned about, and matches what
the parent design chose. If the cut-off trail looks wrong on hardware, that
map is the follow-up — it is a change of internals behind an unchanged
`weight()` call, so no consumer would move.

`retarget` is idempotent when `to` is unchanged — the same edge-detector
discipline `tick_tooltip` needed in P3b1, and for the same reason: it is called
from a per-move handler that fires far more often than the state changes.

**Why not extend `HoverDwell`.** The parent design's argument survives
unchanged and is worth restating, because P3b1 added a second consumer of
`hover_widget` and the reasoning now has to hold for both: `hover_widget`
becomes `None` when the pointer leaves the panel, which is exactly when the
fade-out must still be running. The dwell timer must also survive small
movements within a row so tooltips do not flicker, while the transition starts
whenever the target changes. They answer different questions.

No interaction with P3b1's `tooltip_shown` / `tooltip_snapshot`: those key off
`HoverDwell::is_ready`, which a transition neither reads nor writes.

### 2. Where each transition lives

| Model | Owner | Retargeted from |
|---|---|---|
| 1 | `SettingsPanel.hover_transition` | `mouse.rs:395`, beside the existing `hover_widget` assignment |
| 2 | `ContextMenu.hover_transition` | wherever `menu.hovered` is written (`event_handler/accessibility.rs:265` and the mouse path) |
| 3 | `ClientState.tab_hover` | `mouse.rs:433`, replacing the change-only redraw |
| 4 | `ClientState.window_button_hover` | `mouse.rs:459`, likewise |

Model 2's owner is worth one note: **P3b1 clones `ContextMenu` into a
render-only ghost on dismiss.** A transition inside it is cloned too, and since
`Timed` is a value queried with `now`, the ghost's hover fade keeps running
while the menu fades out. That is harmless and arguably right — the item the
pointer left should not snap back at the moment the menu starts leaving. It is
called out here so a reviewer does not read it as an oversight.

### 3. Applying the weight

Each site lerps rather than adding a layer. Sketches, not final code:

```rust
// Model 1 — an additive fill, so the weight is just alpha.
let w = sp.hover_transition.weight(spec.id(), now);
let fill = if spec.focused() {
    Some(theme.tokens.surface_2)          // focus still wins, opaque
} else if w > 0.0 && spec.enabled() && spec.kind().is_interactive() {
    let s = theme.tokens.surface_3;
    Some([s[0], s[1], s[2], s[3] * HOVER_ALPHA * w])
} else {
    None
};

// Model 3 — a brightening, so the weight lerps the colour itself.
let tab_bg = if is_active {
    active_bg
} else if has_activity {
    activity_bg
} else if cfg.hover_highlight {
    let w = state.tab_hover.weight(pane_id, now);
    lerp_rgba(inactive_bg, brighten(inactive_bg, [0.06, 0.06, 0.08]), w)
} else {
    inactive_bg
};
```

A shared `color_util::lerp_rgba(a, b, t)` covers models 2, 3 and 4 — three
call sites, so it is a helper rather than a premature abstraction. It belongs
in `color_util.rs` beside `with_alpha`, which G11 added for the same kind of
reason.

**Focus precedence stays as it is** (model 1). A focused row shows `surface_2`
and no hover fill today; making hover fade *under* an opaque focus fill would
be invisible work, and making it fade *over* one changes a shipped appearance
for reasons unrelated to motion. If the focus/hover interaction is worth
revisiting, that is its own change.

**The tab gate stays a gate.** With `tab_bar.hover_highlight = false` there is
no transition — not a transition to a zero-weight target. Retargeting is
skipped entirely, so the config key keeps meaning exactly what it means now.

### 4. Durations and curves

The parent design's table gives widget hover 100 ms `EasyEase` in and out. All
four models take the same pair: hover is one gesture, and giving a tab a
different hover speed from a settings row would read as inconsistency rather
than as hierarchy.

| Surface | In | Out |
|---|---|---|
| All four hover models | 100 ms `EasyEase` | 100 ms `EasyEase` |

`duration::FASTER` is 100. Routed through `AnimationsConfig::scaled_duration_ms`
inside `retarget`, so `animations.enabled = false` / `intensity = "off"` yields
0, the `Timed` is born finished, and every model snaps exactly as it does
today. That is the whole reduced-motion path, inherited free from P3a.

### 5. The aggregate

`ClientState::has_active_animation` gains four clauses — one per model. P3b1's
crate-guide note already warns that a surface which does not add its clause
will simply never animate; models 3 and 4 are the first *non-overlay* consumers
of that rule, and their existing change-only `request_redraw` calls
(`mouse.rs:433`, `:459`) become redundant once the aggregate covers them. Leave
them: a redraw on the frame the target changes is still correct, and removing
them is a separate simplification.

## Testing

CI-verifiable, no GPU required:

- **`HoverTransition`** — retargeting from A to B gives A `1 - p` and B `p`;
  everything else 0; a transition interrupted mid-flight is continuous;
  retargeting to the same id is a no-op that does not restart the fade; leaving
  a model entirely (`to = None`) still fades the last item out; a zero duration
  makes the transition finished on creation.
- **`lerp_rgba`** — endpoints exact at `t = 0` and `t = 1`, midpoint linear,
  `t` clamped.
- **Per model** — retargeting makes `has_active_animation` true and it goes
  false once the transition is done; four models, four clauses.
- **The tab gate** — with `hover_highlight = false`, moving the pointer across
  tabs starts no transition and the aggregate stays false.
- **Focus precedence** — a focused row's fill is `surface_2` regardless of
  hover weight.
- **The idle property, again** — P3b1's `a_fully_idle_state_wants_no_animation_frames`
  must still pass with four more ways to break it.

### Not verifiable here — stated plainly

Whether 100 ms reads as responsive or as lag; whether a tab's +0.06 brightening
is even perceptible as a fade or whether the delta is too small for the
transition to register; whether the Close button fading to red rather than
snapping to it weakens the "this is destructive" signal. None of that is
judgeable from CI or from a container, and motion cannot be captured by the
plan's screenshot convention. This joins the on-device verification backlog,
where the P3a and P3b1 entries already note there is no established capture
format for transitions.

The Close-button question is the one worth putting in front of a human first:
it is the only case in P3b2 where the hovered appearance carries a warning
rather than an affordance.

## Risks

- **Four models, one generic type.** A bug in `HoverTransition` is a bug
  everywhere. Mitigated by testing the type directly rather than through four
  consumers, exactly as P3b1 did with `SurfaceMotion`.
- **`ui_verts.rs` is not the overlay layer.** Models 3 and 4 live in the tab-bar
  builder, which has no unit-test net and takes `state: &mut ClientState`.
  P3b1's nine gate+fade sites at least shared one shape; these two are
  hand-edited colour choices inside a long builder. Keep the diff to the colour
  expressions.
- **A brightening delta may be below the perceptual floor for a fade.** +0.06
  on one channel pair is a small step; a 100 ms interpolation across it may be
  indistinguishable from a snap. This is a "the work may not be visible" risk,
  not a correctness risk, and only hardware answers it.
- **P3b1's ghost carries a running transition** (model 2). Called out above as
  intended; the risk is that a future reader reads it as a leak.

## Delivery

Two PRs to `master`, splitting on the boundary that matters — the overlay layer
has a test net and a shared visual language, the title bar has neither:

| PR | Scope |
|---|---|
| **P3b2a** | `HoverTransition`, `lerp_rgba`, and the two overlay models: the settings widget row and the context menu |
| **P3b2b** | The two title-bar models: the tab bar and the window buttons, plus their aggregate clauses |

Each: English commits and PR text, `cargo clippy --workspace --all-targets -- -D warnings`
and `cargo fmt --check` green, full workspace suite green. No config key and no
locale change is expected in either — `tab_bar.hover_highlight` already exists
and keeps its meaning — so `docs/CONFIGURATION.md` and `nexterm-i18n/locales/`
stay untouched; if that changes, the key-parity test and `doc_matches_schema`
are the guards. No `Cargo.lock` change is expected, so no flatpak sources
regeneration.

## Open question for the maintainer

**Does the tab bar's hover brightening need to grow to be worth animating?**
Model 3 interpolates across +0.06/+0.06/+0.08 — the smallest delta of the four.
If it proves imperceptible on hardware, the options are to leave it (a fade
nobody sees costs nothing but the code), to increase the delta (a visual change
beyond motion, and beyond this phase's remit), or to drop model 3 from P3b2
after all. This design assumes "leave it" and flags it rather than pre-deciding
a question only a display can settle.
