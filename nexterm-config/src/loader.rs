//! 設定ローダー — TOML → Lua の2層ロードを実装する

use std::path::PathBuf;

use anyhow::Result;
use mlua::prelude::*;
use tracing::{debug, info, warn};

use crate::schema::{ColorScheme, Config, FontConfig, HooksConfig, KeyBinding, ShellConfig, StatusBarConfig, TabBarConfig};

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

        // Step 1: TOML を読み込む
        let toml_path = toml_path();
        if toml_path.exists() {
            match Self::load_toml(&toml_path) {
                Ok(toml_config) => {
                    config = merge_toml(config, toml_config);
                    info!("TOML 設定を読み込みました: {}", toml_path.display());
                }
                Err(e) => {
                    warn!("TOML 設定の読み込みに失敗しました（デフォルト使用）: {}", e);
                }
            }
        } else {
            debug!(
                "TOML 設定ファイルが見つかりません（デフォルト使用）: {}",
                toml_path.display()
            );
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

    /// TOML ファイルを読み込む
    fn load_toml(path: &std::path::Path) -> Result<TomlConfig> {
        let content = std::fs::read_to_string(path)?;
        let parsed: TomlConfig = toml::from_str(&content)?;
        Ok(parsed)
    }

    /// Lua スクリプトを実行して Config を更新する
    fn apply_lua(config: &mut Config, path: &std::path::Path) -> Result<()> {
        let lua = Lua::new();

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

/// TOML から部分的に読み込む中間構造体（全フィールドが Optional）
#[derive(Debug, serde::Deserialize)]
pub struct TomlConfig {
    pub font: Option<FontConfig>,
    pub colors: Option<TomlColors>,
    pub shell: Option<ShellConfig>,
    pub keys: Option<Vec<KeyBinding>>,
    pub status_bar: Option<StatusBarConfig>,
    pub scrollback_lines: Option<usize>,
    pub tab_bar: Option<TabBarConfig>,
    pub hooks: Option<HooksConfig>,
}

/// TOML の colors セクション
#[derive(Debug, serde::Deserialize)]
pub struct TomlColors {
    pub scheme: Option<String>,
}

/// TOML 設定をデフォルト Config にマージする
pub fn merge_toml(mut base: Config, toml: TomlConfig) -> Config {
    if let Some(font) = toml.font {
        base.font = font;
    }
    if let Some(colors) = toml.colors
        && let Some(scheme) = colors.scheme {
            base.colors = parse_color_scheme(&scheme);
        }
    if let Some(shell) = toml.shell {
        base.shell = shell;
    }
    if let Some(keys) = toml.keys {
        base.keys = keys;
    }
    if let Some(sb) = toml.status_bar {
        base.status_bar = sb;
    }
    if let Some(lines) = toml.scrollback_lines {
        base.scrollback_lines = lines;
    }
    if let Some(tab_bar) = toml.tab_bar {
        base.tab_bar = tab_bar;
    }
    if let Some(hooks) = toml.hooks {
        base.hooks = hooks;
    }
    base
}

/// カラースキーム文字列をパースする
pub fn parse_color_scheme(s: &str) -> ColorScheme {
    use crate::schema::BuiltinScheme;
    match s {
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
    font.set("family", config.font.family.clone()).map_err(lua_err)?;
    font.set("size", config.font.size).map_err(lua_err)?;
    font.set("ligatures", config.font.ligatures).map_err(lua_err)?;
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
    tab_bar.set("enabled", config.tab_bar.enabled).map_err(lua_err)?;
    tab_bar.set("height", config.tab_bar.height).map_err(lua_err)?;
    tab_bar.set("active_tab_bg", config.tab_bar.active_tab_bg.clone()).map_err(lua_err)?;
    tab_bar.set("inactive_tab_bg", config.tab_bar.inactive_tab_bg.clone()).map_err(lua_err)?;
    tab_bar.set("separator", config.tab_bar.separator.clone()).map_err(lua_err)?;
    tbl.set("tab_bar", tab_bar).map_err(lua_err)?;

    // hooks テーブル（nil = 未設定）
    let hooks = lua.create_table().map_err(lua_err)?;
    hooks.set("on_pane_open", config.hooks.on_pane_open.clone()).map_err(lua_err)?;
    hooks.set("on_pane_close", config.hooks.on_pane_close.clone()).map_err(lua_err)?;
    hooks.set("on_session_start", config.hooks.on_session_start.clone()).map_err(lua_err)?;
    hooks.set("on_attach", config.hooks.on_attach.clone()).map_err(lua_err)?;
    hooks.set("on_detach", config.hooks.on_detach.clone()).map_err(lua_err)?;
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
        && let Ok(program) = shell.get::<String>("program") {
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
        config.hooks.on_session_start = hooks.get::<Option<String>>("on_session_start").ok().flatten();
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
            std::env::var("APPDATA")
                .ok()
                .map(std::path::PathBuf::from)
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
        // scrollback_lines はセクション前に書く（TOML ルール）
        let toml_str = r#"
scrollback_lines = 10000

[font]
family = "JetBrains Mono"
size = 16.0
ligatures = false
"#;
        let parsed: TomlConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.font.as_ref().unwrap().family, "JetBrains Mono");
        assert_eq!(parsed.font.as_ref().unwrap().size, 16.0);
        assert!(!parsed.font.as_ref().unwrap().ligatures);
        assert_eq!(parsed.scrollback_lines, Some(10000));
    }

    #[test]
    fn tomlマージが正しく動作する() {
        let base = Config::default();
        let toml = TomlConfig {
            font: Some(FontConfig {
                family: "Fira Code".to_string(),
                size: 13.0,
                ligatures: true,
                font_fallbacks: vec![],
            }),
            colors: None,
            shell: None,
            keys: None,
            status_bar: None,
            scrollback_lines: Some(20000),
            tab_bar: None,
            hooks: None,
        };
        let merged = merge_toml(base, toml);
        assert_eq!(merged.font.family, "Fira Code");
        assert_eq!(merged.scrollback_lines, 20000);
    }

    #[test]
    fn luaで設定を上書きできる() {
        let lua = Lua::new();
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
