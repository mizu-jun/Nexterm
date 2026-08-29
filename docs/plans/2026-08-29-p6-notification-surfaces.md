# P6 — Notification surfaces (design spec)

Status: **approved 2026-08-29** — §4 signed off by the maintainer; ready to implement
Date: 2026-08-29
Parent plan: [`ui-ux-modernization-v3.md`](./ui-ux-modernization-v3.md) § P6
Addresses: principles Calm + Personal

---

## 0. Why this spec exists

The parent roadmap describes P6 as three bullets. Measuring first — as the P4
and P5 specs did — shows the first is built on a wrong premise, the second is a
security change that should not be made, and only the third survives intact.
Both corrections are settled: the scope below is what P6 implements.

1. **"New `overlay/infobar.rs` … reuses the update-banner slot pattern in
   `ClientState`."** There is no *the* slot to reuse. Nexterm already ships
   **three** top-of-screen banners, each with its own state field, its own
   builder, and its own hand-written stacking arithmetic. P6 is not "add a
   fourth"; it is "there are already three, and they disagree with each other".

2. **"Reclassify `ConsentDialog` kinds … move low-risk notices to InfoBar."**
   All three consent kinds are *pre-action authorisations* — the action does not
   happen unless the user says yes. An InfoBar is by definition non-blocking, so
   moving one there means performing the action and reporting it afterwards.
   That is a change in security posture, not in presentation. **Dropped**
   (§4, signed off 2026-08-29); the mechanism for a user who finds a prompt
   noisy already exists and is config, not UI.

3. **"New strings ×8 locales."** Stands, and the count is in §3.5.

The correction the measurement points at: **P6 is a consolidation, not an
addition.** Its value is that the three banners stop being three.

---

## 1. Baseline measurement

### 1.1 The three banners

| Surface | State field | Trigger | Dismiss | Colour |
|---|---|---|---|---|
| Update | `update_banner: Option<String>` | GitHub Releases poll, 5 s after start | `Esc`; `Enter` opens the release page | `semantic_success` |
| Offline | `offline_banner_since: Option<Instant>` | IPC not yet connected | clears on connect (`lifecycle.rs:313`) | `semantic_warning` |
| Error | `error_banner: Option<String>` | `ServerToClient::Error` — PTY launch failure, config load error, split failure | `Esc` | `semantic_error` |

Three builders in `ui_verts.rs`, **79 + 63 + 90 = 232 lines**, structurally
identical: pick a colour, compute `bar_y`, call `draw_banner_bg`, correct the
label against the returned ground, emit one string. The stacking is open-coded
in each — the error banner re-derives its offset by testing the other two:

```rust
let mut bar_y = 0.0_f32;
if state.update_banner.is_some()  { bar_y += bar_h; }
if state.offline_banner_since.is_some() { bar_y += bar_h; }
```

A fourth banner means editing three functions. This is the same shape the P1
widget layer was built to remove, one layer up.

### 1.2 Accessibility: the most important banner is invisible

| Surface | AccessKit node | In `tree_state_hash` | In the SR alert region |
|---|---|---|---|
| Update | ✅ `build_update_banner_node`, `Role::Alert` | ✅ | — |
| Offline | ❌ **none** | ✅ (`is_some()`) | — |
| Error | ❌ **none** | ❌ **absent** | ❌ |

`error_banner` occurs **zero times** in `accessibility.rs`. It is the surface
that reports *"your shell could not be launched"* and *"your config failed to
load"*, and a screen-reader user is never told. It is not in the tree hash
either, so even the generic rebuild that might have caught it does not fire.

The SR alert region (`state.alerts`) does not cover the gap: `AlertKind` has two
variants, `Bell` and `Notification`, and `add_alert` has exactly two production
call sites (`server_message.rs:99`, `lifecycle.rs:489`).

`offline_banner_since` is the inverse defect — it is hashed, so its appearance
forces a tree rebuild, but no node is ever added. The rebuild buys nothing.

### 1.3 No motion

The banners are not among the ten surfaces P3b1 gave `SurfaceMotion`. They pop
in and out on a frame boundary, and `ClientState::has_active_animation` — the
single place that decides whether to request another frame — knows nothing about
them. Every other overlay in the product animates; these three do not.

### 1.4 No auto-dismissal

The roadmap's word is "auto-dismissing". None of the three are. Update and error
persist until `Esc`; offline persists until the connection succeeds. An error
from a transient failure stays on screen indefinitely.

### 1.5 They occlude chrome, and content, without reflow

All three draw from `y = 0`, and `grid_offset_y` is `tab_bar_h + padding_y` —
unchanged by their presence (`render_frame.rs:280`). The banners are built at
lines 1212 / 1233 / 1253, *after* the tab bar at line 876, so they draw over it.

With the default `tab_bar.height = 32` and a bar of `cell_h * 1.4`, one banner
roughly covers the tab bar. All three stack to about three times that, covering
the tab bar and roughly two rows of terminal output — which are **not reflowed
and not scrolled**, only hidden. There is no cap on the stack.

---

## 2. Decisions

