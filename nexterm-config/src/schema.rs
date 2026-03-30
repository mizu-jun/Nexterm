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
    /// フォントフォールバックチェーン（グリフが見つからない場合に順番に試行）
    #[serde(default)]
    pub font_fallbacks: Vec<String>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_string(),
            size: 14.0,
            ligatures: true,
            font_fallbacks: vec![],
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

/// カスタムカラーパレット（TOML で定義）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomPalette {
    /// 前景色 (#RRGGBB)
    pub foreground: String,
    /// 背景色 (#RRGGBB)
    pub background: String,
    /// カーソル色 (#RRGGBB)
    pub cursor: String,
    /// ANSI 16色 (#RRGGBB × 16: black, red, green, yellow, blue, magenta, cyan, white, bright×8)
    pub ansi: Vec<String>,
}

/// カラースキーム設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ColorScheme {
    /// 組み込みスキーム名
    Builtin(BuiltinScheme),
    /// カスタムパレット
    Custom(CustomPalette),
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
        {
            // PowerShell 7 → PowerShell 5 → cmd.exe の優先順でデフォルトシェルを選択する
            let (program, args) =
                if std::path::Path::new("C:\\Program Files\\PowerShell\\7\\pwsh.exe").exists() {
                    (
                        "C:\\Program Files\\PowerShell\\7\\pwsh.exe".to_string(),
                        vec!["-NoLogo".to_string()],
                    )
                } else if std::path::Path::new(
                    "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                )
                .exists()
                {
                    (
                        "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
                            .to_string(),
                        vec!["-NoLogo".to_string()],
                    )
                } else {
                    // 最終フォールバック: cmd.exe
                    (
                        "C:\\Windows\\System32\\cmd.exe".to_string(),
                        vec![],
                    )
                };
            return Self { program, args };
        }

        #[cfg(not(windows))]
        Self {
            program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            args: vec![],
        }
    }
}

/// ステータスバー設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
pub struct StatusBarConfig {
    /// ステータスバーを表示するか（Phase 3 で使用）
    pub enabled: bool,
    /// 表示する Lua ウィジェットリスト
    pub widgets: Vec<String>,
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

/// ログ設定
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct LogConfig {
    /// 自動ログ有効化
    #[serde(default)]
    pub auto_log: bool,
    /// ログ保存ディレクトリ
    pub log_dir: Option<String>,
    /// タイムスタンプ付きログ
    #[serde(default)]
    pub timestamp: bool,
    /// ANSI エスケープ除去
    #[serde(default)]
    pub strip_ansi: bool,
    /// ログファイル名テンプレート
    ///
    /// 利用可能なプレースホルダー:
    ///   {session}  — セッション名
    ///   {pane}     — ペイン ID
    ///   {datetime} — 起動時刻 (YYYYMMDD_HHMMSS)
    ///
    /// 例: "{session}_{pane}_{datetime}.log"
    /// デフォルト: None（ディレクトリ + 固定名）
    pub file_name_template: Option<String>,
    /// raw PTY バイト列をバイナリファイル (.bin) にも保存するか
    #[serde(default)]
    pub binary_log: bool,
}

/// シリアルポート設定（接続プリセット）
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SerialPortConfig {
    /// 表示名
    pub name: String,
    /// デバイスパス（例: "/dev/ttyUSB0", "COM3"）
    pub port: String,
    /// ボーレート
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    /// データビット: 5, 6, 7, 8
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    /// ストップビット: 1, 2
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    /// パリティ: "none", "odd", "even"
    #[serde(default = "default_parity")]
    pub parity: String,
}

fn default_baud_rate() -> u32 { 115200 }
fn default_data_bits() -> u8 { 8 }
fn default_stop_bits() -> u8 { 1 }
fn default_parity() -> String { "none".to_string() }

/// Lua マクロ定義（設定ファイルで [[macros]] として登録する）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroConfig {
    /// コマンドパレット / マクロピッカーに表示する名前
    pub name: String,
    /// マクロの説明文（オプション）
    #[serde(default)]
    pub description: String,
    /// nexterm.lua 内の Lua 関数名
    /// この関数は `function(session: string, pane_id: number) -> string` のシグネチャを持つ
    pub lua_fn: String,
}

