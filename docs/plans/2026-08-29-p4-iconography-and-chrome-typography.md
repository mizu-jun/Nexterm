# P4 — Iconography & chrome typography: design specification

Status: **accepted** (2026-08-29). The three design questions this spec opened
are decided in §9; nothing here is gated on further input.
Scope: UI/UX modernization v3 phase P4 (`plans/ui-ux-modernization-v3.md:215`),
addressing G8 (principles: Familiar, Personal).

This document is the design spec for both halves of P4:

- **P4a** — bundle a Fluent icon font and replace the Unicode glyphs the chrome
  currently borrows from the user's terminal font.
- **P4b** — make the chrome type ramp (`TypeRamp`, shipped unused in P1a) real,
  by giving the chrome a text path that can render at a size other than the
  terminal cell.

## 1. Why this phase is larger than the roadmap says

The roadmap sizes P4 as **M** and describes P4b as "apply the chrome type ramp
via `FontManager`". That framing does not survive contact with the code. Two
facts, both measured on `e667c12`:

1. **The type ramp has no readers.** `TypeRamp` (caption / body / body_strong /
   subtitle / title, each a `TypeStyle { size, line_height, weight }`) is
   defined in `nexterm-config/src/schema/metrics.rs:125` and scaled correctly by
   `MetricTokens::scaled`. `grep -rn "type_ramp" nexterm-client-gpu/src`
   returns **zero hits**. Nothing in the client has ever asked for a font size.

2. **The chrome has no variable-size text path.** Every chrome string is drawn
   by `add_string_verts` → `add_char_verts` (`vertex_util.rs:575`), which
   advances by exactly `cell_w` per column and rasterises through
   `FontManager::rasterize_char`, whose size is the terminal cell. The only
   variable-size rasteriser in the crate, `rasterize_scaled_text`
   (`font.rs:316`), exists for the OSC 66 Text Sizing Protocol and is called
   from one place in `render_frame.rs`. `cell_w` appears ~450 times across
   `nexterm-client-gpu/src`, including 72 times in `ui_verts.rs` and 59 in
   `overlay/dialog.rs`.

So P4b is not "wire up an existing knob". It is "add a proportional text
primitive to a renderer that has only ever had a character grid, then adopt it
somewhere". The adoption surface is what makes this dangerous, and §5 bounds it
deliberately rather than converting the chrome wholesale.

P4a is comparatively self-contained, but it is not free either: the glyph atlas
cache key cannot currently tell two fonts apart (§4.2).

## 2. Goals and non-goals

### Goals

