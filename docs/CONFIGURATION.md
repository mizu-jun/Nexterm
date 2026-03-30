# nexterm 設定リファレンス

## 設定ファイルの場所

| OS | TOML パス | Lua パス |
|----|-----------|---------|
| Linux | `~/.config/nexterm/nexterm.toml` | `~/.config/nexterm/nexterm.lua` |
| macOS | `~/Library/Application Support/nexterm/nexterm.toml` | `~/Library/Application Support/nexterm/nexterm.lua` |
| Windows | `%APPDATA%\nexterm\nexterm.toml` | `%APPDATA%\nexterm\nexterm.lua` |

環境変数 `XDG_CONFIG_HOME` が設定されている場合は `$XDG_CONFIG_HOME/nexterm/` が優先される（Linux のみ）。

---

## ロード順序

```
1. ビルトインデフォルト値
2. nexterm.toml  （存在する場合）
3. nexterm.lua   （存在する場合）
```

後から読み込んだ値が優先される。TOML で設定した値を Lua で上書きすることもできる。

---

## nexterm.toml リファレンス

### `[font]` — フォント設定

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `family` | String | `"monospace"` | フォントファミリー名 |
| `size` | float | `14.0` | フォントサイズ（pt） |
| `ligatures` | bool | `true` | プログラミングリガチャを有効にする |
| `font_fallbacks` | String[] | `[]` | グリフが見つからない場合に順番に試行するフォールバックフォントのリスト |

```toml
[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji", "Symbols Nerd Font"]
```

### `[colors]` — カラースキーム

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `scheme` | String | `"dark"` | 使用するカラースキーム名 |

#### 組み込みスキーム

| 値 | 説明 |
|----|------|
| `"dark"` | デフォルトダーク |
| `"light"` | ライト |
| `"tokyonight"` | Tokyo Night |
| `"solarized"` | Solarized Dark |
| `"gruvbox"` | Gruvbox Dark |

```toml
[colors]
scheme = "tokyonight"
```

### `[shell]` — シェル設定

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `program` | String | OS 依存 | シェルプログラムのフルパス |
| `args` | String[] | `[]` | シェルに渡す引数 |

OS ごとのデフォルト値：
- **Windows**: `C:\Program Files\PowerShell\7\pwsh.exe`（なければ `powershell.exe`）
- **Linux / macOS**: `$SHELL` 環境変数（なければ `/bin/sh`）

```toml
[shell]
program = "/usr/bin/fish"
args = []
```

### `scrollback_lines` — スクロールバック行数

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `scrollback_lines` | usize | `50000` | スクロールバックバッファの最大行数 |

`scrollback_lines` はトップレベルキーとして記述する（セクション不要）。

```toml
scrollback_lines = 10000
```

### `[status_bar]` — ステータスバー

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `enabled` | bool | `false` | ステータスバーを表示する |
| `widgets` | String[] | `[]` | ステータスバーに表示する Lua 式のリスト |

`widgets` の各要素は **Lua 式の文字列** として評価される。評価結果は `String` 型として連結され、スペース 2 個で区切られて右端に表示される。

#### ウィジェット式の例

| Lua 式 | 出力例 | 説明 |
|--------|--------|------|
| `'os.date("%H:%M:%S")'` | `14:23:01` | 現在時刻（秒まで） |
| `'os.date("%Y-%m-%d")'` | `2026-03-26` | 現在日付 |
| `'"nexterm"'` | `nexterm` | 固定文字列（外側は TOML 文字列、内側は Lua 文字列リテラル） |
| `'tostring(math.pi):sub(1,6)'` | `3.1415` | 任意の Lua 式 |

> **注意**: TOML 内で Lua 文字列リテラルを書く場合、二重引用符が TOML と Lua で競合するため、TOML 側をシングルクォート文字列にすることを推奨する。

```toml
[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']
```

評価は **1 秒ごと** に行われる（GPU クライアントの `about_to_wait` フック内）。