### D1 — Consolidate; do not add

One `InfoBar` stack owns every non-blocking status message. The three existing
banners become three *kinds* of the same surface, not three surfaces. A fourth
message type then costs one enum arm, not a fourth builder.

The gate that makes this stick is structural, not visual: after P6 there must be
**no second place that computes a banner's `y`**.

### D2 — Overlay, do not reflow

An InfoBar that pushes the grid down changes the terminal's row count, which
means a PTY resize — a `SIGWINCH` and a full application redraw — every time a
banner appears or disappears. For a surface that is supposed to be *calm*, and
that can be triggered by a background poll, that is the wrong trade. The stack
keeps overlaying.

What changes is *what* it overlays: the stack moves **below the tab bar**.
Chrome hiding chrome is the worse of the two costs — the tab bar is how the user
navigates, and hiding it while an error is up is exactly when they need it.

### D3 — Severity decides the dismissal policy

| Kind | Severity | Auto-dismiss | Rationale |
|---|---|---|---|
| Error | error | **never** | It reports that something the user asked for did not happen |
| Offline | warning | never (self-clearing) | Its whole content is "still not connected"; it ends when that ends |
| Update | info | after a timeout | Purely informational, and it has a second life in the settings panel |

"Auto-dismissing" from the roadmap therefore applies to exactly one of the three
today. That is worth stating rather than building a timer every kind ignores.

### D4 — Only the top bar carries an activation

Today `Enter` opens the release page whenever `update_banner.is_some()`. With a
stack, "the bar `Enter` acts on" has to be defined rather than inherited. It is
the **top** bar, and only if its kind has an activation at all — so `Enter` does
nothing while an error bar sits above the update bar.

This is a behaviour change hiding inside a refactor, which is why it is a
decision here and not something to discover during P6b. It is cheap to revisit
in P6a if the ordering in §3.2 turns out to bury the update bar too often.

---

## 3. Design

### 3.1 State

```rust
/// A non-blocking status message. Fluent's InfoBar, not its Dialog or Flyout.
pub struct InfoBar {
    pub kind: InfoBarKind,
    pub entrance: Timed,
    /// `Some` once the bar has been dismissed and is only being drawn out.
    pub exit: Option<Timed>,
    /// Wall-clock deadline for an auto-dismissing kind.
    pub expires_at: Option<Instant>,
}

pub enum InfoBarKind {
    UpdateAvailable { version: String },
    Offline { since: Instant },
    ServerError { message: String },
}
```

`ClientState` keeps a single `info_bars: VecDeque<InfoBar>`, replacing the three
`Option` fields. The three fields are **removed**, not wrapped — the same
reasoning as P5's flat text tokens: leaving them lets a future call site keep
setting a banner that the stack does not know about, and removal makes the
compiler enumerate the sites.

`InfoBarKind` carries severity (`fn severity(&self) -> Severity`) and its own
`semantic_*` mapping, so the colour choice stops being open-coded per builder.

### 3.2 Layout

One function owns the stack:

```rust
/// Y of each visible bar, top-down, starting below the tab bar.
fn bar_rects(bars: &[InfoBar], tab_bar_h: f32, cell_h: f32) -> Vec<Rect>
```

Pure, so the cap and the ordering are unit-testable without a GPU. Ordering is
by severity then by age, so an error never sits below an update notice.

**Cap: two visible bars.** Beyond that the stack would eat terminal rows it does
not own. A third and further bar is counted, not drawn, and the bottom bar gains
a localised `"+{count} more"` suffix. §1.5's unbounded stack is the defect this
closes.

### 3.3 Accessibility

Every bar gets an AccessKit node with `Role::Alert`, from one builder driven by
`InfoBarKind` — the same "describe it once" shape the widget layer uses. This is
what closes §1.2: the error banner becomes announceable for the first time.

`tree_state_hash` gains the stack (kind discriminant + message + count). The
elapsed-seconds text of the offline bar is deliberately *excluded* from the hash,
preserving the existing comment's reasoning at `accessibility.rs:2276` — a
per-second rebuild buys a screen reader nothing.

### 3.4 Motion

Each bar carries its own `Timed` entrance and a `(ghost, Timed)` exit, the
`Option`-shaped pattern P3b1 established. The three-place checklist in
`nexterm-client-gpu/CLAUDE.md` applies and is the likeliest thing to get wrong:
the `has_active_animation` clause, the retire call in `lifecycle.rs`, and the
`apply_surface_fade` around the builder. A bar that animates in but never
retires leaves the event loop requesting frames forever.

### 3.5 Strings

Existing keys are reused where the text is unchanged — `update-available`,
`offline-banner-connecting`, `error-banner-prefix`. **New: one key**,
`infobar-more-count` (`"+{count} more"`), added to all 8 locales.

The roadmap's "new strings ×8 locales" is therefore one string, not a set. The
consolidation deliberately does not reword what the banners say; changing the
copy and the architecture in the same PR would make a visual regression
impossible to attribute.

---

## 4. The consent reclassification — **dropped** (signed off 2026-08-29)

