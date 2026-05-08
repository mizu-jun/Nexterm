//! 設定スキーマ定義

use serde::{Deserialize, Serialize};

/// フォント設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
            size: 15.0,
            ligatures: true,
            // プログラミングフォント → CJK → 絵文字 の順に試行
            font_fallbacks: vec![
                "Cascadia Code".to_string(),
                "JetBrains Mono".to_string(),
                "Fira Code".to_string(),
                "Noto Sans Mono CJK JP".to_string(),
                "Noto Color Emoji".to_string(),
            ],
        }
    }
}

/// カラースキームのパレット（前景・背景・ANSI 16色）
#[derive(Debug, Clone)]
pub struct SchemePalette {
    /// デフォルト前景色 [R, G, B]
    pub fg: [u8; 3],
    /// デフォルト背景色 [R, G, B]
    pub bg: [u8; 3],
    /// ANSI 16色パレット（0=black … 15=bright white）
    pub ansi: [[u8; 3]; 16],
}

/// 組み込みカラースキーム
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinScheme {
    /// ダークテーマ
    Dark,
    /// ライトテーマ
    Light,
    /// Tokyo Night テーマ
    TokyoNight,
    /// Solarized テーマ
    Solarized,
    /// Gruvbox テーマ
    Gruvbox,
    /// Catppuccin テーマ
    Catppuccin,
    /// Dracula テーマ
    Dracula,
    /// Nord テーマ
    Nord,
    #[serde(rename = "onedark")]
    /// One Dark テーマ
    OneDark,
}

