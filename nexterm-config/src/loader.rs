//! Configuration loader — implements the two-layer TOML → Lua load.

use std::path::PathBuf;

use anyhow::{Context, Result};
use mlua::prelude::*;
use tracing::{info, warn};

use crate::schema::{ColorScheme, Config};

/// Returns the path to the configuration directory.
pub fn config_dir() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nexterm")
}

/// Returns the path to the TOML configuration file.
pub fn toml_path() -> PathBuf {
    config_dir().join("nexterm.toml")
}

/// Returns the path to the Lua configuration file.
pub fn lua_path() -> PathBuf {
    config_dir().join("nexterm.lua")
}

/// Helper that converts a `LuaError` into an `anyhow::Error`.
fn lua_err(e: LuaError) -> anyhow::Error {
    anyhow::anyhow!("Lua error: {}", e)
}

/// Configuration loader.
pub struct ConfigLoader;

impl ConfigLoader {
    /// Loads the configuration (TOML first, Lua second).
    ///
    /// 1. Start from the built-in defaults.
    /// 2. If `nexterm.toml` exists, load and merge it.
    /// 3. If `nexterm.lua` exists, execute and merge it.
    pub fn load() -> Result<Config> {
        let mut config = Config::default();

        // Step 1: read the TOML (deserialize directly into `Config`).
        let toml_path = toml_path();
        if toml_path.exists() {
            match Self::load_toml(&toml_path) {
                Ok(loaded) => {
                    config = loaded;
                    info!("Loaded the TOML configuration: {}", toml_path.display());
                }
                Err(e) => {
                    let msg = format!("Failed to load the TOML configuration: {}", e);
                    warn!("{}", msg);
                    config.config_errors.push(msg);
                }
            }
        } else {
            // First launch: generate the default configuration file.
            if let Err(e) = Self::write_default_config(&toml_path) {
                warn!("Failed to generate the default configuration file: {}", e);
            } else {
                info!(
                    "Generated the default configuration file: {}",
                    toml_path.display()
                );
            }
        }

        // Step 2: execute Lua and merge.
        let lua_path = lua_path();
        if lua_path.exists() {
            match Self::apply_lua(&mut config, &lua_path) {
                Ok(()) => {
                    info!("Applied the Lua configuration: {}", lua_path.display());
                }
                Err(e) => {
                    let msg = format!("Lua configuration error ({}): {}", lua_path.display(), e);
                    warn!("{}", msg);
                    // Collect the error so the client can surface it.
                    config.config_errors.push(msg);
                }
            }
        }

        Ok(config)
    }

    /// Writes the default configuration file (called only on first launch).
    fn write_default_config(path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, DEFAULT_CONFIG_TOML)?;
        Ok(())
    }

    /// Deserializes the TOML file directly into a `Config`.
    ///
    /// Every field of `Config` carries `#[serde(default)]`, so missing fields
    /// are filled in with `Default::default()`.
    fn load_toml(path: &std::path::Path) -> Result<Config> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read the TOML file: {}", path.display()))?;
        let parsed: Config = toml::from_str(&content)
            .with_context(|| format!("failed to parse the TOML file: {}", path.display()))?;
        Ok(parsed)
    }

    /// Executes the Lua script and updates the configuration.
    fn apply_lua(config: &mut Config, path: &std::path::Path) -> Result<()> {
        // CRITICAL #4: use the sandboxed Lua (os/io/package disabled).
        let lua = crate::lua_sandbox::sandboxed_lua()
            .map_err(|e| anyhow::anyhow!("failed to initialize the sandboxed Lua: {}", e))?;

        // Convert the current configuration into a Lua table and bind it globally.
        let config_table = config_to_lua_table(&lua, config)?;
        lua.globals()
            .set("nexterm", config_table.clone())
            .map_err(lua_err)?;

        // Register it under `package.preload["nexterm"]` so that
        // `require("nexterm")` returns the same table.
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

        // Execute the Lua file.
        let script = std::fs::read_to_string(path)?;
        let result: LuaValue = lua.load(&script).eval().map_err(lua_err)?;

        // Merge the returned table back into `Config`.
        if let LuaValue::Table(tbl) = result {
            apply_lua_table_to_config(config, &tbl)?;
        }

        Ok(())
    }
}

/// Parses a color-scheme string (kept `pub` for backward compatibility).
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

