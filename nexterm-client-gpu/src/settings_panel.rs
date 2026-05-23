//! 設定パネル — Ctrl+, でフローティング UI を表示する（左サイドバー付き多カテゴリ設計）

use anyhow::Result;
use nexterm_config::toml_path;

/// スライダーの種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderType {
    FontSize,
    WindowOpacity,
    /// Phase 5-11-6 #6: ウィンドウ内水平パディング (0〜32 px)
    WindowPaddingX,
    /// Phase 5-11-6 #6: ウィンドウ内垂直パディング (0〜32 px)
    WindowPaddingY,
}

/// マウスドラッグ中のスライダー状態
#[derive(Debug, Clone)]
pub struct SliderDrag {
    /// どのスライダーをドラッグしているか
    pub slider_type: SliderType,
    /// スライダートラックの開始 X 座標（ピクセル）
    pub track_x: f32,
    /// スライダートラックの幅（ピクセル）
    pub track_w: f32,
    /// スライダーの最小値
    #[allow(dead_code)]
    pub min_val: f32,
    /// スライダーの最大値
    #[allow(dead_code)]
    pub max_val: f32,
}

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
            SettingsCategory::Startup => "▶",
            SettingsCategory::Font => "Aa",
            SettingsCategory::Theme => "◐",
            SettingsCategory::Window => "▢",
            SettingsCategory::Ssh => "⊞",
            SettingsCategory::Keybindings => "⌨",
            SettingsCategory::Profiles => "◉",
        }
    }
}

/// プロファイルエントリ（設定パネル内で編集可能）
#[derive(Debug, Clone)]
pub struct ProfileEntry {
    pub name: String,
    pub icon: String,
    #[allow(dead_code)]
    pub shell_program: String,
    #[allow(dead_code)]
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

/// SSH ホストエントリ（Phase 5-11-8 Step 8-1: 設定パネル内で表示専用）
///
/// `nexterm-config::HostConfig` のうち SR / 設定パネルでの表示に必要な
/// フィールドだけを抜き出した軽量な構造。
/// Step 8-2 / 8-3 で編集機能を追加する際にフィールドを増やす予定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHostEntry {
    /// 表示名（`HostConfig.name`）
    pub name: String,
    /// ホスト名または IP アドレス
    pub host: String,
    /// SSH ポート
    pub port: u16,
    /// ユーザー名
    pub username: String,
    /// 認証方式（"password" / "key" / "agent"）
    pub auth_type: String,
}

impl SshHostEntry {
    /// SR / UI で読み上げ / 描画する 1 行ラベルを生成する。
    /// 例: `"myhost (alice@example.com:2222)"`
    pub fn label(&self) -> String {
        let user_part = if self.username.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.username, self.host)
        };
        let endpoint = if self.port == 22 || self.port == 0 {
            user_part
        } else {
            format!("{}:{}", user_part, self.port)
        };
        if self.name.is_empty() {
            endpoint
        } else {
            format!("{} ({})", self.name, endpoint)
        }
    }
}