impl BuiltinScheme {
    /// スキームの表示名を返す
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::TokyoNight => "Tokyo Night",
            Self::Solarized => "Solarized",
            Self::Gruvbox => "Gruvbox",
            Self::Catppuccin => "Catppuccin",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::OneDark => "One Dark",
        }
    }

    /// スキームのカラーパレットを返す
    pub fn palette(&self) -> SchemePalette {
        match self {
            Self::Dark => SchemePalette {
                fg: [0xD8, 0xD8, 0xD8],
                bg: [0x0D, 0x0D, 0x0D],
                ansi: [
                    [0x00, 0x00, 0x00],
                    [0x80, 0x00, 0x00],
                    [0x00, 0x80, 0x00],
                    [0x80, 0x80, 0x00],
                    [0x00, 0x00, 0x80],
                    [0x80, 0x00, 0x80],
                    [0x00, 0x80, 0x80],
                    [0xC0, 0xC0, 0xC0],
                    [0x80, 0x80, 0x80],
                    [0xFF, 0x00, 0x00],
                    [0x00, 0xFF, 0x00],
                    [0xFF, 0xFF, 0x00],
                    [0x00, 0x00, 0xFF],
                    [0xFF, 0x00, 0xFF],
                    [0x00, 0xFF, 0xFF],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::Light => SchemePalette {
                fg: [0x2C, 0x2C, 0x2C],
                bg: [0xF2, 0xF2, 0xF2],
                ansi: [
                    [0x00, 0x00, 0x00],
                    [0xC0, 0x00, 0x00],
                    [0x00, 0x80, 0x00],
                    [0x80, 0x80, 0x00],
                    [0x00, 0x00, 0xC0],
                    [0xC0, 0x00, 0xC0],
                    [0x00, 0x80, 0x80],
                    [0xC0, 0xC0, 0xC0],
                    [0x60, 0x60, 0x60],
                    [0xFF, 0x40, 0x40],
                    [0x00, 0xC0, 0x00],
                    [0xC0, 0xC0, 0x00],
                    [0x40, 0x40, 0xFF],
                    [0xFF, 0x40, 0xFF],
                    [0x00, 0xC0, 0xC0],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::TokyoNight => SchemePalette {
                fg: [0xC0, 0xCA, 0xF5],
                bg: [0x1A, 0x1B, 0x26],
                ansi: [
                    [0x15, 0x16, 0x20],
                    [0xF7, 0x76, 0x8E],
                    [0x9E, 0xCE, 0x6A],
                    [0xE0, 0xAF, 0x68],
                    [0x7A, 0xA2, 0xF7],
                    [0xBB, 0x9A, 0xF7],
                    [0x7D, 0xCF, 0xFF],
                    [0xA9, 0xB1, 0xD6],
                    [0x41, 0x4B, 0x67],
                    [0xF7, 0x76, 0x8E],
                    [0x9E, 0xCE, 0x6A],
                    [0xE0, 0xAF, 0x68],
                    [0x7A, 0xA2, 0xF7],
                    [0xBB, 0x9A, 0xF7],
                    [0x7D, 0xCF, 0xFF],
                    [0xC0, 0xCA, 0xF5],
                ],
            },
            Self::Solarized => SchemePalette {
                fg: [0x83, 0x94, 0x96],
                bg: [0x00, 0x2B, 0x36],
                ansi: [
                    [0x07, 0x36, 0x42],
                    [0xDC, 0x32, 0x2F],
                    [0x85, 0x99, 0x00],
                    [0xB5, 0x89, 0x00],
                    [0x26, 0x8B, 0xD2],
                    [0xD3, 0x36, 0x82],
                    [0x2A, 0xA1, 0x98],
                    [0xEE, 0xE8, 0xD5],
                    [0x00, 0x2B, 0x36],
                    [0xCB, 0x4B, 0x16],
                    [0x58, 0x6E, 0x75],
                    [0x65, 0x7B, 0x83],
                    [0x83, 0x94, 0x96],
                    [0x6C, 0x71, 0xC4],
                    [0x93, 0xA1, 0xA1],
                    [0xFD, 0xF6, 0xE3],
                ],
            },
            Self::Gruvbox => SchemePalette {
                fg: [0xEB, 0xDB, 0xB2],
                bg: [0x28, 0x28, 0x28],
                ansi: [
                    [0x28, 0x28, 0x28],
                    [0xCC, 0x24, 0x1D],
                    [0x98, 0x97, 0x1A],
                    [0xD7, 0x99, 0x21],
                    [0x45, 0x85, 0x88],
                    [0xB1, 0x62, 0x86],
                    [0x68, 0x9D, 0x6A],
                    [0xA8, 0x99, 0x84],
                    [0x92, 0x83, 0x74],
                    [0xFB, 0x49, 0x34],
                    [0xB8, 0xBB, 0x26],
                    [0xFA, 0xBD, 0x2F],
                    [0x83, 0xA5, 0x98],
                    [0xD3, 0x86, 0x9B],
                    [0x8E, 0xC0, 0x7C],
                    [0xEB, 0xDB, 0xB2],
                ],
            },
            Self::Catppuccin => SchemePalette {
                // Catppuccin Mocha
                fg: [0xCD, 0xD6, 0xF4],
                bg: [0x1E, 0x1E, 0x2E],
                ansi: [
                    [0x45, 0x47, 0x5A],
                    [0xF3, 0x8B, 0xA8],
                    [0xA6, 0xE3, 0xA1],
                    [0xF9, 0xE2, 0xAF],
                    [0x89, 0xB4, 0xFA],
                    [0xF5, 0xC2, 0xE7],
                    [0x94, 0xE2, 0xD5],
                    [0xBA, 0xC2, 0xDE],
                    [0x58, 0x5B, 0x70],
                    [0xF3, 0x8B, 0xA8],
                    [0xA6, 0xE3, 0xA1],
                    [0xF9, 0xE2, 0xAF],
                    [0x89, 0xB4, 0xFA],
                    [0xF5, 0xC2, 0xE7],
                    [0x94, 0xE2, 0xD5],
                    [0xA6, 0xAD, 0xC8],
                ],
            },
            Self::Dracula => SchemePalette {
                fg: [0xF8, 0xF8, 0xF2],
                bg: [0x28, 0x2A, 0x36],
                ansi: [
                    [0x21, 0x22, 0x2C],
                    [0xFF, 0x55, 0x55],
                    [0x50, 0xFA, 0x7B],
                    [0xF1, 0xFA, 0x8C],
                    [0xBD, 0x93, 0xF9],
                    [0xFF, 0x79, 0xC6],
                    [0x8B, 0xE9, 0xFD],
                    [0xF8, 0xF8, 0xF2],
                    [0x6B, 0x72, 0x89],
                    [0xFF, 0x6E, 0x6E],
                    [0x69, 0xFF, 0x94],
                    [0xFF, 0xFF, 0xA5],
                    [0xD6, 0xAC, 0xFF],
                    [0xFF, 0x92, 0xDF],
                    [0xA4, 0xFF, 0xFF],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
            Self::Nord => SchemePalette {
                fg: [0xD8, 0xDE, 0xE9],
                bg: [0x2E, 0x34, 0x40],
                ansi: [
                    [0x3B, 0x42, 0x52],
                    [0xBF, 0x61, 0x6A],
                    [0xA3, 0xBE, 0x8C],
                    [0xEB, 0xCB, 0x8B],
                    [0x81, 0xA1, 0xC1],
                    [0xB4, 0x8E, 0xAD],
                    [0x88, 0xC0, 0xD0],
                    [0xE5, 0xE9, 0xF0],
                    [0x4C, 0x56, 0x6A],
                    [0xBF, 0x61, 0x6A],
                    [0xA3, 0xBE, 0x8C],
                    [0xEB, 0xCB, 0x8B],
                    [0x81, 0xA1, 0xC1],
                    [0xB4, 0x8E, 0xAD],
                    [0x8F, 0xBD, 0xBB],
                    [0xEC, 0xEF, 0xF4],
                ],
            },
            Self::OneDark => SchemePalette {
                fg: [0xAB, 0xB2, 0xBF],
                bg: [0x28, 0x2C, 0x34],
                ansi: [
                    [0x28, 0x2C, 0x34],
                    [0xE0, 0x6C, 0x75],
                    [0x98, 0xC3, 0x79],
                    [0xE5, 0xC0, 0x7B],
                    [0x61, 0xAF, 0xEF],
                    [0xC6, 0x78, 0xDD],
                    [0x56, 0xB6, 0xC2],
                    [0xAB, 0xB2, 0xBF],
                    [0x5C, 0x63, 0x70],
                    [0xE0, 0x6C, 0x75],
                    [0x98, 0xC3, 0x79],
                    [0xE5, 0xC0, 0x7B],
                    [0x61, 0xAF, 0xEF],
                    [0xC6, 0x78, 0xDD],
                    [0x56, 0xB6, 0xC2],
                    [0xFF, 0xFF, 0xFF],
                ],
            },
        }
    }
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
///
/// TOML では以下の 3 形式を受け付ける（カスタム deserializer で対応）:
/// 1. `colors = "tokyonight"` — 文字列で組み込みスキーム指定
/// 2. `[colors] scheme = "tokyonight"` — テーブル形式（公式ドキュメント記載）
/// 3. `[colors] foreground = "#..." background = "#..." ...` — 完全カスタムパレット
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ColorScheme {
    /// 組み込みスキーム名
    Builtin(BuiltinScheme),
    /// カスタムパレット
    Custom(CustomPalette),
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::Builtin(BuiltinScheme::TokyoNight)
    }
}

impl<'de> Deserialize<'de> for ColorScheme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct ColorSchemeVisitor;

        impl<'de> Visitor<'de> for ColorSchemeVisitor {
            type Value = ColorScheme;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(
                    "string (組み込みスキーム名) または table ([colors] scheme = \"...\" / カスタムパレット)",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(ColorScheme::Builtin(parse_builtin_scheme(value)))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(ColorScheme::Builtin(parse_builtin_scheme(&value)))
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                // 一旦すべてのキーを HashMap に集める
                let mut entries: std::collections::HashMap<String, toml::Value> =
                    std::collections::HashMap::new();
                while let Some((key, value)) = map.next_entry::<String, toml::Value>()? {
                    entries.insert(key, value);
                }

                // パターン 2: [colors] scheme = "tokyonight"
                if let Some(scheme) = entries.get("scheme")
                    && let Some(name) = scheme.as_str()
                {
                    return Ok(ColorScheme::Builtin(parse_builtin_scheme(name)));
                }

                // パターン 3: フルカスタムパレット（foreground / background / cursor / ansi）
                let palette: CustomPalette = toml::Value::Table(entries.into_iter().collect())
                    .try_into()
                    .map_err(|e| {
                        de::Error::custom(format!("カスタムパレットのパースに失敗: {}", e))
                    })?;
                Ok(ColorScheme::Custom(palette))
            }
        }

        deserializer.deserialize_any(ColorSchemeVisitor)
    }
}

/// 組み込みスキーム名を `BuiltinScheme` にパースする。未知の値は Dark にフォールバック。
fn parse_builtin_scheme(s: &str) -> BuiltinScheme {
    match s.to_lowercase().as_str() {
        "dark" => BuiltinScheme::Dark,
        "light" => BuiltinScheme::Light,
        "tokyonight" => BuiltinScheme::TokyoNight,
        "solarized" => BuiltinScheme::Solarized,
        "gruvbox" => BuiltinScheme::Gruvbox,
        _ => BuiltinScheme::Dark,
    }
}

/// シェル設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
            // %ProgramFiles%\PowerShell\* を動的スキャンして最新バージョンを選択する
            let prog_files =
                std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
            let ps_root = std::path::Path::new(&prog_files).join("PowerShell");
            if let Ok(entries) = std::fs::read_dir(&ps_root) {
                let mut pwsh: Option<std::path::PathBuf> = None;
                for e in entries.flatten() {
                    let candidate = e.path().join("pwsh.exe");
                    if candidate.exists() && pwsh.as_ref().is_none_or(|p| candidate > *p) {
                        pwsh = Some(candidate);
                    }
                }
                if let Some(path) = pwsh {
                    return Self {
                        program: path.to_string_lossy().into_owned(),
                        args: vec!["-NoLogo".to_string()],
                    };
                }
            }
            // PowerShell 5 フォールバック
            let ps5 = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
            if std::path::Path::new(ps5).exists() {
                return Self {
                    program: ps5.to_string(),
                    args: vec!["-NoLogo".to_string()],
                };
            }
            // 最終フォールバック: cmd.exe
            Self {
                program: "C:\\Windows\\System32\\cmd.exe".to_string(),
                args: vec![],
            }
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
#[serde(default)]
pub struct StatusBarConfig {
    /// ステータスバーを表示するか
    pub enabled: bool,
    /// 左側に表示するウィジェット（ビルトインキーワードまたは Lua 式）
    ///
    /// ビルトインキーワード: "time", "date", "hostname", "session", "pane_id"
    /// それ以外は Lua 式として評価される
    #[serde(default)]
    pub widgets: Vec<String>,
    /// 右側に表示するウィジェット
    #[serde(default)]
    pub right_widgets: Vec<String>,
    /// ステータスバーの背景色（RRGGBB 形式、省略時はデフォルト）
    #[serde(default)]
    pub background_color: Option<String>,
    /// ウィジェット区切り文字（デフォルト: "  "）
    #[serde(default = "default_widget_separator")]
    pub separator: String,
}

fn default_widget_separator() -> String {
    "  ".to_string()
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            widgets: vec![],
            right_widgets: vec!["time".to_string()],
            background_color: None,
            separator: default_widget_separator(),
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
#[serde(default)]
pub struct WindowConfig {
    /// ウィンドウの不透明度（0.0 = 完全透明、1.0 = 不透明）
    #[serde(default = "default_background_opacity")]
    pub background_opacity: f32,
    /// macOS のウィンドウぼかし強度（0 = なし）
    #[serde(default)]
    pub macos_window_background_blur: u32,
    /// ウィンドウ装飾
    #[serde(default)]
    pub decorations: WindowDecorations,
    /// ペインレイアウトモード: "bsp"（手動分割・デフォルト）または "tiling"（均等自動配置）
    #[serde(default = "default_layout_mode")]
    pub layout_mode: String,
    /// ウィンドウ内の水平パディング（ピクセル）。デフォルト: 0
    #[serde(default)]
    pub padding_x: u32,
    /// ウィンドウ内の垂直パディング（ピクセル）。デフォルト: 0
    #[serde(default)]
    pub padding_y: u32,
}

fn default_background_opacity() -> f32 {
    // デフォルト 0.95（若干透過）。完全不透明にしたい場合は nexterm.toml で 1.0 に設定
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
        }
    }
}

/// カーソルの表示スタイル
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    /// ブロック型（全セルを塗りつぶす）
    #[default]
    Block,
    /// ビーム型（縦 2px の細線）
    Beam,
    /// アンダーライン型（横 2px の下線）
    Underline,
}

/// タブバー設定（WezTerm スタイル）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
            height: 32,
            // Tokyo Night アクセントカラー
            active_tab_bg: "#3B4261".to_string(),
            inactive_tab_bg: "#1E2030".to_string(),
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

fn default_baud_rate() -> u32 {
    115200
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_parity() -> String {
    "none".to_string()
}

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
    /// グループ名（ホストをカテゴリ分けするための任意文字列）
    #[serde(default)]
    pub group: String,
    /// タグ一覧（複数のラベルで絞り込みに使用する）
    #[serde(default)]
    pub tags: Vec<String>,
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

/// TOTP 認証設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebAuthConfig {
    /// TOTP 認証を有効にするか（デフォルト: false）
    #[serde(default)]
    pub totp_enabled: bool,
    /// TOTP シークレット（Base32 エンコード）。未設定の場合は初回起動時に生成してブラウザで設定
    pub totp_secret: Option<String>,
    /// 認証アプリに表示する発行者名（デフォルト: "Nexterm"）
    #[serde(default = "default_totp_issuer")]
    pub issuer: String,
    /// OAuth2 / OIDC 設定（設定された場合は TOTP より優先）
    #[serde(default)]
    pub oauth: OAuthConfig,
    /// セッション有効期限（秒）。デフォルト: 86400（24 時間）
    #[serde(default = "default_session_timeout_secs")]
    pub session_timeout_secs: u64,
}

fn default_totp_issuer() -> String {
    "Nexterm".to_string()
}

fn default_session_timeout_secs() -> u64 {
    86_400
}

impl Default for WebAuthConfig {
    fn default() -> Self {
        Self {
            totp_enabled: false,
            totp_secret: None,
            issuer: default_totp_issuer(),
            oauth: OAuthConfig::default(),
            session_timeout_secs: default_session_timeout_secs(),
        }
    }
}

/// OAuth2 / OIDC 認証設定
///
/// 対応プロバイダー: GitHub / Google / Azure AD / 任意の OIDC プロバイダー
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OAuthConfig {
    /// OAuth2 を有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// プロバイダー識別子: "github" | "google" | "azure" | "oidc"
    #[serde(default)]
    pub provider: String,
    /// クライアント ID
    pub client_id: Option<String>,
    /// クライアントシークレット（環境変数 NEXTERM_OAUTH_CLIENT_SECRET での上書き推奨）
    pub client_secret: Option<String>,
    /// OIDC ディスカバリー URL（provider = "oidc" の場合に使用）
    /// 例: "https://login.microsoftonline.com/{tenant}/v2.0"
    pub issuer_url: Option<String>,
    /// 許可するメールアドレスのリスト（空 = 全員許可）
    #[serde(default)]
    pub allowed_emails: Vec<String>,
    /// 許可する GitHub Organization 名のリスト（provider = "github" のみ）
    #[serde(default)]
    pub allowed_orgs: Vec<String>,
    /// OAuth2 コールバック URL（デフォルト: "http://localhost:{port}/auth/callback"）
    pub redirect_url: Option<String>,
}

/// TLS / HTTPS 設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TlsConfig {
    /// HTTPS を有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// 証明書ファイルパス（PEM）。省略時は自己署名証明書を自動生成
    pub cert_file: Option<String>,
    /// 秘密鍵ファイルパス（PEM）
    pub key_file: Option<String>,
}

