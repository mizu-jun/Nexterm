# nexterm 基本設計書

## 設計目標

1. **デーモンレス再接続** — クライアントが切断してもセッションが生き続ける
2. **高速レンダリング** — GPU (wgpu) によるゼロコピーに近いグリッド描画
3. **シンプルな状態管理** — クライアントはグリッドのコピーを持ち、差分だけを受け取る
4. **クロスプラットフォーム** — Linux / macOS / Windows を単一コードベースでサポート

---

## ADR-001: デーモンレス設計

### 背景

tmux はサーバープロセス（デーモン）が常駐してセッションを管理する。
nexterm も同様にサーバープロセスが PTY を保持するが、
「デーモン」とは呼ばず、ユーザーが明示的に起動・終了できる設計とした。

### 決定

- `nexterm-server` がサーバープロセスとして独立して動作する
- クライアントはいつでも接続・切断できる
- 切断中も PTY プロセスは継続して動作する

### 実現方法

`Pane` の PTY 読み取りスレッドは `Arc<Mutex<Sender<ServerToClient>>>` を持つ。
クライアント再接続時に `update_tx()` でチャネルを差し替えるだけでよい。

---

## ADR-002: BSP レイアウトエンジン採用

### 背景

tmux は `%` / `"` コマンドでペインを分割する。
nexterm は BSP（Binary Space Partition）ツリーを採用し、任意深さの分割を統一的に扱う。

### 決定

```rust
enum SplitNode {
    Pane { pane_id: u32 },
    Split { dir, ratio, left, right },
}
```

- 分割は常に「フォーカスペインを 2 分割」する
- 比率はデフォルト 50:50（将来的にドラッグで変更可能）
- 境界線は 1 セル幅で固定

### トレードオフ

- **メリット**: 再帰的な計算で任意の複雑なレイアウトを表現できる
- **デメリット**: tmux のような「等幅 3 分割」は直接表現できない（中間ノードの比率調整で対応）

---

## ADR-003: GPU クライアントに wgpu を採用

### 背景

ターミナルエミュレータのテキスト描画は CPU だと大量の文字列処理が必要。
GPU を使えばグリフアトラスから UV サンプリングするだけで高速描画できる。

### 決定

- wgpu: クロスプラットフォーム GPU API（Vulkan / Metal / DX12 / WebGPU）
- cosmic-text: Unicode・CJK 対応グリフラスタライゼーション

### 描画パス構成

```
Pass 1: 背景矩形 (セル背景色・カーソル)
Pass 2: テキスト (グリフアトラスからのサンプリング)
Pass 3: 画像    (Sixel/Kitty RGBA テクスチャ)
```

---

## ADR-004: IPC フォーマットに bincode を採用

### 背景

JSON は人が読めるが低速。Protobuf はスキーマ定義が必要で Rust との相性が低い。

### 決定

`bincode` クレートを採用。

- **メリット**: Rust の `serde` と完全に統合されており、型定義のみで実装できる
- **メリット**: 非常に高速・低オーバーヘッド
- **デメリット**: 人が読めない（デバッグ時にバイナリをダンプする必要がある）
- **デメリット**: 言語をまたぐ実装が困難（現時点では問題なし）

---

## ADR-005: 設定に TOML + Lua の 2 層構成を採用

### 背景

静的な設定は TOML が最適。しかし動的なステータスバーウィジェットのような処理には
スクリプティング言語が必要。

### 決定

- `config.toml`: デフォルト値・フォント・カラースキームなど静的設定
- `config.lua`: 動的オーバーライド（Lua 5.4 を `mlua` クレートで組み込み）

### ロード順序

```
デフォルト値 → config.toml → config.lua
```

後から読み込んだ値が優先される（上書き）。

---

## ADR-006: TUI フォールバッククライアントの同梱

### 背景

wgpu が使えない環境（コンテナ内、SSH 接続先など）でも使いたい。

### 決定

`nexterm-client-tui` を ratatui + crossterm で実装する。
サーバーとのプロトコルは GPU クライアントと共通（`nexterm-proto`）。

### 機能制限

- 画像プロトコル（Sixel / Kitty）は非対応（`ImagePlaced` を無視）
- スクロールバック・コマンドパレットは未実装（Phase 3 検討中）
- マルチペインは「フォーカスペインのみ表示」に制限

---

## グリッド差分プロトコルの設計

### 問題

毎フレーム全グリッドを送信すると帯域を無駄に消費する。

### 解決策

サーバー側の `Screen` がダーティフラグ（`dirty: Vec<bool>`）を管理する。
PTY 出力を処理するたびに変更行をマークし、`take_dirty_rows()` で差分を取り出す。

```
Client                Server
  │                     │
  │                  PTY 出力 "hello\r\n"
  │                  → Screen.dirty[0] = true
  │<── GridDiff ────────│
  │    dirty_rows=[row0]│
```

クライアントはローカルグリッドに差分をマージするだけでよい。

---

## セキュリティ設計

### Unix ソケット権限

`chmod 0600` でオーナーのみアクセス可能にする。

### Windows Named Pipe

`ServerOptions::new()` のデフォルト設定では作成者のみアクセス可能。

### 認証

現時点では認証機能なし。ローカル通信のみを前提とする。
ネットワーク越し接続は SSH トンネル等を別途設定することを推奨する。

---

## パフォーマンス設計

### PTY 読み取りスレッド

