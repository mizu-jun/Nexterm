//! Display-related configuration: window, tab bar, and cursor.

use serde::{Deserialize, Serialize};

/// Window decoration mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WindowDecorations {
    /// Show the OS-native title bar and borders.
    #[default]
    Full,
    /// No title bar and no borders (borderless).
    None,
    /// Hide only the title bar.
    NoTitle,
}

/// Background-image fit mode (Sprint 5-7 / Phase 3-1).
///
/// Determines whether the aspect ratio is preserved and how cropping or
/// padding is handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundFit {
    /// Cover the entire screen (aspect ratio preserved; overflow is cropped).
    #[default]
    Cover,
    /// Fit inside the screen (aspect ratio preserved; the margin is transparent).
    Contain,
    /// Stretch to the screen size exactly (ignores the aspect ratio).
    Stretch,
    /// Center the image at its natural size (no scaling).
    Center,
    /// Tile the image (no scaling).
    Tile,
}

/// Background-image configuration (Sprint 5-7 / Phase 3-1).
///
/// Displays an image behind the terminal. The image is loaded once at startup
/// (hot reload is not supported). Supported formats: PNG / JPEG.
///
/// ```toml
/// [window.background_image]
/// path = "~/wallpaper.png"
/// opacity = 0.3
/// fit = "cover"
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundImageConfig {
    /// Image file path (`~` is expanded).
    pub path: String,
    /// Image opacity (0.0 = fully transparent, 1.0 = opaque). Default: 0.3.
    #[serde(default = "default_image_opacity")]
    pub opacity: f32,
    /// Fit mode (cover / contain / stretch / center / tile). Default: cover.
    #[serde(default)]
    pub fit: BackgroundFit,
}

fn default_image_opacity() -> f32 {
    // 0.3 keeps the terminal readable while leaving the image visible.
    0.3
}

impl Default for BackgroundImageConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            opacity: default_image_opacity(),
            fit: BackgroundFit::default(),
        }
    }
}

impl BackgroundImageConfig {
    /// Treats the config as enabled only when `path` is non-empty.
    pub fn is_enabled(&self) -> bool {
        !self.path.trim().is_empty()
    }

