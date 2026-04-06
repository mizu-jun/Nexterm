# WASM Plugins

Nexterm has a built-in **WebAssembly (WASM)** plugin system.
Plugins run in a sandboxed `wasmi` runtime with no direct system access —
they can only interact with Nexterm through the defined host import API.

## How It Works

```
PTY output  → PluginManager.on_output()  → each plugin's nexterm_on_output()
Command     → PluginManager.on_command() → each plugin's nexterm_on_command()
```

All plugins receive the same events in registration order.
A plugin can suppress output (return 1 from `nexterm_on_output`) to replace it.

---

## Plugin ABI Reference

### Exports (your plugin must implement these)

| Export | Signature | Notes |
|--------|-----------|-------|
| `nexterm_init` | `() -> ()` | Optional. Called once when the plugin loads. |
| `nexterm_on_output` | `(ptr: i32, len: i32, pane_id: i32) -> i32` | Return **0** = pass through, **1** = suppress original output |
| `nexterm_on_command` | `(ptr: i32, len: i32) -> i32` | Return **0** = handled, **1** = not handled (pass to next plugin) |

### Host Imports (`nexterm` module)

| Function | Signature | Description |
|----------|-----------|-------------|
| `nexterm.log` | `(ptr: i32, len: i32)` | Write a UTF-8 string to the nexterm-server log |
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | Write UTF-8 text to a pane (`pane_id=0` = focused pane) |
| `nexterm.now_ms` | `() -> i64` | Current Unix timestamp in milliseconds |

---

## Quick Start (Rust)

### 1. Create a new crate

```sh
cargo new --lib my-plugin
cd my-plugin
```

### 2. Edit `Cargo.toml`

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "s"
lto = true
strip = true
```

### 3. Write `src/lib.rs`

```rust
extern "C" {
    fn nexterm_log(ptr: *const u8, len: usize);
    fn nexterm_write_pane(pane_id: i32, ptr: *const u8, len: usize);
}

fn log(msg: &str) {
    let b = msg.as_bytes();
    unsafe { nexterm_log(b.as_ptr(), b.len()) };
}

#[no_mangle]
pub extern "C" fn nexterm_init() {
    log("[my-plugin] loaded");
}

#[no_mangle]
pub extern "C" fn nexterm_on_output(
    output_ptr: *const u8,
    output_len: usize,
    pane_id: i32,
) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let text = std::str::from_utf8(bytes).unwrap_or("");

    if text.contains("TODO") {
        log(&format!("[my-plugin] TODO found in pane {}", pane_id));
    }

    0 // pass through
}

#[no_mangle]
pub extern "C" fn nexterm_on_command(cmd_ptr: *const u8, cmd_len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len) };
    let cmd = std::str::from_utf8(bytes).unwrap_or("").trim();

    if cmd == ":hello" {
        let msg = b"Hello from my-plugin!\r\n";
        unsafe { nexterm_write_pane(0, msg.as_ptr(), msg.len()) };
        return 0; // handled
    }

    1 // not handled
}
```

### 4. Build and install

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown

mkdir -p ~/.config/nexterm/plugins
cp target/wasm32-unknown-unknown/release/my_plugin.wasm \
   ~/.config/nexterm/plugins/
```

### 5. Register in `nexterm.toml`

```toml
[[plugins]]
path = "~/.config/nexterm/plugins/my_plugin.wasm"
```

Restart `nexterm-server` to load the plugin.

---

## Writing Plugins in Other Languages

Any language that compiles to `wasm32-unknown-unknown` (bare WASM, no WASI) works.

### C example (using clang)

```c
// plugin.c
#include <stdint.h>
#include <string.h>

__attribute__((import_module("nexterm"), import_name("log")))
extern void nexterm_log(const uint8_t* ptr, int32_t len);

__attribute__((export_name("nexterm_on_output")))
int32_t nexterm_on_output(const uint8_t* ptr, int32_t len, int32_t pane_id) {
    (void)pane_id;
    // search for "panic" in output
    if (memmem(ptr, len, "panic", 5) != NULL) {
        const char* msg = "[c-plugin] panic detected!";
        nexterm_log((const uint8_t*)msg, strlen(msg));
    }
    return 0;
}

__attribute__((export_name("nexterm_on_command")))
int32_t nexterm_on_command(const uint8_t* ptr, int32_t len) {
    (void)ptr; (void)len;
    return 1; // not handled
}
```

```sh
clang --target=wasm32 -nostdlib -Wl,--no-entry \
  -Wl,--export=nexterm_on_output \
  -Wl,--export=nexterm_on_command \
  -o plugin.wasm plugin.c
```

---

## Example Plugins

Three ready-to-build samples are in `examples/plugins/`:

| Plugin | What it does |
|--------|--------------|
| `error-detector` | Highlights lines containing "error"; `:error-reset` clears count |
| `command-counter` | Tracks OSC 133 D marks; `:count-show` / `:count-reset` |
| `timestamp-injector` | Prepends HH:MM:SS.mmm to output lines; `:ts-on` / `:ts-off` |

Build all three:

```sh
cd examples/plugins
for dir in error-detector command-counter timestamp-injector; do
  (cd "$dir" && cargo build --release --target wasm32-unknown-unknown)
done
```

---

## Installing a Plugin

```sh
mkdir -p ~/.config/nexterm/plugins
cp my_plugin.wasm ~/.config/nexterm/plugins/
```

Declare it in `nexterm.toml`:

```toml
[[plugins]]
path = "~/.config/nexterm/plugins/my_plugin.wasm"

# Optional: disable without removing
# enabled = false
```

---

## Debugging

Plugin `nexterm.log` calls appear in the server log:

```sh
# Linux / macOS
NEXTERM_LOG=debug nexterm-server 2>&1 | grep '\[plugin\]'

# Structured log file
tail -f /tmp/nexterm.log   # if [log] file is set in nexterm.toml
```
