# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

---

## [0.7.5] - 2026-04-06

### Fixed

**GPU クライアント: 描画総点検による品質修正**
- スクロールバック表示の `visible_rows` 計算にステータスバー高さ（`cell_h`）を追加。最下行がステータスバーと重なっていた問題を修正。
- `ScaleFactorChanged`（DPI 変更）イベントで `cols/rows` を再計算しサーバーへ Resize 通知を送るよう修正。高 DPI ディスプレイへの移動時にレイアウトがずれる問題を解消。
- 右クリックコンテキストメニューの y 座標に `tab_bar_h` を反映。タブバー有効時にメニュー位置がズレる問題を修正。
- `GlyphAtlas` 満杯時に `cleared_this_frame` フラグを追加。次フレーム開始時にフラグをリセットすることで、UV 不整合によるグリフ化けを防止。
- `font.rs` の `family_owned` をすべてのパスで事前宣言し、ライフタイム構造を明確化。

---

## [0.7.4] - 2026-04-06

### Fixed

**Windows GPU クライアント: CJK全角文字の文字間スペース問題を修正**
- `rasterize_char()` に `wide: bool` パラメータを追加し、CJK等の全角文字（Unicode幅≥2）は 2 セル分のバッファ（`display_cols = 2.0`）でレンダリングするように変更。
- `GlyphKey` に `wide` フィールドを追加し、全角・半角を別々にアトラスキャッシュ。
- 日本語・中国語・韓国語等の文字が等間隔で正しく表示されるようになった。

**Windows GPU クライアント: タブバーとターミナルコンテンツの重なり問題を修正**
- タブバー（上部）と1行目ターミナルコンテンツが同じ y=0 に描画され重なっていた問題を修正。
- `build_grid_verts` / `build_scrollback_verts` に `y_offset: f32` パラメータを追加。
- マルチペイン用 `_in_rect` 関数も `off_y = row_offset * cell_h + tab_bar_h` に修正。
- ペイン境界線・番号バッジもタブバー高さを考慮するよう更新。

**Windows GPU クライアント: 右側の黒帯（未使用領域）を修正**
- ターミナルの行数計算（`rows`）がウィンドウ高さ全体を使用していたため、タブバー・ステータスバーと重なっていた問題を修正。
- `rows = (height - tab_bar_h - status_bar_h) / cell_h` で正確な有効行数を算出。
- ウィンドウ初期化時・リサイズイベント時の両方を修正。
- マウスクリック座標→セル座標変換でも `tab_bar_h` を減算して正確な行選択を実現。

---

## [0.7.3] - 2026-04-06

### Fixed

**Windows GPU クライアント: フォント文字間隔ずれを修正**
- `Attrs::new()` のデフォルトが `Family::SansSerif` のため、Windows 上でプロポーショナルフォント（Segoe UI 等）にフォールバックしていた問題を修正。
- `measure_char_width` と `rasterize_char` で `Family::Monospace` または `Family::Name(family)` を明示的に指定。
- 設定フォント名が `"monospace"` の場合は `Family::Monospace`（fontdb がシステムの等幅フォントを選択）、具体名（`Consolas`, `JetBrains Mono` 等）は `Family::Name` で直接指定。
- セル幅計測を `Buffer::draw()` のインクピクセル計測から `layout_runs()` の advance width 取得に変更。right bearing を含む正確なセル幅を算出することで文字間隔が正確になった。
- 「Wi ndows PowerShe l l」のように文字の間に余分なスペースが入る問題を解消。

**シェーダーホットリロード・ギャラリー・移行ツール**
- `WgpuState::reload_shader_pipelines()` を追加: WGSL ファイル変更時にシェーダーをホットリロード（再起動不要）。
- `examples/shaders/`: CRT・Matrix・Glow（背景）/ Grayscale・Amber（テキスト）のサンプル WGSL シェーダーを同梱。
- `nexterm-ctl import-ghostty`: Ghostty 設定ファイルをインポートして nexterm config に変換。
- `nexterm-ctl service install/uninstall/status`: systemd（Linux）/ launchd（macOS）自動起動サービス管理。

---

## [0.7.2] - 2026-04-05

### Added

