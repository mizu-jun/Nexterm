# WASM プラグイン

Nexterm は **WebAssembly (WASM)** ベースのプラグインシステムを内蔵しています。
プラグインはサンドボックス化された WASM 環境（wasmi）で動作するため、システムへの直接アクセスはできません。

## API バージョン

- **現行**: `PLUGIN_API_VERSION = 2`
- **後方互換**: `MIN_SUPPORTED_API_VERSION = 1` で v1 プラグインも継続サポート（deprecation 警告付き）

詳細仕様: [docs/plugin-api.md](../../plugin-api.md) / 移行手順: [docs/MIGRATION.md](../../MIGRATION.md)

### v1 / v2 の主な違い

| 機能 | v1 | v2 |
|------|----|----|
| 入力データ | 生バイト列（ESC 含む） | **サニタイズ済み**（ESC/CSI/OSC/DCS/APC + C0 制御を除去、`\t\r\n` 除く） |
| `write_pane` 宛先 | 任意の `pane_id` 可 | **PaneId 許可リスト**: `on_output(pane_id)` 中はその ID のみ、`on_command` 中は不可 |
| ロード時 | deprecation 警告ログ | サイレント |

## プラグインの仕組み

```
PTY 出力 → PluginManager.on_output() → 各プラグインの nexterm_on_output()
コマンド → PluginManager.on_command() → 各プラグインの nexterm_on_command()
```

### ホストインポート API

プラグインから Nexterm の機能を呼び出せるインポートです。

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `nexterm.api_version` | `() -> i32` | ホストの API バージョンを返す（現行: `2`）|
| `nexterm.log` | `(ptr: i32, len: i32)` | ログメッセージ（nexterm-server のログに出力）|
| `nexterm.write_pane` | `(pane_id: i32, ptr: i32, len: i32)` | ペインへのテキスト書き込み（v2 では許可リストでフィルタ）|

---

## プラグインの作成

Rust で WASM プラグインを作成する例:

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

// グローバル状態（必要な場合）
static COUNTER: Mutex<u64> = Mutex::new(0);

/// PTY 出力を受け取るコールバック
/// output: ペインに出力されたバイト列（UTF-8 テキスト）
/// pane_id: ペイン ID
#[no_mangle]
pub extern "C" fn nexterm_on_output(output_ptr: i32, output_len: i32, pane_id: i32) {
    let output = unsafe {
        std::slice::from_raw_parts(output_ptr as *const u8, output_len as usize)
    };
    let text = std::str::from_utf8(output).unwrap_or("");

    // 例: "error" を含む出力を検出してログに記録する
    if text.contains("error") {
        let msg = format!("エラーを検出: pane_id={}\0", pane_id);
        unsafe { nexterm_log(msg.as_ptr() as i32, msg.len() as i32 - 1); }
    }
}

/// コマンド入力を受け取るコールバック
#[no_mangle]
pub extern "C" fn nexterm_on_command(cmd_ptr: i32, cmd_len: i32, pane_id: i32) {
    let _ = (cmd_ptr, cmd_len, pane_id);
    // コマンド履歴の記録などに使用可能
}

// ホストから提供される関数
extern "C" {
    fn nexterm_log(ptr: i32, len: i32);
    fn nexterm_write_pane(pane_id: i32, ptr: i32, len: i32);
}
```

```bash
# WASM にコンパイル
cargo build --target wasm32-unknown-unknown --release
# → target/wasm32-unknown-unknown/release/my_nexterm_plugin.wasm
```

---

## プラグインのインストール

```bash
# プラグインディレクトリに配置
mkdir -p ~/.config/nexterm/plugins
cp my_nexterm_plugin.wasm ~/.config/nexterm/plugins/
```

Nexterm 再起動（またはサーバー再起動）でプラグインが自動ロードされます。

---

## 設定

```toml
# nexterm.toml

# カスタムプラグインディレクトリ（デフォルト: ~/.config/nexterm/plugins）
plugin_dir = "/path/to/plugins"

# プラグインを完全に無効にする
plugins_disabled = false
```

---

## デバッグ

プラグインのログは nexterm-server のログに出力されます:

```bash
# Linux / macOS
journalctl --user -u nexterm-server -f
# または
tail -f ~/.local/share/nexterm/nexterm-server.log

# Windows
Get-Content "$env:LOCALAPPDATA\nexterm\nexterm-server.log" -Wait
```
