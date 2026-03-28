# nexterm — 日本語ドキュメント

> **English documentation:** [README.md](README.md)

Rust 製のターミナルマルチプレクサ。tmux/zellij インスパイアで、wgpu による GPU レンダリングと Lua 設定システムを搭載する。

## 特徴

- **GPU レンダリング** — wgpu + cosmic-text による高速フォントレンダリング
- **デーモンレス設計** — サーバープロセスが PTY を保持し、クライアント再接続時にセッションを復元
- **BSP 分割レイアウト** — Binary Space Partition で任意深さのペイン分割に対応
- **Lua + TOML 設定** — TOML でデフォルト値、Lua で動的オーバーライド。ファイル変更を自動検知してリアルタイム反映
- **マウス操作** — クリックでペインフォーカス切り替え、ホイールでスクロールバック操作、Ctrl+Click で URL を開く
- **クリップボード統合** — Ctrl+Shift+C でコピー、Ctrl+Shift+V でペースト（arboard）
- **コピーモード** — Vim 風テキスト選択（Ctrl+[, hjkl, v, y）
- **Lua ステータスバー** — `os.date()` 等の Lua 式をステータスラインに 1 秒ごとに表示
- **タブバー** — WezTerm スタイルのタブバー（ペインラベル + `❯` セパレータ）
- **セッション録画** — `nexterm-ctl record start/stop` で PTY 出力をファイルに保存
- **BEL 通知** — VT BEL (`\x07`) 受信時に OS ウィンドウ注目要求をトリガー
- **フォントサイズ変更** — Ctrl+= / Ctrl+- / Ctrl+0 でランタイムにサイズを変更
- **ウィンドウ透過** — 不透明度・ボーダーレス・macOS ぼかしを設定ファイルで制御
- **URL 検出** — グリッド内の URL をアンダーライン表示し Ctrl+Click でブラウザ起動
- **nexterm-ctl** — セッションの一覧・作成・終了・録画を行う CLI ツール
- **画像プロトコル** — Sixel / Kitty 両形式の画像表示に対応
- **TUI フォールバック** — GPU クライアントが使えない環境向けに ratatui ベースの TUI クライアントを同梱
- **クロスプラットフォーム** — Linux / macOS / Windows 対応（Windows は ConPTY + Named Pipe を使用）
- **多言語対応** — UI を 8 言語に対応（英語・フランス語・ドイツ語・スペイン語・イタリア語・簡体字中国語・日本語・韓国語）

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

### Phase 4: ローカライゼーション & プロジェクト構造（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 4-1 | 標準 OSS ディレクトリ構造（.github, examples, tests） | ✅ |
| 4-2 | nexterm-i18n クレート（JSON ロケール埋め込み, sys-locale 検出） | ✅ |
| 4-3 | UI 文字列の 8 言語対応 | ✅ |
| 4-4 | 英語 README・ドキュメント整備 | ✅ |

### Phase 5: UX 強化（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 5-A | セッション録画（`nexterm-ctl record start/stop`） | ✅ |
| 5-B | WezTerm スタイル タブバー（`❯` セパレータ） | ✅ |
| 5-C | ウィンドウ透過・ぼかし・ボーダーレス | ✅ |
| 5-D | Vim 風コピーモード（Ctrl+[, hjkl, v で選択, y でヤンク） | ✅ |
| 5-E | ランタイム フォントサイズ変更（Ctrl+= / Ctrl+- / Ctrl+0） | ✅ |
| 5-F | URL 検出 + Ctrl+Click でブラウザ起動 | ✅ |
| 5-G | VT BEL 通知 → OS ウィンドウ注目要求 | ✅ |

### Phase 6: セキュリティ & 信頼性（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 6-1 | IPC ピア UID 検証（Linux: SO_PEERCRED、macOS: getpeereid） | ✅ |
| 6-2 | `StartRecording` のパストラバーサル防止 | ✅ |
| 6-3 | Windows Named Pipe: リモートクライアント拒否 | ✅ |
| 6-4 | LuaWorker バックグラウンドスレッド（メインスレッドのブロックを解消） | ✅ |
| 6-5 | セッションスナップショット永続化（JSON、自動保存・復元） | ✅ |

**テスト**: 86 件以上、全通過

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

### フォントサイズ

| キー | 動作 |
|------|------|
| `Ctrl+=` | フォントサイズを 1pt 増やす |
| `Ctrl+-` | フォントサイズを 1pt 減らす |
| `Ctrl+0` | フォントサイズを設定値にリセット |

### クリップボード

| キー | 動作 |
|------|------|
| `Ctrl+Shift+C` | フォーカスペインの可視グリッドをクリップボードにコピー |
| `Ctrl+Shift+V` | クリップボードの内容をフォーカスペインにペースト |

### コピーモード（Vim 風）

| キー | 動作 |
|------|------|
| `Ctrl+[` | コピーモードに入る |
| `h` / `j` / `k` / `l` | カーソルを左 / 下 / 上 / 右に移動 |
| `v` | 選択開始をトグル |
| `y` | 選択範囲をクリップボードにコピーしてモード終了 |
| `q` / `Escape` | コピーモードを終了 |

### マウス操作

| 操作 | 動作 |
|------|------|
| 左クリック | クリックしたペインにフォーカスを移動 |
| `Ctrl` + 左クリック | カーソル下の URL をブラウザで開く |
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

# PTY 出力をファイルに録画する
nexterm-ctl record start work output.log

# 録画を停止する
nexterm-ctl record stop work
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

[window]
background_opacity = 0.95
decorations = "full"   # "full" | "none" | "notitle"

[tab_bar]
enabled = true
height = 28
active_tab_bg = "#ae8b2d"
inactive_tab_bg = "#5c6d74"
separator = "❯"
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
