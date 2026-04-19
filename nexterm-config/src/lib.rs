#![warn(missing_docs)]
//! nexterm-config — Lua + TOML 2層設定システム
//!
//! ロード順序:
//!   1. ビルトインデフォルト値
//!   2. ~/.config/nexterm/nexterm.toml を読み込み
//!   3. ~/.config/nexterm/nexterm.lua が存在すれば実行してマージ
//!   4. ファイル変更監視 → ホットリロード

pub mod defaults;
pub mod keyring;
pub mod loader;
pub mod lua_hooks;
pub mod lua_worker;
pub mod schema;
pub mod status_bar;
pub mod watcher;

pub use loader::{ConfigLoader, lua_path, toml_path};
pub use lua_hooks::{HookEvent, LuaHookRunner};
pub use schema::{
    AccessLogConfig, BuiltinScheme, ColorScheme, Config, CustomPalette, FontConfig, GpuConfig,
    HooksConfig, HostConfig, KeyBinding, LogConfig, MacroConfig, OAuthConfig, Profile,
    SchemePalette, SerialPortConfig, ShellConfig, StatusBarConfig, TabBarConfig, TlsConfig,
    WebAuthConfig, WebConfig, WindowConfig, WindowDecorations,
};
pub use status_bar::{evaluate_builtin, StatusBarEvaluator, WidgetContext};
pub use watcher::{watch_config, ConfigRx};
