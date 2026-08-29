# P5 — Contrast everywhere & a high-contrast scheme (design spec)

Status: **draft, awaiting sign-off**
Date: 2026-08-29
Parent plan: [`ui-ux-modernization-v3.md`](./ui-ux-modernization-v3.md) § P5
Addresses: G10 (principle: Complete + Coherent)

---

## 0. Why this spec exists

The parent roadmap sizes P5 as **M** and describes it as four bullets. Measuring
the current state first — as P4's spec did — shows two of those bullets are
mis-stated and one is already shipped:

1. **"AccessKit roles/status flow from `WidgetSpec` (P1)" is already done.**
   `accessibility.rs:1450-1506` derives `Role` from `WidgetDesc.kind` and reads
   `toggled` / `value` / `disabled` / `invalid` off the descriptor. P1b/P1c
   delivered it. **Dropped from P5.**

2. **The contrast defect is systemic, not per-scheme.** P2b/P3b3 recorded it as
   "Solarized and OneDark carry contrast defects". The measurement in §1 shows
   `text_muted` fails WCAG AA on **all nine** built-in schemes at **every**
   surface level, and the Light scheme's semantic green/yellow reach as low as
   **1.18:1**.

3. **`ensure_readable` cannot deliver the acceptance criterion.** It raises
   alpha only (`settings/row.rs:41-56`); when a colour still falls short at
   `alpha = 1.0` it returns that failing colour as the "best achievable
   result". Most of the failures in §1 are hue/luminance failures, not
   translucency failures, so relocating this helper to shared utilities — the
   roadmap's first bullet — would move the code without moving a single number.

P5 therefore fixes contrast **in the token derivation**, so every built-in
scheme *and* every user custom palette is born legible, and the automated gate
becomes a property of `DesignTokens` rather than a property of call sites.

---

## 1. Baseline measurement

WCAG 2.x ratios, alpha-composited over the opaque surface, 9 built-in schemes ×
4 surface levels × 7 text-role tokens (252 cells). Legend: `AAA` ≥ 7.0, blank ≥
4.5, `!!` ≥ 3.0, `XX` < 3.0.

