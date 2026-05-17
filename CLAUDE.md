# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## ビルドコマンド

```bash
# Linux 開発依存ライブラリ（Ubuntu/Debianの場合）
sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev

# 全クレートをビルド
cargo build

# リリースビルド
cargo build --release

# 特定クレートのみ
cargo build -p nexterm-server
cargo build -p nexterm-client-gpu
cargo build -p nexterm-ctl

# テスト実行
cargo test
cargo test -p nexterm-vt                      # 特定クレート
cargo test bsp_split                           # テスト名でフィルタ
cargo test --test ipc_integration              # nexterm-server 統合テスト
cargo test --test snapshot_roundtrip           # スナップショット往復テスト

# Lint
cargo clippy -- -D warnings        # PRマージ必須条件
cargo fmt --check                  # PRマージ必須条件
cargo fmt                          # フォーマット適用
cargo audit                        # 脆弱性チェック（cargo install cargo-audit が必要）

# デバッグ実行
NEXTERM_LOG=debug nexterm-server
NEXTERM_LOG=trace nexterm-client-gpu   # IPC全メッセージ表示
```

## アーキテクチャ

### プロセス構成

```
nexterm (= nexterm-client-gpu の bin name "nexterm" — シングルバイナリ)
  ├─ nexterm_server::run_server()  内部 tokio タスク (PTY セッション保持)
  └─ wgpu レンダラー + winit       (GUIクライアント)
```

別配布バイナリ（補助）:
- `nexterm-client-tui` — TUI フォールバック（ratatui + crossterm）
- `nexterm-server` — サーバーを単独プロセスで起動したいときに使用（systemd 等）
- `nexterm-ctl` — CLI 操作ツール（list/new/attach/kill/record）

IPC通信はUnixソケット (`$XDG_RUNTIME_DIR/nexterm.sock`) またはWindowsの名前付きパイプ (`\\.\pipe\nexterm-<USERNAME>`) を使用。メッセージは4バイトLEプレフィックス付きbincodeシリアライズ（Sprint 5-1 で postcard に移行済み、`nexterm-proto/src/codec.rs` 参照）。`nexterm` 単一バイナリ実行時もこの IPC を通じて GUI と内部サーバータスクが通信し、`nexterm-ctl` 等も同じソケット経由で接続する。

v1.4.0 で旧 `nexterm-launcher` クレートを削除。v0.9.3 でシングルバイナリ化（client-gpu の bin "nexterm" が内部でサーバータスクを起動）を実装した時点で launcher は役目を終えていたが、削除し忘れていたため bin name 衝突を起こしていた。詳細は v1.4.0 リリースノート参照。

### クレート依存関係

- `nexterm-proto` — 全IPC型定義。他の全クレートが依存する中心クレート。変更は全クレートに影響する
- `nexterm-vt` — `vte`クレートのラッパー。VT100/ANSIパーサ + 仮想スクリーン (`Grid`) + Sixel/Kitty画像デコード
- `nexterm-server` — PTYサーバー。`SessionManager → Session → Window (BSP) → Pane` の階層構造
- `nexterm-config` — TOML+Luaコンフィグ。ロード順: デフォルト値 → config.toml → config.lua。`notify`クレートによるホットリロード
- `nexterm-client-gpu` — wgpuレンダラー (winit 0.30 ApplicationHandler)。3パスレンダリング: 背景矩形→テキスト→画像。アップデートチェック（`update_checker.rs`）の詳細は後述の「GPUクライアント内部構造」を参照
- `nexterm-client-tui` — ratatui+crossterm によるTUIフォールバック
- `nexterm-ssh` — russh 0.60 ベースのSSHクライアント（GHSA-f5v4-2wr6-hqmg pre-auth DoS 対策で 0.60 に更新、`ring` backend を使用して NASM 依存を回避）
- `nexterm-plugin` — wasmiベースのWASMプラグインランタイム。`PLUGIN_API_VERSION = 1` が安定 ABI を識別する。`PluginManager::unload(path)` / `reload(path)` でランタイムアンロード/再ロードに対応。プラグインは `nexterm_meta` エクスポートで名前・バージョンを公開できる。`SessionManager.plugin_manager` に `Arc<Mutex<Option<PluginManager>>>` として保持され、IPC (`ListPlugins`/`LoadPlugin`/`UnloadPlugin`/`ReloadPlugin`) で操作可能
- `nexterm-i18n` — 8言語対応 (en/ja/zh-CN/ko/de/fr/es/it)。ユーザー向け文字列は`fl!`マクロ必須
- `nexterm-ctl` — CLIツール (list/new/attach/kill/record)

### サーバー内部構造 (`nexterm-server/src/`)

