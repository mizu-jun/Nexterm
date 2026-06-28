//! Configuration schema definitions (organized into 8 submodules).
//!
//! Most type definitions are split into submodules; this file owns [`Config`]
//! itself and the default key bindings. External users should consume the
//! types via the `nexterm_config::*` re-exports.

pub mod animations;
pub mod blocks;
pub mod color;
pub mod font;
pub mod gpu;
pub mod hosts;
pub mod log;
pub mod quake;
pub mod security;
pub mod shell;
pub mod tokens;
pub mod ui;
pub mod web;
pub mod window;

pub use animations::{AnimationIntensity, AnimationsConfig};
pub use blocks::BlocksConfig;
pub use color::{BuiltinScheme, ColorScheme, CustomPalette, InactivePaneHsbConfig, SchemePalette};
pub use font::FontConfig;
pub use gpu::{ApiVersion, GpuConfig, PresentModeConfig, Profile};
pub use hosts::{HooksConfig, HostConfig};
pub use log::{LogConfig, StatusBarConfig};
pub use quake::{QuakeEdge, QuakeModeConfig};
pub use security::{ConsentPolicy, SecurityConfig};
pub use shell::{KeyBinding, MacroConfig, SerialPortConfig, ShellConfig};
pub use tokens::{DesignTokens, parse_hex_color, resolve as resolve_color};
pub use ui::UiConfig;
pub use web::{AccessLogConfig, OAuthConfig, TlsConfig, WebAuthConfig, WebConfig};
pub use window::{
    BackgroundFit, BackgroundImageConfig, CloseAction, CursorConfig, CursorStyle, GradientConfig,
    TabBarConfig, WindowConfig, WindowDecorations,
};

use serde::{Deserialize, Serialize};

