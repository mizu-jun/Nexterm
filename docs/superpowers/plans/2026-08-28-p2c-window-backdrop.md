# P2c `window.backdrop` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the OS window backdrop a user choice (`window.backdrop = auto | mica | mica-alt | acrylic | none`), mapped to the native material on Windows and macOS and to nothing on Linux, closing the last unchecked item of UI/UX v3 phase P2.

**Architecture:** A pure, OS-parameterised resolver in `nexterm-config` turns the config value into a `ResolvedBackdrop`; `nexterm-client-gpu/src/platform.rs` applies that to a window through `DwmSetWindowAttribute` (Windows, own code) or `window-vibrancy` (macOS, new macOS-only dependency); both window-creation paths consult it for transparency and then apply it. Keeping the resolver and the DWM value mapping free of `cfg!` is what makes the Windows and macOS routing testable from Linux.

**Tech Stack:** Rust 2024, winit 0.30, `windows-sys` 0.59 (existing), `window-vibrancy` 0.8 (new, macOS only), `toml_edit`, `nexterm-i18n` (Fluent-style JSON, 8 locales).

**Spec:** `docs/superpowers/specs/2026-08-28-p2c-window-backdrop-design.md`

## Global Constraints

- **Comments, doc-comments and commit messages: English.** Chat replies to the maintainer: Japanese. (`CLAUDE.md`, "Language Policy")
- **No `unwrap()`.** Use `?` or `expect("concrete reason")`. Propagate with `anyhow::Result`.
- **Every `unsafe` block carries a `// SAFETY:` comment.**
- **Every user-facing string goes through `nexterm_i18n::fl!` and lands in all 8 locale files** under `nexterm-i18n/locales/` (`en, ja, zh-CN, ko, de, fr, es, it` — flat `.json`, not `.ftl`).
- **Gates before any PR:** `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo test`.
- **`nexterm-client-gpu` is binary-only** (no `[lib]`). Its unit tests run with `cargo test -p nexterm-client-gpu --bins`.
- **Never invoke bare `rustfmt <file>`** — it defaults to style edition 2015 and reorders imports the opposite way from `cargo fmt`, which turns into a Windows/macOS/Linux-wide "Check formatting" CI failure. Use `cargo fmt`.
- **Whenever `Cargo.lock` changes:** run `bash scripts/regenerate-flatpak-sources.sh` and commit `pkg/flatpak/cargo-sources.json`. The flatpak CI job diffs against it. (In this devcontainer the generator's Python needs a venv.)
- **`auto` must keep resolving to Mica Alt on Windows.** It is the shipped behaviour; changing it would alter every Windows user's appearance on upgrade. Task 1 pins this with a test.

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `nexterm-config/src/schema/window.rs` | `WindowBackdrop`, `BackdropTarget`, `ResolvedBackdrop`, the resolver, the `WindowConfig.backdrop` field | 1, 2 |
| `nexterm-config/src/schema/mod.rs`, `src/lib.rs` | Re-exports of the three new types | 2 |
| `nexterm-config/tests/doc_matches_schema.rs` | Phantom-key guard extended to the removed field | 2 |
| `nexterm-client-gpu/src/platform.rs` | `dwm_backdrop_value` (pure, all platforms) + `apply_backdrop` (per-OS) | 3, 4 |
| `nexterm-client-gpu/Cargo.toml` | macOS-only `window-vibrancy` dependency | 4 |
| `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs` | Primary window: transparency, backdrop, the one-shot warning | 5 |
| `nexterm-client-gpu/src/renderer/event_handler/mod.rs` | Secondary windows: transparency, backdrop | 5 |
| `docs/CONFIGURATION.md`, `docs/ARCHITECTURE.md`, `nexterm-client-gpu/CLAUDE.md`, `CHANGELOG.md`, `docs/plans/ui-ux-modernization-v3.md` | Documentation | 2, 6 |
| `nexterm-client-gpu/src/settings/mod.rs`, `settings/window.rs`, `settings/save.rs`, `settings/row_filter.rs` | Settings-panel state, cycling, TOML write-back, search labels | 7, 8 |
| `nexterm-client-gpu/src/renderer/overlay/widgets/settings_window.rs` | The Window-tab row | 8 |
| `nexterm-i18n/locales/*.json` | 6 new keys × 8 locales | 8 |

---

# PR P2c-1 — config, platform layer, window creation, docs

## Task 1: The backdrop enum and its pure resolver

**Files:**
- Modify: `nexterm-config/src/schema/window.rs` (insert after the `window_decorations_tests` module, which ends around line 70)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum WindowBackdrop { Auto, Mica, MicaAlt, Acrylic, None }` — `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default`, serde `rename_all = "kebab-case"`, `#[default] Auto`
  - `pub enum BackdropTarget { Windows, MacOs, Other }` with `pub const fn current() -> Self`
  - `pub enum ResolvedBackdrop { None, Mica, MicaAlt, Acrylic, Vibrancy }` with `pub const fn needs_transparent_window(self) -> bool`
  - `WindowBackdrop::resolve(self, target: BackdropTarget) -> ResolvedBackdrop`

- [ ] **Step 1: Write the failing tests**

Append to `nexterm-config/src/schema/window.rs`:

```rust
#[cfg(test)]
mod window_backdrop_tests {
    use super::*;

    /// The whole routing table, spelled out. This is the only place the
    /// Windows and macOS behaviour can be asserted from a Linux machine, so it
    /// is written as data rather than as three separate tests.
    #[test]
    fn resolution_table_is_stable() {
        use BackdropTarget::*;
        use ResolvedBackdrop as R;
        use WindowBackdrop::*;

        let cases = [
            (Auto, Windows, R::MicaAlt),
            (Auto, MacOs, R::None),
            (Auto, Other, R::None),
            (Mica, Windows, R::Mica),
            (Mica, MacOs, R::Vibrancy),
            (Mica, Other, R::None),
            (MicaAlt, Windows, R::MicaAlt),
            (MicaAlt, MacOs, R::Vibrancy),
            (MicaAlt, Other, R::None),
            (Acrylic, Windows, R::Acrylic),
            (Acrylic, MacOs, R::Vibrancy),
            (Acrylic, Other, R::None),
            (None, Windows, R::None),
            (None, MacOs, R::None),
            (None, Other, R::None),
        ];
        assert_eq!(cases.len(), 15, "5 config values x 3 targets");

        for (backdrop, target, expected) in cases {
            assert_eq!(
                backdrop.resolve(target),
                expected,
                "{backdrop:?} on {target:?}"
            );
        }
    }

    /// Before P2c the client hard-coded `DWMSBT_TABBEDWINDOW`. `auto` is the
    /// default, so if it stopped resolving to Mica Alt every Windows user's
    /// window would change appearance on upgrade.
    #[test]
    fn auto_preserves_the_shipped_windows_behaviour() {
        assert_eq!(
            WindowBackdrop::Auto.resolve(BackdropTarget::Windows),
            ResolvedBackdrop::MicaAlt
        );
    }

    /// `macos_window_background_blur` never had a reader, so macOS shipped
    /// with no backdrop. `auto` must not turn one on: nobody on this project
    /// can look at the result.
    #[test]
    fn auto_preserves_the_shipped_macos_behaviour() {
        assert_eq!(
            WindowBackdrop::Auto.resolve(BackdropTarget::MacOs),
            ResolvedBackdrop::None
        );
    }

    #[test]
    fn only_none_skips_the_transparent_window() {
        assert!(!ResolvedBackdrop::None.needs_transparent_window());
        for resolved in [
            ResolvedBackdrop::Mica,
            ResolvedBackdrop::MicaAlt,
            ResolvedBackdrop::Acrylic,
            ResolvedBackdrop::Vibrancy,
        ] {
            assert!(resolved.needs_transparent_window(), "{resolved:?}");
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-config window_backdrop`
Expected: FAIL to compile — `cannot find type WindowBackdrop in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the test module, after `window_decorations_tests`:

```rust
/// OS-native window backdrop material (UI/UX v3 P2c).
///
/// Windows draws these through `DWMWA_SYSTEMBACKDROP_TYPE`; macOS has a single
/// `NSVisualEffectView` material family, so every non-`None` value resolves to
/// the same vibrancy there; Linux has no cross-compositor equivalent and
/// resolves everything to `None` (`window.in_app_blur_enabled` is the
/// in-app substitute).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum WindowBackdrop {
    /// Whatever each OS already did before this setting existed: Mica Alt on
    /// Windows, nothing on macOS and Linux.
    #[default]
    Auto,
    /// Windows Mica (`DWMSBT_MAINWINDOW`). macOS resolves this to vibrancy.
    Mica,
    /// Windows Mica Alt (`DWMSBT_TABBEDWINDOW`). macOS resolves this to
    /// vibrancy.
    MicaAlt,
    /// Windows Acrylic (`DWMSBT_TRANSIENTWINDOW`). macOS resolves this to
    /// vibrancy.
    Acrylic,
    /// No OS backdrop.
    None,
}

