//! Widget specs for the Blocks settings category (UI/UX v3 phase P1c).
//!
//! The smallest migrated tab: three rows and a static tip line. Blocks has no
//! keyboard focus counter — it is mouse-driven and saves immediately — so no
//! row ever reports `focused`, and the tip line stays in the renderer because
//! it is prose rather than a control.

use crate::settings_panel::SettingsPanel;

use super::action::WidgetAction;
use super::geometry::TabGeometry;
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Blocks category.
pub(crate) const BLOCKS_CATEGORY: u8 = 7;

/// Row indices, matching `visible_blocks_rows` order.
pub(crate) mod row {
    /// `blocks.enabled` toggle.
    pub const ENABLED: u16 = 0;
    /// `blocks.border_width_px`, cycled 1..=8.
    pub const BORDER_WIDTH: u16 = 1;
    /// `blocks.show_exit_code_badge` toggle.
    pub const STATUS_BADGE: u16 = 2;
}

/// Number of rows in the Blocks category.
pub(crate) const BLOCKS_ROW_COUNT: usize = 3;

/// Highest border width before the cycler wraps back to 1.
const BORDER_WIDTH_MAX: u8 = 8;

/// Vertical pitch between rows, in cell heights.
const ROW_PITCH: f32 = 1.6;
/// Offset of the first row below the content top, in cell heights.
const ROWS_TOP: f32 = 0.5;
/// Row box height, in cell heights.
const ROW_H: f32 = 1.2;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;

/// Describe every control of the Blocks tab, without laying it out.
pub(crate) fn blocks_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    vec![
        WidgetDesc::new(
            WidgetId::new(BLOCKS_CATEGORY, row::ENABLED),
            WidgetKind::Toggle {
                on: sp.blocks_enabled,
            },
            nexterm_i18n::fl!("settings-blocks-enabled"),
        ),
        WidgetDesc::new(
            WidgetId::new(BLOCKS_CATEGORY, row::BORDER_WIDTH),
            WidgetKind::Cycle {
                value: sp.blocks_border_width_px.to_string(),
            },
            nexterm_i18n::fl!("settings-blocks-border-width"),
        ),
        WidgetDesc::new(
            WidgetId::new(BLOCKS_CATEGORY, row::STATUS_BADGE),
            WidgetKind::Toggle {
                on: sp.blocks_show_exit_code_badge,
            },
            nexterm_i18n::fl!("settings-blocks-status-badge"),
        ),
    ]
}

/// Lay the Blocks tab out for this frame.
///
/// Rows collapsed by the sidebar search get a zero-sized rect, which both
/// `hit_test` and `draw_widget` treat as absent.
pub(crate) fn build_blocks_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    let visible = sp.visible_blocks_rows();
    let hovered = sp
        .hover_widget
        .filter(|h| h.category == BLOCKS_CATEGORY)
        .map(|h| h.index);

    blocks_widget_descs(sp)
        .into_iter()
        .map(|desc| {
            let matched = sp.label_matches_search(&desc.label);
            let desc = desc.search_match(matched);
            let index = desc.id.index;
            let Some(slot) = crate::settings_panel::slot_of(&visible, index as usize) else {
                return desc
                    .place(WidgetRect::default(), WidgetRect::default())
                    .hovered(false);
            };
            let y = g.content_top + g.cell_h * (ROWS_TOP + slot as f32 * ROW_PITCH);
            let rect = WidgetRect::new(
                g.content_inner_x - g.cell_w * ROW_BLEED,
                y - g.cell_h * 0.1,
                (g.content_w - g.cell_w * (ROW_BLEED + 0.4)).max(0.0),
                g.cell_h * ROW_H,
            );
            let control = WidgetRect::new(
                g.content_inner_x + layout.control_x_off,
                rect.y,
                layout.control_w,
                rect.h,
            );
            desc.place(rect, control).hovered(hovered == Some(index))
        })
        .collect()
}

