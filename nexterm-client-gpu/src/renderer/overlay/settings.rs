//! 設定パネル (Ctrl+,) の頂点ビルダー。

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::super::WgpuState;

impl WgpuState {
    /// 設定パネル頂点を構築する（Ctrl+, でオープン）
    ///
    /// タブ 0=Font, 1=Colors, 2=Window のパネルを表示する。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_settings_panel_verts(
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
}