/// Top-level configuration structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Errors raised while loading configuration (Lua / TOML parse errors).
    /// Skipped during serialization — runtime-only.
    #[serde(skip)]
    pub config_errors: Vec<String>,

    /// Configuration API version (managed with SemVer).
    #[serde(default)]
    pub api_version: ApiVersion,

    /// Font configuration.
    #[serde(default)]
    pub font: FontConfig,

    /// Color scheme.
    #[serde(default)]
    pub colors: ColorScheme,

    /// Sprint 5-15 / UI/UX Modernization v2 Phase 3: follow the OS light/dark
    /// preference at runtime. When `true`, [`colors_light`] is used while the
    /// OS reports a light theme and [`colors_dark`] is used while it reports
    /// dark; otherwise [`colors`] is used unchanged.
    #[serde(default)]
    pub colors_follow_system: bool,

    /// Built-in scheme to switch to when the OS reports a **light** theme and
    /// [`colors_follow_system`] is on. `None` falls back to [`BuiltinScheme::Light`].
    #[serde(default)]
    pub colors_light: Option<BuiltinScheme>,

    /// Built-in scheme to switch to when the OS reports a **dark** theme and
    /// [`colors_follow_system`] is on. `None` falls back to [`BuiltinScheme::TokyoNight`].
    #[serde(default)]
    pub colors_dark: Option<BuiltinScheme>,

    /// Phase 6 (UI/UX v2): inactive-pane HSB transform (WezTerm-style).
    /// Replaces the flat-black dim overlay with a configurable
    /// brightness / saturation knob. See [`InactivePaneHsbConfig`].
    #[serde(default)]
    pub inactive_pane_hsb: InactivePaneHsbConfig,

    /// Shell configuration.
    #[serde(default)]
    pub shell: ShellConfig,

    /// Key bindings.
    #[serde(default)]
    pub keys: Vec<KeyBinding>,

    /// Status bar (Phase 3).
    #[serde(default)]
    pub status_bar: StatusBarConfig,

    /// Scrollback line count.
    #[serde(default = "default_scrollback")]
    pub scrollback_lines: usize,

    /// Window configuration (transparency, blur, decorations).
    #[serde(default)]
    pub window: WindowConfig,

    /// Tab-bar configuration.
    #[serde(default)]
    pub tab_bar: TabBarConfig,

    /// SSH host list.
    #[serde(default)]
    pub hosts: Vec<HostConfig>,

    /// Lua macro list (defined as `[[macros]]` tables).
    #[serde(default)]
    pub macros: Vec<MacroConfig>,

    /// Serial-port presets.
    #[serde(default)]
    pub serial_ports: Vec<SerialPortConfig>,

    /// Logging configuration.
    #[serde(default)]
    pub log: LogConfig,

    /// Terminal hooks (event-driven shell commands).
    #[serde(default)]
    pub hooks: HooksConfig,

    /// Web terminal configuration (WebSocket + xterm.js).
    #[serde(default)]
    pub web: WebConfig,

    /// Named configuration profiles.
    #[serde(default)]
    pub profiles: Vec<Profile>,

    /// Currently active profile name (`None` = use the default configuration).
    #[serde(default)]
    pub active_profile: Option<String>,

    /// Directory that holds WASM plugins (`None` = use the default directory).
    /// Default: `~/.config/nexterm/plugins` (Linux/macOS) or
    /// `%APPDATA%\nexterm\plugins` (Windows).
    #[serde(default)]
    pub plugin_dir: Option<String>,

    /// Whether plugins are disabled.
    #[serde(default)]
    pub plugins_disabled: bool,

    /// GPU renderer configuration.
    #[serde(default)]
    pub gpu: GpuConfig,

    /// Display language (`"auto"` = OS detection, or `"en"` / `"ja"` / `"fr"`
    /// / `"de"` / `"es"` / `"it"` / `"zh-CN"` / `"ko"`).
    #[serde(default = "default_language")]
    pub language: String,

    /// Cursor display style (`"block"` / `"beam"` / `"underline"`). Default: block.
    #[serde(default)]
    pub cursor_style: CursorStyle,

    /// Phase 5 (UI/UX v2): cursor blink + smooth-motion configuration.
    /// Independent of [`cursor_style`] — controls *when* the cursor is drawn
    /// (blink) and *how it moves between cells* (smooth motion). Defaults
    /// match the xterm cadence (530 ms blink half-period, smooth motion on).
    #[serde(default)]
    pub cursor: CursorConfig,

    /// Whether to check the GitHub Releases API for the latest version at
    /// startup (default: `true`).
    #[serde(default = "default_auto_check_update")]
    pub auto_check_update: bool,

    /// Security / consent policy (external URLs, OSC 52, OSC notifications).
    #[serde(default)]
    pub security: SecurityConfig,

    /// Leader key (tmux-prefix style). Expands `<leader>` inside key bindings
    /// to this value.
    /// Default: `"ctrl+b"` (tmux-compatible). To avoid clashing with Emacs's
    /// C-b, set this to `"ctrl+q"` or similar.
    /// Sprint 5-7 / UI-1-3.
    #[serde(default = "default_leader_key")]
    pub leader_key: String,

    /// Quake-mode configuration (Sprint 5-7 / Phase 2-2).
    /// A Tilix / Guake-style toggle that slides the window in from a screen
    /// edge via a global hotkey. Default: `enabled = false`.
    #[serde(default)]
    pub quake_mode: QuakeModeConfig,

    /// Animation configuration (Sprint 5-7 / Phase 3-2).
    /// Provides unified control over UI animations such as tab switching and
    /// pane insertion. Setting `enabled = false` or `intensity = "off"`
    /// applies every change instantly (reduced-motion support).
    #[serde(default)]
    pub animations: AnimationsConfig,

    /// Command-blocks UI (Phase 2 of the blocks feature). Drives the Warp-style
    /// left border / exit-status badge overlay rendered alongside the grid when
    /// the shell emits OSC 133 prompt sequences. Disable with `enabled = false`
    /// to skip the overlay pass entirely.
    #[serde(default)]
    pub blocks: BlocksConfig,

    /// UI chrome appearance (Sprint 5-15 / UI/UX Modernization v2 Phase 1).
    /// Drives the SDF rounded-rect background pipeline. Setting every radius
    /// to `0.0` reproduces the pre-v2 flat-rect look.
    #[serde(default)]
    pub ui: UiConfig,
}

