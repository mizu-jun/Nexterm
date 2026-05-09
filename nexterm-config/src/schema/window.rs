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
