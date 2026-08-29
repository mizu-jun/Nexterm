# P3c OS Reduced-Motion Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every animation in the client stop when the OS says the user prefers reduced motion, while leaving the user a way to overrule that.

**Architecture:** `animations.enabled` becomes tri-state (`auto` / `true` / `false`) and `AnimationsConfig` gains a private, never-serialized `os_reduced_motion` flag that the platform layer sets. `effective_multiplier()` returns 0 for `No`, or for `Auto` when the OS flag is set — so the ~70 places that read `config.animations` and every animation built in P3a–P3b3 inherit the switch without a single change.

**Tech Stack:** Rust; `windows-sys 0.59` (already a dependency) on Windows; `objc2` core on macOS; no Linux detection.

**Spec:** `docs/superpowers/specs/2026-08-29-p3c-reduced-motion-design.md`

## Global Constraints

- Base branch is `p3c-reduced-motion`, forked from `master` at `ecd8b10`.
- Comments and doc-strings in **English**. Conversation and commit messages in Japanese.
- No `unwrap()`. Use `?` or `expect("reason")`.
- `cargo clippy -- -D warnings` and `cargo fmt --check` must pass before every commit.
- `cargo test` must stay green (workspace-wide; `nexterm-client-gpu` alone has 1031 tests at the base commit and has **no lib target**, so `cargo test -p nexterm-client-gpu --lib` is not a valid invocation).
- Detection may only ever **disable** motion. There is no path from `os_reduced_motion` to a larger multiplier.
- The OS-derived value must never reach `config.toml`.
- Every new user-facing string goes into **all 8 locale files** under `nexterm-i18n/locales/` via `nexterm_i18n::fl!`.
- Reduced motion means 0 ms everywhere. No animation gets a bespoke reduced variant.

---

### Task 1: Tri-state `animations.enabled` and the OS flag

