//! Widget specs for the Profiles settings category (UI/UX v3 phase P1c).
//!
//! The first list-shaped tab on the widget layer, and the degenerate form of
//! the shape: one active-profile cycler above a read-only entry list. There
//! is no focus counter — `selected_profile` (which list entry is selected)
//! and `active_profile_index` (which profile is applied, 0 = none) are the
//! only state, and they are deliberately distinct: `ListItem { selected }`
//! reflects the former, the cycler's value the latter.

use crate::settings_panel::SettingsPanel;

use super::super::settings::layout::LIST_ROW_PITCH;
use super::action::WidgetAction;
use super::geometry::TabGeometry;
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Profiles category.
pub(crate) const PROFILES_CATEGORY: u8 = 6;

/// Widget indices. The list is user-populated, so unlike the row-shaped tabs
/// there is no fixed row count: entry `i` lives at `LIST_BASE + i`.
pub(crate) mod row {
    /// Active-profile cycler (absent while no profiles exist).
    pub const ACTIVE: u16 = 0;
    /// First list entry.
    pub const LIST_BASE: u16 = 1;
}

/// Offset of the cycler row below the content top, in cell heights.
const ACTIVE_ROW_Y: f32 = 1.7;
/// Cycler row box height, in cell heights.
const ACTIVE_ROW_H: f32 = 1.2;
/// Offset of the first list entry below the content top, in cell heights
/// (the cycler row occupies the space in between).
const LIST_TOP: f32 = 3.0;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;

/// Describe every control of the Profiles tab, without laying it out.
///
/// Empty list → no widgets at all: the empty state is prose in the renderer,
/// and a cycler with nothing to activate would be a lie.
pub(crate) fn profiles_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    if sp.profiles.is_empty() {
        return Vec::new();
    }
    let active_value = sp
        .active_profile_name()
        .map(str::to_string)
        .unwrap_or_else(|| nexterm_i18n::fl!("settings-profiles-none"));
    let mut descs = Vec::with_capacity(1 + sp.profiles.len());
    descs.push(WidgetDesc::new(
        WidgetId::new(PROFILES_CATEGORY, row::ACTIVE),
        WidgetKind::Cycle {
            value: active_value,
        },
        nexterm_i18n::fl!("settings-profiles-active"),
    ));
    for (i, prof) in sp.profiles.iter().enumerate() {
        let label = if prof.icon.is_empty() {
            prof.name.clone()
        } else {
            format!("{} {}", prof.icon, prof.name)
        };
        descs.push(WidgetDesc::new(
            WidgetId::new(PROFILES_CATEGORY, row::LIST_BASE + i as u16),
            WidgetKind::ListItem {
                selected: sp.selected_profile == i,
            },
            label,
        ));
    }
    descs
}

/// Lay the Profiles tab out for this frame.
pub(crate) fn build_profiles_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    let hovered = sp
        .hover_widget
        .filter(|h| h.category == PROFILES_CATEGORY)
        .map(|h| h.index);

    profiles_widget_descs(sp)
        .into_iter()
        .map(|desc| {
            let matched = sp.label_matches_search(&desc.label);
            let desc = desc.search_match(matched);
            let index = desc.id.index;
            let x = g.content_inner_x - g.cell_w * ROW_BLEED;
            let row_w = (g.content_w - g.cell_w * (ROW_BLEED + 0.4)).max(0.0);
            let (rect, control) = if index == row::ACTIVE {
                let y = g.content_top + g.cell_h * ACTIVE_ROW_Y;
                let rect = WidgetRect::new(x, y - g.cell_h * 0.1, row_w, g.cell_h * ACTIVE_ROW_H);
                let control = WidgetRect::new(
                    g.content_inner_x + layout.control_x_off,
                    rect.y,
                    layout.control_w,
                    rect.h,
                );
                (rect, control)
            } else {
                let entry = (index - row::LIST_BASE) as f32;
                let y = g.content_top + g.cell_h * (LIST_TOP + entry * LIST_ROW_PITCH);
                // A list entry spans the whole row; there is no separate
                // control column, so the control rect is the row itself.
                let rect = WidgetRect::new(x, y - g.cell_h * 0.1, row_w, g.cell_h);
                (rect, rect)
            };
            desc.place(rect, control).hovered(hovered == Some(index))
        })
        .collect()
}

