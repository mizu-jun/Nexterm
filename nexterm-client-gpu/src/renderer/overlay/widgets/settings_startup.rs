//! Widget specs for the Startup settings category (UI/UX v3 phase P1c).
//!
//! Four rows: the language picker, the update-check toggle, and the two shell
//! fields. Like Font, the rows are spaced irregularly because explanatory
//! lines sit between them; those stay in the renderer.

use crate::settings_panel::{LANGUAGE_OPTIONS, SettingsPanel};

use super::action::WidgetAction;
use super::geometry::TabGeometry;
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Startup category.
pub(crate) const STARTUP_CATEGORY: u8 = 0;

/// Row indices, matching `focused_widget_index`.
pub(crate) mod row {
    /// UI language (cycler).
    pub const LANGUAGE: u16 = 0;
    /// "Check for updates on startup" toggle.
    pub const CHECK_UPDATES: u16 = 1;
    /// Shell program path (typed).
    pub const SHELL_PROGRAM: u16 = 2;
    /// Shell arguments (typed).
    pub const SHELL_ARGS: u16 = 3;
}

/// Number of rows in the Startup category.
pub(crate) const STARTUP_ROW_COUNT: usize = 4;

/// Y offset of each row below the content top, in cell heights.
const ROW_OFFSETS: [f32; STARTUP_ROW_COUNT] = [0.5, 3.0, 5.8, 7.2];
/// Row box height, in cell heights.
const ROW_H: f32 = 1.2;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;

/// Display name of the selected language, or `Auto` when the index is unset.
fn language_label(sp: &SettingsPanel) -> &'static str {
    LANGUAGE_OPTIONS
        .get(sp.language_index)
        .map(|(name, _)| *name)
        .unwrap_or("Auto")
}

/// Describe every control of the Startup tab, without laying it out.
pub(crate) fn startup_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    let focus = sp.focused_widget_index;
    // Only the focused shell field carries the live edit buffer.
    let shell_editing = |index: u16| focus == index && sp.shell_field_editing.is_some();
    let shell_value = |index: u16, committed: &str| match sp.shell_field_editing.as_ref() {
        Some(state) if focus == index => state.buffer.clone(),
        _ => committed.to_string(),
    };
    // The caret only means anything for the field that owns the live buffer.
    let shell_caret = |index: u16| match sp.shell_field_editing.as_ref() {
        Some(state) if focus == index => Some(state.cursor),
        _ => None,
    };

    vec![
        WidgetDesc::new(
            WidgetId::new(STARTUP_CATEGORY, row::LANGUAGE),
            WidgetKind::Cycle {
                value: language_label(sp).to_string(),
            },
            nexterm_i18n::fl!("settings-startup-language"),
        )
        .focused(focus == row::LANGUAGE),
        WidgetDesc::new(
            WidgetId::new(STARTUP_CATEGORY, row::CHECK_UPDATES),
            WidgetKind::Toggle {
                on: sp.auto_check_update,
            },
            nexterm_i18n::fl!("settings-startup-check-updates"),
        )
        .focused(focus == row::CHECK_UPDATES),
        WidgetDesc::new(
            WidgetId::new(STARTUP_CATEGORY, row::SHELL_PROGRAM),
            WidgetKind::Text {
                value: shell_value(row::SHELL_PROGRAM, &sp.shell_program),
                editing: shell_editing(row::SHELL_PROGRAM),
                caret: shell_caret(row::SHELL_PROGRAM),
            },
            nexterm_i18n::fl!("settings-startup-shell-program"),
        )
        .focused(focus == row::SHELL_PROGRAM),
        WidgetDesc::new(
            WidgetId::new(STARTUP_CATEGORY, row::SHELL_ARGS),
            WidgetKind::Text {
                value: shell_value(row::SHELL_ARGS, &sp.shell_args),
                editing: shell_editing(row::SHELL_ARGS),
                caret: shell_caret(row::SHELL_ARGS),
            },
            nexterm_i18n::fl!("settings-startup-shell-args"),
        )
        .focused(focus == row::SHELL_ARGS),
    ]
}

/// Lay the Startup tab out for this frame.
pub(crate) fn build_startup_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    let hovered = sp
        .hover_widget
        .filter(|h| h.category == STARTUP_CATEGORY)
        .map(|h| h.index);

    startup_widget_descs(sp)
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
            desc.place(rect, control).hovered(hovered == Some(index))
        })
        .collect()
}

/// Y position of the note that explains the language picker.
pub(crate) fn language_note_y(g: &TabGeometry) -> f32 {
    g.content_top + g.cell_h * (ROW_OFFSETS[row::CHECK_UPDATES as usize] + 1.4)
}

