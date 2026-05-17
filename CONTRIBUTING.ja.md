# nexterm へのコントリビューション（日本語）

> **English:** [CONTRIBUTING.md](CONTRIBUTING.md)

## 前提条件

| ツール | バージョン | 用途 |
|--------|-----------|------|
| Rust | 1.80 以上 | コンパイル |
| cargo | （Rust 同梱） | ビルド・テスト |

### OS 別の追加要件

**Windows**
- Visual Studio Build Tools（C++ コンポーネント）

**Linux**
```bash
sudo apt install libx11-dev libxkbcommon-dev libwayland-dev
```

**macOS**
- Xcode Command Line Tools（`xcode-select --install`）

---

## ビルド

```bash
# 全クレートをビルドする
cargo build

# リリースビルド
cargo build --release

# 特定クレートのみ
cargo build -p nexterm-server
cargo build -p nexterm-client-gpu
cargo build -p nexterm-ctl
```

---

## テスト

```bash
# 全テストを実行する
cargo test

# 特定クレートのみ
cargo test -p nexterm-vt
cargo test -p nexterm-server
cargo test -p nexterm-ctl

# テスト名でフィルタする
cargo test bsp_垂直分割
```

---

## Lint / フォーマット

```bash
# clippy（警告を全部エラーにして実行）
cargo clippy -- -D warnings

# フォーマット確認
cargo fmt --check

# フォーマット適用
cargo fmt
```

PR は `cargo clippy` と `cargo fmt --check` が通ることが必須条件。

---

## クレート構成

```
nexterm/
├── nexterm-proto        # IPC メッセージ型・シリアライズ（共有クレート）
├── nexterm-vt           # VT100 パーサ・仮想スクリーン・画像デコード
├── nexterm-server       # PTY サーバー（IPC + セッション管理）
├── nexterm-config       # 設定ロード（TOML + Lua）+ StatusBarEvaluator
├── nexterm-client-tui   # TUI クライアント（ratatui + crossterm）
├── nexterm-client-gpu   # GPU クライアント（wgpu + winit）
└── nexterm-ctl          # セッション制御 CLI（list / new / attach / kill）
```

新機能を追加する際は、どのクレートが担当すべきかを `docs/ARCHITECTURE.md` の依存グラフを参照して判断する。
`nexterm-proto` への変更はすべてのクレートに影響するため慎重に行うこと。

---

## コーディング規約

### 全般

- 関数・型・フィールドに日本語コメントを付ける
- 変数名・関数名は英語スネークケース / キャメルケース
- `unwrap()` は禁止（`?` 演算子または `expect("理由")` を使う）
- エラーは `anyhow::Result` で伝播する

### 非同期コード

- `tokio::spawn` でタスクを生成する
- ブロッキング処理は `tokio::task::spawn_blocking` を使う
- `Arc<Mutex<T>>` は tokio の `Mutex` を使う（IPC 層）、同期処理は `std::sync::Mutex` を使う（PTY 読み取りスレッド）

### テスト

- テスト関数名は日本語で記述する（例: `fn bsp_垂直分割のレイアウト計算()`）
- 新機能には必ずユニットテストを追加する
- `cargo test` が全通過することを確認してから PR を作成する

---

## ブランチ戦略

| ブランチ | 用途 |
|---------|------|
| `main` | 安定版。直接プッシュ禁止 |
| `feature/<name>` | 新機能開発 |
| `fix/<name>` | バグ修正 |

---

## PR のガイドライン

1. **フィーチャーブランチ**から `main` へ PR を出す
2. タイトルは日本語で `<type>: <内容>` 形式（例: `feat: マウスクリックフォーカスを追加`）
3. `cargo test` と `cargo clippy` が通ること
4. `docs/` の関連ドキュメントを更新すること

### コミットメッセージ形式

```
<type>: <説明>

<本文（任意）>
```

| type | 用途 |
|------|------|
| `feat` | 新機能 |
| `fix` | バグ修正 |
| `refactor` | リファクタリング |
| `test` | テスト追加・修正 |
| `docs` | ドキュメント |
| `chore` | ビルド・依存関係の変更 |
| `perf` | パフォーマンス改善 |

---

## デバッグ

### ログの有効化

```bash
# サーバー
NEXTERM_LOG=debug nexterm-server

# GPU クライアント
NEXTERM_LOG=debug nexterm-client-gpu

# Windows
set NEXTERM_LOG=debug && nexterm-server.exe
```

ログレベル: `error` / `warn` / `info` / `debug` / `trace`

### IPC メッセージのデバッグ

`NEXTERM_LOG=trace` で全 IPC メッセージが出力される（大量のログが出るため開発時のみ推奨）。

---

## 主要な依存クレートのバージョン

| クレート | バージョン | 用途 |
|---------|-----------|------|
| `tokio` | 1 | 非同期ランタイム |
| `postcard` | 1 (use-std) | IPC シリアライズ（Sprint 5-1 / ADR-0006 で bincode から移行） |
| `serde` | 1 | シリアライズ |
| `anyhow` | 1 | エラーハンドリング |
| `tracing` | 0.1 | ログ |
| `portable-pty` | 0.8 | PTY 管理 |
| `vte` | 0.13 | VT シーケンスパーサ |
| `wgpu` | 22 | GPU レンダリング |
| `winit` | 0.30 | ウィンドウ管理 |
| `cosmic-text` | 0.12 | フォントレンダリング |
| `ratatui` | 0.27 | TUI レンダリング |
| `crossterm` | 0.27 | TUI 入出力 |
| `mlua` | 0.10 | Lua 組み込み |
| `toml` | 0.8 | TOML パーサ |
| `notify` | 6 | ファイル監視 |
| `arboard` | 3 | クリップボード操作 |
| `clap` | 4 | CLI 引数パーサ |
