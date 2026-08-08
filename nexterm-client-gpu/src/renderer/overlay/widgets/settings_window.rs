//! Widget specs for the Window settings category (UI/UX v3 phase P1c).
//!
//! The Window tab is the first consumer of the `Slider` and `Text` widget
//! kinds, and the first migrated tab whose rows can be collapsed by the
//! sidebar search: [`build_window_widgets`] lays out only the rows
//! `SettingsPanel::visible_window_rows` reports and gives the rest a
//! zero-sized rect, which `hit_test` and `draw_widget` both treat as absent.
//!
//! As with `settings_theme`, [`window_widget_descs`] is the single semantic
//! definition (consumed by the AccessKit tree) and `build_window_widgets` is
//! that list plus geometry (consumed by the renderer and the hit-test).

use crate::settings_panel::SettingsPanel;

use super::geometry::TabGeometry;
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Window category.
pub(crate) const WINDOW_CATEGORY: u8 = 3;

/// Row indices, in the order they are drawn. These match the pre-migration
/// `window_field_focus` values and the `window_row_labels` order, so the
/// keyboard handler and the search filter keep working unchanged.
pub(crate) mod row {
    /// Background opacity (slider).
    pub const OPACITY: u16 = 0;
    /// Cursor style (cycler).
    pub const CURSOR_STYLE: u16 = 1;
    /// Horizontal padding (slider).
    pub const PADDING_X: u16 = 2;
    /// Vertical padding (slider).
    pub const PADDING_Y: u16 = 3;
    /// Present mode (cycler).
    pub const PRESENT_MODE: u16 = 4;
    /// Cursor blink (toggle).
    pub const CURSOR_BLINK: u16 = 5;
    /// Scrollback lines (slider).
    pub const SCROLLBACK: u16 = 6;
    /// Show tab number (toggle).
    pub const SHOW_TAB_NUMBER: u16 = 7;
    /// Show new-tab button (toggle).
    pub const SHOW_NEW_TAB_BUTTON: u16 = 8;
    /// Animations enabled (toggle).
    pub const ANIMATIONS_ENABLED: u16 = 9;
    /// Animation intensity (cycler).
    pub const ANIMATION_INTENSITY: u16 = 10;
    /// Window decorations (cycler).
    pub const DECORATIONS: u16 = 11;
    /// Close action (cycler).
    pub const CLOSE_ACTION: u16 = 12;
    /// FPS limit (slider).
    pub const FPS_LIMIT: u16 = 13;
}

/// Number of rows in the Window category.
pub(crate) const WINDOW_ROW_COUNT: usize = 14;

// Slider ranges, mirroring the clamps the `set_*` / `*_increase` setters
// already apply. They live here so the slider fraction, the AccessKit
// min/max, and the keyboard steppers cannot disagree.
/// Opacity range.
const OPACITY_RANGE: (f32, f32) = (0.1, 1.0);
/// Padding range, in pixels.
const PADDING_RANGE: (f32, f32) = (0.0, 32.0);
/// Scrollback range, in lines.
const SCROLLBACK_RANGE: (f32, f32) = (100.0, 100_000.0);
/// FPS-limit range. 0 means "unlimited" and sits at the bottom of the track.
const FPS_RANGE: (f32, f32) = (0.0, 240.0);

/// Localised label for a row, without the value the widget renders itself.
fn label(index: u16) -> String {
    use nexterm_i18n::fl;
    match index {
        row::OPACITY => fl!("settings-window-opacity-label"),
        row::CURSOR_STYLE => fl!("settings-window-cursor-style"),
        row::PADDING_X => fl!("settings-window-horizontal-padding-label"),
        row::PADDING_Y => fl!("settings-window-vertical-padding-label"),
        row::PRESENT_MODE => fl!("settings-window-present-mode"),
        row::CURSOR_BLINK => fl!("settings-window-cursor-blink-label"),
        row::SCROLLBACK => fl!("settings-window-scrollback-lines-label"),
        row::SHOW_TAB_NUMBER => fl!("settings-window-show-tab-number-label"),
        row::SHOW_NEW_TAB_BUTTON => fl!("settings-window-show-new-tab-button-label"),
        row::ANIMATIONS_ENABLED => fl!("settings-window-animations-enabled-label"),
        row::ANIMATION_INTENSITY => fl!("settings-window-animation-intensity"),
        row::DECORATIONS => fl!("settings-window-decorations"),
        row::CLOSE_ACTION => fl!("settings-window-close-action"),
        row::FPS_LIMIT => fl!("settings-window-fps-limit-label"),
        _ => String::new(),
    }
}