    /// Returns `opacity` clamped to the range `[0.0, 1.0]`.
    pub fn clamped_opacity(&self) -> f32 {
        self.opacity.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod background_image_tests {
    use super::*;

    #[test]
    fn default_is_disabled_with_an_empty_path() {
        let cfg = BackgroundImageConfig::default();
        assert!(cfg.path.is_empty());
        assert!(!cfg.is_enabled());
        assert_eq!(cfg.fit, BackgroundFit::Cover);
        assert!((cfg.opacity - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn whitespace_only_path_is_treated_as_disabled() {
        let cfg = BackgroundImageConfig {
            path: "   ".to_string(),
            ..BackgroundImageConfig::default()
        };
        assert!(!cfg.is_enabled());
    }

    #[test]
    fn specifying_a_path_enables_the_background() {
        let cfg = BackgroundImageConfig {
            path: "~/wall.png".to_string(),
            ..BackgroundImageConfig::default()
        };
        assert!(cfg.is_enabled());
    }

    #[test]
    fn opacity_is_clamped_to_0_through_1() {
        let cfg = BackgroundImageConfig {
            opacity: -0.5,
            ..BackgroundImageConfig::default()
        };
        assert!((cfg.clamped_opacity() - 0.0).abs() < f32::EPSILON);
        let cfg = BackgroundImageConfig {
            opacity: 1.5,
            ..BackgroundImageConfig::default()
        };
        assert!((cfg.clamped_opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parses_from_toml() {
        let toml_str = r#"
[window.background_image]
path = "~/wallpaper.png"
opacity = 0.5
fit = "contain"
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        let bg = parsed.window.background_image.unwrap();
        assert_eq!(bg.path, "~/wallpaper.png");
        assert!((bg.opacity - 0.5).abs() < f32::EPSILON);
        assert_eq!(bg.fit, BackgroundFit::Contain);
    }

    #[test]
    fn background_image_is_none_by_default() {
        let cfg = WindowConfig::default();
        assert!(cfg.background_image.is_none());
    }
}

/// Behavior when the user closes an OS window (Sprint 5-7 / Phase 4-1).
///
/// Decides what happens to the corresponding server-side Window (a logical
/// window) when the user closes the client's OS Window (via the × button,
/// Cmd+W, Ctrl+Shift+Q, etc.). The hybrid approach from Phase 4 open question
/// #1:
///
/// - `Prompt` (default): show a confirmation dialog only when a foreground
///   process is still running; otherwise kill (balances accidental-close
///   protection with intuitive behavior).
/// - `Detach`: always keep the server-side Window (tmux-style detached
///   session). The user can reattach it with `nexterm-ctl attach`, which is
///   resilient to accidentally losing long-running jobs.
/// - `Kill`: always destroy. Every pane is killed and the server Window is
///   removed (the behavior of modern GUI terminals such as Windows Terminal
///   and VS Code).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CloseAction {
    /// Show a confirmation dialog only if a foreground process exists; kill
    /// otherwise.
    #[default]
    Prompt,
    /// Always keep the server-side Window (detach).
    Detach,
    /// Always destroy the server-side Window (kill).
    Kill,
}

/// Window configuration (opacity, blur, decorations).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    /// Window opacity (0.0 = fully transparent, 1.0 = opaque).
    #[serde(default = "default_background_opacity")]
    pub background_opacity: f32,
    /// macOS window blur strength (0 = none).
    #[serde(default)]
    pub macos_window_background_blur: u32,
    /// Window decorations.
    #[serde(default)]
    pub decorations: WindowDecorations,
    /// Pane layout mode: `"bsp"` (manual splits; default) or `"tiling"`
    /// (automatic even tiling).
    #[serde(default = "default_layout_mode")]
    pub layout_mode: String,
    /// Horizontal padding inside the window (pixels). Default: 0.
    #[serde(default)]
    pub padding_x: u32,
    /// Vertical padding inside the window (pixels). Default: 0.
    #[serde(default)]
    pub padding_y: u32,
    /// Background-image configuration (Sprint 5-7 / Phase 3-1). `None` = no
    /// background image.
    #[serde(default)]
    pub background_image: Option<BackgroundImageConfig>,
    /// Behavior when the OS Window is closed (Sprint 5-7 / Phase 4-1).
    /// One of `prompt` / `detach` / `kill`. Default: `prompt`.
    /// See [`CloseAction`] for details.
    #[serde(default)]
    pub close_action: CloseAction,
}

fn default_background_opacity() -> f32 {
    // Default 0.95 (slightly transparent). Set to 1.0 in `nexterm.toml` for
    // a fully opaque window.
    0.95
}

fn default_layout_mode() -> String {
    "bsp".to_string()
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            background_opacity: default_background_opacity(),
            macos_window_background_blur: 0,
            decorations: WindowDecorations::Full,
            layout_mode: default_layout_mode(),
            padding_x: 0,
            padding_y: 0,
            background_image: None,
            close_action: CloseAction::default(),
        }
    }
}

#[cfg(test)]
mod close_action_tests {
    use super::*;

    #[test]
    fn default_is_prompt() {
        assert_eq!(CloseAction::default(), CloseAction::Prompt);
        let cfg = WindowConfig::default();
        assert_eq!(cfg.close_action, CloseAction::Prompt);
    }

    #[test]
    fn toml_parses_prompt() {
        let toml_str = r#"
[window]
close_action = "prompt"
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.window.close_action, CloseAction::Prompt);
    }

    #[test]
    fn toml_parses_detach() {
        let toml_str = r#"
[window]
close_action = "detach"
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.window.close_action, CloseAction::Detach);
    }

    #[test]
    fn toml_parses_kill() {
        let toml_str = r#"
[window]
close_action = "kill"
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.window.close_action, CloseAction::Kill);
    }

    #[test]
    fn omitting_close_action_in_toml_uses_the_default() {
        let toml_str = r#"
[window]
background_opacity = 0.9
"#;
        let parsed: super::super::Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.window.close_action, CloseAction::Prompt);
    }

