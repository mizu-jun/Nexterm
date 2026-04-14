# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

---

## [0.9.4] - 2026-04-14

### Fixed

- **PowerShell クラッシュ修正**: `nexterm-vt` の `erase_in_line` / `erase_in_display` / `scroll_up` で直接配列インデックスアクセスを使っていた箇所を `Grid::clear_row()` / `Grid::copy_row()` 安全メソッドに置換。PSReadLine が送る複雑な VT シーケンスによる IndexError パニックを防止。

### Added

- **設定パネル マウス操作**: サイドバーカテゴリ・フォントサイズ/不透明度スライダー・テーマカラードットをマウスでクリック・ドラッグ操作可能に。スライダーのドラッグ終了時に自動保存。パネル外クリックで閉じる。

### Changed

- **ターミナル透過表示**: ターミナル背景がデフォルト 95% 不透明（`background_opacity = 0.95`）になり、背景が薄く透けるようになった。設定パネルとコンテキストメニューは常に完全不透明を維持。`nexterm.toml` の `[window] background_opacity` で 0.1〜1.0 の範囲で調整可能。
- **メモリ使用量削減**: `cosmic-text` の `FontSystem` 初期化をシステム全スキャンから OS 別フォントディレクトリ絞り込みロードに変更（macOS: `/System/Library/Fonts`、Windows: `C:\Windows\Fonts`）。推定 ~30-40MB のメモリ削減。

---

## [0.8.0] - 2026-04-06

### Added

**Web Terminal: OAuth2 / SSO authentication**
- OAuth2/OIDC support for GitHub, Google, Azure AD, and any generic OIDC provider.
- Authorization Code Flow with CSRF protection (state parameter, 10-minute TTL).
- Access control via `allowed_emails` and `allowed_orgs` (GitHub only).
- Client secret can be set via `NEXTERM_OAUTH_CLIENT_SECRET` environment variable (recommended over storing in `nexterm.toml`).
- OAuth login button automatically injected into the login page when OAuth is enabled.

**Web Terminal: session management improvements**
- Configurable session TTL via `[web.auth] session_timeout_secs` (default: 86400 s = 24 h).
- Concurrent session limit via `[web] max_sessions` (0 = unlimited); oldest session is evicted when limit is reached.
- Explicit logout endpoint: `POST /auth/logout` revokes the session cookie.

**Web Terminal: HTTPS enforcement**
- New `[web] force_https = true` option; checks `X-Forwarded-Proto` and issues 301 redirects for HTTP requests (useful behind a TLS-terminating reverse proxy).

**Web Terminal: access log**
- New `[web.access_log]` section; logs every request (including WebSocket upgrades and failed auth attempts).
- CSV output to a configurable file path, or to the server log via `tracing` when no file is set.
- Fields: `timestamp`, `remote_addr`, `method`, `path`, `status`, `auth_method`, `user_id`.

**TUI client: multi-pane support**
- Ctrl+B prefix key system for pane management.
- Horizontal/vertical split, focus cycling, pane close, zoom.
- Status bar showing active session and pane count.
- Full help overlay (Ctrl+B ?).

**SSH host manager enhancements**
- Tag-based filtering and group management.
- Connection history with frequency-based sorting.
- Bulk operations (connect all in group, disconnect all).

**WASM plugin examples**
- Three ready-to-build sample plugins: `error-detector`, `command-counter`, `timestamp-injector`.
- Full plugin documentation including C and Rust examples.

**Documentation**
- Quickstart guide improvements, configuration snippet collection, Lua macro recipe collection.
- Full web terminal authentication reference including enterprise GitHub SSO example.

### Changed

- `[web.auth]` now contains `session_timeout_secs` field (previously hardcoded to 24 h).
- `[web]` has new fields: `max_sessions`, `force_https`, `access_log`.
- `nexterm-config`: `OAuthConfig` and `AccessLogConfig` are now publicly exported.

---

## [0.7.6] - 2026-04-06

### Added

**GPU client: TUI-parity tab bar and settings panel**
- Tab bar now displays the OSC 0/2 window title (e.g. current working directory) in each tab label, matching the TUI client behaviour.
- "⚙ Settings" button rendered on the right side of the tab bar; clicking it toggles the settings panel without a keyboard shortcut.
- Mouse click hit-testing on tab bar: clicking a tab switches the active pane; clicking the settings button opens/closes the panel.
- Settings panel font family field is now fully editable: press **F** (on Font tab) to enter edit mode, type the family name, **Backspace** to delete, **Enter** to confirm, **Escape** to cancel. Characters are intercepted before forwarding to the server.
- `PaneState` carries a `title: String` field updated by `ServerToClient::TitleChanged` messages.
- `ClientState` carries `tab_hit_rects` and `settings_tab_rect` populated each frame by `build_tab_bar_verts`.

