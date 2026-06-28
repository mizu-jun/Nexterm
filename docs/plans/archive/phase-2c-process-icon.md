# Phase 2c — Per-tab process / shell icon

Status: planning (2026-06-28)
Source: `docs/plans/ui-ux-modernization-v2.md` Phase 2c (deferred → in
planning).
Owner: `mizu-jun`. PR: TBD (not yet branched).

## Goal

Render a Nerd Font glyph next to each tab label so users can tell at
a glance what is running in each pane (e.g. ` vim`, ` ssh`, ` git`,
` zsh`). The glyph reflects the **foreground** process inside the
pane's PTY, not the shell itself.

WezTerm and Ghostty both ship this; it is one of the visible polish
gaps remaining after the core plan (Phases 1–6 + 4b/5b/6b + 3b)
shipped on 2026-06-28.

## Why this is bigger than it looks

Three independent moving parts cross the daemonless boundary:

1. **Server-side process inspection** is OS-specific. Each OS needs
   its own way to walk from `pane.pid` (the shell PID) to the
   foreground descendant's name. Existing helpers cover
   `has_foreground_process` (bool) for all three OSes, but the
   *name* path is not yet implemented.

2. **IPC** must carry the name to the client. The current
   `TitleChanged { pane_id, title }` does not have a process slot,
   and adding one or introducing a new variant is a wire-format
   break — `PROTOCOL_VERSION` must bump 8 → 9.

3. **Renderer** needs a glyph map + Nerd Font fallback path. The
   tab bar in `nexterm-client-gpu/src/renderer/overlay/tab_bar.rs`
   currently consumes only `PaneState.title`; threading
   `process_name` through plus the glyph lookup is touchpoint #3.

Each part is small individually; the friction is in their joint
delivery within one PR.

## Out of scope (deferred to follow-ups)

- Hover-detail tooltip ("running: vim, last command: cargo test")
- Process-tree depth heuristics beyond "topmost descendant"
- Real-time refresh on every keystroke — 1 Hz polling is plenty
- Custom user glyph overrides via Lua/TOML

## Design

### Server: foreground process name lookup

Extend `Pane` with a single new method:

```rust
impl Pane {
    /// Return the executable name (e.g. "vim", "ssh", "node") of the
    /// topmost foreground descendant of the shell, or `None` when no
    /// child is running.
    pub fn foreground_process_name(&self) -> Option<String>;
}
```

Per-OS implementation:

| OS | Strategy |
|---|---|
| **Linux** | Read `/proc/{shell_pid}/stat` → extract `tpgid` (field 8). Read `/proc/{tpgid}/comm` (or follow `/proc/{tpgid}/exe` → basename). `tpgid <= 0` or `tpgid == shell_pgid` → `None` (sitting at the shell prompt). |
| **macOS** | Use the existing `ps -A -o pid=,ppid=,comm=` infrastructure (`read_has_foreground_process` already shells out). Build a child-of map keyed by ppid, walk down from `shell_pid`, take the deepest descendant's `comm`. `None` when the shell has no children. |
| **Windows** | `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)` + `Process32FirstW` / `Process32NextW`. Build the same parent-map and pick the deepest descendant. Use `windows-sys` (already a dependency). Strip `.exe` from the result so the glyph map matches against bare names. |

Caveats and decisions:

- **Cost**: `ps -A` on macOS is ~10–30 ms. Polling once per pane per
  second is acceptable but the polling task should batch — run `ps`
  **once** per tick and feed every pane.
- **`comm` truncation**: Linux truncates `comm` to 15 chars. The
  glyph map is keyed on the first 15 chars only.
- **Falling back gracefully**: any error → `None`. The client renders
  no icon, matching the `show_process_icon = false` look.

### Polling loop

Where to drive the polling:

- **Option A (per-pane task)**: each `Pane::spawn` starts a
  `tokio::time::interval(1s)` task. Simple, but multiplies tokio
  task count per session.
- **Option B (session-wide ticker)** ✅: `SessionManager` runs one
  `interval(1s)` task that iterates every pane each tick. Lower
  task count; centralises the `ps` fan-out on macOS.

