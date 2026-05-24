//! 設定パネル — Ctrl+, でフローティング UI を表示する（左サイドバー付き多カテゴリ設計）

use anyhow::Result;
use nexterm_config::toml_path;

/// Phase 5-11-8 Step 8-3 (Sub-phase A): インラインテキスト入力状態
///
/// 設定パネル内の TextInput フィールドの編集中状態を保持する。
/// SSH ホストの name / host / username フィールド編集に使用する。
/// IME preedit（Sub-phase B）は `preedit` フィールドで保持する。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInputState {
    /// 編集中バッファ
    pub buffer: String,
    /// カーソル位置（`buffer` 内のバイトインデックス）
    /// 不変条件: `buffer.is_char_boundary(cursor) == true`
    pub cursor: usize,
    /// IME 変換中テキスト（Sub-phase B で使用）。`None` は変換中でないことを示す。
    pub preedit: Option<String>,
}

impl TextInputState {
    /// 初期文字列で TextInputState を作る。カーソルは末尾。
    pub fn new(initial: String) -> Self {
        let cursor = initial.len();
        Self {
            buffer: initial,
            cursor,
            preedit: None,
        }
    }

    /// カーソル位置に文字を 1 つ挿入し、カーソルをその文字の直後に進める。
    pub fn insert_char(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    /// カーソル位置に文字列を挿入し、カーソルをその末尾に進める。
    /// IME `Commit` 経路で複数文字を一括挿入する際にも使う。
    pub fn insert_str(&mut self, s: &str) {
        self.buffer.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// カーソル直前の 1 文字を削除する（Backspace）。
    /// マルチバイト境界を尊重するため `floor_char_boundary` 相当の手動探索を行う。
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // カーソル直前の文字境界を探す
        let mut prev = self.cursor - 1;
        while prev > 0 && !self.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        self.buffer.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    /// カーソル直後の 1 文字を削除する（Delete）。
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut next = self.cursor + 1;
        while next < self.buffer.len() && !self.buffer.is_char_boundary(next) {
            next += 1;
        }
        self.buffer.replace_range(self.cursor..next, "");
    }

    /// カーソルを 1 文字左へ移動する。
    pub fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut prev = self.cursor - 1;
        while prev > 0 && !self.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        self.cursor = prev;
    }

