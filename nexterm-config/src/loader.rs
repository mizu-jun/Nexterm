//! 設定ローダー — TOML → Lua の2層ロードを実装する

use std::path::PathBuf;

use anyhow::{Context, Result};
use mlua::prelude::*;
use tracing::{info, warn};

use crate::schema::{ColorScheme, Config};

/// 設定ディレクトリのパスを返す
pub fn config_dir() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nexterm")
}

/// TOML 設定ファイルのパスを返す
pub fn toml_path() -> PathBuf {
    config_dir().join("nexterm.toml")
}

/// Lua 設定ファイルのパスを返す
pub fn lua_path() -> PathBuf {
    config_dir().join("nexterm.lua")
}

/// LuaError を anyhow::Error に変換するヘルパー
fn lua_err(e: LuaError) -> anyhow::Error {
    anyhow::anyhow!("Lua エラー: {}", e)
}

/// 設定ローダー
pub struct ConfigLoader;

impl ConfigLoader {
    /// 設定を読み込む（TOML → Lua の順）
    ///
    /// 1. ビルトインデフォルト値から開始
    /// 2. nexterm.toml が存在すれば読み込んでマージ
    /// 3. nexterm.lua が存在すれば実行してマージ
    pub fn load() -> Result<Config> {
        let mut config = Config::default();

        // Step 1: TOML を読み込む（Config を直接 deserialize する）
        let toml_path = toml_path();
        if toml_path.exists() {
            match Self::load_toml(&toml_path) {
                Ok(loaded) => {
                    config = loaded;
                    info!("TOML 設定を読み込みました: {}", toml_path.display());
                }
                Err(e) => {
                    let msg = format!("TOML 設定の読み込みに失敗しました: {}", e);
                    warn!("{}", msg);
                    config.config_errors.push(msg);
                }
            }
        } else {
            // 初回起動: デフォルト設定ファイルを生成する
            if let Err(e) = Self::write_default_config(&toml_path) {
                warn!("デフォルト設定ファイルの生成に失敗しました: {}", e);
            } else {
                info!(
                    "デフォルト設定ファイルを生成しました: {}",
                    toml_path.display()
                );
            }
        }

        // Step 2: Lua を実行してマージ
        let lua_path = lua_path();
        if lua_path.exists() {
            match Self::apply_lua(&mut config, &lua_path) {
                Ok(()) => {
                    info!("Lua 設定を適用しました: {}", lua_path.display());
                }
                Err(e) => {
                    let msg = format!("Lua 設定エラー ({}): {}", lua_path.display(), e);
                    warn!("{}", msg);
                    // クライアントへ通知するためにエラーを収集する
                    config.config_errors.push(msg);
                }
            }
        }

        Ok(config)
    }

    /// デフォルト設定ファイルを書き出す（初回起動時のみ呼ばれる）
    fn write_default_config(path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, DEFAULT_CONFIG_TOML)?;
        Ok(())
    }

    /// TOML ファイルを `Config` に直接 deserialize する。
    ///
    /// `Config` の全フィールドに `#[serde(default)]` が付いているため、
    /// TOML に書かれていないフィールドは `Default::default()` で埋まる。
    fn load_toml(path: &std::path::Path) -> Result<Config> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("TOML ファイル読み込み失敗: {}", path.display()))?;
        let parsed: Config = toml::from_str(&content)
            .with_context(|| format!("TOML パース失敗: {}", path.display()))?;
        Ok(parsed)
    }

    /// Lua スクリプトを実行して Config を更新する
    fn apply_lua(config: &mut Config, path: &std::path::Path) -> Result<()> {
        // CRITICAL #4: サンドボックス化された Lua を使用（os/io/package 無効）
        let lua = crate::lua_sandbox::sandboxed_lua()
            .map_err(|e| anyhow::anyhow!("サンドボックス Lua の初期化に失敗: {}", e))?;

        // 現在の設定を Lua テーブルに変換してグローバルに設定
        let config_table = config_to_lua_table(&lua, config)?;
        lua.globals()
            .set("nexterm", config_table.clone())
            .map_err(lua_err)?;

        // package.preload["nexterm"] に登録して require("nexterm") で取得できるようにする
        let preload: LuaTable = lua
            .globals()
            .get::<LuaTable>("package")
            .map_err(lua_err)?
            .get("preload")
            .map_err(lua_err)?;
        let tbl = config_table.clone();
        preload
            .set(
                "nexterm",
                lua.create_function(move |_, ()| Ok(tbl.clone()))
                    .map_err(lua_err)?,
            )
            .map_err(lua_err)?;

        // Lua ファイルを実行する
        let script = std::fs::read_to_string(path)?;
        let result: LuaValue = lua.load(&script).eval().map_err(lua_err)?;

        // 戻り値のテーブルを Config にマージする
        if let LuaValue::Table(tbl) = result {
            apply_lua_table_to_config(config, &tbl)?;
        }

        Ok(())
    }
}