```
=== Dark
  s0: primary=13.64AAA secondary=8.50AAA muted=3.84!! success=14.16AAA warning=18.10AAA error=4.86   accent=2.26XX
  s1: primary=12.40AAA secondary=7.95AAA muted=3.80!! success=12.88AAA warning=16.46AAA error=4.42!! accent=2.06XX
  s2: primary=10.55AAA secondary=7.01AAA muted=3.59!! success=10.96AAA warning=14.00AAA error=3.76!! accent=1.75XX
  s3: primary=8.50AAA  secondary=5.87    muted=3.23!! success=8.83AAA  warning=11.29AAA error=3.03!! accent=1.41XX
=== Light
  s0: primary=12.47AAA secondary=6.40    muted=2.75XX success=2.20XX  warning=1.74XX  error=3.10!! accent=5.52
  s1: primary=11.24AAA secondary=6.00    muted=2.67XX success=1.98XX  warning=1.57XX  error=2.79XX accent=4.98
  s2: primary=9.85AAA  secondary=5.51    muted=2.58XX success=1.73XX  warning=1.37XX  error=2.44XX accent=4.36!!
  s3: primary=8.45AAA  secondary=4.99    muted=2.47XX success=1.49XX  warning=1.18XX  error=2.10XX accent=3.74!!
=== TokyoNight
  s0: primary=10.59AAA secondary=6.92    muted=3.46!! success=9.35AAA warning=8.55AAA error=6.46   accent=6.79
  s1: primary=9.23AAA  secondary=6.22    muted=3.28!! success=8.16AAA warning=7.45AAA error=5.63   accent=5.92
  s2: primary=7.58AAA  secondary=5.29    muted=2.99XX success=6.69    warning=6.12    error=4.62   accent=4.86
  s3: primary=5.99     secondary=4.35!!  muted=2.63XX success=5.29    warning=4.84    error=3.65!! accent=3.84!!
=== Solarized
  s0: primary=4.75     secondary=3.44!!  muted=2.13XX success=2.79XX  warning=3.37!!  error=3.26!! accent=4.75
  s1: primary=4.08!!   secondary=3.06!!  muted=2.00XX success=2.40XX  warning=2.89XX  error=2.80XX accent=4.08!!
  s2: primary=3.33!!   secondary=2.61XX  muted=1.82XX success=1.96XX  warning=2.36XX  error=2.28XX accent=3.33!!
  s3: primary=2.64XX   secondary=2.16XX  muted=1.62XX success=1.55XX  warning=1.87XX  error=1.81XX accent=2.64XX
=== Gruvbox
  s0: primary=10.75AAA secondary=7.14AAA muted=3.65!! success=7.14AAA warning=8.69AAA error=4.29!! accent=5.48
  s1: primary=9.15AAA  secondary=6.25    muted=3.37!! success=6.08    warning=7.40AAA error=3.65!! accent=4.66
  s2: primary=7.38AAA  secondary=5.22    muted=3.00XX success=4.91    warning=5.97    error=2.94XX accent=3.76!!
  s3: primary=5.80     secondary=4.25!!  muted=2.61XX success=3.86!!  warning=4.70    error=2.31XX accent=2.96XX
=== Catppuccin
  s0: primary=11.34AAA secondary=7.41AAA muted=3.68!! success=11.03AAA warning=12.91AAA error=7.08AAA accent=7.79AAA
  s1: primary=9.81AAA  secondary=6.60    muted=3.46!! success=9.54AAA  warning=11.17AAA error=6.13     accent=6.74
  s2: primary=8.01AAA  secondary=5.57    muted=3.12!! success=7.79AAA  warning=9.11AAA  error=5.00     accent=5.50
  s3: primary=6.31     secondary=4.56    muted=2.73XX success=6.14     warning=7.19AAA  error=3.94!!   accent=4.34!!
=== Dracula
  s0: primary=13.36AAA secondary=8.73AAA muted=4.29!! success=11.08AAA warning=13.63AAA error=5.23   accent=7.60AAA
  s1: primary=11.33AAA secondary=7.60AAA muted=3.93!! success=9.39AAA  warning=11.56AAA error=4.43!! accent=6.44
  s2: primary=9.13AAA  secondary=6.31    muted=3.47!! success=7.57AAA  warning=9.32AAA  error=3.57!! accent=5.19
  s3: primary=7.18AAA  secondary=5.13    muted=3.00!! success=5.95     warning=7.33AAA  error=2.81XX accent=4.08!!
=== Nord
  s0: primary=9.25AAA  secondary=6.31    muted=3.40!! success=6.13     warning=8.00AAA error=3.05!! accent=4.64
  s1: primary=7.77AAA  secondary=5.45    muted=3.09!! success=5.15     warning=6.72    error=2.57XX accent=3.90!!
  s2: primary=6.23     secondary=4.52    muted=2.72XX success=4.13!!   warning=5.39    error=2.06XX accent=3.13!!
  s3: primary=4.91     secondary=3.69!!  muted=2.37XX success=3.25!!   warning=4.25!!  error=1.62XX accent=2.46XX
=== OneDark
  s0: primary=6.57     secondary=4.62    muted=2.67XX success=6.94     warning=8.10AAA error=4.38!! accent=5.92
  s1: primary=5.56     secondary=4.04!!  muted=2.46XX success=5.88     warning=6.86    error=3.71!! accent=5.02
  s2: primary=4.48!!   secondary=3.38!!  muted=2.20XX success=4.73     warning=5.53    error=2.99XX accent=4.04!!
  s3: primary=3.52!!   secondary=2.77XX  muted=1.93XX success=3.72!!   warning=4.34!!  error=2.35XX accent=3.18!!
```

