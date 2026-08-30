# N-4 — The context menu, the SFTP dialog and the delete modals leave the cell path (design spec)

Status: **draft** — §7 awaiting sign-off
Date: 2026-08-30
Parent plan: [`ui-ux-modernization-v3.md`](./ui-ux-modernization-v3.md) § P4
Predecessors: [`2026-08-29-p4-iconography-and-chrome-typography.md`](./2026-08-29-p4-iconography-and-chrome-typography.md) §§ 5.2, 8–8.4 · [`2026-08-30-n3-tab-bar-labels.md`](./2026-08-30-n3-tab-bar-labels.md)
Addresses: the surfaces P4f left on the cell path, minus the two it excluded by design

---

## 0. Why this spec exists

P4f closed with a list: *"the status bar (cell-aligned by design), tab-bar labels
(N-3), the SFTP file-transfer dialog, the context menu, the hand-written parts of
`ssh_tab.rs` / `keybindings_tab.rs`, and a chrome font family (N-5)."* N-3 took
the tab bar. Of what is left, the status bar and N-5 are deliberate exclusions,
so N-4 is the remainder — and measuring it first, as every phase since P4c has,
found the same shape a third time:

1. **The context menu's click target does not match what is drawn.** Not a
   typography concern, not a translation concern: two hard-coded constants sit
   where a measurement belongs. Every menu the app builds is drawn wider than
   its hit region, so the right-hand part of every row — the whole hint
   column — is dead to the mouse today. §1.2.
2. **Two of these surfaces were never internationalised.** The SFTP dialog
   entirely, and the context menu in eight items of the one function every
   right-click opens — so that menu renders half in the user's language and
   half in English. Neither has a key in any of the eight locales, against the
   repo's own convention. §1.3.
3. **The two delete modals drifted.** They were copied, and four differences
   accumulated between the copies — including two competing translation keys
   for the same button. §1.4.

The through-line, stated once so it does not have to be re-derived per phase:
**an unmeasured width is a latent defect, and the ramp is what stops it being
survivable.** N-3 said this of tab widths. N-4 says it of menu widths, field
columns and button boxes.

One thing is genuinely new here. In N-3 and P4c the duplicated width lived in
Rust. In N-4 part of the layout lives in `nexterm-i18n/locales/*.json` — a
button's bracket and a label's leading indent are *inside the translated
string*. Fixing the Rust alone would leave the decoration behind. §1.4.

---

## 1. Baseline measurement

All line numbers are at `ffa7d9c`.

### 1.1 One menu width, five copies

`renderer/overlay/dialog.rs:200-215` computes the drawn width:

```rust
let max_label_w = menu.items.iter().map(|i| visual_width(&i.label)).max().unwrap_or(8);
let max_hint_w  = menu.items.iter().map(|i| visual_width(&i.hint)).max().unwrap_or(0);
let min_cells   = max_label_w + max_hint_w + 5;      // pad .9 + gap 2 + pad 1.5
let menu_w      = (min_cells as f32).max(16.0) * cell_w;
```

Four other places need that same number:

| Site | What it does | What it uses |
|---|---|---|
| `dialog.rs:214` | draws the panel, rows, hover fill, right-aligned hints | the formula above |
| `mouse.rs:645` | clamps a menu opened from the tab-bar `▾` to the window edge | a **second copy** of the formula |
| `mouse.rs:754` | clamps a menu opened by right-click | a **third copy** of the formula |
| `mouse.rs:580` | decides which row is hovered | `18.0 * cell_w` |
| `mouse.rs:1726` | decides which row was clicked | `18.0 * cell_w` |

Three transcriptions of one formula, and two constants that are not the formula
at all. `mouse.rs:1724` carries this comment:

```rust
// Use the same value as the drawn width
// (changing this misaligns drawing and click detection).
let menu_w = 18.0 * cell_w;
```

