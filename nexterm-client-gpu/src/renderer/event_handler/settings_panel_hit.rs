//! Mouse hit-testing against the settings panel.
//!
//! Extracted from `event_handler.rs`:
//! - `SettingsPanelHit` enum (kinds of hit results)
//! - `EventHandler::hit_test_settings_panel`

use super::EventHandler;

/// Result of a mouse hit-test against the settings panel.
pub(super) enum SettingsPanelHit {
    /// Click outside the panel → close the panel.
    Outside,
    /// Title-bar area (reserved for future drag-to-move and similar).
    TitleBar,
    /// Click on a sidebar category.
    Category(usize),
    /// Click/drag on a slider.
    Slider {
        slider_type: crate::settings_panel::SliderType,
        track_x: f32,
        track_w: f32,
        #[allow(dead_code)]
        min: f32,
        #[allow(dead_code)]
        max: f32,
    },
    /// Theme color dot.
    ThemeColor(usize),
    /// Phase 5-11-6 #6: click on a row inside the Window category (changes focus + optional action).
    /// row 0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode.
    /// Clicking on the label area of rows 1 / 4 also cycles the value.
    WindowRow(u8),
    /// Empty area inside the panel (no-op).
    PanelBackground,
}

impl EventHandler {
    /// Run a mouse hit-test against the settings panel.
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

        // Panel dimensions (same formula as `build_settings_panel_verts`).
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

        // Outside the panel → close.
        if cx < px || cx > px + panel_w || cy < py || cy > py + panel_h {
            return SettingsPanelHit::Outside;
        }

        // Title bar.
        let title_h = cell_h * 1.4;
        if cy < py + title_h {
            return SettingsPanelHit::TitleBar;
        }

        // Sidebar category.
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

        // Content-area hit-test.
        let content_top = py + title_h + cell_h * 0.5;
        let bar_w = content_w - cell_w * 3.0;

        match &sp.category {
            SettingsCategory::Font => {
                // Font-size slider.
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
                // Theme color dots.
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
                // Phase 5-11-6 #6: hit-test for the 5-field layout.
                //   row 0=opacity / 1=cursor_style / 2=padding_x / 3=padding_y / 4=present_mode
                //   Geometry mirrors overlay/settings.rs: labels_top = content_top + cell_h*0.6, row_h = cell_h*3.2.
                let labels_top = content_top + cell_h * 0.6;
                let row_h = cell_h * 3.2;

                // Row 0: opacity slider.
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

                // Row 2: padding_x slider.
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

                // Row 3: padding_y slider.
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

                // Row click detection (label area = each row's y..y+row_h).
                // Clicking the label of rows 1 / 4 is expected to cycle the value (handled on the mouse side).
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
