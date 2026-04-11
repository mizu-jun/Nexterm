//! 設定パネル — Ctrl+, でフローティング UI を表示する（左サイドバー付き多カテゴリ設計）

use anyhow::Result;
use nexterm_config::toml_path;

/// サイドバーのカテゴリ
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsCategory {
    Startup,
    Font,
    Theme,
    Window,
    Ssh,
    Keybindings,
    Profiles,
}

impl SettingsCategory {
    pub const ALL: &'static [SettingsCategory] = &[
        SettingsCategory::Startup,
        SettingsCategory::Font,
        SettingsCategory::Theme,
        SettingsCategory::Window,
        SettingsCategory::Ssh,
        SettingsCategory::Keybindings,
        SettingsCategory::Profiles,
    ];

    pub fn label(&self) -> &str {
        match self {
            SettingsCategory::Startup => "スタートアップ",
            SettingsCategory::Font => "フォント",
            SettingsCategory::Theme => "テーマ",
            SettingsCategory::Window => "ウィンドウ",
            SettingsCategory::Ssh => "SSH",
            SettingsCategory::Keybindings => "キーバインド",
            SettingsCategory::Profiles => "プロファイル",
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            SettingsCategory::Startup => ">",
            SettingsCategory::Font => "F",
            SettingsCategory::Theme => "*",
            SettingsCategory::Window => "#",
            SettingsCategory::Ssh => "S",
            SettingsCategory::Keybindings => "K",
            SettingsCategory::Profiles => "P",
        }
    }
}

/// プロファイルエントリ（設定パネル内で編集可能）
#[derive(Debug, Clone)]
pub struct ProfileEntry {
    pub name: String,
    pub icon: String,
    pub shell_program: String,
    pub working_dir: String,
}

impl Default for ProfileEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            icon: ">".to_string(),
            shell_program: String::new(),
            working_dir: String::new(),
        }
    }
}

/// 設定パネルの状態
pub struct SettingsPanel {
    pub is_open: bool,
    /// 選択中のカテゴリ
    pub category: SettingsCategory,
    /// フォントサイズ（スライダー値）
    pub font_size: f32,
    /// カラースキーム選択インデックス
    pub scheme_index: usize,
    /// 不透明度
    pub opacity: f32,
    /// 変更があるか
    pub dirty: bool,
    /// フォントファミリー名（編集可能）
    pub font_family: String,
    /// フォントファミリー入力フィールドがフォーカスされているか
    pub font_family_editing: bool,
    /// プロファイル一覧
    pub profiles: Vec<ProfileEntry>,
    /// 選択中のプロファイルインデックス
    pub selected_profile: usize,
    /// 起動時セッション名
    pub startup_session: String,
    /// タブ名変更中のウィンドウ ID（None = 変更なし）
    pub tab_rename_editing: Option<u32>,
    /// タブ名変更中のテキスト
    pub tab_rename_text: String,
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
        // config.profiles から ProfileEntry を生成する
        let profiles: Vec<ProfileEntry> = config
            .profiles
            .iter()
            .map(|p| ProfileEntry {
                name: p.name.clone(),
                icon: p.icon.clone(),
                shell_program: p
                    .shell
                    .as_ref()
                    .map(|s| s.program.clone())
                    .unwrap_or_default(),
                working_dir: p.working_dir.clone().unwrap_or_default(),
            })
            .collect();
        Self {
            is_open: false,
            category: SettingsCategory::Font,
            font_size: config.font.size,
            scheme_index,
            opacity: config.window.background_opacity,
            dirty: false,
            font_family: config.font.family.clone(),
            font_family_editing: false,
            profiles,
            selected_profile: 0,
            startup_session: "main".to_string(),
            tab_rename_editing: None,
            tab_rename_text: String::new(),
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.dirty = false;
        self.font_family_editing = false;
        self.tab_rename_editing = None;
    }

    /// 左サイドバーの前のカテゴリへ移動する
    pub fn prev_category(&mut self) {
        let idx = Self::category_index(&self.category);
        let len = SettingsCategory::ALL.len();
        self.category = SettingsCategory::ALL[(idx + len - 1) % len].clone();
    }

    /// 左サイドバーの次のカテゴリへ移動する
    pub fn next_category(&mut self) {
        let idx = Self::category_index(&self.category);
        self.category = SettingsCategory::ALL[(idx + 1) % SettingsCategory::ALL.len()].clone();
    }

    fn category_index(cat: &SettingsCategory) -> usize {
        SettingsCategory::ALL
            .iter()
            .position(|c| c == cat)
            .unwrap_or(0)
    }

    /// 後方互換: tab インデックスでカテゴリを設定する（旧 API）
    pub fn next_tab(&mut self) {
        self.next_category();
    }

    pub fn prev_tab(&mut self) {
        self.prev_category();
    }

    /// フォントファミリー入力フィールドに文字を追加する
    pub fn push_font_family_char(&mut self, ch: char) {
        if self.font_family_editing {
            self.font_family.push(ch);
            self.dirty = true;
        }
    }

    /// フォントファミリー入力フィールドの末尾を削除する
    pub fn pop_font_family_char(&mut self) {
        if self.font_family_editing {
            self.font_family.pop();
            self.dirty = true;
        }
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

    /// タブ名変更を開始する
    pub fn begin_tab_rename(&mut self, window_id: u32, current_name: &str) {
        self.tab_rename_editing = Some(window_id);
        self.tab_rename_text = current_name.to_string();
    }

    /// タブ名変更をキャンセルする
    pub fn cancel_tab_rename(&mut self) {
        self.tab_rename_editing = None;
        self.tab_rename_text.clear();
    }

    /// タブ名変更中に文字を追加する
    pub fn push_tab_rename_char(&mut self, ch: char) {
        if self.tab_rename_editing.is_some() {
            self.tab_rename_text.push(ch);
        }
    }

    /// タブ名変更中に末尾を削除する
    pub fn pop_tab_rename_char(&mut self) {
        if self.tab_rename_editing.is_some() {
            self.tab_rename_text.pop();
        }
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

        // [font].family
        if !self.font_family.is_empty() {
            doc["font"]["family"] = toml_edit::value(self.font_family.as_str());
        }

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
        assert_eq!(panel.category, SettingsCategory::Font);
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

    #[test]
    fn tab_rename_lifecycle() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert!(panel.tab_rename_editing.is_none());

        panel.begin_tab_rename(42, "main");
        assert_eq!(panel.tab_rename_editing, Some(42));
        assert_eq!(panel.tab_rename_text, "main");

        panel.push_tab_rename_char('!');
        assert_eq!(panel.tab_rename_text, "main!");

        panel.pop_tab_rename_char();
        assert_eq!(panel.tab_rename_text, "main");

        panel.cancel_tab_rename();
        assert!(panel.tab_rename_editing.is_none());
        assert!(panel.tab_rename_text.is_empty());
    }

    #[test]
    fn category_navigation() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.category = SettingsCategory::Font;
        panel.next_category();
        assert_eq!(panel.category, SettingsCategory::Theme);
        panel.prev_category();
        assert_eq!(panel.category, SettingsCategory::Font);
    }
}