/// 設定パネルの状態
pub struct SettingsPanel {
    pub is_open: bool,
    /// 開閉アニメーションの進行度（0.0 = 完全に閉じた状態, 1.0 = 完全に開いた状態）
    /// 毎フレーム renderer 側で加算される
    pub open_progress: f32,
    /// マウスドラッグ中のスライダー（None = ドラッグ中でない）
    pub drag_slider: Option<SliderDrag>,
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
    /// SSH ホスト一覧（Phase 5-11-8 Step 8-1: 表示専用、`config.hosts` から生成）
    pub ssh_hosts: Vec<SshHostEntry>,
    /// 選択中の SSH ホストインデックス（`ssh_hosts` 内）
    pub selected_host_index: usize,
    /// 起動時セッション名
    #[allow(dead_code)]
    pub startup_session: String,
    /// タブ名変更中のウィンドウ ID（None = 変更なし）
    pub tab_rename_editing: Option<u32>,
    /// タブ名変更中のテキスト
    pub tab_rename_text: String,
    /// 言語選択インデックス（LANGUAGE_OPTIONS の位置）
    pub language_index: usize,
    /// 起動時に更新確認を行うか
    pub auto_check_update: bool,
    /// カーソル形状（Phase 5-11-6 #6）。block / beam / underline。
    /// 保存時は TOML の top-level `cursor_style` に書き戻す。
    pub cursor_style: nexterm_config::CursorStyle,
    /// ウィンドウ内の水平パディング（ピクセル、0〜32）。
    /// 保存時は TOML の `[window].padding_x` に書き戻す。
    pub padding_x: u32,
    /// ウィンドウ内の垂直パディング（ピクセル、0〜32）。
    pub padding_y: u32,
    /// GPU プレゼンテーションモード（fifo / mailbox / auto）。
    /// 保存時は TOML の `[gpu].present_mode` に書き戻す。
    pub present_mode: nexterm_config::PresentModeConfig,
    /// Phase 5-11-6 #6: Window カテゴリ内のフォーカス中フィールド。
    /// 0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode
    pub window_field_focus: u8,
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
        // Phase 5-11-8 Step 8-1: config.hosts から SshHostEntry を生成する
        let ssh_hosts: Vec<SshHostEntry> = config
            .hosts
            .iter()
            .map(|h| SshHostEntry {
                name: h.name.clone(),
                host: h.host.clone(),
                port: h.port,
                username: h.username.clone(),
                auth_type: h.auth_type.clone(),
            })
            .collect();
        let language_index = LANGUAGE_OPTIONS
            .iter()
            .position(|(_, code)| *code == config.language.as_str())
            .unwrap_or(0);
        Self {
            is_open: false,
            open_progress: 0.0,
            drag_slider: None,
            category: SettingsCategory::Font,
            font_size: config.font.size,
            scheme_index,
            opacity: config.window.background_opacity,
            dirty: false,
            font_family: config.font.family.clone(),
            font_family_editing: false,
            profiles,
            selected_profile: 0,
            ssh_hosts,
            selected_host_index: 0,
            startup_session: "main".to_string(),
            tab_rename_editing: None,
            tab_rename_text: String::new(),
            language_index,
            auto_check_update: config.auto_check_update,
            cursor_style: config.cursor_style.clone(),
            // padding_x / padding_y は config では u32 だが UI 上は 0〜32 にクランプ
            padding_x: config.window.padding_x.min(32),
            padding_y: config.window.padding_y.min(32),
            present_mode: config.gpu.present_mode.clone(),
            window_field_focus: 0,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        // open_progress は 0 から始めてアニメーションを再生する
        self.open_progress = 0.0;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.open_progress = 0.0;
        self.drag_slider = None;
        self.dirty = false;
        self.font_family_editing = false;
        self.tab_rename_editing = None;
    }

