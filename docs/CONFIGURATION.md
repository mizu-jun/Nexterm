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
| `action` | String | アクション名 |

#### アクション一覧

| アクション | 説明 |
|-----------|------|
| `SplitVertical` | フォーカスペインを左右に分割 |
| `SplitHorizontal` | フォーカスペインを上下に分割 |
| `FocusNextPane` | 次のペインにフォーカス移動 |
| `FocusPrevPane` | 前のペインにフォーカス移動 |
| `Detach` | セッションからデタッチ |
| `CommandPalette` | コマンドパレットを開く/閉じる |

```toml
[[keys]]
key = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key = "ctrl+shift+-"
action = "SplitHorizontal"

[[keys]]
key = "ctrl+shift+p"
action = "CommandPalette"
```

### `[[hosts]]` — SSH ホスト登録

SSH 接続先を事前登録する。コマンドパレットから選択して接続できる。

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `name` | String | — | 表示名（必須） |
| `host` | String | — | ホスト名または IP アドレス（必須） |
| `port` | u16 | `22` | SSH ポート番号 |
| `username` | String | — | ユーザー名（必須） |
| `auth_type` | String | `"key"` | 認証方式: `"password"` または `"key"` |
| `key_path` | String | — | 秘密鍵ファイルパス（`auth_type = "key"` の場合） |

パスワードは設定ファイルに平文保存せず、OS キーチェーンに安全保存する（`nexterm-config::keyring` 参照）。

```toml
[[hosts]]
name = "本番サーバー"
host = "192.168.1.100"
port = 22
username = "deploy"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

[[hosts]]
name = "開発サーバー"
host = "dev.example.com"
port = 2222
username = "ubuntu"
auth_type = "password"
# パスワードは OS キーチェーンに保存される
```

---

### `[log]` — ログ設定

PTY 出力のログ記録設定。

| キー | 型 | デフォルト | 説明 |
|-----|----|-----------|------|
| `auto_log` | bool | `false` | セッション開始時に自動的にログを開始する |
| `log_dir` | String | — | ログファイルの保存ディレクトリ |
| `timestamp` | bool | `false` | 各行の先頭に `[HH:MM:SS]` タイムスタンプを付与する |
| `strip_ansi` | bool | `false` | ログファイルから ANSI エスケープシーケンスを除去する |

```toml
[log]
auto_log = true
log_dir = "~/nexterm-logs"
timestamp = true
strip_ansi = true
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

[[keys]]
key = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key = "ctrl+shift+-"
action = "SplitHorizontal"

[[hosts]]
name = "my-server"
host = "192.168.1.100"
port = 22
username = "admin"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

[log]
auto_log = false
timestamp = true
strip_ansi = true
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