- `session.rs` — `SessionManager`, `Session`, BSPレイアウトエンジン
- `window/` — `Window` の実装（モジュール化）:
  - `mod.rs` — `Window` 本体（BSPツリー + Pane管理）
  - `bsp.rs` — BSP分割アルゴリズム (`PaneRect` / `SplitDir` を公開)
  - `tiling.rs` — タイリングレイアウトロジック
  - `floating.rs` — フローティングウィンドウ (`FloatRect` を公開)
  - `tests.rs` — `bsp_split` 等のレイアウトユニットテスト
- `pane.rs` — `Pane` (PTY + PTYリーダースレッド + 録画ログライター)
- `ipc/` — IPCモジュール:
  - `platform.rs` — Unix/Windows リスナー・UID検証 (SO_PEERCRED/getpeereid)
  - `handler.rs` — クライアント読み書きループ
  - `dispatch.rs` — 40+ IPCコマンドのディスパッチロジック
  - `key.rs` — キーコード → VTエスケープシーケンス変換（ユニットテスト8件付き）
  - `sftp.rs` — SFTPアップロード・ダウンロードヘルパー
  - `plugin_dispatch.rs` — プラグイン IPC コマンド (`ListPlugins`/`LoadPlugin`/`UnloadPlugin`/`ReloadPlugin`) のハンドラ
- `persist.rs` / `snapshot.rs` — セッション永続化 (JSON、`~/.local/state/nexterm/snapshot.json`)。スキーマ v2（`SNAPSHOT_VERSION=2`、最低サポート v1）。旧 v1 スナップショットは `load_snapshot()` が自動マイグレーション
- `hooks.rs` — Luaフックイベント処理
- `serial.rs` — シリアルポート接続
- `template.rs` — セッションテンプレート
- `web/` — Web ターミナル機能（axum WebSocket + xterm.js）:
  - `mod.rs` — エンドポイント・ルーティング
  - `auth.rs` — トークン認証
  - `oauth.rs` — OAuth 認証フロー
  - `otp.rs` — TOTP（時間ベースワンタイムパスワード）
  - `tls.rs` — TLS 設定・証明書ロード
  - `access_log.rs` — アクセスログ
- `test_utils.rs` — テスト用ヘルパー（ライブラリ内テスト共有）

### 統合テスト (`nexterm-server/tests/`)

- `ipc_integration.rs` — IPC コマンド全体の往復テスト
- `snapshot_roundtrip.rs` — スナップショット保存→ロードの往復テスト

### GPUクライアント内部構造 (`nexterm-client-gpu/src/`)

- `renderer.rs` — wgpu初期化 + 3パスレンダリングパイプライン + cosmic-textグリフアトラス + winit イベントループ
- `state.rs` — `ClientState` (panes/pane_layouts/copy_mode/search/context_menu等の状態管理)
- `font.rs` — `FontManager` (cosmic-textラッパー、CJK幅計算)
- `glyph_atlas.rs` — GPUグリフアトラス管理。`LruCache` でグリフをキャッシュ（フォント変更後の古いエントリを自動削除）。`new_with_config(device, atlas_size)` で設定値を最大サイズとして初期化する
- `shaders.rs` — WGSLシェーダー定数
- `vertex_util.rs` — 頂点バッファユーティリティ
- `color_util.rs` — カラーパレット変換
- `key_map.rs` — キー入力マッピング
- `connection.rs` — サーバーへのIPC接続管理
- `settings_panel.rs` — `Ctrl+,`で開く設定パネルUI (7カテゴリ、toml_editで書き戻し、`LANGUAGE_OPTIONS`で言語選択)
- `palette.rs` — コマンドパレット（Ctrl+Shift+P）。`SkimMatcherV2` で fuzzy 検索。Sprint 5-7 / Phase 3-3 で `execute_action` 全アクション（Quit / ClosePane / NewWindow / QuickSelect / SetBroadcastOn/Off 等を含む 25 件）を網羅 + 使用履歴を `~/.local/state/nexterm/palette_history.json`（atomic write + 0600）に永続化。`rank_actions` 純関数: クエリ空時は履歴順（last_used 降順 → use_count 降順）、クエリ有時は fuzzy スコア + `history_bonus`（use_count×10 上限 100 + 1日以内+100/1週間以内+50）。選択時に `record_use` で記録
- `scrollback.rs` — スクロールバック管理 + インクリメンタル検索
- `host_manager.rs` — SSHホストマネージャーUI。`load_history()` / `save_history()` で接続頻度を `host_history.json` に永続化。`PasswordModal` 構造体で `auth_type="password"` ホストのパスワード入力モーダルを管理
- `macro_picker.rs` — Luaマクロピッカーui
- `update_checker.rs` — 起動 5 秒後に GitHub Releases API をポーリングして新バージョンを検出。`auto_check_update = false` で無効化可能。結果は `ClientState.update_banner` に格納され、`Esc` で閉じる / `Enter` でリリースページを開く
- `platform.rs` — プラットフォーム依存ユーティリティ。`apply_acrylic_blur` は Windows 11 で `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE=4)` により Acrylic 効果を適用（Windows 10 以下では何もしない）。`open_releases_url` はリリースページを既定ブラウザで開く
- `renderer/background_pass.rs` — 背景画像レンダリング（Sprint 5-7 / Phase 3-1）。`WindowConfig.background_image` 設定時のみ起動時に画像をロードし、毎フレームで clear → 背景画像 → セル背景 → テキストの順で描画する。fit モード（cover/contain/stretch/center/tile）ごとの NDC + UV 計算を `compute_background_quad` 純関数で実装し 11 件のユニットテスト付き。4096x4096 を超える画像は Lanczos3 で自動ダウンスケール。Tile モードでタイル数が 256 を超えるケースは Stretch にフォールバック（実害回避）。既存の `image_pipeline`（Sixel/Kitty 用）を再利用するため独自パイプラインは作らない。サポート形式: PNG / JPEG（workspace の `image` クレートで有効な features）
- `animations.rs` — UI アニメーション基盤（Sprint 5-7 / Phase 3-2）。`ease_out_cubic` / `linear` 等の easing 関数と `AnimationManager`（タブ切替・ペイン追加の時刻記録）を提供。レンダラーは `tab_switch_progress(now, duration)` / `pane_fade_in_progress(id, now, duration)` で進捗 [0,1] を取得する。`Config.animations` で `enabled=false` または `intensity="off"` の場合は `scaled_duration_ms` が 0 を返し全アニメーションが即時反映（reduced motion 対応）。intensity は `off`/`subtle`(×0.5)/`normal`(×1.0)/`energetic`(×1.5) の 4 段階。タブ切替は 200ms の ease-out（アクセントラインが中央から横に伸びる + フェードイン）、新規ペイン追加は 250ms の白いオーバーレイ alpha=0.35→0 フェード

