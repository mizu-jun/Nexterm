# nexterm

Rust 製のターミナルマルチプレクサ。tmux/zellij インスパイアで、wgpu による GPU レンダリングと Lua 設定システムを搭載する。

## 特徴

- **GPU レンダリング** — wgpu + cosmic-text による高速フォントレンダリング
- **デーモンレス設計** — サーバープロセスが PTY を保持し、クライアント再接続時にセッションを復元
- **BSP 分割レイアウト** — Binary Space Partition で任意深さのペイン分割に対応
- **Lua + TOML 設定** — TOML でデフォルト値、Lua で動的オーバーライド。ファイル変更を自動検知してリアルタイム反映
- **マウス操作** — クリックでペインフォーカス切り替え、ホイールでスクロールバック操作
- **クリップボード統合** — Ctrl+Shift+C でコピー、Ctrl+Shift+V でペースト（arboard）
- **Lua ステータスバー** — `os.date()` 等の Lua 式をステータスラインに 1 秒ごとに表示
- **nexterm-ctl** — セッションの一覧・作成・終了を行う CLI ツール
- **画像プロトコル** — Sixel / Kitty 両形式の画像表示に対応
- **TUI フォールバック** — GPU クライアントが使えない環境向けに ratatui ベースの TUI クライアントを同梱
- **クロスプラットフォーム** — Linux / macOS / Windows 対応（Windows は ConPTY + Named Pipe を使用）

## 実装状況

### Phase 1: コア基盤（完成）

| コンポーネント | 内容 | 状態 |
|--------------|------|------|
| `nexterm-proto` | IPC プロトコル型定義 (bincode) | ✅ |
| `nexterm-vt` | VT100/ANSI パーサ + Sixel/Kitty デコード | ✅ |
| `nexterm-server` | PTY サーバー (セッション・ウィンドウ・ペイン管理) | ✅ |
| `nexterm-client-tui` | TUI クライアント (ratatui + crossterm) | ✅ |

### Phase 2: GPU クライアント & 設定（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 2-1 | nexterm-config — TOML + Lua 設定 + ホットリロード | ✅ |
| 2-2 | nexterm-client-gpu — wgpu レンダラー基盤 | ✅ |
| 2-3 | Sixel / Kitty 画像プロトコル対応 | ✅ |
| 2-4 | スクロールバック & インクリメンタル検索 | ✅ |
| 2-5 | コマンドパレット (fuzzy マッチ) | ✅ |

### Phase 3: マルチペイン & 拡張（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 3-1 | サーバー側 BSP レイアウトモデル | ✅ |
| 3-2 | プロトコル拡張 (LayoutChanged / FocusPane / PasteText) | ✅ |
| 3-3 | GPU クライアント マルチペイン分割表示 | ✅ |
| 3-4 | マウスサポート (クリックフォーカス / ホイールスクロール) | ✅ |
| 3-5 | クリップボード統合 (Ctrl+Shift+C/V) | ✅ |
| 3-6 | nexterm-ctl CLI ツール (list / new / attach / kill) | ✅ |
| 3-7 | 設定ホットリロード → GPU クライアント即時反映 | ✅ |
| 3-8 | Lua ステータスバーウィジェット | ✅ |

**テスト**: 69 件、全通過

## クレート構成

```
nexterm/
├── nexterm-proto        # IPC メッセージ型・シリアライズ
├── nexterm-vt           # VT100 パーサ・仮想スクリーン・画像デコード
├── nexterm-server       # PTY サーバー (IPC + セッション管理)
├── nexterm-config       # 設定ロード (TOML + Lua) + StatusBarEvaluator
├── nexterm-client-tui   # TUI クライアント
├── nexterm-client-gpu   # GPU クライアント (wgpu + winit)
└── nexterm-ctl          # セッション制御 CLI
```

## ビルド

### 前提条件

- Rust 1.80 以上
- Windows の場合: Visual Studio Build Tools (C++ コンパイラ)
- Linux の場合: `libx11-dev`, `libxkbcommon-dev`, `libwayland-dev`（winit 依存）

### ビルド

```bash
# 全クレートをビルド
cargo build --release

# サーバーのみ
cargo build --release -p nexterm-server

# GPU クライアントのみ
cargo build --release -p nexterm-client-gpu
```

### テスト

```bash
cargo test
```

## 使い方

### サーバーを起動する

```bash
# デバッグログ付きで起動
NEXTERM_LOG=info nexterm-server

# Windows
set NEXTERM_LOG=info && nexterm-server.exe
```

サーバーは以下のソケットをリッスンする：