Reproduce with the throwaway harness described in §7.

### Reading of the matrix

| Finding | Evidence |
|---|---|
| `text_muted` (fg @ α 0.48) never reaches AA | best cell is Dracula/s0 at **4.29**; 36/36 cells fail |
| `accent_primary` is unusable as a text colour on Dark | **2.26 → 1.41** across s0..s3 |
| Light's semantic green/yellow are unusable as text | **1.18 – 2.20** (bright ANSI hues on a light ground) |
| Solarized fails body text below `surface_0` | primary **4.08 / 3.33 / 2.64** |
| Contrast decays monotonically s0 → s3 | the surface ramp lifts the ground toward the text |

---

## 2. The two decisions taken

### D1 — Fix in the token layer (confirmed 2026-08-29)

Correction happens inside `DesignTokens::from_palette`, not at draw sites. One
place fixes all nine built-ins *and* every user `CustomPalette`, and the
automated gate in §5 becomes a property of the token set rather than an audit of
99 scattered call sites. Cost accepted: **every scheme changes appearance** —
most visibly, muted text gets brighter/darker and stops being "fg at 48%".

### D2 — Semantic tokens split by role, not darkened wholesale

Measured usage of `semantic_*` across the client:

| Role | Sites | Examples |
|---|---|---|
| **Fill** (dominant) | ~24 | update/warning/error banner backgrounds (`ui_verts.rs:1281/1366/1435`), SFTP picker accent stripe (`picker.rs:172`), `danger_fill`, error borders/underlines |
| **Text** (minority) | ~6 | keychain toggle label (`dialog.rs:127`), prefilled hint (`dialog.rs:151`), settings validation lines |

Darkening the raw token to satisfy a text floor would wreck the banner fills on
every scheme — the visually dominant use. So:

- **`semantic_success` / `warning` / `error` / `info` keep their raw ANSI hue**
  and are the *fill* role. Their gate is the WCAG **non-text 3:1** floor
  against the surface they sit on.
- A **corrected text variant** is derived per surface for the text role.

The same split applies to `accent_primary`: it stays raw as the focus-ring /
stripe / selection fill, and gains a corrected text variant (Dark's 2.26:1 is a
text failure, not a focus-ring failure).

---

## 3. Design

### 3.1 The correction function

New in `nexterm-config/src/schema/tokens.rs`, public so the client and the tests
share one definition:

```rust
/// Adjust `color` until it reaches `min_ratio` WCAG contrast against the
/// opaque `bg`, in two stages, stopping at the first that succeeds.
pub fn contrast_correct(color: [f32; 4], bg: [f32; 3], min_ratio: f32) -> [f32; 4]
```

- **Stage 1 — alpha.** Raise alpha toward 1.0 (this is `ensure_readable`'s
  entire behaviour, preserved: a translucency problem is solved without
  touching the hue).
- **Stage 2 — value.** If opaque still falls short, ramp HSV `v` toward the
  extreme that contrasts with `bg` (down for a light ground, up for a dark
  one), preserving hue.
- **Stage 3 — saturation.** Needed on the *lighten* path only. Scaling RGB down
  always reaches black, so darkening never runs out of room; scaling up stops
  the moment the largest channel hits 1.0, and a saturated hue pinned at
  `v = 1` can still be dark (pure blue is `Y = 0.0722`). Past that point the
  only way up is to tint toward white, which costs saturation.

Contrast is monotone in the stage parameter, so each stage is a **12-iteration
binary search**, not a linear crawl. `from_palette` runs every frame
(`render_frame.rs:221`); the whole token set costs on the order of a few hundred
float ops, which is the same order as today's derivation.

**Guarantee and its limit.** Against a background of relative luminance `Y`, the
best achievable ratio is `max(1.05/(Y+0.05), (Y+0.05)/0.05)`. These meet at
`Y = 0.1791`, where the ceiling is **4.58:1**.

