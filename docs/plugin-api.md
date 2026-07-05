# Nexterm WASM Plugin API

**Current API Version:** 3 (`PLUGIN_API_VERSION = 3`)
**Minimum Supported Version:** 1 (`MIN_SUPPORTED_API_VERSION = 1`)
**Minimum Version for the read API:** 3 (`MIN_READ_API_VERSION = 3`)

Nexterm supports WebAssembly (WASM) plugins via the [wasmi](https://github.com/wasmi-labs/wasmi) runtime. Plugins run in a sandboxed WASM environment and communicate with the host through a stable ABI.

---

## API Versions at a Glance

| Capability | v1 (legacy) | v2 | v3 (current) |
|---|---|---|---|
| `nexterm_on_output` / `nexterm_on_command` input | Raw PTY bytes (incl. ESC sequences) | **Sanitized**: ESC/CSI/OSC/DCS/APC + C0 controls (except `\t\r\n`) removed | Same as v2 |
| `nexterm.write_pane(pane_id, ...)` | Any pane allowed | **PaneId allowlist**: only the pane that emitted output (in `on_output`); none (in `on_command`) | Same as v2 |
| Read host imports (`read_pane` / `read_grid` / `read_scrollback`) | absent | absent | **available**, gated by the `plugin_read` policy (default `deny`), scoped per hook to the current pane |
| Load behavior | Loads with **deprecation warning** | Loads silently | Loads silently |
| `nexterm_api_version` export | Optional (omitted = v1) | Should return `2` | Should return `3` |

> **v1 and v2 plugins continue to work** via graceful downgrade. The host detects the plugin's declared API version at load time. Plugins without `nexterm_api_version` are treated as v1 and a deprecation warning is logged.
>
> v3 is a **superset** of v2: it only adds the read host imports. A v2 plugin needs no changes to run on a v3 host. v1 support will be removed in a future release. Migrate by exporting `nexterm_api_version() -> i32 = 3` and adapting to sanitized inputs / pane allowlist rules.

---

## Quick Start

```sh
# Add the WASM target
rustup target add wasm32-unknown-unknown

# Build the sample plugin
cd examples/plugins/echo-suppress
cargo build --release --target wasm32-unknown-unknown

# Load at runtime
nexterm-ctl plugin load ./target/wasm32-unknown-unknown/release/echo_suppress.wasm

# Check loaded plugins (shows api_version column)
nexterm-ctl plugin list

# Unload
nexterm-ctl plugin unload ./target/wasm32-unknown-unknown/release/echo_suppress.wasm
```

---

## Host Imports (functions the plugin can call)

| Import | Signature | Description |
|--------|-----------|-------------|
| `nexterm.api_version` | `() -> i32` | Returns `PLUGIN_API_VERSION` (currently `3`). Call to verify host capability. |
| `nexterm.log` | `(ptr: i32, len: i32)` | Write a UTF-8 string to the Nexterm log (tracing info level). |
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | Write raw bytes to the specified pane's PTY input. **In v2+, restricted to allowlisted pane IDs per call** (see below). |
| `nexterm.read_pane` | `(pane_id: i32, out_ptr: i32, out_max: i32) -> i32` | **v3.** Copy the visible pane text (UTF-8) into `out_ptr`. Returns bytes written or a negative error code (see the read API section). |
| `nexterm.read_grid` | `(pane_id: i32, out_ptr: i32, out_max: i32) -> i32` | **v3.** Copy the structured grid dump (ADR-0008 §3 wire format). |
| `nexterm.read_scrollback` | `(pane_id: i32, start_line: i32, max_lines: i32, out_ptr: i32, out_max: i32) -> i32` | **v3.** Copy `max_lines` scrollback text lines starting at `start_line`. |

### Rust import declarations

```rust
#[link(wasm_import_module = "nexterm")]
unsafe extern "C" {
    fn api_version() -> i32;
    fn log(ptr: *const u8, len: usize);
    fn write_pane(pane_id: i32, ptr: *const u8, len: usize);
    // v3 read API (import only what you use):
    fn read_pane(pane_id: i32, out_ptr: *mut u8, out_max: usize) -> i32;
    fn read_grid(pane_id: i32, out_ptr: *mut u8, out_max: usize) -> i32;
    fn read_scrollback(
        pane_id: i32,
        start_line: i32,
        max_lines: i32,
        out_ptr: *mut u8,
        out_max: usize,
    ) -> i32;
}
```

---

## v2 Behavior Details

### Input sanitization (v2 only)

Before calling `nexterm_on_output` / `nexterm_on_command`, the host strips:

- `ESC` (`0x1B`) and any following CSI / OSC / DCS / APC / PM sequence (until ST or BEL terminator)
- C0 control bytes (`0x00..=0x1F` except `\t \n \r`) and `0x7F` (DEL)

The plugin receives plain text only. Bytes that pass through:

- `\t` (`0x09`), `\n` (`0x0A`), `\r` (`0x0D`)
- Printable ASCII (`0x20..=0x7E`)
- UTF-8 multi-byte sequences (`0x80..=0xFF`)

This prevents plugins from observing clipboard / hyperlink / title escape sequences and from being accidentally tricked by injected control bytes. v1 plugins continue to receive raw bytes for backwards compatibility.

### Pane ID allowlist (v2 only)

`nexterm.write_pane(pane_id, ...)` is gated by a per-call allowlist:

| Hook | Allowlist |
|---|---|
| `nexterm_on_output(pane_id, ...)` | `{pane_id}` only |
| `nexterm_on_command(...)` | empty (no writes allowed) |
| `nexterm_init` / `nexterm_meta` | empty |

Calls to `write_pane` outside the allowlist are silently ignored, with a `warn` log indicating the rejection. v1 plugins are not subject to this restriction.

---

## v3 Read API (F3 / ADR-0008)

v3 adds three host imports that let a plugin **read** terminal contents:

| Import | Returns |
|---|---|
| `read_pane(pane_id, out_ptr, out_max)` | Visible pane text as UTF-8. |
| `read_grid(pane_id, out_ptr, out_max)` | Structured grid dump (see wire format below). |
| `read_scrollback(pane_id, start_line, max_lines, out_ptr, out_max)` | Up to `max_lines` scrollback text lines from `start_line`. |

Each writes at most `out_max` bytes into the plugin's linear memory at `out_ptr`
and returns the number of bytes written (`>= 0`), or a negative error code:

| Code | Meaning |
|---|---|
| `-1` | Wrong ABI — the plugin did not declare `nexterm_api_version() >= 3`. |
| `-2` | Unknown or out-of-scope pane (not the pane the hook fired for). |
| `-3` | Output buffer too small (`out_max` cannot hold the result). |
| `-4` | Disabled by the `plugin_read` policy. |

### Consent policy

Reads are an information-egress channel, so they are gated by the server-side
`plugin_read` consent policy, which defaults to **`deny`** (fail-safe). Enable
it explicitly in `config.toml`:

```toml
[security]
plugin_read = "allow"           # default "deny"; "prompt" is treated as "deny" for now
plugin_read_max_bytes = 1048576 # optional per-read cap (default 1 MiB)
```

`prompt` currently behaves as `deny` because a server-side plugin call has no
synchronous UI path; an interactive consent flow may arrive later.

### Scope

Reads are scoped **per hook** to the pane the plugin is currently handling
(the `pane_id` passed to `nexterm_on_output`). Reading any other pane returns
`-2`. Results are capped at `plugin_read_max_bytes` — text is truncated at a
UTF-8 boundary, the grid dump at a whole-row boundary (with its `rows` header
rewritten to match).

### Grid dump wire format (`read_grid`, ADR-0008 §3)

Little-endian, row-major:

```
u16 cols
u16 rows
repeat cols*rows times:
  u32 codepoint     // Unicode scalar of the cell glyph
  u8  fg_index      // palette index, or 0xFF for the scheme default / truecolor
  u8  bg_index      // palette index, or 0xFF for the scheme default / truecolor
  u8  attr_bits     // bit0 bold, bit1 italic, bit2 underline, bit3 reverse
  u8  reserved      // 0
```

### Example

`examples/plugins/screen-digest/` is a minimal `read_pane` consumer: on each
command completion it reads the visible pane and logs a line/character digest.

---

## Plugin Exports (functions the plugin must/may implement)

### `nexterm_api_version` (recommended)

Declare the API version the plugin targets. Plugins that omit this export are treated as v1. Return `3` to use the read API; `2` if you only need the sanitization / write-pane allowlist behavior.

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nexterm_api_version() -> i32 {
    3
}
```

### `nexterm_meta` (optional)

Publish plugin name and version to the host. Shown in `nexterm-ctl plugin list`.

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nexterm_meta(
    name_buf: *mut u8,
    name_max: usize,
    ver_buf: *mut u8,
    ver_max: usize,
) -> i32 {
    // Write null-terminated strings into name_buf / ver_buf
    // Return value is ignored (use 0)
    0
}
```

### `nexterm_init` (optional)

Called once after the plugin is instantiated.

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nexterm_init() {
    let ver = unsafe { api_version() };
    assert!(ver >= 2, "Host older than v2; this plugin requires v2+");
}
```

### `nexterm_on_output` (optional)

Called for every chunk of PTY output before it is sent to the client.

```
Parameters:
  pane_id:    i32  — source pane ID (also the only pane writable in v2)
  output_ptr: i32  — pointer to UTF-8 output bytes in linear memory
  output_len: i32  — byte length

Returns:
  0 — pass output through (no change)
  1 — suppress output (client does not receive it)
```

**v2 input is sanitized** (see above). **v1 input is raw bytes** including ESC sequences.

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nexterm_on_output(
    pane_id: i32,
    output_ptr: *const u8,
    output_len: usize,
) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let text = std::str::from_utf8(bytes).unwrap_or("");
    if text.contains("SECRET") { 1 } else { 0 }
}
```

> Note: the host calls this with arguments `(pane_id, ptr, len)` in that order — `pane_id` first. Refer to `examples/plugins/screen-digest/` for the canonical signature. (Some older bundled samples still declare the parameters in the legacy `(ptr, len, pane_id)` order and are pending a fix.)

### `nexterm_on_command` (optional)

Called when a user runs a `:command` via the command palette.

```
Parameters:
  cmd_ptr: i32 — pointer to `:cmd arg` formatted UTF-8 string
  cmd_len: i32 — byte length

Returns:
  0 — command handled (stop processing)
  1 — not handled (pass to next plugin)
```

In v2, `write_pane` cannot be called from this hook (allowlist is empty). Use `nexterm.log` for diagnostic output.

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nexterm_on_command(cmd_ptr: *const u8, cmd_len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len) };
    let cmd = std::str::from_utf8(bytes).unwrap_or("");
    if cmd.trim() == ":my-command" {
        // handle it
        return 0;
    }
    1
}
```

---

## Sandboxing

The plugin host applies these limits to every plugin (regardless of API version):

| Limit | Value | Rationale |
|---|---|---|
| Fuel per call | `10_000_000` instructions | Prevents infinite loops / busy waits |
| Linear memory | `MAX_MEMORY_PAGES = 256` (= 16 MiB) | Prevents `memory.grow` exhaustion |
| Imports | Only `nexterm.{api_version, log, write_pane}` and (v3) `{read_pane, read_grid, read_scrollback}` | No filesystem, network, or syscall access |

Calls that exceed fuel are aborted with `TrappedFuelExhausted`. Memory above 16 MiB at instantiation time is rejected.

---

## Memory Layout

The host uses fixed offsets for passing data to plugin hooks:

- **Hook data offset**: `64 KiB` (0x10000) — The host writes input data starting here.
- **Meta name buffer**: `64 KiB` (128 bytes max).
- **Meta version buffer**: `64 KiB + 128` (128 bytes max).

Plugins should ensure their WASM linear memory is at least **128 KiB** (default for `cdylib` targets is sufficient).

---

## Managing Plugins via `nexterm-ctl`

```sh
# List all currently loaded plugins (shows path, api_version, name, version)
nexterm-ctl plugin list

