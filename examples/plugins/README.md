# Nexterm WASM Plugin Examples

Five ready-to-build sample plugins demonstrating the Nexterm WASM plugin API.

## Prerequisites

```sh
rustup target add wasm32-unknown-unknown
```

## Build All Samples

```sh
# From this directory
for dir in echo-suppress error-detector command-counter timestamp-injector screen-digest; do
  (cd "$dir" && cargo build --release --target wasm32-unknown-unknown)
done
```

Output files:
```
echo-suppress/target/wasm32-unknown-unknown/release/echo_suppress.wasm
error-detector/target/wasm32-unknown-unknown/release/error_detector.wasm
command-counter/target/wasm32-unknown-unknown/release/command_counter.wasm
timestamp-injector/target/wasm32-unknown-unknown/release/timestamp_injector.wasm
screen-digest/target/wasm32-unknown-unknown/release/screen_digest.wasm
```

## Install

Copy the `.wasm` files to your config directory and register them in `nexterm.toml`:

```toml
[[plugins]]
path = "~/.config/nexterm/plugins/error_detector.wasm"

[[plugins]]
path = "~/.config/nexterm/plugins/command_counter.wasm"

[[plugins]]
path = "~/.config/nexterm/plugins/timestamp_injector.wasm"
```

---

## Plugin Descriptions

### echo-suppress ⭐ (API version demo)

Demonstrates `nexterm_meta` (plugin name/version) and `api_version()` import.
Suppresses any PTY output line that starts with `^` (common shell autocomplete noise).

```sh
nexterm-ctl plugin load ./echo-suppress/target/wasm32-unknown-unknown/release/echo_suppress.wasm
```

### error-detector

Watches PTY output for lines containing "error" (case-insensitive) and writes a
highlighted notice back to the same pane.

Custom commands:
- `:error-reset` — reset the error counter

### command-counter

Tracks OSC 133 D semantic marks to count how many commands have been run and
records the last exit code.

Custom commands:
- `:count-show`  — print current count and last exit code
- `:count-reset` — reset counters

### timestamp-injector

Prepends a `HH:MM:SS.mmm |` timestamp to each output line. Disabled by default
to avoid interfering with normal use.

Custom commands:
- `:ts-on`  — enable timestamp injection
- `:ts-off` — disable timestamp injection

### screen-digest ⭐ (v3 read API demo)

Demonstrates the **v3 read API** (`read_pane`; F3 / ADR-0008). On each command
completion (OSC 133 `D` mark) it reads the visible pane text and logs a digest
(non-blank line count and character count). It never writes to the pane.

The read API is gated by the server-side `plugin_read` consent policy, which
defaults to **`deny`**. To let this plugin read the pane, opt in explicitly:

```toml
[security]
plugin_read = "allow"
```

While `plugin_read` is `deny` (or `prompt`, currently treated as `deny`),
`read_pane` returns `-4` and the digest reports nothing.

---

## Writing Your Own Plugin

Any language that compiles to `wasm32-unknown-unknown` works.

### Required exports

| Export | Signature | Notes |
|--------|-----------|-------|
| `nexterm_init` | `() -> ()` | Optional. Called once on load. |
| `nexterm_on_output` | `(pane_id: i32, ptr: i32, len: i32) -> i32` | `pane_id` is passed **first**. Return 0 = pass through, 1 = suppress |
| `nexterm_on_command` | `(ptr: i32, len: i32) -> i32` | Return 0 = handled, 1 = not handled |

### Host imports (`nexterm` module)

| Import | Signature | Notes |
|--------|-----------|-------|
| `nexterm.api_version` | `() -> i32` | Returns `PLUGIN_API_VERSION` (host current: `3`) |
| `nexterm.log` | `(ptr: i32, len: i32)` | Write to nexterm log |
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | Write text to a pane |
| `nexterm.read_pane` | `(pane_id: i32, out_ptr: i32, out_max: i32) -> i32` | **v3.** Copy visible pane text into `out_ptr`. Returns bytes written, or a negative error code (see below) |
| `nexterm.read_grid` | `(pane_id: i32, out_ptr: i32, out_max: i32) -> i32` | **v3.** Copy the structured grid dump (ADR-0008 §3 wire format) |
| `nexterm.read_scrollback` | `(pane_id: i32, start_line: i32, max_lines: i32, out_ptr: i32, out_max: i32) -> i32` | **v3.** Copy `max_lines` of scrollback text starting at `start_line` |

