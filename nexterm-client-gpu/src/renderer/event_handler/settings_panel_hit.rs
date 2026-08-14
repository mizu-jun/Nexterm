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
    /// UI/UX v3 P1c: click on a row inside the Font category. The payload is
    /// the widget index; a press on the size slider's track returns `Slider`
    /// instead so a drag can start.
    FontRow(u16),
    /// UI/UX v3 P1c: click on a row inside the Startup category.
    StartupRow(u16),
    /// Theme color dot.
    ThemeColor(usize),
    /// UI/UX v3 P1b: click on a non-swatch row of the Theme category. The
    /// payload is the widget index (`THEME_SCHEME` / `THEME_FOLLOW_SYSTEM`);
    /// clicking focuses the row and, for the toggle, flips it.
    ThemeRow(u16),
    /// Click on a row inside the Window category. The payload is the widget
    /// index; see `widgets::settings_window::row` for the names. Clicking a
    /// toggle or cycler row also changes its value, while numeric rows only
    /// take focus (a press on a slider track returns `Slider` instead, so a
    /// drag can start).
    WindowRow(u16),
    /// Phase 2c follow-up: click on a row inside the Blocks category.
    /// row 0 = `blocks_enabled` toggle, row 1 = increment `border_width_px`
    /// (wraps from 8 back to 1), row 2 = `show_exit_code_badge` toggle.
    /// The "Editing" hint row (3) intentionally has no hit zone.
    BlocksRow(u16),
    /// Click on a row inside the Security category. row 0..=3 are consent-policy
    /// cyclers (click cycles forward), 4..=6 are byte-cap fields (click focuses
    /// and starts editing). The footer note row has no hit zone.
    SecurityRow(u16),
    /// UI/UX v3 P1c: click on a row inside the Profiles category. Index 0 is
    /// the active-profile cycler (click cycles forward); `LIST_BASE + i`
    /// selects entry `i`. Before the migration the list had no hit zone at
    /// all (selection was AccessKit-only).
    ProfilesRow(u16),
    /// P4 (WT-like UX): the "Open config.toml" link in the footer bar.
    OpenConfigFile,
    /// P2-A (WT-like UX): the "reset category to defaults" link in the
    /// footer bar (only shown for resettable categories).
    ResetCategory,
    /// Empty area inside the panel (no-op).
    PanelBackground,
    /// Phase 4 (UI/UX v2): click landed on the sidebar search input.
    SearchInput,
}

