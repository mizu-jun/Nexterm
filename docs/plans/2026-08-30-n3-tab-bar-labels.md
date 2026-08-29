# N-3 — Tab-bar labels on the chrome type ramp (design spec)

Status: **draft 2026-08-30** — measured, not yet approved
Date: 2026-08-30
Parent plan: [`ui-ux-modernization-v3.md`](./ui-ux-modernization-v3.md) § P4
Predecessor: [`2026-08-29-p4-iconography-and-chrome-typography.md`](./2026-08-29-p4-iconography-and-chrome-typography.md) §§ 5.2, 8
Addresses: the last named exclusion of P4 (`N-3`)

---

## 0. Why this spec exists

P4b excluded tab-bar labels with one line — *"requires rebuilding tab width and
hit computation off measured text"* — and P4e repeated it. Measuring the tab bar
first, as P4c–P4f each did, changes the shape of the work in two directions at
once:

1. **The stated blocker is smaller than it reads.** The hit computation is
   *already* downstream of the width: the renderer publishes
   `state.tab_hit_rects` every frame and `mouse.rs` reads it. There is no
   mirrored formula to keep in step — the P4c problem does not exist here.
2. **There is a defect underneath it that has nothing to do with typography.**
   The tab bar sizes a tab by *character count* and draws its label by *display
   width*. For a CJK title those disagree by a factor of two, today, on the cell
   path. §1.2.

The correction the measurement points at: **N-3 is a bug fix that a typography
change happens to force.** Sizing text you did not measure is the defect; the
ramp is what makes it impossible to keep getting away with.

---

## 1. Baseline measurement

All line numbers are `nexterm-client-gpu/src/renderer/ui_verts.rs` at
`a4812a2` unless stated otherwise.

### 1.1 One width, seven consumers

```rust
let label_w = (label.chars().count() as f32 * cell_w + padding * 2.0)
    .min(tab_area_w - x_offset);            // :363
```

| Consumer | Uses `label_w` for |
|---|---|
| Tab pill | the rounded-rect background |
| Active accent underline | width and centring (`accent_w = label_w * progress`) |
| Top highlight | same rect |
| OSC 9;4 progress bar | `label_w * frac` |
| `tab_hit_rects` | the click / drag / context-menu region (`mouse.rs:422, 503, 1396`) |
| `tab_close_hit_rects` / `tab_tearout_hit_rects` | both buttons are inset from `x_offset + label_w` |
| The "no more room" break | `label_w < cell_w * 2.0` ends the loop |

Seven consumers, one number — which is the good news. Nothing recomputes it, so
correcting *the number* corrects all seven at once.

### 1.2 The defect: counted in characters, drawn in cells

`label_w` counts **characters**. `add_string_verts` advances by
**display width** (`vertex_util.rs:830`, `UnicodeWidthChar::width`), so a
full-width character consumes two cells.

For an ASCII title the two agree and always have. For a CJK title they do not:

| Title | `chars().count()` | drawn width | tab width |
|---|---|---|---|
| `build` | 5 | 5 cells | 5 cells + padding |
| `ビルド` | 3 | **6 cells** | **3 cells** + padding |

The consequences are all visible and all current:

- The label **draws past its own pill** and over the neighbouring tab.
- `tab_hit_rects` records the narrow width, so the overhanging half of the
  label belongs to the *next* tab: clicking the text you are reading activates
  a different pane.
- The close and tear-out buttons are placed from the narrow width, so they sit
  *inside* the drawn text.

This is not a regression the ramp introduces; it is a defect the ramp forces
into the open, because once glyphs advance by their own measured width there is
no character count that could ever be right.

### 1.3 The label is four things concatenated

```
" [3] {nerd-glyph} My title ● "
```

Assembled at :328–:360 as one `String` from up to four parts — the optional
`[N]` tab number, the optional Nerd Font process glyph
(`tab_icons::glyph_for_process`), the title, and an activity dot for an
inactive tab with output. Only the composed string is ever measured or drawn.

