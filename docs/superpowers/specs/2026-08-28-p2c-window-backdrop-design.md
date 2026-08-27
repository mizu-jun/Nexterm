# P2c: `window.backdrop` — OS-Native Backdrop Materials

Status: proposed
Related plan: `docs/plans/ui-ux-modernization-v3.md`, section "P2 — Depth & materials (XL)" (lines 167-187), checklist item "P2c `window.backdrop` config (Win/macOS/Linux)" (line 491)
Related prior work: P2a soft shadows (#63, #64), P2b in-app acrylic (#74)

## Goal

Turn the window backdrop into a user choice. Today Nexterm applies one
hard-coded OS material on Windows and nothing anywhere else; a documented
macOS field exists but is wired to nothing. This phase introduces
`window.backdrop = auto | mica | mica-alt | acrylic | none`, maps it to the
native material on each OS, and closes P2 — the last unchecked item in the
"Depth & materials" phase.

## What exists today (measured, not assumed)

- `nexterm-client-gpu/src/platform.rs:12` — `apply_acrylic_blur` calls
  `DwmSetWindowAttribute(hwnd, 38 /* DWMWA_SYSTEMBACKDROP_TYPE */, &4u32, 4)`.
- The value `4` is **`DWMSBT_TABBEDWINDOW`, i.e. Mica Alt** — not Acrylic. The
  documented enum is `DWMSBT_AUTO=0, DWMSBT_NONE=1, DWMSBT_MAINWINDOW=2 (Mica),
  DWMSBT_TRANSIENTWINDOW=3 (Acrylic), DWMSBT_TABBEDWINDOW=4 (Mica Alt)`
  ([dwmapi.h](https://learn.microsoft.com/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type)).
  The function name, its doc comment, `nexterm-client-gpu/CLAUDE.md`, and
  `CHANGELOG.md:2898` all call it "Acrylic" and all name the constant
  `DWMWCP_ACRYLIC`, which is from the *corner preference* enum entirely. The
  plan's line 56 ("Mica Alt") is the only place that got it right.
- The call site is `renderer/event_handler/lifecycle.rs:108`, unconditional and
  primary-window-only. `spawn_os_window`
  (`renderer/event_handler/mod.rs:306`) creates secondary OS windows and never
  applies any backdrop.
- Both window-creation paths compute transparency as
  `background_opacity < 1.0` (`lifecycle.rs:38`, `mod.rs:312`). Default opacity
  is `0.95`, so the Mica Alt applied today shows through **5% of the surface**.
- `WindowConfig.macos_window_background_blur: u32` (`schema/window.rs:276`,
  default `0`) has no reader anywhere in the workspace. It is documented in
  `docs/CONFIGURATION.md:188` and `docs/ARCHITECTURE.md:427` as though it works.

## Scope

**In scope:**
- A `WindowBackdrop` enum in `nexterm-config` with a **pure, OS-parameterised
  resolver**, plus removal of the dead `macos_window_background_blur` field.
- `platform::apply_backdrop`, replacing `apply_acrylic_blur`: Windows via the
  existing `DwmSetWindowAttribute` call extended to all five values; macOS via
  `window-vibrancy`; Linux a documented no-op.
- Transparency and backdrop application on **both** window-creation paths.
- A startup warning when a backdrop is requested that the current opacity makes
  invisible.
- A Window-tab settings cycler and its eight locale strings.
- Correcting the "Acrylic"/`DWMWCP_ACRYLIC` misnomer at every site.

**Out of scope:**
- Bumping the workspace `windows-sys` from 0.59 to 0.60. Unrelated churn.
- Runtime (no-restart) backdrop switching. `decorations` already sets the
  precedent that window attributes apply at startup.
- Linux compositor blur (KDE `KWindowEffects`, Hyprland rules). The plan routes
  Linux to `none` plus P2b's in-app blur; nothing here changes that.
- Tuning any material constant against real hardware. See "Unverifiable here".

## Dependency review: `window-vibrancy`

Vetted before adoption, as the plan requires (line 302):

| Item | Measured value | Verdict |
|---|---|---|
| License | `Apache-2.0 OR MIT` | Matches Nexterm's `MIT OR Apache-2.0` |
| Maintenance | tauri-apps; 0.8.0 released 2026-07-16; 0.6/0.7/0.8 within 18 months | Active |
| `raw-window-handle` | `^0.6` | Matches winit 0.30 |
| `objc2` family | `^0.6`, macOS-target-gated | New subtree, macOS only |
| `windows-sys` | `^0.60`, Windows-target-gated | Avoided — see below |
| Linux | Unsupported by the crate, explicitly | Matches the plan's routing |

**It is adopted for macOS only.** Windows keeps Nexterm's own DWM call, for two
reasons: the crate's `apply_acrylic` falls back on Windows 10 to the
undocumented `SetWindowCompositionAttribute` path, whose resize performance
problem is documented in the crate's own README — a poor trade for a terminal
that resizes constantly — and a Windows-side adoption would pull `windows-sys`
0.60 alongside the workspace's 0.59.

The crate is declared under `[target.'cfg(target_os = "macos")'.dependencies]`,
so its Windows dependencies are never compiled. **Measured during
implementation:** `Cargo.lock` does record the union across all targets —
adding `window-vibrancy` brought in 12 new lock entries (`windows-sys 0.60.2`,
`windows-targets`, the eight per-architecture `windows_*` target crates, and
`objc2-quartz-core`), which also landed in `pkg/flatpak/cargo-sources.json`
(156 added lines) once it was regenerated. None of it is built on Linux:
`window-vibrancy` itself is gated to `cfg(target_os = "macos")`, so `cargo
check`/`test`/`clippy` never touch it or anything it depends on outside a
macOS build.

## Architecture

### 1. Config layer — a pure resolver

```rust
#[derive(..., Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum WindowBackdrop { #[default] Auto, Mica, MicaAlt, Acrylic, None }
```

`WindowDecorations` next door uses `rename_all = "lowercase"`; `mica-alt`
requires kebab-case, which leaves the other four spellings identical either way.

Resolution does **not** use `cfg!`. It takes the target OS as a parameter:

```rust
pub enum BackdropTarget { Windows, MacOs, Other }
pub enum ResolvedBackdrop { None, Mica, MicaAlt, Acrylic, Vibrancy }

pub fn resolve(backdrop: WindowBackdrop, target: BackdropTarget) -> ResolvedBackdrop
```

This is the single most important testability decision in the phase: it makes
the Windows and macOS routing tables assertable from a Linux CI runner and from
the maintainer's Linux container, where neither OS can be run.

| Config value | Windows | macOS | Linux / other |
|---|---|---|---|
| `auto` (default) | Mica Alt | None | None |
| `mica` | Mica | Vibrancy | None |
| `mica-alt` | Mica Alt | Vibrancy | None |
| `acrylic` | Acrylic | Vibrancy | None |
| `none` | None | None | None |

Two deliberate asymmetries:

- **`auto` preserves today's behaviour exactly.** On Windows that means Mica
  Alt, which is what ships now (however mislabelled). On macOS it means
  *nothing*, because `macos_window_background_blur` never did anything: making
  `auto` turn vibrancy on would change the default appearance of an OS this
  project cannot test.
- **macOS collapses all three materials onto vibrancy.** AppKit has one
  material family, not a Mica/Acrylic distinction. Mapping `mica` to "no
  backdrop" on macOS would be a silent no-op for a user who explicitly asked
  for a material; mapping it to the one material that exists is the honest
  reading of the request.

`ResolvedBackdrop::needs_transparent_window()` returns `true` for everything
except `None`.

`macos_window_background_blur` is **removed**. The TOML parser ignores unknown
keys, so existing config files keep loading; `docs/CONFIGURATION.md` gains a
line pointing at `backdrop` in its place.

### 2. Platform layer — `platform.rs`

`apply_acrylic_blur` becomes `apply_backdrop(window: &Window, resolved:
ResolvedBackdrop)`.

- **Windows.** `dwm_backdrop_value(ResolvedBackdrop) -> Option<u32>` is a plain
  function compiled on **every** platform: `None => Some(1)`, `Mica => Some(2)`,
  `Acrylic => Some(3)`, `MicaAlt => Some(4)`, and `Vibrancy => None` — the macOS
  material has no DWM equivalent, and returning `Option` states that in the type
  rather than in a comment. A `None` return skips the call. Only the `unsafe`
  `DwmSetWindowAttribute` call stays behind `#[cfg(windows)]`. The existing
  `// SAFETY:` comment is retained and extended. The doc comment is rewritten to
  name the real enum, and the Windows 11 build 22621 floor is stated (below it,
  the attribute does not exist and the call is a no-op).
- **macOS.** `window_vibrancy::apply_vibrancy(window, material, None, None)` for
  `Vibrancy`, `clear_vibrancy(window)` for `None`. The material is
  `NSVisualEffectMaterial::UnderWindowBackground`.
  **This is an unmeasured initial recipe**, in the same class as #63's
  `shadow_params` and P2b's `ACRYLIC_TINT_OPACITY`: chosen because AppKit
  documents it as the material for window backgrounds, expected to need tuning
  on real hardware rather than merely confirming.
- **Linux and everything else.** No-op, documented as such.
- Failures are logged at `warn` and swallowed. A backdrop that cannot be applied
  must never prevent the window from opening.

### 3. Window creation — both paths

- Transparency becomes `background_opacity < 1.0 ||
  resolved.needs_transparent_window()`, in `lifecycle.rs:38` and `mod.rs:312`.
  Without this, `backdrop = "acrylic"` with an opaque-configured window creates
  a non-transparent surface and the backdrop cannot appear at all.
- `apply_backdrop` is called on both the primary window and every window from
  `spawn_os_window`. Secondary windows currently get no backdrop, which is an
  inconsistency this phase removes rather than preserves.
- When `resolved != None && background_opacity >= 1.0`, log one `warn` at
  startup naming both settings. Per the approved design, **nothing is
  overridden**: the configured opacity wins, and the user is told why they see
  no material. `docs/CONFIGURATION.md` states the same constraint.

### 4. Settings panel

A `BACKDROP` cycler as row 16 of the Window tab (`WINDOW_ROW_COUNT` 16 → 17),
following `DECORATIONS` — the existing precedent for a restart-required cycler —
in `settings_window.rs`, `settings/save.rs` (`toml_edit` write-back), and all
eight locale files (`settings-window-backdrop`).

### 5. Documentation

`docs/CONFIGURATION.md` (the `backdrop` row, its value table, the opacity
caveat, the restart-scope table at line 1206, and the removal of the three
`macos_window_background_blur` mentions), `docs/ARCHITECTURE.md:427`,
`nexterm-client-gpu/CLAUDE.md`'s `platform.rs` bullet, `CHANGELOG.md`, and the
plan's P2c checkbox plus a new on-device backlog entry.

`Cargo.lock` changes, so `bash scripts/regenerate-flatpak-sources.sh` runs and
its output is committed.

## Testing

Verifiable in the maintainer's Linux container and in CI:

| Target | Method |
|---|---|
| `resolve()` | 5 values × 3 targets = 15 cases |
| `dwm_backdrop_value()` | Mapping asserted against the documented DWM constants, on any platform |
| `needs_transparent_window()` | One case per resolved variant |
| Serde | kebab-case round-trip, `auto` default, unknown value rejected, a config carrying the removed `macos_window_background_blur` key still parses |
| Settings panel | Widget-action test, following the `in_app_blur` precedent |
| Locales | The existing key-parity test |

Standard gates: `cargo clippy -- -D warnings`, `cargo fmt --check`, full suite.

### Unverifiable here — state plainly, do not imply otherwise

Neither `DwmSetWindowAttribute` nor `apply_vibrancy` is executed by any test.
The Windows and macOS CI jobs prove **that the code compiles**, nothing more.
Every appearance claim in this phase is unverified, joining the on-device
backlog rather than resolving any of it:

- Whether Mica, Mica Alt and Acrylic are visually distinguishable in Nexterm at
  all, given that the terminal paints over `background_opacity` of the surface.
- The `NSVisualEffectMaterial::UnderWindowBackground` choice.
- Whether the corrected Windows mapping changes anything a user would notice —
  `auto` resolves to the same value 4 that ships today, so it should not.
- Whether backdrop materials interact acceptably with P2b's in-app acrylic when
  both are enabled.

## Risks

- **Silent no-op on Windows 10.** `DWMWA_SYSTEMBACKDROP_TYPE` needs build 22621.
  A Windows 10 user setting `backdrop = "mica"` gets nothing and no error, since
  `DwmSetWindowAttribute` returns a failure code the current code ignores.
  Mitigation: document the floor; log the failing HRESULT at `debug`.
- **The default is a rename, not a change.** `auto` must keep resolving to Mica
  Alt on Windows. A test pins this, because "fixing" the misnomer by switching
  the default to Acrylic would change every Windows user's appearance silently.
- **macOS is written blind.** No contributor on this change can run it. The
  material constant and the `clear_vibrancy` path are reasoned, not seen.

## Delivery

Two PRs, split so each stays reviewable:

- **P2c-1** — config enum, resolver, platform layer, window creation, docs.
- **P2c-2** — settings panel row and locale strings.