The v3 read imports are gated by the server-side `plugin_read` policy
(default `deny`) and are scoped, per hook, to the pane the plugin is currently
handling. Negative return codes: `-1` wrong ABI, `-2` unknown/out-of-scope
pane, `-3` output buffer too small, `-4` disabled by the `plugin_read` policy.

### Optional exports

| Export | Signature | Notes |
|--------|-----------|-------|
| `nexterm_api_version` | `() -> i32` | **Required for v2**: declares the API version the plugin targets. If not exported or returning `1`, the plugin is treated as v1 and a deprecation warning is logged |
| `nexterm_meta` | `(name_buf: i32, name_max: i32, ver_buf: i32, ver_max: i32) -> i32` | Plugin name/version for `nexterm-ctl plugin list` |

---

## Plugin API v1 → v2 migration guide (Sprint 5-4 / F1)

Aligned with the host's `PLUGIN_API_VERSION = 2`, the four sample plugins have been migrated to v2.

### Behavioural differences between v1 and v2

| Item | v1 (legacy) | v2 (recommended) |
|------|------------|-----------------|
| `nexterm_api_version` | not exported / returns `1` | returns `2` |
| `pane_id` in `nexterm_on_output` | not sanitised (invalid values may be passed) | sanitised, guaranteed-valid pane_id |
| `write_pane` allowlist | no check (any pane writable) | only panes registered in `allowed_panes` |
| Clipboard writes (OSC 52) | auto-allowed | host-side allowlist enforcement |
| Notification publishing | auto-allowed | host-side allowlist enforcement |
| Deprecation warning | logged once | none |

### Migration steps (porting an existing v1 plugin to v2)

1. Add a `nexterm_api_version()` export to the source:

   ```rust
   #[no_mangle]
   pub extern "C" fn nexterm_api_version() -> i32 {
       2
   }
   ```

2. If you call `write_pane`, register the destination panes in `allowed_panes`
   (the host calls `register_pane(plugin_id, pane_id)` — to be added in v2).

3. Bump the version in `Cargo.toml` to `0.2.0` or higher.

4. Rebuild with `cargo build --release --target wasm32-unknown-unknown`.

### End-of-life for v1 support

Plugin v1 support **will be removed at the v2.0 release**. Until then v1 plugins can still
be loaded, but a deprecation warning is logged. Write new plugins against v2 or v3.

---

## Plugin API v2 → v3 (F3 / ADR-0008)

`PLUGIN_API_VERSION` is now **3**. v3 is a **superset** of v2: v2 plugins keep
working unchanged. v3 only *adds* the read host imports — a plugin opts in by
exporting `nexterm_api_version() -> 3` and importing one or more of
`read_pane` / `read_grid` / `read_scrollback`.

### What v3 adds

| Item | v2 | v3 |
|------|----|----|
| `nexterm_api_version` | returns `2` | returns `3` |
| Read host imports | absent | `read_pane` / `read_grid` / `read_scrollback` |
| Read consent | n/a | server-side `plugin_read` policy (default **`deny`**) |
| Read scope | n/a | per hook, limited to the pane being handled |

### Enabling reads

The read API is a new information-egress channel, so it stays off unless the
operator opts in:

```toml
[security]
plugin_read = "allow"          # default is "deny"; "prompt" is treated as "deny" for now
plugin_read_max_bytes = 1048576 # optional cap per read (default 1 MiB)
```

See `screen-digest` above for a minimal `read_pane` example, and
[docs/adr/0008-plugin-read-api.md](../../docs/adr/0008-plugin-read-api.md) for
the trust-boundary rationale and the grid dump wire format.

See [docs/plugin-api.md](../../docs/plugin-api.md) for the full API reference.
