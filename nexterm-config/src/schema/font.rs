//! フォント設定

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