- G-1 Chrome icons render identically on a machine with no user-installed font
  (the roadmap's stated acceptance criterion for P4).
- G-2 Icon sizes come from the 16 / 20 / 24 px steps, not from the terminal cell
  size, so icon weight stays stable when the user changes font size.
- G-3 The chrome can draw text at a `TypeStyle` from `MetricTokens.type_ramp`,
  with correct proportional advances and a measurable width.
- G-4 A bounded, listed set of chrome surfaces adopts the ramp (§5.2).
- G-5 No new Cargo dependency. Subsetting happens offline; the subset is
  committed as an asset. (This also means `Cargo.lock` does not change, so the
  flatpak `cargo-sources.json` regeneration rule does not fire.)

### Non-goals

- N-1 The terminal grid keeps the single user-configured font at the single cell
  size. Nothing in this phase touches `grid_verts.rs`.
- N-2 The Nerd Font pipeline for terminal-content icons (`tab_icons.rs`,
  `TabBarConfig.show_process_icon`) is untouched.
- N-3 Tab-bar labels do **not** move to the ramp. Tab widths and their hit
  regions are computed in cell multiples in `ui_verts.rs`; making label text
  proportional would change where tabs start and end and therefore what a click
  lands on. Only the two tab-bar *icons* (§5.1) change.
- N-4 Converting the whole chrome to proportional text. Explicitly deferred; see
  §8.
- N-5 A user-configurable chrome font family. The ramp selects size and weight
  from the existing chrome font resolution; family selection is out of scope.

## 3. Current state (evidence)

The glyphs P4a replaces, all verified present in the tree:

| Site | File | Current glyph |
|---|---|---|
| Settings sidebar × 9 categories | `settings/category.rs:57-66` | `▶ Aa ◐ ▢ ⊞ ⌨ ◉ ▤ ⚿` |
| Tab tear-out button | `renderer/ui_verts.rs:495` | `↗` |
| Tab close button | `renderer/ui_verts.rs:511` | `×` |
| New-tab dropdown chevron | `renderer/ui_verts.rs:701` | `▾` |
| Window buttons | `renderer/ui_verts.rs:785-787` | `─` `□` `❐` `×` |
| Cycler chevrons | `overlay/widgets/draw/controls.rs:145,156` | `‹` `›` |
| Profile entry marker | `overlay/widgets/settings_profiles.rs:231` | `λ` |

Two properties of the sidebar case matter for the design. The icon is
concatenated into the same string as the label
(`format!("  {} {}", cat.icon(), cat.label())`, `settings/sidebar.rs:187-192`)
and that string is then truncated by `truncate_to_width`. And `cat.icon()` is
called from the draw path only — `accessibility.rs` builds its node names from
`label()`, so no icon character currently reaches a screen reader. Splitting the
draw into an icon run plus a text run must keep it that way.

## 4. P4a — icon font

### 4.1 Asset and subsetting

Upstream is `microsoft/fluentui-system-icons` (MIT). Measured today:
`fonts/FluentSystemIcons-Regular.ttf` is **2,818,416 bytes** and its companion
codepoint map lists **9,708 icons**, all in the Private Use Area from `U+F101`
upward. Embedding the full face via `include_bytes!` would add 2.8 MB to every
binary to use roughly twenty glyphs, so the asset is a **committed subset**:

- `assets/fonts/NextermIcons-Regular.ttf` — the subset, committed.
- `scripts/subset-icon-font.sh` — regenerates it from a pinned upstream commit
  (**D-3**: `fb047fb395f45ccf1129f8eaee672c9dfa99152e`, the last commit to touch
  `fonts/FluentSystemIcons-Regular.ttf`, 2026-08-21) using `fonttools`'
  `pyftsubset`, reading the glyph list from a single source
  of truth (§4.3). The container's Python needs a venv for `fonttools`; the
  script creates one rather than assuming a global install.
- The script is a maintenance tool, not a build step. CI does not run it and
  `build.rs` does not exist for this; the subset changes only when the icon list
  changes, and the diff is reviewed like any other asset.

Licensing: create `THIRD-PARTY-NOTICES.md` at the repo root (the repo currently
carries only `LICENSE-MIT` and `LICENSE-APACHE`) and record the upstream MIT
text, the pinned tag, and the subsetting provenance. `deny.toml` governs Cargo
dependencies and will not see a vendored font, so the notice file is the only
control here — state that explicitly in the file so a future reader does not
assume `cargo deny` covers it.

**Codepoint collision note.** Fluent's PUA range (`U+F101`+) sits *inside* the
Nerd Font PUA range (`U+E000`–`U+F8FF`) that `tab_icons.rs` uses. The two must
never resolve through the same font. The atlas key change in §4.2 is what makes
that a structural guarantee rather than a convention.

**Font-selection note (found by CI, after P4a-2 merged into this branch).**
Requesting `Family::Name(ICON_FAMILY)` is a *preference*, not a restriction:
cosmic-text still falls back to other installed faces for a codepoint the
requested family lacks. So an icon the subset does not ship did not render
nothing — it rendered whatever some system font mapped at that codepoint. The
first version of the "missing icon draws nothing" test asserted this using
`U+F8FF` and passed in a bare container while failing on all three CI runners,
which is what surfaced it. `FontManager` now records the subset's face id and
`rasterize_icon` discards any glyph that did not come from it. The test uses
`'A'` instead: certainly absent from the subset, certainly present in any Latin
font, so it reproduces the fallback anywhere rather than depending on the
runner's font set.

### 4.2 Font role: the atlas key and the rasteriser

`GlyphKey` (`glyph_atlas.rs:58`) is `{ ch, bold, italic, wide }`. With a second
face registered in the same `FontSystem`, `U+F101` from the icon font and
`U+F101` from a user's Nerd Font would share a cache slot and whichever
rasterised first would win. The key gains a font-source discriminant:

```rust
enum FontRole { Terminal, Icon }          // extended by P4b, see §5.3
struct GlyphKey { ch, bold, italic, wide, role, size_px }
```

`size_px` is quantised (rounded to an integer px) so that a resize does not
generate an unbounded key space, and it is what lets a 16 px and a 20 px icon
coexist in the atlas. `FontManager::rasterize_char` grows a sibling that takes
an explicit family and pixel size instead of reading `self.family` and
`self.metrics`; the existing entry point keeps its signature and delegates.

Atlas capacity needs a second look: `lru_cap_from_cell` (`glyph_atlas.rs:220`)
derives the LRU cap from the cell dimensions on the assumption that every entry
is one cell. Icons at 24 px next to a 12 px cell break that assumption. The cap
formula is adjusted and its existing unit tests extended rather than replaced.

### 4.3 Icon table

One `icons.rs` module owns the name → codepoint mapping, generated from the
upstream JSON by the same script that subsets, so the font and the table cannot
drift. Proposed assignments (every name below verified to exist upstream today):

| Site | Fluent name | Size |
|---|---|---|
| Startup | `ic_fluent_play_20_regular` | 16 |
| Font | `ic_fluent_text_font_20_regular` | 16 |
| Theme | `ic_fluent_dark_theme_20_regular` | 16 |
| Window | `ic_fluent_window_20_regular` | 16 |
| Ssh | `ic_fluent_server_20_regular` | 16 |
| Keybindings | `ic_fluent_keyboard_20_regular` | 16 |
| Profiles | `ic_fluent_person_20_regular` | 16 |
| Blocks | `ic_fluent_text_bullet_list_square_20_regular` | 16 |
| Security | `ic_fluent_shield_20_regular` | 16 |
| Tab tear-out | `ic_fluent_arrow_export_16_regular` | 16 |
| Tab close | `ic_fluent_dismiss_16_regular` | 16 |
| Dropdown / cycler chevrons | `ic_fluent_chevron_down_16_regular`, `..._left_16_`, `..._right_16_` | 16 |
| Window minimise | `ic_fluent_subtract_16_regular` | 16 |
| Window maximise | `ic_fluent_maximize_16_regular` | 16 |
| Window restore | `ic_fluent_square_multiple_16_regular` | 16 |
| Window close | `ic_fluent_dismiss_16_regular` | 16 |
| Profile entry marker | `ic_fluent_window_console_20_regular` | 16 |

**D-1 — the caption buttons move with everything else.** The concern was that
the Fluent *icon* font may not match the Segoe Fluent Icons *caption-button*
shapes Windows users recognise, particularly restore. It does not gate this
phase, for a reason that decides itself: the caption buttons currently draw
`─ □ ❐ ×` out of the user's terminal font, which is precisely the failure G-1
exists to eliminate. Leaving them behind would mean shipping P4 with its stated
acceptance criterion unmet on the most visible control in the window. The four
chosen glyphs are shape-equivalent to the Windows 11 caption set (minimise = a
rule, maximise = a square, restore = overlapping squares, close = a cross), and
the residual risk is stroke weight and optical size — both tunable at draw time
without changing the icon choice. The comparison against the OS shell goes to
the on-device backlog (§6) as an explicitly untested item, not to a merge gate.

### 4.4 Drawing

A new `add_icon_verts(codepoint, x, y, size_px, color, …)` in `vertex_util.rs`,
parallel to `add_string_verts` but advancing by the icon's own box rather than
`cell_w`, and centring the icon in the slot the caller reserved. Call sites keep
their existing geometry: the sidebar reserves the same leading slot it fills
today with two spaces plus a glyph, and the tab / window buttons keep their
current square hit regions (`mouse.rs`), so **no hit-test moves in P4a**. That
is a deliberate constraint — P4a should be visually loud and behaviourally
silent.

## 5. P4b — chrome type ramp

### 5.1 The primitive

A chrome text *run*: a string drawn at a given `TypeStyle`, with proportional
advances, returning its measured width.

```rust
// vertex_util.rs
fn add_run_verts(text: &str, style: &TypeStyle, x, y, color, …) -> f32;  // → advance width
fn measure_run(text: &str, style: &TypeStyle, font: &mut FontManager) -> f32;
fn truncate_run_to_width(text: &str, style: &TypeStyle, max_w: f32, …) -> String;
```

Shaping goes through a cosmic-text `Buffer` at the style's size to obtain
per-glyph x offsets, then each glyph is rasterised and cached individually under
the extended `GlyphKey`. **Design decision:** glyphs are keyed and cached by
`char`, not by shaped glyph id. This gives correct advances and correct CJK
widths but drops kerning pairs and ligatures within a chrome run. That is
acceptable for UI labels and keeps the atlas key comparable to the existing one;
the alternative — a run-level cache like `LigatureKey` — is a much larger cache
with worse hit rates for text that changes every frame (search queries, hit
counts). Record this in the module doc so it is not "fixed" by accident.

`measure_run` must be cheap enough to call during layout. Chrome labels are
short and mostly static, so a small memoisation sits in `FontManager`.

**Correction 1 (P4b-1, as built): the memo is keyed per `(char, size, bold)`,
not per `(text, style)`.** Two reasons the per-string form was wrong. Chrome
labels share characters heavily, and some of the text measured every frame is
*live* — a search query, a `(N)` hit count — so a whole-string cache would miss
on exactly the text measured most. More importantly, the per-character key is
what makes measuring and drawing agree *by construction*: `add_run_verts` places
each glyph at the running sum of the very numbers `measure_run` adds up, so
§5.4's "no second width formula" is a structural property rather than a
convention to maintain. No invalidation hook is needed either: a font family or
size change rebuilds the whole `FontManager`, and a DPI change cannot stale an
entry because the key is the *physical* size the advance was measured at.

**Correction 2: calling this "a proportional text path" oversells it.** With a
monospace terminal font — which is every realistic configuration, and the only
thing `FontManager` resolves — every Latin advance is equal, so for Latin text
the run path lands in the same places the cell path did. What it actually buys
is that the advance is *measured at the ramp's size* instead of assumed to be
the cell, plus correct widths for CJK and for any proportional fallback face
that steps in. That is enough for the ramp to work, and it is a narrower claim
than §5.1 originally made. A test pins the monospace-equality property so this
is not re-read as a defect later.

One thing the test suite cannot pin: CJK measuring wider than Latin. It does on
a machine with a CJK face installed, but this devcontainer resolves `fc-list
:lang=ja` to zero faces, so both fall back to the same face and measure
identically. Asserting the strict inequality would make the suite depend on the
runner's installed fonts; the tests assert the font-independent property (a run
is the sum of its measured glyphs, and no glyph measures zero) instead.

