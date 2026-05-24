//! 設定パネルに対するマウスヒットテスト
//!
//! `event_handler.rs` から抽出した:
//! - `SettingsPanelHit` enum（ヒット結果の種別）
//! - `EventHandler::hit_test_settings_panel`

use super::EventHandler;

/// 設定パネルに対するマウスヒットテスト結果
pub(super) enum SettingsPanelHit {
    /// パネル外をクリック → パネルを閉じる
    Outside,
    /// タイトルバーエリア（ドラッグ移動等の将来拡張用）
    TitleBar,
    /// サイドバーカテゴリをクリック
    Category(usize),
    /// スライダーをクリック/ドラッグ
    Slider {
        slider_type: crate::settings_panel::SliderType,
        track_x: f32,
        track_w: f32,
        #[allow(dead_code)]
        min: f32,
        #[allow(dead_code)]
        max: f32,
    },
    /// テーマカラードット
    ThemeColor(usize),
    /// Phase 5-11-6 #6: Window カテゴリ内の行クリック（フォーカス変更 + 任意操作）。
    /// 行 0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode
    /// 行 1 / 4 のラベル領域クリックは値サイクル動作を伴う。
    WindowRow(u8),
    /// パネル内の空白エリア（何もしない）
    PanelBackground,
}

impl EventHandler {
    /// 設定パネルに対するマウスヒットテストを実行する
    pub(super) fn hit_test_settings_panel(&self, cx: f32, cy: f32) -> SettingsPanelHit {
        use crate::settings_panel::{SettingsCategory, SliderType};

        let sp = &self.app.state.settings_panel;
        if !sp.is_open {
            return SettingsPanelHit::Outside;
        }
        let (sw, sh) = match self.wgpu_state.as_ref() {
            Some(w) => (
                w.surface_config.width as f32,
                w.surface_config.height as f32,
            ),
            None => return SettingsPanelHit::Outside,
        };
        let cell_w = self.app.font.cell_width();
        let cell_h = self.app.font.cell_height();

        // パネル寸法 (build_settings_panel_verts と同じ式)
        let panel_w = (sw * 0.72).min(sw - cell_w * 4.0);
        let panel_h = (sh * 0.75).min(sh - cell_h * 4.0);
        let px = (sw - panel_w) / 2.0;
        let eased = sp.eased_progress();
        let slide_offset = (1.0 - eased) * 16.0;
        let py = (sh - panel_h) / 2.0 + slide_offset;

        let sidebar_w = cell_w * 18.0;
        let content_x = px + sidebar_w;
        let content_w = panel_w - sidebar_w;
        let content_inner_x = content_x + cell_w;

        // パネル外 → 閉じる
        if cx < px || cx > px + panel_w || cy < py || cy > py + panel_h {
            return SettingsPanelHit::Outside;
        }

        // タイトルバー
        let title_h = cell_h * 1.4;
        if cy < py + title_h {
            return SettingsPanelHit::TitleBar;
        }

        // サイドバーカテゴリ
        let sidebar_top = py + title_h;
        let cat_item_h = cell_h * 1.3;
        if cx < px + sidebar_w {
            let rel_y = cy - sidebar_top;
            if rel_y >= 0.0 {
                let cat_idx = (rel_y / cat_item_h) as usize;
                if cat_idx < SettingsCategory::ALL.len() {
                    return SettingsPanelHit::Category(cat_idx);
                }
            }
            return SettingsPanelHit::PanelBackground;
        }

        // コンテンツ領域ヒットテスト
        let content_top = py + title_h + cell_h * 0.5;
        let bar_w = content_w - cell_w * 3.0;

        match &sp.category {
            SettingsCategory::Font => {
                // フォントサイズスライダー
                let bar_y = content_top + cell_h * 4.2;
                if cy >= bar_y - cell_h * 0.5
                    && cy <= bar_y + cell_h
                    && cx >= content_inner_x
                    && cx <= content_inner_x + bar_w
                {
                    return SettingsPanelHit::Slider {
                        slider_type: SliderType::FontSize,
                        track_x: content_inner_x,
                        track_w: bar_w,
                        min: 8.0,
                        max: 32.0,
                    };
                }
            }
            SettingsCategory::Theme => {
                // テーマカラードット
                let dot_y = content_top + cell_h * 2.5;
                let dot_gap = (content_w - cell_w * 2.0) / 9.0;
                let dot_size = cell_w * 1.2;
                if cy >= dot_y && cy <= dot_y + cell_h {
                    for i in 0..9_usize {
                        let dot_x = content_inner_x + i as f32 * dot_gap;
                        if cx >= dot_x && cx <= dot_x + dot_size {
                            return SettingsPanelHit::ThemeColor(i);
                        }
                    }
                }
            }
            SettingsCategory::Window => {
                // Phase 5-11-6 #6: 5 フィールド構成のヒットテスト
                //   行 0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode
                //   overlay/settings.rs と幾何同期: labels_top = content_top + cell_h*0.6, row_h = cell_h*3.2
                let labels_top = content_top + cell_h * 0.6;
                let row_h = cell_h * 3.2;

                // 行 0 opacity スライダー
                let opacity_bar_y = labels_top + cell_h * 1.4;
                if cy >= opacity_bar_y - cell_h * 0.5
                    && cy <= opacity_bar_y + cell_h
                    && cx >= content_inner_x
                    && cx <= content_inner_x + bar_w
                {
                    return SettingsPanelHit::Slider {
                        slider_type: SliderType::WindowOpacity,
                        track_x: content_inner_x,
                        track_w: bar_w,
                        min: 0.1,
                        max: 1.0,
                    };
                }

                // 行 2 padding_x スライダー
                let px_bar_y = labels_top + row_h * 2.0 + cell_h * 1.4;
                let px_bar_w = bar_w * 0.6;
                if cy >= px_bar_y - cell_h * 0.5
                    && cy <= px_bar_y + cell_h
                    && cx >= content_inner_x
                    && cx <= content_inner_x + px_bar_w
                {
                    return SettingsPanelHit::Slider {
                        slider_type: SliderType::WindowPaddingX,
                        track_x: content_inner_x,
                        track_w: px_bar_w,
                        min: 0.0,
                        max: 32.0,
                    };
                }

                // 行 3 padding_y スライダー
                let py_bar_y = labels_top + row_h * 3.0 + cell_h * 1.4;
                let py_bar_w = bar_w * 0.6;
                if cy >= py_bar_y - cell_h * 0.5
                    && cy <= py_bar_y + cell_h
                    && cx >= content_inner_x
                    && cx <= content_inner_x + py_bar_w
                {
                    return SettingsPanelHit::Slider {
                        slider_type: SliderType::WindowPaddingY,
                        track_x: content_inner_x,
                        track_w: py_bar_w,
                        min: 0.0,
                        max: 32.0,
                    };
                }

                // 行クリック判定（ラベル領域 = 各行の y..y+row_h）。
                // 行 1 / 4 はラベルクリックで値サイクル動作を伴う想定（mouse 側で処理）。
                for row in 0u8..5 {
                    let row_y = labels_top + row_h * row as f32;
                    if cy >= row_y - cell_h * 0.3 && cy <= row_y + cell_h * 2.5 {
                        return SettingsPanelHit::WindowRow(row);
                    }
                }
            }
            _ => {}
        }

        SettingsPanelHit::PanelBackground
    }
}
