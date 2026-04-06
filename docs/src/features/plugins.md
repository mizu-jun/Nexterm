# WASM Plugins

Nexterm has a built-in **WebAssembly (WASM)** plugin system.
Plugins run in a sandboxed WASM environment with no direct system access.

## How It Works

```
PTY output  → PluginManager.on_output()  → each plugin's nexterm_on_output()
Command     → PluginManager.on_command() → each plugin's nexterm_on_command()
```

### Host Import API

Functions that plugins can call into Nexterm:

| Function | Signature | Description |
|----------|-----------|-------------|
| `nexterm.log` | `(ptr: i32, len: i32)` | Log a message (written to nexterm-server's log) |
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | Write text to a pane |

---

## Writing a Plugin

Example of a Rust-based WASM plugin:

```toml
# Cargo.toml
[package]
name = "my-nexterm-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "s"
lto = true
```

```rust
// src/lib.rs
use std::sync::Mutex;

// Global state (if needed)
static COUNTER: Mutex<u64> = Mutex::new(0);

/// Callback that receives PTY output
/// output: bytes written to the pane (UTF-8 text)
/// pane_id: pane identifier
#[no_mangle]
pub extern "C" fn nexterm_on_output(output_ptr: i32, output_len: i32, pane_id: i32) {
    let output = unsafe {
        std::slice::from_raw_parts(output_ptr as *const u8, output_len as usize)
    };
    let text = std::str::from_utf8(output).unwrap_or("");

    // Example: detect "error" in output and log it
    if text.contains("error") {
        let msg = format!("Error detected: pane_id={}\0", pane_id);
        unsafe { nexterm_log(msg.as_ptr() as i32, msg.len() as i32 - 1); }
    }
}

/// Callback that receives command input
#[no_mangle]
pub extern "C" fn nexterm_on_command(cmd_ptr: i32, cmd_len: i32, pane_id: i32) {
    let _ = (cmd_ptr, cmd_len, pane_id);
    // Can be used to record command history, etc.
}

// Functions provided by the host
extern "C" {
    fn nexterm_log(ptr: i32, len: i32);
    fn nexterm_write_pane(pane_id: i32, ptr: i32, len: i32);
}
```

```bash
# Compile to WASM
cargo build --target wasm32-unknown-unknown --release
# → target/wasm32-unknown-unknown/release/my_nexterm_plugin.wasm
```

---

## Installing a Plugin

```bash
# Place the plugin in the plugin directory
mkdir -p ~/.config/nexterm/plugins
cp my_nexterm_plugin.wasm ~/.config/nexterm/plugins/
```

The plugin is auto-loaded on the next Nexterm restart (or server restart).

---

## Configuration

```toml
# nexterm.toml

# Custom plugin directory (default: ~/.config/nexterm/plugins)
plugin_dir = "/path/to/plugins"

# Disable all plugins entirely
plugins_disabled = false
```

---

## Debugging

Plugin log messages are written to the nexterm-server log:

```bash
# Linux / macOS
journalctl --user -u nexterm-server -f
# or
tail -f ~/.local/share/nexterm/nexterm-server.log

# Windows
Get-Content "$env:LOCALAPPDATA\nexterm\nexterm-server.log" -Wait
```
