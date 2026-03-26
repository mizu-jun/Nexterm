# nexterm アーキテクチャ設計書

## 概要

nexterm はデーモンレス設計の Rust 製ターミナルマルチプレクサである。
サーバープロセスが PTY セッションを保持し続け、クライアントは随時接続・切断できる。
GPU クライアント（wgpu）と TUI クライアント（ratatui）の 2 種類のフロントエンドを持つ。

---

## クレート依存グラフ

```
nexterm-client-gpu
  └── nexterm-proto
  └── nexterm-config

nexterm-client-tui
  └── nexterm-proto

nexterm-server
  └── nexterm-proto
  └── nexterm-vt

nexterm-proto   (共有型・メッセージ定義)
nexterm-vt      (VT100 パーサ・仮想スクリーン)
nexterm-config  (TOML + Lua 設定)
```

循環依存はない。`nexterm-proto` が唯一の共有クレートであり、すべての IPC 型を定義する。

---

## プロセス構成

```
┌───────────────────────────────────────┐
│       nexterm-client-gpu / tui         │
│   winit イベントループ / crossterm      │
│   wgpu レンダラー / ratatui レンダラー   │
└──────────────────┬────────────────────┘
                   │ IPC (bincode / Named Pipe / Unix Socket)
┌──────────────────▼────────────────────┐
│          nexterm-server                │
│   SessionManager                       │
│     └── Session                        │
│           └── Window (BSP レイアウト)   │
│                 └── Pane (PTY 管理)    │
└──────────────────┬────────────────────┘
                   │ portable-pty
┌──────────────────▼────────────────────┐
│     OS PTY (ConPTY / Unix PTY)         │
│     シェル / アプリケーション            │
└───────────────────────────────────────┘
```

---

## サーバー側アーキテクチャ

### セッション階層

```
SessionManager
  └── HashMap<String, Session>   (セッション名 → Session)
        └── Session
              ├── name: String
              ├── cols, rows: u16   (端末全体サイズ)
              ├── client_tx: Option<Sender<ServerToClient>>
              └── HashMap<u32, Window>  (Window ID → Window)
                    └── Window
                          ├── id, name: String
                          ├── focused_pane_id: u32
                          ├── layout: SplitNode        (BSP ツリー)
                          └── HashMap<u32, Pane>       (Pane ID → Pane)
                                └── Pane
                                      ├── id: u32
                                      ├── cols, rows: u16
                                      ├── shared_tx: Arc<Mutex<Sender<ServerToClient>>>
                                      ├── master: Box<dyn MasterPty>
                                      └── writer: Mutex<Box<dyn Write>>
```

### PTY 読み取りスレッド

各 `Pane` は生成時に `tokio::task::spawn_blocking` で読み取りスレッドを起動する。

```
PTY 読み取りスレッド (blocking)
  loop {
    reader.read(&mut buf)
    VtParser::advance(buf)
    Screen::take_dirty_rows()  → GridDiff メッセージ送信
    Screen::take_pending_images() → ImagePlaced メッセージ送信
  }
```

PTY 出力先チャネルは `Arc<Mutex<Sender<ServerToClient>>>` で保持し、
クライアント再接続時に `update_tx()` で差し替える。これによりデーモンレス設計を実現している。

### クライアント再接続フロー

```
クライアント接続
  → IPC::Attach { session_name }
  → get_or_create_and_attach()
      → Session::attach(new_tx)
          → window.update_tx_for_all(&tx)   (全ペインの Sender を差し替え)
  → FullRefresh 送信
  → LayoutChanged 送信
  → SessionList 送信
```

---

## BSP レイアウトエンジン

### データ構造

```rust
enum SplitNode {
    Pane { pane_id: u32 },
    Split {
        dir: SplitDir,   // Vertical(左右) | Horizontal(上下)
        ratio: f32,      // 左/上の占有率 (0.0〜1.0)
        left: Box<SplitNode>,
        right: Box<SplitNode>,
    },
}
```

### ペイン分割の手順（Chicken-and-Egg 問題の解決）

1. `new_pane_id()` で ID を事前発行する
2. `layout.insert_after(focused_id, new_id, dir)` でツリーに挿入する
3. `compute_layouts(cols, rows)` で全ペインの矩形を再帰計算する
4. `Pane::spawn_with_id(new_id, rect.cols, rect.rows, ...)` でペインを生成する
5. 既存ペインを新しいサイズにリサイズする

### レイアウト計算（再帰）

```
compute(col_off, row_off, cols, rows, out):
  Pane → out.push(PaneRect { pane_id, col_off, row_off, cols, rows })
  Split(Vertical):
    left_cols = floor(cols * ratio)
    right_cols = cols - left_cols - 1   // 境界線 1 列
    compute(left, col_off, ...)
    compute(right, col_off + left_cols + 1, ...)
  Split(Horizontal):
    top_rows = floor(rows * ratio)
    bot_rows = rows - top_rows - 1      // 境界線 1 行
    compute(left, row_off, ...)
    compute(right, row_off + top_rows + 1, ...)
```