## 重要な実装パターン

### PTYリーダースレッド (daemonless設計の核心)

各Paneは`tokio::task::spawn_blocking`でリーダースレッドを起動。クライアントの接続/切断時は`Arc<Mutex<Sender<ServerToClient>>>`をアトミックにスワップするため、セッションがクライアント切断後も生き続ける。

### BSPレイアウト (pane分割)

`SplitNode`列挙型の再帰ツリー。Pane追加は「ID事前確保 → ツリー挿入 → 全paneサイズ再計算 → PTYスポーン → 既存paneリサイズ」の順で行うこと (chicken-and-egg問題回避)。

### Luaワーカー

`mlua::Lua`インスタンスは`nexterm-lua-worker`という専用OSスレッドに閉じ込め、メインスレッドとはチャネルで通信する。`StatusBarEvaluator`は毎秒評価を要求し、キャッシュ済み値を即時返してバックグラウンド更新する。

### 設定パネルのTOML書き戻し

`toml_edit`クレートを使い既存コメントや構造を保持したまま値を更新する。`toml`クレートで全書き換えしないこと。

### 言語選択

`settings_panel.rs` の `LANGUAGE_OPTIONS: &[(&str, &str)]`（表示名, 言語コード）で管理。設定パネルで変更すると `config.toml` の `language` キーに書き戻され、次回起動時に `nexterm-i18n` が適用する。新しい表示文字列を追加する際は `nexterm-i18n/locales/` 配下の**全8言語JSONファイル**に追加すること。

### コンテキストメニュー幅

`renderer.rs` の `build_context_menu_verts` でメニュー幅をテキスト長に応じて動的計算する。固定幅にしないこと（翻訳テキストが長い言語でオーバーフローする）。

### カーソルスタイル・ウィンドウパディング・PresentMode

- `nexterm-config` の `CursorStyle`（block/beam/underline）を `config.cursor_style` で指定。`vertex_util::draw_cursor()` で形状を描き分ける
- `WindowConfig.padding_x` / `padding_y`（ピクセル）: グリッド描画の基点オフセットとして使用。`grid_offset_y = tab_bar_h + padding_y` で計算
- `GpuConfig.present_mode`（fifo/mailbox/auto）: `WgpuState::new` 内で `wgpu::PresentMode` に変換して `SurfaceConfiguration` に設定する

## コーディング規約

- `unwrap()`禁止。`?` または `expect("理由")`を使用
- エラーは`anyhow::Result`で伝播
- async: `tokio::spawn` / blocking処理は`tokio::task::spawn_blocking`
- IPC用Mutex: `tokio::sync::Mutex`、PTYリーダースレッド用: `std::sync::Mutex`
- ユーザー向け文字列: `nexterm_i18n::fl!`マクロ必須、`nexterm-i18n/locales/`の全8言語に追加
- プロトコルメッセージ追加時は`nexterm-proto/src/message.rs`と`nexterm-proto/src/grid.rs`の両方を確認