    /// カーソルを 1 文字右へ移動する。
    pub fn move_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut next = self.cursor + 1;
        while next < self.buffer.len() && !self.buffer.is_char_boundary(next) {
            next += 1;
        }
        self.cursor = next;
    }

    /// カーソルを先頭へ移動する。
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// カーソルを末尾へ移動する。
    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    /// 表示用文字列を返す。preedit が None なら buffer をそのまま、
    /// Some(pe) ならカーソル位置に挿入された文字列を返す。
    pub fn display_string(&self) -> String {
        match &self.preedit {
            None => self.buffer.clone(),
            Some(pe) => {
                let mut s = self.buffer.clone();
                s.insert_str(self.cursor, pe);
                s
            }
        }
    }

    /// 表示用文字列上のカーソル位置（バイト単位）を返す。
    /// preedit がある場合は preedit の末尾を指す（IME 確定前の見た目に合わせる）。
    pub fn display_cursor(&self) -> usize {
        match &self.preedit {
            None => self.cursor,
            Some(pe) => self.cursor + pe.len(),
        }
    }
}

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
    /// Phase 5-11-8 Step 8-2: Ssh カテゴリ内のフォーカス中フィールド。
    /// 0=ListBox（ホスト選択） / 1=name / 2=host / 3=port / 4=username / 5=auth_type
    /// 範囲: 0..=5。AccessKit Focus / 上下キーで更新する。
    pub ssh_field_focus: u8,
    /// Phase 5-11-8 Step 8-3 (Sub-phase A): SSH ホストフィールドの編集中状態。
    /// `Some(state)` で編集モード ON、`None` で OFF。`ssh_field_focus` が 1/2/4
    /// （name/host/username）のフィールドに対応する。Enter で開始、Enter で確定、
    /// Esc でキャンセル。port / auth_type は Sub-phase C で別の UI（SpinButton /
    /// ComboBox）を使うため Option には入らない。
    pub ssh_field_editing: Option<TextInputState>,
    /// Phase 5-11-8 Step 8-3 (Sub-phase D): SSH 削除確認ダイアログが開いているか。
    /// `true` のとき `Role::AlertDialog` のモーダル（NodeId 47）を表示し、
    /// Confirm（48） / Cancel（49）ボタンで操作する。Esc キーは Cancel に等しい。
    pub ssh_delete_dialog_open: bool,
    /// Phase 5-11-8 Step 8-3 (Sub-phase D): 削除確認ダイアログでフォーカスされて
    /// いるボタン。`false` = Cancel（49、デフォルト・誤削除防止）、`true` = Confirm（48）。
    /// ←/→ で切り替え、Enter で実行。
    pub ssh_delete_dialog_confirm_focused: bool,
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
            ssh_field_focus: 0,
            ssh_field_editing: None,
            ssh_delete_dialog_open: false,
            ssh_delete_dialog_confirm_focused: false,
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
        // Phase 5-11-8 Step 8-3 (Sub-phase A): SSH フィールド編集モードも解除
        self.ssh_field_editing = None;
        // Phase 5-11-8 Step 8-3 (Sub-phase D): 削除確認ダイアログも閉じる
        self.ssh_delete_dialog_open = false;
        self.ssh_delete_dialog_confirm_focused = false;
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

    // ===== Phase 5-11-8 Step 8-2: SSH ホストフィールド編集 =====
    //
    // 選択中ホスト（`ssh_hosts[selected_host_index]`）の 5 フィールドを編集する。
    // AccessKit `Action::SetValue` 経路（TextInput / SpinButton）と `Action::Click`
    // 経路（ComboBox サイクル）の両方をサポートする。すべての変更は `dirty = true`。

    /// 認証方式の選択肢（auth_type の値）。`HostConfig` の serde 仕様に揃える。
    pub const SSH_AUTH_TYPES: &'static [&'static str] = &["password", "key", "agent"];

    /// 選択中ホストが存在すれば可変参照を返す。
    fn selected_ssh_host_mut(&mut self) -> Option<&mut SshHostEntry> {
        self.ssh_hosts.get_mut(self.selected_host_index)
    }

    /// name フィールドを更新する（TextInput SetValue 経路）。
    pub fn set_ssh_host_name(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.name = text;
            self.dirty = true;
        }
    }

    /// host フィールドを更新する（TextInput SetValue 経路）。
    pub fn set_ssh_host_host(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.host = text;
            self.dirty = true;
        }
    }

    /// username フィールドを更新する（TextInput SetValue 経路）。
    pub fn set_ssh_host_username(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.username = text;
            self.dirty = true;
        }
    }

    /// port フィールドを更新する（SpinButton SetValue 経路）。
    /// f64 を u16 にクランプ（1〜65535）。
    pub fn set_ssh_host_port_value(&mut self, v: f64) {
        let clamped = v.round().clamp(1.0, 65535.0) as u16;
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = clamped;
            self.dirty = true;
        }
    }

    /// port を +1 する（SpinButton Increment 経路、65535 で上限クランプ）。
    /// `u16::saturating_add` が 65535 で自動的に飽和するため明示的な `.min()` は不要。
    pub fn increase_ssh_host_port(&mut self) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = host.port.saturating_add(1);
            self.dirty = true;
        }
    }

    /// port を -1 する（SpinButton Decrement 経路、1 で下限クランプ）。
    pub fn decrease_ssh_host_port(&mut self) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = host.port.saturating_sub(1).max(1);
            self.dirty = true;
        }
    }

    /// auth_type を次の値に切り替える（ComboBox Click / Increment 経路）。
    /// `SSH_AUTH_TYPES` を循環する。未知の値が入っていた場合は先頭にリセット。
    pub fn next_ssh_auth_type(&mut self) {
        let types = Self::SSH_AUTH_TYPES;
        if let Some(host) = self.selected_ssh_host_mut() {
            let current = types.iter().position(|&t| t == host.auth_type).unwrap_or(0);
            host.auth_type = types[(current + 1) % types.len()].to_string();
            self.dirty = true;
        }
    }

    /// auth_type を前の値に切り替える（ComboBox Decrement 経路）。
    pub fn prev_ssh_auth_type(&mut self) {
        let types = Self::SSH_AUTH_TYPES;
        if let Some(host) = self.selected_ssh_host_mut() {
            let current = types.iter().position(|&t| t == host.auth_type).unwrap_or(0);
            host.auth_type = types[(current + types.len() - 1) % types.len()].to_string();
            self.dirty = true;
        }
    }

    // ===== Phase 5-11-8 Step 8-3 (Sub-phase D): Add / Delete + 削除確認ダイアログ =====
    //
    // - `add_ssh_host`: 全空 + port=22 + auth_type="password" の新規ホストを末尾に追加し、
    //   選択を新規ホストへ移動、name フィールド（field_id=1）の編集モードを即時開始する。
    // - `open_ssh_delete_dialog`: 削除確認ダイアログを開く。デフォルトのフォーカスは
    //   Cancel ボタン（誤削除防止）。
    // - `cancel_ssh_delete_dialog`: ダイアログを閉じる（削除実行なし）。
    // - `confirm_ssh_delete_dialog`: 選択中ホストを削除し、ダイアログを閉じる。
    //   削除後の選択行は n クランプ（リストが詰まる、末尾なら n-1）。

    /// 新規 SSH ホストを末尾に追加し、編集を開始する（Add ボタン経路）。
    ///
    /// デフォルト値: `name=""`, `host=""`, `port=22`, `username=""`, `auth_type="password"`。
    /// 追加直後は `selected_host_index = ssh_hosts.len() - 1` で新規ホストを選択、
    /// `ssh_field_focus = 1`（name）に移動し、`begin_ssh_field_edit()` で即時編集モード
    /// 開始する。これにより SR ユーザーは Add 押下直後から名前入力を始められる。
    pub fn add_ssh_host(&mut self) {
        let new_host = SshHostEntry {
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            auth_type: "password".to_string(),
        };
        self.ssh_hosts.push(new_host);
        self.selected_host_index = self.ssh_hosts.len() - 1;
        self.ssh_field_focus = 1;
        // 即時編集モード開始（name フィールド）
        self.ssh_field_editing = Some(TextInputState::new(String::new()));
        self.dirty = true;
    }

    /// 削除確認ダイアログを開く（Delete ボタン経路）。
    ///
    /// 空リスト時は何もしない（disabled 扱い）。デフォルトフォーカスは
    /// Cancel ボタンで、誤削除を防ぐ標準的な UX。
    pub fn open_ssh_delete_dialog(&mut self) {
        if self.ssh_hosts.is_empty() {
            return;
        }
        self.ssh_delete_dialog_open = true;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// 削除確認ダイアログを閉じる（Cancel ボタン or Esc キー経路）。
    /// ホストには変更を加えない。
    pub fn cancel_ssh_delete_dialog(&mut self) {
        self.ssh_delete_dialog_open = false;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// 削除確認ダイアログで「削除」を確定する（Confirm ボタン or Enter キー経路）。
    ///
    /// 選択中ホストを削除し、ダイアログを閉じる。削除後の選択行は n クランプ:
    /// - 削除前 selected_host_index=n、ssh_hosts.len()=L とすると
    /// - 削除後の有効インデックス上限は L-1 → 0 にクランプ
    /// - n が末尾だった場合は n-1 が新しい選択行
    /// - リストが空になった場合は selected_host_index=0 に戻し、ssh_field_focus=0
    pub fn confirm_ssh_delete_dialog(&mut self) {
        if self.selected_host_index < self.ssh_hosts.len() {
            self.ssh_hosts.remove(self.selected_host_index);
            // n クランプ: 末尾を削除した場合は n-1 にする
            if !self.ssh_hosts.is_empty() && self.selected_host_index >= self.ssh_hosts.len() {
                self.selected_host_index = self.ssh_hosts.len() - 1;
            }
            // 空になった場合は ListBox にフォーカスを戻す
            if self.ssh_hosts.is_empty() {
                self.selected_host_index = 0;
                self.ssh_field_focus = 0;
            }
            self.dirty = true;
        }
        self.ssh_delete_dialog_open = false;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// 削除確認ダイアログの ←/→ キーでフォーカスを切り替える（Confirm ↔ Cancel）。
    pub fn toggle_ssh_delete_dialog_focus(&mut self) {
        self.ssh_delete_dialog_confirm_focused = !self.ssh_delete_dialog_confirm_focused;
    }

    // ===== Phase 5-11-8 Step 8-3 (Sub-phase A): SSH フィールド インライン編集 =====
    //
    // `ssh_field_focus` が 1 (name) / 2 (host) / 4 (username) のときに Enter キーで
    // 編集モードを開始し、`ssh_field_editing = Some(TextInputState::new(current))` で
    // バッファを初期化する。再度 Enter で `set_ssh_host_*` 経由でホストに書き戻す。

    /// 現在の `ssh_field_focus` に対応する TextInput 編集モードを開始する。
    ///
    /// 戻り値: 編集モード開始に成功したら `true`、対応するフィールドが TextInput
    /// でない（port/auth_type/ListBox）か、選択ホストが存在しない場合は `false`。
    pub fn begin_ssh_field_edit(&mut self) -> bool {
        let initial = {
            let Some(host) = self.ssh_hosts.get(self.selected_host_index) else {
                return false;
            };
            match self.ssh_field_focus {
                1 => host.name.clone(),
                2 => host.host.clone(),
                4 => host.username.clone(),
                _ => return false,
            }
        };
        self.ssh_field_editing = Some(TextInputState::new(initial));
        true
    }

    /// 編集中のバッファをホストフィールドに書き戻し、編集モードを終了する。
    /// 戻り値: 書き戻しが行われたら `true`。
    pub fn commit_ssh_field_edit(&mut self) -> bool {
        let Some(state) = self.ssh_field_editing.take() else {
            return false;
        };
        let text = state.buffer;
        match self.ssh_field_focus {
            1 => self.set_ssh_host_name(text),
            2 => self.set_ssh_host_host(text),
            4 => self.set_ssh_host_username(text),
            _ => return false,
        }
        true
    }

    /// 編集中のバッファを破棄して編集モードを終了する。
    /// 戻り値: 編集モードが有効だったら `true`。
    pub fn cancel_ssh_field_edit(&mut self) -> bool {
        self.ssh_field_editing.take().is_some()
    }

    /// 編集中バッファのカーソル位置に文字を 1 つ挿入する。
    /// 編集モードでない場合は何もしない。
    pub fn ssh_field_insert_char(&mut self, ch: char) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.insert_char(ch);
        }
    }

    /// 編集中バッファのカーソル位置に文字列を挿入する（IME Commit 経路）。
    pub fn ssh_field_insert_str(&mut self, s: &str) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.insert_str(s);
        }
    }

    /// 編集中バッファのカーソル直前の 1 文字を削除する（Backspace）。
    pub fn ssh_field_backspace(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.backspace();
        }
    }

    /// 編集中バッファのカーソル直後の 1 文字を削除する（Delete）。
    pub fn ssh_field_delete(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.delete_forward();
        }
    }

    /// 編集中バッファのカーソルを 1 文字左へ動かす。
    pub fn ssh_field_move_left(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_left();
        }
    }

    /// 編集中バッファのカーソルを 1 文字右へ動かす。
    pub fn ssh_field_move_right(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_right();
        }
    }

    /// 編集中バッファのカーソルを先頭へ動かす。
    pub fn ssh_field_move_home(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_home();
        }
    }

    /// 編集中バッファのカーソルを末尾へ動かす。
    pub fn ssh_field_move_end(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_end();
        }
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

        // Phase 5-11-8 Step 8-2: [[hosts]] への in-place 書き戻し。
        //
        // 既存の `ArrayOfTables` がある場合はインデックス単位でフィールドだけ更新し、
        // `key_path` / `forward_local` / `proxy_jump` 等の未管理フィールドを保持する。
        // 配列長が `self.ssh_hosts` と一致しない（Step 8-3 の Add/Delete 後）場合は
        // 末尾の差分のみ調整する。
        write_ssh_hosts_back(&mut doc, &self.ssh_hosts);

        // 親ディレクトリを作成する
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, doc.to_string())?;
        Ok(())
    }
}