The comment states the invariant correctly and the line below it breaks the
invariant. It is P4c's footer links again — a mirrored formula that drifted —
except that here the drift already reaches the mouse.

### 1.2 The defect in numbers

The drawn width is `max(label + hint + 5, 16)` cells. The hit width is a flat
`18`. Computed from the current locale files for the three menus
`state/menus.rs` builds with fixed content:

| Menu | Widest label | Widest hint | Drawn | Hit-tested | Dead |
|---|---|---|---|---|---|
| `new_window_system_menu` | `Close this window only` (22) | — | 27 | 18 | 9 cells (33 %) |
| `new_default` | same, via detach/close (22) | `Ctrl+B  %` (9) | 36 | 18 | 18 cells (50 %) |
| `new_for_block` | `Collapse / expand block` (23) | `Ctrl+Shift+L` (12) | 40 | 18 | 22 cells (55 %) |

(`new_tab_dropdown` varies with the user's profile names and is at least as wide
as `new_default`.)

**The defect is one-directional, and it is larger than half the menu.** The
16-cell floor is unreachable — the narrowest menu the app can build is 27 cells
— so no menu is drawn *narrower* than its hit region, and a click outside a menu
cannot fire a row. Every menu is drawn *wider*, and the overhang is dead:
clicking a row on its hint, or anywhere right of its label, dismisses without
acting. On the block menu that is the right 55 %.

The hint column is the natural place to click on a wide row, and it is exactly
the part that does nothing. Localisation makes it worse: the same three menus at
`ja` measure 29 / 38 / 43 cells, so a Japanese user's dead zone is 11 / 20 / 25
cells.

Vertical placement has no such split — both the builder and both hit-tests step
by `i * cell_h`, and separators occupy a full row in all three. A separator can
therefore be "hit", and `input_handler/action.rs:386` makes that a documented
no-op. Nothing to fix; recorded so the next reader does not re-investigate it.

### 1.3 Strings that never reached `fl!`

Two of these surfaces draw hard-coded English. Both matter to N-4 for the same
reason: a string that has not been translated has never had its width tested
against anything but English, so measuring it is measuring the easy case.

**The context menu, in eight items — all of them in one function.**
`state/menus.rs` calls `fl!` thirteen times, and in `new_default` (`:138-197`)
does not:

```
Copy · Paste · Select All · Split Vertical · Split Horizontal
Close Pane · Search... · Settings...
```

`new_default` is the menu every right-click in a pane opens, and it also draws
`Detach to new window` and `Close this window only` through `fl!`. A Japanese
user therefore reads a menu that is *half* translated, line by line — which is
worse than one that is not translated at all, because it looks like a rendering
fault rather than a missing locale.

The width consequence is smaller here than it first appears, and the spec should
say so rather than overstate the case: `new_default`'s widest label is the
localised `Close this window only` (22 cells), not one of the eight. Translating
`Split Horizontal` (16) does not by itself resize the menu at `en`. It can at
another locale — nothing bounds a translation to its source's width — which is
the reason to land the strings behind the measurement gate rather than the
reason the gate exists.

**The SFTP dialog, entirely.** `renderer/overlay/picker.rs:276-297`:

```rust
let title = if ft.mode == "upload" {
    "SFTP Upload  (Tab=next, Enter=send, Esc=cancel)"
} else { /* … Download … */ };
let field_labels = ["Host:", "Local:", "Remote:"];
```

Five distinct strings, none of which exists in any of
`nexterm-i18n/locales/*.json`. (`palette-sftp-upload`
does exist — that is the command-palette *entry*, a different string.) The root
`CLAUDE.md` requires every user-facing string to go through `fl!` and be added
to all eight locales, so both of these are standing violations, not new
requirements N-4 invents.

It is also inseparable from the measurement work. The field column is fixed:

```rust
add_px_rect(px + cell_w * 8.0, row_y, pw - cell_w * 9.0, cell_h, …);   // :310
```