/// アクセスログ設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AccessLogConfig {
    /// アクセスログを有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// ログファイルパス。省略時はサーバーログ（tracing）に出力
    pub file: Option<String>,
}

/// Web ターミナル設定（WebSocket + xterm.js）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    /// Web ターミナルを有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// 待ち受けポート（デフォルト: 7681）
    #[serde(default = "default_web_port")]
    pub port: u16,
    /// 認証トークン — 後方互換性のために残す（TOTP と併用不可）
    pub token: Option<String>,
    /// TOTP 認証設定
    #[serde(default)]
    pub auth: WebAuthConfig,
    /// TLS / HTTPS 設定
    #[serde(default)]
    pub tls: TlsConfig,
    /// HTTP アクセス時に HTTPS へ強制リダイレクトするか（デフォルト: false）
    /// tls.enabled = true の場合のみ有効
    #[serde(default)]
    pub force_https: bool,
    /// 同時セッション数の上限（0 = 無制限。デフォルト: 0）
    #[serde(default)]
    pub max_sessions: usize,
    /// アクセスログ設定
    #[serde(default)]
    pub access_log: AccessLogConfig,
    /// **危険**: TLS 設定失敗時に平文 HTTP でフォールバック起動を許可するか（デフォルト: false）
    ///
    /// `tls.enabled = true` で証明書ファイル不在・読み込み失敗・パーミッションエラー
    /// 等が起きた場合の挙動を制御する:
    /// - `false`（デフォルト・推奨）: サーバー起動を中止する。セッショントークンや
    ///   TOTP コードが平文で漏れることを防ぐ。
    /// - `true`: 警告ログを出して HTTP にフォールバックする（テスト・開発のみ）。
    #[serde(default)]
    pub allow_http_fallback: bool,
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
            auth: WebAuthConfig::default(),
            tls: TlsConfig::default(),
            force_https: false,
            max_sessions: 0,
            access_log: AccessLogConfig::default(),
            allow_http_fallback: false,
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

