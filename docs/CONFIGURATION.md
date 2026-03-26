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

```toml
[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true
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

---

## 完全な nexterm.toml の例

```toml
# スクロールバック行数
scrollback_lines = 10000

[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true

[colors]
scheme = "tokyonight"

[shell]
program = "/usr/bin/zsh"
args = []

[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']

[[keys]]
key = "ctrl+shift+\\"
action = "SplitVertical"

[[keys]]
key = "ctrl+shift+-"
action = "SplitHorizontal"
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

> ホットリロードは `notify` クレートによるファイル監視で実装されている。変更検知から反映まで通常 100ms 以内。
