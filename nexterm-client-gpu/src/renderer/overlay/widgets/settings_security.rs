//! Widget specs for the Security settings category (UI/UX v3 phase P1c).
//!
//! Seven rows: four consent-policy cyclers followed by three byte-cap fields.
//! The byte caps are `Text` widgets rather than sliders because they are typed
//! into — the panel keeps an edit buffer while one is active, and the widget
//! shows that buffer instead of the committed value.

use crate::settings_panel::SettingsPanel;

use super::settings_theme::{TabGeometry, WidgetAction};
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Security category.
pub(crate) const SECURITY_CATEGORY: u8 = 8;

/// Number of rows in the Security category.
pub(crate) const SECURITY_ROW_COUNT: usize = 7;

/// Index of the first byte-cap row. Rows below this are policy cyclers.
pub(crate) const FIRST_BYTE_CAP_ROW: u8 = 4;

/// Vertical pitch between rows, in cell heights.
const ROW_PITCH: f32 = 1.4;
/// Offset of the first row below the content top, in cell heights.
const ROWS_TOP: f32 = 0.5;
/// Row box height, in cell heights.
const ROW_H: f32 = 1.2;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;

/// Describe every control of the Security tab, without laying it out.
pub(crate) fn security_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    (0..SECURITY_ROW_COUNT as u8)
        .map(|index| {
            let focused = sp.security_field_focus == index;
            let kind = if let Some(policy) = sp.security_policy_at(index) {
                WidgetKind::Cycle {
                    value: SettingsPanel::consent_display_label(policy),
                }
            } else {
                // While this field is being edited the buffer is the live
                // value; otherwise show what is committed.
                let editing = focused && sp.security_field_editing.is_some();
                let value = match (editing, sp.security_field_editing.as_ref()) {
                    (true, Some(state)) => state.buffer.clone(),
                    _ => sp.security_bytes_at(index).unwrap_or(0).to_string(),
                };
                WidgetKind::Text { value, editing }
            };
            WidgetDesc::new(
                WidgetId::new(SECURITY_CATEGORY, index),
                kind,
                SettingsPanel::security_field_label(index),
            )
            .focused(focused)
        })
        .collect()
}

/// Lay the Security tab out for this frame.
pub(crate) fn build_security_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    let visible = sp.visible_security_rows();
    let hovered = sp
        .hover_widget
        .filter(|h| h.category == SECURITY_CATEGORY)
        .map(|h| h.index);

    security_widget_descs(sp)
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

/// Y position of the footer note, below every row.
///
/// Anchored to the full row count rather than the visible one, matching the
/// pre-migration renderer.
pub(crate) fn note_y(g: &TabGeometry) -> f32 {
    g.content_top + g.cell_h * (ROWS_TOP + SECURITY_ROW_COUNT as f32 * ROW_PITCH)
}

