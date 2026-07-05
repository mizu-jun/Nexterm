# ADR-0008: Plugin read API (read_pane / read_grid / read_scrollback) and its trust boundary

## Status

Accepted (2026-07-04)

## Context

Through Plugin API v2 (see [ADR-0003](0003-plugin-api-v2.md)) the WASM plugin host
is **push-only**: a plugin observes terminal activity solely through the
`nexterm_on_output` / `nexterm_on_command` hooks, and it may write back to the
pane it is currently handling via the `write_pane` host import (scoped by
`allowed_panes`). A plugin cannot *ask* the host for terminal state — it only
sees the bytes the host chooses to deliver, one burst at a time.

The competitive-gap roadmap (`docs/plans/gap-roadmap-2026h2.md`, Phase 4 / F3)
calls for a **pull** capability: plugins that can actively read the current
screen contents and scrollback history. This unlocks the class of plugins the
roadmap is aiming at (command-block analysis, AI assistants running *as
plugins* rather than in the core, linters that inspect the whole viewport,
etc.).

Exposing screen and scrollback contents to plugins opens a **new information
egress boundary**. Until now a plugin could only act on the stream as it flowed
by; with a read API a plugin can exfiltrate the full terminal buffer —
including secrets that scrolled past, command history, and output the user has
since cleared from view. This is materially different from the v2 write
restriction and must be gated deliberately.

### Constraints discovered during design

- **Visible grid is already mirrored to the main thread.** Each `Pane` holds
  `latest_grid: Arc<Mutex<Grid>>` (added in v1.9.3) which the PTY reader thread
  refreshes after every burst. A read of the *visible* screen is therefore a
  cheap synchronous lock-and-clone.
- **Scrollback is trapped in the reader thread.** The `VtParser` (which owns
  scrollback) lives thread-local to the PTY reader; it is not visible to the
  main thread. Exposing scrollback requires a new mirror or a request/response
  channel to the reader.
- **The plugin host runs server-side; the consent UI runs client-side.** The
  existing interactive consent flow (`ConsentPolicy` +
  `ConsentKind`/`pending_consent`, see
  `nexterm-client-gpu/src/renderer/event_handler/consent.rs`) is driven from the
  GPU client. Read host imports, however, are invoked **synchronously** inside a
  WASM call on the server. Prompting the user mid-call would require a blocking
  round-trip to the client, which does not fit the synchronous host-import
  model.

## Decision

We will add a **Plugin read API at ABI v3** with a **server-side static consent
policy** and a **fail-safe-deny default**.

### 1. ABI version bump: `PLUGIN_API_VERSION` 2 → 3

The read host imports are only linked for plugins that declare
`nexterm_api_version() >= 3`. v1 and v2 plugins never see them (backwards
compatible; the imports simply do not resolve for older plugins, exactly as
`write_pane` behaves per version today). The v1-removal timeline of ADR-0003 is
unchanged.

### 2. New host imports (v3)

```wat
;; Visible screen as UTF-8 text (rows joined by LF, trailing blanks trimmed).
(import "nexterm" "read_pane"       (func (param i32 i32 i32) (result i32)))
;;   params: pane_id, out_ptr, out_max   -> bytes written (or negative error)

;; Visible screen as a compact structured cell dump (format below).
(import "nexterm" "read_grid"       (func (param i32 i32 i32) (result i32)))
;;   params: pane_id, out_ptr, out_max   -> bytes written (or negative error)

;; Scrollback lines as UTF-8 text (LF-joined), newest-last.
(import "nexterm" "read_scrollback" (func (param i32 i32 i32 i32 i32) (result i32)))
;;   params: pane_id, start_line, max_lines, out_ptr, out_max -> bytes written
```

Return-value convention (all three): a non-negative value is the number of
bytes written into `[out_ptr, out_ptr+out_max)`; a negative value is an error
code (`-1` = permission denied, `-2` = unknown pane, `-3` = buffer too small,
`-4` = read disabled by policy). The host never writes past `out_max`.