`tokio::task::spawn_blocking` で OS スレッドプールに投入する。
tokio の非同期ランタイムをブロックしない。

### グリッド差分

差分のみ送信することで、アイドル状態のペインはトラフィックがゼロ。

### GPU レンダリング

- グリフアトラス: 初回レンダリング時にラスタライズ済みグリフをテクスチャにキャッシュ
- 頂点バッファ: フレームごとに差分のみ更新（GPU メモリを効率的に使用）
- ポーリング間隔: 16ms（約 60fps）で PTY 出力を確認・再描画

---

---

## ADR-007: マウスサポートの設計

### 背景

winit 0.30 では `ApplicationHandler` トレイトに `window_event()` が必須。
`CursorMoved`・`MouseInput`・`MouseWheel` イベントがここで受け取れる。

### 決定

- **クリックフォーカス**: `CursorMoved` でカーソル位置をキャッシュし、`MouseInput { Left, Released }` でセル座標に変換して `FocusPane { pane_id }` を送信する
- **ホイールスクロール**: `MouseWheel` イベントで `scroll_up` / `scroll_down` を呼び出す（3 行単位）
- セル座標変換式: `pane_id = layout.iter().find(|p| point_in_pane(cursor, p))`

### トレードオフ

- **メリット**: サーバーから `LayoutChanged` で受け取ったペイン矩形情報を使うため、サーバー・クライアント間の同期が不要
- **デメリット**: 境界線ピクセルをクリックした場合の挙動が未定義（現時点は無視）

---

## ADR-008: クリップボード統合に arboard を採用

### 背景

クロスプラットフォームのクリップボード操作を提供するクレートとして、`arboard`（Arboard Clipboard）と `clipboard`（古いクレート）がある。

### 決定

`arboard = "3"` を採用。

- **メリット**: Windows（OLE）・macOS（NSPasteboard）・Linux（X11/Wayland）の 3 OS に対応
- **メリット**: 画像（RGBA）とテキストの両方をサポート
- **デメリット**: `arboard::Clipboard::new()` はメインスレッド必須（一部 OS）。使用のたびに生成する

### 実現方法

- `Ctrl+Shift+V`: `arboard::Clipboard::new()?.get_text()` → `ClientToServer::PasteText { text }` を送信
- `Ctrl+Shift+C`: フォーカスペインのグリッドを `grid_to_text()` でプレーンテキスト変換 → `arboard::Clipboard::new()?.set_text()`

---

## ADR-009: nexterm-ctl を独立クレートとして実装

### 背景

`tmux` の `tmux list-sessions` / `tmux kill-session` に相当するセッション管理 CLI が必要。
GPU クライアントや TUI クライアントを起動せずに操作できることが要件。

### 決定

`nexterm-ctl` を独立した `[[bin]]` クレートとして実装。

- IPC 接続は `nexterm-proto` の型を再利用
- トランスポートは GPU/TUI クライアントと同一の Named Pipe / Unix Socket
- サブコマンド: `list` / `new` / `attach` / `kill`（`clap derive` で実装）

### `attach` サブコマンドについて

`nexterm-ctl` 自体はインタラクティブな端末入出力を行わないため、`attach` はアタッチ方法の案内メッセージを表示するのみとした。実際のアタッチは `nexterm-client-gpu` または `nexterm-client-tui` が担う。

---

## ADR-010: 設定ホットリロードに notify クレートを採用

### 背景

設定ファイルの変更をポーリングで検知するとレイテンシが高く CPU を浪費する。

### 決定

`notify = "6"` を採用し、OS のネイティブファイル監視 API を使用する。

- Linux: `inotify`
- macOS: `kqueue` / FSEvents
- Windows: `ReadDirectoryChangesW`

### 実現方法

`watch_config(tx: Sender<Config>)` 関数が `RecommendedWatcher` を生成し、設定ファイル変更を検知したら新しい `Config` を送信する。GPU クライアントは `about_to_wait` フックで `config_rx.try_recv()` をポーリングし、新設定を受け取った場合は適用する。

フォントファミリー・サイズが変わった場合のみグリフアトラスを再生成する（重い処理のため差分チェックが必要）。

---

## ADR-011: Lua ステータスバーの評価方式

### 背景

ステータスバーウィジェットを Lua 式で定義すると、`os.date()` のような時刻を返す式が書ける。しかし Lua の評価はメインスレッドで行う必要がある（`mlua::Lua` が `!Send`）。

### 決定

`StatusBarEvaluator` を `nexterm-config` クレートに実装し、`EventHandler`（winit メインスレッド）に保持させる。評価は `about_to_wait` フック内で **1 秒ごと** に行い、結果を `status_bar_text: String` にキャッシュする。

### トレードオフ

- **メリット**: Lua 評価をメインスレッドに閉じ込めることで `Send` 制約の問題を回避
- **デメリット**: 重い Lua 処理があるとフレームレートに影響する（ユーザー設定の責任）
- **デメリット**: 1 秒未満の精度が必要なウィジェット（ミリ秒カウンター等）は不向き

---

## 今後の設計課題

| 課題 | 優先度 | 概要 |
|------|--------|------|
| セッション永続化 | 中 | サーバー再起動後にセッション情報を復元する |
| ペイン境界ドラッグ | 低 | マウスドラッグで BSP ツリーの比率を変更する |
| TUI クライアントのスクロールバック | 低 | ratatui クライアントでもスクロールバック操作を追加する |