/// Apply an action to the Security widget at `index`.
pub(crate) fn apply_security_action(
    sp: &mut SettingsPanel,
    index: u8,
    action: WidgetAction,
) -> bool {
    if index as usize >= SECURITY_ROW_COUNT {
        return false;
    }
    // The increase/decrease setters act on the focused field, as they do for
    // the keyboard.
    sp.security_field_focus = index;
    match action {
        WidgetAction::Next => sp.security_field_increase(),
        WidgetAction::Prev => sp.security_field_decrease(),
        // Activating a policy cycler steps it forward — the same thing a
        // click did before the migration. Activating a byte cap starts
        // editing it rather than changing the number.
        WidgetAction::Activate => {
            if index < FIRST_BYTE_CAP_ROW {
                sp.security_field_increase()
            } else {
                sp.begin_security_edit()
            }
        }
        // Typed fields are committed through the edit buffer, and a policy is
        // not a number: neither takes a direct numeric value.
        // Byte caps commit through the edit buffer, not a direct set.
        WidgetAction::SetValue(_) | WidgetAction::SetText(_) => return false,
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

    #[test]
    fn row_count_matches_the_panel_constant() {
        assert_eq!(
            SECURITY_ROW_COUNT,
            SettingsPanel::SECURITY_FIELD_COUNT as usize
        );
    }

    #[test]
    fn policies_are_cyclers_and_byte_caps_are_text_fields() {
        let descs = security_widget_descs(&SettingsPanel::default());
        assert_eq!(descs.len(), SECURITY_ROW_COUNT);
        for (i, d) in descs.iter().enumerate() {
            let i = i as u8;
            if i < FIRST_BYTE_CAP_ROW {
                assert!(
                    matches!(d.kind, WidgetKind::Cycle { .. }),
                    "row {i} should be a policy cycler"
                );
            } else {
                assert!(
                    matches!(d.kind, WidgetKind::Text { .. }),
                    "row {i} should be a byte-cap field"
                );
            }
            assert!(!d.label.is_empty(), "row {i} has no label");
        }
    }

    #[test]
    fn an_edited_field_shows_its_buffer() {
        let mut sp = SettingsPanel {
            security_field_focus: FIRST_BYTE_CAP_ROW,
            ..Default::default()
        };
        sp.begin_security_edit();
        sp.security_field_insert_char('7');

        let descs = security_widget_descs(&sp);
        let WidgetKind::Text { value, editing } = &descs[FIRST_BYTE_CAP_ROW as usize].kind else {
            panic!("byte caps must be text fields");
        };
        assert!(*editing);
        assert!(value.ends_with('7'), "the live buffer must be shown");
    }

    #[test]
    fn an_unfocused_field_shows_the_committed_value() {
        let mut sp = SettingsPanel {
            security_field_focus: FIRST_BYTE_CAP_ROW,
            ..Default::default()
        };
        sp.begin_security_edit();
        // Move focus away: the other byte caps must still read as committed.
        sp.security_field_focus = FIRST_BYTE_CAP_ROW + 1;

        let descs = security_widget_descs(&sp);
        let WidgetKind::Text { editing, .. } = &descs[FIRST_BYTE_CAP_ROW as usize].kind else {
            panic!("byte caps must be text fields");
        };
        assert!(!*editing);
    }

    #[test]
    fn exactly_one_row_is_focused() {
        let sp = SettingsPanel {
            security_field_focus: 3,
            ..Default::default()
        };
        let focused: Vec<_> = security_widget_descs(&sp)
            .into_iter()
            .filter(|d| d.focused)
            .collect();
        assert_eq!(focused.len(), 1);
        assert_eq!(focused[0].id.index, 3);
    }

    #[test]
    fn activating_a_policy_cycles_it() {
        let mut sp = SettingsPanel::default();
        let before = sp.security_policy_at(0);
        assert!(apply_security_action(&mut sp, 0, WidgetAction::Activate));
        assert_ne!(sp.security_policy_at(0), before);
    }

    #[test]
    fn activating_a_byte_cap_starts_editing_instead_of_changing_it() {
        let mut sp = SettingsPanel::default();
        let before = sp.security_bytes_at(FIRST_BYTE_CAP_ROW);
        assert!(apply_security_action(
            &mut sp,
            FIRST_BYTE_CAP_ROW,
            WidgetAction::Activate
        ));
        assert!(sp.security_field_editing.is_some());
        assert_eq!(sp.security_bytes_at(FIRST_BYTE_CAP_ROW), before);
    }

    #[test]
    fn set_value_and_out_of_range_rows_are_refused() {
        let mut sp = SettingsPanel::default();
        assert!(!apply_security_action(
            &mut sp,
            0,
            WidgetAction::SetValue(1.0)
        ));
        assert!(!apply_security_action(
            &mut sp,
            SECURITY_ROW_COUNT as u8,
            WidgetAction::Activate
        ));
    }

    #[test]
    fn rows_stack_without_overlapping_and_the_note_sits_below() {
        let g = geometry();
        let specs = build_security_widgets(&SettingsPanel::default(), &g);
        for pair in specs.windows(2) {
            assert!(pair[0].rect.y + pair[0].rect.h <= pair[1].rect.y + 0.001);
        }
        let last = specs.last().expect("rows present");
        assert!(note_y(&g) >= last.rect.y);
    }
}