The Nerd Font glyph looked like the part that would not survive a naive port —
until it was checked. `FontManager::chrome_attrs` (`font.rs:413`) builds its
attributes from **`self.family`**, the same family `rasterize_char` uses
(`font.rs:445`), out of the same `FontSystem`. Chrome and terminal share one
fallback chain and differ only in size and weight (P4b §5.3 says as much). So a
glyph resolves identically on both paths, and — more decisively — `measure_run`
and `add_run_verts` consult the same `chrome_advance`, so whatever the font
answers is used for *both* the measurement and the draw. A run cannot overflow
its own measurement no matter how odd the advance is.

What is left is narrower and real: `rasterize_chrome_char` sizes a glyph's box
to `(ceil(advance), ceil(line_height))` and does **not** crop to ink
(`font.rs:346`), so a glyph whose ink overhangs its advance — which Nerd Font
icons routinely do — is clipped on the chrome path where the terminal path gave
it a whole cell. That is a *clipping* risk, not a width risk, and §2's D3 is
scoped to it.

### 1.4 Truncation is by characters too

```rust
let truncated: String = raw_title.chars().take(24).collect();   // :331
```

Twenty-four characters is 24 cells of Latin or 48 of Japanese, so the cap does
not bound the drawn width either. `truncate_run_to_width` (P4b) exists precisely
for this and is already used by the settings rows and, since P4e, the pickers.

### 1.5 What N-3 does *not* have to rebuild

The roadmap's "and hit computation" reads like the P4c footer-link problem —
two files computing the same geometry. It is not. `tab_hit_rects`,
`tab_close_hit_rects` and `tab_tearout_hit_rects` are **published by the
renderer into `ClientState` every frame** and merely read by `mouse.rs`. The
drag ghost and the insertion indicator read the same table.

So the hit regions follow the width automatically. What N-3 must do is make the
width *correct*, not make a second place agree with it.

---

## 2. Decisions

### D1 — Measure the label; never count it

One function owns a tab's width, taking the composed label and returning
measured pixels. Every consumer in §1.1 keeps reading the same variable.

### D2 — Truncate to a width budget, not to 24 characters

The budget is the room left in the tab area, and truncation goes through
`truncate_run_to_width`, so the drawn label provably fits the pill it is
measured into. This is what closes §1.2 for long titles as well as CJK ones.

### D3 — The process icon is its own run, for clipping, not for width

The Nerd Font glyph leaves the label string and is drawn as a separate run at a
measured offset. **The reason is clipping, not measurement** (§1.3): the chrome
rasteriser boxes a glyph to its advance without cropping to ink, and a Nerd Font
icon commonly overhangs. Drawing it through the icon path, which crops, keeps it
whole.

Width is *not* a reason: chrome and terminal share a family and a fallback
chain, and measurement and drawing share `chrome_advance`, so an icon left in
the label would be measured exactly as wide as it is drawn — merely, perhaps,
clipped.

The `[N]` prefix and the activity dot stay inside the label. The dot carries the
same non-risk: a missing glyph measures **zero** (`font.rs:339` keeps it at zero
deliberately), which costs the dot its space rather than overflowing the tab.
§7 asks what to do in that case.

### D4 — Tabs stay content-sized

Windows Terminal equalises tab widths and scrolls the strip when it overflows.
Nexterm sizes each tab to its content and stops drawing when it runs out of room
(§1.1, the `cell_w * 2.0` break). **N-3 does not change that.** Equal-width tabs
and a scrolling strip are a layout redesign with their own hit-region, drag and
overflow-affordance questions; folding them into a typography change would make
any visual regression unattributable — the same reasoning P6 used for copy vs
architecture.

---

## 3. Design

### 3.1 The width function

```rust
/// Width of one tab, in physical pixels: the measured label plus padding,
/// clamped to the room left in the strip.
fn tab_width(
    label: &str,
    style: &TypeStyle,
    icon_w: f32,      // 0.0 when no process icon is drawn (D3)
    padding: f32,
    room_left: f32,
    font: &mut FontManager,
) -> f32
```

Not pure — it measures — so the *clamping* half is split out into a pure
`fit_tab_width(content_w, padding, room_left) -> f32` that the tests drive
without a device, the same split `place_links` / `footer_links` took in P4c.

### 3.2 Vertical placement

Rows keep their geometry: `bar_h` is `tab_bar.height` from config and the label
centres on the run's `line_h` instead of `cell_h`, exactly as the pickers do
(`draw_picker_run`).