### 5.2 Adoption sites (the bounded list)

Adopted in P4b:

| Surface | Ramp step | Why it is safe |
|---|---|---|
| Settings panel title | `title` | Text is centred/left-placed inside a panel whose geometry comes from `MetricTokens`; no hit region derives from its width. |
| Settings section headers | `subtitle` | Same. |
| Settings row labels and values | `body` / `body_strong` | Rows are full-width hit rects built by `WidgetSpec::place`; the text never bounds the click target. |
| Settings sidebar labels | `body` | Row rects are fixed-width. |
| Tooltip text | `caption` | The tooltip *sizes itself* from its text, so it stays self-consistent as long as sizing and drawing use the same measurement. |
| Dialog title and body | `title` / `body` | Dialog box geometry is fixed; button rects keep their current cell-derived sizes in this phase. `title` here follows the ramp's own stated intent — `metrics.rs` documents Title 28/36 as "dialog titles" and Subtitle 20/28 as "section headers inside a panel". |

**As built (P4b-2).** All six landed, with three things worth recording:

- *Row text went further than "labels and values".* Adopting only the label
  would have left one row mixing a 14 px label with cell-sized field text, so
  every text-bearing control in the widget layer moved together — label, cycler
  value, slider readout, text field, key capture, list entry, button label —
  through two shared helpers (`draw_row_run`, `draw_row_run_centred`). That is
  also what keeps a future control from silently reintroducing the cell path.
