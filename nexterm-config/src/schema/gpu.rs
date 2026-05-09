//! GPU レンダラー設定・API バージョン・プロファイル

use serde::{Deserialize, Serialize};

use super::color::ColorScheme;
use super::font::FontConfig;
use super::shell::ShellConfig;
use super::window::TabBarConfig;

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
    pub fn apply_to(&self, base: &super::Config) -> super::Config {
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