---

## IPC 層

### トランスポート

| OS | トランスポート | パス |
|----|--------------|------|
| Linux / macOS | Unix Domain Socket | `$XDG_RUNTIME_DIR/nexterm.sock` |
| Windows | Named Pipe | `\\.\pipe\nexterm-<USERNAME>` |

### フレーミング

すべてのメッセージは 4 バイト LE 長さプレフィックス + bincode ペイロードで送受信する。

```
┌────────────────┬─────────────────────────┐
│ 4B (LE u32)    │ N バイト (bincode)       │
│ ペイロード長   │ メッセージ本体           │
└────────────────┴─────────────────────────┘
```

### スレッドモデル（サーバー側）

```
tokio::spawn(handle_client)
  ├── tokio::spawn (送信ループ: rx → write_half)
  └── 受信ループ:  read_half → dispatch()
```

各クライアント接続ごとに非同期タスクが 2 つ起動する（送信・受信を分離）。

---

## VT パーサ

`nexterm-vt` クレートは `vte` クレートをラップして仮想スクリーンを管理する。

```
VtParser
  ├── vte::Parser     (バイトストリーム → コールバック)
  └── Screen
        ├── Grid (Cell[][] : 仮想グリッド)
        ├── dirty: Vec<bool>     (行ダーティフラグ)
        ├── cursor: (u16, u16)
        └── pending_images: Vec<PendingImage>
```

### ダーティ差分の配信

- `Screen::take_dirty_rows()` がダーティ行を `Vec<DirtyRow>` として取り出す
- `DirtyRow { row: u16, cells: Vec<Cell> }` を `GridDiff` メッセージでクライアントに送信する
- クライアントは受信した差分をローカルグリッドにマージする

### 画像プロトコル

| プロトコル | デコーダ | 送信メッセージ |
|-----------|---------|-------------|
| Sixel | DCS `q` シーケンスのデコード | `ImagePlaced { rgba, width, height, col, row }` |
| Kitty | APC `G` シーケンスのデコード | 同上 |

---

## GPU クライアント（nexterm-client-gpu）

### レンダリングパイプライン

wgpu のカスタムシェーダーと cosmic-text のグリフアトラスを組み合わせた 3 パス構成。

```
Render Pass
  ├── Pass 1: 背景矩形 (bg_verts)
  │     └── グリッド各セルの背景色 + カーソル矩形
  ├── Pass 2: テキスト (text_verts)
  │     └── cosmic-text グリフアトラスから UV サンプリング
  └── Pass 3: 画像 (img_verts)
        └── ImagePlaced RGBA テクスチャ
```

### マルチペイン描画

`pane_layouts` が非空のとき（サーバー接続済み）、各 `PaneLayout` のオフセットを使って
それぞれのペインを正しい位置に描画する。

```
for layout in pane_layouts:
  off_x = layout.col_offset * cell_w
  off_y = layout.row_offset * cell_h
  rect  = (off_x, off_y, layout.cols * cell_w, layout.rows * cell_h)

  if scroll_active:
    build_scrollback_verts_in_rect(pane, layout, rect)
  else:
    build_grid_verts_in_rect(pane, layout, rect)
    build_border_verts(pane, layout)   // 隣接ペインとの境界線
```

非フォーカスペインのテキスト色は 70% に減光する。
フォーカスペインの境界線はブルー `[0.30, 0.55, 0.90]`、非フォーカスはグレー `[0.35, 0.35, 0.42]`。

### イベントループ（winit 0.30 ApplicationHandler）

```
ApplicationHandler
  ├── new_events()          — アプリ起動
  ├── resumed()             — ウィンドウ生成・wgpu 初期化
  ├── window_event()
  │     ├── KeyboardInput   → ClientToServer::KeyEvent
  │     ├── Resized         → ClientToServer::Resize
  │     └── CloseRequested  → exit
  └── about_to_wait()       — PTY 出力ポーリング (16ms 間隔) → 再描画
```

### クライアント状態管理（ClientState）

```
ClientState
  ├── panes: HashMap<u32, PaneState>      (受信済みグリッド)
  ├── focused_pane_id: Option<u32>
  ├── pane_layouts: HashMap<u32, PaneLayout>
  ├── palette: CommandPalette             (コマンドパレット)
  └── search: SearchState                 (インクリメンタル検索)

PaneState
  ├── grid: Grid
  ├── cursor_col, cursor_row: u16
  ├── scrollback: Scrollback
  ├── scroll_offset: usize
  └── images: HashMap<u32, PlacedImage>
```

---

## TUI クライアント（nexterm-client-tui）

ratatui + crossterm で構築した軽量フォールバッククライアント。
GPU が使えない環境（SSH 接続先など）での利用を想定している。