Choose **B**. Snapshot pane IDs under the session lock, drop the
lock, run the OS-specific lookups, then re-acquire the lock to
diff against the cached last-seen name and broadcast on change.

### IPC

Add a new `ServerToClient` variant:

```rust
ProcessChanged {
    pane_id: u32,
    /// Foreground process name (e.g. "vim", "ssh"). `None` when the
    /// shell is at the prompt or detection failed.
    process_name: Option<String>,
}
```

**PROTOCOL_VERSION bump 8 → 9.** No client-side replay logic is
needed — `ProcessChanged` is purely informational; the client just
stores the latest value.

Why a new variant rather than extending `TitleChanged`:

- Title changes (OSC 0 / 2) and process changes have **different
  cadences and triggers**. Coupling them forces a redundant title
  re-send each second.
- Postcard adds enum variants tag-prefixed; adding a new variant is
  a wire-format break either way (same as extending an existing
  one with new fields), so there is no compatibility advantage to
  the more invasive option.

### Config

Single new boolean on the existing `TabBarConfig`:

```toml
[tab_bar]
show_process_icon = false   # opt-in; default off until Nerd Font is required reading
```

When `false`, the server still polls and broadcasts (cheap), but
the client skips the glyph prefix. Polling stays on so toggling
the flag at runtime works without a restart.

> **Open question**: gate the *polling* on `show_process_icon`
> instead, to avoid the ~ps cost on macOS for users who disabled
> the feature? Decision: **yes**, server checks
> `SharedRuntimeConfig.tab_bar.show_process_icon` each tick and
> skips the lookup when false. Simpler than wiring a separate
> "subscribe to process updates" message.

### Client

- `PaneState` gains `process_name: Option<String>`.
- `apply_server_message` handles `ProcessChanged` by writing the
  field, marking the tab bar dirty (cache invalidation).
- A new module `nexterm-client-gpu/src/tab_icons.rs` holds the
  pure glyph map:

  ```rust
  pub fn glyph_for_process(name: &str) -> Option<&'static str> {
      match name {
          "vim" | "nvim" | "vi" => Some("\u{e62b}"),     //  
          "ssh" | "sshd"         => Some("\u{f489}"),     //  
          "git"                  => Some("\u{f1d3}"),     //  
          "node" | "deno" | "bun"=> Some("\u{e718}"),     //  
          "python" | "python3" | "ipython" => Some("\u{e606}"),  //  
          "cargo" | "rustc"      => Some("\u{e7a8}"),     //  
          "docker"               => Some("\u{f308}"),     //  
          "tmux" | "screen"      => Some("\u{f120}"),     //  
          "bash" | "zsh" | "fish"=> Some("\u{f120}"),     //  
          "pwsh" | "powershell"  => Some("\u{f0a0a}"),    //  
          _ => None,
      }
  }
  ```

  Pure function so the glyph table is unit-testable without a
  renderer. Codepoints can be tuned in review.
- `renderer/overlay/tab_bar.rs` prefixes the tab label with the
  glyph when (a) `tab_bar.show_process_icon == true`, (b) the pane
  has a `process_name`, and (c) `glyph_for_process` returns
  `Some`. Failures are silent (no glyph, no fallback character) so
  users without a Nerd Font see a sensible layout.

### Tests

| Layer | Tests |
|---|---|
| `nexterm-server` | parser for `/proc/{pid}/stat` (handle parens / spaces in comm); golden-input ps output → expected leaf; mock Toolhelp32 traversal (Windows test gated on `cfg(target_os = "windows")`) |
| `nexterm-proto` | round-trip of `ServerToClient::ProcessChanged` through postcard |
| `nexterm-client-gpu` | `glyph_for_process` returns the expected codepoint for every name in the map; unknown names → `None`; `PaneState.process_name` is updated by `apply_server_message`; tab bar respects `show_process_icon = false` |

Test count target: ≥ 10 new tests.

## Scope cuts on offer