/// Apply an action to the Startup widget at `index`.
pub(crate) fn apply_startup_action(
    sp: &mut SettingsPanel,
    index: u16,
    action: WidgetAction,
) -> bool {
    if index as usize >= STARTUP_ROW_COUNT {
        return false;
    }
    sp.focused_widget_index = index;
    match action {
        // Nothing in this tab is numeric.
        WidgetAction::SetValue(_) => return false,
        WidgetAction::SetText(text) => match index {
            row::SHELL_PROGRAM => {
                sp.shell_program = text;
                sp.dirty = true;
            }
            row::SHELL_ARGS => {
                sp.shell_args = text;
                sp.dirty = true;
            }
            _ => return false,
        },
        WidgetAction::Next | WidgetAction::Activate if index == row::LANGUAGE => sp.next_language(),
        WidgetAction::Prev if index == row::LANGUAGE => sp.prev_language(),
        // A toggle flips whichever way it is stepped.
        _ if index == row::CHECK_UPDATES => sp.toggle_auto_check_update(),
        // The shell fields are typed: activating starts an edit, stepping
        // does nothing.
        WidgetAction::Activate => {
            sp.begin_shell_field_edit();
        }
        _ => {}
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
        assert_eq!(ROW_OFFSETS.len(), STARTUP_ROW_COUNT);
    }

    #[test]
    fn describes_every_row_with_the_expected_kind() {
        let descs = startup_widget_descs(&SettingsPanel::default());
        assert_eq!(descs.len(), STARTUP_ROW_COUNT);
        assert!(matches!(
            descs[row::LANGUAGE as usize].kind,
            WidgetKind::Cycle { .. }
        ));
        assert!(matches!(
            descs[row::CHECK_UPDATES as usize].kind,
            WidgetKind::Toggle { .. }
        ));
        assert!(matches!(
            descs[row::SHELL_PROGRAM as usize].kind,
            WidgetKind::Text { .. }
        ));
        assert!(matches!(
            descs[row::SHELL_ARGS as usize].kind,
            WidgetKind::Text { .. }
        ));
        for d in &descs {
            assert!(!d.label.is_empty());
        }
    }

    #[test]
    fn only_the_focused_shell_field_shows_the_edit_buffer() {
        let mut sp = SettingsPanel {
            focused_widget_index: row::SHELL_PROGRAM,
            ..Default::default()
        };
        sp.begin_shell_field_edit();
        sp.shell_field_insert_char('x');

        let descs = startup_widget_descs(&sp);
        let WidgetKind::Text { editing, .. } = &descs[row::SHELL_PROGRAM as usize].kind else {
            panic!("shell program must be a text field");
        };
        assert!(*editing);
        let WidgetKind::Text { editing, .. } = &descs[row::SHELL_ARGS as usize].kind else {
            panic!("shell args must be a text field");
        };
        assert!(
            !*editing,
            "the unfocused field must not borrow the other's buffer"
        );
    }

    #[test]
    fn language_falls_back_to_auto_for_an_unknown_index() {
        let sp = SettingsPanel {
            language_index: LANGUAGE_OPTIONS.len() + 10,
            ..Default::default()
        };
        assert_eq!(language_label(&sp), "Auto");
    }

    #[test]
    fn the_language_cycler_steps_both_ways() {
        let mut sp = SettingsPanel::default();
        let before = sp.language_index;
        apply_startup_action(&mut sp, row::LANGUAGE, WidgetAction::Next);
        assert_ne!(sp.language_index, before);
        apply_startup_action(&mut sp, row::LANGUAGE, WidgetAction::Prev);
        assert_eq!(sp.language_index, before);
    }

    #[test]
    fn the_update_toggle_flips_whichever_way_it_is_stepped() {
        let mut sp = SettingsPanel::default();
        let before = sp.auto_check_update;
        apply_startup_action(&mut sp, row::CHECK_UPDATES, WidgetAction::Activate);
        assert_eq!(sp.auto_check_update, !before);
        apply_startup_action(&mut sp, row::CHECK_UPDATES, WidgetAction::Next);
        assert_eq!(sp.auto_check_update, before);
    }

    #[test]
    fn activating_a_shell_field_starts_an_edit() {
        let mut sp = SettingsPanel::default();
        assert!(apply_startup_action(
            &mut sp,
            row::SHELL_PROGRAM,
            WidgetAction::Activate
        ));
        assert!(sp.shell_field_editing.is_some());
    }

    #[test]
    fn set_value_and_out_of_range_rows_are_refused() {
        let mut sp = SettingsPanel::default();
        assert!(!apply_startup_action(
            &mut sp,
            row::LANGUAGE,
            WidgetAction::SetValue(1.0)
        ));
        assert!(!apply_startup_action(
            &mut sp,
            STARTUP_ROW_COUNT as u16,
            WidgetAction::Activate
        ));
    }

    #[test]
    fn rows_stack_without_overlapping() {
        let specs = build_startup_widgets(&SettingsPanel::default(), &geometry());
        for pair in specs.windows(2) {
            assert!(pair[0].rect.y + pair[0].rect.h <= pair[1].rect.y + 0.001);
        }
    }

    #[test]
    fn the_language_note_sits_between_the_toggle_and_the_shell_fields() {
        let g = geometry();
        let specs = build_startup_widgets(&SettingsPanel::default(), &g);
        let note = language_note_y(&g);
        assert!(note >= specs[row::CHECK_UPDATES as usize].rect.y);
        assert!(note <= specs[row::SHELL_PROGRAM as usize].rect.y);
    }
}