### Changed

- `render()` and `build_tab_bar_verts()` now take `&mut ClientState` to allow per-frame hit-rect writes.

### Documentation

- All documentation converted to English as the primary language.
- Japanese translations added for user-facing docs: `shaders.ja.md`, `performance.ja.md`, `graphics.ja.md`, `plugins.ja.md`.
- `docs/ARCHITECTURE.md` and `docs/CONFIGURATION.md` fully translated to English.

---

## [0.7.5] - 2026-04-06

### Fixed

**GPU client: rendering quality pass**
- Added status bar height (`cell_h`) to the `visible_rows` calculation in scrollback view, fixing overlap between the last row and the status bar.
- `ScaleFactorChanged` (DPI change) event now recalculates `cols`/`rows` and sends a Resize notification to the server, resolving layout shift when moving to a high-DPI display.
- Applied `tab_bar_h` offset to the right-click context menu y-coordinate, fixing menu position when the tab bar is enabled.
- Added `cleared_this_frame` flag to `GlyphAtlas`; resetting the flag at the start of each frame prevents glyph corruption from stale UV coordinates after an atlas overflow mid-frame.
- Pre-declared `family_owned` for all code paths in `font.rs` to clarify lifetime structure.

---

## [0.7.4] - 2026-04-06

### Fixed

**GPU client (Windows): fix CJK full-width character spacing**
- Added `wide: bool` parameter to `rasterize_char()`; full-width characters (Unicode width ≥ 2) now render into a 2-cell buffer (`display_cols = 2.0`).
- Added `wide` field to `GlyphKey` so full-width and half-width glyphs are cached separately in the atlas.
- Japanese, Chinese, Korean, and other CJK characters are now evenly spaced and correctly rendered.

**GPU client (Windows): fix tab bar / terminal content overlap**
- Fixed tab bar (at y=0) and row-1 terminal content being drawn at the same y-coordinate.
- Added `y_offset: f32` parameter to `build_grid_verts` / `build_scrollback_verts`.
- Multi-pane `_in_rect` functions updated to use `off_y = row_offset * cell_h + tab_bar_h`.
- Pane borders and number badges now account for the tab bar height.

**GPU client (Windows): fix black band on the right side**
- The `rows` calculation was using the full window height, causing overlap with the tab bar and status bar.
- Fixed with `rows = (height - tab_bar_h - status_bar_h) / cell_h` for accurate usable row count.
- Corrected in both the initial window setup and resize event handler.
- Mouse click → cell coordinate conversion now subtracts `tab_bar_h` for accurate row targeting.

---

## [0.7.3] - 2026-04-06

### Fixed

**GPU client (Windows): fix font character spacing**
- `Attrs::new()` defaulted to `Family::SansSerif`, causing fallback to a proportional font (Segoe UI, etc.) on Windows.
- `measure_char_width` and `rasterize_char` now explicitly set `Family::Monospace` or `Family::Name(family)`.
- Config font name `"monospace"` maps to `Family::Monospace` (fontdb selects the system monospace font); specific names (`Consolas`, `JetBrains Mono`, etc.) use `Family::Name` directly.
- Cell width measurement switched from `Buffer::draw()` ink pixels to `layout_runs()` advance width, which includes right bearing for accurate character spacing.
- Eliminates the "Wi ndows PowerShe l l" extra-space rendering bug.

**Shader hot-reload, gallery, and migration tools**
- Added `WgpuState::reload_shader_pipelines()`: hot-reloads WGSL shaders on file change (no restart needed).
- `examples/shaders/`: bundled sample WGSL shaders — CRT, Matrix, Glow (background) / Grayscale, Amber (text).
- `nexterm-ctl import-ghostty`: imports a Ghostty config file and converts it to nexterm config.
- `nexterm-ctl service install/uninstall/status`: manages autostart services via systemd (Linux) / launchd (macOS).

---

## [0.7.2] - 2026-04-05

### Added

**Custom WGSL shader support**
- Added `[gpu]` section to `nexterm-config` (`custom_bg_shader` / `custom_text_shader` / `fps_limit` / `atlas_size`).
- GPU client loads WGSL files from the specified paths at startup (falls back to built-in shaders on failure).
- Enables custom effects such as CRT scanlines and glow.