/// カラースキーム文字列をパースする（後方互換のため public のまま残す）
pub fn parse_color_scheme(s: &str) -> ColorScheme {
    use crate::schema::BuiltinScheme;
    match s.to_lowercase().as_str() {
        "dark" => ColorScheme::Builtin(BuiltinScheme::Dark),
        "light" => ColorScheme::Builtin(BuiltinScheme::Light),
        "tokyonight" => ColorScheme::Builtin(BuiltinScheme::TokyoNight),
        "solarized" => ColorScheme::Builtin(BuiltinScheme::Solarized),
        "gruvbox" => ColorScheme::Builtin(BuiltinScheme::Gruvbox),
        _other => ColorScheme::Builtin(BuiltinScheme::Dark),
    }
}

/// Config を Lua テーブルに変換する（mlua 0.10 ではライフタイム不要）
fn config_to_lua_table(lua: &Lua, config: &Config) -> Result<LuaTable> {
    let tbl = lua.create_table().map_err(lua_err)?;

    // font テーブル
    let font = lua.create_table().map_err(lua_err)?;
    font.set("family", config.font.family.clone())
        .map_err(lua_err)?;
    font.set("size", config.font.size).map_err(lua_err)?;
    font.set("ligatures", config.font.ligatures)
        .map_err(lua_err)?;
    tbl.set("font", font).map_err(lua_err)?;

    // colors（文字列として渡す）
    let scheme_str = match &config.colors {
        ColorScheme::Builtin(b) => format!("{:?}", b).to_lowercase(),
        ColorScheme::Custom(_) => "custom".to_string(),
    };
    tbl.set("colors", scheme_str).map_err(lua_err)?;

    // shell テーブル
    let shell = lua.create_table().map_err(lua_err)?;
    shell
        .set("program", config.shell.program.clone())
        .map_err(lua_err)?;
    tbl.set("shell", shell).map_err(lua_err)?;

    // scrollback_lines
    tbl.set("scrollback_lines", config.scrollback_lines)
        .map_err(lua_err)?;

    // tab_bar テーブル
    let tab_bar = lua.create_table().map_err(lua_err)?;
    tab_bar
        .set("enabled", config.tab_bar.enabled)
        .map_err(lua_err)?;
    tab_bar
        .set("height", config.tab_bar.height)
        .map_err(lua_err)?;
    tab_bar
        .set("active_tab_bg", config.tab_bar.active_tab_bg.clone())
        .map_err(lua_err)?;
    tab_bar
        .set("inactive_tab_bg", config.tab_bar.inactive_tab_bg.clone())
        .map_err(lua_err)?;
    tab_bar
        .set("separator", config.tab_bar.separator.clone())
        .map_err(lua_err)?;
    tbl.set("tab_bar", tab_bar).map_err(lua_err)?;

    // hooks テーブル（nil = 未設定）
    let hooks = lua.create_table().map_err(lua_err)?;
    hooks
        .set("on_pane_open", config.hooks.on_pane_open.clone())
        .map_err(lua_err)?;
    hooks
        .set("on_pane_close", config.hooks.on_pane_close.clone())
        .map_err(lua_err)?;
    hooks
        .set("on_session_start", config.hooks.on_session_start.clone())
        .map_err(lua_err)?;
    hooks
        .set("on_attach", config.hooks.on_attach.clone())
        .map_err(lua_err)?;
    hooks
        .set("on_detach", config.hooks.on_detach.clone())
        .map_err(lua_err)?;
    tbl.set("hooks", hooks).map_err(lua_err)?;

    Ok(tbl)
}

