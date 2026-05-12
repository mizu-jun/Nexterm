//! 設定スキーマ定義（8 サブモジュールで構成）
//!
//! 詳細な型定義はサブモジュールに分割されており、本ファイルは [`Config`] と
//! デフォルトキーバインドを保持する。外部からの利用は `nexterm_config::*` の
//! 再 export 経由を推奨する。

pub mod color;
pub mod font;
pub mod gpu;
pub mod hosts;
pub mod log;
pub mod security;
pub mod shell;
pub mod web;
pub mod window;

pub use color::{BuiltinScheme, ColorScheme, CustomPalette, SchemePalette};
pub use font::FontConfig;
pub use gpu::{ApiVersion, GpuConfig, PresentModeConfig, Profile};
pub use hosts::{HooksConfig, HostConfig};
pub use log::{LogConfig, StatusBarConfig};
pub use security::{ConsentPolicy, SecurityConfig};
pub use shell::{KeyBinding, MacroConfig, SerialPortConfig, ShellConfig};
pub use web::{AccessLogConfig, OAuthConfig, TlsConfig, WebAuthConfig, WebConfig};
pub use window::{CursorStyle, TabBarConfig, WindowConfig, WindowDecorations};

use serde::{Deserialize, Serialize};

/// メイン設定構造体
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// 設定ロード時に発生したエラー一覧（Lua/TOML パースエラー）
    /// シリアライズ対象外 — ランタイムのみで使用する
    #[serde(skip)]
    pub config_errors: Vec<String>,

    /// 設定 API バージョン（SemVer 管理）
    #[serde(default)]
    pub api_version: ApiVersion,

    /// フォント設定
    #[serde(default)]
    pub font: FontConfig,

    /// カラースキーム
    #[serde(default)]
    pub colors: ColorScheme,

    /// シェル設定
    #[serde(default)]
    pub shell: ShellConfig,

    /// キーバインド
    #[serde(default)]
    pub keys: Vec<KeyBinding>,

    /// ステータスバー（Phase 3）
    #[serde(default)]
    pub status_bar: StatusBarConfig,

    /// スクロールバック行数
    #[serde(default = "default_scrollback")]
    pub scrollback_lines: usize,

    /// ウィンドウ設定（透過・ぼかし・装飾）
    #[serde(default)]
    pub window: WindowConfig,

    /// タブバー設定
    #[serde(default)]
    pub tab_bar: TabBarConfig,

    /// SSH ホスト一覧
    #[serde(default)]
    pub hosts: Vec<HostConfig>,

    /// Lua マクロ一覧（`[[macros]]` テーブルで定義）
    #[serde(default)]
    pub macros: Vec<MacroConfig>,

    /// シリアルポートプリセット一覧
    #[serde(default)]
    pub serial_ports: Vec<SerialPortConfig>,

    /// ログ設定
    #[serde(default)]
    pub log: LogConfig,

    /// ターミナルフック（イベント駆動シェルコマンド）
    #[serde(default)]
    pub hooks: HooksConfig,

    /// Web ターミナル設定（WebSocket + xterm.js）
    #[serde(default)]
    pub web: WebConfig,

    /// 名前付き設定プロファイル一覧
    #[serde(default)]
    pub profiles: Vec<Profile>,

    /// 現在アクティブなプロファイル名（None = デフォルト設定を使用）
    #[serde(default)]
    pub active_profile: Option<String>,

    /// WASM プラグインを格納するディレクトリ（None = デフォルトディレクトリを使用）
    /// デフォルト: `~/.config/nexterm/plugins`（Linux/macOS）/ `%APPDATA%\nexterm\plugins`（Windows）
    #[serde(default)]
    pub plugin_dir: Option<String>,

    /// プラグインを無効にするかどうか
    #[serde(default)]
    pub plugins_disabled: bool,

    /// GPU レンダラー設定
    #[serde(default)]
    pub gpu: GpuConfig,

    /// 表示言語（"auto" = OS 検出, "en"/"ja"/"fr"/"de"/"es"/"it"/"zh-CN"/"ko"）
    #[serde(default = "default_language")]
    pub language: String,

    /// カーソルの表示スタイル（"block"/"beam"/"underline"）。デフォルト: block
    #[serde(default)]
    pub cursor_style: CursorStyle,

    /// 起動時に GitHub Releases API で最新バージョンを確認するか（デフォルト: true）
    #[serde(default = "default_auto_check_update")]
    pub auto_check_update: bool,

    /// セキュリティ・同意ポリシー（外部 URL / OSC 52 / OSC 通知）
    #[serde(default)]
    pub security: SecurityConfig,
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
            auto_check_update: default_auto_check_update(),
            security: SecurityConfig::default(),
        }
    }
}

