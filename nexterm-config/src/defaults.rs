//! デフォルト設定ファイルのテンプレート

/// デフォルト nexterm.toml の内容
pub const DEFAULT_TOML: &str = r#"
# nexterm 設定ファイル
# 詳細は https://github.com/kusanagi-jn/nexterm を参照

[font]
family = "monospace"
size = 14.0
ligatures = true

[colors]
# 組み込みスキーム: "dark" | "light" | "tokyonight" | "solarized" | "gruvbox"
# カスタムスキームは nexterm.lua で定義可能
scheme = "dark"

[shell]
# program = "/bin/zsh"   # デフォルトは $SHELL 環境変数
# args = []

scrollback_lines = 50000
"#;

/// デフォルト nexterm.lua のスターターテンプレート
pub const DEFAULT_LUA: &str = r#"
-- nexterm.lua — 高度設定（TOML の値を上書き可能）
-- このファイルは nexterm.toml より後にロードされます

local config = require("nexterm")

-- フォントをカスタマイズする例:
-- config.font.family = "JetBrains Mono"
-- config.font.size = 15.0

-- カラースキームをカスタマイズする例:
-- config.colors = "tokyonight"

-- キーバインドを追加する例:
-- config.keys:add({ key = "ctrl+a", action = "SelectAll" })

return config
"#;
