# nexterm IPC プロトコル仕様

## 概要

nexterm のクライアント-サーバー間通信は **bincode** シリアライズ + **4 バイト LE 長さプレフィックス** フレーミングで定義する。
トランスポート層は OS により異なるが、フレーミングとメッセージ形式は共通である。

---

## トランスポート層

| OS | トランスポート | パス |
|----|--------------|------|
| Linux / macOS | Unix Domain Socket | `$XDG_RUNTIME_DIR/nexterm.sock` (mode 0600) |
| Windows | Named Pipe | `\\.\pipe\nexterm-<USERNAME>` |

---

## フレーミング

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
├─────────────────────────────────────────────────────────────────┤
│              Payload Length (u32, little-endian)                │
├─────────────────────────────────────────────────────────────────┤
│              Payload (bincode-encoded message)                  │
│                        (variable length)                        │
└─────────────────────────────────────────────────────────────────┘
```

- `Payload Length` はペイロードのバイト数（ヘッダー自身を含まない）
- ペイロードは `bincode` のデフォルト設定でエンコードされた列挙型

---

## クライアント → サーバー メッセージ (`ClientToServer`)

### `Ping`

接続確認。サーバーは `Pong` を返す。

```
{ Ping }
```

### `Attach`

セッションにアタッチする。セッションが存在しない場合は新規作成する。

```
{ Attach { session_name: String } }
```

**応答**: `FullRefresh` → `LayoutChanged` → `SessionList`（順番に送信）

### `Detach`

セッションからデタッチする（クライアントの意図的な切断）。

```
{ Detach }
```

### `KeyEvent`

キー入力イベント。サーバーがフォーカスペインの PTY に書き込む。

```
{ KeyEvent { code: KeyCode, modifiers: Modifiers } }
```

#### `KeyCode` 値

| 値 | 説明 |
|----|------|
| `Char(char)` | 通常文字 |
| `F(u8)` | ファンクションキー F1〜F12 |
| `Enter` | Enter キー |
| `Backspace` | Backspace キー |
| `Delete` | Delete キー |
| `Escape` | Escape キー |
| `Tab` | Tab キー |
| `BackTab` | Shift+Tab |
| `Up` / `Down` / `Left` / `Right` | 矢印キー |
| `Home` / `End` | Home / End キー |
| `PageUp` / `PageDown` | ページスクロール |
| `Insert` | Insert キー |

#### `Modifiers` ビットフラグ

| ビット | 修飾キー |
|-------|---------|
| `0b0001` | Shift |
| `0b0010` | Ctrl |
| `0b0100` | Alt |
| `0b1000` | Meta (Super) |

### `Resize`

端末サイズ変更通知。サーバーはすべてのペインを BSP 計算で再配置する。

```
{ Resize { cols: u16, rows: u16 } }
```

**応答**: `LayoutChanged`

### `SplitVertical`

フォーカスペインを左右に分割する（境界線 1 列を挟んで等分）。

```
{ SplitVertical }
```

**応答**: `FullRefresh` → `LayoutChanged`

### `SplitHorizontal`

フォーカスペインを上下に分割する（境界線 1 行を挟んで等分）。

```
{ SplitHorizontal }
```

**応答**: `FullRefresh` → `LayoutChanged`

### `FocusNextPane`

ペイン ID 昇順で次のペインにフォーカスを移動する。

```
{ FocusNextPane }
```

**応答**: `LayoutChanged`

### `FocusPrevPane`

ペイン ID 昇順で前のペインにフォーカスを移動する。

```
{ FocusPrevPane }
```

**応答**: `LayoutChanged`

### `FocusPane`

指定した ID のペインにフォーカスを移動する（マウスクリックなど）。

```
{ FocusPane { pane_id: u32 } }
```

**応答**: `LayoutChanged`

### `PasteText`

テキストをフォーカスペインの PTY にそのまま書き込む（クリップボードペースト用）。

```
{ PasteText { text: String } }
```

**応答**: なし（PTY への書き込みのみ）

### `ListSessions`

アタッチなしでセッション一覧を取得する（`nexterm-ctl list` が使用）。

```
{ ListSessions }
```

**応答**: `SessionList`

### `KillSession`

指定したセッションを強制終了する（PTY プロセスを Drop する）。

```
{ KillSession { name: String } }
```

**応答**: `SessionList`（終了後の最新一覧）または `Error`

### `StartRecording`

フォーカスペインの PTY 出力をファイルへの録画を開始する。

```
{ StartRecording { session_name: String, output_path: String } }
```

**応答**: `RecordingStarted` または `Error`

### `StopRecording`

録画を停止する。

```
{ StopRecording { session_name: String } }
```

**応答**: `RecordingStopped` または `Error`

---

## サーバー → クライアント メッセージ (`ServerToClient`)

### `Pong`

`Ping` への応答。

```
{ Pong }
```

### `FullRefresh`

ペインのグリッド全体スナップショット。アタッチ時・ペイン作成時に送信する。

```
{ FullRefresh { pane_id: u32, grid: Grid } }
```

#### `Grid` 構造体

```
Grid {
    width: u16,
    height: u16,
    rows: Vec<Vec<Cell>>,   // rows[y][x]
    cursor_col: u16,
    cursor_row: u16,
}
```

#### `Cell` 構造体

```
Cell {
    ch: char,                  // 表示文字 (デフォルト: ' ')
    fg: Color,                 // 前景色
    bg: Color,                 // 背景色
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}
```

#### `Color` 型

```
Color = Reset | Indexed(u8) | Rgb(u8, u8, u8)
```

### `GridDiff`

差分グリッド更新。ダーティ行のみを送信する。

```
{
    GridDiff {
        pane_id: u32,
        dirty_rows: Vec<DirtyRow>,
        cursor_col: u16,
        cursor_row: u16,
    }
}
```

#### `DirtyRow` 構造体

```
DirtyRow {
    row: u16,              // 行インデックス (0 始まり)
    cells: Vec<Cell>,      // 行全体のセル配列
}
```

### `LayoutChanged`

ペインレイアウトの変更通知。分割・フォーカス変更・リサイズ時に送信する。

```
{
    LayoutChanged {
        panes: Vec<PaneLayout>,
        focused_pane_id: u32,
    }
}
```

#### `PaneLayout` 構造体

```
PaneLayout {
    pane_id: u32,
    col_offset: u16,   // ウィンドウ内の列オフセット (0 始まり)
    row_offset: u16,   // ウィンドウ内の行オフセット (0 始まり)
    cols: u16,         // ペインの幅
    rows: u16,         // ペインの高さ
    is_focused: bool,
}
```

### `SessionList`

セッション一覧。`Attach` 成功後に送信する。

```
{ SessionList { sessions: Vec<SessionInfo> } }
```

#### `SessionInfo` 構造体

```
SessionInfo {
    name: String,
    window_count: u32,
    attached: bool,
}
```

### `ImagePlaced`

Sixel / Kitty プロトコルの画像配置通知。

```
{
    ImagePlaced {
        pane_id: u32,
        image_id: u32,
        col: u16,        // グリッド上の配置列
        row: u16,        // グリッド上の配置行
        width: u32,      // 画像ピクセル幅
        height: u32,     // 画像ピクセル高さ
        rgba: Vec<u8>,   // RGBA ピクセルデータ (width * height * 4 バイト)
    }
}
```

### `Bell`

VT BEL（`\x07`）をフォーカスペインが受信したときに送信する。クライアントは OS ウィンドウ注目要求（`request_user_attention`）をトリガーする。

```
{ Bell { pane_id: u32 } }
```

### `RecordingStarted`

録画開始の確認通知。

```
{ RecordingStarted { pane_id: u32, path: String } }
```

### `RecordingStopped`

録画停止の確認通知。

```
{ RecordingStopped { pane_id: u32 } }
```

### `Error`

エラー通知。操作が失敗した場合に送信する。

```
{ Error { message: String } }
```

---

## 接続シーケンス図

### 通常クライアント（GPU / TUI）

```
Client                          Server
  │                               │
  │──── Attach { "main" } ───────>│
  │                               │ get_or_create_and_attach()
  │<─── FullRefresh { pane_id=1 }─│
  │<─── LayoutChanged { panes }───│
  │<─── SessionList { sessions }──│
  │                               │
  │──── Resize { 220, 60 } ──────>│
  │<─── LayoutChanged { panes }───│
  │                               │
  │──── KeyEvent { 'l', CTRL } ──>│
  │                    PTY 書き込み│
  │<─── GridDiff { pane_id=1 } ───│
  │                               │
  │──── SplitVertical ───────────>│
  │<─── FullRefresh { pane_id=2 }─│
  │<─── LayoutChanged { panes }───│
  │                               │
  │──── Detach ──────────────────>│
  │  (接続を閉じる)                │ session.detach()
