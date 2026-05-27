//! Default configuration-file templates.

/// Contents of the default `nexterm.toml`.
pub const DEFAULT_TOML: &str = r#"
# nexterm configuration file.
# See https://github.com/mizu-jun/nexterm for details.

[font]
# family = "Cascadia Code"    # recommended on Windows
# family = "JetBrains Mono"   # recommended cross-platform default
# family = "Fira Code"        # supports ligatures
family = "monospace"
size = 15.0
ligatures = true
# Font fallback chain (each entry is tried in order when a glyph is missing).
# font_fallbacks = ["Noto Sans Mono CJK JP", "Noto Color Emoji"]

[colors]
# Built-in schemes: "dark" | "light" | "tokyonight" | "solarized" | "gruvbox".
# Custom schemes can be defined in `nexterm.lua`.
scheme = "tokyonight"

[shell]
# program = "/bin/zsh"   # defaults to $SHELL.
# args = []

scrollback_lines = 50000
"#;

/// Starter template for `nexterm.lua`.
pub const DEFAULT_LUA: &str = r#"
-- nexterm.lua — advanced configuration (overrides values from the TOML).
-- This file is loaded after `nexterm.toml`.

local config = require("nexterm")

-- Customizing the font, for example:
-- config.font.family = "Cascadia Code"   -- recommended on Windows
-- config.font.family = "JetBrains Mono"  -- recommended cross-platform default
-- config.font.size = 15.0

-- Customizing the color scheme, for example:
-- config.colors = "tokyonight"

-- Adding a key binding, for example:
-- config.keys:add({ key = "ctrl+a", action = "SelectAll" })

return config
"#;
