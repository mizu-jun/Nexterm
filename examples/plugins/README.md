# Nexterm WASM Plugin Examples

Four ready-to-build sample plugins demonstrating the Nexterm WASM plugin API.

## Prerequisites

```sh
rustup target add wasm32-unknown-unknown
```

## Build All Samples

```sh
# From this directory
for dir in echo-suppress error-detector command-counter timestamp-injector; do
  (cd "$dir" && cargo build --release --target wasm32-unknown-unknown)
done
```

Output files:
```
echo-suppress/target/wasm32-unknown-unknown/release/echo_suppress.wasm
error-detector/target/wasm32-unknown-unknown/release/error_detector.wasm
command-counter/target/wasm32-unknown-unknown/release/command_counter.wasm
timestamp-injector/target/wasm32-unknown-unknown/release/timestamp_injector.wasm
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

---

## Writing Your Own Plugin

Any language that compiles to `wasm32-unknown-unknown` works.

### Required exports

| Export | Signature | Notes |
|--------|-----------|-------|
| `nexterm_init` | `() -> ()` | Optional. Called once on load. |
| `nexterm_on_output` | `(ptr: i32, len: i32, pane_id: i32) -> i32` | Return 0 = pass through, 1 = suppress |
| `nexterm_on_command` | `(ptr: i32, len: i32) -> i32` | Return 0 = handled, 1 = not handled |

### Host imports (`nexterm` module)

| Import | Signature | Notes |
|--------|-----------|-------|
| `nexterm.api_version` | `() -> i32` | Returns `PLUGIN_API_VERSION` (currently `1`) |
| `nexterm.log` | `(ptr: i32, len: i32)` | Write to nexterm log |
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | Write text to a pane |

### Optional exports

| Export | Signature | Notes |
|--------|-----------|-------|
| `nexterm_meta` | `(name_buf: i32, name_max: i32, ver_buf: i32, ver_max: i32) -> i32` | Plugin name/version for `nexterm-ctl plugin list` |

See [docs/plugin-api.md](../../docs/plugin-api.md) for the full API reference.