/// 名前付き設定プロファイル（フォント・カラー・シェルを上書きできる）
///
/// ```toml
/// [[profiles]]
/// name = "dark"
///
/// [profiles.font]
/// family = "Hack Nerd Font"
/// size = 14.0
///
/// [profiles.colors]
/// scheme = "catppuccin"
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Profile {
    /// プロファイル名（一意）
    pub name: String,
    /// タブ・コンテキストメニューに表示するアイコン（絵文字またはASCII文字）
    #[serde(default)]
    pub icon: String,
    /// フォント設定（None = Config の font を使用）
    #[serde(default)]
    pub font: Option<FontConfig>,
    /// カラースキーム設定（None = Config の colors を使用）
    #[serde(default)]
    pub colors: Option<ColorScheme>,
    /// シェル設定（None = Config の shell を使用）
    #[serde(default)]
    pub shell: Option<ShellConfig>,
    /// スクロールバック行数（None = Config の値を使用）
    #[serde(default)]
    pub scrollback_lines: Option<usize>,
    /// タブバー設定（None = Config の tab_bar を使用）
    #[serde(default)]
    pub tab_bar: Option<TabBarConfig>,
    /// 起動時の作業ディレクトリ（None = デフォルト）
    #[serde(default)]
    pub working_dir: Option<String>,
    /// 追加環境変数（シェル起動時に設定する）
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

impl Profile {
    /// このプロファイルを `base` Config に適用して新しい Config を返す
    pub fn apply_to(&self, base: &Config) -> Config {
        let mut result = base.clone();
        if let Some(font) = &self.font {
            result.font = font.clone();
        }
        if let Some(colors) = &self.colors {
            result.colors = colors.clone();
        }
        if let Some(shell) = &self.shell {
            result.shell = shell.clone();
        }
        if let Some(lines) = self.scrollback_lines {
            result.scrollback_lines = lines;
        }
        if let Some(tab_bar) = &self.tab_bar {
            result.tab_bar = tab_bar.clone();
        }
        result
    }
}

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
}

