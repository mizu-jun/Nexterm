//! Sprint 2-1 Phase A: オーバーレイ UI 頂点ビルダー
//!
//! `renderer.rs` から抽出したオーバーレイ系頂点ビルダー 7 メソッド。

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::{ClientState, ContextMenu};
use crate::vertex_util::{add_px_rect, add_string_verts, visual_width};

use super::WgpuState;

impl WgpuState {
    /// コマンドパレット頂点を構築する（中央フローティング）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_palette_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let palette = &state.palette;
        let items = palette.filtered();
        let palette_cols: f32 = 40.0;
        let palette_rows = (items.len() + 2).min(12) as f32; // クエリ行 + 最大10アイテム + マージン

        let pw = palette_cols * cell_w;
        let ph = palette_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パレット背景（ダークグレー）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.15, 0.15, 0.18, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 外枠（やや明るい）
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            [0.4, 0.6, 1.0, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // クエリ行
        let query_text = format!("> {}", palette.query);
        add_string_verts(
            &query_text,
            px + cell_w,
            py + cell_h * 0.1,
            [1.0, 1.0, 1.0, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // アクション一覧
        for (i, action) in items.iter().enumerate().take(palette_rows as usize - 1) {
            let item_py = py + cell_h * (i as f32 + 1.2);
            if i == palette.selected {
                // 選択行ハイライト
                add_px_rect(
                    px + 2.0,
                    item_py,
                    pw - 4.0,
                    cell_h,
                    [0.25, 0.40, 0.65, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let prefix = if i == palette.selected { "> " } else { "  " };
            let label = format!("{}{}", prefix, action.label);
            let fg = if i == palette.selected {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.75, 0.75, 0.78, 1.0]
            };
            add_string_verts(
                &label,
                px + cell_w,
                item_py,
                fg,
                i == palette.selected,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }

    /// 設定パネル頂点を構築する（Ctrl+, でオープン）
    ///
    /// タブ 0=Font, 1=Colors, 2=Window のパネルを表示する。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_settings_panel_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        use crate::settings_panel::SettingsCategory;

        let sp = &state.settings_panel;
        if !sp.is_open {
            return;
        }

        // 開閉アニメーション: イーズアウトキュービックでスムーズに表示する
        let eased = sp.eased_progress();

        // パネルサイズ（左サイドバー付き）
        let panel_w = (sw * 0.72).min(sw - cell_w * 4.0);
        let panel_h = (sh * 0.75).min(sh - cell_h * 4.0);
        let px = (sw - panel_w) / 2.0;
        // スライドアップ: 開始時は 16px 下から徐々に定位置へ移動する
        let slide_offset = (1.0 - eased) * 16.0;
        let py = (sh - panel_h) / 2.0 + slide_offset;

        // サイドバー幅・コンテンツ領域（日本語カテゴリ名を考慮して18セル分確保）
        let sidebar_w = cell_w * 18.0;
        let content_x = px + sidebar_w;
        let content_w = panel_w - sidebar_w;

        // ドロップシャドウ（4px オフセット）
        add_px_rect(
            px + 4.0,
            py + 4.0,
            panel_w,
            panel_h,
            [0.04, 0.04, 0.06, 0.85],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // 枠線（外側 1px、アクセントカラー薄め）
        add_px_rect(
            px - 1.0,
            py - 1.0,
            panel_w + 2.0,
            panel_h + 2.0,
            [0.478, 0.635, 0.969, 0.20],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // パネル背景（完全不透明: ターミナル透過設定に関わらず常に不透明）
        add_px_rect(
            px,
            py,
            panel_w,
            panel_h,
            [0.102, 0.106, 0.149, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // タイトルバー（#1E2030、不透明）
        let title_h = cell_h * 1.4;
        add_px_rect(
            px,
            py,
            panel_w,
            title_h,
            [0.118, 0.125, 0.188, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // タイトルバー上端アクセント線（3px、#7AA2F7）
        add_px_rect(
            px,
            py,
            panel_w,
            3.0,
            [0.478, 0.635, 0.969, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 内側1px薄めのグロー
        add_px_rect(
            px,
            py + 3.0,
            panel_w,
            1.0,
            [0.478, 0.635, 0.969, 0.25],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // タイトル
        add_string_verts(
            " * Nexterm Settings",
            px + cell_w * 0.5,
            py + cell_h * 0.2,
            [0.663, 0.694, 0.839, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
        // 閉じるボタンヒント
        let close_text = "Esc";
        let close_x = px + panel_w - close_text.len() as f32 * cell_w - cell_w;
        add_string_verts(
            close_text,
            close_x,
            py + cell_h * 0.2,
            [0.478, 0.635, 0.969, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // サイドバー背景（不透明）
        let sidebar_top = py + title_h;
        let sidebar_h = panel_h - title_h - cell_h * 1.5;
        add_px_rect(
            px,
            sidebar_top,
            sidebar_w,
            sidebar_h,
            [0.066, 0.070, 0.102, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // サイドバー区切り線（アクセントカラー薄め）
        add_px_rect(
            px + sidebar_w,
            sidebar_top,
            1.0,
            sidebar_h,
            [0.478, 0.635, 0.969, 0.30],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // サイドバーカテゴリ一覧
        let cat_item_h = cell_h * 1.3;
        for (i, cat) in SettingsCategory::ALL.iter().enumerate() {
            let item_y = sidebar_top + i as f32 * cat_item_h + cell_h * 0.3;
            let is_active = &sp.category == cat;
            if is_active {
                // アクティブ項目: 青みを強めたアクセント背景（完全不透明）
                add_px_rect(
                    px,
                    item_y - cell_h * 0.15,
                    sidebar_w,
                    cat_item_h,
                    [0.149, 0.200, 0.320, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // 左端インジケーター（3px + 内側1px薄め）
                add_px_rect(
                    px,
                    item_y - cell_h * 0.15,
                    3.0,
                    cat_item_h,
                    [0.478, 0.635, 0.969, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                add_px_rect(
                    px + 3.0,
                    item_y - cell_h * 0.15,
                    1.0,
                    cat_item_h,
                    [0.478, 0.635, 0.969, 0.35],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let label = format!("  {} {}", cat.icon(), cat.label());
            let fg = if is_active {
                [0.753, 0.808, 0.969, 1.0]
            } else {
                [0.502, 0.533, 0.647, 1.0]
            };
            add_string_verts(
                &label,
                px + cell_w * 0.5,
                item_y,
                fg,
                is_active,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // コンテンツ領域
        let content_top = py + title_h + cell_h * 0.5;
        let content_inner_x = content_x + cell_w;

        match &sp.category {
            SettingsCategory::Font => {
                // フォントファミリー
                let family_cursor = if sp.font_family_editing { "|" } else { "" };
                let family_line = format!("Family:  {}{}", sp.font_family, family_cursor);
                if sp.font_family_editing {
                    let field_w = content_w - cell_w * 2.0;
                    add_px_rect(
                        content_inner_x,
                        content_top + cell_h * 1.0,
                        field_w,
                        cell_h,
                        [0.149, 0.188, 0.278, 1.0],
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                add_string_verts(
                    &family_line,
                    content_inner_x,
                    content_top + cell_h * 1.0,
                    [0.8, 0.85, 0.9, 1.0],
                    sp.font_family_editing,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let hint = if sp.font_family_editing {
                    "(Enter=確定  Esc=キャンセル)"
                } else {
                    "(F キーで編集)"
                };
                add_string_verts(
                    hint,
                    content_inner_x,
                    content_top + cell_h * 1.9,
                    [0.376, 0.408, 0.518, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // フォントサイズ
                let size_line = format!("Size:    {:.1}pt", sp.font_size);
                add_string_verts(
                    &size_line,
                    content_inner_x,
                    content_top + cell_h * 3.0,
                    [0.9, 0.95, 1.0, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // サイズバー（8〜32pt）
                let bar_w = content_w - cell_w * 3.0;
                let bar_y = content_top + cell_h * 4.2;
                add_px_rect(
                    content_inner_x,
                    bar_y,
                    bar_w,
                    cell_h * 0.35,
                    [0.176, 0.192, 0.286, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                let fill = ((sp.font_size - 8.0) / 24.0).clamp(0.0, 1.0);
                add_px_rect(
                    content_inner_x,
                    bar_y,
                    bar_w * fill,
                    cell_h * 0.35,
                    [0.478, 0.635, 0.969, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                add_string_verts(
                    "(↑/↓ で変更)",
                    content_inner_x,
                    content_top + cell_h * 4.8,
                    [0.376, 0.408, 0.518, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
            SettingsCategory::Theme => {
                // カラースキーム
                let scheme_line = format!("テーマ:  {}  (←/→)", sp.scheme_name());
                add_string_verts(
                    &scheme_line,
                    content_inner_x,
                    content_top + cell_h * 1.0,
                    [0.9, 0.95, 1.0, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // スキームプレビュードット（9個）
                let dot_y = content_top + cell_h * 2.5;
                let scheme_names = [
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
                let schemes_colors: [[f32; 4]; 9] = [
                    [0.15, 0.15, 0.18, 1.0],
                    [0.95, 0.95, 0.92, 1.0],
                    [0.10, 0.10, 0.20, 1.0],
                    [0.00, 0.17, 0.21, 1.0],
                    [0.28, 0.26, 0.22, 1.0],
                    [0.19, 0.17, 0.23, 1.0],
                    [0.16, 0.13, 0.23, 1.0],
                    [0.18, 0.20, 0.25, 1.0],
                    [0.16, 0.18, 0.22, 1.0],
                ];
                let dot_size = cell_w * 1.2;
                let dot_gap = (content_w - cell_w * 2.0) / 9.0;
                for (i, (&col, name)) in schemes_colors.iter().zip(scheme_names.iter()).enumerate()
                {
                    let dot_x = content_inner_x + i as f32 * dot_gap;
                    let is_sel = sp.scheme_index == i;
                    if is_sel {
                        add_px_rect(
                            dot_x - 2.0,
                            dot_y - 2.0,
                            dot_size + 4.0,
                            cell_h + 4.0,
                            [0.478, 0.635, 0.969, 1.0],
                            sw,
                            sh,
                            bg_verts,
                            bg_idx,
                        );
                    }
                    add_px_rect(
                        dot_x, dot_y, dot_size, cell_h, col, sw, sh, bg_verts, bg_idx,
                    );
                    let name_y = dot_y + cell_h * 1.3;
                    let short = &name[..3.min(name.len())];
                    add_string_verts(
                        short,
                        dot_x,
                        name_y,
                        if is_sel {
                            [0.663, 0.694, 0.839, 1.0]
                        } else {
                            [0.376, 0.408, 0.518, 1.0]
                        },
                        is_sel,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                }
            }
            SettingsCategory::Window => {
                // 不透明度
                let opacity_line = format!("不透明度:  {:.0}%  (↑/↓)", sp.opacity * 100.0);
                add_string_verts(
                    &opacity_line,
                    content_inner_x,
                    content_top + cell_h * 1.0,
                    [0.9, 0.95, 1.0, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let bar_w = content_w - cell_w * 3.0;
                let bar_y = content_top + cell_h * 2.4;
                add_px_rect(
                    content_inner_x,
                    bar_y,
                    bar_w,
                    cell_h * 0.35,
                    [0.176, 0.192, 0.286, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                add_px_rect(
                    content_inner_x,
                    bar_y,
                    bar_w * sp.opacity,
                    cell_h * 0.35,
                    [0.478, 0.635, 0.969, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            SettingsCategory::Profiles => {
                add_string_verts(
                    "プロファイル一覧:",
                    content_inner_x,
                    content_top + cell_h * 0.5,
                    [0.663, 0.694, 0.839, 1.0],
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                if sp.profiles.is_empty() {
                    add_string_verts(
                        "プロファイルがありません",
                        content_inner_x,
                        content_top + cell_h * 1.8,
                        [0.376, 0.408, 0.518, 1.0],
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                    add_string_verts(
                        "nexterm.toml に [[profiles]] を追加してください",
                        content_inner_x,
                        content_top + cell_h * 2.7,
                        [0.376, 0.408, 0.518, 1.0],
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                } else {
                    for (i, prof) in sp.profiles.iter().enumerate() {
                        let item_y = content_top + cell_h * (1.5 + i as f32 * 1.2);
                        let is_sel = sp.selected_profile == i;
                        if is_sel {
                            add_px_rect(
                                content_inner_x - cell_w * 0.3,
                                item_y - cell_h * 0.1,
                                content_w - cell_w * 0.7,
                                cell_h,
                                [0.149, 0.188, 0.278, 1.0],
                                sw,
                                sh,
                                bg_verts,
                                bg_idx,
                            );
                        }
                        let label = format!("{} {}", prof.icon, prof.name);
                        let fg = if is_sel {
                            [0.753, 0.808, 0.969, 1.0]
                        } else {
                            [0.502, 0.533, 0.647, 1.0]
                        };
                        add_string_verts(
                            &label,
                            content_inner_x,
                            item_y,
                            fg,
                            is_sel,
                            sw,
                            sh,
                            cell_w,
                            font,
                            atlas,
                            &self.queue,
                            text_verts,
                            text_idx,
                        );
                    }
                }
            }
            SettingsCategory::Startup => {
                use crate::settings_panel::LANGUAGE_OPTIONS;

                // 言語選択ラベル
                add_string_verts(
                    "言語 / Language",
                    content_inner_x,
                    content_top + cell_h * 0.5,
                    [0.663, 0.694, 0.839, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // 選択バー背景
                let sel_y = content_top + cell_h * 1.6;
                let sel_w = content_w - cell_w * 2.0;
                add_px_rect(
                    content_inner_x,
                    sel_y,
                    sel_w,
                    cell_h,
                    [0.149, 0.188, 0.278, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );

                // 現在の言語名表示
                let lang_label = LANGUAGE_OPTIONS
                    .get(sp.language_index)
                    .map(|(name, _)| *name)
                    .unwrap_or("Auto");
                let lang_text = format!("< {} >", lang_label);
                add_string_verts(
                    &lang_text,
                    content_inner_x + cell_w * 0.5,
                    sel_y + cell_h * 0.1,
                    [0.95, 0.96, 1.0, 1.0],
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // 更新確認トグル
                let check_label = "起動時に更新を確認する";
                let check_y = content_top + cell_h * 3.0;
                add_string_verts(
                    check_label,
                    content_inner_x,
                    check_y,
                    [0.663, 0.694, 0.839, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let toggle_str = if sp.auto_check_update {
                    "[ON ]"
                } else {
                    "[OFF]"
                };
                let toggle_color = if sp.auto_check_update {
                    [0.15, 0.85, 0.45, 1.0]
                } else {
                    [0.55, 0.55, 0.55, 1.0]
                };
                add_string_verts(
                    toggle_str,
                    content_inner_x + check_label.len() as f32 * cell_w * 0.6 + cell_w,
                    check_y,
                    toggle_color,
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );

                // 変更は次回起動時に反映される旨の注記
                add_string_verts(
                    "※ 言語変更は次回起動時に反映されます",
                    content_inner_x,
                    content_top + cell_h * 4.4,
                    [0.376, 0.408, 0.518, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
            _ => {
                // SSH・キーバインドは近日実装予定
                let msg = match &sp.category {
                    SettingsCategory::Ssh => "SSH ホストは nexterm.toml の [[hosts]] で管理します",
                    SettingsCategory::Keybindings => {
                        "キーバインドは nexterm.toml の [[keys]] で管理します"
                    }
                    _ => "",
                };
                add_string_verts(
                    msg,
                    content_inner_x,
                    content_top + cell_h * 2.0,
                    [0.376, 0.408, 0.518, 1.0],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
        }

        // ボトムバー（保存・キャンセル）
        let bottom_y = py + panel_h - cell_h * 1.5;
        add_px_rect(
            px,
            bottom_y,
            panel_w,
            1.0,
            [0.176, 0.192, 0.286, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_px_rect(
            px,
            bottom_y + 1.0,
            panel_w,
            cell_h * 1.5 - 1.0,
            [0.118, 0.125, 0.188, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_string_verts(
            "  Enter=保存  Esc=キャンセル  Tab=次のカテゴリ",
            px + cell_w * 0.5,
            bottom_y + cell_h * 0.3,
            [0.376, 0.408, 0.518, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // フェードインオーバーレイ: パネルと同色で、open_progress が進むにつれて透明になる
        // eased=1.0 のときはオーバーレイなし（完全に表示）
        let fade_alpha = (1.0 - eased) * 0.95;
        if fade_alpha > 0.01 {
            add_px_rect(
                px - 1.0,
                py - 1.0,
                panel_w + 2.0,
                panel_h + 2.0,
                [0.102, 0.106, 0.149, fade_alpha],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }
    }

    /// SFTP ファイル転送ダイアログ頂点を構築する
    ///
    /// ホスト名・ローカルパス・リモートパスの 3 フィールドを入力する。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_file_transfer_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let ft = &state.file_transfer;
        let panel_cols: f32 = 56.0;
        let panel_rows: f32 = 7.0; // タイトル + ホスト + ローカル + リモート + ヒント

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景（深青緑）
        let bg_color = if ft.mode == "upload" {
            [0.05, 0.15, 0.20, 0.96]
        } else {
            [0.05, 0.20, 0.12, 0.96]
        };
        add_px_rect(px, py, pw, ph, bg_color, sw, sh, bg_verts, bg_idx);
        let accent = if ft.mode == "upload" {
            [0.2, 0.8, 1.0, 1.0]
        } else {
            [0.2, 1.0, 0.6, 1.0]
        };
        add_px_rect(px, py, pw, 2.0, accent, sw, sh, bg_verts, bg_idx);

        // タイトル
        let title = if ft.mode == "upload" {
            "SFTP Upload  (Tab=next, Enter=send, Esc=cancel)"
        } else {
            "SFTP Download  (Tab=next, Enter=send, Esc=cancel)"
        };
        add_string_verts(
            title,
            px + cell_w,
            py + cell_h * 0.1,
            accent,
            true,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        let field_labels = ["Host:", "Local:", "Remote:"];
        let field_values = [&ft.host_name, &ft.local_path, &ft.remote_path];

        for (i, (label, value)) in field_labels.iter().zip(field_values.iter()).enumerate() {
            let row_y = py + cell_h * (i as f32 * 1.5 + 1.3);
            let is_active = i == ft.field;

            // フィールド背景（アクティブは明るく）
            let field_bg = if is_active {
                [0.15, 0.25, 0.35, 1.0]
            } else {
                [0.10, 0.15, 0.20, 1.0]
            };
            add_px_rect(
                px + cell_w * 8.0,
                row_y,
                pw - cell_w * 9.0,
                cell_h,
                field_bg,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );

            // ラベル
            add_string_verts(
                label,
                px + cell_w,
                row_y,
                if is_active {
                    [0.9, 0.95, 1.0, 1.0]
                } else {
                    [0.6, 0.65, 0.7, 1.0]
                },
                is_active,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );

            // 入力値 + カーソル
            let display = if is_active {
                format!("{}_", value)
            } else {
                value.to_string()
            };
            add_string_verts(
                &display,
                px + cell_w * 8.5,
                row_y,
                if is_active {
                    [1.0, 1.0, 0.8, 1.0]
                } else {
                    [0.8, 0.85, 0.8, 1.0]
                },
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }

    /// Lua マクロピッカー頂点を構築する（中央フローティングリスト）
    ///
    /// 定義済みマクロを一覧表示し、Enter で実行する。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_macro_picker_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let mp = &state.macro_picker;
        let items = mp.filtered();
        let panel_cols: f32 = 50.0;
        let panel_rows = (items.len() + 3).min(14) as f32;

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景（深紫系）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.12, 0.08, 0.20, 0.96],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上端アクセント線（紫/ピンク）
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            [0.7, 0.3, 1.0, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // タイトル行
        add_string_verts(
            "Lua Macros",
            px + cell_w,
            py + cell_h * 0.1,
            [0.8, 0.5, 1.0, 1.0],
            true,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // クエリ行
        let query_text = format!("> {}", mp.query);
        add_string_verts(
            &query_text,
            px + cell_w,
            py + cell_h * 1.1,
            [1.0, 1.0, 1.0, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // マクロ一覧
        for (i, mac) in items.iter().enumerate().take(panel_rows as usize - 2) {
            let item_py = py + cell_h * (i as f32 + 2.2);
            let is_selected = i == mp.selected;
            if is_selected {
                add_px_rect(
                    px + 2.0,
                    item_py,
                    pw - 4.0,
                    cell_h,
                    [0.35, 0.15, 0.50, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            let prefix = if is_selected { "> " } else { "  " };
            let desc = if mac.description.is_empty() {
                &mac.lua_fn
            } else {
                &mac.description
            };
            let label = format!("{}{:<22} {}", prefix, mac.name, desc);
            let fg = if is_selected {
                [0.95, 0.8, 1.0, 1.0]
            } else {
                [0.70, 0.60, 0.78, 1.0]
            };
            add_string_verts(
                &label,
                px + cell_w,
                item_py,
                fg,
                is_selected,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // 空マクロ時のヒント
        if items.is_empty() {
            add_string_verts(
                "  (no macros in config)",
                px + cell_w,
                py + cell_h * 2.2,
                [0.5, 0.5, 0.5, 1.0],
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }

    /// ホストマネージャ頂点を構築する（中央フローティングリスト）
    ///
    /// コマンドパレットと同様のレイアウトで SSH ホスト一覧を表示する。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_host_manager_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let hm = &state.host_manager;
        let items = hm.filtered();
        let panel_cols: f32 = 52.0;
        let panel_rows = (items.len() + 3).min(14) as f32; // タイトル + クエリ + 最大12項目

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景（深めの紺）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.08, 0.12, 0.22, 0.96],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上端アクセント線（緑系）
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            [0.2, 0.8, 0.5, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // タイトル行
        add_string_verts(
            "SSH Hosts",
            px + cell_w,
            py + cell_h * 0.1,
            [0.2, 0.9, 0.6, 1.0],
            true,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // クエリ行
        let query_text = format!("> {}", hm.query);
        add_string_verts(
            &query_text,
            px + cell_w,
            py + cell_h * 1.1,
            [1.0, 1.0, 1.0, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // ホスト一覧（タイトル+クエリ = 2行分オフセット）
        for (i, host) in items.iter().enumerate().take(panel_rows as usize - 2) {
            let item_py = py + cell_h * (i as f32 + 2.2);
            let is_selected = i == hm.selected;
            if is_selected {
                add_px_rect(
                    px + 2.0,
                    item_py,
                    pw - 4.0,
                    cell_h,
                    [0.15, 0.45, 0.30, 1.0],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            // 表示フォーマット: "> name  user@host:port"
            let prefix = if is_selected { "> " } else { "  " };
            let label = format!(
                "{}{:<20} {}@{}:{}",
                prefix, host.name, host.username, host.host, host.port
            );
            let fg = if is_selected {
                [0.9, 1.0, 0.9, 1.0]
            } else {
                [0.70, 0.75, 0.72, 1.0]
            };
            add_string_verts(
                &label,
                px + cell_w,
                item_py,
                fg,
                is_selected,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // 空ホスト時のヒント
        if items.is_empty() {
            add_string_verts(
                "  (no hosts in config)",
                px + cell_w,
                py + cell_h * 2.2,
                [0.5, 0.5, 0.5, 1.0],
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }

    /// パスワード入力モーダルの頂点を構築する
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_password_modal_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let Some(modal) = &state.host_manager.password_modal else {
            return;
        };

        let pw = 44.0 * cell_w;
        let ph = 6.0 * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // 背景（濃い紺）
        add_px_rect(
            px,
            py,
            pw,
            ph,
            [0.06, 0.10, 0.20, 0.97],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 上端アクセント線（オレンジ）
        add_px_rect(
            px,
            py,
            pw,
            2.0,
            [0.9, 0.5, 0.1, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // タイトル
        let title = format!(
            "Password: {}@{}:{}",
            modal.host.username, modal.host.host, modal.host.port
        );
        add_string_verts(
            &title,
            px + cell_w,
            py + cell_h * 0.15,
            [0.9, 0.6, 0.2, 1.0],
            true,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // パスワード入力欄（マスク表示）
        // HIGH H-6: input は private な Zeroizing<String> なので input_len() 経由で文字数のみ取得
        let masked = "*".repeat(modal.input_len());
        let prompt = format!("> {}_", masked);
        add_string_verts(
            &prompt,
            px + cell_w,
            py + cell_h * 1.3,
            [1.0, 1.0, 1.0, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // エラーメッセージ
        if let Some(err) = &modal.error {
            add_string_verts(
                err,
                px + cell_w,
                py + cell_h * 2.5,
                [1.0, 0.3, 0.3, 1.0],
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // remember 状態（OS キーチェーン保存トグル）
        let remember_label = if modal.remember {
            "[X] OS キーチェーンに保存する (Tab で切替)"
        } else {
            "[ ] OS キーチェーンに保存する (Tab で切替)"
        };
        let remember_color = if modal.remember {
            [0.4, 0.9, 0.5, 1.0]
        } else {
            [0.6, 0.6, 0.6, 1.0]
        };
        add_string_verts(
            remember_label,
            px + cell_w,
            py + cell_h * 3.2,
            remember_color,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
        if modal.prefilled {
            add_string_verts(
                "(キーチェーンから自動入力済み)",
                px + cell_w,
                py + cell_h * 2.0,
                [0.5, 0.7, 1.0, 1.0],
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // ヒント
        add_string_verts(
            "Enter=接続  Tab=保存切替  Esc=キャンセル",
            px + cell_w,
            py + cell_h * 4.1,
            [0.45, 0.50, 0.48, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }

    /// コンテキストメニュー頂点を構築する（右クリック時のポップアップ）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_context_menu_verts(
        &self,
        menu: &ContextMenu,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // ラベルとヒントの最大表示幅からメニュー幅を動的に計算する
        let max_label_w = menu
            .items
            .iter()
            .map(|item| visual_width(&item.label))
            .max()
            .unwrap_or(8);
        let max_hint_w = menu
            .items
            .iter()
            .map(|item| visual_width(&item.hint))
            .max()
            .unwrap_or(0);
        // 左パディング(0.9) + ラベル + ギャップ(2) + ヒント + 右パディング(1.5)
        let min_cells = max_label_w + max_hint_w + 5;
        let menu_w = (min_cells as f32).max(16.0) * cell_w;
        let menu_h = menu.items.len() as f32 * cell_h;
        let mx = menu.x;
        let my = menu.y;

        // ドロップシャドウ（3px オフセット）
        add_px_rect(
            mx + 3.0,
            my + 3.0,
            menu_w,
            menu_h,
            [0.02, 0.02, 0.04, 0.80],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // 枠線（外側 1px、アクセントカラー薄め）
        add_px_rect(
            mx - 1.0,
            my - 1.0,
            menu_w + 2.0,
            menu_h + 2.0,
            [0.478, 0.635, 0.969, 0.15],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // メニュー全体の背景（完全不透明: ターミナル透過設定に関わらず常に不透明）
        add_px_rect(
            mx,
            my,
            menu_w,
            menu_h,
            [0.10, 0.11, 0.18, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // 上端のアクセント線（3px 太め）
        add_px_rect(
            mx,
            my,
            menu_w,
            3.0,
            [0.478, 0.635, 0.969, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        for (i, item) in menu.items.iter().enumerate() {
            use crate::state::ContextMenuAction;
            let item_y = my + i as f32 * cell_h;

            if matches!(item.action, ContextMenuAction::Separator) {
                // セパレーター: 中央に水平線を描く
                let sep_y = item_y + cell_h * 0.45;
                add_px_rect(
                    mx + cell_w * 0.5,
                    sep_y,
                    menu_w - cell_w,
                    1.0,
                    [0.28, 0.32, 0.45, 0.70],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                continue;
            }

            // ホバーハイライト背景（セパレーター以外）
            if menu.hovered == Some(i) {
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    menu_w - 4.0,
                    cell_h - 2.0,
                    [0.149, 0.200, 0.320, 0.90],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // ホバー時の左アクセント線（3px）
                add_px_rect(
                    mx + 2.0,
                    item_y + 1.0,
                    3.0,
                    cell_h - 2.0,
                    [0.478, 0.635, 0.969, 0.90],
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // ラベルテキスト（左パディング 0.9セル分）
            let text_color = if menu.hovered == Some(i) {
                [0.95, 0.96, 1.0, 1.0] // ホバー時は少し明るく
            } else {
                [0.75, 0.78, 0.88, 1.0] // 通常は少し抑えた色
            };
            add_string_verts(
                &item.label,
                mx + cell_w * 0.9,
                item_y + cell_h * 0.1,
                text_color,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );

            // キーヒントテキスト（右寄せ、グレー）
            if !item.hint.is_empty() {
                let hint_visual_w = visual_width(&item.hint) as f32;
                let hint_x = mx + menu_w - (hint_visual_w * cell_w + cell_w * 0.5);
                add_string_verts(
                    &item.hint,
                    hint_x,
                    item_y + cell_h * 0.1,
                    [0.45, 0.48, 0.60, 0.80],
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
        }
    }
}