Eight cells is enough for `Remote:` (7) and nothing more. The translations do
not exist yet, so the overflow is a projection rather than a measurement — but
it is a safe one: `リモートパス:` is 13 cells, and the eight locales already
contain plenty of labels that run two to three times their English source (`de`
renders `[ Cancel (Esc) ]` as `[ Abbrechen (Esc) ]`, 16 → 19). Translating
without measuring would overrun the field on the first non-English locale, and
measuring without translating would leave three strings the ramp cannot help.
The two land together or neither does.

The panel is `56.0` cells wide (`:243`) with no clamp against `sw`, so a narrow
window already lets it run past both edges.

### 1.4 The two delete modals drifted in four places

`ssh_tab.rs::draw_delete_dialog` (171 lines) and
`keybindings_tab.rs::draw_delete_dialog` (155 lines) are a copy and its
original. `diff` puts the differences at four points beyond the expected two
(the selected entry, the title key):

1. **Two translation keys for one button.** `settings-dialog-cancel-plain` in
   SSH, `settings-dialog-cancel-bracketed` in Keybindings.
2. **The cancel label's colour.** Keybindings moves it `primary → secondary`
   when focus leaves; SSH holds `primary` in both states, so its cancel button
   signals focus by background only.
3. **The label's x offset.** `cell_w * 0.5` in SSH, `cell_w` in Keybindings —
   in a button both size at `cell_w * 14.0`.
4. **The hint line.** SSH draws `settings-ssh-delete-hint` under the buttons;
   Keybindings draws nothing.

None of the four is a decision anyone made. They are what a copy does when the
copies are edited months apart, and (2) is a real accessibility difference
between two dialogs that do the same thing.

**The decoration is inside the translations.** This is the part that makes the
fix cross the Rust boundary:

```json
"settings-dialog-cancel-bracketed": "[ Cancel (Esc) ]",
"settings-dialog-cancel-plain":     "  Cancel (Esc)",
"settings-ssh-delete-hint":         "  Use <- -> / Tab to switch buttons / …",
```

The brackets stand in for a button border that `add_px_rect` now draws anyway,
and the leading spaces are an indent from when the only positioning available
was a whole cell. Both are dead weight at best. Once the label is centred in
its box, a two-space prefix moves the text off-centre by one cell — the
decoration stops being cosmetic and becomes a layout bug. It is N-3's
*"the label's decorative spaces are gone"* finding, one layer further out: in
the locale data, replicated eight times.

Button geometry is unmeasured in both files:

```rust
let dlg_btn_w = cell_w * 14.0;      // both, regardless of the label
```

Fourteen cells fits `  Cancel (Esc)` (14). It does not fit
`[ Abbrechen (Esc) ]` (19) or `[ キャンセル (Esc) ]` (18 cells). P4c already
solved this shape for the consent dialog — `measure_run(label) + cell_w * 1.5`
— and the fix simply never reached these two files.

### 1.5 What N-4 does not rebuild

- **The status bar.** Cell-aligned by design; the Lua status format is
  column-oriented. Unchanged from P4b's reasoning.
- **A chrome font family.** N-5.
- **The widget layer.** `CLAUDE.md` records the decision that a modal over the
  panel is not a settings row, so the delete dialogs stay hand-written. N-4
  de-duplicates them; it does not migrate them into `widgets/`.
- **Menu keyboard navigation and actions.** N-4 changes where a row *is*, never
  what it does.
- **The AccessKit tree.** Neither the context menu nor the SFTP dialog has
  nodes today. That is a P6c-shaped gap, and mixing it into a geometry change
  would repeat the mistake P4c avoided by deferring the footer links to P4d.
  Recorded in §8.

---

## 2. Decisions

### D1 — One module owns the menu's geometry; five sites read it

A new `renderer/menu_layout.rs`, in the shape `renderer/tab_layout.rs` took in
N-3:

```rust
pub(crate) fn menu_width(items: &[ContextMenuItem], cell_w: f32, font: &mut FontManager) -> f32;
pub(crate) fn item_at(menu: &ContextMenu, x: f32, y: f32, cell_w: f32, cell_h: f32,
                      font: &mut FontManager) -> Option<usize>;
```

The builder, both clamps and both hit-tests call these and hold no arithmetic of
their own. Rejected alternatives:

- **Store the width on `ContextMenu`.** `state/menus.rs` has no `FontManager`,
  and a stored width goes stale on a font or DPI change. N-3 rejected the same
  idea for the same reason.
- **Publish rects from the renderer, as the tab bar does with `tab_hit_rects`.**
  It works for tabs because a tab is drawn every frame before it can be
  clicked. A context menu can be opened and clicked inside one frame, so the
  first click would test against an empty vector.

`item_at` takes `&mut FontManager`, which makes both mouse paths `&mut self` —
the same signature change P4c made to the settings hit-test, for the same
reason, with the same per-character measurement cache underneath.

### D2 — Fix the hit-test before changing the measurement

N-4a moves all five sites onto `menu_layout` while `menu_width` still computes
`visual_width(label) + visual_width(hint) + 5`. Nothing about the rendered
result changes; the phantom target and the dead column disappear. N-4b then
swaps the body of `menu_width` for `measure_run` and touches no caller.

Reviewing a defect fix and a typography change in one diff is what P4e
explicitly avoided by holding the picker colours back a PR. Same reasoning.

### D3 — The SFTP dialog derives its width; it does not declare one

The label column becomes the widest measured label plus a gap. The panel width
becomes `label_column + field_min + padding`, clamped to `sw - 4 cells` in the
manner of `build_consent_dialog_verts` (`dialog.rs:393`). 56 and 8 stop being
constants.

Row pitch and panel height stay in cells. Vertical rhythm is not what breaks.

### D4 — Fourteen new locale keys, replacing zero

Six for the SFTP dialog — `sftp-title-upload`, `sftp-title-download`,
`sftp-hint`, `sftp-field-host`, `sftp-field-local`, `sftp-field-remote` — and
eight for the context menu — `context-menu-copy`, `-paste`, `-select-all`,
`-split-vertical`, `-split-horizontal`, `-close-pane`, `-search`, `-settings` —
named to match the thirteen `context-menu-*` keys already beside them. All
fourteen in all eight locales.

The SFTP keyboard hint moves out of the title into `sftp-hint` rather than
staying concatenated into it: a title that also documents three shortcuts is not
a title, and the ramp draws the two at different steps.

The context-menu eight are in scope on the half-translated-menu argument of
§1.3, not on a width argument — at `en` they do not set the width. They land in
the same PR as the measurement because a translation can widen a label without
warning, and that PR is the one that has a gate for it.

### D5 — One delete modal, and its decoration leaves the locale data

`settings/delete_dialog.rs` owns the modal. Both tabs call it with what
actually differs: the title key, the confirm-label key, the target name, the
focus flag, and an `Option` hint key.

The four drifts resolve as follows, each toward the behaviour that is already
correct somewhere:

| Drift | Resolution |
|---|---|
| Two cancel keys | One key, `settings-dialog-cancel`, with **no brackets and no leading spaces**. The two old keys are deleted from all eight locales. |
| Cancel colour | Keybindings' behaviour — `primary` focused, `secondary` unfocused. A button that shows focus only by its background is the weaker of the two. |
| Label x offset | Neither. The label is centred in its box, so the offset ceases to exist. |
| The hint line | Kept, as an `Option`. SSH passes its key; Keybindings passes `None`, preserving today's appearance for both. Whether Keybindings *should* have a hint is a UX question, not a geometry one. |

`settings-ssh-delete-hint` keeps its leading spaces for now — it is a left-aligned
line, so they are only an indent, and stripping them is a translation change
with no geometric consequence. Noted in §8 rather than bundled here.