impl EventHandler {
    /// Run a mouse hit-test against the settings panel.
    pub(super) fn hit_test_settings_panel(&self, cx: f32, cy: f32) -> SettingsPanelHit {
        use crate::renderer::overlay::widgets::geometry::TabGeometry;
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
        let base_x = (sw - panel_w) / 2.0;
        let eased = sp.eased_progress();
        let slide_offset = (1.0 - eased) * 16.0;
        let base_y = (sh - panel_h) / 2.0 + slide_offset;
        // Phase 3 (UI 4-tasks, 2026-06-12): mirror the drag offset + clamp
        // from `build_settings_panel_verts` so the hit-test stays aligned with
        // the rendered panel even after the user drags it.
        let title_h_for_clamp = cell_h * 1.4;
        let (px, py) = crate::settings_panel::clamp_panel_position(
            base_x,
            base_y,
            panel_w,
            panel_h,
            sw,
            sh,
            title_h_for_clamp,
            sp.drag_offset,
        );

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

        // Footer bar — mirrors the `bottom_y` formula in
        // `overlay/settings/mod.rs`. The right-aligned "Open config.toml"
        // link is the only clickable region; the rest is a no-op. Checked
        // before the sidebar branch because the footer spans the full panel
        // width below the sidebar.
        let footer_y = py + panel_h - cell_h * 1.5;
        if cy >= footer_y {
            let label = format!("↗ {}", nexterm_i18n::fl!("settings-open-config-file"));
            let label_w = crate::vertex_util::visual_width(&label) as f32 * cell_w;
            if cx >= px + panel_w - label_w - cell_w {
                return SettingsPanelHit::OpenConfigFile;
            }
            // P2-A: "reset category to defaults" link, left of the
            // open-config link (mirrors `overlay/settings/mod.rs`).
            if sp.category_resettable() {
                let reset_label = format!("↺ {}", nexterm_i18n::fl!("settings-reset-category"));
                let reset_w = crate::vertex_util::visual_width(&reset_label) as f32 * cell_w;
                let reset_x = px + panel_w - label_w - cell_w * 3.0 - reset_w;
                if cx >= reset_x && cx < reset_x + reset_w + cell_w {
                    return SettingsPanelHit::ResetCategory;
                }
            }
            return SettingsPanelHit::PanelBackground;
        }

        // Sidebar — first the Phase 4 search input (reserved strip at the
        // top), then the category list below it.
        let sidebar_top = py + title_h;
        let search_h = cell_h * 1.6;
        let categories_top = sidebar_top + search_h;
        let cat_item_h = cell_h * 1.3;
        if cx < px + sidebar_w {
            // Search box hit-region (matches the box drawn in overlay/settings.rs).
            let search_pad = cell_w * 0.5;
            let search_box_y = sidebar_top + cell_h * 0.2;
            let search_box_h = cell_h * 1.1;
            if cx >= px + search_pad
                && cx < px + sidebar_w - search_pad
                && cy >= search_box_y
                && cy < search_box_y + search_box_h
            {
                return SettingsPanelHit::SearchInput;
            }
            let rel_y = cy - categories_top;
            if rel_y >= 0.0 {
                let cat_idx = (rel_y / cat_item_h) as usize;
                // Bound by `filtered_categories().len()`; the click handler
                // resolves the index into the same filtered list.
                let visible = sp.filtered_categories().len();
                if cat_idx < visible {
                    return SettingsPanelHit::Category(cat_idx);
                }
            }
            return SettingsPanelHit::PanelBackground;
        }

        // Content-area hit-test.
        let content_top = py + title_h + cell_h * 0.5;
        // Built once: every migrated category lays out from the same geometry.
        // The renderer translates the content by `-offset_px` when the panel
        // is scrolled, so the hit geometry has to shift the same way or a
        // scrolled panel routes clicks to the wrong row. (Same convention as
        // the tooltip anchor in `overlay/settings/mod.rs`.)
        let geometry = TabGeometry {
            content_top: content_top - sp.scroll.offset_px,
            content_inner_x,
            content_w,
            cell_w,
            cell_h,
        };

        match &sp.category {
            SettingsCategory::Font => {
                // UI/UX v3 P1c: rows come from the widget layer. Rows were
                // not clickable before; only the size slider was.
                use crate::renderer::overlay::widgets::draw::slider_track_rect;
                use crate::renderer::overlay::widgets::settings_font::{
                    FONT_SIZE_RANGE, build_font_widgets, row,
                };

                let specs = build_font_widgets(sp, &geometry);
                if let Some(id) = crate::renderer::overlay::widgets::spec::hit_test(&specs, cx, cy)
                {
                    if let Some(spec) = specs.iter().find(|s| s.id() == id)
                        && id.index == row::SIZE
                    {
                        let track = slider_track_rect(spec.control_rect, cell_w, cell_h);
                        if cx >= track.x
                            && cx <= track.x + track.w
                            && cy >= track.y - cell_h * 0.5
                            && cy <= track.y + track.h + cell_h * 0.5
                        {
                            return SettingsPanelHit::Slider {
                                slider_type: SliderType::FontSize,
                                track_x: track.x,
                                track_w: track.w,
                                min: FONT_SIZE_RANGE.0,
                                max: FONT_SIZE_RANGE.1,
                            };
                        }
                    }
                    return SettingsPanelHit::FontRow(id.index);
                }
            }
            SettingsCategory::Startup => {
                // UI/UX v3 P1c: the Startup rows had no hit zone at all
                // before; they are clickable now that they are widgets.
                use crate::renderer::overlay::widgets::settings_startup::build_startup_widgets;

                let specs = build_startup_widgets(sp, &geometry);
                if let Some(id) = crate::renderer::overlay::widgets::spec::hit_test(&specs, cx, cy)
                {
                    return SettingsPanelHit::StartupRow(id.index);
                }
            }
            SettingsCategory::Theme => {
                // UI/UX v3 P1b: the Theme tab is built from widget specs, so
                // the geometry lives in exactly one place and this branch
                // just hit-tests what the renderer drew.
                use crate::renderer::overlay::widgets::settings_theme::{
                    build_theme_widgets, swatch_index_of,
                };
                let specs = build_theme_widgets(sp, &geometry);
                if let Some(id) = crate::renderer::overlay::widgets::spec::hit_test(&specs, cx, cy)
                {
                    if let Some(i) = swatch_index_of(id) {
                        return SettingsPanelHit::ThemeColor(i);
                    }
                    return SettingsPanelHit::ThemeRow(id.index);
                }
            }
            SettingsCategory::Window => {
                // UI/UX v3 P1c: the Window tab is built from widget specs, so
                // the row geometry (including search collapse) lives in one
                // place and this branch only classifies the hit.
                use crate::renderer::overlay::widgets::draw::slider_track_rect;
                use crate::renderer::overlay::widgets::settings_window::drag_slider_of;

                let specs =
                    crate::renderer::overlay::widgets::settings_window::build_window_widgets(
                        sp, &geometry,
                    );
                if let Some(id) = crate::renderer::overlay::widgets::spec::hit_test(&specs, cx, cy)
                {
                    // A press on the track itself starts a drag; anywhere else
                    // on the row is a plain row click.
                    if let (Some(spec), Some((slider_type, min, max))) = (
                        specs.iter().find(|s| s.id() == id),
                        drag_slider_of(id.index),
                    ) {
                        let track = slider_track_rect(spec.control_rect, cell_w, cell_h);
                        // Grow the grab region vertically: the drawn track is
                        // only a few pixels tall.
                        if cx >= track.x
                            && cx <= track.x + track.w
                            && cy >= track.y - cell_h * 0.5
                            && cy <= track.y + track.h + cell_h * 0.5
                        {
                            return SettingsPanelHit::Slider {
                                slider_type,
                                track_x: track.x,
                                track_w: track.w,
                                min,
                                max,
                            };
                        }
                    }
                    return SettingsPanelHit::WindowRow(id.index);
                }
            }
            SettingsCategory::Blocks => {
                use crate::renderer::overlay::widgets::settings_blocks::build_blocks_widgets;

                let specs = build_blocks_widgets(sp, &geometry);
                if let Some(id) = crate::renderer::overlay::widgets::spec::hit_test(&specs, cx, cy)
                {
                    return SettingsPanelHit::BlocksRow(id.index);
                }
            }
            SettingsCategory::Security => {
                use crate::renderer::overlay::widgets::settings_security::build_security_widgets;

                let specs = build_security_widgets(sp, &geometry);
                if let Some(id) = crate::renderer::overlay::widgets::spec::hit_test(&specs, cx, cy)
                {
                    return SettingsPanelHit::SecurityRow(id.index);
                }
            }
            SettingsCategory::Profiles => {
                use crate::renderer::overlay::widgets::settings_profiles::build_profiles_widgets;

                let specs = build_profiles_widgets(sp, &geometry);
                if let Some(id) = crate::renderer::overlay::widgets::spec::hit_test(&specs, cx, cy)
                {
                    return SettingsPanelHit::ProfilesRow(id.index);
                }
            }
            _ => {}
        }

        SettingsPanelHit::PanelBackground
    }
}