- *The tooltip needed a signature change, not just a draw change.* §5.2 called
  it safe because it "sizes itself from its text" — true, but the sizing lives
  in `place_tooltip`, a **pure, unit-tested** function with no `FontManager`.
  Sharing one measurement therefore meant passing the width in rather than
  deriving it inside: `place_tooltip` now takes `text_w` and `line_h`, and
  `measure_tooltip` is the one place callers get them.
- *Dialog **button** labels stayed on the cell path.* The titles and body text
  moved (they are drawn at a fixed `px + cell_w`, so nothing geometric depends
  on their width), but a consent dialog's buttons are laid out from their label
  widths and those widths feed click targets. Moving them is the same
  hit-region change the footer links need, and belongs with them in §8 rather
  than smuggled into a typography PR.

Not adopted (with reason):

- Tab-bar labels — N-3.
- Status bar — cell-aligned by design; the Lua status format is column-oriented.
- Command palette and host manager — row lists whose text is currently truncated
  by `truncate_to_width`; safe in principle, deferred to keep the diff
  reviewable. Follow-up in §8.
- Anything inside the grid — N-1.

### 5.3 Chrome font resolution

The ramp specifies size, line height and weight — not family. The chrome
currently draws in the user's terminal font, and P4b does not change that: the
run path resolves the same family as `rasterize_char`, at the ramp's size and
weight. `FontRole` therefore gains a `Chrome` variant that differs from
`Terminal` only in that its `size_px` and weight come from the ramp. This keeps
P4b a *typography* change, not a *font-selection* change, which is what makes it
possible to review the two independently.