impl Config {
    /// アクティブプロファイルを適用した設定を返す。
    /// プロファイルが未設定または存在しない場合は self を clone して返す。
    pub fn effective(&self) -> Config {
        if let Some(ref name) = self.active_profile
            && let Some(profile) = self.profiles.iter().find(|p| &p.name == name)
        {
            return profile.apply_to(self);
        }
        self.clone()
    }

    /// 指定名のプロファイルをアクティブにする（存在しない名前は無視）
    pub fn activate_profile(&mut self, name: &str) {
        if self.profiles.iter().any(|p| p.name == name) {
            self.active_profile = Some(name.to_string());
        }
    }

    /// プロファイルをクリアしてデフォルト設定に戻す
    pub fn clear_active_profile(&mut self) {
        self.active_profile = None;
    }
}

/// デフォルトキーバインド（tmux 互換）
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
        // Sprint 5-4 / D8: tmux 流の Ctrl+B Z に加えて、初心者向けの直感的バインド
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
    fn デフォルト設定が生成できる() {
        let config = Config::default();
        assert_eq!(config.api_version.0, "1.0");
        assert!(config.font.size > 0.0);
        assert!(!config.shell.program.is_empty());
        assert!(config.scrollback_lines > 0);
    }

    #[test]
    fn フォントデフォルト値() {
        let font = FontConfig::default();
        assert!(font.ligatures);
        assert_eq!(font.size, 15.0);
    }

    /// Sprint 5-4 / D8: ToggleZoom はデフォルトキーバインドに 2 つ存在する
    /// （tmux 流 `Ctrl+B Z` + 初心者向け `Ctrl+Shift+Z`）
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
            "ToggleZoom は 2 つのデフォルトバインドを持つべき"
        );
        let keys: Vec<&str> = zoom_bindings.iter().map(|b| b.key.as_str()).collect();
        assert!(keys.contains(&"ctrl+b z"));
        assert!(keys.contains(&"ctrl+shift+z"));
    }

    /// Sprint 5-4 / D8: QuickSelect もキーバインド経由でアクセスできる
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
    fn tomlシリアライズ往復() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.font, parsed.font);
        assert_eq!(config.scrollback_lines, parsed.scrollback_lines);
    }

    #[test]
    fn プロファイルが設定に適用される() {
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
        // プロファイルで指定していない設定はベースのまま
        assert_eq!(effective.scrollback_lines, config.scrollback_lines);
    }

    #[test]
    fn 存在しないプロファイルは無視される() {
        let mut config = Config::default();
        config.activate_profile("non-existent");
        // 存在しない場合は active_profile が変わらない
        assert_eq!(config.active_profile, None);
    }

    #[test]
    fn プロファイルなしはベース設定をそのまま返す() {
        let config = Config::default();
        let effective = config.effective();
        assert_eq!(effective.font, config.font);
    }

    #[test]
    fn プロファイルをtrueでパースできる() {
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
    fn status_bar_config_デフォルト値() {
        let sb = StatusBarConfig::default();
        assert!(!sb.enabled);
        assert!(sb.widgets.is_empty());
        // right_widgets にデフォルトで "time" が含まれること
        assert!(sb.right_widgets.contains(&"time".to_string()));
        assert_eq!(sb.separator, "  ");
    }

    #[test]
    fn status_bar_config_toml往復() {
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
    fn plugin_dir_はデフォルトnone() {
        let config = Config::default();
        assert!(config.plugin_dir.is_none());
        assert!(!config.plugins_disabled);
    }

    #[test]
    fn window_config_デフォルト値() {
        let w = WindowConfig::default();
        assert!((w.background_opacity - 0.95).abs() < f32::EPSILON);
        assert_eq!(w.macos_window_background_blur, 0);
        assert_eq!(w.decorations, WindowDecorations::Full);
        assert_eq!(w.layout_mode, "bsp");
    }

    #[test]
    fn window_config_layout_modeをtomlで設定() {
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
    fn webconfig_の_allow_http_fallback_デフォルトは_false() {
        // CRITICAL #3 対応: 安全なデフォルトであることを保証する
        let cfg = WebConfig::default();
        assert!(
            !cfg.allow_http_fallback,
            "allow_http_fallback のデフォルトは false でなければならない（HTTP フォールバック禁止）"
        );
    }

    #[test]
    fn webconfig_は_toml_から_allow_http_fallback_を読める() {
        let toml_str = r#"
enabled = true
allow_http_fallback = true
"#;
        let cfg: WebConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.allow_http_fallback);
    }
}
