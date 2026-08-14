//! Keyboard focus movement, derived from the widget descriptors (UI/UX v3 P1c
//! follow-up).
//!
//! Each settings category used to carry its own `next_<tab>_field` /
//! `prev_<tab>_field` pair. All five were the same function with a different
//! `*_FIELD_COUNT` constant, and each one had to be kept in step with the
//! descriptors its category builds — the exact duplication the widget layer
//! exists to remove. This module walks those descriptors instead, so a category
//! gains keyboard navigation by describing its controls and nothing else.
//!
//! Which categories this covers is deliberately bounded: see
//! [`focusable_indices`].

use crate::settings_panel::{SettingsCategory, SettingsPanel};

use super::spec::{WidgetDesc, WidgetKind};

/// Whether the keyboard focus ring stops on `desc`.
///
/// A label is not a control. A disabled control is skipped rather than focused
/// and then rejected — the pre-migration navigation hand-coded exactly this for
/// the Delete buttons of the list-shaped tabs.
///
/// Swatches are skipped for a different reason: they are a redundant mouse
/// affordance for the cycler row above them (both write `scheme_index`), so
/// giving each one a stop would add nine of them for a value the cycler already
/// sets. This preserves the pre-migration behaviour, where the Theme counter
/// addressed rows only.
fn is_focus_stop(desc: &WidgetDesc) -> bool {
    desc.enabled && desc.kind.is_interactive() && !matches!(desc.kind, WidgetKind::Swatch { .. })
}

/// Widget indices the keyboard focus ring visits in the current category, in
/// order, or `None` when the category is not driven from here.
///
/// The exclusions are not oversights:
/// - **Ssh / Keybindings** reserve index 0 for the entry list as a whole, with
///   the position inside it held in `selected_host_index` /
///   `selected_key_index`. Their descriptors instead carry one entry per list
///   row (`LIST_BASE + i`), so a plain walk would report focus indices the rest
///   of the code reads as something else. Their bespoke arrow handling stays
///   until that convention is migrated.
/// - **Profiles / Blocks** have no keyboard field navigation to preserve: an
///   arrow key in either category moves to the neighbouring category today.
pub(crate) fn focusable_indices(sp: &SettingsPanel) -> Option<Vec<u16>> {
    let descs = match sp.category {
        SettingsCategory::Window => super::settings_window::window_widget_descs(sp),
        SettingsCategory::Font => super::settings_font::font_widget_descs(sp),
        SettingsCategory::Theme => super::settings_theme::theme_widget_descs(sp),
        SettingsCategory::Startup => super::settings_startup::startup_widget_descs(sp),
        SettingsCategory::Security => super::settings_security::security_widget_descs(sp),
        SettingsCategory::Ssh
        | SettingsCategory::Keybindings
        | SettingsCategory::Profiles
        | SettingsCategory::Blocks => return None,
    };
    Some(
        descs
            .iter()
            .filter(|d| is_focus_stop(d))
            .map(|d| d.id.index)
            .collect(),
    )
}

/// Move focus to the next stop in the current category.
///
/// Returns `false` when there is none — the caller then moves to the next
/// category, matching what the per-tab helpers signalled with the same `bool`.
/// Also returns `false` for a category this module does not drive, so an
/// unmigrated one keeps falling through to its own handling.
pub(crate) fn focus_next(sp: &mut SettingsPanel) -> bool {
    step(sp, Direction::Next)
}

/// Move focus to the previous stop in the current category. See [`focus_next`].
pub(crate) fn focus_prev(sp: &mut SettingsPanel) -> bool {
    step(sp, Direction::Prev)
}

enum Direction {
    Next,
    Prev,
}

