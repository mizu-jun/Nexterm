# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **正本:** [CLAUDE.md](CLAUDE.md)（英語版）。本ファイルはローカル参照用の翻訳であり正本ではない。両者が食い違う場合は英語版が勝つ。

## 言語ポリシー

Nexterm は世界中に配布されるオープンソースプロジェクトだが、主要メンテナは日本語で作業する。したがって次のように使い分ける。

**日本語（対話面 — メンテナがリアルタイムで読むもの）:**
- **Claude Code CLI の会話**: チャット返答・進捗報告・ターン末サマリ・選択肢ブロック・確認質問はすべて日本語。グローバルの「日本語で回答する」ルールを本リポジトリ向けに明示したものであり、英語で会話するモードは存在しない
- **個人ブランチのローカルコミットメッセージ**: 反復作業中は日本語で構わない

**英語（世界に出る成果物 — 外部コントリビュータが読むもの）:**
- **ソースコードのコメント**（`//`・`///`・doc コメント・`expect("...")` メッセージ・`panic!` メッセージ・`log::*!` の文字列・`anyhow!`/`bail!` のリテラル）: 英語のみ
- **リポジトリのドキュメント**（`README.md`・`docs/**`・`CHANGELOG.md`・ADR・`examples/**/README.md`・`nexterm-vt/fuzz/README.md` 等）: 英語を正本とする。日本語訳を用意する場合は英語版の隣に `*.ja.md` として置く（例: `README.md` + `README.ja.md`）。現状バイリンガル維持しているのはトップレベルの `README` のみ
- **`master` 向け PR のコミットメッセージ**および **PR の説明・タイトル**: 英語
- **Git タグと GitHub Release ノート**: 英語（日本語の補足は歓迎だが正文ではない）
- **Claude Code の指示ファイル**（`CLAUDE.md`）: 英語。本ファイル `CLAUDE.ja.md` はローカル参照用の翻訳であり**正本ではない**

**アプリのユーザー向け文字列（別の話）:**
- **実行中のアプリに表示される文字列**: `nexterm-i18n`（Fluent + JSON ロケール）が管理する。新しい文字列は `nexterm-i18n/locales/` 配下の**全 8 ロケールファイル**に追加すること。レンダラーに自然言語をハードコードしない

新しいドキュメントを追加するときは既定で英語とし、その文書に日本語での可読性が必要な場合のみ `*.ja.md` を併設する。

**判断の目安:** Claude とのターミナルセッション内で人間が読むものなら日本語、リポジトリや GitHub に載って世界に出るものなら英語。

## ドキュメントマップと役割分担

永続ドキュメントはそれぞれ 1 つの役割を持つ。新しい内容はその役割を持つファイルに書き、他のファイルに重複させないこと。

| ドキュメント | 役割 | 更新タイミング |
|---|---|---|
| `CLAUDE.md` | Claude Code への作業ルール（どう作業するか） | ルール変更時。陳腐化した項目は削除 |
| `docs/PRODUCT.md` | 製品要求・ビジョン・非目標（何をなぜ作るか） | マイナー/メジャーリリース節目でレビュー |
| `docs/ARCHITECTURE.md` | システム設計（どう作られているか） | 構造変更時 |
| `docs/adr/` | 個別の設計判断の記録（なぜそう決めたか） | 追記のみ。採択済み ADR は書き換えない |
| `docs/plans/` | フェーズ付き作業計画・進捗（作業単位のステアリングファイル） | 作業中は随時。完了後は `plans/archive/` へ |
| `CHANGELOG.md` | リリース履歴 | リリース時 |

## ビルドコマンド