/// The OS a backdrop is resolved for.
///
/// Taken as a parameter rather than read from `cfg!`, so the Windows and macOS
/// routing can be asserted on any platform — including the Linux CI runners,
/// which are the only machines that run these tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropTarget {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux and everything else.
    Other,
}

impl BackdropTarget {
    /// The target this binary was compiled for.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other
        }
    }
}

/// A [`WindowBackdrop`] resolved for one OS: what the platform layer must
/// actually apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBackdrop {
    /// Apply no backdrop (and clear any previously applied one).
    None,
    /// Windows Mica.
    Mica,
    /// Windows Mica Alt.
    MicaAlt,
    /// Windows Acrylic.
    Acrylic,
    /// macOS `NSVisualEffectView` vibrancy.
    Vibrancy,
}

impl ResolvedBackdrop {
    /// Whether the window must be created transparent for this material to be
    /// visible. An opaque surface hides the backdrop entirely, no matter what
    /// DWM or AppKit was told.
    pub const fn needs_transparent_window(self) -> bool {
        !matches!(self, ResolvedBackdrop::None)
    }
}

impl WindowBackdrop {
    /// Resolve this setting for one OS. See the table in
    /// `docs/superpowers/specs/2026-08-28-p2c-window-backdrop-design.md`.
    pub const fn resolve(self, target: BackdropTarget) -> ResolvedBackdrop {
        use BackdropTarget as T;
        use WindowBackdrop as B;
        match (self, target) {
            // Linux and everything else: no native material exists.
            (_, T::Other) => ResolvedBackdrop::None,
            (B::None, _) => ResolvedBackdrop::None,
            (B::Auto, T::Windows) => ResolvedBackdrop::MicaAlt,
            (B::Auto, T::MacOs) => ResolvedBackdrop::None,
            (B::Mica, T::Windows) => ResolvedBackdrop::Mica,
            (B::MicaAlt, T::Windows) => ResolvedBackdrop::MicaAlt,
            (B::Acrylic, T::Windows) => ResolvedBackdrop::Acrylic,
            // AppKit has one material family, so an explicit request for any
            // of the three lands on the one material that exists. Mapping it
            // to `None` instead would silently ignore what the user asked for.
            (B::Mica | B::MicaAlt | B::Acrylic, T::MacOs) => ResolvedBackdrop::Vibrancy,
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-config window_backdrop`
Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
cargo fmt
cargo clippy -p nexterm-config -- -D warnings
git add nexterm-config/src/schema/window.rs
git commit -m "feat(config): resolve a window backdrop per OS, without cfg!

The resolver takes the target OS as a parameter instead of reading cfg!,
which is what lets the Windows and macOS routing be asserted from the
Linux machines that are the only ones running these tests.

auto resolves to Mica Alt on Windows and to nothing on macOS: both are
what ships today, and a default that changed either would alter an
existing user's window on upgrade."
```

---

## Task 2: Wire the field in, remove the dead one, keep the docs reconciled

`nexterm-config/tests/doc_matches_schema.rs` asserts that every key in `docs/CONFIGURATION.md`'s complete example is a real `Config` field. Removing `macos_window_background_blur` from the schema therefore *breaks that test* until the document is updated — which is the intended forcing function, so do the schema change first and watch it fail.

**Files:**
- Modify: `nexterm-config/src/schema/window.rs:274-276` (remove the field), `:336-338` (remove from `Default`), and the `WindowConfig` field list (add `backdrop`)
- Modify: `nexterm-config/src/schema/mod.rs:44` and `nexterm-config/src/lib.rs:25-32` (re-exports)
- Modify: `nexterm-config/src/loader.rs:402` (the commented example)
- Modify: `nexterm-config/src/schema/mod.rs:597` (a test asserting the removed field)
- Modify: `nexterm-config/tests/doc_matches_schema.rs:93-107`
- Modify: `docs/CONFIGURATION.md:187-206`, `:984`, `:1206`

**Interfaces:**
- Consumes: `WindowBackdrop`, `BackdropTarget`, `ResolvedBackdrop` from Task 1.
- Produces: `WindowConfig.backdrop: WindowBackdrop`; the three types re-exported from the `nexterm_config` crate root.

- [ ] **Step 1: Write the failing tests**

Append to the `window_backdrop_tests` module in `nexterm-config/src/schema/window.rs`:

```rust
    #[test]
    fn defaults_to_auto() {
        assert_eq!(WindowConfig::default().backdrop, WindowBackdrop::Auto);
    }

    #[test]
    fn parses_kebab_case_from_toml() {
        let parsed: super::super::Config = toml::from_str(
            r#"
[window]
backdrop = "mica-alt"
"#,
        )
        .expect("mica-alt must parse");
        assert_eq!(parsed.window.backdrop, WindowBackdrop::MicaAlt);
    }

    #[test]
    fn rejects_an_unknown_backdrop() {
        let parsed: Result<super::super::Config, _> = toml::from_str(
            r#"
[window]
backdrop = "frosted"
"#,
        );
        assert!(
            parsed.is_err(),
            "an unknown backdrop must be a load error, not a silent fallback"
        );
    }

    /// The removed `macos_window_background_blur` never had a reader. Config
    /// files still carrying it must keep loading — the TOML parser ignores
    /// unknown keys, and this pins that it stays that way.
    #[test]
    fn a_config_carrying_the_removed_macos_blur_key_still_loads() {
        let parsed: super::super::Config = toml::from_str(
            r#"
[window]
macos_window_background_blur = 20
background_opacity = 0.9
"#,
        )
        .expect("an old config must not become a load error");
        assert!((parsed.window.background_opacity - 0.9).abs() < f32::EPSILON);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-config window_backdrop`
Expected: FAIL to compile — `no field backdrop on type WindowConfig`.

- [ ] **Step 3: Change the schema**

In `nexterm-config/src/schema/window.rs`, inside `WindowConfig`, **delete**:

```rust
    /// macOS window blur strength (0 = none).
    #[serde(default)]
    pub macos_window_background_blur: u32,
```

and **insert** in its place:

```rust
    /// OS-native backdrop material (UI/UX v3 P2c). Replaces the never-wired
    /// `macos_window_background_blur`, which was removed in the same change.
    #[serde(default)]
    pub backdrop: WindowBackdrop,
```

In `impl Default for WindowConfig`, replace `macos_window_background_blur: 0,` with `backdrop: WindowBackdrop::default(),`.

In `nexterm-config/src/schema/mod.rs:44`, add the three types to the `window::` re-export list alongside `WindowConfig, WindowDecorations`:

```rust
    BackdropTarget, ResolvedBackdrop, TabBarConfig, WindowBackdrop, WindowConfig, WindowDecorations,
```

Do the same in the `pub use schema::{...}` list in `nexterm-config/src/lib.rs` (keep it alphabetised — `BackdropTarget` goes near `BlocksConfig`, `ResolvedBackdrop` near `RadiusTokens`, `WindowBackdrop` before `WindowConfig`).

`nexterm-config/src/schema/mod.rs:597` asserts on the removed field:

```rust
        assert_eq!(w.macos_window_background_blur, 0);
```

Replace it with the equivalent assertion on the new one, adding `WindowBackdrop`
to that test module's `use` list if it is not already in scope:

```rust
        assert_eq!(w.backdrop, WindowBackdrop::Auto);
```

In `nexterm-config/src/loader.rs:402`, replace the commented example line
`# macos_window_background_blur = 20` with `# backdrop = "mica-alt"`.

- [ ] **Step 4: Run the config tests — the doc test must now fail**

Run: `cargo test -p nexterm-config`
Expected: the four new unit tests PASS; `complete_example_uses_only_real_config_keys` **FAILS** with a message naming `macos_window_background_blur`, because `docs/CONFIGURATION.md`'s complete example still sets it.

Record that this failure appeared. It is the whole reason that test exists.

- [ ] **Step 5: Update the documentation**

In `docs/CONFIGURATION.md`, in the `### [window] — Window Appearance` table (around line 185):

- **Delete** the `| \`macos_window_background_blur\` | u32 | \`0\` | ... |` row.
- **Fix** the `background_opacity` row's default: it reads `1.0`, but `default_background_opacity()` returns `0.95`. This is a pre-existing documentation defect, corrected here because the backdrop caveat below depends on the real default.
- **Add** a `backdrop` row:

```markdown
| `backdrop` | String | `"auto"` | OS-native window backdrop material. See below — a backdrop is only visible where `background_opacity` is below `1.0` |
```

Add a values sub-section after the `decorations` one:

```markdown
#### `backdrop` Values

| Value | Windows | macOS | Linux |
|-------|---------|-------|-------|
| `"auto"` | Mica Alt (the material Nexterm has always applied) | None | None |
| `"mica"` | Mica | Vibrancy | None |
| `"mica-alt"` | Mica Alt | Vibrancy | None |
| `"acrylic"` | Acrylic | Vibrancy | None |
| `"none"` | None | None | None |

macOS has a single `NSVisualEffectView` material family, so `mica`,
`mica-alt` and `acrylic` all resolve to the same vibrancy there. Linux has no
cross-compositor equivalent; use `in_app_blur_enabled` instead, which blurs
the terminal behind overlay panels inside the app.

Windows requires Windows 11 build 22621 (22H2) or later —
`DWMWA_SYSTEMBACKDROP_TYPE` does not exist on older builds and the request is
ignored.

> **A backdrop is drawn *behind* the window, so the terminal has to let it
> through.** With `background_opacity = 1.0` the terminal paints over the
> material and nothing is visible; Nexterm logs a warning at startup when that
> combination is configured. Lower `background_opacity` to see the backdrop.
> Nexterm does not override the opacity you configured.
```

In the complete example (around line 984), replace `macos_window_background_blur = 0` with `backdrop = "auto"`.

At line 1206, extend the restart-scope row:

```markdown
| Window transparency / decorations / backdrop | On restart | `background_opacity` / `decorations` / `backdrop` are applied as window attributes at startup |
```

- [ ] **Step 6: Guard the removed key against returning**

In `nexterm-config/tests/doc_matches_schema.rs`, extend the doc comment on `removed_phantom_keys_do_not_return` from "without ever existing in the code" to also cover removed fields, and add the key to `PHANTOM`:

```rust
        // SSH.
        "socks5_proxy",
        "local_forwards",
        // Window: existed as a field but never had a reader; removed in P2c
        // and replaced by `backdrop`.
        "macos_window_background_blur",
```

- [ ] **Step 7: Run the full config suite**

Run: `cargo test -p nexterm-config`
Expected: PASS, including `complete_example_uses_only_real_config_keys` and `removed_phantom_keys_do_not_return`.

Then confirm nothing else in the workspace referenced the removed field:

Run: `grep -rn "macos_window_background_blur" --include=*.rs --include=*.md . | grep -v target`
Expected: only `docs/ARCHITECTURE.md:427` (handled in Task 6) and prose in the plan/spec.

- [ ] **Step 8: Commit**

```bash
cargo fmt
cargo clippy -p nexterm-config -- -D warnings
git add nexterm-config docs/CONFIGURATION.md
git commit -m "feat(config): add window.backdrop, remove the dead macOS blur field

macos_window_background_blur has never had a reader anywhere in the
workspace, while CONFIGURATION.md and ARCHITECTURE.md both described it as
a working setting. It is removed rather than deprecated: the TOML parser
ignores unknown keys, so config files still carrying it keep loading, and a
test pins that.

The doc_matches_schema test caught the documentation half of this on its
own, which is what it was written for. background_opacity's documented
default was wrong in the same table (1.0 against the schema's 0.95) and is
corrected here, because the backdrop caveat depends on it."
```

---

## Task 3: The Windows backdrop path

**Files:**
- Modify: `nexterm-client-gpu/src/platform.rs:1-38` (module doc + replace `apply_acrylic_blur`)

**Interfaces:**
- Consumes: `nexterm_config::ResolvedBackdrop` (Task 1).
- Produces:
  - `pub(crate) const fn dwm_backdrop_value(resolved: ResolvedBackdrop) -> Option<u32>` — compiled on every platform
  - `pub(crate) fn apply_backdrop(window: &winit::window::Window, resolved: ResolvedBackdrop)` — the macOS arm is added in Task 4

- [ ] **Step 1: Write the failing tests**

Append to `nexterm-client-gpu/src/platform.rs`:

```rust
#[cfg(test)]
mod backdrop_tests {
    use super::*;
    use nexterm_config::{BackdropTarget, ResolvedBackdrop, WindowBackdrop};

    /// Pinned against the documented `DWM_SYSTEMBACKDROP_TYPE` enum:
    /// `DWMSBT_AUTO = 0`, `DWMSBT_NONE = 1`, `DWMSBT_MAINWINDOW = 2` (Mica),
    /// `DWMSBT_TRANSIENTWINDOW = 3` (Acrylic), `DWMSBT_TABBEDWINDOW = 4`
    /// (Mica Alt).
    #[test]
    fn dwm_values_match_the_documented_enum() {
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::None), Some(1));
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::Mica), Some(2));
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::Acrylic), Some(3));
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::MicaAlt), Some(4));
    }

    /// Vibrancy is a macOS material. Returning `None` rather than a fallback
    /// number keeps "there is no DWM value for this" in the type instead of in
    /// a comment.
    #[test]
    fn vibrancy_has_no_dwm_value() {
        assert_eq!(dwm_backdrop_value(ResolvedBackdrop::Vibrancy), None);
    }

    /// The pre-P2c client hard-coded the literal `4` under the name
    /// `apply_acrylic_blur`, and the doc comment, the crate CLAUDE.md and
    /// CHANGELOG.md:2898 all called it "Acrylic". It is Mica Alt. This pins
    /// the default end-to-end so the correction cannot drift back.
    #[test]
    fn the_default_windows_backdrop_is_mica_alt() {
        let resolved = WindowBackdrop::Auto.resolve(BackdropTarget::Windows);
        assert_eq!(resolved, ResolvedBackdrop::MicaAlt);
        assert_eq!(dwm_backdrop_value(resolved), Some(4));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu --bins backdrop_tests`
Expected: FAIL to compile — `cannot find function dwm_backdrop_value`.

- [ ] **Step 3: Replace `apply_acrylic_blur`**

In `nexterm-client-gpu/src/platform.rs`, update the module doc's first bullet from

```rust
//! - `apply_acrylic_blur`: enable the Windows 11 Acrylic (frosted glass) effect.
```

to

```rust
//! - `apply_backdrop`: apply the configured OS-native window backdrop material.
```

Delete the whole `apply_acrylic_blur` function (lines 7-38) and put this in its place:

```rust
use nexterm_config::ResolvedBackdrop;

/// The `DWMWA_SYSTEMBACKDROP_TYPE` value for a resolved backdrop.
///
/// The documented enum is `DWMSBT_AUTO = 0`, `DWMSBT_NONE = 1`,
/// `DWMSBT_MAINWINDOW = 2` (Mica), `DWMSBT_TRANSIENTWINDOW = 3` (Acrylic) and
/// `DWMSBT_TABBEDWINDOW = 4` (Mica Alt). Returns `None` for
/// [`ResolvedBackdrop::Vibrancy`], which is a macOS material with no DWM
/// equivalent.
///
/// Compiled on every platform on purpose: the mapping is then testable without
/// a Windows machine, and the Windows CI job is not the only thing standing
/// between a wrong constant and a release.
pub(crate) const fn dwm_backdrop_value(resolved: ResolvedBackdrop) -> Option<u32> {
    match resolved {
        ResolvedBackdrop::None => Some(1),
        ResolvedBackdrop::Mica => Some(2),
        ResolvedBackdrop::Acrylic => Some(3),
        ResolvedBackdrop::MicaAlt => Some(4),
        ResolvedBackdrop::Vibrancy => None,
    }
}

/// Apply the OS-native backdrop material to a window.
///
/// - **Windows**: `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)`. Requires
///   Windows 11 build 22621 (22H2); on older builds the attribute does not
///   exist and the call fails harmlessly.
/// - **macOS**: `NSVisualEffectView` vibrancy, via `window-vibrancy`.
/// - **Everything else**: nothing. Linux has no cross-compositor equivalent;
///   `window.in_app_blur_enabled` (P2b) is the in-app substitute.
///
/// A backdrop that cannot be applied must never stop a window from opening, so
/// every failure here is logged and swallowed.
pub(crate) fn apply_backdrop(window: &winit::window::Window, resolved: ResolvedBackdrop) {
    #[cfg(windows)]
    apply_backdrop_windows(window, resolved);
    #[cfg(not(windows))]
    {
        // Task 4 adds the macOS arm here.
        let _ = (window, resolved);
    }
}

#[cfg(windows)]
fn apply_backdrop_windows(window: &winit::window::Window, resolved: ResolvedBackdrop) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Some(backdrop_type) = dwm_backdrop_value(resolved) else {
        return;
    };
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    // In raw-window-handle 0.6, `hwnd` is a `NonZeroIsize` (= isize).
    // In windows-sys 0.59, `HWND = *mut c_void`, so convert from isize.
    let hwnd = h.hwnd.get() as *mut ::core::ffi::c_void;

    // DWMWA_SYSTEMBACKDROP_TYPE = 38.
    // SAFETY: `hwnd` is a valid window handle obtained from winit, and
    //         `backdrop_type` is a live local `u32` for the duration of the
    //         call, matching the 4-byte size passed alongside it. The
    //         attribute only exists on Windows 11 build 22621 and later; below
    //         that the call returns a failure HRESULT, which is logged rather
    //         than acted on.
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            38,
            &backdrop_type as *const u32 as *const ::core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if hr != 0 {
        tracing::debug!(
            "DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, {backdrop_type}) returned \
             0x{hr:08x}; expected on Windows 10 and on Windows 11 before build 22621"
        );
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu --bins backdrop_tests`
Expected: PASS, 3 tests.

The old call site in `lifecycle.rs:108` now fails to compile. That is expected and is fixed in Task 5 — do **not** patch it here; instead confirm the breakage is the one you expect:

Run: `cargo check -p nexterm-client-gpu 2>&1 | grep -c "apply_acrylic_blur"`
Expected: a non-zero count, all pointing at `lifecycle.rs`.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add nexterm-client-gpu/src/platform.rs
git commit -m "feat(client): map every backdrop onto its documented DWM value

apply_acrylic_blur passed 4 to DWMWA_SYSTEMBACKDROP_TYPE. 4 is
DWMSBT_TABBEDWINDOW, i.e. Mica Alt; DWMSBT_TRANSIENTWINDOW (Acrylic) is 3.
The name, the doc comment, the crate CLAUDE.md and CHANGELOG.md:2898 all
said Acrylic, and all named the constant DWMWCP_ACRYLIC, which belongs to
the corner-preference enum.

The value mapping is a plain const fn compiled everywhere, so a wrong
constant fails on a Linux runner rather than only on Windows CI. The
failing HRESULT that older Windows returns is now logged instead of
discarded.

This leaves lifecycle.rs uncompilable; the call sites move in the next
commit."
```

---

## Task 4: The macOS backdrop path and the new dependency

**Files:**
- Modify: `nexterm-client-gpu/Cargo.toml` (new `[target.'cfg(target_os = "macos")'.dependencies]` section — the file already has `[target.'cfg(unix)'.dependencies]` at line ~69 and `[target.'cfg(windows)'.dependencies]` at ~72; put the macOS block between them)
- Modify: `nexterm-client-gpu/src/platform.rs` (the `apply_backdrop` dispatch + a macOS arm)
- Modify: `Cargo.lock`, `pkg/flatpak/cargo-sources.json` (generated)

**Interfaces:**
- Consumes: `apply_backdrop`, `dwm_backdrop_value` (Task 3).
- Produces: no new signatures — `apply_backdrop` gains its macOS behaviour.

- [ ] **Step 1: Add the dependency**

In `nexterm-client-gpu/Cargo.toml`:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
# UI/UX v3 P2c: NSVisualEffectView-backed window backdrop.
# macOS only, deliberately. The crate's Windows Acrylic path falls back on
# Windows 10 to the undocumented SetWindowCompositionAttribute, whose resize
# cost its own README documents — a bad trade for a terminal — and it wants
# windows-sys 0.60 against this workspace's 0.59. Nexterm keeps its own
# DwmSetWindowAttribute call on Windows instead.
# Licensed Apache-2.0 OR MIT, matching this workspace.
window-vibrancy = "0.8"
```

- [ ] **Step 2: Replace the dispatch arm**

In `platform.rs`, replace the body of `apply_backdrop`:

```rust
pub(crate) fn apply_backdrop(window: &winit::window::Window, resolved: ResolvedBackdrop) {
    #[cfg(windows)]
    apply_backdrop_windows(window, resolved);
    #[cfg(target_os = "macos")]
    apply_backdrop_macos(window, resolved);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (window, resolved);
    }
}

#[cfg(target_os = "macos")]
fn apply_backdrop_macos(window: &winit::window::Window, resolved: ResolvedBackdrop) {
    use window_vibrancy::{NSVisualEffectMaterial, apply_vibrancy, clear_vibrancy};

    let result = match resolved {
        ResolvedBackdrop::None => clear_vibrancy(window).map(|_| ()),
        // AppKit has a single material family, so Mica, Mica Alt and Acrylic
        // all land here (see `WindowBackdrop::resolve`).
        //
        // `UnderWindowBackground` is AppKit's material for window backgrounds.
        // It is an unmeasured initial recipe, in the same class as P2a's
        // `shadow_params` and P2b's `ACRYLIC_TINT_OPACITY`: expected to need
        // tuning against real hardware rather than merely confirming, because
        // nobody on this project can run macOS.
        _ => apply_vibrancy(
            window,
            NSVisualEffectMaterial::UnderWindowBackground,
            None,
            None,
        )
        .map(|_| ()),
    };
    if let Err(e) = result {
        tracing::warn!("failed to apply the macOS window backdrop: {e}");
    }
}
```

If `clear_vibrancy` / `apply_vibrancy` turn out to return something other than a `Result` whose `Ok` is discardable, adjust and **record what the real signatures are** in the task report — the plan's signatures come from docs.rs, not from a compile on macOS.

- [ ] **Step 3: Verify the lock-file question the spec raised**

The spec flagged this as unverified. Settle it now.

Run: `cargo check -p nexterm-client-gpu`
Then:

```bash
grep -n 'name = "window-vibrancy"' -A 12 Cargo.lock
grep -n 'name = "windows-sys"' -A 2 Cargo.lock
grep -n 'name = "objc2"' Cargo.lock
```

Expected: `window-vibrancy` appears with its dependency list. Record **whether `windows-sys 0.60` and the `objc2` crates now appear in `Cargo.lock`**, and state it plainly in the task report — "they do" and "they do not" are both fine results; an unrecorded guess is not. Nothing is built for them on Linux either way.

- [ ] **Step 4: Regenerate the vendored flatpak sources**

Run: `bash scripts/regenerate-flatpak-sources.sh`
Expected: `pkg/flatpak/cargo-sources.json` changes. If the script fails on a missing Python module, create a venv for it — this devcontainer needs one — and record the command that worked.

Run: `git diff --stat pkg/flatpak/cargo-sources.json`
Expected: a non-empty diff.

- [ ] **Step 5: Verify the build and the tests still pass**

Run: `cargo test -p nexterm-client-gpu --bins backdrop_tests`
Expected: PASS, 3 tests (unchanged — the macOS arm is not compiled here).

Run: `cargo clippy -p nexterm-client-gpu -- -D warnings 2>&1 | tail -5`
Expected: only the pre-existing `apply_acrylic_blur` errors from `lifecycle.rs`, which Task 5 fixes.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add nexterm-client-gpu/Cargo.toml nexterm-client-gpu/src/platform.rs Cargo.lock pkg/flatpak/cargo-sources.json
git commit -m "feat(client): apply NSVisualEffectView vibrancy on macOS

window-vibrancy is adopted for macOS only. Its Windows Acrylic path falls
back on Windows 10 to the undocumented SetWindowCompositionAttribute, whose
resize cost its own README documents, and it wants windows-sys 0.60 against
this workspace's 0.59; Nexterm keeps its own DWM call there. Licensed
Apache-2.0 OR MIT, matching this workspace.

The material constant is an unmeasured initial recipe. Nobody on this
project can run macOS, so this arm is written blind and compiles on CI
without ever being looked at."
```

---

## Task 5: Both window-creation paths

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs:36-38` and `:106-108`
- Modify: `nexterm-client-gpu/src/renderer/event_handler/mod.rs:311-312` and after `window.set_ime_allowed(true);` (around line 340)

**Interfaces:**
- Consumes: `platform::apply_backdrop` (Tasks 3-4), `WindowBackdrop::resolve`, `BackdropTarget::current`, `ResolvedBackdrop::needs_transparent_window` (Task 1).
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Fix the primary window**

In `lifecycle.rs`, replace:

```rust
        let win_cfg = &self.app.config.window;
        let transparent = win_cfg.background_opacity < 1.0;
        let decorations = win_cfg.decorations.wants_os_chrome();
```

with:

```rust
        let win_cfg = &self.app.config.window;
        let backdrop = win_cfg
            .backdrop
            .resolve(nexterm_config::BackdropTarget::current());
        // An OS backdrop is drawn behind the window, so it can only show
        // through a transparent surface. Requesting one is therefore itself a
        // reason to create the window transparent, independently of
        // `background_opacity`.
        let transparent = win_cfg.background_opacity < 1.0 || backdrop.needs_transparent_window();
        let decorations = win_cfg.decorations.wants_os_chrome();

        // The terminal still paints `background_opacity` over that surface, so
        // a fully opaque terminal hides the material whatever the OS was told.
        // Say so rather than silently overriding what the user configured.
        if backdrop.needs_transparent_window() && win_cfg.background_opacity >= 1.0 {
            warn!(
                "window.backdrop is set but window.background_opacity is {:.2}: the terminal \
                 paints over the material, so no backdrop will be visible. Lower \
                 background_opacity to see it.",
                win_cfg.background_opacity
            );
        }
```

Confirm `warn` is in scope in this file (`use tracing::warn;` or a `tracing::` prefix — match whatever the file already does; if neither, use `tracing::warn!`).

Then replace the application site:

```rust
        // Apply the Acrylic (frosted-glass) background (Windows 11 only).
        #[cfg(windows)]
        crate::platform::apply_acrylic_blur(&window);
```

with:

```rust
        // Apply the configured OS-native backdrop material (`window.backdrop`).
        // No-op on Linux; see `platform::apply_backdrop`.
        crate::platform::apply_backdrop(&window, backdrop);
```

- [ ] **Step 2: Fix the secondary windows**

In `mod.rs`, in `spawn_os_window`, replace:

```rust
        let win_cfg = &self.app.config.window;
        let transparent = win_cfg.background_opacity < 1.0;
```

with:

```rust
        let win_cfg = &self.app.config.window;
        let backdrop = win_cfg
            .backdrop
            .resolve(nexterm_config::BackdropTarget::current());
        let transparent = win_cfg.background_opacity < 1.0 || backdrop.needs_transparent_window();
```

Then, immediately after `window.set_ime_allowed(true);`, insert:

```rust
        // Secondary OS windows got no backdrop at all before P2c, so a second
        // window did not match the first. The opacity warning stays on the
        // primary path: it is a config problem, not a per-window one.
        crate::platform::apply_backdrop(&window, backdrop);
```

- [ ] **Step 3: Verify the whole crate compiles and the suite passes**

Run: `cargo clippy -p nexterm-client-gpu -- -D warnings`
Expected: clean.

Run: `cargo test -p nexterm-client-gpu --bins`
Expected: PASS.

Run: `grep -rn "apply_acrylic_blur" --include=*.rs . | grep -v target`
Expected: no matches.

- [ ] **Step 4: Commit**

```bash
cargo fmt
git add nexterm-client-gpu/src/renderer/event_handler/
git commit -m "feat(client): apply the configured backdrop to every window

Two gaps close here. Requesting a backdrop now makes the window transparent
on its own: previously only background_opacity < 1.0 did, so
backdrop = \"acrylic\" on an opaque-configured window produced a surface the
material could never show through. And spawn_os_window applied no backdrop
at all, so a second OS window never matched the first.

When a backdrop is configured against an opaque terminal, the mismatch is
logged once at startup. Nothing is overridden: the configured opacity wins
and the user is told why they see no material."
```

---

## Task 6: The remaining documentation

**Files:**
- Modify: `docs/ARCHITECTURE.md:427`
- Modify: `nexterm-client-gpu/CLAUDE.md` (the `platform.rs` bullet)
- Modify: `CHANGELOG.md` (Unreleased section)
- Modify: `docs/plans/ui-ux-modernization-v3.md:455`, `:491`, and the on-device backlog

- [ ] **Step 1: Fix the architecture table**

In `docs/ARCHITECTURE.md:427`, replace the `window.macos_window_background_blur` row with:

```markdown
| `window.backdrop` | String | `"auto"` |
```

- [ ] **Step 2: Fix the crate guidance**

In `nexterm-client-gpu/CLAUDE.md`, replace the `platform.rs` bullet's first sentence:

```markdown
- `platform.rs` — Platform-specific utilities. `apply_backdrop` applies the configured `window.backdrop` material: Windows via `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` (Windows 11 build 22621+; a no-op below that), macOS via `window-vibrancy`, Linux not at all. The value mapping, `dwm_backdrop_value`, is a plain `const fn` compiled on every platform so it is testable without Windows. `open_releases_url` opens the release page in the default browser.
```

- [ ] **Step 3: Add the changelog entry**

Under `## [Unreleased]` in `CHANGELOG.md`:

```markdown
### Added

- `window.backdrop` (`auto` | `mica` | `mica-alt` | `acrylic` | `none`) selects the
  OS-native window backdrop material. Windows maps it to
  `DWMWA_SYSTEMBACKDROP_TYPE` (Windows 11 build 22621+), macOS to
  `NSVisualEffectView` vibrancy via `window-vibrancy`, and Linux to nothing —
  `in_app_blur_enabled` is the in-app substitute there. Secondary OS windows now
  receive the backdrop too; before this they received none.

### Changed

- Requesting a backdrop now creates the window transparent on its own. Previously
  only `background_opacity < 1.0` did, so a backdrop on an opaque-configured
  window could never be visible.

### Fixed

- The Windows backdrop was documented, named and logged as "Acrylic" while
  actually applying Mica Alt: `apply_acrylic_blur` passed `4` to
  `DWMWA_SYSTEMBACKDROP_TYPE`, which is `DWMSBT_TABBEDWINDOW`. The default
  (`backdrop = "auto"`) deliberately keeps applying Mica Alt, so no existing
  Windows window changes appearance — only the naming was wrong.
- `docs/CONFIGURATION.md` documented `background_opacity`'s default as `1.0`; the
  schema default is `0.95`.

### Removed

- `window.macos_window_background_blur`, which had no reader anywhere in the
  workspace while both `CONFIGURATION.md` and `ARCHITECTURE.md` described it as a
  working setting. Config files still carrying the key keep loading — unknown keys
  are ignored — and `window.backdrop` replaces it.
```

- [ ] **Step 4: Update the plan**

In `docs/plans/ui-ux-modernization-v3.md`:

Line 455 — the inventory PR shipped as #73 and the checkbox was never ticked:

```markdown
- [x] CONFIGURATION.md inventory PR — shipped via #73. `Config` has 36 fields; the
      document covered 13 and described ten keys that never existed in the code.
      `nexterm-config/tests/doc_matches_schema.rs` now fails if a documented key
      is not a real field, which is how P2c's removal of
      `macos_window_background_blur` was caught
```

Line 491:

```markdown
- [x] P2c `window.backdrop` config (Win/macOS/Linux) — shipped via P2c-1/P2c-2.
      Closes P2. The Windows material was mislabelled Acrylic throughout while
      applying Mica Alt (`DWMSBT_TABBEDWINDOW`); `auto` keeps applying Mica Alt so
      the correction changes no appearance. macOS resolves every non-`none` value
      to one `NSVisualEffectView` material, since AppKit has no Mica/Acrylic
      distinction; Linux resolves everything to `none`
```

Append to the on-device verification backlog, after the `#74` entry:

```markdown
  - P2c — the backdrop materials themselves, on both OSes that have any. Not
    measured: whether Mica, Mica Alt and Acrylic are distinguishable in Nexterm at
    all, given that the terminal paints `background_opacity` over the surface (at
    the 0.95 default only 5% of the material shows through, which may be why the
    Mica Alt that has shipped since v1.1 has never been remarked on); the macOS
    `NSVisualEffectMaterial::UnderWindowBackground` choice, which is an unmeasured
    initial recipe of exactly the same kind as #63's `shadow_params` and #74's
    `ACRYLIC_TINT_OPACITY`; whether an OS backdrop and P2b's in-app acrylic look
    coherent when both are on. What *is* machine-verified: the five-value routing
    table across three OSes, and the DWM constant for each material — the mapping
    is a plain `const fn`, so a wrong constant fails on a Linux runner rather than
    surviving to a Windows release.
```

- [ ] **Step 5: Verify the doc tests still pass and open the PR**

Run: `cargo test -p nexterm-config --test doc_matches_schema`
Expected: PASS.

Run: `cargo clippy -- -D warnings && cargo fmt --check && cargo test`
Expected: all green.

- [ ] **Step 6: Commit and open PR P2c-1**

```bash
git add docs CHANGELOG.md nexterm-client-gpu/CLAUDE.md
git commit -m "docs: record window.backdrop, and the Acrylic/Mica Alt correction"
git push -u origin p2c-window-backdrop
```

Open the PR with an English title and body (repo policy for anything targeting
`master`). Use `env -u GH_TOKEN -u GITHUB_TOKEN gh ...` so the personal account is
used, and confirm with `gh api user` first.

---

# PR P2c-2 — settings panel

## Task 7: Panel state, cycling and TOML write-back

**Files:**
- Modify: `nexterm-client-gpu/src/settings/mod.rs:253` (field) and `:416` (initialiser)
- Modify: `nexterm-client-gpu/src/settings/window.rs` (after `window_decorations_toml_key`, around line 241)
- Modify: `nexterm-client-gpu/src/settings/save.rs` (beside the `in_app_blur` write-back, around line 96)

**Interfaces:**
- Consumes: `nexterm_config::WindowBackdrop` (Task 1), `WindowConfig.backdrop` (Task 2).
- Produces:
  - `SettingsPanel.window_backdrop: nexterm_config::WindowBackdrop`
  - `next_window_backdrop(&mut self)`, `prev_window_backdrop(&mut self)`
  - `window_backdrop_label(&self) -> String`
  - `window_backdrop_toml_key(&self) -> &'static str`

- [ ] **Step 1: Write the failing test**

Append to the test module at the bottom of `nexterm-client-gpu/src/settings/window.rs` (it already contains a `decorations` cycling + write-back test around line 529 — mirror it):

```rust
    #[test]
    fn backdrop_cycles_and_writes_back() {
        let mut panel = panel();
        assert_eq!(panel.window_backdrop, nexterm_config::WindowBackdrop::Auto);

        panel.next_window_backdrop();
        assert_eq!(panel.window_backdrop, nexterm_config::WindowBackdrop::Mica);

        let toml_str = panel.apply_to_toml_string("");
        assert!(
            toml_str.contains("backdrop = \"mica\""),
            "the cycled value must reach the file: {toml_str}"
        );
    }

    #[test]
    fn backdrop_cycling_is_a_closed_loop_in_both_directions() {
        use nexterm_config::WindowBackdrop::*;
        let mut panel = panel();
        for expected in [Mica, MicaAlt, Acrylic, None, Auto] {
            panel.next_window_backdrop();
            assert_eq!(panel.window_backdrop, expected);
        }
        for expected in [None, Acrylic, MicaAlt, Mica, Auto] {
            panel.prev_window_backdrop();
            assert_eq!(panel.window_backdrop, expected);
        }
    }
```

Check how the existing test builds its panel (the helper is called `panel()` in that module) and use the same helper.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu --bins backdrop_cycles`
Expected: FAIL to compile — `no field window_backdrop`.

- [ ] **Step 3: Add the state**

In `nexterm-client-gpu/src/settings/mod.rs`, beside `pub window_decorations` (line 253):

```rust
    /// `[window].backdrop` (UI/UX v3 P2c). Applied at the next launch, like
    /// `decorations`.
    pub window_backdrop: nexterm_config::WindowBackdrop,
```

and in the initialiser beside `window_decorations: config.window.decorations.clone(),` (line 416):

```rust
            window_backdrop: config.window.backdrop,
```

(`WindowBackdrop` is `Copy`, so no `clone()`.)

In `nexterm-client-gpu/src/settings/window.rs`, after `window_decorations_toml_key`:

```rust
    pub fn next_window_backdrop(&mut self) {
        use nexterm_config::WindowBackdrop::*;
        self.window_backdrop = match self.window_backdrop {
            Auto => Mica,
            Mica => MicaAlt,
            MicaAlt => Acrylic,
            Acrylic => None,
            None => Auto,
        };
        self.dirty = true;
    }

    pub fn prev_window_backdrop(&mut self) {
        use nexterm_config::WindowBackdrop::*;
        self.window_backdrop = match self.window_backdrop {
            Auto => None,
            Mica => Auto,
            MicaAlt => Mica,
            Acrylic => MicaAlt,
            None => Acrylic,
        };
        self.dirty = true;
    }

    pub fn window_backdrop_label(&self) -> String {
        use nexterm_config::WindowBackdrop::*;
        match self.window_backdrop {
            Auto => fl!("settings-value-backdrop-auto"),
            Mica => fl!("settings-value-backdrop-mica"),
            MicaAlt => fl!("settings-value-backdrop-mica-alt"),
            Acrylic => fl!("settings-value-backdrop-acrylic"),
            None => fl!("settings-value-backdrop-none"),
        }
    }

    pub fn window_backdrop_toml_key(&self) -> &'static str {
        use nexterm_config::WindowBackdrop::*;
        match self.window_backdrop {
            Auto => "auto",
            Mica => "mica",
            MicaAlt => "mica-alt",
            Acrylic => "acrylic",
            None => "none",
        }
    }
```

In `nexterm-client-gpu/src/settings/save.rs`, beside the P2b write-back:

```rust
        // [window].backdrop (P2c).
        doc["window"]["backdrop"] = toml_edit::value(self.window_backdrop_toml_key());
```

The locale keys used above do not exist yet, so `fl!` will return the key name at runtime; Task 8 adds them. The tests in this task assert the TOML key, not the label, so they pass either way.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu --bins backdrop_cycles`
Expected: PASS, 2 tests.

- [ ] **Step 5: Commit**

```bash
cargo fmt
cargo clippy -p nexterm-client-gpu -- -D warnings
git add nexterm-client-gpu/src/settings/
git commit -m "feat(client): cycle window.backdrop from the settings panel state

Follows the decorations cycler exactly: a five-value loop, a TOML key
distinct from the localised label, and a write-back through toml_edit that
preserves the rest of the file. The row itself lands in the next commit."
```

---

## Task 8: The Window-tab row and its eight locales

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/overlay/widgets/settings_window.rs:47-59` (row constant + count), `:89` (label), `:151` (kind), `:304` (apply)
- Modify: `nexterm-client-gpu/src/settings/row_filter.rs:63` (search labels)
- Modify: all 8 files in `nexterm-i18n/locales/`

**Interfaces:**
- Consumes: `next_window_backdrop`, `window_backdrop_label` (Task 7).
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Run the existing guard test to see it fail**

`settings_window.rs:349` already asserts `window_row_labels().len() == WINDOW_ROW_COUNT`. Bump the count first so the guard fires:

In `settings_window.rs`, add after `row::IN_APP_BLUR_STRENGTH`:

```rust
    /// OS-native window backdrop (cycler).
    pub const BACKDROP: u16 = 16;
```

and change `WINDOW_ROW_COUNT` from `16` to `17`.

Run: `cargo test -p nexterm-client-gpu --bins settings_window`
Expected: FAIL — the row-label list has 16 entries against a count of 17.

- [ ] **Step 2: Add the row everywhere it is described**

In `settings_window.rs`, in `label()`:

```rust
        row::BACKDROP => fl!("settings-window-backdrop"),
```

in `kind()`:

```rust
        row::BACKDROP => WidgetKind::Cycle {
            value: sp.window_backdrop_label(),
        },
```

in the apply-action match (beside `row::DECORATIONS => sp.next_window_decorations(),`):

```rust
            row::BACKDROP => sp.next_window_backdrop(),
```

In `nexterm-client-gpu/src/settings/row_filter.rs`, append to the `window_row_labels` vector, after the `in-app-blur-strength` entry:

```rust
            fl!("settings-window-backdrop"),
```

- [ ] **Step 3: Add the six keys to all eight locales**

Add to each file in `nexterm-i18n/locales/`, keeping each file's existing key ordering (they are sorted, so `settings-value-backdrop-*` go with the other `settings-value-*` keys and `settings-window-backdrop` with the other `settings-window-*` keys):

| Key | en | ja | de | fr | es | it | ko | zh-CN |
|---|---|---|---|---|---|---|---|---|
| `settings-window-backdrop` | `Backdrop:` | `背景マテリアル:` | `Hintergrund:` | `Arrière-plan :` | `Fondo:` | `Sfondo:` | `배경 재질:` | `背景材质:` |
| `settings-value-backdrop-auto` | `Auto` | `自動` | `Automatisch` | `Automatique` | `Automático` | `Automatico` | `자동` | `自动` |
| `settings-value-backdrop-mica` | `Mica` | `Mica` | `Mica` | `Mica` | `Mica` | `Mica` | `Mica` | `Mica` |
| `settings-value-backdrop-mica-alt` | `Mica Alt` | `Mica Alt` | `Mica Alt` | `Mica Alt` | `Mica Alt` | `Mica Alt` | `Mica Alt` | `Mica Alt` |
| `settings-value-backdrop-acrylic` | `Acrylic` | `Acrylic` | `Acrylic` | `Acrylic` | `Acrylic` | `Acrylic` | `Acrylic` | `Acrylic` |
| `settings-value-backdrop-none` | `None` | `なし` | `Keiner` | `Aucun` | `Ninguno` | `Nessuno` | `없음` | `无` |

Mica and Acrylic are Microsoft material names and stay untranslated in every
locale, the same way the `present_mode` values do.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu --bins settings_window`
Expected: PASS.

Run: `cargo test -p nexterm-i18n`
Expected: PASS — the key-parity test proves all eight files gained the same six keys.

- [ ] **Step 5: Full gates**

Run: `cargo clippy -- -D warnings && cargo fmt --check && cargo test`
Expected: all green.

- [ ] **Step 6: Commit and open PR P2c-2**

```bash
git add nexterm-client-gpu nexterm-i18n
git commit -m "feat(client): add the backdrop row to the Window settings tab

A Cycle row described once in settings_window.rs; the renderer, the
hit-test and the AccessKit tree pick it up from the descriptor, and the
sidebar search picks it up from window_row_labels. Restart-scoped, like the
decorations row beside it.

Mica and Acrylic stay untranslated in all eight locales: they are Microsoft
material names, not descriptions."
git push
```

---

## Verification summary — what these tests do and do not prove

State this plainly in both PR descriptions; do not let the green CI imply more
than it shows.

**Machine-verified:** the five-value × three-OS routing table; the DWM constant
for each material; kebab-case parsing, the `auto` default and rejection of an
unknown value; that an old config carrying `macos_window_background_blur` still
loads; that `CONFIGURATION.md` documents only real fields; that the cycler
reaches the TOML file; that all eight locales carry the six new keys.

**Not verified by anything:** every appearance claim. No test executes
`DwmSetWindowAttribute` or `apply_vibrancy`. The Windows and macOS CI jobs prove
the code compiles, nothing more. The macOS material constant was chosen from
AppKit documentation by someone who cannot run macOS. Whether the three Windows
materials are even distinguishable in Nexterm is unknown, and at the default
`background_opacity = 0.95` only 5% of any of them shows through.