### 3.3 Ramp step

Body for an inactive tab, Body Strong for the active one — the distinction the
cell path drew with its `bold` flag (D-2 of the P4 spec keeps SemiBold mapped
to `bold`).

### 3.4 What the buttons do

The close and tear-out squares stay `cell_w`-sized icon slots inset from the
tab's right edge. They are icon-font draws already (P4a) and are unaffected
except that the edge they inset from is now correct.

---

## 4. Gates

| Gate | Assertion |
|---|---|
| **G-width** | the width a tab is *sized* by is the width its label is *drawn* at — one function feeding both, asserted without reference to any particular glyph's metrics. Deliberately not phrased as "a CJK tab is wide enough": CI's font stack has no real CJK face (§6), so a test that needs double-width metrics would pass for the wrong reason |
| **G-fit** | `fit_tab_width` never returns more than the room left, and a tab that cannot fit its minimum is not drawn |
| **G-hit** | the recorded `tab_hit_rects` entry equals the drawn pill for every tab in a mixed ASCII/CJK strip |
| **G-single** | the tab bar computes a tab's width in exactly one place; a grep gate over `ui_verts.rs` finds no second `chars().count() * cell_w` |
| **G-truncate** | a title longer than the strip is cut by width, and the cut label measures ≤ its budget |
| **G-icon** | with `show_process_icon = true` the icon's width is in the tab width (a tab with an icon is wider than the same tab without) |

`G-hit` is the one that would have caught §1.2, and it is worth writing even
though it looks tautological: today it fails.

---

## 5. PR breakdown

| PR | Scope | Gate |
|---|---|---|
| **N-3a** | `fit_tab_width` + the measured `tab_width`, with the pure tests. No call sites; the tab bar still counts characters. | G-fit |
| **N-3b** | Adopt both in `build_tab_bar_verts`: measured width, width-budget truncation, ramp step, `line_h` centring. The seven consumers follow the corrected number. | G-width, G-hit, G-single, G-truncate |
| **N-3c** | The process icon becomes its own run (D3). | G-icon |

N-3b is the risk concentration: it is the PR that changes what every tab looks
like and where every tab click lands.

---

## 6. Verification

- **Measured, in CI**: the §4 gates.
- **Not covered**: appearance, as with P4–P6. The specific thing to look at on
  device is a strip mixing ASCII and Japanese titles at a small window width,
  which is where §1.2 is visible today and where the fix has to be visible
  tomorrow.
- **Not measurable in CI**: real font metrics. Probing `chrome_advance` in the
  devcontainer returns the *same* advance (8.43 px at 14 px) for `A`, `あ`, `●`,
  `↗` and two Nerd Font private-use codepoints alike — a single substituting
  face answering for everything, including characters it does not have. Any gate
  that assumes double-width CJK or a missing-glyph zero would therefore pass
  vacuously here. This is why `G-width` is phrased as an equality between two
  code paths rather than as a claim about a glyph.
- **Worth checking by hand once**: clicking the right-hand third of a Japanese
  tab label before and after. Before: it activates the neighbouring tab.

---

## 7. Open questions

Two, both for the maintainer:

1. **Minimum tab width.** The loop stops drawing at `label_w < cell_w * 2.0`, a
   cell-derived threshold. Measured text has no natural cell, so the floor
   becomes either a fixed pixel minimum or "enough room for the ellipsis plus
   padding". The second is self-describing; the first is easier to reason about
   on a HiDPI display. Recommendation: the ellipsis rule, since it is the same
   rule `truncate_run_to_width` already applies inside a budget.
2. **What should a zero-advance glyph do?** Now that §1.3 has been checked, the
   activity dot cannot overflow — measurement and drawing agree by
   construction. The remaining case is a font with no `●`: `chrome_advance`
   returns 0 deliberately, so the dot would occupy no space and draw on top of
   the character beside it. The choice is between leaving that (the dot is a
   hint, and its absence is survivable) and substituting a minimum advance when
   a glyph measures zero. Recommendation: leave it, and revisit if a real font
   stack is ever seen to drop it — a substituted width would put a guessed
   number back into the one formula this phase exists to make honest.
