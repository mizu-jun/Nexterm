//! 設定スキーマ定義

use serde::{Deserialize, Serialize};

/// フォント設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontConfig {
    /// フォントファミリー名
    pub family: String,
    /// フォントサイズ（pt）
    pub size: f32,
    /// リガチャを有効にするか
    pub ligatures: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_string(),
            size: 14.0,
            ligatures: true,
        }
    }
}

/// 組み込みカラースキーム
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinScheme {
    Dark,
    Light,
    TokyoNight,
    Solarized,
    Gruvbox,
}

/// カラースキーム設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ColorScheme {
    /// 組み込みスキーム名
    Builtin(BuiltinScheme),
    /// カスタム（将来拡張用）
    Custom(String),
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::Builtin(BuiltinScheme::Dark)
    }
}

/// シェル設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellConfig {
    /// シェルプログラムのパス
    pub program: String,
    /// シェルに渡す引数
    pub args: Vec<String>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        #[cfg(windows)]
        let program = if std::path::Path::new(
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
        )
        .exists()
        {
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe".to_string()
        } else {
            "powershell.exe".to_string()
        };

        #[cfg(not(windows))]
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        Self {
            program,
            args: vec![],
        }
    }
}

/// ステータスバー設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusBarConfig {
    /// ステータスバーを表示するか（Phase 3 で使用）
    pub enabled: bool,
    /// 表示する Lua ウィジェットリスト
    pub widgets: Vec<String>,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            widgets: vec![],
        }
    }
}

/// ウィンドウ装飾の種別
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WindowDecorations {
    /// OS 標準のタイトルバーと境界線を表示する
    #[default]
    Full,
    /// タイトルバーなし・境界線なし（ボーダーレス）
    None,
    /// タイトルバーのみ非表示
    NoTitle,
}

/// ウィンドウ設定（透過・ぼかし・装飾）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowConfig {
    /// ウィンドウの不透明度（0.0 = 完全透明、1.0 = 不透明）
    pub background_opacity: f32,
    /// macOS のウィンドウぼかし強度（0 = なし）
    pub macos_window_background_blur: u32,
    /// ウィンドウ装飾
    pub decorations: WindowDecorations,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            background_opacity: 1.0,
            macos_window_background_blur: 0,
            decorations: WindowDecorations::Full,
        }
    }
}

/// タブバー設定（WezTerm スタイル）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabBarConfig {
    /// タブバーを表示するか
    pub enabled: bool,
    /// タブバーの高さ（ピクセル）
    pub height: u32,
    /// アクティブタブの背景色（RRGGBB）
    pub active_tab_bg: String,
    /// 非アクティブタブの背景色（RRGGBB）
    pub inactive_tab_bg: String,
    /// タブセパレータ文字
    pub separator: String,
}

impl Default for TabBarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            height: 28,
            active_tab_bg: "#ae8b2d".to_string(),
            inactive_tab_bg: "#5c6d74".to_string(),
            separator: "❯".to_string(),
        }
    }
}

/// キーバインド定義
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// キー文字列（例: "ctrl+shift+p"）
    pub key: String,
    /// アクション名（例: "CommandPalette"）
    pub action: String,
}

/// 設定 API バージョン
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiVersion(pub String);

impl Default for ApiVersion {
    fn default() -> Self {
        Self("1.0".to_string())
    }
}

/// nexterm のトップレベル設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
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
}

fn default_scrollback() -> usize {
    50_000
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_version: ApiVersion::default(),
            font: FontConfig::default(),
            colors: ColorScheme::default(),
            shell: ShellConfig::default(),
            keys: default_keybindings(),
            status_bar: StatusBarConfig::default(),
            scrollback_lines: default_scrollback(),
            window: WindowConfig::default(),
            tab_bar: TabBarConfig::default(),
        }
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
        assert_eq!(font.size, 14.0);
    }

    #[test]
    fn tomlシリアライズ往復() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.font, parsed.font);
        assert_eq!(config.scrollback_lines, parsed.scrollback_lines);
    }
}