**D-2 — `body_strong` and `subtitle`/`title` map their SemiBold (600) to the
existing `bold` flag rather than requesting weight 600.** `metrics.rs` specifies
600 for four of the five ramp steps, but the chrome resolves the *user's
terminal font* (§5.3), and monospace faces routinely ship Regular and Bold only.
Asking cosmic-text for 600 would give a different answer on every machine —
sometimes a real SemiBold, sometimes a snap to 400 (making `body` and
`body_strong` indistinguishable), sometimes a synthetic emboldening. `bold`
reproduces exactly what the chrome renders today, so the ramp changes *size*
predictably and leaves *weight* where it already was. `TypeStyle.weight` stays
in the config as the declared intent; the run path reads it as a boolean
threshold (`>= 600` → `bold`). Revisit if and when the chrome gains its own font
family (N-5), which is the point at which requesting a real weight becomes
predictable.

### 5.4 What breaks if this is wrong

The failure mode is not a crash — it is text that overflows its slot or a
truncation that measures with one metric and draws with another. Two guards:

- `truncate_run_to_width` and `add_run_verts` share one measurement function.
  There must be no second width formula anywhere in the run path.
- Every adopted site keeps a unit test asserting that the drawn run's measured
  width is ≤ the slot it was given, for an ASCII label, a CJK label, and a
  label containing both. `tooltip.rs` already has a CJK case (`"あい"`) to
  model this on.

## 6. Verification

CI-verifiable (must pass before each PR merges):

- `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo test` workspace-wide.
- Unit tests: the icon table is total over `SettingsCategory` and over every
  `WindowButton` variant; every codepoint in the table is inside the subset's
  covered range; `GlyphKey` inequality across `FontRole` and across `size_px`;
  run measurement and truncation (§5.4); the adjusted `lru_cap_from_cell`.

Not CI-verifiable, and therefore explicitly logged rather than claimed:

- G-1 itself. "Renders without any user-installed font" is a GPU output
  property. It joins the on-device verification backlog in
  `ui-ux-modernization-v3.md`, with the specific checks: the nine sidebar icons
  at both scale factors; caption buttons on Windows against the OS shell (O-1);
  the ramp's title/subtitle/body sizes against a CJK locale, where the chrome
  font is likeliest to substitute.

The backlog entry must say what was *not* looked at, in the style the P3 entries
already use. Do not write "verified" for anything only reasoned about.

## 7. Delivery

Four PRs, each independently revertible:

| PR | Contents | Gate |
|---|---|---|
| P4a-1 | Subset script, committed subset, `THIRD-PARTY-NOTICES.md`, font registration, `FontRole` + `GlyphKey` extension, atlas capacity fix, `add_icon_verts`. No call site changes. | Tests green; atlas tests extended. |
| P4a-2 | Replace all call sites in the §3 table, caption buttons included (D-1). Hit regions unchanged. | Icon table totality tests. |
| | **Shipped.** One correction to §3: the profile entry marker is *not* chrome. `Profile.icon` in `nexterm-config` is a user-supplied string ("emoji or ASCII"), so replacing it would override the user's own choice; it was dropped from the icon set, leaving 18 icons over 17 codepoints. Two further sites stay on Unicode deliberately: the footer's `↗ Open config.toml` and `↺ Reset category` links, whose **`visual_width` drives their hit region** in `settings_panel_hit.rs` — converting them would move a click target, which is exactly what this PR promised not to do. They need the label/icon split the sidebar received, and are listed in §8. | |
| P4b-1 | `add_run_verts` / `measure_run` / `truncate_run_to_width`, `FontRole::Chrome`, weight-as-`bold` mapping (D-2), measurement memoisation. No adoption. | §5.4 tests. |
| | **Shipped**, with two corrections to §5.1 below. | |
| P4b-2 | Adopt the ramp at the six §5.2 surfaces. | Per-site width tests. |
| | **Shipped**, with the scope notes in §5.2 below. | |