/// The kind and current value of a row.
fn kind(sp: &SettingsPanel, index: u16) -> WidgetKind {
    match index {
        row::OPACITY => WidgetKind::Slider {
            value: sp.opacity,
            min: OPACITY_RANGE.0,
            max: OPACITY_RANGE.1,
            step: 0.05,
            display: format!("{:.0}%", sp.opacity * 100.0),
        },
        row::CURSOR_STYLE => WidgetKind::Cycle {
            value: sp.cursor_style_label().to_string(),
        },
        row::PADDING_X => WidgetKind::Slider {
            value: sp.padding_x as f32,
            min: PADDING_RANGE.0,
            max: PADDING_RANGE.1,
            step: 1.0,
            display: format!("{} px", sp.padding_x),
        },
        row::PADDING_Y => WidgetKind::Slider {
            value: sp.padding_y as f32,
            min: PADDING_RANGE.0,
            max: PADDING_RANGE.1,
            step: 1.0,
            display: format!("{} px", sp.padding_y),
        },
        row::PRESENT_MODE => WidgetKind::Cycle {
            value: sp.present_mode_label().to_string(),
        },
        row::CURSOR_BLINK => WidgetKind::Toggle {
            on: sp.cursor_blink_enabled,
        },
        row::SCROLLBACK => WidgetKind::Slider {
            value: sp.scrollback_lines as f32,
            min: SCROLLBACK_RANGE.0,
            max: SCROLLBACK_RANGE.1,
            step: 100.0,
            display: sp.scrollback_lines.to_string(),
        },
        row::SHOW_TAB_NUMBER => WidgetKind::Toggle {
            on: sp.tab_show_tab_number,
        },
        row::SHOW_NEW_TAB_BUTTON => WidgetKind::Toggle {
            on: sp.tab_show_new_tab_button,
        },
        row::ANIMATIONS_ENABLED => WidgetKind::Toggle {
            on: sp.animations_enabled,
        },
        row::ANIMATION_INTENSITY => WidgetKind::Cycle {
            value: sp.animations_intensity_label().to_string(),
        },
        row::DECORATIONS => WidgetKind::Cycle {
            value: sp.window_decorations_label().to_string(),
        },
        row::CLOSE_ACTION => WidgetKind::Cycle {
            value: sp.window_close_action_label().to_string(),
        },
        row::FPS_LIMIT => WidgetKind::Slider {
            value: sp.fps_limit as f32,
            min: FPS_RANGE.0,
            max: FPS_RANGE.1,
            step: 10.0,
            display: sp.fps_limit_label(),
        },
        _ => WidgetKind::Label,
    }
}

/// Describe every control of the Window tab, without laying it out.
pub(crate) fn window_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    (0..WINDOW_ROW_COUNT as u16)
        .map(|index| {
            WidgetDesc::new(
                WidgetId::new(WINDOW_CATEGORY, index),
                kind(sp, index),
                label(index),
            )
            .focused(sp.window_field_focus == index)
        })
        .collect()
}

/// Vertical pitch between Window rows, as a multiple of the cell height.
const ROW_PITCH: f32 = 3.2;
/// Offset of the first row below the content top, in cell heights. The gap
/// holds the navigation hint line.
const ROWS_TOP: f32 = 0.6;
/// Row box height, in cell heights.
const ROW_H: f32 = 3.0;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;

