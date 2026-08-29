# P3c: OS Reduced-Motion Detection

Status: proposed
Related plan: `docs/plans/ui-ux-modernization-v3.md`, section "P3 — Motion language (M–L)", checklist item "P3c OS reduced-motion detection"
Prior work: P3a (#77, #78) — `Timed`, `Curve`, `duration`, `scaled_duration_ms`; P3b1 (#79) — `SurfaceMotion`; P3b2 (#80, #82) — `HoverTransition`; P3b3 (#83) — `PressPulse`

P3a through P3b3 gave this client a motion language. P3c is the switch that
turns it off for the people who asked their OS to stop moving things.

## What exists today (measured, not assumed)

Every animation in the client — surface open/close, hover cross-fade, press
pulse, tab accent, cursor blink — reaches its duration through
`AnimationsConfig::scaled_duration_ms`, which returns 0 when
`effective_multiplier()` is 0. Setting `animations.enabled = false` or
`intensity = "off"` in `config.toml` already disables all of it.

Nothing reads any OS accessibility preference. The plan's own line —
"Linux: manual `animations.enabled` config remains the fallback" — is
currently the behaviour on all three platforms.

Two facts from the codebase shape the design:

- **`config.animations` is read in ~70 places**, always as `&AnimationsConfig`
  passed into an animation type. None of them computes a multiplier itself.
  That is what makes this phase small: the switch belongs inside
  `AnimationsConfig`, and no call site has to learn about it.
- **The settings panel writes `config.toml` back through `toml_edit`.** An
  OS-derived value that lives in the same struct as the user's authored value
  can be persisted by that path, silently writing a setting the user never
  chose.

### Correction to the plan

The plan says macOS detection "shares the objc2 dependency decision with P2".
That is stale: P2c did not add objc2. It added `window-vibrancy = "0.8"`
(`nexterm-client-gpu/Cargo.toml:71-79`), which does not expose
`accessibilityDisplayShouldReduceMotion`. P3c therefore makes its own
dependency decision — see "Platform detection" below.

Windows needs no new dependency, verified against the vendored crate:
`SPI_GETCLIENTAREAANIMATION` and `SystemParametersInfoW` are both in
`Win32_UI_WindowsAndMessaging`, a feature this workspace already enables on
`windows-sys 0.59`.

## Config semantics

`animations.enabled` becomes tri-state:

```rust
pub enum AnimationsEnabled { Auto, Yes, No }   // serde: "auto" / true / false

pub struct AnimationsConfig {
    pub enabled: AnimationsEnabled,
    pub intensity: AnimationIntensity,
    /// Set by the platform layer each time it samples the OS. Never read
    /// from or written to `config.toml`.
    #[serde(skip)]
    os_reduced_motion: bool,
}
```

`effective_multiplier()` returns 0 when `enabled == No`, or when
`enabled == Auto && os_reduced_motion`; otherwise the intensity multiplier as
today. **No animation code changes.** All ~70 sites keep passing
`&config.animations`, and every phase from P3a onward inherits the switch
because they all bottom out in `scaled_duration_ms`.

Three properties this shape buys:

- **The user can overrule the OS.** `enabled = true` means "animate anyway",
  which the maintainer chose over an OS-absolute rule so there is a way out.
  `enabled = false` still means "never animate", OS or no OS.
- **Detection only ever disables.** `os_reduced_motion` has no path to raising
  a multiplier. This is the plan's constraint, enforced structurally rather
  than by care.
- **The OS value cannot reach disk.** `#[serde(skip)]` and a private field
  make the boundary a type property, the same move P3b1 used to keep a
  password out of a render ghost.

**Backward compatibility.** `enabled = true` and `enabled = false` keep
parsing (serde untagged: bool or the string `"auto"`). The default changes
from `true` to `Auto`, so a user who never wrote the key **does** get new
behaviour — that is the point of the phase, and it only ever removes motion.

## Platform detection

One function, deliberately thin:

```rust
// nexterm-client-gpu/src/platform.rs
pub fn reduced_motion() -> Option<bool>   // None = cannot tell / unsupported
```

- **Windows**: `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, ..)` fills a
  `BOOL` that is TRUE when client-area animations are *enabled*, so reduced
  motion is its negation. Returns `None` if the call fails.
- **macOS**: `NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion`,
  reached with **objc2 core only** — `class!(NSWorkspace)` plus `msg_send!` —
  rather than pulling in the `objc2-app-kit` subtree for one BOOL.
  **This is the design's one unverified assumption**: it depends on the
  AppKit class being registered in the running process, which it is only
  because winit and `window-vibrancy` already link AppKit. If the runtime
  lookup turns out not to resolve, the fallback is `objc2-app-kit` behind the
  existing `cfg(target_os = "macos")` block — a heavier dependency, not a
  redesign. Implementation must confirm this before the API is built on it.
- **Linux**: returns `None`. GNOME's `enable-animations` and the XDG settings
  portal are out of scope for P3c, per the plan; the manual config setting
  stays the fallback.

`None` is treated as "not reduced": `Auto` on Linux behaves exactly as
`enabled = true` does today.

## When it is sampled

At startup, and again on `WindowEvent::Focused(true)`.

The client has no `Focused` arm today; it does handle `ThemeChanged`, so
reacting to an OS preference is an established shape here. Focus-gain is the
sample point because it matches what the user actually does: open System
Settings, change the preference, come back to the terminal. It costs one
cheap syscall per focus gain and needs no observer machinery on either
platform.

Only a *changed* value triggers `request_redraw`; an unchanged sample does
nothing.

Native change notifications (`WM_SETTINGCHANGE` on Windows, an `NSWorkspace`
notification observer on macOS) are out of scope. They would be two separate
new mechanisms for a preference that changes a handful of times in a user's
life.

## Settings panel

`row::ANIMATIONS_ENABLED` is a boolean toggle today
(`widgets/settings_window.rs:148`, `:302`). It becomes a three-value cycler —
auto / on / off — modelled on the intensity cycler immediately beside it,
which already has this exact shape.

That needs new display strings in **all 8 locale files** under
`nexterm-i18n/locales/`. The cycler must also show what `auto` currently
resolves to, or the row is a lie on a machine where the OS says reduce: the
value text reads `auto (reduced)` / `auto (normal)` — resolved at draw time
from the same `os_reduced_motion` the multiplier uses.

## Testing

In `nexterm-config` (all pure, no OS calls):

1. `Auto` + `os_reduced_motion = true` → multiplier 0.
2. `Yes` + `os_reduced_motion = true` → the intensity multiplier. Pins that
   the user can overrule the OS.
3. `No` + `os_reduced_motion = false` → 0. Pins that the OS cannot *enable*
   motion.
4. `enabled = true` and `enabled = false` in TOML still parse, to `Yes` and
   `No`. Backward compatibility.
5. A config serialized after `os_reduced_motion` was set to `true` does not
   contain the field. Pins the write-back boundary.
6. Omitting the key yields `Auto`.

`platform::reduced_motion()` itself is not unit-tested: it is a single
FFI call per platform with no branching worth pinning, and CI cannot set an
OS accessibility preference. Keeping it thin is what makes that acceptable —
every decision lives in the config layer above it, where tests 1-6 run.

Not covered, and not claimed: that a real Windows or macOS machine reports
the preference correctly. This is a two-minute manual check per platform and
belongs on the on-device verification backlog with the rest of P3.

## Non-goals

- Linux detection (GNOME gsettings, XDG portal).
- Native change notifications on either platform.
- Any change to what "reduced motion" *means* per animation. Reduced motion
  is 0 ms everywhere, exactly as `intensity = "off"` already behaves. No
  animation gets a special reduced variant.
