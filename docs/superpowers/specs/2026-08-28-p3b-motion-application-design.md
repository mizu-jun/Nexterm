# P3b: Applying the Motion Language — Hover, Press, and Overlay Transitions

Status: proposed
Related plan: `docs/plans/ui-ux-modernization-v3.md`, section "P3 — Motion language (M–L)", checklist item "P3b widget and overlay motion"
Related prior work: P3a motion foundation (#77, #78) — `Timed`, `Curve`, `duration`, animation-driven redraw
Spec for P3a: `docs/superpowers/specs/2026-08-28-p3a-motion-foundation-design.md`

P3a built the foundation and proved it on one surface. P3b spends it everywhere
it applies, and adds the one widget state the codebase never had.

## Goal

Give every chrome surface the same motion vocabulary: controls that respond to
the pointer and the keyboard rather than snapping, and overlays that arrive and
leave rather than appearing and vanishing.

## What exists today (measured, not assumed)

### Hover exists in exactly two places

- **The widget layer.** Every migrated settings tab builds its specs with
  `desc.place(rect, control).hovered(hovered == Some(index))` — a single
  `Option<u16>` per frame. `draw/mod.rs:152` paints a hover row fill when
  `spec.hovered && spec.enabled() && spec.kind().is_interactive()`, with no
  transition of any kind.
- **The context menu.** `ContextMenu` (`state/menus.rs:111`) carries its own
  `hovered: Option<usize>`, and `dialog.rs:271` paints a fill plus a 3 px left
  accent line from it. This is a second, independent hover model.

`palette.rs`, `host_manager.rs` and `macro_picker.rs` have **no hover at all** —
they are keyboard-driven and highlight a `selected` index. The consent and
close-window dialogs likewise highlight a selected *button*, not a hovered one.
Selection is a different thing from hover and P3b does not animate it: with
key-repeat, a selection that fades lags behind the arrow key that moved it.

`HoverDwell` (`settings/hover.rs`, 97 lines) stores `{ category, index, since }`
and is entered from `mouse.rs:396`. `enter` keeps the running timer when the
pointer stays on the same control. So the *entry* timestamp a fade-in needs
already exists; the widget the pointer **left** does not.

### Press does not exist

`WidgetSpec` (`spec.rs:342`) carries `desc`, `rect`, `control_rect` and
`hovered`. There is no `pressed`, and no draw code reads one. The plan's phrase
"widget hover/press" therefore describes, for press, a state that must be built
before it can be animated: pointer-down tracking, a visual, and a route through
the three consumers of `WidgetDesc` (draw, hit-test, AccessKit).

### Ten stored overlay surfaces, in two shapes, plus one derived

`bool`-shaped — the surface owns an `is_open: bool`:

| Type | Location |
|---|---|
| `SettingsPanel` | `settings/mod.rs:77` — **already animated by P3a**, by hand |
| `HostManager` | `host_manager.rs:424` |
| `MacroPicker` | `macro_picker.rs:16` |
| `CommandPalette` | `palette.rs:47` |
| `BlockNameModal` | `state/blocks.rs:45` |
| `FileTransferDialog` | `state/menus.rs:354` |

`Option`-shaped — the state *is* the openness, so closing destroys the content:

| Field | Type | `Clone`? |
|---|---|---|
| `ClientState.context_menu` | `ContextMenu` (`menus.rs:111`) | yes |
| `ClientState.pending_consent` | `ConsentDialog` (`consent.rs:44`) | yes |
| `ClientState.close_window_dialog` | `CloseWindowDialog` (`state/mod.rs:403`) | yes |
| `HostManager.password_modal` | `PasswordModal` (`host_manager.rs:328`) | **see below** |

**Derived, not stored:** the tooltip has no open flag. It is drawn when
`HoverDwell::is_ready(now)` returns true, and only from
`renderer/overlay/settings/theme_tab.rs:134` — the Theme tab is currently the
only tab that shows one. Its "openness" is a predicate over the dwell timer, so
it needs a different treatment from the ten stored surfaces.

### `PasswordModal` holds a secret that must not be cloned

`PasswordModal.input` is a **private** `zeroize::Zeroizing<String>`. Its doc
comment states the reason: the memory is reliably zeroed on drop, which "reduces
the risk of password leakage via keyloggers or memory scraping."

A render-only ghost holding a clone of this struct would put a second copy of
the password in memory and keep it alive for the whole exit animation — paying a
security cost for a cosmetic effect.

The renderer already respects this boundary. `build_password_modal_verts`
(`dialog.rs:19`) reads only `modal.host.username`, `modal.host.host`,
`modal.host.port`, `modal.input_len()`, `modal.error`, `modal.remember` and
`modal.prefilled`, under an explicit comment:

> `// HIGH H-6: `input` is a private Zeroizing<String>, so only retrieve the char count via input_len()`

So a prior security review already drew this line. P3b follows it rather than
inventing a new one.

## Scope

**In scope:**

- `SurfaceMotion`, a shared open/close timer pair in `animations/`, and the
  retrofit of P3a's hand-written settings-panel fields onto it.
- Open/close motion for all ten stored surfaces and for the tooltip.
- A hover cross-fade for the two hover models: the widget layer and the context
  menu.
- A `pressed` widget state — pointer-held and keyboard/AccessKit activation
  flash — with its visual and its motion.

**Out of scope:**

- Animating `selected` highlights in the palette, host manager, macro picker,
  consent and close-window dialogs. Selection is not hover, and a fade would lag
  behind key-repeat.
- OS reduced-motion detection — P3c. `animations.enabled` / `intensity` remain
  the controls, and they work automatically for every `Timed` P3b creates.
- Reclassifying any consent surface (Dialog vs InfoBar) — that is P6, and the
  plan calls it security-sensitive.
- Tooltips on tabs other than Theme. P3b animates the tooltip that exists; it
  does not add new ones.
- Any new config key or user-facing string.

## Architecture

### 1. `SurfaceMotion` — the shared timer pair

`nexterm-client-gpu/src/animations/surface.rs`:

```rust
pub struct SurfaceMotion {
    open_anim: Option<Timed>,
    closing: Option<Timed>,
}

impl SurfaceMotion {
    pub fn open(&mut self, now: Instant, anim: &AnimationsConfig, ms: u32, curve: Curve);
    pub fn close(&mut self, now: Instant, anim: &AnimationsConfig, ms: u32, curve: Curve);
    /// Visibility in [0, 1]: 0 hidden, 1 fully shown.
    pub fn progress(&self, now: Instant) -> f32;
    /// Whether the renderer should draw the surface at all.
    pub fn is_visible(&self) -> bool;
    /// Whether another frame is needed.
    pub fn is_active(&self, now: Instant) -> bool;
    /// Drop a finished exit animation.
    pub fn retire(&mut self, now: Instant);
}
```

The body is P3a's settings-panel logic lifted verbatim, including its two
ordering rules: read the on-screen value **before** overwriting either field,
and pass `1.0 - visibility` when starting a close, because `closing` counts up
while visibility counts down.

**P3a's hand-written fields are replaced by this type.** Leaving one bespoke
copy beside ten shared ones is the worst of both.

### 2. Applying it — two shapes, one exception

**`bool`-shaped surfaces** gain `motion: SurfaceMotion` next to the existing
`is_open`. `is_open` remains the single truth for input routing and the
AccessKit tree and still flips the instant the user acts; only the renderer
consults `motion`. This is P3a's contract, unchanged.

**`Option`-shaped surfaces** gain `closing: Option<(T, Timed)>` — the ghost owns
a clone of the content, so the live field can go `None` immediately. Input and
accessibility keep reading the live `Option`; the renderer draws the live one
when present and the ghost otherwise.

**`PasswordModal` uses a redacted ghost.** No clone of the secret:

```rust
/// What the password modal's exit animation needs to draw, and nothing more.
///
/// Deliberately not a clone of `PasswordModal`: its `input` is a private
/// `Zeroizing<String>` whose whole point is to minimise how long the secret
/// exists. `input_len` is the same thing `build_password_modal_verts` already
/// reads (see the H-6 comment there) — the mask is drawn from a count, never
/// from the characters.
pub struct PasswordModalGhost {
    username: String,
    host: String,
    port: u16,
    input_len: usize,
    error: Option<String>,
    remember: bool,
    prefilled: bool,
}
```

**The tooltip** has no stored openness, so it gets a `SurfaceMotion` driven by
the dwell predicate: when `HoverDwell::is_ready` turns true, open it; when the
dwell target changes or clears, close it. The ghost problem does not arise — a
tooltip's content is its anchor and its text, both derivable while closing from
the same snapshot the motion carries.

### 3. Hover — a single-slot cross-fade

One pointer means one transition in flight:

```rust
pub struct HoverTransition {
    from: Option<WidgetId>,
    to: Option<WidgetId>,
    anim: Timed,
}
```

Drawing reads it as: alpha is `progress` for `to`, `1 - progress` for `from`,
and 0 for everything else.

**Why not extend `HoverDwell`.** `hover_widget` becomes `None` when the pointer
leaves the panel, which is exactly when the fade-out must still be running. The
dwell timer and the transition timer also answer different questions — the dwell
must survive small movements within a row so tooltips do not flicker, while the
transition starts whenever the target changes. Keeping them separate lets each
stay simple.

The context menu gets the same shape with `usize` in place of `WidgetId`.

### 4. Press — the new state

```rust
pub enum PressState {
    /// Pointer is down and still over the control.
    Held(WidgetId),
    /// Activated from the keyboard or AccessKit; decays on its own.
    Flash(WidgetId, Timed),
}
```

One slot, for the same reason hover has one. `WidgetSpec` gains
`pressed: bool`, and `draw_widget` consumes it beside `hovered`, so a control
kind gets press for free once the shared chrome draws it.

Following Fluent: pressing and then dragging off the control clears the visual
but keeps the capture, so releasing outside does not activate. `Flash` exists so
that a keyboard user pressing Enter sees that something happened — today they
get no feedback at all. It is started from `apply_<tab>_action`, the single
state transition the mouse, keyboard and AccessKit paths already share, so all
three routes light up without three separate wirings.

### 5. Durations and curves

Fluent's rule is that larger elements and longer travel take longer:

| Surface | In | Out |
|---|---|---|
| Widget hover | 100 ms `EasyEase` | 100 ms `EasyEase` |
| Widget press | 50 ms `DecelerateMax` | 100 ms `AccelerateMax` (flash decay) |
| Context menu, tooltip | 150 ms `DecelerateMax` | 100 ms `AccelerateMax` |
| Dialogs and large panels | 300 ms `DecelerateMax` | 150 ms `AccelerateMax` |

The settings panel keeps P3a's 200 / 150 rather than moving to the dialog row —
changing a shipped feel is a separate decision from giving nine other surfaces
one for the first time.

### 6. The aggregate

`ClientState::has_active_animation` gains one clause per surface. With
`SurfaceMotion` each clause is a single `is_active(now)` call, and P3a's
crate-guide note already warns that a surface which does not add its clause will
simply never animate.

## Testing

CI-verifiable, no GPU required:

- **`SurfaceMotion`** — the same property set P3a pinned for the settings panel,
  now once instead of eleven times: open runs 0 → 1; close leaves the surface
  visible while fading; reopening mid-fade is continuous; a zero duration is
  finished on creation.
- **Every surface** — opening makes `has_active_animation` true and closing
  leaves the logical state (`is_open == false`, or the live `Option` at `None`)
  while `is_visible()` is still true.
- **The redacted ghost** — a test that `PasswordModalGhost` has no field of a
  string type carrying the password, and that the mask length it produces
  matches `input_len()`. The point is to make a future "just clone the modal"
  refactor fail a test rather than pass review.
- **Hover** — a transition from A to B gives A `1 - p` and B `p`; leaving the
  panel entirely still fades the last widget out; a transition interrupted
  mid-flight is continuous.
- **Press** — `Held` sets `pressed` on exactly one widget; dragging off clears
  the visual; `Flash` decays to nothing without further input; a zero duration
  makes the flash invisible rather than permanent.
- **The idle property, again** — a state with nothing animating returns `false`
  from the aggregate. This is the phase's acceptance criterion and it now has
  eleven more ways to break.

### Not verifiable here — stated plainly

Whether the durations feel right, whether a 100 ms hover reads as responsive or
as lag, and whether the press flash is long enough to notice and short enough
not to annoy, cannot be judged from CI or from a container. Motion also cannot
be captured by the screenshot convention the plan asks for. This joins the
on-device verification backlog, which the P3a entry already notes has no
established capture format for transitions.

## Risks

- **Eleven surfaces, one shared type.** A bug in `SurfaceMotion` is a bug
  everywhere. Mitigated by lifting P3a's already-reviewed logic verbatim rather
  than rewriting it, and by testing the type directly.
- **Retrofitting shipped code.** The settings panel works today; moving it onto
  the shared type risks regressing it for no user-visible gain. Its P3a tests
  move with it unchanged and must stay green.
- **Press is behaviour, not motion.** It changes what the UI does, not just how
  it gets there, and it touches the hit-test and AccessKit paths. It is the one
  part of P3b that could break input handling, and it should land in its own PR.
- **The password ghost.** The whole point is that it does not carry the secret.
  A future contributor "simplifying" it into a `PasswordModal` clone would
  silently undo that; the test named above exists to make that loud.
- **Consent dialogs now linger visually.** `pending_consent` goes `None`
  immediately, so nothing can be answered during the fade — but a security
  prompt that is still on screen after it stopped accepting input deserves a
  look on real hardware before P6 touches consent surfaces again.

## Delivery

Three PRs to `master`, mirroring how P2 and P3a shipped:

| PR | Scope |
|---|---|
| **P3b1** | `SurfaceMotion`, the settings-panel retrofit, and open/close for all ten stored surfaces plus the tooltip |
| **P3b2** | The hover cross-fade, for the widget layer and the context menu |
| **P3b3** | The `pressed` state, its visual, and its motion |

Each: English commits and PR text, `cargo clippy -- -D warnings` and
`cargo fmt --check` green, full workspace suite green. No config key and no
locale change is expected in any of the three, so `docs/CONFIGURATION.md` and
`nexterm-i18n/locales/` stay untouched; if that changes, the key-parity test and
`doc_matches_schema` are the guards. No `Cargo.lock` change is expected, so no
flatpak sources regeneration.