/// Y position just below the last visible row, where the tip line goes.
pub(crate) fn tip_y(sp: &SettingsPanel, g: &TabGeometry) -> f32 {
    let visible = sp.visible_blocks_rows();
    g.content_top + g.cell_h * (ROWS_TOP + visible.len() as f32 * ROW_PITCH)
}

/// Apply an action to the Blocks widget at `index`.
///
/// Every row commits immediately, matching the pre-migration mouse handler:
/// the caller persists once this returns true.
pub(crate) fn apply_blocks_action(
    sp: &mut SettingsPanel,
    index: u16,
    action: WidgetAction,
) -> bool {
    // Nothing here is numeric, so a SetValue has no meaning.
    if index as usize >= BLOCKS_ROW_COUNT
        || matches!(action, WidgetAction::SetValue(_) | WidgetAction::SetText(_))
    {
        return false;
    }
    match index {
        row::ENABLED => sp.blocks_enabled = !sp.blocks_enabled,
        row::BORDER_WIDTH => {
            sp.blocks_border_width_px = match action {
                WidgetAction::Prev if sp.blocks_border_width_px <= 1 => BORDER_WIDTH_MAX,
                WidgetAction::Prev => sp.blocks_border_width_px - 1,
                _ if sp.blocks_border_width_px >= BORDER_WIDTH_MAX => 1,
                _ => sp.blocks_border_width_px + 1,
            }
        }
        row::STATUS_BADGE => sp.blocks_show_exit_code_badge = !sp.blocks_show_exit_code_badge,
        _ => return false,
    }
    sp.dirty = true;
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

    #[test]
    fn describes_every_row_once() {
        let descs = blocks_widget_descs(&SettingsPanel::default());
        assert_eq!(descs.len(), BLOCKS_ROW_COUNT);
        for (i, d) in descs.iter().enumerate() {
            assert_eq!(d.id.index as usize, i);
            assert!(!d.label.is_empty());
        }
    }

    #[test]
    fn no_row_reports_focus() {
        // Blocks has no keyboard focus counter; claiming focus would draw a
        // ring the keyboard can never move.
        assert!(
            blocks_widget_descs(&SettingsPanel::default())
                .iter()
                .all(|d| !d.focused)
        );
    }

    #[test]
    fn toggles_flip_and_mark_dirty() {
        let mut sp = SettingsPanel {
            dirty: false,
            ..Default::default()
        };
        let before = sp.blocks_enabled;
        assert!(apply_blocks_action(
            &mut sp,
            row::ENABLED,
            WidgetAction::Activate
        ));
        assert_eq!(sp.blocks_enabled, !before);
        assert!(sp.dirty, "the Blocks tab saves immediately");
    }

    #[test]
    fn border_width_wraps_in_both_directions() {
        let mut sp = SettingsPanel {
            blocks_border_width_px: BORDER_WIDTH_MAX,
            ..Default::default()
        };
        apply_blocks_action(&mut sp, row::BORDER_WIDTH, WidgetAction::Next);
        assert_eq!(sp.blocks_border_width_px, 1, "wraps forward");

        apply_blocks_action(&mut sp, row::BORDER_WIDTH, WidgetAction::Prev);
        assert_eq!(sp.blocks_border_width_px, BORDER_WIDTH_MAX, "wraps back");
    }

    #[test]
    fn a_numeric_set_value_is_refused() {
        let mut sp = SettingsPanel::default();
        assert!(!apply_blocks_action(
            &mut sp,
            row::ENABLED,
            WidgetAction::SetValue(1.0)
        ));
    }

    #[test]
    fn rows_stack_without_overlapping() {
        let specs = build_blocks_widgets(&SettingsPanel::default(), &geometry());
        for pair in specs.windows(2) {
            assert!(pair[0].rect.y + pair[0].rect.h <= pair[1].rect.y + 0.001);
        }
    }

    #[test]
    fn the_tip_line_sits_below_the_last_visible_row() {
        let sp = SettingsPanel::default();
        let g = geometry();
        let specs = build_blocks_widgets(&sp, &g);
        let last = specs.last().expect("rows present");
        assert!(tip_y(&sp, &g) >= last.rect.y);
    }
}