/// Lay the Window tab out for this frame.
///
/// Rows hidden by the sidebar search get a zero-sized rect rather than being
/// dropped, so widget indices stay aligned with `window_field_focus` and the
/// AccessKit tree.
pub(crate) fn build_window_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    let visible = sp.visible_window_rows();
    let hovered = sp
        .hover_widget
        .filter(|h| h.category == WINDOW_CATEGORY)
        .map(|h| h.index);
    let rows_top = g.content_top + g.cell_h * ROWS_TOP;

    window_widget_descs(sp)
        .into_iter()
        .map(|desc| {
            let matched = sp.label_matches_search(&desc.label);
            let desc = desc.search_match(matched);
            let index = desc.id.index;
            let Some(slot) = crate::settings_panel::slot_of(&visible, index as usize) else {
                // Collapsed by the search filter: no hit region, nothing drawn.
                return desc
                    .place(WidgetRect::default(), WidgetRect::default())
                    .hovered(false);
            };
            let row_y = rows_top + g.cell_h * ROW_PITCH * slot as f32;
            let rect = WidgetRect::new(
                g.content_inner_x - g.cell_w * ROW_BLEED,
                row_y - g.cell_h * 0.1,
                (g.content_w - g.cell_w * (ROW_BLEED + 0.4)).max(0.0),
                g.cell_h * ROW_H,
            );
            let control = WidgetRect::new(
                g.content_inner_x + layout.control_x_off,
                rect.y,
                layout.control_w,
                g.cell_h * 1.2,
            );
            desc.place(rect, control).hovered(hovered == Some(index))
        })
        .collect()
}

/// The draggable sliders, with the range the drag maps onto.
///
/// Only the three rows that supported dragging before the migration are
/// listed: `SliderType` has no variant for scrollback or the FPS limit, so
/// those two render as sliders but are still adjusted with the keyboard or a
/// click, exactly as before.
pub(crate) fn drag_slider_of(index: u16) -> Option<(crate::settings_panel::SliderType, f32, f32)> {
    use crate::settings_panel::SliderType;
    match index {
        row::OPACITY => Some((SliderType::WindowOpacity, OPACITY_RANGE.0, OPACITY_RANGE.1)),
        row::PADDING_X => Some((SliderType::WindowPaddingX, PADDING_RANGE.0, PADDING_RANGE.1)),
        row::PADDING_Y => Some((SliderType::WindowPaddingY, PADDING_RANGE.0, PADDING_RANGE.1)),
        _ => None,
    }
}