**Button width** follows P4c exactly: `measure_run(label, style, font) + cell_w * 1.5`,
with `n - 1` gaps between `n` buttons.

### D6 — Ramp steps

| Text | Step |
|---|---|
| Context-menu label | `body` |
| Context-menu hint | `caption` |
| SFTP title | `title` |
| SFTP keyboard hint | `caption` |
| SFTP field label / value | `body` / `body` |
| Delete-modal title | `title` |
| Delete-modal message / button label | `body` / `body_strong` |
| Delete-modal hint | `caption` |
| Tab empty-state / range indicator / note | `caption` |

Following P4b's D-2, `body_strong` maps its 600 to the existing `bold` flag
rather than requesting weight 600 from the user's terminal font.

---

## 3. Design

### 3.1 `menu_layout::menu_width`

```rust
pub(crate) fn menu_width(items, cell_w, font) -> f32 {
    let label = items.iter().map(|i| measure_run(&i.label, &BODY, font)).fold(0.0, f32::max);
    let hint  = items.iter().map(|i| measure_run(&i.hint,  &CAPTION, font)).fold(0.0, f32::max);
    (label + hint + PAD_L + GAP + PAD_R).max(MIN_W * cell_w)
}
```

`PAD_L`, `GAP`, `PAD_R` stay expressed in cells (`0.9`, `2.0`, `1.5`) so the
menu's proportions do not shift; only the text contribution is measured. The
floor stays at 16 cells — it exists so a two-item menu is not a sliver, and
that reason is independent of how text is measured.

Labels and hints are measured at their own ramp steps, because they are drawn at
them. Measuring both at `body` would put the hint column in the wrong place by
the difference between the two sizes.

### 3.2 `menu_layout::item_at`

```rust
pub(crate) fn item_at(menu, x, y, cell_w, cell_h, font) -> Option<usize> {
    let w = menu_width(&menu.items, cell_w, font);
    if x < menu.x || x > menu.x + w { return None; }
    let i = ((y - menu.y) / cell_h).floor();
    // reject negatives before the cast; a click above the menu must not wrap
    …
}
```

Both hit-tests call this. The hover path additionally maps a hit on a
`Separator` to `None`, which is what the renderer already assumes when it skips
that row's fill.

### 3.3 Truncation

None of these surfaces truncates today, and N-4 does not add truncation to the
context menu: a menu sizes itself to its content, so there is nothing to
truncate against. The SFTP field *value* is different — a long path already
overruns its box on the cell path — and it gains `truncate_run_to_width` against
the measured field width, sharing the single measurement per §5.4 of the P4
spec.

### 3.4 What moves and what does not

Nothing about menu row height, panel elevation, accent stripe, hover animation,
press pulse or dismissal changes. The context-menu work is a width change and a
hit-test change; the delete-modal work is a de-duplication plus a button-box
change; the SFTP work is a translation plus a column change.

---

## 4. Gates

Structural tests, in the manner of the P4c footer gate and the N-3b tab gate:

- **G-menu-width**: no file outside `menu_layout.rs` contains the width formula
  or the constant `18.0 * cell_w`; `mouse.rs` calls `menu_width` / `item_at`
  and computes no menu geometry itself.
- **G-menu-agree**: for a generated set of menus (short, wide, CJK labels, empty
  hints), every x inside the drawn width returns `Some(i)` for an actionable row
  and every x outside returns `None` — the invariant `mouse.rs:1724`'s comment
  claims today. Separator rows return `None` at every x, which is the one place
  the hit region is deliberately narrower than the drawn row.
- **G-i18n**: the fourteen new keys exist in all eight locale files; `picker.rs`
  and `menus.rs` contain no user-facing string literal.
- **G-delete-once**: `cell_w * 14.0` appears in neither tab; neither tab
  contains its own `draw_delete_dialog`; the two old cancel keys are absent
  from all eight locales.