```

### nexterm-ctl list

```
nexterm-ctl                     Server
  │                               │
  │──── ListSessions ────────────>│
  │<─── SessionList { sessions }──│
  │  (接続を閉じる)                │
```

### nexterm-ctl kill

```
nexterm-ctl                     Server
  │                               │
  │──── KillSession { "main" } ──>│
  │                               │ sessions.remove("main")
  │<─── SessionList { sessions }──│
  │  (接続を閉じる)                │
```

### クリップボードペースト（Ctrl+Shift+V）

```
GPU Client                      Server
  │  arboard::get_text()          │
  │──── PasteText { text } ──────>│
  │                    PTY 書き込み│
  │<─── GridDiff { pane_id=N } ───│
```

### セッション録画（nexterm-ctl record）

```
nexterm-ctl                     Server
  │                               │
  │── StartRecording { "main",    │
  │       "output.log" } ────────>│  BufWriter<File> を生成
  │<── RecordingStarted { pane=1, │
  │        path="output.log" } ───│
  │  (接続を閉じる)                │
  │                               │  PTY → parser → BufWriter (バックグラウンド)
  │                               │
  │── StopRecording { "main" } ──>│  BufWriter::flush + drop
  │<── RecordingStopped { pane=1 }│
  │  (接続を閉じる)                │
```

### VT BEL 通知

```
PTY (Shell)         Server                GPU Client
  │                   │                       │
  │── \x07 ──────────>│                       │
  │              take_pending_bell()           │
  │<── Bell { pane_id=1 } ───────────────────>│
  │                   │          request_user_attention()
  │                   │               (OS ウィンドウ点滅)
```

---

## バージョン管理

現在のプロトコルバージョンは **1.0**。
bincode のデフォルト設定（little-endian, fixed-width integer）を使用する。
プロトコル変更時は後方互換性のない変更を避け、新しいメッセージバリアントを追加する形で拡張する。