**カスタム WGSL シェーダーサポート**
- `nexterm-config`: `[gpu]` セクションを追加（`custom_bg_shader` / `custom_text_shader` / `fps_limit` / `atlas_size`）。
- GPU クライアント起動時に指定パスから WGSL ファイルを読み込む（読み込み失敗時はビルトインにフォールバック）。
- CRT スキャンライン・グロー効果などのカスタムエフェクトが実装可能。

**ドキュメントサイト充実**
- `docs/src/features/graphics.md`: Sixel / Kitty グラフィックスプロトコルのガイド。
- `docs/src/features/plugins.md`: WASM プラグイン開発ガイド（Rust サンプルコード付き）。
- `docs/src/advanced/shaders.md`: カスタム WGSL シェーダーリファレンスと実装例。
- `docs/src/advanced/performance.md`: パフォーマンスチューニングガイド。

### Performance

**GPU バッファ再利用による描画最適化**
- `WgpuState` に再利用可能な頂点・インデックスバッファを追加。
- 毎フレームの `create_buffer_init`（GPU アロケーション）を廃止し、`queue.write_buffer` による上書きに変更。
- バッファ容量不足時のみ 2 倍サイズで再確保（通常は再確保なし）。
- 80×24 ターミナルで GPU アロケーション回数を **約 4 回/フレーム → 0 回/フレーム** に削減。

**FPS 制限機能**
- `gpu.fps_limit`（デフォルト 60 FPS）でフレームレートを制御。
- 0 を設定すると制限なし（vsync のみ）。

**ASCII グリフ事前ウォームアップ**
- 起動時に ASCII 印字可能文字（0x20–0x7E）を Bold/Regular でグリフアトラスに事前ロード。
- 初回キーストロークのラスタライズ遅延を排除。

**ランチャー起動時間最適化**
- `wait_for_server` のポーリング間隔を指数バックオフ方式に変更（10ms, 10ms, 10ms, 20ms, 50ms, 100ms）。
- サーバーが高速起動した場合の平均待機時間を **100ms → 約 30ms** に短縮。

---

## [0.7.1] - 2026-04-05

### Fixed

**macOS Intel ビルドの ad-hoc codesign 失敗を修正**
- 個別バイナリ署名後にバンドル全体を署名すると subcomponent エラーが発生する問題を修正。
- `codesign --force --deep --sign - dist/Nexterm.app` で一括署名に変更。

---

## [0.7.0] - 2026-04-05

### Added

**フローティングペイン**
- `OpenFloatingPane` / `CloseFloatingPane` / `MoveFloatingPane` / `ResizeFloatingPane` IPC コマンドを追加。
- GPU クライアントに `FloatRect` キャッシュと `floating_pane_rects` フィールドを追加。

**WASM プラグインシステム**
- `nexterm-plugin` クレートを新規作成（wasmi 0.38 ベース）。
- ビルトインプラグイン API: `nexterm_on_output`, `nexterm_on_command`, ホストインポート `nexterm.log` / `nexterm.write_pane`。
- 設定に `plugin_dir` / `plugins_disabled` フィールドを追加。

**ステータスバーウィジェット強化**
- ビルトインウィジェット: `"time"`, `"date"`, `"hostname"`, `"session"`, `"pane_id"`。
- `right_widgets`（右寄せ）・`separator` フィールドを `StatusBarConfig` に追加。
- `WidgetContext` でセッション名・ペイン ID をウィジェットに渡せるように。

**Linux パッケージング**
- `linux/AppRun`: AppImage エントリーポイントスクリプト。
- `pkg/flatpak/`: Flatpak マニフェスト + AppStream メタデータ。
- GitHub Actions に AppImage ビルド・アップロードステップを追加。
- `.github/workflows/flatpak.yml`: Flatpak 専用ビルドワークフロー。

**テストカバレッジ向上**
- 全テスト数 145 → 178 件（+33件）。
- 追加対象: nexterm-proto, nexterm-client-tui, nexterm-vt, nexterm-config, nexterm-plugin。

---

## [0.6.0] - 2026-04-05

### Added

**カラースキーム 4 種追加（Catppuccin / Dracula / Nord / One Dark）**

- `nexterm-config`: `BuiltinScheme` に `Catppuccin`・`Dracula`・`Nord`・`OneDark` を追加。
- 全 9 スキームの fg/bg/ANSI[16] カラーパレットを定義し、GPU レンダラーのターミナル描画に反映。
- Settings パネル（`[Colors]` タブ）でスキームドット 9 個表示に拡張。

