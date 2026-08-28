//! Widget specs for the Font settings category (UI/UX v3 phase P1c).
//!
//! Four rows, spaced irregularly because hint lines sit between them; the
//! renderer still draws those hints, which are prose rather than controls.
//! The font-size slider keeps its drag support, now derived from the same
//! `slider_track_rect` the drawing code uses.

use crate::settings_panel::SettingsPanel;

use super::action::WidgetAction;
use super::geometry::TabGeometry;
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Font category.
pub(crate) const FONT_CATEGORY: u8 = 1;

/// Row indices, matching `focused_widget_index`.
pub(crate) mod row {
    /// Font family name (typed).
    pub const FAMILY: u16 = 0;
    /// Font size (slider).
    pub const SIZE: u16 = 1;
    /// Ligatures on/off.
    pub const LIGATURES: u16 = 2;
    /// Comma-separated fallback list (typed).
    pub const FALLBACKS: u16 = 3;
}

/// Number of rows in the Font category.
pub(crate) const FONT_ROW_COUNT: usize = 4;

/// Font-size range, matching the clamps `set_font_size_value` applies.
pub(crate) const FONT_SIZE_RANGE: (f32, f32) = (8.0, 32.0);
/// Font-size step, matching `increase_font_size` / `decrease_font_size`.
const FONT_SIZE_STEP: f32 = 0.5;

/// Y offset of each row below the content top, in cell heights. The gaps hold
/// the hint lines the renderer draws.
const ROW_OFFSETS: [f32; FONT_ROW_COUNT] = [1.0, 3.0, 6.0, 7.4];
/// Row box height, in cell heights.
const ROW_H: f32 = 1.2;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;

/// Describe every control of the Font tab, without laying it out.
pub(crate) fn font_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    let focus = sp.focused_widget_index;
    vec![
        WidgetDesc::new(
            WidgetId::new(FONT_CATEGORY, row::FAMILY),
            WidgetKind::Text {
                value: sp.font_family.clone(),
                editing: sp.font_family_editing,
                // This field tracks no cursor of its own, so the caret goes
                // to the end of the value.
                caret: None,
            },
            nexterm_i18n::fl!("settings-font-family"),
        )
        // Editing implies focus even if the counter points elsewhere.
        .focused(focus == row::FAMILY || sp.font_family_editing),
        WidgetDesc::new(
            WidgetId::new(FONT_CATEGORY, row::SIZE),
            WidgetKind::Slider {
                value: sp.font_size,
                min: FONT_SIZE_RANGE.0,
                max: FONT_SIZE_RANGE.1,
                step: FONT_SIZE_STEP,
                display: format!("{:.1}", sp.font_size),
            },
            nexterm_i18n::fl!("settings-font-size"),
        )
        .focused(focus == row::SIZE),
        WidgetDesc::new(
            WidgetId::new(FONT_CATEGORY, row::LIGATURES),
            WidgetKind::Toggle {
                on: sp.font_ligatures,
            },
            nexterm_i18n::fl!("settings-font-ligatures"),
        )
        .focused(focus == row::LIGATURES),
        WidgetDesc::new(
            WidgetId::new(FONT_CATEGORY, row::FALLBACKS),
            WidgetKind::Text {
                // While editing, the buffer is the live value.
                value: match sp.font_fallbacks_editing.as_ref() {
                    Some(state) => state.buffer.clone(),
                    None => sp.font_fallbacks_text.clone(),
                },
                editing: sp.font_fallbacks_editing.is_some(),
                caret: sp.font_fallbacks_editing.as_ref().map(|s| s.cursor),
            },
            nexterm_i18n::fl!("settings-font-fallbacks"),
        )
        .focused(focus == row::FALLBACKS || sp.font_fallbacks_editing.is_some()),
    ]
}

/// Lay the Font tab out for this frame.
pub(crate) fn build_font_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);

    font_widget_descs(sp)
        .into_iter()
        .map(|desc| {
            let matched = sp.label_matches_search(&desc.label);
            let desc = desc.search_match(matched);
            let index = desc.id.index;
            let y = g.content_top + g.cell_h * ROW_OFFSETS[index as usize];
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
            desc.place(rect, control)
        })
        .collect()
}

/// Y position of the hint line that follows the row at `index`, in pixels.
///
/// The renderer draws hints under the family, size and fallbacks rows.
pub(crate) fn hint_y(g: &TabGeometry, index: u16) -> f32 {
    let base = ROW_OFFSETS
        .get(index as usize)
        .copied()
        .unwrap_or(*ROW_OFFSETS.last().expect("ROW_OFFSETS is non-empty"));
    g.content_top + g.cell_h * (base + 0.9)
}