fn step(sp: &mut SettingsPanel, dir: Direction) -> bool {
    let Some(stops) = focusable_indices(sp) else {
        return false;
    };
    if stops.is_empty() {
        return false;
    }
    let current = sp.focused_widget_index;
    let target = match dir {
        // A focus index that is not itself a stop (a disabled row that went
        // disabled under the focus, say) still has a well-defined neighbour:
        // the nearest stop past it in the direction of travel.
        Direction::Next => stops.iter().find(|&&i| i > current).copied(),
        Direction::Prev => stops.iter().rev().find(|&&i| i < current).copied(),
    };
    match target {
        Some(i) => {
            sp.focused_widget_index = i;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::settings_font::FONT_ROW_COUNT;
    use super::super::settings_security::SECURITY_ROW_COUNT;
    use super::super::settings_startup::STARTUP_ROW_COUNT;
    use super::super::settings_window::WINDOW_ROW_COUNT;
    use super::*;
    use crate::settings_panel::SettingsCategory;

    fn panel(category: SettingsCategory) -> SettingsPanel {
        SettingsPanel {
            category,
            ..Default::default()
        }
    }

    /// The five categories this module drives walk 0, 1, 2, … and stop at the
    /// end, which is what their retired `next_<tab>_field` helpers did.
    #[test]
    fn focus_walks_every_row_of_a_dense_category() {
        // The expected stop counts are the values the retired
        // `<tab>_FIELD_COUNT` constants held, taken from each category's row
        // count where the widget module publishes one. Theme's 2 is a literal
        // because its swatches make the descriptor count (11) differ from its
        // row count — which is the case this test exists to pin.
        for (category, count) in [
            (SettingsCategory::Window, WINDOW_ROW_COUNT as u16),
            (SettingsCategory::Font, FONT_ROW_COUNT as u16),
            (SettingsCategory::Theme, 2u16),
            (SettingsCategory::Startup, STARTUP_ROW_COUNT as u16),
            (SettingsCategory::Security, SECURITY_ROW_COUNT as u16),
        ] {
            let mut sp = panel(category.clone());
            for expected in 1..count {
                assert!(
                    focus_next(&mut sp),
                    "{category:?} should still move at {expected}"
                );
                assert_eq!(sp.focused_widget_index, expected, "{category:?}");
            }
            assert!(
                !focus_next(&mut sp),
                "{category:?} must report the end of its rows so the caller \
                 moves to the next category"
            );
            assert_eq!(
                sp.focused_widget_index,
                count - 1,
                "{category:?} focus must not run past the last row"
            );
        }
    }

    /// And backwards, stopping at the first row.
    #[test]
    fn focus_walks_back_to_the_first_row() {
        let mut sp = panel(SettingsCategory::Window);
        sp.focused_widget_index = 2;
        assert!(focus_prev(&mut sp));
        assert_eq!(sp.focused_widget_index, 1);
        assert!(focus_prev(&mut sp));
        assert_eq!(sp.focused_widget_index, 0);
        assert!(!focus_prev(&mut sp), "the first row is the boundary");
        assert_eq!(sp.focused_widget_index, 0);
    }

    /// An index that is not itself a stop still has a well-defined neighbour.
    /// Reachable in Theme, whose swatch indices sit above the two rows: focus
    /// parked on one (a mouse click sets `focused_widget_index` directly) must
    /// still walk back into the rows rather than wedge.
    #[test]
    fn focus_recovers_from_an_index_that_is_not_a_stop() {
        let mut sp = panel(SettingsCategory::Theme);
        sp.focused_widget_index = 15; // a swatch, not a stop
        assert!(focus_prev(&mut sp), "must find the row below it");
        assert_eq!(
            sp.focused_widget_index, 1,
            "the last row before the swatches"
        );

        sp.focused_widget_index = 15;
        assert!(
            !focus_next(&mut sp),
            "nothing is a stop past the swatches, so the caller changes category"
        );
    }

    /// The Theme swatches are described but must not take a focus stop: they
    /// duplicate the cycler on row 0, so nine extra stops would be nine stops
    /// for a value already reachable.
    #[test]
    fn theme_swatches_are_not_focus_stops() {
        let sp = panel(SettingsCategory::Theme);
        let stops = focusable_indices(&sp).expect("Theme is driven here");
        assert_eq!(
            stops.len(),
            2,
            "only the two rows are stops, not the swatches: {stops:?}"
        );
        let described = super::super::settings_theme::theme_widget_descs(&sp).len();
        assert!(
            described > stops.len(),
            "the swatches must still be described for the mouse and a reader"
        );
    }

    /// The categories with their own arrow handling must report "not mine" so
    /// that handling keeps running.
    #[test]
    fn unmigrated_categories_are_left_alone() {
        for category in [
            SettingsCategory::Ssh,
            SettingsCategory::Keybindings,
            SettingsCategory::Profiles,
            SettingsCategory::Blocks,
        ] {
            let mut sp = panel(category.clone());
            sp.focused_widget_index = 1;
            assert!(focusable_indices(&sp).is_none(), "{category:?}");
            assert!(!focus_next(&mut sp), "{category:?}");
            assert!(!focus_prev(&mut sp), "{category:?}");
            assert_eq!(
                sp.focused_widget_index, 1,
                "{category:?} focus must be untouched"
            );
        }
    }

    /// A disabled row is stepped over, not focused. No category this module
    /// drives has one today, so the guarantee is asserted directly on the
    /// predicate that provides it.
    #[test]
    fn a_disabled_row_is_not_a_focus_stop() {
        use super::super::spec::{WidgetId, WidgetKind};
        let mut desc =
            WidgetDesc::new(WidgetId::new(3, 0), WidgetKind::Toggle { on: false }, "row");
        assert!(is_focus_stop(&desc));
        desc.enabled = false;
        assert!(!is_focus_stop(&desc));
    }

    /// A label is not a control.
    #[test]
    fn a_label_is_not_a_focus_stop() {
        use super::super::spec::WidgetId;
        let desc = WidgetDesc::new(WidgetId::new(3, 0), WidgetKind::Label, "note");
        assert!(!is_focus_stop(&desc));
    }
}
