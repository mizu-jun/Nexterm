# ADR-0004: Hybrid configuration with TOML + Lua

## Status

Accepted (2026-05-12; records the Sprint 1–2 decision retroactively)

## Context

Terminal emulators differ widely in how they configure themselves:

- **Static**: Alacritty (TOML), Windows Terminal (JSON)
- **Dynamic**: WezTerm (Lua), kitty (its own KSL)
- **Hybrid**: Ghostty (text + environment variables)

The trade-offs we faced:

- **Static only**: simple, easy type-checking, easy hot-reload — but no conditionals or computed values. Insufficient flexibility for things like the status bar.
- **Dynamic only**: fully programmable, dynamic evaluation (e.g. status bar) is easy — but configs become long, the barrier for newcomers is high, and errors surface only at runtime.
- **Hybrid**: static parts in TOML, dynamic parts (status bar, hooks) in Lua.

## Decision

**Adopt the hybrid form.**

1. `~/.config/nexterm/config.toml` — static settings (fonts, colours, keybindings, …)
2. `~/.config/nexterm/config.lua` — optional; Lua functions for dynamic computation (status-bar left/right expressions, hooks)
3. Load order:
   1. Built-in defaults
   2. Merge in `config.toml`
   3. If `config.lua` exists, run it and merge the result
4. Watch the files for changes → hot reload

### Where Lua is used

- Status-bar left/right expressions (evaluated every second, cached in `StatusBarEvaluator`).
- Hooks (`HookEvent` reacting to OSC 133 semantic zones).
- Dynamic generation of configuration values (switch based on environment variables or time).

### Constraints on Lua

- The `mlua::Lua` instance is confined to a dedicated OS thread (`nexterm-lua-worker`) and communicates with the main thread over channels (working around Send/Sync limitations).
- Lua is sandboxed (`os.execute` and other risky APIs are restricted in `nexterm-config/src/lua_sandbox.rs`).

## Consequences

### Positive

- Newcomers can stay in TOML alone. Users who do not want Lua never have to touch it.
- Power users can write the status bar and hooks freely in Lua.
- TOML's type checking (via serde) catches most configuration errors at startup.
- Combines WezTerm-class flexibility with Alacritty-class simplicity.

### Negative

- Users must learn two formats (although Lua is optional).
- Implementation cost for Lua-thread management and sandboxing.
- Lua errors surface at runtime, which is later than TOML.
- More documentation to maintain.

## Alternatives

- **Alternative A: TOML only** — simple, but dynamic things like the status bar become weak.
- **Alternative B: Lua only (WezTerm-style)** — fully flexible but raises the barrier for newcomers.
- **Alternative C: JSON + JS (VS Code-style)** — bundling a Node runtime is heavy and complicates security.
- **Alternative D: a custom DSL** — kitty does this with KSL, but a new language adds learning cost.

## References

- `nexterm-config/src/loader.rs` — the TOML + Lua load order
- `nexterm-config/src/lua_worker.rs` — the Lua-dedicated thread
- `nexterm-config/src/lua_sandbox.rs` — Lua sandbox
- `nexterm-config/src/status_bar.rs` — Lua-driven status-bar evaluation
- WezTerm comparison: https://wezfurlong.org/wezterm/config/files.html