/// Lua テーブルの値を Config にマージする
pub fn apply_lua_table_to_config(config: &mut Config, tbl: &LuaTable) -> Result<()> {
    // font
    if let Ok(LuaValue::Table(font)) = tbl.get("font") {
        if let Ok(family) = font.get::<String>("family") {
            config.font.family = family;
        }
        if let Ok(size) = font.get::<f32>("size") {
            config.font.size = size;
        }
        if let Ok(ligatures) = font.get::<bool>("ligatures") {
            config.font.ligatures = ligatures;
        }
    }

    // colors
    if let Ok(scheme) = tbl.get::<String>("colors") {
        config.colors = parse_color_scheme(&scheme);
    }

    // shell
    if let Ok(LuaValue::Table(shell)) = tbl.get("shell")
        && let Ok(program) = shell.get::<String>("program")
    {
        config.shell.program = program;
    }

    // scrollback_lines
    if let Ok(lines) = tbl.get::<usize>("scrollback_lines") {
        config.scrollback_lines = lines;
    }

    // tab_bar
    if let Ok(LuaValue::Table(tab_bar)) = tbl.get("tab_bar") {
        if let Ok(enabled) = tab_bar.get::<bool>("enabled") {
            config.tab_bar.enabled = enabled;
        }
        if let Ok(height) = tab_bar.get::<u32>("height") {
            config.tab_bar.height = height;
        }
        if let Ok(active_tab_bg) = tab_bar.get::<String>("active_tab_bg") {
            config.tab_bar.active_tab_bg = active_tab_bg;
        }
        if let Ok(inactive_tab_bg) = tab_bar.get::<String>("inactive_tab_bg") {
            config.tab_bar.inactive_tab_bg = inactive_tab_bg;
        }
        if let Ok(separator) = tab_bar.get::<String>("separator") {
            config.tab_bar.separator = separator;
        }
    }

    // hooks
    if let Ok(LuaValue::Table(hooks)) = tbl.get("hooks") {
        config.hooks.on_pane_open = hooks.get::<Option<String>>("on_pane_open").ok().flatten();
        config.hooks.on_pane_close = hooks.get::<Option<String>>("on_pane_close").ok().flatten();
        config.hooks.on_session_start = hooks
            .get::<Option<String>>("on_session_start")
            .ok()
            .flatten();
        config.hooks.on_attach = hooks.get::<Option<String>>("on_attach").ok().flatten();
        config.hooks.on_detach = hooks.get::<Option<String>>("on_detach").ok().flatten();
    }

    Ok(())
}

// 設定ディレクトリの解決（標準ライブラリのみで実装）
mod dirs_next {
    pub fn config_dir() -> Option<std::path::PathBuf> {
        #[cfg(windows)]
        {
            std::env::var("APPDATA").ok().map(std::path::PathBuf::from)
        }
        #[cfg(target_os = "macos")]
        {
            std::env::var("HOME").ok().map(|h| {
                std::path::PathBuf::from(h)
                    .join("Library")
                    .join("Application Support")
            })
        }
        #[cfg(all(not(windows), not(target_os = "macos")))]
        {
            std::env::var("XDG_CONFIG_HOME")
                .ok()
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| std::path::PathBuf::from(h).join(".config"))
                })
        }
    }
}