**Files:**
- Modify: `nexterm-config/src/schema/animations.rs` (the whole file: new enum, new field, `effective_multiplier`, tests)
- Modify: `nexterm-client-gpu/src/animations/surface.rs:98`, `nexterm-client-gpu/src/animations/hover.rs:144`, `nexterm-client-gpu/src/animations/press.rs:81`, `nexterm-client-gpu/src/settings/open_close_animation_tests.rs:15` (test helpers that build `AnimationsConfig { enabled: false, .. }`)
- Modify: `nexterm-client-gpu/src/settings/mod.rs:446` (reads `config.animations.enabled` into a `bool` field — Task 4 reshapes this properly; here just make it compile by mapping `Yes`/`Auto` → `true`, `No` → `false`)
- Test: `nexterm-config/src/schema/animations.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `nexterm_config::AnimationsEnabled` (`Auto` | `Yes` | `No`, `Default = Auto`);
  `AnimationsConfig.enabled: AnimationsEnabled`;
  `AnimationsConfig::set_os_reduced_motion(&mut self, reduced: bool)`;
  `AnimationsConfig::os_reduced_motion(&self) -> bool`.
  Tasks 2–5 use exactly these.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `nexterm-config/src/schema/animations.rs`:

```rust
    #[test]
    fn auto_follows_the_os_when_it_asks_for_reduced_motion() {
        let mut cfg = AnimationsConfig::default();
        assert_eq!(cfg.enabled, AnimationsEnabled::Auto, "auto is the default");
        assert!(cfg.effective_multiplier() > 0.0);
        cfg.set_os_reduced_motion(true);
        assert_eq!(cfg.effective_multiplier(), 0.0);
        assert_eq!(cfg.scaled_duration_ms(200), 0);
    }

    /// The escape hatch: an explicit `true` outranks the OS. Without this the
    /// setting would be unusable for a user whose OS-wide preference is not
    /// what they want inside a terminal.
    #[test]
    fn an_explicit_yes_overrules_the_os() {
        let mut cfg = AnimationsConfig {
            enabled: AnimationsEnabled::Yes,
            intensity: AnimationIntensity::Normal,
            ..AnimationsConfig::default()
        };
        cfg.set_os_reduced_motion(true);
        assert_eq!(cfg.effective_multiplier(), 1.0);
        assert_eq!(cfg.scaled_duration_ms(200), 200);
    }

    /// Detection only ever disables. An OS that is *not* asking for reduced
    /// motion must never revive animations the user turned off.
    #[test]
    fn the_os_can_never_enable_motion() {
        let mut cfg = AnimationsConfig {
            enabled: AnimationsEnabled::No,
            intensity: AnimationIntensity::Energetic,
            ..AnimationsConfig::default()
        };
        cfg.set_os_reduced_motion(false);
        assert_eq!(cfg.effective_multiplier(), 0.0);
    }

    #[test]
    fn the_pre_p3c_boolean_spellings_still_parse() {
        let on: AnimationsConfig = toml::from_str("enabled = true").expect("bool true parses");
        assert_eq!(on.enabled, AnimationsEnabled::Yes);
        let off: AnimationsConfig = toml::from_str("enabled = false").expect("bool false parses");
        assert_eq!(off.enabled, AnimationsEnabled::No);
        let auto: AnimationsConfig = toml::from_str(r#"enabled = "auto""#).expect("auto parses");
        assert_eq!(auto.enabled, AnimationsEnabled::Auto);
        let omitted: AnimationsConfig = toml::from_str("").expect("empty parses");
        assert_eq!(omitted.enabled, AnimationsEnabled::Auto);
    }

    /// The write-back boundary. The settings panel serializes this struct's
    /// neighbours through `toml_edit`; an OS-derived value that leaked into
    /// the document would persist a setting the user never chose.
    #[test]
    fn the_os_flag_is_never_serialized() {
        let mut cfg = AnimationsConfig::default();
        cfg.set_os_reduced_motion(true);
        let text = toml::to_string(&cfg).expect("serializes");
        assert!(
            !text.contains("os_reduced_motion"),
            "OS state leaked into the document: {text}"
        );
        assert!(text.contains("auto"), "auto round-trips as a string: {text}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-config animations`
Expected: compile error — `cannot find type AnimationsEnabled in this scope`.

- [ ] **Step 3: Write the implementation**

In `nexterm-config/src/schema/animations.rs`, add the enum above `AnimationsConfig`:

```rust
/// Whether animations run (UI/UX v3 P3c).
///
/// Tri-state rather than a bool because there are three distinct user
/// intents: "do what my OS asks" (the default), "animate regardless", and
/// "never animate". The middle one is the escape hatch — an OS-wide reduced
/// motion preference is not always what someone wants inside a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnimationsEnabled {
    /// Follow the OS accessibility preference; animate where there is none.
    #[default]
    Auto,
    /// Always animate, even when the OS asks for reduced motion.
    Yes,
    /// Never animate.
    No,
}

impl<'de> Deserialize<'de> for AnimationsEnabled {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Str(String),
        }
        // Accepts the pre-P3c booleans unchanged; `"auto"` is the new spelling.
        match Raw::deserialize(d)? {
            Raw::Bool(true) => Ok(Self::Yes),
            Raw::Bool(false) => Ok(Self::No),
            Raw::Str(s) if s.eq_ignore_ascii_case("auto") => Ok(Self::Auto),
            Raw::Str(s) if s.eq_ignore_ascii_case("true") => Ok(Self::Yes),
            Raw::Str(s) if s.eq_ignore_ascii_case("false") => Ok(Self::No),
            Raw::Str(s) => Err(serde::de::Error::custom(format!(
                "animations.enabled must be true, false or \"auto\" (got {s:?})"
            ))),
        }
    }
}

impl Serialize for AnimationsEnabled {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => s.serialize_str("auto"),
            Self::Yes => s.serialize_bool(true),
            Self::No => s.serialize_bool(false),
        }
    }
}
```

Replace the `enabled` field and `default_animations_enabled`:

```rust
    /// Master switch: `auto` (follow the OS), `true`, or `false`.
    #[serde(default)]
    pub enabled: AnimationsEnabled,
    /// Animation intensity (off / subtle / normal / energetic).
    #[serde(default)]
    pub intensity: AnimationIntensity,
    /// What the OS accessibility preference last reported, written by the
    /// client's platform layer. Never read from or written to `config.toml`:
    /// it is not the user's setting, and persisting it would bake a machine's
    /// state into a file the user edits and syncs.
    #[serde(skip)]
    os_reduced_motion: bool,
```

Delete `fn default_animations_enabled()` and update `Default`:

```rust
impl Default for AnimationsConfig {
    fn default() -> Self {
        Self {
            enabled: AnimationsEnabled::default(),
            intensity: AnimationIntensity::default(),
            os_reduced_motion: false,
        }
    }
}
```

Add the accessors and rewrite the multiplier:

```rust
    /// Record what the OS accessibility preference reports. Called by the
    /// client at startup and whenever the window regains focus.
    pub fn set_os_reduced_motion(&mut self, reduced: bool) {
        self.os_reduced_motion = reduced;
    }

    /// What the OS last reported. The settings panel shows this so the
    /// `auto` row can say which way it currently resolves.
    pub fn os_reduced_motion(&self) -> bool {
        self.os_reduced_motion
    }

    /// Returns the effective multiplier (0 when motion is off).
    pub fn effective_multiplier(&self) -> f32 {
        match self.enabled {
            AnimationsEnabled::No => 0.0,
            AnimationsEnabled::Auto if self.os_reduced_motion => 0.0,
            AnimationsEnabled::Auto | AnimationsEnabled::Yes => self.intensity.multiplier(),
        }
    }
```

Export the new type from the crate root beside `AnimationIntensity` (follow the existing `pub use` line for it).

Then fix the four test helpers and the one settings read that no longer compile:

- `animations/surface.rs:98`, `animations/hover.rs:144`, `animations/press.rs:81`, `settings/open_close_animation_tests.rs:15`: change `enabled: false,` to `enabled: nexterm_config::AnimationsEnabled::No,`.
- `settings/mod.rs:446`: change `animations_enabled: config.animations.enabled,` to
  `animations_enabled: config.animations.enabled != nexterm_config::AnimationsEnabled::No,`
  with the comment `// UI/UX v3 P3c: Task 4 replaces this bool mirror with the tri-state.`
- `nexterm-config/src/schema/animations.rs:161`: the existing `assert!(parsed.animations.enabled)` becomes
  `assert_eq!(parsed.animations.enabled, AnimationsEnabled::Yes);`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-config animations`, then `cargo test` (workspace), `cargo clippy -- -D warnings`, `cargo fmt --check`.
Expected: all green. Note that `settings/mod.rs` still holds a bool mirror — that is deliberate and Task 4 fixes it.

- [ ] **Step 5: Commit**

```bash
git add nexterm-config/src/schema/animations.rs nexterm-client-gpu/src
git commit -m "feat(config): tri-state animations.enabled with an OS reduced-motion flag

auto / true / false の 3 値にし、OS 由来の状態は #[serde(skip)] の
非公開フィールドとして持つ。乗数の計算は AnimationsConfig 内に
閉じているため、約 70 箇所の読み出しとアニメーション側のコードは
変更不要。既存の enabled = true / false もそのまま読める。"
```

---

### Task 2: `platform::reduced_motion()`

**Files:**
- Modify: `nexterm-client-gpu/src/platform.rs` (add the function; the file already holds the other per-OS shims)
- Modify: `nexterm-client-gpu/Cargo.toml` (macOS dependency block at line 71)
- Test: none — see the note in Step 3

**Interfaces:**
- Produces: `crate::platform::reduced_motion() -> Option<bool>` (`None` = cannot tell / unsupported). Task 3 is its only caller.

- [ ] **Step 1: Verify the macOS assumption before writing the API**

The spec's one unverified assumption is that `objc2`'s runtime class lookup
finds `NSWorkspace` without pulling in `objc2-app-kit`. Settle it first:

```bash
cargo add objc2@0.6 --target 'cfg(target_os = "macos")' -p nexterm-client-gpu --dry-run
```

If this repo cannot build for macOS from your machine (it usually cannot —
CI is the only macOS), do **not** guess. Write the macOS arm as specified in
Step 3, mark it in your report as compiled-but-unverified, and let the macOS
CI job be the check. If CI fails on the class lookup, the fallback named in
the spec is `objc2-app-kit` behind the same `cfg`, which is a dependency
swap and not a redesign.

- [ ] **Step 2: Add the macOS dependency**

In `nexterm-client-gpu/Cargo.toml`, inside the existing
`[target.'cfg(target_os = "macos")'.dependencies]` block, below
`window-vibrancy`:

```toml
# UI/UX v3 P3c: reads NSWorkspace.accessibilityDisplayShouldReduceMotion.
# objc2 core only — the typed `objc2-app-kit` subtree is not worth pulling in
# for one BOOL, and the class is already registered because winit and
# window-vibrancy link AppKit.
# Licensed Apache-2.0 OR MIT, matching this workspace.
objc2 = "0.6"
```

- [ ] **Step 3: Write the implementation**

Append to `nexterm-client-gpu/src/platform.rs`:

```rust
/// Whether the OS asks for reduced motion (UI/UX v3 P3c).
///
/// `None` means "cannot tell": an unsupported platform, or a failed call.
/// Callers treat `None` as "not reduced", so a detection failure can only
/// ever leave motion as the user configured it.
///
/// Deliberately thin. It is one FFI call per platform with no branching
/// worth pinning, and CI cannot set an OS accessibility preference — so all
/// of the decision logic lives above it in `AnimationsConfig`, where it is
/// unit-tested.
pub(crate) fn reduced_motion() -> Option<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut enabled: i32 = 0;
        // SAFETY: `SPI_GETCLIENTAREAANIMATION` writes one BOOL through
        // `pvParam`; `enabled` is a live, correctly sized local.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut enabled as *mut i32).cast(),
                0,
            )
        };
        if ok == 0 {
            return None;
        }
        // The flag reports whether client-area animations are ENABLED, so
        // reduced motion is its negation.
        return Some(enabled == 0);
    }
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        // SAFETY: `sharedWorkspace` returns a singleton owned by AppKit, and
        // `accessibilityDisplayShouldReduceMotion` is a documented BOOL
        // property on it. Both selectors take no arguments.
        let reduced: bool = unsafe {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return None;
            }
            msg_send![workspace, accessibilityDisplayShouldReduceMotion]
        };
        return Some(reduced);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Linux: out of scope for P3c. GNOME's `enable-animations` and the
        // XDG settings portal are both plausible later, but the manual
        // `animations.enabled` setting is the documented fallback for now.
        None
    }
}
```

- [ ] **Step 4: Verify it compiles on this platform**

Run: `cargo build -p nexterm-client-gpu`, then `cargo clippy -- -D warnings` and `cargo fmt --check`.
Expected: clean. On Linux the function body is the `None` arm; the Windows
and macOS arms are checked by CI's other jobs.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src/platform.rs nexterm-client-gpu/Cargo.toml
git commit -m "feat(client): read the OS reduced-motion preference

Windows は既存の windows-sys で SPI_GETCLIENTAREAANIMATION を読む
（追加依存なし）。macOS は objc2 コアのみで NSWorkspace を叩き、
objc2-app-kit のサブツリーは入れない。Linux は None を返す。
判定不能は「抑制なし」として扱うため、検出失敗が動きを勝手に
止めることはない。"
```