/// Apply an action to the Font widget at `index`.
pub(crate) fn apply_font_action(sp: &mut SettingsPanel, index: u16, action: WidgetAction) -> bool {
    if index as usize >= FONT_ROW_COUNT {
        return false;
    }
    sp.focused_widget_index = index;
    match action {
        WidgetAction::Next => sp.font_field_increase(),
        WidgetAction::Prev => sp.font_field_decrease(),
        WidgetAction::SetValue(v) => match index {
            row::SIZE => sp.set_font_size_value(v),
            // The typed fields take a string, and a toggle is not a number.
            _ => return false,
        },
        WidgetAction::SetText(text) => match index {
            row::FAMILY => {
                sp.font_family = text;
                sp.dirty = true;
            }
            row::FALLBACKS => {
                sp.font_fallbacks_text = text;
                sp.dirty = true;
            }
            _ => return false,
        },
        WidgetAction::Activate => match index {
            row::LIGATURES => sp.toggle_font_ligatures(),
            row::FAMILY => sp.font_family_editing = true,
            row::FALLBACKS => {
                sp.begin_font_fallbacks_edit();
            }
            // Clicking a slider row focuses it without nudging the value.
            _ => {}
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

    #[test]
    fn row_offsets_cover_every_row() {
        assert_eq!(ROW_OFFSETS.len(), FONT_ROW_COUNT);
    }

    #[test]
    fn describes_every_row_with_the_expected_kind() {
        let descs = font_widget_descs(&SettingsPanel::default());
        assert_eq!(descs.len(), FONT_ROW_COUNT);
        assert!(matches!(
            descs[row::FAMILY as usize].kind,
            WidgetKind::Text { .. }
        ));
        assert!(matches!(
            descs[row::SIZE as usize].kind,
            WidgetKind::Slider { .. }
        ));
        assert!(matches!(
            descs[row::LIGATURES as usize].kind,
            WidgetKind::Toggle { .. }
        ));
        assert!(matches!(
            descs[row::FALLBACKS as usize].kind,
            WidgetKind::Text { .. }
        ));
    }

    #[test]
    fn editing_a_text_field_implies_focus() {
        // Editing is entered from a click, which need not have moved the
        // focus counter; a ring-less edit box would read as inactive.
        let sp = SettingsPanel {
            focused_widget_index: row::SIZE,
            font_family_editing: true,
            ..Default::default()
        };
        let descs = font_widget_descs(&sp);
        assert!(descs[row::FAMILY as usize].focused);
    }

    #[test]
    fn the_size_slider_carries_its_real_range() {
        let sp = SettingsPanel::default();
        let WidgetKind::Slider {
            value,
            min,
            max,
            step,
            ..
        } = &font_widget_descs(&sp)[row::SIZE as usize].kind
        else {
            panic!("size must be a slider");
        };
        assert_eq!((*min, *max), FONT_SIZE_RANGE);
        assert_eq!(*step, FONT_SIZE_STEP);
        assert!(value >= min && value <= max);
    }

    #[test]
    fn set_value_clamps_through_the_existing_setter() {
        let mut sp = SettingsPanel::default();
        apply_font_action(&mut sp, row::SIZE, WidgetAction::SetValue(1000.0));
        assert!(sp.font_size <= FONT_SIZE_RANGE.1);
        apply_font_action(&mut sp, row::SIZE, WidgetAction::SetValue(-5.0));
        assert!(sp.font_size >= FONT_SIZE_RANGE.0);
    }

    #[test]
    fn set_value_is_refused_on_non_numeric_rows() {
        let mut sp = SettingsPanel::default();
        assert!(!apply_font_action(
            &mut sp,
            row::LIGATURES,
            WidgetAction::SetValue(1.0)
        ));
    }

    #[test]
    fn activating_rows_toggles_or_starts_editing() {
        let mut sp = SettingsPanel::default();
        let before = sp.font_ligatures;
        apply_font_action(&mut sp, row::LIGATURES, WidgetAction::Activate);
        assert_eq!(sp.font_ligatures, !before);

        apply_font_action(&mut sp, row::FAMILY, WidgetAction::Activate);
        assert!(sp.font_family_editing);
    }

    #[test]
    fn activating_the_slider_row_only_focuses_it() {
        let mut sp = SettingsPanel::default();
        let before = sp.font_size;
        assert!(apply_font_action(
            &mut sp,
            row::SIZE,
            WidgetAction::Activate
        ));
        assert_eq!(sp.font_size, before);
        assert_eq!(sp.focused_widget_index, row::SIZE);
    }

    #[test]
    fn an_out_of_range_row_is_refused() {
        let mut sp = SettingsPanel::default();
        assert!(!apply_font_action(
            &mut sp,
            FONT_ROW_COUNT as u16,
            WidgetAction::Activate
        ));
    }

    #[test]
    fn rows_stack_without_overlapping() {
        let specs = build_font_widgets(&SettingsPanel::default(), &geometry());
        for pair in specs.windows(2) {
            assert!(
                pair[0].rect.y + pair[0].rect.h <= pair[1].rect.y + 0.001,
                "rows {:?} and {:?} overlap",
                pair[0].id(),
                pair[1].id()
            );
        }
    }

    #[test]
    fn hints_sit_between_their_row_and_the_next() {
        let g = geometry();
        let specs = build_font_widgets(&SettingsPanel::default(), &g);
        for index in [row::FAMILY, row::SIZE] {
            let this = &specs[index as usize];
            let next = &specs[index as usize + 1];
            let hint = hint_y(&g, index);
            assert!(hint >= this.rect.y, "hint overlaps its own row");
            assert!(hint <= next.rect.y, "hint runs into the next row");
        }
    }
}
