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
    /// Phase 5 (UI/UX v2): two-stop linear-gradient background. Mutually
    /// exclusive with `background_image` — when both are set, the image wins
    /// (the renderer skips the gradient drawcall). `None` = no gradient.
    #[serde(default)]
    pub gradient: Option<GradientConfig>,
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
            gradient: None,
            close_action: CloseAction::default(),
        }
    }
}

/// Phase 5 (UI/UX v2): linear-gradient background configuration.
///
/// Renders a two-stop linear gradient across the entire window. Angle follows
/// the CSS convention: 0° = bottom → top, 90° = left → right, 180° = top →
/// bottom, 270° = right → left. Values outside [0, 360) are wrapped.
///
/// ```toml
/// [window.gradient]
/// from = "#1a1a2e"
/// to = "#16213e"
/// angle = 180.0
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GradientConfig {
    /// Start colour (hex `#RRGGBB` or `#RRGGBBAA`).
    pub from: String,
    /// End colour (hex `#RRGGBB` or `#RRGGBBAA`).
    pub to: String,
    /// Gradient angle in degrees. Default: 180.0 (top → bottom).
    #[serde(default = "default_gradient_angle")]
    pub angle: f32,
}

fn default_gradient_angle() -> f32 {
    180.0
}

impl Default for GradientConfig {
    fn default() -> Self {
        Self {
            from: String::new(),
            to: String::new(),
            angle: default_gradient_angle(),
        }
    }
}

impl GradientConfig {
    /// A gradient is considered enabled only when both stops have non-empty
    /// hex strings. This keeps `[window.gradient]` sections that exist for
    /// future editing from rendering an all-black panel.
    pub fn is_enabled(&self) -> bool {
        !self.from.trim().is_empty() && !self.to.trim().is_empty()
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

/// Phase 5 (UI/UX v2): cursor blink + smooth-motion configuration.
///
/// Independent of [`CursorStyle`] (block / beam / underline). All fields
/// default to "on" with the de-facto xterm blink interval so existing users
/// see the cursor blink the moment they upgrade; users on low-power devices
/// can disable both by setting the respective fields to `false`.
///
/// ```toml
/// [cursor]
/// blink_enabled = true
/// blink_interval_ms = 530
/// smooth_motion = true
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CursorConfig {
    /// Whether the cursor blinks. Default: `true` (matches xterm).
    #[serde(default = "default_blink_enabled")]
    pub blink_enabled: bool,
    /// Blink half-period in milliseconds. Default: 530 (the xterm cadence —
    /// one full on/off cycle is 1060 ms). Values < 50 ms are clamped at
    /// render time to avoid epileptic flicker.
    #[serde(default = "default_blink_interval_ms")]
    pub blink_interval_ms: u32,
    /// Whether the cursor interpolates smoothly between cells when it moves.
    /// Default: `true`. When false the cursor snaps immediately, matching
    /// the pre-Phase-5 behaviour.
    #[serde(default = "default_smooth_motion")]
    pub smooth_motion: bool,
}

fn default_blink_enabled() -> bool {
    true
}

fn default_blink_interval_ms() -> u32 {
    530
}

fn default_smooth_motion() -> bool {
    true
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            blink_enabled: default_blink_enabled(),
            blink_interval_ms: default_blink_interval_ms(),
            smooth_motion: default_smooth_motion(),
        }
    }
}

impl CursorConfig {
    /// Clamp `blink_interval_ms` to a safe minimum so a hostile / typo'd
    /// value cannot drive the cursor into seizure-inducing flicker. 50 ms
    /// equals 20 Hz, well below the photosensitive-epilepsy guideline of
    /// 3 Hz, but a reasonable floor.
    pub fn safe_blink_interval_ms(&self) -> u32 {
        self.blink_interval_ms.max(50)
    }