- **G-decoration**: no locale value for a *button* key begins with `[`, `  ` or
  ends with `]`. This is the gate that keeps the decoration from creeping back
  in through a translation PR, which is the failure mode §1.4 describes.

Not verifiable in CI, for the reason N-3 recorded: real font metrics. The
devcontainer resolves `fc-list :lang=ja` to zero faces, so a CJK label measures
the same as Latin here. The tests assert the font-independent properties (a run
is the sum of its glyphs; measured width and hit width are the same number) and
leave "CJK is wider" to the manual pass.

---

## 5. PR breakdown

| PR | Content | Gate |
|---|---|---|
| **N-4a** | `menu_layout.rs` with the *existing* `visual_width` formula; all five sites converted; `18.0 * cell_w` deleted. Behaviour: the click defect disappears, pixels unchanged. | G-menu-width, G-menu-agree |
| **N-4b** | `menu_width` / row drawing move to `measure_run`; the eight hard-coded `new_default` labels become `context-menu-*` keys × 8 locales. Both can change the width, so they land behind the same gate. No caller changes. | G-menu-agree still holds, G-i18n (menu half) |
| **N-4c** | SFTP: six locale keys × 8, measured label column, derived and clamped panel width, value truncation. | G-i18n (SFTP half) |
| **N-4d** | `settings/delete_dialog.rs`; both tabs converted; the four drifts resolved per D5; `settings-dialog-cancel` replaces two keys in eight locales. | G-delete-once, G-decoration |
| **N-4e** | Both tabs' hand-written text (empty state, range indicator, note) onto the ramp. | — |

N-4a before N-4b is the D2 split. N-4c, N-4d and N-4e are independent of the
menu work and of each other; the order above is by decreasing user impact.

---

## 6. Verification

Per PR: `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo test`
workspace-wide.

Manual pass, once N-4e lands (no CI substitute exists for any of these):

1. Right-click in a pane with a named block present — the widest menu the app
   builds, and the one whose right 55 % is dead today. Click each row *on its
   hint*. Every row must act.
2. Open the window system menu (the narrowest, 27 cells) and click just outside
   its right border. Nothing must happen. §1.2 shows no menu is currently drawn
   narrower than the 18-cell hit region, so this is regression cover rather than
   a fix being confirmed — `menu_width`'s floor must not become reachable.
3. `ja` locale, SFTP upload: the three field labels must not overlap their
   input boxes, and the panel must stay inside a 80×24 window.
4. `de` locale, delete a keybinding and an SSH host: both button labels must sit
   inside their boxes and be centred, and both dialogs must look like each
   other.

---

## 7. Decisions to sign off

- **D1** — `menu_layout.rs` owns menu geometry; `ContextMenu` does not store a
  width and the renderer does not publish rects.
- **D2** — N-4a fixes the hit-test on the cell path; N-4b changes the
  measurement.
- **D3** — SFTP panel and label column are derived and clamped, not declared.
- **D4** — fourteen new keys across eight locales: six for SFTP (the keyboard
  hint separating from the title) and eight for `new_default`'s untranslated
  items, which today render in English beside localised neighbours.
- **D5** — one shared delete modal; cancel becomes one undecorated key; the
  focus-colour behaviour follows Keybindings; the hint stays optional.
- **D6** — ramp steps as tabulated.

## 8. Deferred

- **AccessKit nodes for the context menu and the SFTP dialog.** Neither is in
  the tree. Both are keyboard-driven, so this is a P6c-shaped announcement gap
  rather than an unreachable control — but the menu in particular is a list
  with a selection and would need `Role::Menu` / `MenuItem` modelling, which is
  more than a geometry PR should carry.
- **`settings-ssh-delete-hint`'s leading spaces**, and whether the Keybindings
  modal should have a hint line at all.
- **The status bar** (cell-aligned by design) and **a chrome font family**
  (N-5), unchanged from P4f.
