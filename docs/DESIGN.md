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

### Unix ドメインソケット

- `chmod 0600` でオーナーのみアクセス可能にする
- 接続受け付け後、`SO_PEERCRED`（Linux）/ `getpeereid()`（macOS/BSD）でクライアントの UID を取得し、サーバーの UID と照合する。UID 不一致の場合は即座に接続を切断する

### Windows Named Pipe

- `ServerOptions::reject_remote_clients(true)` で同一マシン外からの接続を拒否する
- デフォルト DACL により作成者のみアクセス可能

### パストラバーサル防止

`StartRecording { path }` ハンドラーは `validate_recording_path()` で `..` コンポーネントと空パスを事前に拒否する。

### 認証

UID 検証のみで、パスワード等の認証機能はなし。ローカル通信のみを前提とする。
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

## ADR-011: Lua ステータスバーの評価方式（LuaWorker バックグラウンドスレッド）

### 背景

ステータスバーウィジェットを Lua 式で定義すると、`os.date()` のような時刻を返す式が書ける。`mlua::Lua` は `!Send + !Sync` であるため、インスタンスをスレッド間で移動できない。当初はメインスレッドで同期評価する設計だったが、重い Lua 処理があると winit イベントループをブロックしてフレームレートが低下するリスクがあった。

### 決定

`LuaWorker` を `nexterm-config` クレートに実装する。`Lua` インスタンスは専用の OS スレッド（`std::thread::spawn`）内で生成・保持し、メインスレッドからはチャネル経由でリクエストを送る。

```
メインスレッド (winit)
  └── LuaWorker::eval_widgets(&widgets) → Arc<Mutex<String>> からキャッシュを即返却
          │ try_send (SyncChannel)
          ▼
Lua ワーカースレッド (nexterm-lua-worker)
  └── loop { recv() → Lua::eval() → Arc<Mutex<String>>.lock().write() }
```

- `request_tx: SyncSender<LuaRequest>` に `try_send` を使い、チャネルが満杯なら破棄（ブロックしない）
- 前回の評価結果が `cache: Arc<Mutex<String>>` に常に保持されており、`eval_widgets()` は即座に返す

### トレードオフ

- **メリット**: メインスレッドが Lua 評価をブロックしない（フレームレートへの影響ゼロ）
- **メリット**: `Lua` インスタンスのスレッド間移動が不要（`!Send` 制約を自然に回避）
- **デメリット**: 評価結果が最大 1 フレーム遅延する（ステータスバー用途では無問題）
- **デメリット**: Lua 評価が遅い場合、前回の結果が表示され続ける

---

## ADR-012: セッション永続化（JSON スナップショット）

### 背景

サーバーを再起動するとセッション・ウィンドウ・ペイン・BSP レイアウトがすべて失われていた。tmux は `.tmux_resurrect` / `tmux-continuum` プラグインで対応しているが、nexterm はサーバー自体がスナップショットを管理する設計とした。

### 決定

サーバーシャットダウン時に全セッション状態を JSON ファイルに保存し、次回起動時に復元する。

**保存形式**: `serde_json` によるプリティ JSON（human-readable、デバッグが容易）

**保存パス**:
- Linux / macOS: `~/.local/state/nexterm/snapshot.json`
- Windows: `%APPDATA%\nexterm\snapshot.json`

**スナップショット型**:

```rust
ServerSnapshot { version: u32, sessions: Vec<SessionSnapshot>, saved_at: u64 }
SessionSnapshot { name, shell, cols, rows, windows, focused_window_id }
WindowSnapshot  { id, name, focused_pane_id, layout: SplitNodeSnapshot }
SplitNodeSnapshot::Pane   { pane_id, cwd: Option<PathBuf> }
SplitNodeSnapshot::Split  { dir, ratio, left, right }
```

**復元フロー**:

```
起動時:
  persist::load_snapshot()
    → SessionManager::restore_from_snapshot()
        → Session::restore_from_snapshot()
            → Window::restore_from_snapshot()
                → Pane::spawn_with_cwd(id, cols, rows, tx, shell, cwd)
  → set_min_pane_id(max_id + 1)   // ID 衝突防止
  → set_min_window_id(max_id + 1)

終了時:
  SessionManager::to_snapshot()
    → persist::save_snapshot()
```

**作業ディレクトリの復元**:
- Linux: `/proc/{pid}/cwd` シンボリックリンクから子プロセスの cwd を取得
- 他 OS: 復元時は `None`（シェル起動時のデフォルトディレクトリになる）

### トレードオフ

- **メリット**: バイナリ形式でなく JSON のため、バージョン管理・デバッグが容易
- **メリット**: `version` フィールドで互換性チェックができる（不一致時はスキップ）
- **デメリット**: PTY の仮想スクリーン内容（グリッド）は保存しない（シェルを再起動するだけ）
- **デメリット**: Linux 以外ではペインの作業ディレクトリが復元されない

---

## ADR-013: IPC セキュリティ（UID 検証とパストラバーサル防止）

### 背景

Unix ドメインソケットのパーミッション 0600 と Windows Named Pipe のデフォルト DACL はオーナーのみアクセスを制限するが、共有サーバーやコンテナ環境ではソケットファイル自体のパーミッション変更や権限昇格攻撃のリスクがある。また、`StartRecording { path }` の引数にパストラバーサル（`../etc/passwd` 等）が渡された場合、任意のファイルパスに書き込まれるリスクがあった。

### 決定

**UID ピア検証（Unix のみ）**:

接続受け付け後、カーネルの `SO_PEERCRED` / `getpeereid()` でクライアントの UID を取得し、サーバーの `euid` と照合する。不一致の場合は即座に接続を切断する。

| OS | 実装 |
|----|------|
| Linux | `getsockopt(SO_PEERCRED)` → `ucred.uid` |
| macOS / BSD | `libc::getpeereid(fd, &uid, &gid)` |
| その他 Unix | UID 検証をスキップ（警告ログのみ） |

**Windows Named Pipe**: `.reject_remote_clients(true)` で同一マシン外からの接続を拒否する。

**パストラバーサル防止**:

`StartRecording { path }` ハンドラーで `validate_recording_path()` を先行実行。`std::path::Component::ParentDir` (`..`) を含むパスや空パスはエラーで返す。

### トレードオフ

- **メリット**: OS レベルで同一ユーザーのみに制限できる（ファイルパーミッションへの依存を減らす）
- **デメリット**: `SO_PEERCRED` は Linux 限定、`getpeereid` は macOS/BSD 限定のため条件コンパイルが複雑になる
- **デメリット**: `setuid` バイナリや sudo 経由での接続は意図せず拒否される可能性がある

---

## 今後の設計課題

| 課題 | 優先度 | 概要 |
|------|--------|------|
| ペイン境界ドラッグ | 低 | マウスドラッグで BSP ツリーの比率を変更する |
| TUI クライアントのスクロールバック | 低 | ratatui クライアントでもスクロールバック操作を追加する |
| macOS / Windows の cwd 復元 | 低 | `/proc` に頼らない移植可能な作業ディレクトリ取得 |