### `[window]` — ウィンドウ外観

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `background_opacity` | float | `1.0` | ウィンドウ背景の不透明度（0.0 = 完全透明、1.0 = 不透明）。透過を有効にする場合はコンポジタが必要 |
| `macos_window_background_blur` | u32 | `0` | macOS のウィンドウぼかし強度（0 = 無効） |
| `decorations` | String | `"full"` | ウィンドウ装飾の種別 |

#### `decorations` の値

| 値 | 説明 |
|----|------|
| `"full"` | OS 標準のタイトルバーと境界線を表示 |
| `"none"` | タイトルバーと境界線を非表示（ボーダーレス） |
| `"notitle"` | タイトルバーのみ非表示 |

```toml
[window]
background_opacity = 0.92
macos_window_background_blur = 20
decorations = "notitle"
```

### `[terminal]` — ターミナル機能設定

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `alt_screen_buffer` | bool | `true` | 代替スクリーンバッファ対応（SMCUP/RMCUP） |
| `dec_mode_47_1047_1049` | bool | `true` | DEC Private Mode 47/1047/1049 対応 |
| `osc_window_title` | bool | `true` | OSC 0/1/2 ウィンドウタイトル対応 |
| `osc_notifications` | bool | `true` | OSC 9 デスクトップ通知対応 |
| `cjk_width` | bool | `true` | CJK 文字幅の正確な計算 |
| `ime_support` | bool | `true` | IME（入力メソッドエディタ）対応 |

```toml
[terminal]
alt_screen_buffer = true
dec_mode_47_1047_1049 = true
osc_window_title = true
osc_notifications = true
cjk_width = true
ime_support = true
```

代替スクリーンバッファは `less`, `vim`, `htop` などのアプリケーションが画面をクリアして表示を切り替える際に使用されます。

### `[tab_bar]` — タブバー（WezTerm スタイル）

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `enabled` | bool | `true` | タブバーを表示する |
| `height` | u32 | `28` | タブバーの高さ（ピクセル） |
| `active_tab_bg` | String | `"#ae8b2d"` | アクティブタブの背景色（`#rrggbb` 形式） |
| `inactive_tab_bg` | String | `"#5c6d74"` | 非アクティブタブの背景色（`#rrggbb` 形式） |
| `separator` | String | `"❯"` | タブ間のセパレータ文字 |

```toml
[tab_bar]
enabled = true
height = 28
active_tab_bg = "#ae8b2d"
inactive_tab_bg = "#5c6d74"
separator = "❯"
```

### `[[keys]]` — キーバインド

カスタムキーバインドを配列で定義する。デフォルトバインドを上書きしたい場合に使用する。

| キー | 型 | 説明 |
|-----|----|------|
| `key` | String | キー文字列（例: `"ctrl+shift+p"`） |
| `action` | String | アクション名またはカスタム Lua コード |
| `command` | String | （オプション）実行コマンド |

#### デフォルトアクション一覧

| アクション | 説明 |
|-----------|------|
| `SplitVertical` | フォーカスペインを左右に分割 |
| `SplitHorizontal` | フォーカスペインを上下に分割 |
| `FocusNextPane` | 次のペインにフォーカス移動 |
| `FocusPrevPane` | 前のペインにフォーカス移動 |
| `Detach` | セッションからデタッチ |
| `SearchScrollback` | スクロールバック検索を開始 |
| `DisplayPanes` | ペイン番号表示モード（ナビゲーション用） |
| `ClosePane` | フォーカスペインを閉じる |
| `NewWindow` | 新しいウィンドウを作成 |
| `ToggleZoom` | フォーカスペインをズーム/通常表示 |
| `SwapPaneNext` | フォーカスペインを次の兄弟と入れ替え |
| `SwapPanePrev` | フォーカスペインを前の兄弟と入れ替え |
| `BreakPane` | フォーカスペインを新ウィンドウに切り出し |
| `ShowHostManager` | SSH ホストマネージャを開く |
| `ShowMacroPicker` | Lua マクロピッカーを開く |
| `SftpUploadDialog` | SFTP アップロードダイアログを開く |
| `SftpDownloadDialog` | SFTP ダウンロードダイアログを開く |
| `ConnectSerialPrompt` | シリアルポート接続ダイアログを開く |
| `QuickSelect` | Quick Select モード（URL・パス・IP・ハッシュ） |