# Load a plugin from a .wasm file
nexterm-ctl plugin load /path/to/plugin.wasm

# Unload a plugin (by the same path used to load it)
nexterm-ctl plugin unload /path/to/plugin.wasm

# Reload a plugin (unload + load, picks up file changes)
nexterm-ctl plugin reload /path/to/plugin.wasm
```

---

## Auto-load at Startup

Place `.wasm` files in the plugin directory. The server loads all `.wasm` files automatically on startup.

**Default plugin directory:**
- Linux/macOS: `~/.config/nexterm/plugins/`
- Windows: `%APPDATA%\nexterm\plugins\`

Override in `config.toml`:

```toml
[plugins]
disabled = false
dir = "/opt/nexterm/plugins"
```

---

## Sample Plugins

| Sample | Location | API | Demonstrates |
|--------|----------|-----|--------------|
| `echo-suppress` | `examples/plugins/echo-suppress/` | v2 | `nexterm_meta`, `api_version`, output suppression |
| `error-detector` | `examples/plugins/error-detector/` | v2 | Error pattern detection, write_pane |
| `command-counter` | `examples/plugins/command-counter/` | v2 | Command hook, atomic state |
| `timestamp-injector` | `examples/plugins/timestamp-injector/` | v2 | Output prefix injection |
| `screen-digest` | `examples/plugins/screen-digest/` | v3 | `read_pane`, consent-gated reads |

> The bundled samples target v2, except `screen-digest`, which demonstrates the v3 read API. See `examples/plugins/README.md` for build and install steps.

---

## ABI Stability

- `PLUGIN_API_VERSION = 3` is the current stable target.
- `MIN_SUPPORTED_API_VERSION = 1` is enforced at load time. Plugins declaring versions older than `1` or newer than the host's `PLUGIN_API_VERSION` are rejected.
- `MIN_READ_API_VERSION = 3`: the read imports are only usable by plugins that declare `nexterm_api_version() >= 3`; a v1/v2 plugin calling them gets `-1`.
- v1 → v2 → v3 are **non-breaking**: older plugins continue to load and run. v3 only *adds* the read imports; v2 plugins need no changes.
- Future API revisions will increment `PLUGIN_API_VERSION`. Always export `nexterm_api_version()` and refuse to start (`nexterm_init` panic) if the host's version is below your minimum.