fn default_leader_key() -> String {
    "ctrl+b".to_string()
}

fn default_auto_check_update() -> bool {
    true
}

fn default_scrollback() -> usize {
    50_000
}

fn default_language() -> String {
    "auto".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_errors: Vec::new(),
            api_version: ApiVersion::default(),
            font: FontConfig::default(),
            colors: ColorScheme::default(),
            colors_follow_system: false,
            colors_light: None,
            colors_dark: None,
            inactive_pane_hsb: InactivePaneHsbConfig::default(),
            shell: ShellConfig::default(),
            keys: default_keybindings(),
            status_bar: StatusBarConfig::default(),
            scrollback_lines: default_scrollback(),
            window: WindowConfig::default(),
            tab_bar: TabBarConfig::default(),
            hosts: Vec::new(),
            macros: Vec::new(),
            serial_ports: Vec::new(),
            log: LogConfig::default(),
            hooks: HooksConfig::default(),
            web: WebConfig::default(),
            profiles: Vec::new(),
            active_profile: None,
            plugin_dir: None,
            plugins_disabled: false,
            gpu: GpuConfig::default(),
            language: default_language(),
            cursor_style: CursorStyle::default(),
            cursor: CursorConfig::default(),
            auto_check_update: default_auto_check_update(),
            security: SecurityConfig::default(),
            leader_key: default_leader_key(),
            quake_mode: QuakeModeConfig::default(),
            animations: AnimationsConfig::default(),
            blocks: BlocksConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

impl Config {
    /// Expands `<leader>` in a key-binding string into `self.leader_key`
    /// (Sprint 5-7 / UI-1-3).
    ///
    /// For example, when `leader_key = "ctrl+q"`, `"<leader> %"` becomes
    /// `"ctrl+q %"`. Strings that do not contain `<leader>` are returned
    /// unchanged (backward-compatible).
    pub fn expand_leader(&self, key: &str) -> String {
        if key.contains("<leader>") {
            key.replace("<leader>", &self.leader_key)
        } else {
            key.to_string()
        }
    }

    /// Returns the configuration with the active profile applied.
    /// When no profile is active or the named profile is missing, returns a
    /// clone of `self`.
    pub fn effective(&self) -> Config {
        if let Some(ref name) = self.active_profile
            && let Some(profile) = self.profiles.iter().find(|p| &p.name == name)
        {
            return profile.apply_to(self);
        }
        self.clone()
    }

    /// Activates the profile with the given name (ignored if no such profile
    /// exists).
    pub fn activate_profile(&mut self, name: &str) {
        if self.profiles.iter().any(|p| p.name == name) {
            self.active_profile = Some(name.to_string());
        }
    }

    /// Clears the active profile and reverts to the default configuration.
    pub fn clear_active_profile(&mut self) {
        self.active_profile = None;
    }

    /// Resolve the color scheme that should be used for this frame, honouring
    /// [`colors_follow_system`] (Sprint 5-15 / UI/UX Modernization v2 Phase 3).
    ///
    /// * `os_dark` is `Some(true)` when the OS reports a dark theme, `Some(false)`
    ///   when light, and `None` when the OS theme is unknown (e.g. unsupported
    ///   platform). When `colors_follow_system` is off or `os_dark` is `None`,
    ///   the configured [`colors`] is returned verbatim.
    /// * When following is active and the OS theme is known, the matching
    ///   [`colors_light`] / [`colors_dark`] override is returned (falling back
    ///   to [`BuiltinScheme::Light`] / [`BuiltinScheme::TokyoNight`]).
    pub fn effective_color_scheme(&self, os_dark: Option<bool>) -> ColorScheme {
        if !self.colors_follow_system {
            return self.colors.clone();
        }
        match os_dark {
            Some(true) => {
                ColorScheme::Builtin(self.colors_dark.unwrap_or(BuiltinScheme::TokyoNight))
            }
            Some(false) => ColorScheme::Builtin(self.colors_light.unwrap_or(BuiltinScheme::Light)),
            None => self.colors.clone(),
        }
    }
}

/// Default key bindings (tmux-compatible).
fn default_keybindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding {
            key: "ctrl+b %".to_string(),
            action: "SplitVertical".to_string(),
        },
        KeyBinding {
            key: "ctrl+b \"".to_string(),
            action: "SplitHorizontal".to_string(),
        },
        KeyBinding {
            key: "ctrl+b o".to_string(),
            action: "FocusNextPane".to_string(),
        },
        KeyBinding {
            key: "ctrl+b d".to_string(),
            action: "Detach".to_string(),
        },
        KeyBinding {
            key: "ctrl+shift+p".to_string(),
            action: "CommandPalette".to_string(),
        },
        KeyBinding {
            key: "ctrl+b z".to_string(),
            action: "ToggleZoom".to_string(),
        },
        // Sprint 5-4 / D8: in addition to the tmux-style Ctrl+B Z, add a more
        // discoverable binding for newcomers.
        KeyBinding {
            key: "ctrl+shift+z".to_string(),
            action: "ToggleZoom".to_string(),
        },
        KeyBinding {
            key: "ctrl+b {".to_string(),
            action: "SwapPanePrev".to_string(),
        },
        KeyBinding {
            key: "ctrl+b }".to_string(),
            action: "SwapPaneNext".to_string(),
        },
        KeyBinding {
            key: "ctrl+b !".to_string(),
            action: "BreakPane".to_string(),
        },
        // Sprint 5-8 / Phase 4-5: default bindings related to tab tearing.
        // `<leader> D` = ctrl+b D detaches the current tab into a new OS
        // Window (Wayland alternative UX #2).
        KeyBinding {
            key: "ctrl+b d".to_string(),
            action: "DetachToNewWindow".to_string(),
        },
        // `<leader> w` = ctrl+b w closes only the current OS Window
        // (exits when it is the last one).
        KeyBinding {
            key: "ctrl+b w".to_string(),
            action: "CloseOsWindow".to_string(),
        },
        KeyBinding {
            key: "ctrl+shift+space".to_string(),
            action: "QuickSelect".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_can_be_constructed() {
        let config = Config::default();
        assert_eq!(config.api_version.0, "1.0");
        assert!(config.font.size > 0.0);
        assert!(!config.shell.program.is_empty());
        assert!(config.scrollback_lines > 0);
    }

    #[test]
    fn font_default_values() {
        let font = FontConfig::default();
        assert!(font.ligatures);
        assert_eq!(font.size, 15.0);
    }

    // ---- Sprint 5-7 / UI-1-3: leader-key expansion tests ----

    #[test]
    fn leader_key_default_is_ctrl_b() {
        let cfg = Config::default();
        assert_eq!(cfg.leader_key, "ctrl+b");
    }

    #[test]
    fn expand_leader_replaces_leader() {
        let cfg = Config {
            leader_key: "ctrl+q".to_string(),
            ..Default::default()
        };
        assert_eq!(cfg.expand_leader("<leader> %"), "ctrl+q %");
        assert_eq!(cfg.expand_leader("<leader> \""), "ctrl+q \"");
    }

    #[test]
    fn expand_leader_returns_unchanged_when_no_leader_present() {
        let cfg = Config::default();
        assert_eq!(cfg.expand_leader("ctrl+b %"), "ctrl+b %");
        assert_eq!(cfg.expand_leader("ctrl+shift+p"), "ctrl+shift+p");
    }

    #[test]
    fn expand_leader_replaces_multiple_occurrences() {
        let cfg = Config::default();
        // Normally there is only one, but check just in case.
        assert_eq!(cfg.expand_leader("<leader>+<leader>"), "ctrl+b+ctrl+b");
    }

    /// Sprint 5-4 / D8: there are two default key bindings for `ToggleZoom`
    /// (the tmux-style `Ctrl+B Z` and the newcomer-friendly `Ctrl+Shift+Z`).
    #[test]
    fn toggle_zoom_has_two_default_bindings() {
        let bindings = default_keybindings();
        let zoom_bindings: Vec<&KeyBinding> = bindings
            .iter()
            .filter(|b| b.action == "ToggleZoom")
            .collect();
        assert_eq!(
            zoom_bindings.len(),
            2,
            "ToggleZoom should have 2 default bindings"
        );
        let keys: Vec<&str> = zoom_bindings.iter().map(|b| b.key.as_str()).collect();
        assert!(keys.contains(&"ctrl+b z"));
        assert!(keys.contains(&"ctrl+shift+z"));
    }

    /// Sprint 5-4 / D8: `QuickSelect` is also reachable via a default key binding.
    #[test]
    fn quick_select_has_default_binding() {
        let bindings = default_keybindings();
        assert!(
            bindings
                .iter()
                .any(|b| b.action == "QuickSelect" && b.key == "ctrl+shift+space")
        );
    }

    #[test]
    fn toml_serialize_roundtrip() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.font, parsed.font);
        assert_eq!(config.scrollback_lines, parsed.scrollback_lines);
    }

    #[test]
    fn profile_is_applied_to_config() {
        let mut config = Config::default();
        config.profiles.push(Profile {
            name: "big-font".to_string(),
            font: Some(FontConfig {
                family: "Hack Nerd Font".to_string(),
                size: 20.0,
                ..FontConfig::default()
            }),
            ..Profile::default()
        });

        config.activate_profile("big-font");
        let effective = config.effective();
        assert_eq!(effective.font.size, 20.0);
        assert_eq!(effective.font.family, "Hack Nerd Font");
        // Settings not overridden by the profile keep their base values.
        assert_eq!(effective.scrollback_lines, config.scrollback_lines);
    }

    #[test]
    fn nonexistent_profile_is_ignored() {
        let mut config = Config::default();
        config.activate_profile("non-existent");
        // The active profile should remain unchanged when the name is unknown.
        assert_eq!(config.active_profile, None);
    }

    #[test]
    fn no_active_profile_returns_the_base_config() {
        let config = Config::default();
        let effective = config.effective();
        assert_eq!(effective.font, config.font);
    }

    #[test]
    fn profile_parses_from_toml() {
        let toml_str = r#"
[[profiles]]
name = "minimal"

[profiles.font]
family = "Inconsolata"
size = 12.0
ligatures = false
"#;
        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.profiles.len(), 1);
        assert_eq!(parsed.profiles[0].name, "minimal");
        let font = parsed.profiles[0].font.as_ref().unwrap();
        assert_eq!(font.size, 12.0);
        assert!(!font.ligatures);
    }

    #[test]
    fn status_bar_config_default_values() {
        let sb = StatusBarConfig::default();
        assert!(!sb.enabled);
        assert!(sb.widgets.is_empty());
        // `right_widgets` should include `"time"` by default.
        assert!(sb.right_widgets.contains(&"time".to_string()));
        assert_eq!(sb.separator, "  ");
    }

    #[test]
    fn status_bar_config_toml_roundtrip() {
        let toml_str = r#"
[status_bar]
enabled = true
widgets = ["session", "pane_id"]
right_widgets = ["time", "date"]
separator = " | "
"#;
        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert!(parsed.status_bar.enabled);
        assert_eq!(parsed.status_bar.widgets, vec!["session", "pane_id"]);
        assert_eq!(parsed.status_bar.right_widgets, vec!["time", "date"]);
        assert_eq!(parsed.status_bar.separator, " | ");
    }

    #[test]
    fn plugin_dir_defaults_to_none() {
        let config = Config::default();
        assert!(config.plugin_dir.is_none());
        assert!(!config.plugins_disabled);
    }

    #[test]
    fn window_config_default_values() {
        let w = WindowConfig::default();
        assert!((w.background_opacity - 0.95).abs() < f32::EPSILON);
        assert_eq!(w.macos_window_background_blur, 0);
        assert_eq!(w.decorations, WindowDecorations::Full);
        assert_eq!(w.layout_mode, "bsp");
    }

    #[test]
    fn window_config_layout_mode_can_be_set_via_toml() {
        let toml_str = r#"
[window]
background_opacity = 0.9
layout_mode = "tiling"
"#;
        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.window.layout_mode, "tiling");
        assert!((parsed.window.background_opacity - 0.9).abs() < 0.001);
    }

    #[test]
    fn builtin_scheme_display_names() {
        assert_eq!(BuiltinScheme::Dark.display_name(), "Dark");
        assert_eq!(BuiltinScheme::TokyoNight.display_name(), "Tokyo Night");
        assert_eq!(BuiltinScheme::OneDark.display_name(), "One Dark");
    }

    #[test]
    fn webconfig_allow_http_fallback_defaults_to_false() {
        // CRITICAL #3: guarantees the safe default.
        let cfg = WebConfig::default();
        assert!(
            !cfg.allow_http_fallback,
            "the default of allow_http_fallback must be false (no HTTP fallback)"
        );
    }

    // ---- Sprint 5-15 / Phase 3: OS theme follow ----

    #[test]
    fn effective_color_scheme_returns_explicit_colors_when_follow_disabled() {
        let cfg = Config {
            colors: ColorScheme::Builtin(BuiltinScheme::Gruvbox),
            colors_follow_system: false,
            colors_light: Some(BuiltinScheme::Light),
            colors_dark: Some(BuiltinScheme::Dark),
            ..Default::default()
        };
        // `colors_light` / `colors_dark` must be ignored.
        assert_eq!(
            cfg.effective_color_scheme(Some(true)),
            ColorScheme::Builtin(BuiltinScheme::Gruvbox)
        );
        assert_eq!(
            cfg.effective_color_scheme(Some(false)),
            ColorScheme::Builtin(BuiltinScheme::Gruvbox)
        );
    }

    #[test]
    fn effective_color_scheme_follows_os_dark_when_enabled() {
        let cfg = Config {
            colors_follow_system: true,
            colors_light: Some(BuiltinScheme::Solarized),
            colors_dark: Some(BuiltinScheme::Dracula),
            ..Default::default()
        };
        assert_eq!(
            cfg.effective_color_scheme(Some(true)),
            ColorScheme::Builtin(BuiltinScheme::Dracula)
        );
        assert_eq!(
            cfg.effective_color_scheme(Some(false)),
            ColorScheme::Builtin(BuiltinScheme::Solarized)
        );
    }

    #[test]
    fn effective_color_scheme_falls_back_when_no_override_set() {
        let cfg = Config {
            colors_follow_system: true,
            colors_light: None,
            colors_dark: None,
            ..Default::default()
        };
        assert_eq!(
            cfg.effective_color_scheme(Some(true)),
            ColorScheme::Builtin(BuiltinScheme::TokyoNight)
        );
        assert_eq!(
            cfg.effective_color_scheme(Some(false)),
            ColorScheme::Builtin(BuiltinScheme::Light)
        );
    }

    #[test]
    fn effective_color_scheme_falls_back_to_colors_when_os_unknown() {
        // When the OS theme is unknown (None), the configured `colors` is
        // returned even if `colors_follow_system` is on so the user is never
        // shown a surprise scheme.
        let cfg = Config {
            colors: ColorScheme::Builtin(BuiltinScheme::Nord),
            colors_follow_system: true,
            colors_dark: Some(BuiltinScheme::Dracula),
            ..Default::default()
        };
        assert_eq!(
            cfg.effective_color_scheme(None),
            ColorScheme::Builtin(BuiltinScheme::Nord)
        );
    }

    #[test]
    fn webconfig_reads_allow_http_fallback_from_toml() {
        let toml_str = r#"
enabled = true
allow_http_fallback = true
"#;
        let cfg: WebConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.allow_http_fallback);
    }
}
