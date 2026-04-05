//! 設定パネル — Ctrl+, でフローティング UI を表示する

use anyhow::Result;
use nexterm_config::toml_path;

/// 設定パネルの状態
pub struct SettingsPanel {
    pub is_open: bool,
    /// 選択中のタブ: 0=Font, 1=Colors, 2=Window
    pub tab: usize,
    /// フォントサイズ（スライダー値）
    pub font_size: f32,
    /// カラースキーム選択インデックス
    pub scheme_index: usize,
    /// 不透明度
    pub opacity: f32,
    /// 変更があるか
    pub dirty: bool,
    /// フォントファミリー名（表示用）
    pub font_family: String,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        let config = nexterm_config::Config::default();
        Self::new(&config)
    }
}

impl SettingsPanel {
    pub fn new(config: &nexterm_config::Config) -> Self {
        let scheme_index = scheme_name_to_index(&config.colors);
        Self {
            is_open: false,
            tab: 0,
            font_size: config.font.size,
            scheme_index,
            opacity: config.window.background_opacity,
            dirty: false,
            font_family: config.font.family.clone(),
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.dirty = false;
    }

    pub fn next_tab(&mut self) {
        self.tab = (self.tab + 1) % 3;
    }

    pub fn prev_tab(&mut self) {
        self.tab = if self.tab == 0 { 2 } else { self.tab - 1 };
    }

    pub fn increase_font_size(&mut self) {
        self.font_size = (self.font_size + 0.5).min(32.0);
        self.dirty = true;
    }

    pub fn decrease_font_size(&mut self) {
        self.font_size = (self.font_size - 0.5).max(8.0);
        self.dirty = true;
    }

    pub fn next_scheme(&mut self) {
        self.scheme_index = (self.scheme_index + 1) % 9;
        self.dirty = true;
    }

    pub fn prev_scheme(&mut self) {
        self.scheme_index = if self.scheme_index == 0 {
            8
        } else {
            self.scheme_index - 1
        };
        self.dirty = true;
    }

    pub fn increase_opacity(&mut self) {
        self.opacity = (self.opacity + 0.05).min(1.0);
        self.dirty = true;
    }

    pub fn decrease_opacity(&mut self) {
        self.opacity = (self.opacity - 0.05).max(0.1);
        self.dirty = true;
    }

    /// scheme_index からスキーム名を返す
    pub fn scheme_name(&self) -> &str {
        const SCHEMES: [&str; 9] = [
            "dark",
            "light",
            "tokyonight",
            "solarized",
            "gruvbox",
            "catppuccin",
            "dracula",
            "nord",
            "onedark",
        ];
        SCHEMES[self.scheme_index % 9]
    }

    /// 現在の設定を nexterm.toml に書き込む
    pub fn save_to_toml(&self) -> Result<()> {
        let path = toml_path();

        // 既存 TOML を読む（なければ空文字列から始める）
        let existing = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };

        let mut doc: toml_edit::DocumentMut = existing.parse().unwrap_or_default();

        // [font].size
        doc["font"]["size"] = toml_edit::value(self.font_size as f64);

        // [colors].scheme
        doc["colors"]["scheme"] = toml_edit::value(self.scheme_name());

        // [window].background_opacity
        doc["window"]["background_opacity"] = toml_edit::value(self.opacity as f64);

        // 親ディレクトリを作成する
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, doc.to_string())?;
        Ok(())
    }
}

/// カラースキームをインデックスに変換する
fn scheme_name_to_index(colors: &nexterm_config::ColorScheme) -> usize {
    use nexterm_config::{BuiltinScheme, ColorScheme};
    match colors {
        ColorScheme::Builtin(b) => match b {
            BuiltinScheme::Dark => 0,
            BuiltinScheme::Light => 1,
            BuiltinScheme::TokyoNight => 2,
            BuiltinScheme::Solarized => 3,
            BuiltinScheme::Gruvbox => 4,
            BuiltinScheme::Catppuccin => 5,
            BuiltinScheme::Dracula => 6,
            BuiltinScheme::Nord => 7,
            BuiltinScheme::OneDark => 8,
        },
        ColorScheme::Custom(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn default_state_from_config() {
        let config = Config::default();
        let panel = SettingsPanel::new(&config);
        assert!(!panel.is_open);
        assert_eq!(panel.tab, 0);
        assert!(!panel.dirty);
        assert_eq!(panel.font_size, config.font.size);
        assert_eq!(panel.opacity, config.window.background_opacity);
    }

    #[test]
    fn font_size_clamped() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.font_size = 32.0;
        panel.increase_font_size();
        assert_eq!(panel.font_size, 32.0, "上限 32.0 を超えてはいけない");

        panel.font_size = 8.0;
        panel.decrease_font_size();
        assert_eq!(panel.font_size, 8.0, "下限 8.0 を下回ってはいけない");

        panel.font_size = 14.0;
        panel.increase_font_size();
        assert!((panel.font_size - 14.5).abs() < f32::EPSILON);
        assert!(panel.dirty);
    }

    #[test]
    fn scheme_wraps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.scheme_index = 8;
        panel.next_scheme();
        assert_eq!(
            panel.scheme_index, 0,
            "インデックス 8 の次は 0 にラップする"
        );

        panel.scheme_index = 0;
        panel.prev_scheme();
        assert_eq!(
            panel.scheme_index, 8,
            "インデックス 0 の前は 8 にラップする"
        );
    }
}