#### カスタムキーバインド例

```toml
# 標準アクション
[[keys]]
key = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key = "ctrl+shift+-"
action = "SplitHorizontal"

[[keys]]
key = "ctrl+shift+p"
action = "CommandPalette"

# カスタムコマンド実行
[[keys]]
key = "ctrl+alt+t"
command = "echo 'Hello from nexterm' | figlet"
```

#### 右クリックコンテキストメニュー

GPU クライアント内で右クリックするとコンテキストメニューが表示されます：

- **Copy** — フォーカスペイン全体をコピー
- **Paste** — クリップボード内容をペースト
- **Split Vertical** — ペインを左右に分割
- **Split Horizontal** — ペインを上下に分割
- **Close Pane** — ペインを閉じる
- **Display Panes** — ペイン番号表示モードを開始

#### ペイン番号表示モード（Display Panes）

`Display Panes` または `Ctrl+G` でペイン番号オーバーレイが表示されます。
表示されたペイン番号を入力するか、矢印キーで選択してペイン間を移動できます。

### `[[hosts]]` — SSH ホスト登録

SSH 接続先を事前登録する。コマンドパレットから選択して接続できる。

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `name` | String | — | 表示名（必須） |
| `host` | String | — | ホスト名または IP アドレス（必須） |
| `port` | u16 | `22` | SSH ポート番号 |
| `username` | String | — | ユーザー名（必須） |
| `auth_type` | String | `"key"` | 認証方式: `"password"`, `"key"`, `"agent"` |
| `key_path` | String | — | 秘密鍵ファイルパス（`auth_type = "key"` の場合） |
| `proxy_jump` | String | — | ProxyJump ホスト名（多段接続） |
| `socks5_proxy` | String | — | SOCKS5 プロキシアドレス（`host:port` 形式） |
| `local_forwards` | Table[] | — | ローカルポートフォワーディング設定 |
| `forward_remote` | Table[] | — | リモートポートフォワーディング設定 (`-R`) |

#### SSH 認証方式

- `"password"` — パスワード認証（OS キーチェーンに安全保存）
- `"key"` — 公開鍵認証（秘密鍵ファイル指定）
- `"agent"` — SSH エージェント認証（SSH_AUTH_SOCK を使用）

#### ローカルポートフォワーディング

ローカルポート → リモートホスト:ポート のマッピング。

```toml
[[hosts.local_forwards]]
local_port = 8080
remote_host = "localhost"
remote_port = 3000
```

#### SSH ホスト設定例

```toml
# 公開鍵認証
[[hosts]]
name = "本番サーバー"
host = "192.168.1.100"
port = 22
username = "deploy"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

# パスワード認証
[[hosts]]
name = "開発サーバー"
host = "dev.example.com"
port = 2222
username = "ubuntu"
auth_type = "password"
# パスワードは OS キーチェーンに保存される

# SSH エージェント認証
[[hosts]]
name = "ステージング"
host = "staging.example.com"
port = 22
username = "app"
auth_type = "agent"

# ProxyJump 経由の接続
[[hosts]]
name = "内部サーバー"
host = "internal.company.local"
port = 22
username = "admin"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"
proxy_jump = "bastion.company.com"

# SOCKS5 プロキシ経由
[[hosts]]
name = "リモートサーバー"
host = "remote.example.com"
port = 22
username = "user"
auth_type = "key"
key_path = "~/.ssh/id_rsa"
socks5_proxy = "proxy.example.com:1080"

# ローカルポートフォワーディング付き
[[hosts]]
name = "DB サーバー"
host = "db.internal"
port = 22
username = "dbadmin"
auth_type = "key"
key_path = "~/.ssh/db_key"

[[hosts.local_forwards]]
local_port = 5432
remote_host = "localhost"
remote_port = 5432
```