**Documentation site expansion**
- `docs/src/features/graphics.md`: Sixel / Kitty graphics protocol guide.
- `docs/src/features/plugins.md`: WASM plugin development guide (with Rust sample code).
- `docs/src/advanced/shaders.md`: custom WGSL shader reference and examples.
- `docs/src/advanced/performance.md`: performance tuning guide.

### Performance

**GPU buffer reuse for rendering optimization**
- Added reusable vertex/index buffers to `WgpuState`.
- Replaced per-frame `create_buffer_init` (GPU allocation) with `queue.write_buffer` overwrites.
- Buffers are only reallocated (2× size) when capacity is exceeded; no reallocation in normal operation.
- GPU allocation count for an 80×24 terminal drops from **~4 per frame → 0 per frame**.

**FPS cap**
- `gpu.fps_limit` (default 60 FPS) controls the frame rate.
- Set to 0 for uncapped (vsync only).

**ASCII glyph pre-warming**
- ASCII printable characters (0x20–0x7E) are pre-loaded into the glyph atlas at startup in both Regular and Bold.
- Eliminates first-keystroke rasterization latency.

**Launcher startup time optimization**
- Changed `wait_for_server` polling to exponential backoff (10 ms, 10 ms, 10 ms, 20 ms, 50 ms, 100 ms).
- Average server-ready detection time reduced from **100 ms → ~30 ms** when the server starts quickly.

---

## [0.7.1] - 2026-04-05

### Fixed

**Fix ad-hoc codesign failure on macOS Intel builds**
- Signing individual binaries before signing the whole app bundle caused a subcomponent error.
- Changed to a single `codesign --force --deep --sign - dist/Nexterm.app` for the full bundle.

---

## [0.7.0] - 2026-04-05

### Added

**Floating panes**
- Added `OpenFloatingPane` / `CloseFloatingPane` / `MoveFloatingPane` / `ResizeFloatingPane` IPC commands.
- Added `FloatRect` cache and `floating_pane_rects` field to the GPU client.

**WASM plugin system**
- New `nexterm-plugin` crate (wasmi 0.38-based sandboxed WASM runtime).
- Built-in plugin API: `nexterm_on_output`, `nexterm_on_command`; host imports: `nexterm.log`, `nexterm.write_pane`.
- Added `plugin_dir` / `plugins_disabled` fields to config.

**Status bar widget enhancements**
- Built-in widgets: `"time"`, `"date"`, `"hostname"`, `"session"`, `"pane_id"`.
- Added `right_widgets` (right-aligned) and `separator` fields to `StatusBarConfig`.
- `WidgetContext` now passes session name and pane ID to widgets.

**Linux packaging**
- `linux/AppRun`: AppImage entry-point script.
- `pkg/flatpak/`: Flatpak manifest + AppStream metadata.
- Added AppImage build and upload step to GitHub Actions.
- `.github/workflows/flatpak.yml`: dedicated Flatpak build workflow.

**Test coverage improvements**
- Total test count: 145 → 178 (+33 tests).
- New tests in: nexterm-proto, nexterm-client-tui, nexterm-vt, nexterm-config, nexterm-plugin.

---

## [0.6.0] - 2026-04-05

### Added

**Four new built-in color schemes (Catppuccin / Dracula / Nord / One Dark)**

- Added `Catppuccin`, `Dracula`, `Nord`, and `OneDark` to `BuiltinScheme` in `nexterm-config`.
- Defined full fg/bg/ANSI[16] color palettes for all 9 schemes; reflected in the GPU renderer's terminal drawing.
- Settings panel (`[Colors]` tab) expanded to show all 9 scheme dots.

**Shell completion script generation**

- Added `nexterm-ctl completions <shell>` command.
  Outputs completion scripts for bash / zsh / fish / powershell / elvish to stdout.

**Man page generation**

- Added `nexterm-ctl man` command.
  Outputs a troff-format man page to stdout (`nexterm-ctl man > nexterm-ctl.1` to save).

**Bracketed paste mode (DEC ?2004)**

- VT parser now interprets `CSI ?2004h` / `CSI ?2004l` to track bracketed paste mode.
- When the mode is active, pasted text is wrapped with `ESC[200~` … `ESC[201~` before sending to the PTY.
  Prevents accidental command execution in zsh, fish, vim, and other shells/editors.

**Auto-load `~/.ssh/config`**

- Host Manager (`Ctrl+Shift+H`) now parses `~/.ssh/config` at startup and merges entries with `[[hosts]]`.
- `Host *` wildcards are excluded. Duplicate entries (same host + port already in `nexterm.toml`) are suppressed.

**Vim-compatible copy mode keys**