## UI/UX 改善時のガイドライン（重要）

本プロジェクトは Rust + wgpu + cosmic-text による独自 GPU レンダリングであり、Web フロントエンド（HTML / CSS / React / Vue / DOM）は一切存在しない。

- **`frontend-design` グローバル SKILL は本プロジェクトでは適用しない**。当該スキルは Web UI（HTML/CSS/JS、React、CSS 変数、CSS アニメーション、ブラウザ向けフォントペア等）を前提に設計されているため、Nexterm の wgpu レンダラーには出力形式が合わない。
- UI 提案では以下の既存パターンに従うこと:
  - **レンダリング**: `renderer/overlay/`（タブバー・ステータスバー・ダイアログ）と `vertex_util.rs` の頂点バッファビルダーで描画する。CSS / DOM を生成しない
  - **フォント**: `font.rs` の `FontManager`（cosmic-text ラッパー）経由で扱う。Google Fonts / Web フォントの参照は不可
  - **配色**: `color_util.rs` のパレット変換ヘルパーと `ColorScheme`（設定パネルでテーマ切替）を使用する
  - **アニメーション**: フレーム駆動。`prefers-reduced-motion` 等の CSS Media Query は存在しない。代わりに `config.toml` の設定で動きの強度を切り替える
  - **文字列**: ユーザー向け文字列は必ず `nexterm_i18n::fl!` で全 8 言語に追加する
  - **アクセシビリティ観点**: コントラスト比 4.5:1 以上、キーボードのみで全操作可能、IME 競合に配慮（既存の `ime_preedit` 経路を再利用）
- UI/UX 改善対象の主な領域: `settings_panel.rs` / `host_manager.rs` / `palette.rs` / `macro_picker.rs` / `renderer/overlay/` / `state/menus.rs`

## リリースフロー

リリースは`.github/workflows/release.yml`で自動化。バージョンタグ (`v*.*.*`) のプッシュでトリガーされる。WiX v3でWindowsインストーラー (`.msi`) をビルド。`wix/main.wxs`でコンポーネントを管理 (`nexterm-client-gpu.exe`は含まない)。

CIは`.github/workflows/ci.yml`で`master`ブランチへの push / PR をトリガーとして設定済み。Linux / macOS / Windows の3 OS マトリクスで `cargo test` / `cargo clippy -- -D warnings` / `cargo fmt --check` を実行する。

バージョンバンプは`Cargo.toml`の`[workspace.package] version`を更新すること（個別クレートのCargo.tomlではなく、ワークスペースルートのみ）。ワークスペースは Rust 2024 edition (`edition = "2024"`) を使用しているため、ビルドには Rust 1.85 以降が必要。

Flatpakビルドは`.github/workflows/flatpak.yml`で`ubuntu-latest`ランナー上で実行する。`container:`ブロックを使うと`apt-get`が利用できなくなるため使用しないこと。`flatpak remote-add`・`flatpak install`・`flatpak-builder`にはすべて`--user`フラグが必要（CI環境ではシステム操作の権限がない）。

flatpak-builder のサンドボックスはネットワーク隔離されているため、cargo の依存は事前に vendor して `pkg/flatpak/cargo-sources.json` に格納し、manifest の `sources` から参照する。**Cargo.lock を変更したら必ず `bash scripts/regenerate-flatpak-sources.sh` を実行して `cargo-sources.json` を再生成しコミットすること**。CI (`flatpak.yml`) は最初のステップで `flatpak-cargo-generator.py` を走らせて `cargo-sources.json` との diff を取り、不一致なら失敗するため再生成漏れを早期検知できる。ビルドは `CARGO_NET_OFFLINE=true` + `cargo --offline build` でオフライン強制。

russh 0.59 / 0.60 の SSH エージェント認証では `request_identities()` が返す `Vec<AgentIdentity>` のループ変数は `&AgentIdentity` 型。`authenticate_publickey_with` の第2引数は `ssh_key::PublicKey` であるため `identity.public_key().into_owned()` で取得すること（russh 0.58 では `identity.clone()` で `PublicKey` を取得していたが、0.59 で型が変わった）。0.59 → 0.60 では本コードベースで使用する API に破壊的変更なし。`Cargo.toml` で `default-features = false, features = ["ring", "rsa", "flate2"]` を指定して `aws-lc-rs` backend を回避することで、Windows 等の NASM 未導入環境でもビルド可能。

WiX v3 の `candle.exe` にプリプロセッサ変数を渡す際は `-dName=Value` 形式（スペースなし）を使うこと。PowerShell から呼び出す場合は `-d "Name=Value"` とすると2引数に分割されて `CNDL0289` エラーになる。正しい形式: `"-dVersion=$version"`。