/// キーバインド定義
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// キー文字列（例: "ctrl+shift+p"）
    pub key: String,
    /// アクション名（例: "CommandPalette"）
    pub action: String,
}

/// SSH ホスト設定
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct HostConfig {
    /// 表示名
    pub name: String,
    /// ホスト名または IP アドレス
    pub host: String,
    /// SSH ポート（デフォルト: 22）
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// ユーザー名
    pub username: String,
    /// 認証方式: "password", "key", "agent"
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    /// 秘密鍵ファイルパス（auth_type = "key" の場合）
    pub key_path: Option<String>,
    /// ローカルポートフォワーディング設定（例: "8080:localhost:80"）
    #[serde(default)]
    pub forward_local: Vec<String>,
    /// リモートポートフォワーディング設定（例: "9090:localhost:9090"）
    #[serde(default)]
    pub forward_remote: Vec<String>,
    /// ProxyJump ホスト名（hosts に登録されたエントリ名）
    pub proxy_jump: Option<String>,
    /// X11 フォワーディングを有効にするか（ssh -X 相当）
    #[serde(default)]
    pub x11_forward: bool,
    /// 信頼された X11 フォワーディング（ssh -Y 相当）
    #[serde(default)]
    pub x11_trusted: bool,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_auth_type() -> String {
    "key".to_string()
}

/// ターミナルフック設定 — イベント発生時に実行するシェルコマンドまたは Lua 関数
///
/// シェルコマンドフック: 文字列で指定（`sh -c` で実行）
///   `$NEXTERM_PANE_ID` / `$NEXTERM_SESSION` 環境変数が利用可能
///
/// Lua 関数フック: `lua_on_*` フィールドに Lua 関数名を指定
///   設定ファイル内で `function on_pane_open(session, pane_id) ... end` のように定義する
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// 新しいペインが開かれたときに実行するシェルコマンド
    pub on_pane_open: Option<String>,
    /// ペインが閉じられたときに実行するシェルコマンド
    pub on_pane_close: Option<String>,
    /// 新しいセッションが開始されたときに実行するシェルコマンド
    pub on_session_start: Option<String>,
    /// セッションにクライアントがアタッチしたときに実行するシェルコマンド
    pub on_attach: Option<String>,
    /// クライアントがセッションからデタッチしたときに実行するシェルコマンド
    pub on_detach: Option<String>,
    /// ペインが開かれたときに呼び出す Lua 関数名（例: "on_pane_open"）
    pub lua_on_pane_open: Option<String>,
    /// ペインが閉じられたときに呼び出す Lua 関数名
    pub lua_on_pane_close: Option<String>,
    /// セッション開始時に呼び出す Lua 関数名
    pub lua_on_session_start: Option<String>,
    /// アタッチ時に呼び出す Lua 関数名
    pub lua_on_attach: Option<String>,
    /// デタッチ時に呼び出す Lua 関数名
    pub lua_on_detach: Option<String>,
}

/// Web ターミナル設定（WebSocket + xterm.js）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebConfig {
    /// Web ターミナルを有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// 待ち受けポート（デフォルト: 7681）
    #[serde(default = "default_web_port")]
    pub port: u16,
    /// 認証トークン（None の場合は認証なし）
    pub token: Option<String>,
}

fn default_web_port() -> u16 {
    7681
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_web_port(),
            token: None,
        }
    }
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

    /// Lua マクロ一覧（[[macros]] テーブルで定義）
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
}

fn default_scrollback() -> usize {
    50_000
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
        KeyBinding {
            key: "ctrl+b z".to_string(),
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