```bash
# Linux 開発依存ライブラリ（Ubuntu/Debianの場合）
sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev

# PRマージ必須条件
cargo clippy -- -D warnings
cargo fmt --check

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

IPC通信はUnixソケット (`$XDG_RUNTIME_DIR/nexterm.sock`) またはWindowsの名前付きパイプ (`\\.\pipe\nexterm-<USERNAME>`) を使用。メッセージは4バイトLEプレフィックス付き postcard シリアライズ（Sprint 5-1 / ADR-0006 で bincode 1.x から移行済み、`nexterm-proto/src/codec.rs` 参照）。`nexterm` 単一バイナリ実行時もこの IPC を通じて GUI と内部サーバータスクが通信し、`nexterm-ctl` 等も同じソケット経由で接続する。

v1.4.0 で旧 `nexterm-launcher` クレートを削除。v0.9.3 でシングルバイナリ化（client-gpu の bin "nexterm" が内部でサーバータスクを起動）を実装した時点で launcher は役目を終えていたが、削除し忘れていたため bin name 衝突を起こしていた。詳細は v1.4.0 リリースノート参照。

### クレート依存関係

- `nexterm-proto` — 全IPC型定義。他の全クレートが依存する中心クレート。変更は全クレートに影響する
- `nexterm-client-core` — クライアント側 IPC 実装の共通化（Sprint 3-6）。`nexterm-client-gpu` / `nexterm-client-tui` の `connection.rs` に重複していた UDS / Windows 名前付きパイプのフレーミング・ハンドシェイク・送受信タスク管理を集約し、`Connection` を公開する。GPU / TUI 両クライアントが依存する
- `nexterm-vt` — `vte`クレートのラッパー。VT100/ANSIパーサ + 仮想スクリーン (`Grid`) + Sixel/Kitty画像デコード
- `nexterm-server` — PTYサーバー。`SessionManager → Session → Window (BSP) → Pane` の階層構造
- `nexterm-config` — TOML+Luaコンフィグ。ロード順: デフォルト値 → config.toml → config.lua。`notify`クレートによるホットリロード
- `nexterm-client-gpu` — wgpuレンダラー (winit 0.30 ApplicationHandler)。3パスレンダリング: 背景矩形→テキスト→画像
- `nexterm-client-tui` — ratatui+crossterm によるTUIフォールバック
- `nexterm-ssh` — russh 0.60 ベースのSSHクライアント（GHSA-f5v4-2wr6-hqmg pre-auth DoS 対策で 0.60 に更新、`ring` backend を使用して NASM 依存を回避）
- `nexterm-plugin` — wasmiベースのWASMプラグインランタイム。`PLUGIN_API_VERSION = 1` が安定 ABI を識別する。`PluginManager::unload(path)` / `reload(path)` でランタイムアンロード/再ロードに対応。プラグインは `nexterm_meta` エクスポートで名前・バージョンを公開できる。`SessionManager.plugin_manager` に `Arc<Mutex<Option<PluginManager>>>` として保持され、IPC (`ListPlugins`/`LoadPlugin`/`UnloadPlugin`/`ReloadPlugin`) で操作可能
- `nexterm-i18n` — 8言語対応 (en/ja/zh-CN/ko/de/fr/es/it)。ユーザー向け文字列は`fl!`マクロ必須

### クレート別ガイド

クレート内部の詳細は、作業中のクレートディレクトリから遅延ロードされる:

- `nexterm-server/CLAUDE.md` — サーバー内部構造（session / window / ipc / persist / web）
- `nexterm-client-gpu/CLAUDE.md` — GPU クライアント内部構造（レンダラー・ウィジェット層・パレット・アニメーション）

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

リリース・CI・パッケージングの詳細（バージョンタグ運用、WiX v3 MSI、Flatpak ビルド、russh の feature フラグ）は `release-flow` skill に移動した。リリース作業時やこれらのパイプラインの調査時に呼び出す。

リリース時以外に発火する規則が 1 つだけここに残る: **`Cargo.lock` が変わったら `bash scripts/regenerate-flatpak-sources.sh` を実行して `pkg/flatpak/cargo-sources.json` を再生成しコミットする。** flatpak CI はこのファイルと差分照合し、不一致でジョブを失敗させる。