> **4.5:1 is always reachable. 7:1 is not** — any surface whose luminance lands
> near 0.18 caps out just above AA.

`contrast_correct` therefore returns its best effort and **cannot promise** an
arbitrary `min_ratio`; the guarantee lives in the gate (§5), which asserts what
the derivation actually achieves.

### 3.2 The surface ramp is left alone

> **Corrected 2026-08-29, before implementation.** An earlier draft of this
> section proposed pushing surfaces out of the `Y ∈ [0.13, 0.24]` band, on the
> reasoning that Solarized fails body text because *both* ends drift to
> mid-grey. Measuring the surfaces says otherwise, so the guard is dropped.

Relative luminance of every built-in scheme's four surfaces:

```
        Dark: s0=0.0040 s1=0.0094 s2=0.0198 s3=0.0366   fgY=0.6867
       Light: s0=0.8879 s1=0.7954 s2=0.6903 s3=0.5854   fgY=0.0252
 Tokyo Night: s0=0.0114 s1=0.0204 s2=0.0358 s3=0.0586   fgY=0.6004
   Solarized: s0=0.0199 s1=0.0314 s2=0.0498 s3=0.0760   fgY=0.2821
     Gruvbox: s0=0.0212 s1=0.0337 s2=0.0537 s3=0.0819   fgY=0.7154
  Catppuccin: s0=0.0140 s1=0.0240 s2=0.0407 s3=0.0650   fgY=0.6760
     Dracula: s0=0.0237 s1=0.0369 s2=0.0579 s3=0.0872   fgY=0.9350
        Nord: s0=0.0341 s1=0.0501 s2=0.0747 s3=0.1083   fgY=0.7272
    One Dark: s0=0.0250 s1=0.0386 s2=0.0600 s3=0.0899   fgY=0.4426
```

**No surface of any built-in scheme enters the band** — the darkest schemes top
out at Nord's 0.108 and Light bottoms out at 0.585. Solarized's defect is not a
mid-tone *ground*; it is a mid-tone **foreground** (`fgY = 0.2821`) over a dark
ground. The correction direction there is therefore *lighten*, and stage 2
alone covers it with hue and saturation intact — Solarized's text gets brighter,
not white.

Two further reasons not to add the guard:

- `surface_0` is the user's own terminal background, passed through unchanged.
  A guard could not touch it without changing what the user configured.
- On a dark scheme the ramp climbs away from `surface_0`. Pushing a banded
  surface "in the direction the scheme leans" would push it back *down*, toward
  or past `surface_0`, inverting the ramp that
  `dark_scheme_surfaces_lighten` pins.

A user custom palette *can* place a surface in the band. It needs no guard
either: the §3.1 ceiling there is 4.58:1, still above the 4.5:1 gate.

### 3.3 Token struct shape

Text-role tokens become per-surface, so a call site cannot pick a text colour
without naming the ground it is drawn on:

```rust
/// Which layered chrome surface a run of text sits on.
pub enum SurfaceLevel { S0, S1, S2, S3 }

/// Text-role colours corrected for one surface level.
pub struct TextTokens {
    pub primary: [f32; 4],
    pub secondary: [f32; 4],
    pub muted: [f32; 4],
    pub accent: [f32; 4],
    pub success: [f32; 4],
    pub warning: [f32; 4],
    pub error: [f32; 4],
    pub info: [f32; 4],
}

impl DesignTokens {
    /// Text colours guaranteed legible on `level`.
    pub fn text_on(&self, level: SurfaceLevel) -> &TextTokens;
}
```

Fill-role tokens (`surface_*`, `border_*`, `accent_primary`, `accent_muted`,
`accent_activity`, `semantic_*`, `tab_*_bg`) stay flat and keep their current
meaning and hue.

The flat `text_primary` / `text_secondary` / `text_muted` / `text_on_accent`
fields are **removed**, not deprecated. Leaving them would let a call site keep
silently picking an uncorrected colour — precisely the class of duplication the
P1 widget layer exists to prevent. Removal makes the compiler enumerate the ~99
sites for us.