/// 初回起動時に生成するデフォルト設定テンプレート
///
/// **注意**: 実装の `Config` 構造体と一致するキー名を使用する。
/// 過去のテンプレートにあった `[color_scheme] builtin = ...` /
/// `[tab_bar] show = ...` / `[status_bar] show = ...` は
/// 実装と一致しないキー名でサイレント無視されていたため修正済み。
const DEFAULT_CONFIG_TOML: &str = r#"# Nexterm configuration file
# Documentation: https://github.com/mizu-jun/Nexterm
# This file was auto-generated on first launch. Edit freely.

# Number of scrollback lines to retain per pane
scrollback_lines = 10000

# Display language: "auto" (OS detect) or "en" / "ja" / "fr" / "de" / "es" / "it" / "zh-CN" / "ko"
language = "auto"

# Cursor style: "block" / "beam" / "underline"
cursor_style = "block"

# Check GitHub Releases for new versions on startup (default: true)
auto_check_update = true

[font]
# Font family name (use a monospace/nerd font for best results)
family = "monospace"
size = 14.0
ligatures = true
# font_fallbacks = ["Noto Color Emoji"]

# Built-in color schemes: "dark", "light", "tokyonight", "solarized", "gruvbox"
# 文字列で指定するか [colors] scheme = "..." の形式も可
colors = "tokyonight"

# [shell]
# Override the default shell. Leave commented to use the OS default.
# Windows: "C:\\Program Files\\PowerShell\\7\\pwsh.exe"
# macOS/Linux: auto-detected from $SHELL
# program = "/bin/bash"
# args = ["-NoLogo"]

[tab_bar]
enabled = true
height = 28

[status_bar]
enabled = true

# [window]
# background_opacity = 0.92
# macos_window_background_blur = 20
# decorations = "default"

# [[hosts]]
# name = "production"
# host = "192.168.1.100"
# port = 22
# username = "ops"
# auth_type = "key"

