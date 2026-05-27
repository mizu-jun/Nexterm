//! Font configuration.

use serde::{Deserialize, Serialize};

/// Font configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    /// Font family name.
    pub family: String,
    /// Font size in points.
    pub size: f32,
    /// Whether to enable ligatures.
    pub ligatures: bool,
    /// Font fallback chain (each entry is tried in order when a glyph is missing).
    #[serde(default)]
    pub font_fallbacks: Vec<String>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_string(),
            size: 15.0,
            ligatures: true,
            // Tried in order: programming fonts → CJK → emoji.
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
