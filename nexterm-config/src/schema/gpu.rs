//! GPU renderer configuration, API version, and profiles.

use serde::{Deserialize, Serialize};

use super::color::ColorScheme;
use super::font::FontConfig;
use super::shell::ShellConfig;
use super::window::TabBarConfig;

/// Configuration API version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiVersion(pub String);

impl Default for ApiVersion {
    fn default() -> Self {
        Self("1.0".to_string())
    }
}

/// Named configuration profile that can override font / colors / shell.
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
    /// Profile name (must be unique).
    pub name: String,
    /// Icon displayed in tabs and the context menu (emoji or ASCII).
    #[serde(default)]
    pub icon: String,
    /// Font configuration (`None` = use `Config.font`).
    #[serde(default)]
    pub font: Option<FontConfig>,
    /// Color-scheme configuration (`None` = use `Config.colors`).
    #[serde(default)]
    pub colors: Option<ColorScheme>,
    /// Shell configuration (`None` = use `Config.shell`).
    #[serde(default)]
    pub shell: Option<ShellConfig>,
    /// Scrollback line count (`None` = use `Config`'s value).
    #[serde(default)]
    pub scrollback_lines: Option<usize>,
    /// Tab-bar configuration (`None` = use `Config.tab_bar`).
    #[serde(default)]
    pub tab_bar: Option<TabBarConfig>,
    /// Initial working directory (`None` = the default).
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Extra environment variables to set when launching the shell.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

impl Profile {
    /// Applies this profile to `base` and returns the resulting `Config`.
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

/// wgpu present-mode configuration.
///
/// Sprint 5-3 / C3: the default changed from `Fifo` to `Mailbox`. Mailbox is
/// tearing-free and saves roughly one frame of latency over Fifo
/// (~16 ms at 60 Hz). On environments that do not support it (some Linux
/// Wayland compositors, etc.) the renderer falls back to Fifo automatically
/// (see `select_present_mode` in `renderer/mod.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PresentModeConfig {
    /// Vertical sync (no tearing, but higher latency).
    Fifo,
    /// Queue only the latest frame (low latency; falls back to Fifo on
    /// unsupported environments). Default from Sprint 5-3 / C3 onward.
    #[default]
    Mailbox,
    /// Let the adapter pick the best mode.
    Auto,
}

/// GPU renderer configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GpuConfig {
    /// Path to a custom WGSL shader file for background rectangles (uses the
    /// built-in shader when omitted).
    ///
    /// The shader must implement `@vertex fn vs_main` / `@fragment fn fs_main`.
    /// Vertex input: `position: vec2<f32>`, `color: vec4<f32>`.
    ///
    /// Example: `custom_bg_shader = "~/.config/nexterm/shaders/crt.wgsl"`.
    #[serde(default)]
    pub custom_bg_shader: Option<String>,

    /// Path to a custom WGSL shader file for text (glyphs).
    ///
    /// Vertex input: `position: vec2<f32>`, `uv: vec2<f32>`, `color: vec4<f32>`.
    /// Bindings: `@group(0) @binding(0)` is `glyph_texture`,
    /// `@binding(1)` is `glyph_sampler`.
    #[serde(default)]
    pub custom_text_shader: Option<String>,

    /// Frame-rate cap (FPS). 0 = unlimited (default: 60).
    #[serde(default = "default_fps_limit")]
    pub fps_limit: u32,

    /// Square glyph-atlas size (pixels). Default: 2048.
    /// Raising it to 4096 helps on high-DPI displays or with very large fonts.
    #[serde(default = "default_atlas_size")]
    pub atlas_size: u32,

    /// wgpu present-mode configuration. Default from Sprint 5-3 / C3 onward:
    /// `mailbox` (low latency).
    /// `fifo`: vsync — tearing-free but higher latency (+~16 ms at 60 Hz).
    /// `mailbox`: low latency (falls back to `fifo` on unsupported environments).
    /// `auto`: the adapter chooses.
    #[serde(default)]
    pub present_mode: PresentModeConfig,
}

fn default_fps_limit() -> u32 {
    60
}

fn default_atlas_size() -> u32 {
    2048
}