**Note:** `Cargo.lock` changes here. Per `CLAUDE.md`, run
`bash scripts/regenerate-flatpak-sources.sh` and commit the regenerated
`pkg/flatpak/cargo-sources.json` in the same commit — the flatpak CI job
diffs against that file and fails on a mismatch.

---

### Task 3: Sample it at startup and on focus

**Files:**
- Modify: `nexterm-client-gpu/src/renderer/app.rs:30-53` (`NextermApp::new` — the startup sample)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/mod.rs:558-620` (a new `WindowEvent::Focused` arm beside the existing `ThemeChanged` one)
- Modify: `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs:523-545` (re-apply after a config hot-reload)
- Create: the handler body goes in `nexterm-client-gpu/src/renderer/event_handler/window.rs` if that file holds the other window-level handlers; otherwise put it in `mod.rs` beside the arm
- Test: `nexterm-client-gpu/src/renderer/app.rs` or the module you put the handler in

**Interfaces:**
- Consumes: `crate::platform::reduced_motion()`, `AnimationsConfig::set_os_reduced_motion`, `AnimationsConfig::os_reduced_motion`.
- Produces: `fn refresh_reduced_motion(&mut self) -> bool` on `EventHandler` — returns whether the value changed.

- [ ] **Step 1: Write the failing test**

The FFI call cannot run in CI, but the *hot-reload trap* can and must be
pinned: a config reload replaces `self.app.config` with a freshly
deserialized `Config` whose `os_reduced_motion` is `false`, silently turning
reduced motion back on mid-session. Put this in the same module as
`refresh_reduced_motion`:

```rust
    /// A config hot-reload builds a fresh `Config`, and a fresh
    /// `AnimationsConfig` has `os_reduced_motion = false`. Whatever applies a
    /// reloaded config must re-stamp the OS state, or editing `config.toml`
    /// silently restores animations the OS asked us to stop.
    #[test]
    fn a_reloaded_config_keeps_the_os_reduced_motion_state() {
        let mut old = nexterm_config::AnimationsConfig::default();
        old.set_os_reduced_motion(true);
        assert_eq!(old.effective_multiplier(), 0.0);

        let mut reloaded = nexterm_config::AnimationsConfig::default();
        assert!(
            reloaded.effective_multiplier() > 0.0,
            "a fresh config starts un-stamped — this is the trap"
        );
        reloaded.set_os_reduced_motion(old.os_reduced_motion());
        assert_eq!(reloaded.effective_multiplier(), 0.0);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p nexterm-client-gpu a_reloaded_config_keeps_the_os_reduced_motion_state`
Expected: FAIL — `set_os_reduced_motion` exists (Task 1) but the test is in a module that does not exist yet, so this is a compile error naming the module. If you placed the test in an existing module, it passes immediately; in that case keep it (it documents the trap) and move straight to Step 3, whose implementation is what makes it true of the real code path.

- [ ] **Step 3: Write the implementation**

In `NextermApp::new`, immediately after the `let mut state = ...` line:

```rust
        // UI/UX v3 P3c: sample the OS accessibility preference once at
        // startup. `None` (Linux, or a failed call) means "not reduced".
        let mut config = config;
        config
            .animations
            .set_os_reduced_motion(crate::platform::reduced_motion().unwrap_or(false));
```

(`config` is taken by value, so shadowing it as `mut` is the smallest change; adjust the `Ok(Self { config, .. })` below accordingly.)

On `EventHandler`, add:

```rust
    /// Re-sample the OS reduced-motion preference. Returns whether it moved.
    ///
    /// Called on focus gain rather than through a native change
    /// notification: it matches what the user actually does — open System
    /// Settings, change the preference, come back — and costs one cheap
    /// syscall instead of two separate observer mechanisms.
    pub(super) fn refresh_reduced_motion(&mut self) -> bool {
        let now = crate::platform::reduced_motion().unwrap_or(false);
        if self.app.config.animations.os_reduced_motion() == now {
            return false;
        }
        self.app.config.animations.set_os_reduced_motion(now);
        true
    }
```

Add the event arm beside `WindowEvent::ThemeChanged`:

```rust
            WindowEvent::Focused(true) => {
                if self.refresh_reduced_motion()
                    && let Some(w) = &self.window
                {
                    w.request_redraw();
                }
            }
```

And in `lifecycle.rs`, where the hot-reloaded config is applied, carry the
flag across before the new config replaces the old one:

```rust
            // UI/UX v3 P3c: a reloaded config is freshly deserialized and
            // starts un-stamped. Without this line, editing `config.toml`
            // would silently restore animations the OS asked us to stop.
            let mut new_config = new_config;
            new_config
                .animations
                .set_os_reduced_motion(self.app.config.animations.os_reduced_motion());
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu`, `cargo clippy -- -D warnings`, `cargo fmt --check`.
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src
git commit -m "feat(client): sample reduced motion at startup and on focus gain

ネイティブの変更通知は使わず、フォーカス獲得時に読み直す。
「システム設定を開いて変更 → 戻ってくる」という実際の操作を
そのまま拾えるうえ、両 OS で同じ形になる。config のホット
リロードは新しい Config を作り直すため、OS 状態の引き継ぎを
明示的に行う。"
```

---

### Task 4: The settings panel row

**Files:**
- Modify: `nexterm-client-gpu/src/settings/mod.rs:271` (`animations_enabled: bool` → the tri-state) and `:446` (remove the Task 1 stopgap mapping)
- Modify: `nexterm-client-gpu/src/settings/window_extra.rs:114-118` (`toggle_animations_enabled` → a cycler) and add the label helper beside `animations_intensity_label` at `:144`
- Modify: `nexterm-client-gpu/src/settings/window_extra.rs:26,50` (the keyboard ←/→ arms for row 9)
- Modify: `nexterm-client-gpu/src/settings/save.rs:87` (write-back)
- Modify: `nexterm-client-gpu/src/settings/reset.rs:62` (reset-to-defaults)
- Modify: `nexterm-client-gpu/src/renderer/overlay/widgets/settings_window.rs:148` (`WidgetKind::Toggle` → `WidgetKind::Cycle`) and `:302` (the activate arm)
- Modify: all 8 files in `nexterm-i18n/locales/`
- Test: `nexterm-client-gpu/src/settings/window_extra.rs` (inline tests)

**Interfaces:**
- Consumes: `nexterm_config::AnimationsEnabled` (Task 1).
- Produces: `SettingsPanel.animations_enabled: nexterm_config::AnimationsEnabled`; `SettingsPanel::next_animations_enabled(&mut self)`; `SettingsPanel::prev_animations_enabled(&mut self)`; `SettingsPanel::animations_enabled_label(&self, os_reduced: bool) -> String`; `SettingsPanel::animations_enabled_toml_value(&self) -> toml_edit::Value`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn the_animations_enabled_row_cycles_through_all_three_states() {
        use nexterm_config::AnimationsEnabled::*;
        let mut sp = SettingsPanel::default();
        sp.animations_enabled = Auto;
        sp.next_animations_enabled();
        assert_eq!(sp.animations_enabled, Yes);
        sp.next_animations_enabled();
        assert_eq!(sp.animations_enabled, No);
        sp.next_animations_enabled();
        assert_eq!(sp.animations_enabled, Auto, "wraps");
        sp.prev_animations_enabled();
        assert_eq!(sp.animations_enabled, No, "and goes back the other way");
        assert!(sp.dirty, "cycling marks the panel dirty");
    }

    /// `auto` on its own is a lie on a machine whose OS asks for reduced
    /// motion: the row would read "auto" while every animation is off. The
    /// label has to say which way it currently resolves.
    #[test]
    fn the_auto_label_reports_how_it_currently_resolves() {
        use nexterm_config::AnimationsEnabled::*;
        let mut sp = SettingsPanel::default();
        sp.animations_enabled = Auto;
        let reduced = sp.animations_enabled_label(true);
        let normal = sp.animations_enabled_label(false);
        assert_ne!(reduced, normal, "auto must distinguish the two resolutions");

        // The explicit states say the same thing whatever the OS reports.
        sp.animations_enabled = Yes;
        assert_eq!(
            sp.animations_enabled_label(true),
            sp.animations_enabled_label(false)
        );
    }

    #[test]
    fn each_state_writes_back_its_own_toml_spelling() {
        use nexterm_config::AnimationsEnabled::*;
        let mut sp = SettingsPanel::default();
        sp.animations_enabled = Auto;
        assert_eq!(sp.animations_enabled_toml_value().as_str(), Some("auto"));
        sp.animations_enabled = Yes;
        assert_eq!(sp.animations_enabled_toml_value().as_bool(), Some(true));
        sp.animations_enabled = No;
        assert_eq!(sp.animations_enabled_toml_value().as_bool(), Some(false));
    }
```

If `SettingsPanel::default()` is not how the neighbouring tests in this file build a panel, copy their construction verbatim instead.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p nexterm-client-gpu animations_enabled`
Expected: compile error — `no method named next_animations_enabled`.

- [ ] **Step 3: Write the implementation**

`settings/mod.rs:271`:

```rust
    /// `[animations].enabled` mirror (UI/UX v3 P3c: tri-state).
    pub animations_enabled: nexterm_config::AnimationsEnabled,
```

and `:446` becomes `animations_enabled: config.animations.enabled,` again (the Task 1 stopgap and its comment go away).

`settings/window_extra.rs`, replacing `toggle_animations_enabled`:

```rust
    /// Cycle `[animations].enabled` forward: auto → on → off → auto.
    pub fn next_animations_enabled(&mut self) {
        use nexterm_config::AnimationsEnabled::*;
        self.animations_enabled = match self.animations_enabled {
            Auto => Yes,
            Yes => No,
            No => Auto,
        };
        self.dirty = true;
    }

    /// Cycle `[animations].enabled` backward.
    pub fn prev_animations_enabled(&mut self) {
        use nexterm_config::AnimationsEnabled::*;
        self.animations_enabled = match self.animations_enabled {
            Auto => No,
            No => Yes,
            Yes => Auto,
        };
        self.dirty = true;
    }

    /// Row value text. `os_reduced` is what the OS last reported, so the
    /// `auto` row can say which way it resolves right now.
    pub fn animations_enabled_label(&self, os_reduced: bool) -> String {
        use nexterm_config::AnimationsEnabled::*;
        match self.animations_enabled {
            Auto if os_reduced => fl!("settings-value-animations-auto-reduced"),
            Auto => fl!("settings-value-animations-auto-normal"),
            Yes => fl!("settings-value-animations-on"),
            No => fl!("settings-value-animations-off"),
        }
    }

    /// Write-back value: `"auto"` as a string, the other two as booleans, so
    /// a config that predates P3c keeps the spelling its author used.
    pub fn animations_enabled_toml_value(&self) -> toml_edit::Value {
        use nexterm_config::AnimationsEnabled::*;
        match self.animations_enabled {
            Auto => toml_edit::Value::from("auto"),
            Yes => toml_edit::Value::from(true),
            No => toml_edit::Value::from(false),
        }
    }
```

Change both row-9 arms at `:26` and `:50` from `self.toggle_animations_enabled()` to `self.next_animations_enabled()` and `self.prev_animations_enabled()` respectively.

`settings/save.rs:87`:

```rust
        doc["animations"]["enabled"] = toml_edit::value(self.animations_enabled_toml_value());
```

`settings/reset.rs:62` keeps its shape (`self.animations_enabled = def.animations_enabled;`) and now copies the enum.

`renderer/overlay/widgets/settings_window.rs:148`:

```rust
        row::ANIMATIONS_ENABLED => WidgetKind::Cycle {
            value: sp.animations_enabled_label(animations_os_reduced).to_string(),
        },
```

The builder needs `animations_os_reduced: bool` threaded in from `config.animations.os_reduced_motion()`. Follow how the surrounding builder already receives per-tab values; if it takes a `&SettingsPanel` only, add the bool as a second parameter and update its call site(s) in `renderer/overlay/settings/window_tab.rs`.

`settings_window.rs:302`: `row::ANIMATIONS_ENABLED => sp.next_animations_enabled(),`.

Then add four keys to **each** of the 8 locale files, beside the existing `settings-value-animation-*` block:

| key | en | ja | de |
|---|---|---|---|
| `settings-value-animations-auto-normal` | `Auto (normal)` | `自動（通常）` | `Automatisch (normal)` |
| `settings-value-animations-auto-reduced` | `Auto (reduced)` | `自動（動きを抑制）` | `Automatisch (reduziert)` |
| `settings-value-animations-on` | `On` | `オン` | `Ein` |
| `settings-value-animations-off` | `Off` | `オフ` | `Aus` |

| key | fr | es | it |
|---|---|---|---|
| `settings-value-animations-auto-normal` | `Auto (normal)` | `Auto (normal)` | `Auto (normale)` |
| `settings-value-animations-auto-reduced` | `Auto (réduit)` | `Auto (reducido)` | `Auto (ridotto)` |
| `settings-value-animations-on` | `Activé` | `Activado` | `Attivato` |
| `settings-value-animations-off` | `Désactivé` | `Desactivado` | `Disattivato` |

| key | ko | zh-CN |
|---|---|---|
| `settings-value-animations-auto-normal` | `자동 (보통)` | `自动（普通）` |
| `settings-value-animations-auto-reduced` | `자동 (동작 줄임)` | `自动（减弱动效）` |
| `settings-value-animations-on` | `켜기` | `开启` |
| `settings-value-animations-off` | `끄기` | `关闭` |

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p nexterm-client-gpu`, `cargo clippy -- -D warnings`, `cargo fmt --check`.
Expected: green. `accessibility.rs` needs no change — it maps `WidgetKind::Cycle` to `Role::ComboBox` already (`accessibility.rs:1453`), so the row's screen-reader role follows the widget change automatically.

- [ ] **Step 5: Commit**

```bash
git add nexterm-client-gpu/src nexterm-i18n/locales
git commit -m "feat(client): make the animations row a three-state cycler

auto / on / off を設定パネルから選べるようにした。auto は
現在どちらに解決されているか（通常 / 動きを抑制）を併記する。
併記しないと、OS が抑制を要求している環境で行の表示が
実態と食い違うため。8 ロケール全部に文字列を追加した。"
```

---

### Task 5: Documentation

**Files:**
- Modify: `docs/CONFIGURATION.md:271-290` (the `[animations]` section) and the example block at `:1030`
- Modify: `docs/plans/ui-ux-modernization-v3.md` (the P3c checklist item)
- Test: none (documentation)

**Interfaces:** none.

- [ ] **Step 1: Update `docs/CONFIGURATION.md`**

In the `[animations]` section, document the tri-state, in the file's existing voice: `enabled` accepts `"auto"` (the default), `true` or `false`; `auto` follows the OS accessibility preference — Windows' "Show animations in Windows", macOS' "Reduce motion" — and animates where there is no such preference to read, which today means Linux; `true` animates even when the OS asks for reduced motion; `false` never animates. State that detection can only ever *disable* motion, that the OS value is never written back into this file, and that the preference is re-read whenever the window regains focus. Update the example block at `:1030` to show `enabled = "auto"`.

- [ ] **Step 2: Update the plan checklist**

Mark `P3c OS reduced-motion detection` complete in `docs/plans/ui-ux-modernization-v3.md`, in the same voice as its P3b siblings: the tri-state config, why `auto` is the default, the `#[serde(skip)]` boundary that keeps the OS value out of `config.toml`, focus-gain sampling instead of native change notifications, the correction that P2c never added objc2 (so P3c made its own dependency decision: objc2 core, not `objc2-app-kit`), and Linux detection being deliberately out of scope.

- [ ] **Step 3: Commit**

```bash
git add docs
git commit -m "docs: P3c tri-state animations.enabled and OS reduced-motion

CONFIGURATION.md に 3 値の意味と「検出は無効化方向にしか働かない」
「OS 由来の値は config.toml に書き戻さない」を明記。計画書の P3c を
完了にし、macOS の依存判断が P2 から独立したことも記録した。"
```

---

## Known limitations to state in the PR description

- **Linux has no detection.** `auto` behaves as `true` there. GNOME's `enable-animations` and the XDG settings portal are both plausible later; the manual setting is the documented fallback for now.
- **No native change notifications.** A preference changed while the window already has focus is not noticed until focus is lost and regained.
- **The macOS arm is unverified outside CI.** It rests on `objc2`'s runtime class lookup finding `NSWorkspace`, which holds because winit and `window-vibrancy` link AppKit. If CI disproves it, the fallback is `objc2-app-kit`.
- **Neither platform's detection is verified on a real machine.** The config-layer logic is unit-tested; that the OS reports what we think it reports is a two-minute manual check per platform, on the existing on-device verification backlog.