---

## 4. Built-in high-contrast scheme

New `BuiltinScheme::HighContrast` (`toml_name = "highcontrast"`, `display_name =
"High Contrast"`), modelled on the Windows High Contrast Black palette: pure
black ground, pure white foreground, and ANSI entries chosen so every one clears
**7:1** against the ground. Because the surfaces stay near-black, the 7:1 target
is reachable here even though §3.1 shows it is not reachable in general.

Touch list (all mechanical, all covered by existing tests or the compiler):
`schema/color.rs` (enum, `display_name`, `toml_name`, `all`, `from_toml_name`,
`palette`), `loader.rs:160` and its doc comment at `:382`, `defaults.rs:19`,
`docs/CONFIGURATION.md`, and the theme-gallery cycler in `settings_theme.rs`
(which reads `BuiltinScheme::all()`, so it picks the entry up automatically).
`display_name` returns a `&'static str` rather than going through `fl!`, so
**no locale files change**.

---

## 5. The automated gate

Replaces the roadmap's "every scheme × every token pairing must pass 4.5:1",
which as written is a 252-cell cross-product including pairings no surface ever
draws.

| Gate | Assertion |
|---|---|
| **G-text** | for every `BuiltinScheme` × every `SurfaceLevel` × every field of `TextTokens`: ≥ **4.5:1** against that surface |
| **G-fill** | for every scheme: each raw `semantic_*` and `accent_primary` ≥ **3:1** against every surface it is drawn on (WCAG non-text) |
| **G-onfill** | for every scheme: `on_surface_text(fill)` ≥ **4.5:1** against each banner/badge fill |
| **G-hc** | `HighContrast` passes G-text at **7:1** |
| **G-custom** | a property test over generated random palettes passes G-text — the real defence for user schemes |

`press_never_worsens_text_contrast_on_any_builtin_scheme`
(`color_util.rs`) carries a 10 % escape hatch added in P3b3 explicitly because
"Solarized and OneDark carry contrast defects … tracked for P5". Once G-text
holds, that arm is deleted and the test asserts a flat 4.5:1.

---

## 6. PR breakdown

| PR | Scope | Gate |
|---|---|---|
| **P5a** | `contrast_correct` + `SurfaceLevel` / `TextTokens` + G-text/G-fill/G-custom tests. Flat text fields still present, populated from `S0`, so the tree still builds. | new tests green |
| **P5b** | Remove the flat text fields; migrate all call sites to `text_on(level)`. Largest PR, entirely compiler-driven. | `cargo clippy -- -D warnings`, full suite |
| **P5c** | `BuiltinScheme::HighContrast` + config/docs touch list + G-hc. | G-hc green |
| **P5d** | Retire `settings/row.rs::ensure_readable` in favour of `contrast_correct`; keep one shared helper for *composited* grounds (hover fills over a surface) where the effective background is not a token. Delete the P3b3 escape hatch. | full suite |

P5b is the risk concentration: it is wide but mechanical, and the only PR that
can change a colour by accident rather than by design.

---

## 7. Verification

- **Measured, in CI**: §5 gates. These are the acceptance criteria.
- **Reproducing §1**: a throwaway integration test under
  `nexterm-client-gpu/tests/` that reimplements the WCAG formula against public
  `nexterm_config` types and prints the matrix. Not committed — the committed
  gates assert; the harness only reports.
- **Not covered**: on-device appearance. P5 changes every scheme's chrome
  colours, and P4's chrome typography is *itself* still visually unverified on
  master. Landing P5 on top compounds the unverified visual surface. Recommend a
  single on-device pass covering P4 + P5 before P6.

## 8. Open questions

None blocking. D1 and D2 are settled, and §3.3's field removal is a consequence
of them rather than an independent choice. §3.2 records a proposal that
measurement retired before any code was written.