**シェル補完スクリプト生成**

- `nexterm-ctl completions <shell>` コマンドを追加。
  bash / zsh / fish / powershell / elvish の補完スクリプトを標準出力に出力する。

**man ページ生成**

- `nexterm-ctl man` コマンドを追加。
  troff 形式の man ページを標準出力に出力する（`nexterm-ctl man > nexterm-ctl.1` で保存可能）。

**ブラケットペーストモード（DEC ?2004）実装**

- VT パーサが `CSI ?2004h` / `CSI ?2004l` を解釈してブラケットペーストモードを追跡。
- ペースト時にモードが有効な場合、テキストを `ESC[200~` … `ESC[201~` で囲んで PTY に送信。
  zsh・fish・vim など多くのシェル/エディタでペースト内容が誤実行されなくなる。

**SSH `~/.ssh/config` 自動読み込み**

- ホストマネージャ（`Ctrl+Shift+H`）起動時に `~/.ssh/config` を解析し、
  `[[hosts]]` エントリと合わせてホスト一覧に表示する。
- `Host *` ワイルドカードは除外。`nexterm.toml` に同じホスト+ポートがある場合は重複しない。

**コピーモード vim 互換キー追加**

- `w` / `b`: 単語単位の前後移動。
- `$`: 行末へ移動。
- `Y`: 現在行全体をヤンクしてコピーモードを終了。
- `/`: インクリメンタル検索モード（Enter で確定、n で次のマッチ、Esc でキャンセル）。

**OSC 8 ハイパーリンク対応**

- `nexterm-proto`: `Grid.hyperlinks: Vec<HyperlinkSpan>` を追加。
- VT パーサが `ESC ] 8 ; ; <url> BEL` … `ESC ] 8 ; ; BEL` を解釈してグリッドにスパンを記録。
- GPU クライアントの URL クリック（`Ctrl+Click`）が OSC 8 リンクを優先検出するように。

**タブ/ペインのアクティビティ通知**

- 非フォーカスペインへの出力があると、タブバーのタブにオレンジ背景と「●」インジケーターを表示。

**マウスレポーティング実装（SGR ?1006 / X11 ?1000）**

- VT パーサが `CSI ?1000h` / `CSI ?1006h` を解釈してマウスモードを追跡。
- GPU クライアントのマウスクリック・ドラッグが PTY に SGR エスケープシーケンスで送信される。
- `nexterm-proto`: `ClientToServer::MouseReport` メッセージを追加。

**スクロールバック検索 UI 完成**

- `Scrollback::search_prev()` を追加。`Shift+Enter` または `Shift+N` で前のマッチへ移動。
- 検索バーの UI を改善：カーソル `|`、アクセントライン、キー操作ヒント表示。

**OSC 133 セマンティックゾーン対応**

- VT パーサが `ESC ] 133 ; A/B/C/D BEL` を解釈してプロンプト/コマンド/出力の境界を追跡。
- コマンド終了（D マーク）の exit code がステータスバーに表示される（非 0 時のみ）。
- `nexterm-proto`: `ServerToClient::SemanticMark` メッセージを追加。

**プロファイル機能（名前付き設定セット）**

- `nexterm-config`: `Profile` 構造体と `Config.profiles` / `Config.active_profile` を追加。
- `Profile` はフォント・カラー・シェル・スクロールバック・タブバーをベース設定から上書き可能。
- `Config::effective()` でアクティブプロファイルを適用した設定を返す。
- `Config::activate_profile(name)` / `clear_active_profile()` でプロファイル切り替えを制御。

### Changed

- `nexterm-client-gpu`: `Settings` パネルのスキーム選択が 9 種に対応。

### Tests

- `nexterm-vt`: ブラケットペーストモード有効化 / 無効化テスト追加。OSC 8 ハイパーリンク・OSC 133 セマンティックゾーンテスト追加（計 18 テスト）。
- `nexterm-server`: BSP 4 分割レイアウト・セッション管理 API・SSH config パーサのテスト追加。
- `nexterm-config`: プロファイル適用・TOML パースのテスト追加（計 17 テスト）。

---

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