| OS | パス |
|----|------|
| Linux / macOS | `$XDG_RUNTIME_DIR/nexterm.sock` |
| Windows | `\\.\pipe\nexterm-<USERNAME>` |

### GPU クライアントを起動する

```bash
nexterm-client-gpu
```

起動時に自動的にサーバーへ接続し、`main` セッションにアタッチする。サーバーが起動していない場合はオフラインモードで起動する。

### TUI クライアントを起動する

```bash
nexterm-client-tui
```

## キーバインド（GPU クライアント）

### 全般

| キー | 動作 |
|------|------|
| `Ctrl+Shift+P` | コマンドパレットを開く／閉じる |
| `Ctrl+F` | スクロールバック検索を開始する |
| `PageUp` | スクロールバックを上にスクロール |
| `PageDown` | スクロールバックを下にスクロール |
| `Escape` | 検索・パレットを閉じる |
| `Enter`（検索中） | 次のマッチへ移動 |
| 通常のキー入力 | フォーカスペインの PTY へ転送 |

### クリップボード

| キー | 動作 |
|------|------|
| `Ctrl+Shift+C` | フォーカスペインの可視グリッドをクリップボードにコピー |
| `Ctrl+Shift+V` | クリップボードの内容をフォーカスペインにペースト |

### マウス操作

| 操作 | 動作 |
|------|------|
| 左クリック | クリックしたペインにフォーカスを移動 |
| ホイール上 | スクロールバックを上にスクロール（3行単位） |
| ホイール下 | スクロールバックを下にスクロール（3行単位） |

### ペイン操作（サーバー側プロトコル経由）

| メッセージ | 動作 |
|-----------|------|
| `SplitVertical` | フォーカスペインを左右に分割 |
| `SplitHorizontal` | フォーカスペインを上下に分割 |
| `FocusNextPane` | 次のペインにフォーカス移動 |
| `FocusPrevPane` | 前のペインにフォーカス移動 |
| `FocusPane { pane_id }` | 指定ペインにフォーカス移動（マウスクリック時） |

## nexterm-ctl の使い方

サーバーが起動しているときに使用できるセッション制御 CLI。

```bash
# セッション一覧を表示する
nexterm-ctl list

# 新規セッション 'work' を作成する
nexterm-ctl new work

# セッション 'work' へのアタッチ方法を確認する
nexterm-ctl attach work

# セッション 'work' を強制終了する
nexterm-ctl kill work
```

## 設定

設定ファイルは以下のパスから自動検索される：

| OS | パス |
|----|------|
| Linux / macOS | `~/.config/nexterm/config.toml` |
| Windows | `%APPDATA%\nexterm\config.toml` |

### nexterm.toml の例

```toml
scrollback_lines = 50000

[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true

[colors]
scheme = "tokyonight"

[shell]
program = "/usr/bin/fish"

[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']
```

### nexterm.lua オーバーライドの例

```lua
-- ~/.config/nexterm/nexterm.lua
local cfg = require("nexterm")

-- フォントサイズをランタイムで変更する
cfg.font.size = 16.0

-- ステータスバーに時刻とセッション名を表示する
cfg.status_bar.enabled = true
cfg.status_bar.widgets = { 'os.date("%H:%M")', '"main"' }

return cfg
```

> 設定ファイルを保存するとリアルタイムで反映されます（ホットリロード）。

## アーキテクチャ概要

```
┌─────────────────────────────────────┐
│         nexterm-client-gpu          │
│  wgpu レンダラー / winit イベントループ  │
└──────────────┬──────────────────────┘
               │ IPC (bincode / Named Pipe / Unix Socket)
┌──────────────▼──────────────────────┐
│         nexterm-server              │
│  セッション → ウィンドウ → ペイン(PTY) │
│  BSP レイアウトエンジン               │
└──────────────┬──────────────────────┘
               │ portable-pty
┌──────────────▼──────────────────────┐
│       OS PTY (ConPTY / Unix)        │
│       シェル / アプリケーション        │
└─────────────────────────────────────┘
```

詳細は各ドキュメントを参照：

| ドキュメント | 内容 |
|------------|------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | クレート構成・データフロー・レンダリングパイプライン |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | IPC プロトコル仕様（メッセージ型・フレーミング・シーケンス図） |
| [docs/DESIGN.md](docs/DESIGN.md) | 基本設計書・ADR（設計判断の記録） |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | TOML / Lua 設定の全フィールドリファレンス |
| [CONTRIBUTING.md](CONTRIBUTING.md) | ビルド手順・コーディング規約・PR ガイドライン |

## ライセンス

MIT OR Apache-2.0