> **Decision taken.** The maintainer accepted the recommendation below on
> 2026-08-29: the roadmap's consent-reclassification bullet is dropped, and P6's
> scope is the three banners only. No consent prompt moves out of a modal in
> this phase, and no `ConsentKind` is touched. The reasoning is kept in full
> because a future phase proposing the same change should have to answer it.

The three `ConsentKind` variants are `OpenUrl`, `ClipboardWrite` and
`Notification`. Each is asked *before* the action, and each is gated by a
`ConsentPolicy` (`Allow` / `Deny` / `Prompt`, default `Prompt`) plus a
per-session override.

Moving any of them to an InfoBar means one of two things, and both are worse
than the modal:

- **Act, then notify.** The URL opens, the clipboard is written, or the
  notification is sent, and the user is told afterwards. That is a change in
  security posture disguised as a UI change. All three carry attacker-controlled
  content — a URL, clipboard text, notification text — from whatever is running
  in the pane.
- **Keep it blocking, but draw it as a bar.** A surface that looks non-blocking
  but is not, which is worse than either honest option.

There is no third reading in which "low-risk notice" describes any of them.

**And the escape hatch already exists.** A user who finds a prompt noisy sets
`osc_notification = "allow"` (or `external_url`, or `osc52_clipboard`) in
`config.toml`. That is an explicit, per-category, persisted decision made
outside the moment of attack — which is precisely what a security opt-out should
be, and strictly better than a UI reclassification that would apply to everyone.

Nor is there a currently-modal surface that *should* move: the other modals are
`CloseWindowDialog` (destructive confirmation), `BlockNameModal` (input),
`PasswordModal` (credential entry) and `FileTransferDialog` (progress + input).
None is a passive notice.

**Dropped.** If this is ever revisited it should be reopened as its own spec
with a threat-model section, not folded into a presentation phase.

Consequence for the phase: **P6 touches no security-relevant code.** The consent
flow, `ConsentPolicy`, `ConsentKind`, `pending_consent` and its modal are all
out of scope, and a P6 PR that edits any of them is out of scope by definition.

---

## 5. Gates

| Gate | Assertion |
|---|---|
| **G-single** | `bar_rects` is the only function computing a bar's `y`; a grep gate over `ui_verts.rs` finds no second stacking expression |
| **G-a11y** | every `InfoBarKind` produces an AccessKit node with `Role::Alert` — exhaustive over the enum, so a new kind cannot be added without one |
| **G-hash** | the tree hash changes when a bar is added, removed, or has its message changed, and **does not** change when only the offline bar's elapsed seconds advance |
| **G-cap** | with N bars queued, at most 2 are drawn and the count suffix reports `N - 2` |
| **G-order** | an error bar queued after an update bar is laid out above it |
| **G-idle** | `has_active_animation` is false once every bar's entrance and exit have finished — the P3b1 failure mode, and the one that burns battery silently |
| **G-i18n** | `infobar-more-count` is present in all 8 locale files |

`G-a11y` is the gate that would have caught §1.2's defect, and its exhaustiveness
over the enum is the point: the current three-field design has no shape that
could have failed.

---

## 6. PR breakdown

| PR | Scope | Gate |
|---|---|---|
| **P6a** | `InfoBar` / `InfoBarKind` / `bar_rects` + the pure layout, ordering and cap tests. No call sites yet; the three banners still ship as they are. | G-single, G-cap, G-order |
| **P6b** | Migrate the three banners onto the stack; remove the three `Option` fields and the three builders. Compiler-driven. | full suite; visual parity is the risk |
| **P6c** | AccessKit nodes + tree hash + the `infobar-more-count` string ×8. | G-a11y, G-hash, G-i18n |
| **P6d** | Motion and auto-dismissal for the info severity. | G-idle |

P6b is the risk concentration, as P5b was: wide, mechanical, and the only PR that
can change what the user sees by accident rather than by design.

P6c could be folded into P6b, and is kept separate deliberately — the
accessibility fix is the most valuable change in this phase and should be
reviewable without the migration diff around it.

---

## 7. Verification

- **Measured, in CI**: the §5 gates.
- **Not covered**: appearance. P6 moves the stack below the tab bar and changes
  what is occluded, which no headless test can judge. This lands on the same
  backlog as P4 and P5, and the recommendation from the P5 spec still stands: a
  single on-device pass covering P4 + P5 + P6 before the phase is called done.
- **Specifically worth looking at on device**: three bars stacked at once with a
  small window, where the cap and the count suffix are the only thing between the
  stack and the terminal content.

---

## 8. Open questions

None blocking. Both questions this spec opened were closed before implementation:

- The consent reclassification was a genuine fork and is now settled — dropped,
  §4, signed off 2026-08-29. P6's scope is the three banners.
- The `Enter` binding under a stack became D4 rather than staying a question:
  the top bar carries the activation, and only if its kind has one.

What remains is not a question but a **known limit**: P6 changes what the top of
the screen occludes, and no headless test can judge that (§7). It joins the P4
and P5 on-device backlog rather than blocking the phase.
