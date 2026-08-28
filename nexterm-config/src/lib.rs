#![warn(missing_docs)]
//! nexterm-config — two-layer Lua + TOML configuration system.
//!
//! Load order:
//!   1. Built-in defaults.
//!   2. Read `~/.config/nexterm/nexterm.toml`.
//!   3. If `~/.config/nexterm/nexterm.lua` exists, run it and merge the result.
//!   4. Watch the files for changes → hot reload.

pub mod defaults;
pub mod keyring;
pub mod loader;
pub mod lua_hooks;
pub mod lua_sandbox;
pub mod lua_worker;
pub mod schema;
pub mod status_bar;
pub mod watcher;
pub mod wsl;

pub use loader::{ConfigLoader, lua_path, toml_path};
pub use lua_hooks::{HookEvent, LuaHookRunner};
pub use schema::{
    AccessLogConfig, AnimationIntensity, AnimationsConfig, BackdropTarget, BackgroundFit,
    BackgroundImageConfig, BlocksConfig, BuiltinScheme, CloseAction, ColorScheme, Config,
    ConsentPolicy, CubicBezier, CursorConfig, CursorStyle, CustomPalette, DesignTokens,
    ElevationScale, FontConfig, GpuConfig, GradientConfig, HooksConfig, HostConfig,
    InactivePaneHsbConfig, KeyBinding, LogConfig, MacroConfig, MetricTokens, MotionTokens,
    OAuthConfig, PresentModeConfig, Profile, QuakeEdge, QuakeModeConfig, RadiusTokens,
    ResolvedBackdrop, SchemePalette, SecurityConfig, SerialPortConfig, ShellConfig, SpacingRamp,
    StatusBarConfig, TabBarConfig, TlsConfig, TypeRamp, TypeStyle, UiConfig, WebAuthConfig,
    WebConfig, WindowBackdrop, WindowConfig, WindowDecorations, parse_hex_color, resolve_color,
};
pub use status_bar::{StatusBarEvaluator, WidgetContext, evaluate_builtin};
pub use watcher::{ConfigRx, watch_config};