- `w` / `b`: word-wise forward / backward movement.
- `$`: jump to end of line.
- `Y`: yank the entire current line and exit copy mode.
- `/`: incremental search mode (Enter to confirm, n for next match, Esc to cancel).

**OSC 8 hyperlink support**

- Added `Grid.hyperlinks: Vec<HyperlinkSpan>` to `nexterm-proto`.
- VT parser interprets `ESC ] 8 ; ; <url> BEL` … `ESC ] 8 ; ; BEL` and records spans in the grid.
- GPU client's URL click (`Ctrl+Click`) now detects OSC 8 links first.

**Tab/pane activity notification**

- When output arrives in an unfocused pane, its tab shows an orange background and a `●` indicator.

**Mouse reporting (SGR ?1006 / X11 ?1000)**

- VT parser interprets `CSI ?1000h` / `CSI ?1006h` to track mouse modes.
- GPU client mouse clicks and drags are sent to the PTY as SGR escape sequences.
- Added `ClientToServer::MouseReport` message to `nexterm-proto`.

**Scrollback search UI completed**

- Added `Scrollback::search_prev()`. `Shift+Enter` or `Shift+N` moves to the previous match.
- Improved search bar UI: cursor `|`, accent line, key hint display.

**OSC 133 semantic zones**

- VT parser interprets `ESC ] 133 ; A/B/C/D BEL` to track prompt / command / output boundaries.
- Exit code of a completed command (D mark) is shown in the status bar (non-zero only).
- Added `ServerToClient::SemanticMark` message to `nexterm-proto`.

**Profiles (named configuration sets)**

- Added `Profile` struct and `Config.profiles` / `Config.active_profile` to `nexterm-config`.
- `Profile` can override font, colors, shell, scrollback, and tab bar from the base config.
- `Config::effective()` returns the config with the active profile applied.
- `Config::activate_profile(name)` / `clear_active_profile()` control profile switching.

### Changed

- `nexterm-client-gpu`: Settings panel scheme selector now supports all 9 schemes.

### Tests

- `nexterm-vt`: added bracketed paste mode enable/disable tests; OSC 8 hyperlink and OSC 133 semantic zone tests (18 tests total).
- `nexterm-server`: added BSP 4-split layout, session management API, and SSH config parser tests.
- `nexterm-config`: added profile application and TOML parse tests (17 tests total).

---

## [0.5.5] - 2026-04-05

### Fixed

**Windows — GPU client font rendering fixed**

- Replaced the `cell_w = font_size * 0.6` fixed-ratio heuristic with actual advance width measurement
  by rasterizing the reference character `'0'` at runtime via `layout_runs()`.
  Eliminates extra spaces between characters ("Wi ndows Power She l l").
- Added `scale_factor: f32` to `FontManager::new()`; passes `window.scale_factor()` from winit
  so the physical font size is correctly computed for high-DPI displays (125 %, 150 % scaling).
- Fixed a negative-coordinate wrap bug (`x as u32`) in the `rasterize_char` closure;
  added `if x < 0 || y < 0 { return; }` guard.
- `WindowEvent::ScaleFactorChanged` is now handled: font and glyph atlas are automatically regenerated on DPI change.

**Windows 11 — Acrylic frosted-glass background**

- Calls `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMWCP_ACRYLIC)` to apply
  a frosted-glass effect to the window background, similar to Windows Terminal.
- wgpu Surface composite alpha mode set to `PreMultiplied` for correct transparent blending.
- No effect on Windows 10 or non-Windows platforms; code is `#[cfg(windows)]`-guarded.

---

## [0.5.4] - 2026-04-05

### Fixed

**Windows — console window no longer appears on launch**

Added `#[windows_subsystem = "windows"]` (release builds only) to `nexterm.exe`,
`nexterm-server`, and `nexterm-client-gpu`. Launching `nexterm.exe` from the MSI installer
or Explorer no longer opens a stray black console window.

- Logs are written to `%LOCALAPPDATA%\nexterm\nexterm-server.log` / `nexterm-client.log`
  with daily rotation (`tracing-appender`).
- Errors are reported via `MessageBoxW` dialogs.

**macOS — binaries are ad-hoc signed + Intel Mac support**

- All macOS release binaries are now signed with `codesign --sign -` (ad-hoc).
  `xattr -dr com.apple.quarantine <file>` is all that's needed to bypass Gatekeeper.
- Built `x86_64-apple-darwin` target on the `macos-13` (Intel) runner;
  `nexterm-vX.Y.Z-macos-x86_64.tar.gz` is now included in release assets.

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
