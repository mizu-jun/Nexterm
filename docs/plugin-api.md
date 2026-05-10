# Nexterm WASM Plugin API

**Current API Version:** 2 (`PLUGIN_API_VERSION = 2`)
**Minimum Supported Version:** 1 (`MIN_SUPPORTED_API_VERSION = 1`)

Nexterm supports WebAssembly (WASM) plugins via the [wasmi](https://github.com/wasmi-labs/wasmi) runtime. Plugins run in a sandboxed WASM environment and communicate with the host through a stable ABI.

---

## API Versions at a Glance

| Capability | v1 (legacy) | v2 (current) |
|---|---|---|
| `nexterm_on_output` / `nexterm_on_command` input | Raw PTY bytes (incl. ESC sequences) | **Sanitized**: ESC/CSI/OSC/DCS/APC + C0 controls (except `\t\r\n`) removed |
| `nexterm.write_pane(pane_id, ...)` | Any pane allowed | **PaneId allowlist**: only the pane that emitted output (in `on_output`); none (in `on_command`) |
| Load behavior | Loads with **deprecation warning** | Loads silently |
| `nexterm_api_version` export | Optional (omitted = v1) | Should return `2` |

> **v1 plugins continue to work** via graceful downgrade. The host detects the plugin's declared API version at load time. Plugins without `nexterm_api_version` are treated as v1 and a deprecation warning is logged.
>
> v1 support will be removed in a future release. Migrate by exporting `nexterm_api_version() -> i32 = 2` and adapting to sanitized inputs / pane allowlist rules.

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
| `nexterm.api_version` | `() -> i32` | Returns `PLUGIN_API_VERSION` (currently `2`). Call to verify host capability. |
| `nexterm.log` | `(ptr: i32, len: i32)` | Write a UTF-8 string to the Nexterm log (tracing info level). |
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | Write raw bytes to the specified pane's PTY input. **In v2, restricted to allowlisted pane IDs per call** (see below). |

### Rust import declarations

```rust
#[link(wasm_import_module = "nexterm")]
unsafe extern "C" {
    fn api_version() -> i32;
    fn log(ptr: *const u8, len: usize);
    fn write_pane(pane_id: i32, ptr: *const u8, len: usize);
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

## Plugin Exports (functions the plugin must/may implement)

### `nexterm_api_version` (recommended for v2)

Declare the API version the plugin targets. Plugins that omit this export are treated as v1.

```rust
#[unsafe(no_mangle)]
pub extern "C" fn nexterm_api_version() -> i32 {
    2
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

> Note: the host calls this with arguments `(pane_id, ptr, len)` in that order. Earlier sample plugins used `(ptr, len, pane_id)`; refer to `examples/plugins/echo-suppress/` for the canonical signature.

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
| Imports | Only `nexterm.{api_version, log, write_pane}` | No filesystem, network, or syscall access |

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
| `echo-suppress` | `examples/plugins/echo-suppress/` | v1 | `nexterm_meta`, `api_version`, output suppression |
| `error-detector` | `examples/plugins/error-detector/` | v1 | Error pattern detection, write_pane |
| `command-counter` | `examples/plugins/command-counter/` | v1 | Command hook, atomic state |
| `timestamp-injector` | `examples/plugins/timestamp-injector/` | v1 | Output prefix injection |

> All bundled samples currently target v1. They load with a deprecation warning. Contributions adding v2 samples are welcome.

---

## ABI Stability

- `PLUGIN_API_VERSION = 2` is the current stable target.
- `MIN_SUPPORTED_API_VERSION = 1` is enforced at load time. Plugins declaring versions older than `1` or newer than the host's `PLUGIN_API_VERSION` are rejected.
- v1 → v2 is a **non-breaking change for v1 plugins**: they continue to load and run with legacy behavior plus a one-line deprecation warning.
- Future API revisions will increment `PLUGIN_API_VERSION`. Always export `nexterm_api_version()` and refuse to start (`nexterm_init` panic) if the host's version is below your minimum.
