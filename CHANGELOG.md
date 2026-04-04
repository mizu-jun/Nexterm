# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.5] - 2026-04-05

### Fixed

**Windows — GPU クライアントのフォントが正しく描画されるようになった**

- `cell_w = font_size * 0.6` という誤った固定係数を廃止し、基準文字 `'0'` を
  実際にラスタライズして advance width を計測する方式に変更。
  "Wi ndows Power She l l" のような文字間の余分なスペースが解消される。
- `FontManager::new()` に `scale_factor: f32` パラメーターを追加し、
  winit の `window.scale_factor()` を渡すことで DPI 拡大率（125%・150%）に
  応じた正確なフォントサイズを計算するようになった。
- `rasterize_char` クロージャ内で `x as u32`（負値のラップ）していた
  バグを修正し、`if x < 0 || y < 0 { return; }` ガードを追加。
- `WindowEvent::ScaleFactorChanged` を処理し、DPI 変更時にフォントと
  グリフアトラスを自動再生成するようになった。

**Windows 11 — GPU クライアントに Acrylic すりガラス背景を追加**

- `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMWCP_ACRYLIC)` を呼び出し、
  Windows Terminal に似たすりガラス効果をウィンドウ背景に適用。
- wgpu Surface の composite alpha mode を `PreMultiplied` に設定し、
  透明合成が正しく動作するようにした。
- Windows 10 や他 OS では追加コードは実行されず、動作に影響しない。

---

## [0.5.4] - 2026-04-05

### Fixed

**Windows — 起動時のコンソールウィンドウが表示されなくなった**

`nexterm.exe`、`nexterm-server`、`nexterm-client-gpu` にリリースビルド限定で
`#[windows_subsystem = "windows"]` 属性を追加。MSI インストーラーや
エクスプローラーから `nexterm.exe` を起動した際に、ターミナルウィンドウ以外の
余分なコンソールウィンドウが表示されなくなった。

- ログは `%LOCALAPPDATA%\nexterm\nexterm-server.log` / `nexterm-client.log`
  に日次ローテーションで書き出す（`tracing-appender` 採用）。
- エラーは `MessageBoxW` ダイアログで通知する。

**macOS — バイナリが ad-hoc 署名済みになり、Intel Mac に対応**

- すべての macOS リリースバイナリを `codesign --sign -`（ad-hoc）で署名。
  `xattr -dr com.apple.quarantine <ファイル>` を実行するだけで
  Gatekeeper をバイパスして起動できる。
- `macos-13`（Intel ランナー）で `x86_64-apple-darwin` ターゲットをビルドし
  `nexterm-vX.Y.Z-macos-x86_64.tar.gz` をリリースアセットに追加。

---

## [0.5.1] - 2026-03-31

### Fixed — Windows build & test (4 bugs)

This patch release fixes compilation and test failures that prevented the
Windows binary from being produced in the v0.5.0 release workflow.

| # | Crate / file | Root cause | Fix |
|---|---|---|---|
| 1 | `nexterm-launcher/Cargo.toml` | `windows-sys 0.59` split `CreateFileW` security descriptor handling into a separate `Win32_Security` feature; the feature was missing from the dependency declaration | Added `"Win32_Security"` to the `windows-sys` features list |
| 2 | `nexterm-launcher/src/main.rs` | `GENERIC_READ` was imported from `Win32::Storage::FileSystem`; in `windows-sys 0.59` it was moved to `Win32::Foundation` | Moved `GENERIC_READ` (and `INVALID_HANDLE_VALUE`) to the `Win32::Foundation` use statement |
| 3 | `nexterm-server/src/pane.rs` | `portable_pty` imports were guarded with `#[cfg(unix)]`, preventing `MasterPty`, `NativePtySystem`, `PtySize`, and `CommandBuilder` from being compiled on Windows even though `portable_pty` supports ConPTY on Windows | Removed the `#[cfg(unix)]` attribute from the `portable_pty` use statement |
| 4 | `nexterm-server/src/ipc.rs` | Path-validation unit tests used Unix-style absolute paths (`/home/user/…`, `/etc/passwd`, `/tmp/…`) which are **not** recognised as absolute by `std::path::Path::is_absolute()` on Windows, causing the "reject forbidden absolute paths" test to pass silently for the wrong reason | Added `#[cfg(unix)]` / `#[cfg(windows)]` guards; Windows tests use `%TEMP%\nexterm\…` and `D:\secret\…` / `C:\Windows\System32\…` style paths |

