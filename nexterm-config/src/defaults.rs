//! デフォルト設定ファイルのテンプレート

/// デフォルト nexterm.toml の内容
pub const DEFAULT_TOML: &str = r#"
# nexterm 設定ファイル
# 詳細は https://github.com/mizu-jun/nexterm を参照

[font]
# family = "Cascadia Code"   # Windows 推奨
# family = "JetBrains Mono"  # クロスプラットフォーム推奨
# family = "Fira Code"        # リガチャ対応
family = "monospace"
size = 15.0
ligatures = true
# フォントフォールバックチェーン（グリフが見つからない場合に順番に試行）
# font_fallbacks = ["Noto Sans Mono CJK JP", "Noto Color Emoji"]

[colors]
# 組み込みスキーム: "dark" | "light" | "tokyonight" | "solarized" | "gruvbox"
# カスタムスキームは nexterm.lua で定義可能
scheme = "tokyonight"

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
-- config.font.family = "Cascadia Code"   -- Windows 推奨
-- config.font.family = "JetBrains Mono"  -- クロスプラットフォーム推奨
-- config.font.size = 15.0

-- カラースキームをカスタマイズする例:
-- config.colors = "tokyonight"

-- キーバインドを追加する例:
-- config.keys:add({ key = "ctrl+a", action = "SelectAll" })

return config
"#;
