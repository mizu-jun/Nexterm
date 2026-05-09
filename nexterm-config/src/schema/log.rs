//! ログ・ステータスバーなど観測まわりの設定

use serde::{Deserialize, Serialize};

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
