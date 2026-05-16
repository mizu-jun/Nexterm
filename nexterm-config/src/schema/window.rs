//! ウィンドウ・タブバー・カーソルなど表示まわりの設定

use serde::{Deserialize, Serialize};

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
    /// アクティビティのある非アクティブタブの背景色（RRGGBB）。
    /// Sprint 5-7 / UI-1-1: WezTerm の `format-tab-title` 相当でハイライト色を指定可能に。
    #[serde(default = "default_activity_tab_bg")]
    pub activity_tab_bg: String,
    /// アクティブタブ下端のアクセントライン色（RRGGBB）。
    #[serde(default = "default_active_accent_color")]
    pub active_accent_color: String,
    /// タブラベルにペイン番号を `[1]` 形式で前置するか（Windows Terminal 風）。
    #[serde(default)]
    pub show_tab_number: bool,
    /// 非アクティブタブのテキスト色をどれだけミュート（暗く）するか（0.0=暗い〜1.0=明るい）。
    /// デフォルト 0.55 で WezTerm の `#5c6d74` に近い暗さ。
    #[serde(default = "default_inactive_text_brightness")]
    pub inactive_text_brightness: f32,
    /// マウスホバー時にタブ背景を明るくするか。
    #[serde(default = "default_true")]
    pub hover_highlight: bool,
}

fn default_activity_tab_bg() -> String {
    // やや暖色気味のオレンジで activity をハイライト（WezTerm 流の `#ae8b2d` 近似）
    "#7A4D1A".to_string()
}

fn default_active_accent_color() -> String {
    // Tokyo Night blue (#7AA2F7)
    "#7AA2F7".to_string()
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
            // Tokyo Night アクセントカラー
            active_tab_bg: "#3B4261".to_string(),
            inactive_tab_bg: "#1E2030".to_string(),
            separator: "❯".to_string(),
            activity_tab_bg: default_activity_tab_bg(),
            active_accent_color: default_active_accent_color(),
            show_tab_number: false,
            inactive_text_brightness: default_inactive_text_brightness(),
            hover_highlight: true,
        }
    }
}