#### リモートポートフォワーディング (`-R`)

SSH サーバー側のポートをローカルに転送する（`ssh -R` 相当）。

```toml
[[hosts]]
name = "リモートフォワード例"
host = "example.com"
port = 22
username = "user"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

[[hosts.forward_remote]]
remote_port = 9090
local_host  = "localhost"
local_port  = 9090
```

#### 既知ホスト検証

SSH 接続時に `~/.ssh/known_hosts` のホスト鍵を検証します。未知のホストに接続する場合は、システムプロンプトで確認を求めます。

#### SSH エージェント認証

`auth_type = "agent"` の場合、`SSH_AUTH_SOCK` 環境変数で指定されたソケットを通じてシステムの SSH エージェントを使用します。

---

### `[[macros]]` — Lua マクロ定義

コマンドパレットから呼び出せる Lua マクロを定義する。
`Ctrl+Shift+M` で開くマクロピッカーに一覧表示され、Enter キーで実行される。
マクロ関数の戻り値（文字列）はフォーカスペインの PTY に送信される。

| キー | 型 | 説明 |
|-----|----|------|
| `name` | String | 表示名（必須）。ピッカーで fuzzy 検索される |
| `description` | String | 説明文（省略可。省略時は `lua_fn` を表示） |
| `lua_fn` | String | 実行する Lua グローバル関数名（必須） |

```toml
[[macros]]
name = "top"
description = "フォーカスペインで top を実行"
lua_fn = "macro_top"

[[macros]]
name = "git status"
description = "カレントディレクトリの git status を表示"
lua_fn = "macro_git_status"

[[macros]]
name = "docker ps"
description = "起動中のコンテナ一覧"
lua_fn = "macro_docker_ps"
```

対応する Lua 関数を `nexterm.lua` に定義する：

```lua
-- ~/.config/nexterm/nexterm.lua

-- シグネチャ: function(session: string, pane_id: number) -> string
function macro_top(session, pane_id)
    return "top\n"   -- PTY に送信するテキスト
end

function macro_git_status(session, pane_id)
    return "git status\n"
end

function macro_docker_ps(session, pane_id)
    return "docker ps\n"
end
```

> マクロ関数は `nexterm-lua-hooks` スレッドで同期実行される。500ms のタイムアウトが設定されており、それを超えた場合は実行をキャンセルして `None` を返す。

---

### `[[serial]]` — シリアルポート接続

コマンドパレットの `ConnectSerial` で使用するシリアルポート設定は
接続ダイアログで直接入力するか、プロトコル経由で指定する。

```
ConnectSerial { path: "/dev/ttyUSB0", baud: 115200 }
```

コマンドパレットから `Connect Serial` を選択すると、ポートとボーレートの入力プロンプトが表示される。

---

### `[log]` — ログ設定

PTY 出力のログ記録設定。

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `auto_log` | bool | `false` | セッション開始時に自動的にログを開始する |
| `log_dir` | String | — | ログファイルの保存ディレクトリ |
| `timestamp` | bool | `false` | 各行の先頭に `[HH:MM:SS]` タイムスタンプを付与する |
| `strip_ansi` | bool | `false` | ログファイルから ANSI エスケープシーケンスを除去する |
| `max_log_size` | u64 | `104857600` | ログファイルの最大サイズ（バイト、デフォルト 100MB） |
| `log_template` | String | — | ログファイル名テンプレート（`{session}`, `{date}`, `{time}` 使用可） |
| `binary` | bool | `false` | バイナリ PTY ログモード — 生バイト列をテキストログと並行して記録する |

#### ログローテーション

ログファイルがサイズ制限に達するとローテーションが自動実行されます。既存ファイルは `.1`, `.2`, ... とリネームされ、新しいログは新規ファイルに書き込まれます。

