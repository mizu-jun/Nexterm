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
| `nexterm.api_version` | `() -> i32` | Returns `PLUGIN_API_VERSION` (host current: `2`) |
| `nexterm.log` | `(ptr: i32, len: i32)` | Write to nexterm log |
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | Write text to a pane |

### Optional exports

| Export | Signature | Notes |
|--------|-----------|-------|
| `nexterm_api_version` | `() -> i32` | **v2 必須**: 宣言する API バージョン。未エクスポートまたは `1` の場合は v1 として扱われ deprecation 警告が出る |
| `nexterm_meta` | `(name_buf: i32, name_max: i32, ver_buf: i32, ver_max: i32) -> i32` | Plugin name/version for `nexterm-ctl plugin list` |

---

## Plugin API v1 → v2 移行ガイド（Sprint 5-4 / F1）

`PLUGIN_API_VERSION = 2` の本体に合わせて、サンプルプラグイン 4 件は v2 化されました。

### v1 と v2 の挙動差

| 項目 | v1 (legacy) | v2 (recommended) |
|------|------------|-----------------|
| `nexterm_api_version` | 未エクスポート / `1` を返す | `2` を返す |
| `nexterm_on_output` の `pane_id` | サニタイズ無し（不正な値が渡る可能性） | サニタイズ済みの正しい pane_id |
| `write_pane` 許可リスト | チェック無し（任意の pane に書き込み可能） | `allowed_panes` リストに登録された pane のみ |
| クリップボード書き込み (OSC 52) | 自動許可 | ホスト側で許可リスト検証 |
| 通知発行 | 自動許可 | ホスト側で許可リスト検証 |
| Deprecation 警告 | ログに 1 回出力される | なし |

### 移行手順（既存 v1 プラグインを v2 化する）

1. ソースに `nexterm_api_version()` エクスポートを追加:

   ```rust
   #[no_mangle]
   pub extern "C" fn nexterm_api_version() -> i32 {
       2
   }
   ```

2. `write_pane` を呼ぶ場合、書き込み先 pane を allowed_panes に登録する
   （ホスト側で `register_pane(plugin_id, pane_id)` を呼ぶ — v2 で新設予定）

3. Cargo.toml のバージョンを `0.2.0` 以上にバンプ

4. `cargo build --release --target wasm32-unknown-unknown` で再ビルド

### v1 サポートの終了予定

Plugin v1 サポートは **v2.0 リリース時に削除予定** です。それまでは v1 プラグインも
load 可能ですが、deprecation 警告がログに記録されます。新規プラグインは v2 で作成してください。

See [docs/plugin-api.md](../../docs/plugin-api.md) for the full API reference.