    /// スライダー X 座標からフォントサイズを設定する（マウスクリック/ドラッグ用）
    pub fn set_font_size_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        // フォントサイズ範囲: 8.0〜32.0 (24.0 の範囲、0.5 単位に丸める)
        let raw = 8.0 + ratio * 24.0;
        self.font_size = (raw * 2.0).round() / 2.0;
        self.dirty = true;
    }

    /// スライダー X 座標から不透明度を設定する（マウスクリック/ドラッグ用）
    pub fn set_opacity_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        // 不透明度範囲: 0.1〜1.0 (5% 単位に丸める)
        let raw = 0.1 + ratio * 0.9;
        self.opacity = (raw * 20.0).round() / 20.0;
        self.dirty = true;
    }

    /// Phase 5-11-6 #6: スライダー X 座標から padding_x (0〜32 px) を設定する
    pub fn set_padding_x_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        self.padding_x = (ratio * 32.0).round() as u32;
        self.dirty = true;
    }

    /// Phase 5-11-6 #6: スライダー X 座標から padding_y (0〜32 px) を設定する
    pub fn set_padding_y_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        self.padding_y = (ratio * 32.0).round() as u32;
        self.dirty = true;
    }

    /// イーズアウトキュービック: t^3 の逆関数でスムーズな減速を表現する
    pub fn eased_progress(&self) -> f32 {
        let t = self.open_progress.clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
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
    #[allow(dead_code)]
    pub fn next_tab(&mut self) {
        self.next_category();
    }

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn decrease_opacity(&mut self) {
        self.opacity = (self.opacity - 0.05).max(0.1);
        self.dirty = true;
    }

    /// SR の `Action::SetValue(NumericValue)` 経路用: f64 値を 0.5 単位に丸めて
    /// 8.0〜32.0 にクランプしてフォントサイズに設定する。
    ///
    /// マウスドラッグ経路（`set_font_size_from_slider`）とは入力（ピクセル座標 vs 直接値）が
    /// 異なるが、丸めと clamp の幅は共通。
    pub fn set_font_size_value(&mut self, v: f64) {
        let raw = (v as f32).clamp(8.0, 32.0);
        self.font_size = (raw * 2.0).round() / 2.0;
        self.dirty = true;
    }

    /// SR の `Action::SetValue(NumericValue)` 経路用: f64 値を 0.05 単位に丸めて
    /// 0.1〜1.0 にクランプして不透明度に設定する。
    pub fn set_opacity_value(&mut self, v: f64) {
        let raw = (v as f32).clamp(0.1, 1.0);
        self.opacity = (raw * 20.0).round() / 20.0;
        self.dirty = true;
    }

    /// SR の `Action::Click` 経路用: 自動更新確認チェックボックスをトグル。
    pub fn toggle_auto_check_update(&mut self) {
        self.auto_check_update = !self.auto_check_update;
        self.dirty = true;
    }

    // ===== Phase 5-11-6 #6: カーソルスタイル =====
    //
    // Block / Beam / Underline の 3 値を循環させる。
    // 保存時は TOML の `cursor_style = "block" | "beam" | "underline"` に書き戻す。
    //
    pub fn next_cursor_style(&mut self) {
        use nexterm_config::CursorStyle::*;
        self.cursor_style = match self.cursor_style {
            Block => Beam,
            Beam => Underline,
            Underline => Block,
        };
        self.dirty = true;
    }

    pub fn prev_cursor_style(&mut self) {
        use nexterm_config::CursorStyle::*;
        self.cursor_style = match self.cursor_style {
            Block => Underline,
            Beam => Block,
            Underline => Beam,
        };
        self.dirty = true;
    }

    /// 列挙順序のインデックス（0=Block, 1=Beam, 2=Underline）。UI 描画と AccessKit
    /// `Action::SetValue` 経路で使う（現状はテスト経由のみ）。
    #[allow(dead_code)]
    pub fn cursor_style_index(&self) -> usize {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => 0,
            Beam => 1,
            Underline => 2,
        }
    }

    /// UI に表示するラベル（日本語 + 英語併記）。
    pub fn cursor_style_label(&self) -> &'static str {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => "ブロック / Block",
            Beam => "ビーム / Beam",
            Underline => "アンダーライン / Underline",
        }
    }

    /// TOML 書き戻し用の小文字キー（serde の `rename_all = "lowercase"` に揃える）。
    pub fn cursor_style_toml_key(&self) -> &'static str {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => "block",
            Beam => "beam",
            Underline => "underline",
        }
    }

    // ===== Phase 5-11-6 #6: ウィンドウパディング =====
    //
    // 0〜32 ピクセル。1 px 単位で増減できる。SR の `Action::SetValue(NumericValue)`
    // 経路は f64 を u32 に丸めて clamp する。

    pub fn set_padding_x_value(&mut self, v: f64) {
        self.padding_x = (v.round().clamp(0.0, 32.0)) as u32;
        self.dirty = true;
    }

    pub fn increase_padding_x(&mut self) {
        self.padding_x = (self.padding_x + 1).min(32);
        self.dirty = true;
    }

    pub fn decrease_padding_x(&mut self) {
        self.padding_x = self.padding_x.saturating_sub(1);
        self.dirty = true;
    }

    pub fn set_padding_y_value(&mut self, v: f64) {
        self.padding_y = (v.round().clamp(0.0, 32.0)) as u32;
        self.dirty = true;
    }

    pub fn increase_padding_y(&mut self) {
        self.padding_y = (self.padding_y + 1).min(32);
        self.dirty = true;
    }

    pub fn decrease_padding_y(&mut self) {
        self.padding_y = self.padding_y.saturating_sub(1);
        self.dirty = true;
    }

    // ===== Phase 5-11-6 #6: プレゼンテーションモード =====
    //
    // Fifo / Mailbox / Auto の 3 値を循環させる。
    // 保存時は TOML の `[gpu].present_mode` に書き戻す。

    pub fn next_present_mode(&mut self) {
        use nexterm_config::PresentModeConfig::*;
        self.present_mode = match self.present_mode {
            Fifo => Mailbox,
            Mailbox => Auto,
            Auto => Fifo,
        };
        self.dirty = true;
    }

    pub fn prev_present_mode(&mut self) {
        use nexterm_config::PresentModeConfig::*;
        self.present_mode = match self.present_mode {
            Fifo => Auto,
            Mailbox => Fifo,
            Auto => Mailbox,
        };
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn present_mode_index(&self) -> usize {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => 0,
            Mailbox => 1,
            Auto => 2,
        }
    }

    pub fn present_mode_label(&self) -> &'static str {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => "Fifo (垂直同期 / 高互換性)",
            Mailbox => "Mailbox (低遅延 / 推奨)",
            Auto => "Auto (環境依存)",
        }
    }

    pub fn present_mode_toml_key(&self) -> &'static str {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => "fifo",
            Mailbox => "mailbox",
            Auto => "auto",
        }
    }

    // ===== Phase 5-11-6 #6: Window カテゴリ内フィールドフォーカス =====
    //
    // 0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode
    // ↑/↓ でフィールド間移動、←/→ でフォーカス中フィールドの値を変更する。

    /// Window カテゴリ内のフィールド総数
    pub const WINDOW_FIELD_COUNT: u8 = 5;

    /// 次のフィールドにフォーカスを移す（最後で停止）。
    /// 戻り値: 移動できたら true、すでに最後なら false（カテゴリ移動の判断に使う）。
    pub fn next_window_field(&mut self) -> bool {
        if self.window_field_focus + 1 < Self::WINDOW_FIELD_COUNT {
            self.window_field_focus += 1;
            true
        } else {
            false
        }
    }

    /// 前のフィールドにフォーカスを移す（先頭で停止）。
    pub fn prev_window_field(&mut self) -> bool {
        if self.window_field_focus > 0 {
            self.window_field_focus -= 1;
            true
        } else {
            false
        }
    }

    /// フォーカス中フィールドの値を増加させる（←/→ の右、または ↑ の Window カテゴリ補助）。
    pub fn window_field_increase(&mut self) {
        match self.window_field_focus {
            0 => self.increase_opacity(),
            1 => self.next_cursor_style(),
            2 => self.increase_padding_x(),
            3 => self.increase_padding_y(),
            4 => self.next_present_mode(),
            _ => {}
        }
    }

    /// フォーカス中フィールドの値を減少させる。
    pub fn window_field_decrease(&mut self) {
        match self.window_field_focus {
            0 => self.decrease_opacity(),
            1 => self.prev_cursor_style(),
            2 => self.decrease_padding_x(),
            3 => self.decrease_padding_y(),
            4 => self.prev_present_mode(),
            _ => {}
        }
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

    /// 現在選択中の言語コードを返す
    pub fn language_code(&self) -> &str {
        LANGUAGE_OPTIONS
            .get(self.language_index)
            .map(|(_, code)| *code)
            .unwrap_or("auto")
    }

    /// 次の言語に切り替える
    pub fn next_language(&mut self) {
        self.language_index = (self.language_index + 1) % LANGUAGE_OPTIONS.len();
        self.dirty = true;
    }

    /// 前の言語に切り替える
    pub fn prev_language(&mut self) {
        let len = LANGUAGE_OPTIONS.len();
        self.language_index = (self.language_index + len - 1) % len;
        self.dirty = true;
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

        // [window].padding_x / padding_y（Phase 5-11-6 #6）
        doc["window"]["padding_x"] = toml_edit::value(self.padding_x as i64);
        doc["window"]["padding_y"] = toml_edit::value(self.padding_y as i64);

        // [gpu].present_mode（Phase 5-11-6 #6）
        doc["gpu"]["present_mode"] = toml_edit::value(self.present_mode_toml_key());

        // cursor_style（Phase 5-11-6 #6）
        doc["cursor_style"] = toml_edit::value(self.cursor_style_toml_key());

        // language
        doc["language"] = toml_edit::value(self.language_code());

        // auto_check_update
        doc["auto_check_update"] = toml_edit::value(self.auto_check_update);

        // 親ディレクトリを作成する
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, doc.to_string())?;
        Ok(())
    }
}