The host evaluates guards in a fixed order — **policy → pane allow-list → pane
existence** — and returns the first failure. Because a policy-`deny` short-
circuits before the allow-list is consulted, a plugin cannot probe pane
existence while reads are disabled. During `nexterm_on_command(…)` the read
allow-list is empty (there is no pane context), so **any read call returns
`-2`** (unknown pane); `-1` is reserved and currently unused.

### 3. `read_grid` wire format (language-agnostic)

`read_grid` must be consumable from any WASM guest language, so it does **not**
serialize the Rust `Grid` type. Instead it emits a small documented binary
layout:

```
u16 cols, u16 rows,
then rows*cols cell records, row-major:
  u32 codepoint (Unicode scalar; 0x20 for blank),
  u8  fg_index,  u8 bg_index   (palette index; 0xFF = default),
  u8  attr_bits  (bit0 bold, bit1 italic, bit2 underline, bit3 reverse),
  u8  reserved (0)
```

All integers little-endian (matching the IPC codec convention). This format is
versioned implicitly by the ABI version; a future ABI may extend the cell
record, and the ADR for that bump will document the change.

When the dump would exceed `plugin_read_max_bytes`, the host caps it on a
**whole-row boundary** and rewrites the header's `rows` field to match the
retained payload, so a parser that reads `cols * rows` cells never overruns.
The host never returns a dump ending mid-cell or with a header that overstates
the row count. (In practice the 1 MiB default holds any real screenful, so this
cap only engages for pathologically large grids.)

`fg_index` / `bg_index` are palette indices into the caller's active
`ColorScheme` (the same palette the renderer uses); `0xFF` means "scheme
default". **Image cells** (produced by Sixel / Kitty graphics) are represented
with `codepoint = 0xFFFD` (replacement char) and `attr_bits` bit4
(`image_marker`) set; the pixel data itself is **not** included in `read_grid`.
Access to raw image bytes, if ever needed, would be a separate future host
import with its own ADR — this keeps `read_grid` bounded to the visible-grid
size and avoids smuggling large binary payloads through it.

### 4. Consent model: server-side static policy, default Deny

Add `plugin_read: ConsentPolicy` to `SecurityConfig` (`nexterm-config`).
Semantics differ from the OSC policies because there is no synchronous prompt
path for a server-side WASM call:

- **`allow`** — read imports are live.
- **`deny`** (default) — read imports return `-4` (disabled by policy).
- **`prompt`** — treated as **`deny`** for now (fail-safe). A future ADR may add
  a load-time interactive grant; until then `prompt` never silently enables
  reads.

Note this default is intentionally the **opposite** of the OSC policies (which
default `prompt`): a brand-new egress channel is off unless the operator opts
in. A plugin declaring read intent while the policy is `deny` still loads, but
its read calls are refused — surfaced once via a warn log at load time.

**Policy changes take effect immediately** for subsequent read calls; already-
loaded plugins do not need to be reloaded when `plugin_read` flips.

