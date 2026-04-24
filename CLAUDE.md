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
nexterm-launcher (nexterm.exe / nexterm)
  → nexterm-server  (デーモン: PTYセッション保持)
  → nexterm-client-gpu  (GUIクライアント: wgpu)
  → nexterm-client-tui  (フォールバック: ratatui)
```

IPC通信はUnixソケット (`$XDG_RUNTIME_DIR/nexterm.sock`) またはWindowsの名前付きパイプ (`\\.\pipe\nexterm-<USERNAME>`) を使用。メッセージは4バイトLEプレフィックス付きbincodeシリアライズ。

### クレート依存関係

- `nexterm-proto` — 全IPC型定義。他の全クレートが依存する中心クレート。変更は全クレートに影響する
- `nexterm-vt` — `vte`クレートのラッパー。VT100/ANSIパーサ + 仮想スクリーン (`Grid`) + Sixel/Kitty画像デコード
- `nexterm-server` — PTYサーバー。`SessionManager → Session → Window (BSP) → Pane` の階層構造
- `nexterm-config` — TOML+Luaコンフィグ。ロード順: デフォルト値 → config.toml → config.lua。`notify`クレートによるホットリロード
- `nexterm-client-gpu` — wgpuレンダラー (winit 0.30 ApplicationHandler)。3パスレンダリング: 背景矩形→テキスト→画像
- `nexterm-client-tui` — ratatui+crossterm によるTUIフォールバック
- `nexterm-ssh` — russh 0.58 ベースのSSHクライアント
- `nexterm-plugin` — wasmiベースのWASMプラグインランタイム
- `nexterm-i18n` — 8言語対応 (en/ja/zh-CN/ko/de/fr/es/it)。ユーザー向け文字列は`fl!`マクロ必須
- `nexterm-ctl` — CLIツール (list/new/attach/kill/record)
- `nexterm-launcher` — サーバー自動起動エントリーポイント

### サーバー内部構造 (`nexterm-server/src/`)

- `session.rs` — `SessionManager`, `Session`, BSPレイアウトエンジン
- `window.rs` — `Window` (BSPツリー + Pane管理)
- `pane.rs` — `Pane` (PTY + PTYリーダースレッド + 録画ログライター)
- `ipc/` — IPCモジュール（5ファイル分割）:
  - `platform.rs` — Unix/Windows リスナー・UID検証 (SO_PEERCRED/getpeereid)
  - `handler.rs` — クライアント読み書きループ
  - `dispatch.rs` — 40+ IPCコマンドのディスパッチロジック
  - `key.rs` — キーコード → VTエスケープシーケンス変換（ユニットテスト8件付き）
  - `sftp.rs` — SFTPアップロード・ダウンロードヘルパー
- `persist.rs` / `snapshot.rs` — セッション永続化 (JSON、`~/.local/state/nexterm/snapshot.json`)
- `hooks.rs` — Luaフックイベント処理
- `serial.rs` — シリアルポート接続
- `template.rs` — セッションテンプレート
- `web/` — axum WebSocket + xterm.js埋め込みWebターミナル

### GPUクライアント内部構造 (`nexterm-client-gpu/src/`)

- `renderer.rs` — wgpu初期化 + 3パスレンダリングパイプライン + cosmic-textグリフアトラス + winit イベントループ
- `state.rs` — `ClientState` (panes/pane_layouts/copy_mode/search/context_menu等の状態管理)
- `font.rs` — `FontManager` (cosmic-textラッパー、CJK幅計算)
- `glyph_atlas.rs` — GPUグリフアトラス管理
- `shaders.rs` — WGSLシェーダー定数
- `vertex_util.rs` — 頂点バッファユーティリティ
- `color_util.rs` — カラーパレット変換
- `key_map.rs` — キー入力マッピング
- `connection.rs` — サーバーへのIPC接続管理
- `settings_panel.rs` — `Ctrl+,`で開く設定パネルUI (7カテゴリ、toml_editで書き戻し、`LANGUAGE_OPTIONS`で言語選択)
- `palette.rs` — コマンドパレット (fuzzy-matcher)
- `scrollback.rs` — スクロールバック管理 + インクリメンタル検索
- `host_manager.rs` — SSHホストマネージャーUI
- `macro_picker.rs` — Luaマクロピッカーui

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

## コーディング規約

- `unwrap()`禁止。`?` または `expect("理由")`を使用
- エラーは`anyhow::Result`で伝播
- async: `tokio::spawn` / blocking処理は`tokio::task::spawn_blocking`
- IPC用Mutex: `tokio::sync::Mutex`、PTYリーダースレッド用: `std::sync::Mutex`
- ユーザー向け文字列: `nexterm_i18n::fl!`マクロ必須、`nexterm-i18n/locales/`の全8言語に追加
- プロトコルメッセージ追加時は`nexterm-proto/src/message.rs`と`nexterm-proto/src/grid.rs`の両方を確認

## リリースフロー

リリースは`.github/workflows/release.yml`で自動化。バージョンタグ (`v*.*.*`) のプッシュでトリガーされる。WiX v3でWindowsインストーラー (`.msi`) をビルド。`wix/main.wxs`でコンポーネントを管理 (`nexterm-client-gpu.exe`は含まない)。

CIは`.github/workflows/ci.yml`で`main`/`develop`ブランチをトリガーとして設定されているが、デフォルトブランチは`master`のため、CI自動実行には`ci.yml`のブランチ設定を`master`に修正が必要。

バージョンバンプは`Cargo.toml`の`[workspace.package] version`を更新すること（個別クレートのCargo.tomlではなく、ワークスペースルートのみ）。

Flatpakビルドは`.github/workflows/flatpak.yml`で`ubuntu-latest`ランナー上で実行する。`container:`ブロックを使うと`apt-get`が利用できなくなるため使用しないこと。`flatpak remote-add`・`flatpak install`・`flatpak-builder`にはすべて`--user`フラグが必要（CI環境ではシステム操作の権限がない）。

russh の SSH エージェント認証では `request_identities()` が返す `Vec<PublicKey>` のループ変数は `&PublicKey` 型。`authenticate_publickey_with` に渡す際は `identity.clone()` で所有権を取得すること（`public_key()` メソッドは `PublicKey` 型には存在しない）。