/// `[[hosts]]` 配列を in-place 更新する（Phase 5-11-8 Step 8-2）。
///
/// 既存の `ArrayOfTables` を保持し、SettingsPanel が管理する 5 フィールド
/// (name / host / port / username / auth_type) のみを上書きする。
/// `key_path` / `forward_local` / `proxy_jump` / `tags` 等の未管理フィールドは
/// そのままの形を保持する（ユーザーが TOML で手動設定した値を失わない）。
///
/// 配列長の調整:
/// - `ssh_hosts.len() > arr.len()`: 末尾に新規 Table を追加（Step 8-3 で Add から呼ばれる）
/// - `ssh_hosts.len() < arr.len()`: 末尾の Table を削除（Step 8-3 で Delete から呼ばれる）
/// - 等しい: in-place 更新のみ
pub(crate) fn write_ssh_hosts_back(doc: &mut toml_edit::DocumentMut, hosts: &[SshHostEntry]) {
    // 既存の hosts エントリを ArrayOfTables として取得（なければ作る）。
    let entry = doc
        .entry("hosts")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));

    // 既存 Item が ArrayOfTables でない（手動編集で壊れた）場合は再作成。
    if !entry.is_array_of_tables() {
        *entry = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    let Some(arr) = entry.as_array_of_tables_mut() else {
        return;
    };

    // インデックス単位で 5 フィールドを上書き。
    for (i, host) in hosts.iter().enumerate() {
        if i < arr.len() {
            let t = arr.get_mut(i).expect("既に長さチェック済み");
            t.insert("name", toml_edit::value(host.name.as_str()));
            t.insert("host", toml_edit::value(host.host.as_str()));
            t.insert("port", toml_edit::value(host.port as i64));
            t.insert("username", toml_edit::value(host.username.as_str()));
            t.insert("auth_type", toml_edit::value(host.auth_type.as_str()));
        } else {
            // 新規エントリ追加（Step 8-3 で発火）
            let mut t = toml_edit::Table::new();
            t.insert("name", toml_edit::value(host.name.as_str()));
            t.insert("host", toml_edit::value(host.host.as_str()));
            t.insert("port", toml_edit::value(host.port as i64));
            t.insert("username", toml_edit::value(host.username.as_str()));
            t.insert("auth_type", toml_edit::value(host.auth_type.as_str()));
            arr.push(t);
        }
    }
    // 余剰エントリを末尾から削除（Step 8-3 の Delete で発火）
    while arr.len() > hosts.len() {
        arr.remove(arr.len() - 1);
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

    // ============================================================
    // Sprint 5-11-8 Step 8-3 Sub-phase E: TextInputState 単体テスト
    // ============================================================

    #[test]
    fn text_input_state_new_cursor_at_end() {
        let s = TextInputState::new("hello".to_string());
        assert_eq!(s.buffer, "hello");
        assert_eq!(s.cursor, 5);
        assert!(s.preedit.is_none());

        let empty = TextInputState::new(String::new());
        assert_eq!(empty.cursor, 0);
    }

    #[test]
    fn text_input_state_insert_char_advances_cursor_ascii() {
        let mut s = TextInputState::new(String::new());
        s.insert_char('a');
        s.insert_char('b');
        s.insert_char('c');
        assert_eq!(s.buffer, "abc");
        assert_eq!(s.cursor, 3);
    }

    #[test]
    fn text_input_state_insert_char_advances_cursor_cjk() {
        // 日本語 1 文字 = UTF-8 3 バイト。カーソルもバイト単位で進むこと。
        let mut s = TextInputState::new(String::new());
        s.insert_char('あ');
        assert_eq!(s.buffer, "あ");
        assert_eq!(s.cursor, 3);
        s.insert_char('い');
        assert_eq!(s.buffer, "あい");
        assert_eq!(s.cursor, 6);
    }

    #[test]
    fn text_input_state_backspace_respects_utf8_boundary() {
        // "あい" のバックスペースで "あ" になり、カーソルは 3 バイト目（境界）に置かれる。
        let mut s = TextInputState::new("あい".to_string());
        assert_eq!(s.cursor, 6);
        s.backspace();
        assert_eq!(s.buffer, "あ");
        assert_eq!(s.cursor, 3);
        s.backspace();
        assert_eq!(s.buffer, "");
        assert_eq!(s.cursor, 0);
        // 空文字でのバックスペースは no-op
        s.backspace();
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn text_input_state_move_left_right_clamps_and_respects_boundary() {
        let mut s = TextInputState::new("aあb".to_string());
        // 末尾 (5 = 1 + 3 + 1)
        assert_eq!(s.cursor, 5);
        s.move_left();
        assert_eq!(s.cursor, 4, "b の手前へ");
        s.move_left();
        assert_eq!(s.cursor, 1, "あ の手前（UTF-8 境界を尊重）");
        s.move_left();
        assert_eq!(s.cursor, 0);
        // 先頭でのさらなる左移動は no-op
        s.move_left();
        assert_eq!(s.cursor, 0);

        s.move_right();
        assert_eq!(s.cursor, 1);
        s.move_right();
        assert_eq!(s.cursor, 4, "あ を跨ぐ");
        s.move_right();
        assert_eq!(s.cursor, 5);
        // 末尾でのさらなる右移動は no-op
        s.move_right();
        assert_eq!(s.cursor, 5);
    }

    #[test]
    fn text_input_state_display_string_with_preedit() {
        let mut s = TextInputState::new("ab".to_string());
        s.move_left(); // カーソルを 1 へ
        assert_eq!(s.cursor, 1);
        s.preedit = Some("X".to_string());

        // 表示文字列はカーソル位置に preedit が挿入される
        assert_eq!(s.display_string(), "aXb");
        assert_eq!(s.display_cursor(), 2, "preedit の末尾を指す");

        // preedit クリアで元に戻る
        s.preedit = None;
        assert_eq!(s.display_string(), "ab");
        assert_eq!(s.display_cursor(), 1);
    }

    // ============================================================
    // Sprint 5-11-8 Step 8-3 Sub-phase E: SSH フィールド編集ライフサイクル
    // ============================================================

    fn panel_with_one_host() -> SettingsPanel {
        let mut panel = SettingsPanel::new(&Config::default());
        panel.ssh_hosts.push(SshHostEntry {
            name: "myhost".to_string(),
            host: "example.com".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth_type: "password".to_string(),
        });
        panel.selected_host_index = 0;
        panel
    }

    #[test]
    fn ssh_field_edit_begin_commit_lifecycle() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 1; // name

        assert!(panel.begin_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_some());
        let state = panel.ssh_field_editing.as_ref().unwrap();
        assert_eq!(state.buffer, "myhost");

        // 文字を編集
        panel.ssh_field_insert_char('!');
        assert_eq!(panel.ssh_field_editing.as_ref().unwrap().buffer, "myhost!");

        // コミットでホストに反映
        assert!(panel.commit_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_none());
        assert_eq!(panel.ssh_hosts[0].name, "myhost!");
        assert!(panel.dirty);
    }

    #[test]
    fn ssh_field_edit_cancel_discards_changes() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 2; // host
        panel.begin_ssh_field_edit();
        panel.ssh_field_insert_char('X');

        assert!(panel.cancel_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_none());
        // ホストは変わらない
        assert_eq!(panel.ssh_hosts[0].host, "example.com");
    }

    #[test]
    fn ssh_field_edit_begin_returns_false_for_non_text_fields() {
        let mut panel = panel_with_one_host();
        // port (3) / auth_type (5) / ListBox (0) は TextInput でないため false
        for focus in [0u8, 3, 5, 6, 7] {
            panel.ssh_field_focus = focus;
            assert!(
                !panel.begin_ssh_field_edit(),
                "focus={focus} は TextInput でないため begin_ssh_field_edit は false を返すべき"
            );
            assert!(panel.ssh_field_editing.is_none());
        }
    }

    // ============================================================
    // Sprint 5-11-8 Step 8-3 Sub-phase E: Add / Delete + 確認ダイアログ
    // ============================================================

    #[test]
    fn add_ssh_host_appends_with_defaults_and_enters_edit_mode() {
        let mut panel = SettingsPanel::new(&Config::default());
        assert!(panel.ssh_hosts.is_empty());

        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 1);
        let new_host = &panel.ssh_hosts[0];
        assert_eq!(new_host.name, "");
        assert_eq!(new_host.host, "");
        assert_eq!(new_host.port, 22);
        assert_eq!(new_host.username, "");
        assert_eq!(new_host.auth_type, "password");

        assert_eq!(panel.selected_host_index, 0);
        assert_eq!(panel.ssh_field_focus, 1, "name フィールドにフォーカス");
        assert!(
            panel.ssh_field_editing.is_some(),
            "name 編集モードが即時開始されているべき"
        );
        assert_eq!(
            panel.ssh_field_editing.as_ref().unwrap().buffer,
            "",
            "新規ホストの name は空文字で初期化"
        );
        assert!(panel.dirty);
    }

    #[test]
    fn add_ssh_host_extends_existing_list() {
        let mut panel = panel_with_one_host();
        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(
            panel.selected_host_index, 1,
            "末尾の新規ホストが選択されている"
        );
    }

    #[test]
    fn open_ssh_delete_dialog_noop_when_empty() {
        let mut panel = SettingsPanel::new(&Config::default());
        assert!(panel.ssh_hosts.is_empty());
        panel.open_ssh_delete_dialog();
        assert!(
            !panel.ssh_delete_dialog_open,
            "空リスト時はダイアログは開かない"
        );
    }

    #[test]
    fn open_ssh_delete_dialog_defaults_to_cancel_focus() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        assert!(panel.ssh_delete_dialog_open);
        assert!(
            !panel.ssh_delete_dialog_confirm_focused,
            "誤削除防止: Cancel ボタンがデフォルトフォーカス"
        );
    }

    #[test]
    fn cancel_ssh_delete_dialog_clears_state_and_keeps_host() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        panel.ssh_delete_dialog_confirm_focused = true;
        panel.cancel_ssh_delete_dialog();

        assert!(!panel.ssh_delete_dialog_open);
        assert!(!panel.ssh_delete_dialog_confirm_focused);
        assert_eq!(panel.ssh_hosts.len(), 1, "削除されないこと");
    }

    #[test]
    fn confirm_ssh_delete_dialog_removes_at_end_clamps_to_prev() {
        let mut panel = panel_with_one_host();
        // 2 ホスト用意して末尾を削除
        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(panel.selected_host_index, 1);

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert_eq!(panel.ssh_hosts.len(), 1);
        assert_eq!(
            panel.selected_host_index, 0,
            "末尾を削除したので n クランプで n-1=0 へ"
        );
        assert!(!panel.ssh_delete_dialog_open);
        assert!(panel.dirty);
    }

    #[test]
    fn confirm_ssh_delete_dialog_middle_index_keeps_position() {
        let mut panel = panel_with_one_host();
        panel.add_ssh_host();
        panel.add_ssh_host(); // 計 3 ホスト
        panel.selected_host_index = 1; // 中央を選択

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(
            panel.selected_host_index, 1,
            "中央を削除したので末尾が詰まって index=1 のまま"
        );
    }

    #[test]
    fn confirm_ssh_delete_dialog_empty_after_resets_focus() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 3; // 何でもよい非ゼロ値

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert!(panel.ssh_hosts.is_empty());
        assert_eq!(panel.selected_host_index, 0);
        assert_eq!(
            panel.ssh_field_focus, 0,
            "空になったら ListBox にフォーカスを戻す"
        );
    }

    #[test]
    fn toggle_ssh_delete_dialog_focus_alternates() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        assert!(!panel.ssh_delete_dialog_confirm_focused);

        panel.toggle_ssh_delete_dialog_focus();
        assert!(panel.ssh_delete_dialog_confirm_focused);

        panel.toggle_ssh_delete_dialog_focus();
        assert!(!panel.ssh_delete_dialog_confirm_focused);
    }
}