/// Apply an action to the Profiles widget at `index`.
///
/// The cycler steps `active_profile_index` (a config change: it marks the
/// panel dirty via the shared cycle methods); activating a list entry moves
/// `selected_profile`, which is UI state and stays clean.
pub(crate) fn apply_profiles_action(
    sp: &mut SettingsPanel,
    index: u16,
    action: WidgetAction,
) -> bool {
    // Nothing here is numeric or typed.
    if matches!(action, WidgetAction::SetValue(_) | WidgetAction::SetText(_)) {
        return false;
    }
    if index == row::ACTIVE {
        if sp.profiles.is_empty() {
            return false;
        }
        match action {
            WidgetAction::Prev => sp.prev_active_profile(),
            _ => sp.next_active_profile(),
        }
        return true;
    }
    let entry = (index - row::LIST_BASE) as usize;
    if entry >= sp.profiles.len() || !matches!(action, WidgetAction::Activate) {
        return false;
    }
    sp.selected_profile = entry;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_panel::{ProfileEntry, SettingsPanel};

    fn geometry() -> TabGeometry {
        TabGeometry {
            content_top: 100.0,
            content_inner_x: 200.0,
            content_w: 600.0,
            cell_w: 10.0,
            cell_h: 20.0,
        }
    }

    fn panel_with_profiles(names: &[&str]) -> SettingsPanel {
        let mut sp = SettingsPanel::default();
        for name in names {
            sp.profiles.push(ProfileEntry {
                name: name.to_string(),
                ..Default::default()
            });
        }
        sp
    }

    #[test]
    fn no_profiles_produce_no_widgets() {
        // The empty state stays prose in the renderer; a cycler with nothing
        // to activate would be a lie.
        assert!(profiles_widget_descs(&SettingsPanel::default()).is_empty());
    }

    #[test]
    fn describes_the_active_cycler_then_every_entry() {
        let mut sp = panel_with_profiles(&["work", "personal", "demo"]);
        sp.selected_profile = 1;
        let descs = profiles_widget_descs(&sp);
        assert_eq!(descs.len(), 4);
        assert_eq!(descs[0].id.index, row::ACTIVE);
        assert!(matches!(descs[0].kind, WidgetKind::Cycle { .. }));
        for (i, d) in descs[1..].iter().enumerate() {
            assert_eq!(d.id.index, row::LIST_BASE + i as u16);
            assert_eq!(
                d.kind,
                WidgetKind::ListItem {
                    selected: i == sp.selected_profile
                }
            );
        }
    }

    #[test]
    fn no_row_reports_focus() {
        // Profiles has no keyboard focus counter; claiming focus would draw
        // a ring the keyboard can never move.
        let sp = panel_with_profiles(&["work"]);
        assert!(profiles_widget_descs(&sp).iter().all(|d| !d.focused));
    }

    #[test]
    fn the_cycler_shows_the_none_placeholder_when_nothing_is_active() {
        let sp = panel_with_profiles(&["work"]);
        assert_eq!(sp.active_profile_index, 0);
        let descs = profiles_widget_descs(&sp);
        let WidgetKind::Cycle { value } = &descs[0].kind else {
            panic!("row 0 must be the cycler");
        };
        assert_eq!(value, &nexterm_i18n::fl!("settings-profiles-none"));
    }

    #[test]
    fn the_cycler_shows_the_active_profile_name() {
        let mut sp = panel_with_profiles(&["work", "personal"]);
        sp.next_active_profile();
        let descs = profiles_widget_descs(&sp);
        let WidgetKind::Cycle { value } = &descs[0].kind else {
            panic!("row 0 must be the cycler");
        };
        assert_eq!(value, "work");
    }

    #[test]
    fn list_labels_prefix_the_icon_only_when_present() {
        let mut sp = panel_with_profiles(&["bare"]);
        sp.profiles[0].icon = String::new();
        sp.profiles.push(ProfileEntry {
            name: "iconed".to_string(),
            icon: "λ".to_string(),
            ..Default::default()
        });
        let descs = profiles_widget_descs(&sp);
        assert_eq!(descs[1].label, "bare", "an empty icon adds no space");
        assert_eq!(descs[2].label, "λ iconed");
    }

    #[test]
    fn rows_stack_without_overlapping() {
        let sp = panel_with_profiles(&["a", "b", "c"]);
        let specs = build_profiles_widgets(&sp, &geometry());
        for pair in specs.windows(2) {
            assert!(pair[0].rect.y + pair[0].rect.h <= pair[1].rect.y + 0.001);
        }
    }

    #[test]
    fn activating_a_list_entry_selects_it_without_marking_dirty() {
        let mut sp = panel_with_profiles(&["a", "b", "c"]);
        assert!(apply_profiles_action(
            &mut sp,
            row::LIST_BASE + 2,
            WidgetAction::Activate
        ));
        assert_eq!(sp.selected_profile, 2);
        assert!(!sp.dirty, "list selection is UI state, not a config change");
    }

    #[test]
    fn the_cycler_steps_the_active_profile_in_both_directions() {
        let mut sp = panel_with_profiles(&["a", "b"]);
        assert!(apply_profiles_action(
            &mut sp,
            row::ACTIVE,
            WidgetAction::Next
        ));
        assert_eq!(sp.active_profile_name(), Some("a"));
        assert!(sp.dirty, "the active profile persists to config.toml");

        assert!(apply_profiles_action(
            &mut sp,
            row::ACTIVE,
            WidgetAction::Prev
        ));
        assert_eq!(sp.active_profile_name(), None, "wraps back to none");
    }

    #[test]
    fn a_click_on_the_cycler_steps_forward() {
        // Mouse clicks arrive as Activate, matching the other cyclers.
        let mut sp = panel_with_profiles(&["a"]);
        assert!(apply_profiles_action(
            &mut sp,
            row::ACTIVE,
            WidgetAction::Activate
        ));
        assert_eq!(sp.active_profile_name(), Some("a"));
    }

    #[test]
    fn out_of_range_and_typed_actions_are_refused() {
        let mut sp = panel_with_profiles(&["a"]);
        assert!(!apply_profiles_action(
            &mut sp,
            row::LIST_BASE + 5,
            WidgetAction::Activate
        ));
        assert!(!apply_profiles_action(
            &mut sp,
            row::ACTIVE,
            WidgetAction::SetValue(1.0)
        ));
        assert!(!apply_profiles_action(
            &mut sp,
            row::ACTIVE,
            WidgetAction::SetText("x".to_string())
        ));
        // With no profiles at all, even the cycler refuses.
        let mut empty = SettingsPanel::default();
        assert!(!apply_profiles_action(
            &mut empty,
            row::ACTIVE,
            WidgetAction::Next
        ));
    }
}