**All 93 unit tests now pass on `x86_64-pc-windows-msvc`.**

---

## [0.5.0] - 2026-03-27

### Added

**SSH & Connectivity**
- SSH multi-tab connections — SSH Host Manager (`Ctrl+Shift+H`) opens each host in a new tab
- X11 forwarding — `x11_forward = true` / `x11_trusted = true` in `[[hosts]]` (equivalent to `ssh -X` / `ssh -Y`)

**UX**
- In-app Settings GUI — `Ctrl+,` opens a Font / Colors / Window panel; changes write back to `nexterm.toml` instantly
- Settings action added to command palette (now 17 actions)

**Web Terminal**
- Embedded web terminal — `[web] enabled = true`; xterm.js served at `ws://localhost:7681`
- Token-based auth (`token = "..."` in config), disabled by default

**Package Distribution**
- Homebrew tap formula (`pkg/homebrew/nexterm.rb`)
- Scoop bucket manifest (`pkg/scoop/nexterm.json`)
- winget manifest (`pkg/winget/mizu-jun.Nexterm.yaml`)
- GitHub Pages documentation site auto-deployed via CI

---

## [0.4.0] - 2026-01-15

### Added

**SSH & Connectivity**
- SSH Host Manager — fuzzy-searchable host list (`Ctrl+Shift+H`); connects with one keystroke
- SFTP Upload / Download dialogs (`Ctrl+Shift+U` / `Ctrl+Shift+D`) with live progress bar
- Remote port forwarding (`-R`) over SSH sessions
- Serial port connections (`ConnectSerial` via command palette)

**UX & Pane Management**
- Command palette (Ctrl+Shift+P) extended with 16 actions including SFTP and host manager
- Lua Macro Picker — fuzzy-searchable macro list (`Ctrl+Shift+M`); one-key execution
- Quick Select mode (`Ctrl+Shift+Space`) — highlight URLs, paths, IPs, and hashes
- Pane zoom toggle (`Ctrl+B Z`) — focus a single pane full-screen
- Swap pane with next/previous sibling (`Ctrl+B {` / `Ctrl+B }`)
- Break pane to new window (`Ctrl+B !`)

**Automation**
- Lua event hooks: `on_session_start`, `on_attach`, `on_pane_open`
- Lua Macro engine: define `[[macros]]` in TOML, execute via picker

**Logging**
- Log filename templates (`{session}`, `{date}`, `{time}` placeholders)
- Binary PTY log mode

**Windows**
- MSI installer built with WiX Toolset v3 (CI-automated)
- Windows Service install/uninstall scripts
- Automatic code signing via `signtool.exe` when CI secrets are configured
- `nexterm-launcher` — single `nexterm.exe` auto-starts server + opens GPU client

---

## [0.3.0] - 2025-11-20

### Added

**SSH & Security**
- Known-hosts host key verification
- SSH agent authentication via `SSH_AUTH_SOCK`
- Local port forwarding through SSH tunnels
- ProxyJump multi-hop connection support
- SOCKS5 proxy support

**Terminal & Display**
- Full alternate screen buffer support (SMCUP/RMCUP)
- OSC 0/1/2 window title support
- OSC 9 desktop notifications
- CJK wide character rendering fixes

**GPU Client**
- IME input support (Japanese, Chinese, Korean)
- Keybinding customization
- Right-click context menu (Copy/Paste/Split/ClosePane)
- Pane number overlay in display-panes mode
- Mouse selection with automatic clipboard copy

---

## [0.2.0] - 2025-09-10

### Added
- GPU-accelerated renderer using wgpu + cosmic-text
- Command palette (`Ctrl+Shift+P`) with initial 8 actions
- Split pane: horizontal (`Ctrl+B %`) and vertical (`Ctrl+B "`)
- Scrollback buffer with configurable history size
- Basic session save / restore (JSON snapshots)

---

## [0.1.0] - 2025-07-01

### Added
- Initial release
- TUI client (`nexterm-client-tui`) using ratatui + crossterm
- IPC protocol between server and client (`nexterm-proto`)
- VT parser (`nexterm-vt`) with ANSI/xterm sequence support
- SSH client (`nexterm-ssh`) via `russh`
- TOML configuration (`nexterm-config`)
- i18n support for 8 languages (`nexterm-i18n`)
- `nexterm-ctl` CLI for session management
