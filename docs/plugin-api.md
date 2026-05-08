# Nexterm WASM Plugin API

**API Version:** 1 (`PLUGIN_API_VERSION = 1`)

Nexterm supports WebAssembly (WASM) plugins via the [wasmi](https://github.com/wasmi-labs/wasmi) runtime. Plugins run in a sandboxed WASM environment and communicate with the host through a stable ABI.

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

# Check loaded plugins
nexterm-ctl plugin list

# Unload
nexterm-ctl plugin unload ./target/wasm32-unknown-unknown/release/echo_suppress.wasm
```

---

## Host Imports (functions the plugin can call)

| Import | Signature | Description |
|--------|-----------|-------------|
| `nexterm.api_version` | `() -> i32` | Returns `PLUGIN_API_VERSION` (currently `1`). Call this in `nexterm_init` to verify compatibility. |
| `nexterm.log` | `(ptr: i32, len: i32)` | Write a UTF-8 string to the Nexterm log (tracing info level). |
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | Write raw bytes to the specified pane's PTY input. |

### Rust import declarations

```rust
#[link(wasm_import_module = "nexterm")]
extern "C" {
    fn api_version() -> i32;
    fn log(ptr: *const u8, len: usize);
    fn write_pane(pane_id: i32, ptr: *const u8, len: usize);
}
```

---

## Plugin Exports (functions the plugin must/may implement)

### `nexterm_meta` (optional)

Publish plugin name and version to the host. Shown in `nexterm-ctl plugin list`.

```rust
#[no_mangle]
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
#[no_mangle]
pub extern "C" fn nexterm_init() {
    let ver = unsafe { api_version() };
    assert_eq!(ver, 1, "Unsupported API version");
}
```

### `nexterm_on_output` (optional)

Called for every chunk of PTY output before it is sent to the client.

```
Parameters:
  output_ptr: i32  — pointer to UTF-8 output bytes in linear memory
  output_len: i32  — byte length
  pane_id:    i32  — source pane ID

Returns:
  0 — pass output through (no change)
  1 — suppress output (client does not receive it)
```

```rust
#[no_mangle]
pub extern "C" fn nexterm_on_output(
    output_ptr: *const u8,
    output_len: usize,
    pane_id: i32,
) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let text = std::str::from_utf8(bytes).unwrap_or("");
    if text.contains("SECRET") { 1 } else { 0 }
}
```

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

```rust
#[no_mangle]
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

## Memory Layout

The host uses a fixed offset for passing data to plugin hooks:

- **Hook data offset**: `64 KiB` (0x10000) — The host writes input data starting here.
- **Meta buffers**: `64 KiB` for name (128 bytes), `64 KiB + 128` for version (128 bytes).

Plugins should ensure their WASM linear memory is at least **128 KiB** (default for `cdylib` targets).

---

## Managing Plugins via `nexterm-ctl`

```sh
# List all currently loaded plugins
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

| Sample | Location | Demonstrates |
|--------|----------|--------------|
| `echo-suppress` | `examples/plugins/echo-suppress/` | `nexterm_meta`, `api_version`, output suppression |
| `error-detector` | `examples/plugins/error-detector/` | Error pattern detection, write_pane |
| `command-counter` | `examples/plugins/command-counter/` | Command hook, atomic state |
| `timestamp-injector` | `examples/plugins/timestamp-injector/` | Output prefix injection |

---

## ABI Stability

`PLUGIN_API_VERSION = 1` is frozen. Future breaking changes will increment this number. Always call `api_version()` in `nexterm_init` and refuse to load if the version does not match your expectations.