Because `prompt` is silently downgraded to `deny`, both the generated
`config.toml` comment and the settings-panel label must spell this out (e.g.
"prompt — currently treated as deny; interactive plugin consent is not yet
supported") so operators are not misled into thinking a prompt will appear.

### 5. Pane scoping (defense in depth)

Even when `plugin_read = allow`, reads are limited to the pane the plugin is
currently handling. During `nexterm_on_output(pane_id, …)` a plugin may read
`pane_id` only; during `nexterm_on_command(…)` no pane may be read (there is no
pane context). This reuses the same per-call allow-list mechanism that already
scopes `write_pane`, so a plugin cannot enumerate or read panes it was never
invoked for.

### 6. Size and rate caps (DoS / egress mitigation)

- A single `read_pane` / `read_grid` is bounded by the visible grid size.
- `read_scrollback` honors `max_lines` and a `SecurityConfig.plugin_read_max_bytes`
  ceiling (default 1 MiB, mirroring `osc52_max_bytes`); the host clamps
  `max_lines` to the configured scrollback retention (`scrollback_lines`).
- Reads consume the same per-call fuel budget as any other host import, so a
  plugin cannot loop on reads without exhausting fuel. This applies to
  `read_scrollback` regardless of `max_lines`: a large read still runs under
  the per-call fuel ceiling.

### 7. Scrollback privacy (operator guidance)

Scrollback retains **everything** that scrolled past, including secrets that
were printed and then cleared from view (API keys, passwords, SSH key material
echoed by tooling, PII). A read-capable plugin can therefore observe data the
user believes is gone, and — by reading repeatedly — can detect *changes* to
history, which leaks timing/what-ran information even without full content.

This is a **consent boundary, not a containment boundary**: once
`plugin_read = allow`, a trusted-but-curious or compromised plugin can
exfiltrate buffer contents through its own channels. Operators who need zero
exposure must keep `plugin_read = deny` (the default). Operators who enable it
should treat loaded read-capable plugins with the same trust as any process
that can read the terminal, and clear scrollback (`clear`, `ESC [ 3 J`, or a
fresh session) before running sensitive commands. This guidance ships in the
settings-panel help text and the config comment.

## Consequences

### Positive

- Enables the roadmap's target plugin class (block analysis, AI-as-plugin,
  viewport linters) while keeping the core terminal AI-free.
- Reuses proven infrastructure: `latest_grid` mirroring, the `write_pane`
  per-call allow-list pattern, `ConsentPolicy`, and fuel metering.
- Fail-safe posture: the egress channel is **off by default** and pane-scoped
  even when on.

### Negative

- Scrollback mirroring adds per-pane memory and a small per-burst cost on the
  reader thread (bounded by `scrollback_lines`).
- Maintaining a third ABI version widens the version-branching in the host.
- `prompt` semantics for `plugin_read` are asymmetric with the OSC policies,
  which is a documentation/expectation cost; the asymmetry is deliberate but
  must be called out in the settings UI copy.
- A read-capable plugin that the operator trusts can still exfiltrate buffer
  contents through its own side channels; the policy gate is a *consent*
  boundary, not a *containment* one. This is documented for operators.

## Alternatives

- **Interactive per-plugin consent at load (client round-trip)**: better UX and
  symmetric with OSC prompts, but requires new IPC plumbing and cross-process
  state; deferred to a future ADR. Chosen model keeps F3 shippable and secure
  first.
- **Per-read interactive prompt**: incompatible with the synchronous host-import
  model (would block a WASM call on a UI round-trip). Rejected.
- **Expose reads unconditionally (no policy)**: unacceptable — silently opens an
  egress channel for every loaded plugin. Rejected.
- **Serialize the Rust `Grid` via postcard for `read_grid`**: couples the guest
  ABI to a Rust-specific format; non-Rust WASM plugins could not parse it.
  Rejected in favor of the documented binary layout.
- **Visible grid only (drop scrollback for v1)**: simpler (no reader-thread
  mirroring), but the roadmap explicitly scopes scrollback into F3 and the
  target plugins need history. Rejected.

## References

- [ADR-0003](0003-plugin-api-v2.md) — Plugin API v1 → v2 and the removal timeline
- `docs/plans/gap-roadmap-2026h2.md` — Phase 4 / F3 (read API, consent UI, ADR required)
- `nexterm-plugin/src/lib.rs` — `PLUGIN_API_VERSION`, `write_pane` allow-list, fuel metering
- `nexterm-server/src/pane.rs` — `latest_grid` main-thread mirror (v1.9.3)
- `nexterm-config/src/schema/security.rs` — `ConsentPolicy` / `SecurityConfig`
- `nexterm-client-gpu/src/renderer/event_handler/consent.rs` — existing consent flow (client-side)