Listed by likely impact in this order; pick when sizing the PR:

| Cut | Effect |
|---|---|
| **Linux + Windows MVP, macOS no-op** | Halves OS-specific work; macOS users see no icon. The maintainer's daily driver is Windows so the lossy OS is the least-used by this owner. |
| **Drop the 1 Hz ticker, refresh only on Enter keystrokes** | Saves the periodic IO entirely; UX trade-off is the icon lagging behind long-running commands. Probably unacceptable. |
| **No config flag — always on** | Removes a touchpoint but kills the opt-out, and Nerd Font isn't shipped with most systems by default. Strongly advise against. |
| **Reuse `TitleChanged` instead of a new variant** | Wire bump is the same, but title messages get noisier and the title-rate-limit logic interferes with process-rate. Net negative. |

## Risk register

- **PROTOCOL_VERSION 8 → 9**: forces a synchronised client/server
  upgrade. Acceptable because the single-binary `nexterm` ships
  both. Document in CHANGELOG and the upgrade note.
- **Nerd Font availability**: users without a Nerd Font see
  tofu (or a fallback box). Mitigation: `show_process_icon = false`
  by default. Could later add a glyph-presence check via cosmic-text
  but that is a follow-up.
- **macOS `ps` spawn cost**: 10–30 ms per tick. Acceptable at 1 Hz
  but worth re-evaluating if the polling tick is ever turned up.
- **Toolhelp32 on Windows**: enumerates *all* processes. For a 5000-
  process system this can cost 1–2 ms; acceptable. Worth benchmarking
  on the maintainer's machine before merge.
- **Process renames (e.g. nvim → editor)**: the foreground process'
  `comm` is whatever the executable's name was at exec time. Some
  TUIs change `argv[0]` but not `comm`. Acceptable — `comm` is the
  authoritative ID.

## Delivery plan

Single PR. Branch: `feat/uiux-v2-phase2c`.

Stage order (each compile-clean and test-green before the next):

1. **Server name lookup**: implement `foreground_process_name()` for
   Linux first (cheapest to test), then macOS, then Windows. Land
   the polling ticker on `SessionManager` after Linux works.
2. **IPC**: add `ServerToClient::ProcessChanged`, bump
   `PROTOCOL_VERSION`. Add the round-trip test.
3. **Client storage**: `PaneState.process_name` + `apply_server_
   message` arm + cache invalidation.
4. **Config**: `TabBarConfig.show_process_icon` + writeback path in
   settings panel (already has the Tab bar category — just add a
   row).
5. **Renderer**: `tab_icons.rs` + glyph prefix in `tab_bar.rs`.
6. **Docs**: CHANGELOG entry, plan status update, optional
   `docs/runtime-glyphs.md` listing the map.

Estimated effort: 1.5–2 days for the maintainer. Reviewer overhead:
~1 hr.

## Acceptance criteria

- `cargo test --workspace` green on Linux + Windows CI matrix
- `cargo clippy --all-targets -- -D warnings` clean
- Manual: open three panes (bash + vim + ssh somewhere), watch
  the icons update within 1 s of switching the foreground process
- Manual: `show_process_icon = false` → no icons render; toggle
  to `true` and hot-reload — icons appear without a restart
- Manual: kill the shell from outside, ensure no panic / stale
  cache

## Open questions / decision points before implementation

1. **Glyph for unknown processes**: render nothing (current
   proposal) vs. render a generic `` cog. Vote: render nothing
   so the absence of a glyph remains a signal.
2. **`show_process_icon` default**: `false` (proposed) vs.
   `true`. Nerd Font is not yet a documented dependency, so
   `false` is safer.
3. **Glyph map location**: in-tree constant (current proposal)
   vs. TOML-driven so users can override without recompiling.
   Vote: in-tree for v1, TOML override is a follow-up if there
   is demand.
4. **macOS support**: ship in this PR (proposed) vs. ship Linux
   + Windows now and add macOS in a follow-up. Vote: ship all
   three to avoid a wire-format-breaking 2-step.