```toml
[log]
auto_log = true
log_dir = "~/nexterm-logs"
timestamp = true
strip_ansi = true
max_log_size = 52428800    # 50MB
log_template = "{session}_{date}_{time}.log"   # 例: main_2026-03-30_14-23-01.log
binary = false
```

#### ログファイル名テンプレート

`log_template` では以下のプレースホルダーを使用できる：

| プレースホルダー | 展開値 | 例 |
|--------------|-------|-----|
| `{session}` | セッション名 | `main` |
| `{date}` | 日付 `YYYY-MM-DD` | `2026-03-30` |
| `{time}` | 時刻 `HH-MM-SS` | `14-23-01` |

```toml
# 例: "work_2026-03-30_14-23-01.log"
log_template = "{session}_{date}_{time}.log"
```

#### asciinema v2 形式での録画

`nexterm-ctl record start-cast` / `nexterm-ctl record stop-cast` で asciinema 互換形式での録画ができます。

```bash
nexterm-ctl record start-cast <session> <output.cast>
nexterm-ctl record stop-cast <session>
```

asciinema ツールで再生可能：

```bash
asciinema play output.cast
```

---

### `[colors.custom]` — カスタムカラーパレット

`scheme = "custom"` のときに使用するカスタム 16 色パレット。

| キー | 型 | 説明 |
|-----|----|------|
| `foreground` | String | 前景色（`#rrggbb`） |
| `background` | String | 背景色（`#rrggbb`） |
| `cursor` | String | カーソル色（`#rrggbb`） |
| `ansi` | String[16] | ANSI 16 色（黒・赤・緑・黄・青・マゼンタ・シアン・白、各通常+明るい） |

```toml
[colors]
scheme = "custom"

[colors.custom]
foreground = "#cdd6f4"
background = "#1e1e2e"
cursor = "#f5e0dc"
ansi = [
  "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
  "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
  "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af",
  "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
]
```

---

## 完全な nexterm.toml の例

```toml
# スクロールバック行数
scrollback_lines = 10000

[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji"]

[colors]
scheme = "tokyonight"

[shell]
program = "/usr/bin/zsh"
args = []

[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']

[window]
background_opacity = 0.95
macos_window_background_blur = 0
decorations = "full"

[tab_bar]
enabled = true
height = 28
active_tab_bg = "#ae8b2d"
inactive_tab_bg = "#5c6d74"
separator = "❯"

[terminal]
alt_screen_buffer = true
osc_window_title = true
osc_notifications = true
cjk_width = true
ime_support = true

[[keys]]
key = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key = "ctrl+shift+-"
action = "SplitHorizontal"

[[keys]]
key = "ctrl+shift+p"
action = "CommandPalette"

# 公開鍵認証
[[hosts]]
name = "本番サーバー"
host = "192.168.1.100"
port = 22
username = "deploy"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

# SSH エージェント認証
[[hosts]]
name = "ステージング"
host = "staging.example.com"
port = 22
username = "app"
auth_type = "agent"

# ProxyJump 経由
[[hosts]]
name = "内部サーバー"
host = "internal.company.local"
port = 22
username = "admin"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"
proxy_jump = "bastion.company.com"

[log]
auto_log = false
log_dir = "~/nexterm-logs"
timestamp = true
strip_ansi = true
max_log_size = 104857600
```

---

## nexterm.lua リファレンス

Lua スクリプトは TOML の後に適用される動的オーバーライドとして機能する。
スクリプトは設定テーブルを返す必要がある。

### グローバル変数

| 変数 | 型 | 説明 |
|-----|-----|------|
| `nexterm` | table | 現在の設定テーブル（TOML 適用後の値） |

### 戻り値

スクリプトの最後の式として設定テーブルを返す。返さない場合は TOML の設定がそのまま使われる。

### 設定テーブルの構造

```lua
{
  font = {
    family = "string",
    size   = 14.0,        -- float
    ligatures = true,     -- bool
  },
  colors = "string",      -- スキーム名（フラットな文字列）
  shell = {
    program = "string",
  },
  scrollback_lines = 50000,
}
```

### Lua 設定の例