/// 言語選択肢: (表示名, コード)
pub const LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("Auto (OS)", "auto"),
    ("English", "en"),
    ("日本語", "ja"),
    ("Français", "fr"),
    ("Deutsch", "de"),
    ("Español", "es"),
    ("Italiano", "it"),
    ("中文(简体)", "zh-CN"),
    ("한국어", "ko"),
];

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

    // ===== Phase 5-11-6 #6: cursor_style / padding / present_mode =====

    #[test]
    fn cursor_style_cycle_forward_and_back() {
        use nexterm_config::CursorStyle::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        // Default は Block
        assert_eq!(panel.cursor_style, Block);
        assert_eq!(panel.cursor_style_index(), 0);
        assert_eq!(panel.cursor_style_toml_key(), "block");

        panel.next_cursor_style();
        assert_eq!(panel.cursor_style, Beam);
        assert_eq!(panel.cursor_style_index(), 1);
        assert_eq!(panel.cursor_style_toml_key(), "beam");

        panel.next_cursor_style();
        assert_eq!(panel.cursor_style, Underline);
        assert_eq!(panel.cursor_style_toml_key(), "underline");

        panel.next_cursor_style();
        assert_eq!(panel.cursor_style, Block, "Underline の次は Block にラップ");

        // 逆方向
        panel.prev_cursor_style();
        assert_eq!(panel.cursor_style, Underline, "Block の前は Underline");
        panel.prev_cursor_style();
        assert_eq!(panel.cursor_style, Beam);
        panel.prev_cursor_style();
        assert_eq!(panel.cursor_style, Block);

        assert!(panel.dirty);
    }

    #[test]
    fn cursor_style_labels_are_human_readable() {
        use nexterm_config::CursorStyle::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.cursor_style = Block;
        assert!(panel.cursor_style_label().contains("Block"));
        panel.cursor_style = Beam;
        assert!(panel.cursor_style_label().contains("Beam"));
        panel.cursor_style = Underline;
        assert!(panel.cursor_style_label().contains("Underline"));
    }

    #[test]
    fn padding_x_increase_decrease_clamps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert_eq!(panel.padding_x, 0, "デフォルトは 0");

        // 上限 32 でクランプ
        for _ in 0..40 {
            panel.increase_padding_x();
        }
        assert_eq!(panel.padding_x, 32);

        // 下限 0 でクランプ
        for _ in 0..40 {
            panel.decrease_padding_x();
        }
        assert_eq!(panel.padding_x, 0);

        assert!(panel.dirty);
    }

    #[test]
    fn padding_y_increase_decrease_clamps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        for _ in 0..50 {
            panel.increase_padding_y();
        }
        assert_eq!(panel.padding_y, 32, "上限");
        for _ in 0..50 {
            panel.decrease_padding_y();
        }
        assert_eq!(panel.padding_y, 0, "下限");
    }

    #[test]
    fn padding_set_value_clamps_and_rounds() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.set_padding_x_value(-5.0);
        assert_eq!(panel.padding_x, 0, "負値は 0 にクランプ");
        panel.set_padding_x_value(100.0);
        assert_eq!(panel.padding_x, 32, "上限超は 32 にクランプ");
        panel.set_padding_x_value(15.7);
        assert_eq!(panel.padding_x, 16, "0.5 以上は切り上げ丸め");
        panel.set_padding_x_value(15.3);
        assert_eq!(panel.padding_x, 15, "0.5 未満は切り捨て丸め");

        panel.set_padding_y_value(7.5);
        assert_eq!(
            panel.padding_y, 8,
            "0.5 はバンカーズか四捨五入のいずれか（実装依存）"
        );
    }

    #[test]
    fn present_mode_cycle_forward_and_back() {
        use nexterm_config::PresentModeConfig::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        // Default は Mailbox（Sprint 5-3 / C3 で変更済み）
        assert_eq!(panel.present_mode, Mailbox);
        assert_eq!(panel.present_mode_index(), 1);
        assert_eq!(panel.present_mode_toml_key(), "mailbox");

        panel.next_present_mode();
        assert_eq!(panel.present_mode, Auto);
        panel.next_present_mode();
        assert_eq!(panel.present_mode, Fifo);
        panel.next_present_mode();
        assert_eq!(panel.present_mode, Mailbox);

        // 逆方向
        panel.prev_present_mode();
        assert_eq!(panel.present_mode, Fifo);

        assert!(panel.dirty);
    }

    #[test]
    fn present_mode_labels_are_human_readable() {
        use nexterm_config::PresentModeConfig::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.present_mode = Fifo;
        assert!(panel.present_mode_label().contains("Fifo"));
        panel.present_mode = Mailbox;
        assert!(panel.present_mode_label().contains("Mailbox"));
        panel.present_mode = Auto;
        assert!(panel.present_mode_label().contains("Auto"));
    }

    #[test]
    fn new_reads_config_window_padding_and_present_mode() {
        let mut config = Config::default();
        config.window.padding_x = 12;
        config.window.padding_y = 4;
        config.gpu.present_mode = nexterm_config::PresentModeConfig::Fifo;
        config.cursor_style = nexterm_config::CursorStyle::Beam;

        let panel = SettingsPanel::new(&config);
        assert_eq!(panel.padding_x, 12);
        assert_eq!(panel.padding_y, 4);
        assert_eq!(panel.present_mode, nexterm_config::PresentModeConfig::Fifo);
        assert_eq!(panel.cursor_style, nexterm_config::CursorStyle::Beam);
    }

    #[test]
    fn new_clamps_oversized_padding_from_config() {
        let mut config = Config::default();
        config.window.padding_x = 1000;
        let panel = SettingsPanel::new(&config);
        assert_eq!(
            panel.padding_x, 32,
            "config 側の異常値は new で 32 にクランプ"
        );
    }
}