fn default_auto_check_update() -> bool {
    true
}

/// wgpu の Present Mode 設定
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PresentModeConfig {
    /// 垂直同期（ティアリングなし、レイテンシ高め）。デフォルト
    #[default]
    Fifo,
    /// 最新フレームのみキュー（低レイテンシ、非対応環境では Fifo にフォールバック）
    Mailbox,
    /// アダプタが最適なモードを自動選択
    Auto,
}

/// GPU レンダラー設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GpuConfig {
    /// 背景矩形用カスタム WGSL シェーダーファイルのパス（省略時はビルトイン使用）
    ///
    /// シェーダーは `@vertex fn vs_main` / `@fragment fn fs_main` を実装する必要がある。
    /// 頂点入力: position: vec2<f32>, color: vec4<f32>
    ///
    /// 例: `custom_bg_shader = "~/.config/nexterm/shaders/crt.wgsl"`
    #[serde(default)]
    pub custom_bg_shader: Option<String>,

    /// テキスト（グリフ）用カスタム WGSL シェーダーファイルのパス
    ///
    /// 頂点入力: position: vec2<f32>, uv: vec2<f32>, color: vec4<f32>
    /// バインディング: @group(0) @binding(0) glyph_texture, @binding(1) glyph_sampler
    #[serde(default)]
    pub custom_text_shader: Option<String>,

    /// フレームレート制限（FPS）。0 = 制限なし（デフォルト: 60）
    #[serde(default = "default_fps_limit")]
    pub fps_limit: u32,

    /// グリフアトラスのサイズ（ピクセル、正方形）。デフォルト: 2048
    /// 高DPI や大フォントサイズ使用時は 4096 に増やすと効果的
    #[serde(default = "default_atlas_size")]
    pub atlas_size: u32,

    /// wgpu Present Mode 設定。デフォルト: fifo（垂直同期）
    /// mailbox: 低レイテンシ（非対応環境では fifo にフォールバック）
    /// auto: アダプタ自動選択
    #[serde(default)]
    pub present_mode: PresentModeConfig,
}

fn default_fps_limit() -> u32 {
    60
}

fn default_atlas_size() -> u32 {
    2048
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