- 単一ペイン表示（BSP レイアウト情報の `is_focused` のみ利用）
- `crossterm` のキーイベントを `ClientToServer::KeyEvent` に変換して送信
- `ratatui` の `Paragraph` ウィジェットでグリッドを描画

---

## 設定システム（nexterm-config）

### ロード順序

```
1. デフォルト値 (Rust Default トレイト)
2. config.toml の読み込み（TOML デシリアライズ）
3. config.lua の実行（Lua オーバーライド）
4. 結果を Config 構造体にマージ
```

### ホットリロード

`notify` クレートでファイルシステムイベントを監視する。
設定ファイルが変更されると `ConfigWatcher` が再ロードして `Config` を更新する。

### 設定スキーマ

| フィールド | 型 | デフォルト値 |
|-----------|-----|------------|
| `font.family` | String | `"monospace"` |
| `font.size` | f32 | `14.0` |
| `font.ligatures` | bool | `true` |
| `colors` | ColorScheme | `dark` |
| `shell.program` | String | OS 依存 |
| `scrollback_lines` | usize | `50000` |
| `status_bar.enabled` | bool | `false` |

---

## セッション永続化

サーバーはシャットダウン時にセッション名を `persist.toml` に書き出す（`nexterm-server/src/persist.rs`）。
再起動時にこのファイルを読み込み、セッションを復元する（将来実装予定）。

```
# 保存パス
Linux / macOS: $XDG_RUNTIME_DIR/nexterm-persist.toml
Windows:       %APPDATA%\nexterm\persist.toml
```

---

## エラー処理方針

- すべてのエラーは `anyhow::Result` で伝播する
- IPC 受信ループでのデシリアライズエラーは `continue`（1 メッセージの破棄）
- PTY 読み取りエラーは `break`（スレッド終了、ペインが無効化）
- クライアント切断はエラーではなく正常系（`read_exact` Err → `break` → デタッチ）

---

## テスト戦略

| レイヤー | テスト内容 | 件数 |
|---------|-----------|------|
| nexterm-proto | bincode 往復シリアライズ | 4 件 |
| nexterm-vt | VT シーケンス・ダーティフラグ・リサイズ | 6 件 |
| nexterm-config | デフォルト生成・TOML 往復 | 4 件 |
| nexterm-server | BSP 計算・セッション管理 | 7 件 |
| nexterm-client-gpu | ClientState メッセージ適用・検索ライフサイクル | 3 件 |
| nexterm-client-tui | ClientState メッセージ適用 | 2 件 |
| **合計** | | **26 件以上** |

---

## Phase 3 完了済みタスク

| ステップ | 内容 | 状態 |
|---------|------|------|
| 3-4 | マウスサポート（クリックフォーカス / ホイールスクロール） | ✅ 完了 |
| 3-5 | クリップボード統合（arboard クレート、Ctrl+Shift+C/V） | ✅ 完了 |
| 3-6 | nexterm-ctl CLI（list / new / attach / kill） | ✅ 完了 |
| 3-7 | 設定ホットリロード → GPU クライアント反映 | ✅ 完了 |
| 3-8 | Lua ステータスバーウィジェット | ✅ 完了 |

### 3-4: マウスサポート実装詳細

| イベント | 処理 |
|---------|------|
| `CursorMoved` | カーソル位置を `cursor_position: Option<(f64, f64)>` に保存 |
| `MouseInput Left Released` | セル座標 → `pane_layouts` 検索 → `FocusPane` 送信 |
| `MouseWheel` | `LineDelta` / `PixelDelta` → `scroll_up` / `scroll_down` |

### 3-5: クリップボード統合

| ショートカット | 動作 |
|-------------|------|
| `Ctrl+Shift+C` | フォーカスペインの可視グリッドをテキスト変換してコピー |
| `Ctrl+Shift+V` | クリップボードから `PasteText` メッセージで PTY にペースト |

### 3-6: nexterm-ctl コマンド一覧

```
nexterm-ctl list              セッション一覧を表示
nexterm-ctl new <name>        新規セッションを作成
nexterm-ctl attach <name>     アタッチ方法を案内
nexterm-ctl kill <name>       セッションを強制終了
```

IPC: `ListSessions` / `KillSession` の 2 つの新規プロトコルメッセージを追加。

### 3-7: 設定ホットリロード

`nexterm-config::watch_config()` で `~/.config/nexterm/` を監視。
変更検知時に `Config` を受信し、フォント変更の場合はグリフアトラスも再生成する。

### 3-8: Lua ステータスバーウィジェット

`nexterm-config::StatusBarEvaluator` が Lua 式を評価。
`about_to_wait` で 1 秒ごとに再評価してステータスラインの右端に表示する。

**nexterm.lua 設定例:**

```lua
return {
  status_bar = {
    enabled = true,
    widgets = { 'os.date("%H:%M:%S")', '"nexterm"' },
  }
}
```