    #[test]
    fn invalid_values_fail_to_parse() {
        let toml_str = r#"
[window]
close_action = "invalid"
"#;
        let result: Result<super::super::Config, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "unknown values should fail to parse");
    }

    #[test]
    fn toml_roundtrip_preserves_the_value() {
        // Serialize → deserialize each variant through `WindowConfig` and
        // verify equality.
        for action in [CloseAction::Prompt, CloseAction::Detach, CloseAction::Kill] {
            let cfg = WindowConfig {
                close_action: action,
                ..WindowConfig::default()
            };
            let s = toml::to_string(&cfg).expect("WindowConfig should be serializable");
            let parsed: WindowConfig =
                toml::from_str(&s).expect("serialized output should be deserializable");
            assert_eq!(parsed.close_action, action);
        }
    }
}

/// Cursor display style.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    /// Block (fills the entire cell).
    #[default]
    Block,
    /// Beam (a 2-pixel vertical line).
    Beam,
    /// Underline (a 2-pixel horizontal line).
    Underline,
}

/// Tab-bar configuration (WezTerm-style).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabBarConfig {
    /// Whether to show the tab bar.
    pub enabled: bool,
    /// Tab-bar height (pixels).
    pub height: u32,
    /// Active-tab background color override (`#RRGGBB`).
    ///
    /// `None` (the default) derives the color from the active color scheme via
    /// [`crate::DesignTokens`].  Set to an explicit hex string to pin a color
    /// regardless of the scheme.
    #[serde(default)]
    pub active_tab_bg: Option<String>,
    /// Inactive-tab background color override (`#RRGGBB`).
    ///
    /// `None` derives from the active color scheme.
    #[serde(default)]
    pub inactive_tab_bg: Option<String>,
    /// Tab separator character.
    pub separator: String,
    /// Activity-tab background color override (`#RRGGBB`).
    ///
    /// `None` derives from the active color scheme.
    #[serde(default)]
    pub activity_tab_bg: Option<String>,
    /// Accent-line color at the bottom of the active tab (`#RRGGBB`).
    ///
    /// `None` derives from the active color scheme accent.
    #[serde(default)]
    pub active_accent_color: Option<String>,
    /// Whether to prefix the tab label with the pane number in `[1]` form
    /// (Windows Terminal-style).
    #[serde(default)]
    pub show_tab_number: bool,
    /// How much to mute inactive-tab text (0.0 = darkest, 1.0 = lightest).
    /// The default of 0.55 produces a darkness close to WezTerm's `#5c6d74`.
    #[serde(default = "default_inactive_text_brightness")]
    pub inactive_text_brightness: f32,
    /// Whether to brighten the tab background on mouse hover.
    #[serde(default = "default_true")]
    pub hover_highlight: bool,
    /// Hide the tab bar entirely when only one tab is visible
    /// (WezTerm `hide_tab_bar_if_only_one_tab` equivalent).
    /// Default: `false` (always show the tab bar).
    ///
    /// Sprint 5-15 / UI/UX Modernization v2 Phase 2b.
    #[serde(default)]
    pub hide_when_single: bool,
    /// Render an inline `+` new-tab button on the right side of the tab bar
    /// (just before the Settings button). Click triggers a `NewPane` IPC.
    /// Default: `true` for parity with Windows Terminal and modern emulators.
    ///
    /// Sprint 5-15 / UI/UX Modernization v2 Phase 2b.
    #[serde(default = "default_true")]
    pub show_new_tab_button: bool,
}

fn default_inactive_text_brightness() -> f32 {
    0.55
}

fn default_true() -> bool {
    true
}

impl Default for TabBarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            height: 32,
            // All color fields default to None so DesignTokens drives the look.
            active_tab_bg: None,
            inactive_tab_bg: None,
            separator: "❯".to_string(),
            activity_tab_bg: None,
            active_accent_color: None,
            show_tab_number: false,
            inactive_text_brightness: default_inactive_text_brightness(),
            hover_highlight: true,
            hide_when_single: false,
            show_new_tab_button: true,
        }
    }
}