/// Converts a `Config` into a Lua table (lifetime annotations are not needed
/// in mlua 0.10).
fn config_to_lua_table(lua: &Lua, config: &Config) -> Result<LuaTable> {
    let tbl = lua.create_table().map_err(lua_err)?;

    // `font` table.
    let font = lua.create_table().map_err(lua_err)?;
    font.set("family", config.font.family.clone())
        .map_err(lua_err)?;
    font.set("size", config.font.size).map_err(lua_err)?;
    font.set("ligatures", config.font.ligatures)
        .map_err(lua_err)?;
    tbl.set("font", font).map_err(lua_err)?;

    // `colors` (passed as a string).
    let scheme_str = match &config.colors {
        ColorScheme::Builtin(b) => format!("{:?}", b).to_lowercase(),
        ColorScheme::Custom(_) => "custom".to_string(),
    };
    tbl.set("colors", scheme_str).map_err(lua_err)?;

    // `shell` table.
    let shell = lua.create_table().map_err(lua_err)?;
    shell
        .set("program", config.shell.program.clone())
        .map_err(lua_err)?;
    tbl.set("shell", shell).map_err(lua_err)?;

    // `scrollback_lines`.
    tbl.set("scrollback_lines", config.scrollback_lines)
        .map_err(lua_err)?;

    // `tab_bar` table.
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

    // `hooks` table (nil = unset).
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

/// Merges values from a Lua table into a `Config`.
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
    // Sprint 5-12 Phase 3: extended to also merge `shell.args` from Lua.
    // Example: `shell = { program = "pwsh.exe", args = {"-NoLogo", "-NonInteractive"} }`.
    // The previous implementation discarded `args`, so only `program` was
    // overridden and `args` kept whatever value came from the TOML (or
    // `ShellConfig::default()`).
    if let Ok(LuaValue::Table(shell)) = tbl.get("shell") {
        if let Ok(program) = shell.get::<String>("program") {
            config.shell.program = program;
        }
        if let Ok(LuaValue::Table(args_tbl)) = shell.get("args") {
            let mut args: Vec<String> = Vec::new();
            // Lua tables are 1-indexed.
            for i in 1.. {
                match args_tbl.get::<String>(i) {
                    Ok(arg) => args.push(arg),
                    Err(_) => break,
                }
            }
            if !args.is_empty() {
                config.shell.args = args;
            }
        }
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

// Resolves the configuration directory (using only the standard library).
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

/// Default configuration template generated on first launch.
///
/// **Note**: uses key names that match the actual `Config` struct.
/// Older templates contained `[color_scheme] builtin = ...`,
/// `[tab_bar] show = ...`, and `[status_bar] show = ...`, which were silently
/// ignored because the names did not match the implementation; that has been
/// fixed.
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
# Either pass the name as a string or use the [colors] scheme = "..." table form.
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
    fn default_load_succeeds() {
        let config = ConfigLoader::load().unwrap();
        assert!(!config.shell.program.is_empty());
    }

    #[test]
    fn config_parses_from_toml_string() {
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
    fn config_supports_a_hosts_section() {
        // Regression test for an earlier bug where TomlConfig lacked a `hosts`
        // section and user settings were silently ignored.
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
    fn config_supports_a_window_section() {
        // Regression test for an earlier bug where TomlConfig lacked a `window`
        // section and user settings were silently ignored.
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
    fn config_supports_a_macros_section() {
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
    fn config_supports_cursor_style_and_auto_check_update() {
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
    fn colors_accept_string_scheme_table_and_full_custom_palette() {
        use crate::schema::BuiltinScheme;

        // Form 1: a string.
        let parsed: Config = toml::from_str("colors = \"gruvbox\"").unwrap();
        assert!(matches!(
            parsed.colors,
            ColorScheme::Builtin(BuiltinScheme::Gruvbox)
        ));

        // Form 2: `[colors] scheme = "..."`.
        let parsed: Config = toml::from_str("[colors]\nscheme = \"solarized\"").unwrap();
        assert!(matches!(
            parsed.colors,
            ColorScheme::Builtin(BuiltinScheme::Solarized)
        ));

        // Form 3: full custom palette.
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
            _ => panic!("the custom palette failed to parse"),
        }
    }

    #[test]
    fn default_template_parses_as_a_config() {
        // Confirms the first-launch template itself is well-formed.
        let parsed: Result<Config> = toml::from_str(DEFAULT_CONFIG_TOML).map_err(Into::into);
        assert!(
            parsed.is_ok(),
            "DEFAULT_CONFIG_TOML failed to parse as `Config`: {:?}",
            parsed.err()
        );
        let cfg = parsed.unwrap();
        assert_eq!(cfg.scrollback_lines, 10000);
        assert_eq!(cfg.language, "auto");
        assert!(cfg.tab_bar.enabled);
        assert!(cfg.status_bar.enabled);
        // Regression test for the previous template that used
        // `[color_scheme] builtin = "..."` which was ignored.
        assert!(matches!(
            cfg.colors,
            ColorScheme::Builtin(crate::schema::BuiltinScheme::TokyoNight)
        ));
    }

    #[test]
    fn lua_can_override_the_configuration() {
        let lua = crate::lua_sandbox::sandboxed_lua().unwrap();
        let mut config = Config::default();

        let tbl = config_to_lua_table(&lua, &config).unwrap();

        // Mutate the `font` table directly and apply.
        let font: LuaTable = tbl.get("font").unwrap();
        font.set("size", 20.0f32).unwrap();
        font.set("family", "Hack").unwrap();

        apply_lua_table_to_config(&mut config, &tbl).unwrap();
        assert_eq!(config.font.size, 20.0);
        assert_eq!(config.font.family, "Hack");
    }

    /// Sprint 5-12 Phase 3: regression test for the bug where Lua's
    /// `shell.args` was not merged and silently discarded. The previous
    /// implementation only overrode `shell.program`, dropping `shell.args`.
    #[test]
    fn lua_can_override_shell_args() {
        let lua = crate::lua_sandbox::sandboxed_lua().unwrap();
        let mut config = Config::default();

        let tbl = config_to_lua_table(&lua, &config).unwrap();

        // Update both `shell.program` and `shell.args` from Lua.
        let shell: LuaTable = tbl.get("shell").unwrap();
        shell.set("program", "pwsh.exe").unwrap();
        let args = lua.create_table().unwrap();
        args.set(1, "-NoLogo").unwrap();
        args.set(2, "-NonInteractive").unwrap();
        args.set(3, "-Command").unwrap();
        shell.set("args", args).unwrap();

        apply_lua_table_to_config(&mut config, &tbl).unwrap();
        assert_eq!(config.shell.program, "pwsh.exe");
        assert_eq!(
            config.shell.args,
            vec![
                "-NoLogo".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
            ]
        );
    }

    /// When Lua specifies only `shell.program` and omits `shell.args`, the
    /// existing args are preserved.
    #[test]
    fn lua_keeps_existing_shell_args_when_omitted() {
        let lua = crate::lua_sandbox::sandboxed_lua().unwrap();
        let mut config = Config::default();
        config.shell.args = vec!["--existing".to_string(), "--flag".to_string()];

        let tbl = config_to_lua_table(&lua, &config).unwrap();
        let shell: LuaTable = tbl.get("shell").unwrap();
        shell.set("program", "/bin/bash").unwrap();
        // Intentionally do not set `args`.

        apply_lua_table_to_config(&mut config, &tbl).unwrap();
        assert_eq!(config.shell.program, "/bin/bash");
        assert_eq!(
            config.shell.args,
            vec!["--existing".to_string(), "--flag".to_string()]
        );
    }

    /// When Lua passes an empty table for `shell.args`, the existing value is
    /// preserved. (Erasing the args entirely would need a separate API; the
    /// current behavior errs on the safe side.)
    #[test]
    fn lua_keeps_existing_shell_args_when_table_is_empty() {
        let lua = crate::lua_sandbox::sandboxed_lua().unwrap();
        let mut config = Config::default();
        config.shell.args = vec!["--existing".to_string()];

        let tbl = config_to_lua_table(&lua, &config).unwrap();
        let shell: LuaTable = tbl.get("shell").unwrap();
        shell.set("args", lua.create_table().unwrap()).unwrap();

        apply_lua_table_to_config(&mut config, &tbl).unwrap();
        assert_eq!(config.shell.args, vec!["--existing".to_string()]);
    }

    #[test]
    fn color_scheme_parses_correctly() {
        use crate::schema::BuiltinScheme;
        assert!(matches!(
            parse_color_scheme("tokyonight"),
            ColorScheme::Builtin(BuiltinScheme::TokyoNight)
        ));
        // Unknown scheme names fall back to the default (`Dark`).
        assert!(matches!(
            parse_color_scheme("custom_theme"),
            ColorScheme::Builtin(BuiltinScheme::Dark)
        ));
    }
}