    /// Returns `true` when the cursor should be drawn for an elapsed time
    /// of `t_ms` against this config. Pure helper kept on the type so
    /// renderer code can call it without touching wall-clock state.
    pub fn is_visible_at(&self, t_ms: u64) -> bool {
        if !self.blink_enabled {
            return true;
        }
        let interval = self.safe_blink_interval_ms() as u64;
        // Visible on even half-periods, hidden on odd ones.
        (t_ms / interval).is_multiple_of(2)
    }
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

#[cfg(test)]
mod phase5_config_tests {
    //! Phase 5 (UI/UX v2): CursorConfig + GradientConfig tests.
    use super::*;

    #[test]
    fn cursor_config_defaults_are_xterm_compatible() {
        let c = CursorConfig::default();
        assert!(c.blink_enabled);
        assert_eq!(c.blink_interval_ms, 530);
        assert!(c.smooth_motion);
    }

    #[test]
    fn cursor_is_visible_when_blink_disabled() {
        let c = CursorConfig {
            blink_enabled: false,
            ..CursorConfig::default()
        };
        // Always visible regardless of elapsed time.
        for t in [0u64, 100, 530, 1060, 1_000_000] {
            assert!(c.is_visible_at(t), "blink off at t={} should be visible", t);
        }
    }

    #[test]
    fn cursor_blink_alternates_on_half_period() {
        let c = CursorConfig {
            blink_enabled: true,
            blink_interval_ms: 500,
            ..CursorConfig::default()
        };
        // First half-period: visible.
        assert!(c.is_visible_at(0));
        assert!(c.is_visible_at(499));
        // Second half-period: hidden.
        assert!(!c.is_visible_at(500));
        assert!(!c.is_visible_at(999));
        // Third half-period (= first of next cycle): visible again.
        assert!(c.is_visible_at(1000));
        assert!(c.is_visible_at(1499));
    }

    #[test]
    fn cursor_blink_interval_floor_protects_against_seizure_speeds() {
        let c = CursorConfig {
            blink_enabled: true,
            blink_interval_ms: 1, // Hostile / typo value.
            ..CursorConfig::default()
        };
        // 1ms gets clamped to 50ms (20 Hz), well above the 3 Hz photosensitive
        // threshold for healthy use but below the legal seizure trigger.
        assert_eq!(c.safe_blink_interval_ms(), 50);
        // Visible at 0ms..49ms, hidden at 50ms..99ms.
        assert!(c.is_visible_at(49));
        assert!(!c.is_visible_at(50));
    }

    #[test]
    fn gradient_config_defaults_are_disabled() {
        let g = GradientConfig::default();
        assert!(!g.is_enabled(), "default should not render a gradient");
        assert_eq!(g.angle, 180.0);
    }

    #[test]
    fn gradient_is_enabled_only_when_both_stops_set() {
        let g = GradientConfig {
            from: "#000000".to_string(),
            to: String::new(),
            angle: 0.0,
        };
        assert!(!g.is_enabled());
        let g = GradientConfig {
            from: "#000000".to_string(),
            to: "#ffffff".to_string(),
            angle: 0.0,
        };
        assert!(g.is_enabled());
    }

    #[test]
    fn gradient_round_trips_through_toml() {
        let toml_str = r##"
[window.gradient]
from = "#1a1a2e"
to = "#16213e"
angle = 90.0
"##;
        let parsed: crate::schema::Config = toml::from_str(toml_str).unwrap();
        let g = parsed.window.gradient.expect("gradient should parse");
        assert_eq!(g.from, "#1a1a2e");
        assert_eq!(g.to, "#16213e");
        assert!((g.angle - 90.0).abs() < 1e-6);
    }

    #[test]
    fn cursor_config_round_trips_through_toml() {
        let toml_str = r##"
[cursor]
blink_enabled = false
blink_interval_ms = 250
smooth_motion = false
"##;
        let parsed: crate::schema::Config = toml::from_str(toml_str).unwrap();
        assert!(!parsed.cursor.blink_enabled);
        assert_eq!(parsed.cursor.blink_interval_ms, 250);
        assert!(!parsed.cursor.smooth_motion);
    }
}