```lua
-- ~/.config/nexterm/nexterm.lua

-- 現在の設定を取得する
local cfg = require("nexterm")

-- フォントサイズを変更する
cfg.font.size = 16.0

-- 高解像度ディスプレイでは大きめにする（将来: DPI 取得 API）
cfg.font.family = "Fira Code"

-- スクロールバックを増やす
cfg.scrollback_lines = 100000

-- カラースキームを変更する
cfg.colors = "gruvbox"

return cfg
```

### Lua イベントフック

`hooks` テーブルにコールバック関数を登録することで、Nexterm のイベントに応じた処理を実行できる。

| フック名 | シグネチャ | 発火タイミング |
|---------|-----------|-------------|
| `hooks.on_session_start` | `function(session: string)` | 新しいセッションが初めて作成されたとき |
| `hooks.on_attach` | `function(session: string)` | クライアントがセッションにアタッチしたとき |
| `hooks.on_detach` | `function(session: string)` | クライアントがセッションからデタッチしたとき |
| `hooks.on_pane_open` | `function(session: string, pane_id: number)` | 新しいペインが作成されたとき |
| `hooks.on_pane_close` | `function(session: string, pane_id: number)` | ペインが閉じられたとき |

```lua
-- ~/.config/nexterm/nexterm.lua

-- セッション開始時にログを記録する
hooks.on_session_start = function(session)
    io.write("[nexterm] session started: " .. session .. "\n")
end

-- アタッチ時に通知を表示する
hooks.on_attach = function(session)
    os.execute('notify-send "nexterm" "attached to ' .. session .. '"')
end

-- 新しいペインが開くたびにカウントを記録する
hooks.on_pane_open = function(session, pane_id)
    io.write(string.format("[nexterm] pane %d opened in %s\n", pane_id, session))
end
```

> **スレッドモデル**: フックは `nexterm-lua-hooks` 専用スレッドで実行される（メインスレッドをブロックしない）。フックが例外を投げた場合はエラーログを出力して次のイベントを処理する。

---

### `require("nexterm")` パターン

`nexterm` モジュールは `package.preload` に登録されており、`require` で読み込める。
これにより設定ファイルをモジュールとして分割できる。

```lua
-- nexterm.lua
local cfg = require("nexterm")

-- 別ファイルに分割する場合
-- local theme = require("my_theme")  -- ただし外部ファイルのロードは未実装
```

---

## 設定の優先順位まとめ

```
高
 │  nexterm.lua の戻り値
 │  nexterm.toml の値
 │  ビルトインデフォルト値
低
```

一部のフィールドのみ設定した場合、残りはデフォルト値が使われる（フィールド単位のマージ）。

---

## 設定が反映されるタイミング

設定ファイルを保存すると **GPU クライアントがファイルシステム変更を自動検知** し、リアルタイムで反映する（ホットリロード）。

| 設定項目 | 反映タイミング | 備考 |
|---------|-------------|------|
| フォント設定 | 即時（ホットリロード） | フォントファミリーまたはサイズ変更時はグリフアトラスを再生成する |
| カラースキーム | 即時（ホットリロード） | 次フレームから適用 |
| スクロールバック行数 | 即時（ホットリロード） | 既存バッファには影響しない |
| シェル設定 | セッション作成時（サーバー側） | 実行中のセッションには影響しない |
| キーバインド | 即時（ホットリロード） | 次キーイベントから適用 |
| ステータスバー設定 | 即時（ホットリロード） | `enabled` 変更は次フレームから反映 |
| Lua ウィジェット式 | 1 秒ごとに再評価 | `nexterm.lua` の変更は次の評価サイクルで反映 |
| ウィンドウ透過・装飾 | 再起動時 | `background_opacity` / `decorations` は起動時にウィンドウ属性として適用される |
| タブバー設定 | 即時（ホットリロード） | `enabled` / 色 / セパレータは次フレームから反映 |

> ホットリロードは `notify` クレートによるファイル監視で実装されている。変更検知から反映まで通常 100ms 以内。