Splitting P4a-1 from P4a-2 matters: the first is pure infrastructure with no
visual change, so if something regresses visually the bisect lands on the
second.

## 8. Deferred

- Proportional chrome text at the remaining surfaces (palette, host manager,
  macro picker, notification/InfoBar once P6 adds it).
- Tab-bar label typography, which requires rebuilding tab width and hit
  computation off measured text (N-3).
- A configurable chrome font family, distinct from the terminal font (N-5).
- Filled-variant icons for selected/active states; P4 ships Regular only.
- Dialog **button** labels, and the bespoke text in the hand-written parts of
  `ssh_tab.rs` / `keybindings_tab.rs`, the command palette, the context menu
  and the status bar. The dialog buttons are the notable one: they are sized
  from their label widths and those widths reach click targets, so they need
  the same hit-region work as the footer links below.
- The settings footer's `↗ Open config.toml` and `↺ Reset category` links.
  Their hit regions are computed from the label's `visual_width` in
  `settings_panel_hit.rs`, mirrored in `overlay/settings/mod.rs`, so moving the
  glyph into an icon run means changing the hit computation in both files in
  lockstep.

### 8.1 P4c — as built (2026-08-29), and one correction

Both deferred items above shipped together. Measuring them first corrected one
of the two:

- **The dialog buttons never reached a click target.** The claim above is
  wrong, and it was never checked: `mouse.rs` does not reference
  `pending_consent` or `close_window_dialog` at all. Both dialogs are driven by
  the keyboard (`input_handler/mod.rs`) and by AccessKit
  (`accessibility.rs` — `Click`/`Focus` write `selected` / `selected_button`).
  A button label's width reaches its own box and nothing else, so moving the
  labels to the ramp was plain typography with no hit-region work: box widths
  now come from `measure_run`, padding and gaps unchanged. The row also
  reserves `n - 1` gaps rather than `n`, which had left it half a gap off
  centre.
- **The footer links were the real thing.** The geometry existed twice — the
  builder's `visual_width * cell_w` and the hit-test's copy of it, label
  formatting included — and a proportional width is not a multiple of `cell_w`,
  so the two would have drifted silently. `overlay/settings/footer.rs` now owns
  the labels, the ramp step, the measurement and the rects; the builder and the
  hit-test each make one `footer_links` call. The hit-test became `&mut self`
  so it can measure through the same `FontManager` (one cached run per mouse
  event over an open panel).

Gate: a structural test asserts neither file reconstructs a label or calls
`footer_links` more than once, and that no dialog button is sized from
`visual_width` again.

Observed while measuring, **not** fixed: the footer links have no AccessKit
node at all — `accessibility.rs` never mentions them, so a screen-reader user
cannot reach "Open config.toml" or "Reset category". That is a P6c-shaped
accessibility gap rather than a P4 typography one, and it belongs in its own
change.

Still deferred: the command palette, host manager, macro picker, the
hand-written parts of `ssh_tab.rs` / `keybindings_tab.rs`, the context menu and
the status bar; tab-bar labels (N-3); a chrome font family (N-5).

## 9. Decisions

All three questions this spec opened are decided; no PR below is gated on
further input.

- **D-1 — Caption buttons adopt the icon font in P4a-2** (§4.3). They are the
  most visible place the chrome currently borrows the user's terminal font, so
  excluding them would leave G-1 unmet where it matters most. The shape match
  against the Windows 11 caption set is close; the residual stroke-weight
  question is a draw-time tuning matter and goes to the on-device backlog as an
  untested item.
- **D-2 — SemiBold maps to the existing `bold` flag** (§5.3). The chrome draws
  in the user's terminal font, where a request for weight 600 resolves
  differently on every machine. P4b changes size predictably and leaves weight
  exactly where it renders today.
- **D-3 — The subset is pinned to a commit SHA, not a tag** (§4.1):
  `fb047fb395f45ccf1129f8eaee672c9dfa99152e`. This reverses the recommendation
  the draft carried, on evidence: `microsoft/fluentui-system-icons` tags are
  per-npm-package (`react-icons-svg-sprite-subsetting-webpack-plugin@0.0.6` and
  similar), not font releases, so a tag would name something unrelated to the
  asset being vendored. `THIRD-PARTY-NOTICES.md` records the SHA, the commit
  date, and the sha256 of the upstream TTF the subset was cut from.