# [hooks]
# on_pane_open  = "/path/to/script"
# on_pane_close = "/path/to/script"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn デフォルトロードが成功する() {
        let config = ConfigLoader::load().unwrap();
        assert!(!config.shell.program.is_empty());
    }

    #[test]
    fn toml文字列から設定をパースできる() {
        let toml_str = r#"
scrollback_lines = 10000

[font]
family = "JetBrains Mono"
size = 16.0
ligatures = false
"#;
        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.font.family, "JetBrains Mono");
        assert_eq!(parsed.font.size, 16.0);
        assert!(!parsed.font.ligatures);
        assert_eq!(parsed.scrollback_lines, 10000);
    }

    #[test]
    fn config_に_hosts_セクションを書ける() {
        // 以前 TomlConfig が hosts を持たず、ユーザー設定がサイレント無視されていた問題の回帰テスト
        let toml_str = r#"
[[hosts]]
name = "production"
host = "192.168.1.100"
port = 2222
username = "ops"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"
"#;
        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.hosts.len(), 1);
        assert_eq!(parsed.hosts[0].name, "production");
        assert_eq!(parsed.hosts[0].port, 2222);
        assert_eq!(parsed.hosts[0].username, "ops");
    }

    #[test]
    fn config_に_window_セクションを書ける() {
        // 以前 TomlConfig が window を持たず、ユーザー設定がサイレント無視されていた問題の回帰テスト
        let toml_str = r#"
[window]
background_opacity = 0.85
padding_x = 8
padding_y = 4
"#;
        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.window.background_opacity, 0.85);
        assert_eq!(parsed.window.padding_x, 8);
        assert_eq!(parsed.window.padding_y, 4);
    }

    #[test]
    fn config_に_macros_セクションを書ける() {
        let toml_str = r#"
[[macros]]
name = "git-status"
description = "Show git status"
lua_fn = "macro_git_status"
"#;
        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.macros.len(), 1);
        assert_eq!(parsed.macros[0].name, "git-status");
    }

    #[test]
    fn config_に_cursor_style_と_auto_check_update_を書ける() {
        let toml_str = r#"
cursor_style = "beam"
auto_check_update = false
language = "ja"
"#;
        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            parsed.cursor_style,
            crate::schema::CursorStyle::Beam
        ));
        assert!(!parsed.auto_check_update);
        assert_eq!(parsed.language, "ja");
    }

    #[test]
    fn colors_を文字列でも_scheme_テーブルでも_カスタムでも書ける() {
        use crate::schema::BuiltinScheme;

        // 形式 1: 文字列
        let parsed: Config = toml::from_str("colors = \"gruvbox\"").unwrap();
        assert!(matches!(
            parsed.colors,
            ColorScheme::Builtin(BuiltinScheme::Gruvbox)
        ));

        // 形式 2: [colors] scheme = "..."
        let parsed: Config = toml::from_str("[colors]\nscheme = \"solarized\"").unwrap();
        assert!(matches!(
            parsed.colors,
            ColorScheme::Builtin(BuiltinScheme::Solarized)
        ));

        // 形式 3: フルカスタムパレット
        let custom_toml = r##"
[colors]
foreground = "#cdd6f4"
background = "#1e1e2e"
cursor = "#f5e0dc"
ansi = ["#000000", "#ff0000", "#00ff00", "#ffff00",
        "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
        "#808080", "#ff8080", "#80ff80", "#ffff80",
        "#8080ff", "#ff80ff", "#80ffff", "#ffffff"]
"##;
        let parsed: Config = toml::from_str(custom_toml).unwrap();
        match parsed.colors {
            ColorScheme::Custom(p) => {
                assert_eq!(p.foreground, "#cdd6f4");
                assert_eq!(p.ansi.len(), 16);
            }
            _ => panic!("Custom パレットがパースされなかった"),
        }
    }

    #[test]
    fn デフォルトテンプレートが_config_として_パース可能() {
        // 初回起動時のテンプレート自体が壊れていないことを確認する
        let parsed: Result<Config> = toml::from_str(DEFAULT_CONFIG_TOML).map_err(Into::into);
        assert!(
            parsed.is_ok(),
            "DEFAULT_CONFIG_TOML が Config としてパースできない: {:?}",
            parsed.err()
        );
        let cfg = parsed.unwrap();
        assert_eq!(cfg.scrollback_lines, 10000);
        assert_eq!(cfg.language, "auto");
        assert!(cfg.tab_bar.enabled);
        assert!(cfg.status_bar.enabled);
        // 旧テンプレートは [color_scheme] builtin = "..." だったがそれが効かない問題の回帰テスト
        assert!(matches!(
            cfg.colors,
            ColorScheme::Builtin(crate::schema::BuiltinScheme::TokyoNight)
        ));
    }

    #[test]
    fn luaで設定を上書きできる() {
        let lua = crate::lua_sandbox::sandboxed_lua().unwrap();
        let mut config = Config::default();

        let tbl = config_to_lua_table(&lua, &config).unwrap();

        // font テーブルを直接変更して apply する
        let font: LuaTable = tbl.get("font").unwrap();
        font.set("size", 20.0f32).unwrap();
        font.set("family", "Hack").unwrap();

        apply_lua_table_to_config(&mut config, &tbl).unwrap();
        assert_eq!(config.font.size, 20.0);
        assert_eq!(config.font.family, "Hack");
    }

    #[test]
    fn カラースキームのパース() {
        use crate::schema::BuiltinScheme;
        assert!(matches!(
            parse_color_scheme("tokyonight"),
            ColorScheme::Builtin(BuiltinScheme::TokyoNight)
        ));
        // 未知のスキーム名はデフォルト (Dark) にフォールバックする
        assert!(matches!(
            parse_color_scheme("custom_theme"),
            ColorScheme::Builtin(BuiltinScheme::Dark)
        ));
    }
}