/// Apply an accessibility action to the Window widget at `index`.
///
/// Delegates to the same setters the keyboard ←/→ handler uses, so the two
/// paths cannot drift. Returns whether anything changed.
pub(crate) fn apply_window_action(
    sp: &mut SettingsPanel,
    index: u16,
    action: super::action::WidgetAction,
) -> bool {
    use super::action::WidgetAction;

    if index as usize >= WINDOW_ROW_COUNT {
        return false;
    }
    // Focus the row first: the increase/decrease setters act on the focused
    // field, exactly as they do for the keyboard.
    sp.window_field_focus = index;
    match action {
        WidgetAction::Next => sp.window_field_increase(),
        WidgetAction::Prev => sp.window_field_decrease(),
        // A screen reader can set a numeric control directly; the setters
        // apply the same rounding and clamping the drag path uses.
        // Typed input is not offered by any Window row.
        WidgetAction::SetText(_) => return false,
        WidgetAction::SetValue(v) => match index {
            row::OPACITY => sp.set_opacity_value(v),
            row::PADDING_X => sp.set_padding_x_value(v),
            row::PADDING_Y => sp.set_padding_y_value(v),
            // No setter for the remaining rows; refuse rather than guess.
            _ => return false,
        },
        // Activate is the default action: flip toggles and step cyclers, but
        // leave numeric rows alone — a click on a slider label should not
        // nudge the value.
        WidgetAction::Activate => match index {
            row::CURSOR_STYLE => sp.next_cursor_style(),
            row::PRESENT_MODE => sp.next_present_mode(),
            row::CURSOR_BLINK => sp.toggle_cursor_blink(),
            row::SHOW_TAB_NUMBER => sp.toggle_show_tab_number(),
            row::SHOW_NEW_TAB_BUTTON => sp.toggle_show_new_tab_button(),
            row::ANIMATIONS_ENABLED => sp.toggle_animations_enabled(),
            row::ANIMATION_INTENSITY => sp.next_animations_intensity(),
            row::DECORATIONS => sp.next_window_decorations(),
            row::CLOSE_ACTION => sp.next_window_close_action(),
            _ => return true,
        },
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> TabGeometry {
        TabGeometry {
            content_top: 100.0,
            content_inner_x: 200.0,
            content_w: 600.0,
            cell_w: 10.0,
            cell_h: 20.0,
        }
    }

    fn panel() -> SettingsPanel {
        SettingsPanel::default()
    }

    #[test]
    fn describes_every_row_once() {
        let descs = window_widget_descs(&panel());
        assert_eq!(descs.len(), WINDOW_ROW_COUNT);
        let ids: std::collections::HashSet<_> = descs.iter().map(|d| d.id).collect();
        assert_eq!(ids.len(), descs.len());
        for (i, d) in descs.iter().enumerate() {
            assert_eq!(
                d.id.index as usize, i,
                "row order must match the focus index"
            );
        }
    }

    #[test]
    fn row_count_matches_the_panel_constant() {
        // Guard against the two lists drifting apart, which would misalign
        // every row below the added one.
        assert_eq!(
            WINDOW_ROW_COUNT,
            SettingsPanel::WINDOW_FIELD_COUNT as usize,
            "WINDOW_ROW_COUNT must track WINDOW_FIELD_COUNT"
        );
        assert_eq!(panel().window_row_labels().len(), WINDOW_ROW_COUNT);
    }

    #[test]
    fn every_row_has_a_non_empty_label_and_an_interactive_kind() {
        for d in window_widget_descs(&panel()) {
            assert!(!d.label.is_empty(), "row {:?} has no label", d.id);
            assert!(
                d.kind.is_interactive(),
                "row {:?} fell through to Label",
                d.id
            );
        }
    }

    #[test]
    fn sliders_and_toggles_and_cyclers_are_all_present() {
        let descs = window_widget_descs(&panel());
        let count = |f: fn(&WidgetKind) -> bool| descs.iter().filter(|d| f(&d.kind)).count();
        assert_eq!(count(|k| matches!(k, WidgetKind::Slider { .. })), 5);
        assert_eq!(count(|k| matches!(k, WidgetKind::Toggle { .. })), 4);
        assert_eq!(count(|k| matches!(k, WidgetKind::Cycle { .. })), 5);
    }

    #[test]
    fn slider_fractions_are_normalised() {
        let mut sp = panel();
        sp.opacity = OPACITY_RANGE.0;
        assert_eq!(
            window_widget_descs(&sp)[row::OPACITY as usize]
                .kind
                .slider_fraction(),
            0.0,
            "the minimum sits at the left of the track"
        );

        sp.opacity = OPACITY_RANGE.1;
        assert_eq!(
            window_widget_descs(&sp)[row::OPACITY as usize]
                .kind
                .slider_fraction(),
            1.0
        );
    }

    #[test]
    fn every_slider_carries_a_usable_range() {
        // The range is what a screen reader announces and steps by, so an
        // inverted or zero-width one would make the control unusable.
        for d in window_widget_descs(&panel()) {
            let WidgetKind::Slider {
                value,
                min,
                max,
                step,
                ..
            } = &d.kind
            else {
                continue;
            };
            assert!(max > min, "row {:?} has an empty range", d.id);
            assert!(*step > 0.0, "row {:?} has a non-positive step", d.id);
            assert!(
                value >= min && value <= max,
                "row {:?} value {value} is outside [{min}, {max}]",
                d.id
            );
        }
    }

    #[test]
    fn exactly_one_row_is_focused() {
        let mut sp = panel();
        sp.window_field_focus = row::SCROLLBACK;
        let focused: Vec<_> = window_widget_descs(&sp)
            .into_iter()
            .filter(|d| d.focused)
            .collect();
        assert_eq!(focused.len(), 1);
        assert_eq!(focused[0].id.index, row::SCROLLBACK);
    }

    #[test]
    fn rows_stack_without_overlapping() {
        let g = geometry();
        let specs = build_window_widgets(&panel(), &g);
        for pair in specs.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(
                a.rect.y + a.rect.h <= b.rect.y + 0.001,
                "rows {:?} and {:?} overlap",
                a.id(),
                b.id()
            );
        }
    }

    #[test]
    fn a_search_collapsed_row_gets_no_hit_region() {
        let mut sp = panel();
        // A query that matches the opacity row only.
        sp.search_query = sp.window_row_labels()[row::OPACITY as usize]
            .chars()
            .take(4)
            .collect();
        let visible = sp.visible_window_rows();
        assert!(visible.len() < WINDOW_ROW_COUNT, "the query must filter");

        let specs = build_window_widgets(&sp, &geometry());
        for spec in &specs {
            let shown = visible.contains(&(spec.id().index as usize));
            assert_eq!(
                spec.rect.w > 0.0 && spec.rect.h > 0.0,
                shown,
                "row {:?} visibility disagrees with the filter",
                spec.id()
            );
        }
    }

    #[test]
    fn collapsed_rows_are_never_hit() {
        let mut sp = panel();
        sp.search_query = sp.window_row_labels()[row::OPACITY as usize]
            .chars()
            .take(4)
            .collect();
        let visible = sp.visible_window_rows();
        let g = geometry();
        let specs = build_window_widgets(&sp, &g);

        // Every hit anywhere in the tab must land on a row the filter kept.
        for y in 0..600 {
            if let Some(id) =
                super::super::spec::hit_test(&specs, g.content_inner_x + 10.0, y as f32)
            {
                assert!(
                    visible.contains(&(id.index as usize)),
                    "collapsed row {id:?} was hit at y={y}"
                );
            }
        }
    }

    #[test]
    fn matching_rows_are_flagged_for_the_search_highlight() {
        // The accent-coloured highlight on matching rows predates the widget
        // layer; losing it in the migration would be a silent regression.
        let mut sp = panel();
        assert!(
            build_window_widgets(&sp, &geometry())
                .iter()
                .all(|s| !s.desc.search_match),
            "an idle search must not highlight anything"
        );

        sp.search_query = sp.window_row_labels()[row::OPACITY as usize]
            .chars()
            .take(4)
            .collect();
        let specs = build_window_widgets(&sp, &geometry());
        assert!(
            specs[row::OPACITY as usize].desc.search_match,
            "the matched row must be flagged"
        );
        assert!(
            specs.iter().filter(|s| s.desc.search_match).count() < WINDOW_ROW_COUNT,
            "the flag must discriminate, not blanket every row"
        );
    }

    #[test]
    fn a_query_matching_nothing_keeps_every_row_hittable() {
        // `visible_rows` deliberately falls back to showing everything rather
        // than leaving the user an empty page.
        let mut sp = panel();
        sp.search_query = "zzzzzzzz".to_string();
        let specs = build_window_widgets(&sp, &geometry());
        assert!(specs.iter().all(|s| s.rect.w > 0.0 && s.rect.h > 0.0));
    }

    #[test]
    fn hit_testing_a_visible_row_returns_it() {
        let g = geometry();
        let specs = build_window_widgets(&panel(), &g);
        let target = &specs[row::CURSOR_STYLE as usize];
        let (cx, cy) = target.rect.center();
        assert_eq!(
            super::super::spec::hit_test(&specs, cx, cy),
            Some(target.id())
        );
    }

    #[test]
    fn activate_flips_toggles_and_steps_cyclers() {
        use super::super::action::WidgetAction;

        let mut sp = panel();
        let before = sp.cursor_blink_enabled;
        assert!(apply_window_action(
            &mut sp,
            row::CURSOR_BLINK,
            WidgetAction::Activate
        ));
        assert_eq!(sp.cursor_blink_enabled, !before);

        let before = sp.cursor_style_label().to_string();
        apply_window_action(&mut sp, row::CURSOR_STYLE, WidgetAction::Activate);
        assert_ne!(sp.cursor_style_label(), before);
    }

    #[test]
    fn activate_leaves_numeric_rows_alone() {
        use super::super::action::WidgetAction;

        let mut sp = panel();
        let before = sp.opacity;
        assert!(apply_window_action(
            &mut sp,
            row::OPACITY,
            WidgetAction::Activate
        ));
        assert_eq!(sp.opacity, before, "a click must not nudge a slider");
        assert_eq!(sp.window_field_focus, row::OPACITY, "but it does focus it");
    }

    #[test]
    fn increment_and_decrement_move_a_slider() {
        use super::super::action::WidgetAction;

        let mut sp = panel();
        sp.padding_x = 4;
        apply_window_action(&mut sp, row::PADDING_X, WidgetAction::Next);
        assert!(sp.padding_x > 4);
        apply_window_action(&mut sp, row::PADDING_X, WidgetAction::Prev);
        assert_eq!(sp.padding_x, 4);
    }

    #[test]
    fn an_out_of_range_row_is_refused() {
        use super::super::action::WidgetAction;

        let mut sp = panel();
        assert!(!apply_window_action(
            &mut sp,
            WINDOW_ROW_COUNT as u16,
            WidgetAction::Activate
        ));
    }
}
