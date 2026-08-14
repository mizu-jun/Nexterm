//! Widget specs for the Keybindings settings category (UI/UX v3 phase P1c).
//!
//! The last list-shaped tab, and `WidgetKind::KeyCapture`'s first consumer:
//! a windowed binding list, a key/action edit pair for the selected binding,
//! the Add/Delete buttons, and the always-present leader-key text row below
//! them. Widget indices 1..=5 mirror the existing `key_field_focus` counter
//! (1 key, 2 action, 3 Add, 4 Delete, 5 leader key) as the identity; list
//! entries live at `LIST_BASE + i`. The delete-confirmation dialog is a
//! modal over the whole panel and deliberately stays outside this module.

use crate::settings_panel::{KEYBINDING_ACTIONS, KeyEditMode, SettingsPanel};

use super::super::settings::layout::{LIST_ROW_PITCH, ListWindow, MAX_LIST_ROWS, list_window};
use super::action::WidgetAction;
use super::geometry::TabGeometry;
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Keybindings category.
pub(crate) const KEYBINDINGS_CATEGORY: u8 = 5;

/// Widget indices. 1..=5 mirror `key_field_focus` exactly (0 there means
/// "the list", whose entries are addressed via [`row::LIST_BASE`]` + i`).
pub(crate) mod row {
    /// Key-combination capture of the selected binding.
    pub const FIELD_KEY: u16 = 1;
    /// Action cycler of the selected binding.
    pub const FIELD_ACTION: u16 = 2;
    /// Add-binding button.
    pub const ADD: u16 = 3;
    /// Delete-binding button (disabled while the list is empty).
    pub const DELETE: u16 = 4;
    /// Leader-key text row, always present below the buttons.
    pub const LEADER: u16 = 5;
    /// First list entry.
    pub const LIST_BASE: u16 = 6;
}

/// Offset of the first list row below the content top, in cell heights.
const LIST_TOP: f32 = 1.5;
/// Gap between the windowed list block and the edit-panel header.
const FIELDS_GAP: f32 = 1.4;
/// Offset of the first field row below the edit-panel header.
const FIELD_FIRST: f32 = 1.3;
/// Vertical pitch of the field rows.
const FIELD_PITCH: f32 = 1.1;
/// Gap between the last field row and the button row.
const BUTTONS_GAP: f32 = 2.0;
/// Gap between the button row and the leader-key row.
const LEADER_GAP: f32 = 3.0;
/// Button width/height and the gap between Add and Delete, in cells.
const BTN_W_CELLS: f32 = 26.0;
const BTN_H_CELLS: f32 = 1.4;
const BTN_GAP_CELLS: f32 = 2.0;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;
/// Button-row offset below the content top while the list is empty.
const EMPTY_BUTTONS_TOP: f32 = 4.0;

/// The bounded window over the binding list for the current selection.
/// Shared with the renderer, which draws the range-indicator row from it.
pub(in crate::renderer) fn key_list_window(sp: &SettingsPanel) -> ListWindow {
    list_window(sp.keybindings.len(), sp.selected_key_index, MAX_LIST_ROWS)
}

/// Y position of the edit-panel header: right below the windowed list.
/// Shared with the renderer's dynamic header prose so the two cannot drift.
pub(in crate::renderer) fn key_fields_top(
    sp: &SettingsPanel,
    content_top: f32,
    cell_h: f32,
) -> f32 {
    content_top + cell_h * (LIST_TOP + key_list_window(sp).block_rows() + FIELDS_GAP)
}

/// Y position of the leader-key row.
///
/// Shared with the renderer, which hangs the edit hint and the duplicate-chord
/// warning off the bottom of this row.
pub(in crate::renderer) fn key_leader_y(sp: &SettingsPanel, content_top: f32, cell_h: f32) -> f32 {
    buttons_y(sp, content_top, cell_h) + cell_h * LEADER_GAP
}

/// Y position of the Add/Delete button row.
fn buttons_y(sp: &SettingsPanel, content_top: f32, cell_h: f32) -> f32 {
    if sp.keybindings.is_empty() {
        content_top + cell_h * EMPTY_BUTTONS_TOP
    } else {
        let last_field_y =
            key_fields_top(sp, content_top, cell_h) + cell_h * (FIELD_FIRST + FIELD_PITCH);
        last_field_y + cell_h * BUTTONS_GAP
    }
}

/// The action value shown in the cycler: the action name plus its position
/// in `KEYBINDING_ACTIONS`, or the localized "(unknown)" marker for a value
/// outside the fixed list.
fn action_display(action: &str) -> String {
    match KEYBINDING_ACTIONS.iter().position(|&a| a == action) {
        Some(i) => format!("{} ({}/{})", action, i + 1, KEYBINDING_ACTIONS.len()),
        None => nexterm_i18n::fl!("settings-keybindings-action-unknown", action = action),
    }
}

/// Describe every control of the Keybindings tab, without laying it out.
///
/// The Add/Delete buttons and the leader-key row are always present (the
/// leader key exists independently of the binding list); the list and the
/// key/action pair exist only while there is a binding to show.
pub(crate) fn keybindings_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    let empty = sp.keybindings.is_empty();
    let mut descs = Vec::with_capacity(sp.keybindings.len() + 5);
    if !empty {
        let sel = sp.selected_key_index.min(sp.keybindings.len() - 1);
        for (i, kb) in sp.keybindings.iter().enumerate() {
            descs.push(
                WidgetDesc::new(
                    WidgetId::new(KEYBINDINGS_CATEGORY, row::LIST_BASE + i as u16),
                    WidgetKind::ListItem { selected: sel == i },
                    kb.label(),
                )
                .focused(sp.key_field_focus == 0 && sel == i),
            );
        }

        let kb = &sp.keybindings[sel];
        // Text-mode editing exposes the live buffer as a text widget;
        // otherwise the key is a capture control that reflects Record mode.
        let key_kind = match &sp.key_editing {
            Some(KeyEditMode::Text(state)) => WidgetKind::Text {
                value: state.display_string(),
                editing: true,
                caret: Some(state.display_cursor()),
            },
            Some(KeyEditMode::Record) => WidgetKind::KeyCapture {
                value: kb.key.clone(),
                recording: true,
            },
            None => WidgetKind::KeyCapture {
                value: kb.key.clone(),
                recording: false,
            },
        };
        descs.push(
            WidgetDesc::new(
                WidgetId::new(KEYBINDINGS_CATEGORY, row::FIELD_KEY),
                key_kind,
                nexterm_i18n::fl!("settings-keybindings-field-key"),
            )
            .focused(sp.key_field_focus == 1),
        );
        descs.push(
            WidgetDesc::new(
                WidgetId::new(KEYBINDINGS_CATEGORY, row::FIELD_ACTION),
                WidgetKind::Cycle {
                    value: action_display(&kb.action),
                },
                nexterm_i18n::fl!("settings-keybindings-field-action"),
            )
            .focused(sp.key_field_focus == 2)
            // An action outside `KEYBINDING_ACTIONS` never dispatches, so the
            // row reads as invalid regardless of focus.
            .invalid(!KEYBINDING_ACTIONS.contains(&kb.action.as_str())),
        );
    }
    descs.push(
        WidgetDesc::new(
            WidgetId::new(KEYBINDINGS_CATEGORY, row::ADD),
            WidgetKind::Button { destructive: false },
            nexterm_i18n::fl!("settings-keybindings-add"),
        )
        .focused(sp.key_field_focus == 3),
    );
    let delete_label = if empty {
        nexterm_i18n::fl!("settings-keybindings-delete-disabled")
    } else {
        nexterm_i18n::fl!("settings-keybindings-delete")
    };
    let mut delete = WidgetDesc::new(
        WidgetId::new(KEYBINDINGS_CATEGORY, row::DELETE),
        WidgetKind::Button { destructive: true },
        delete_label,
    )
    .focused(!empty && sp.key_field_focus == 4);
    delete.enabled = !empty;
    descs.push(delete);

    let leader_kind = match &sp.leader_key_editing {
        Some(state) => WidgetKind::Text {
            value: state.display_string(),
            editing: true,
            caret: Some(state.display_cursor()),
        },
        None => WidgetKind::Text {
            value: sp.leader_key.clone(),
            editing: false,
            caret: None,
        },
    };
    descs.push(
        WidgetDesc::new(
            WidgetId::new(KEYBINDINGS_CATEGORY, row::LEADER),
            leader_kind,
            nexterm_i18n::fl!("settings-keybindings-leader-key"),
        )
        .focused(sp.key_field_focus == 5),
    );
    descs
}

/// Lay the Keybindings tab out for this frame.
///
/// List entries outside the bounded window collapse to a zero rect — absent
/// for `hit_test` and `draw_widget`, still described for the AccessKit tree.
pub(crate) fn build_keybindings_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    let hovered = sp
        .hover_widget
        .filter(|h| h.category == KEYBINDINGS_CATEGORY)
        .map(|h| h.index);
    let w = key_list_window(sp);
    let fields_top = key_fields_top(sp, g.content_top, g.cell_h);
    let btn_y = buttons_y(sp, g.content_top, g.cell_h);
    let leader_y = key_leader_y(sp, g.content_top, g.cell_h);

    let x = g.content_inner_x - g.cell_w * ROW_BLEED;
    let row_w = (g.content_w - g.cell_w * (ROW_BLEED + 0.4)).max(0.0);
    let field_rect = |y: f32| {
        let rect = WidgetRect::new(x, y - g.cell_h * 0.1, row_w, g.cell_h);
        let control = WidgetRect::new(
            g.content_inner_x + layout.control_x_off,
            rect.y,
            layout.control_w,
            rect.h,
        );
        (rect, control)
    };

    keybindings_widget_descs(sp)
        .into_iter()
        .map(|desc| {
            let matched = sp.label_matches_search(&desc.label);
            let desc = desc.search_match(matched);
            let index = desc.id.index;
            let (rect, control) = if index >= row::LIST_BASE {
                let i = (index - row::LIST_BASE) as usize;
                if i < w.first || i >= w.first + w.visible {
                    (WidgetRect::default(), WidgetRect::default())
                } else {
                    let y = g.content_top
                        + g.cell_h * (LIST_TOP + (i - w.first) as f32 * LIST_ROW_PITCH);
                    let rect = WidgetRect::new(x, y - g.cell_h * 0.1, row_w, g.cell_h);
                    (rect, rect)
                }
            } else if index == row::ADD || index == row::DELETE {
                let btn_w = g.cell_w * BTN_W_CELLS;
                let bx = if index == row::ADD {
                    x
                } else {
                    x + btn_w + g.cell_w * BTN_GAP_CELLS
                };
                let rect =
                    WidgetRect::new(bx, btn_y - g.cell_h * 0.15, btn_w, g.cell_h * BTN_H_CELLS);
                (rect, rect)
            } else if index == row::LEADER {
                field_rect(leader_y)
            } else {
                let fi = (index - row::FIELD_KEY) as f32;
                field_rect(fields_top + g.cell_h * (FIELD_FIRST + fi * FIELD_PITCH))
            };
            desc.place(rect, control).hovered(hovered == Some(index))
        })
        .collect()
}

/// Apply an action to the Keybindings widget at `index`.
///
/// One router for the mouse and the AccessKit path: activating a list entry
/// selects it, clicking the key field starts recording (SetText overwrites
/// the spelling directly), the action cycler steps or takes a validated
/// direct value, Add appends + starts recording, Delete opens the dialog,
/// and the leader row opens its editor (SetText writes it directly).
pub(crate) fn apply_keybindings_action(
    sp: &mut SettingsPanel,
    index: u16,
    action: WidgetAction,
) -> bool {
    if index >= row::LIST_BASE {
        let i = (index - row::LIST_BASE) as usize;
        if i >= sp.keybindings.len() || !matches!(action, WidgetAction::Activate) {
            return false;
        }
        sp.selected_key_index = i;
        sp.key_field_focus = 0;
        return true;
    }
    // The key/action pair and Delete need a binding to act on; Add and the
    // leader row work on an empty list too.
    if sp.keybindings.is_empty()
        && matches!(index, row::FIELD_KEY | row::FIELD_ACTION | row::DELETE)
    {
        return false;
    }
    match index {
        row::FIELD_KEY => match action {
            WidgetAction::Activate => {
                sp.key_field_focus = 1;
                sp.begin_key_record()
            }
            WidgetAction::SetText(text) => {
                let updated = sp.set_keybinding_key_direct(text);
                sp.key_field_focus = 1;
                updated
            }
            _ => false,
        },
        row::FIELD_ACTION => {
            match action {
                WidgetAction::Activate | WidgetAction::Next => {
                    sp.next_key_action();
                }
                WidgetAction::Prev => {
                    sp.prev_key_action();
                }
                WidgetAction::SetText(text) => {
                    if !sp.set_keybinding_action_direct(&text) {
                        return false;
                    }
                }
                _ => return false,
            }
            sp.key_field_focus = 2;
            true
        }
        row::ADD if matches!(action, WidgetAction::Activate) => {
            sp.add_key_binding();
            true
        }
        row::DELETE if matches!(action, WidgetAction::Activate) => {
            sp.open_key_delete_dialog();
            true
        }
        row::LEADER => match action {
            WidgetAction::Activate => {
                sp.key_field_focus = 5;
                sp.begin_leader_key_edit()
            }
            WidgetAction::SetText(text) => {
                sp.leader_key = text;
                sp.key_field_focus = 5;
                sp.dirty = true;
                true
            }
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_panel::{KeyBindingEntry, SettingsPanel};

    fn geometry() -> TabGeometry {
        TabGeometry {
            content_top: 100.0,
            content_inner_x: 200.0,
            content_w: 600.0,
            cell_w: 10.0,
            cell_h: 20.0,
        }
    }

    /// A panel with no bindings at all: `SettingsPanel::default()` ships with
    /// the preloaded default bindings, which these tests must not see.
    fn empty_panel() -> SettingsPanel {
        let mut sp = SettingsPanel::default();
        sp.keybindings.clear();
        sp
    }

    fn panel_with_bindings(n: usize) -> SettingsPanel {
        let mut sp = empty_panel();
        for i in 0..n {
            sp.keybindings.push(KeyBindingEntry {
                key: format!("ctrl+{i}"),
                action: KEYBINDING_ACTIONS[0].to_string(),
            });
        }
        sp
    }

    #[test]
    fn an_empty_list_exposes_the_buttons_and_the_leader_row() {
        let descs = keybindings_widget_descs(&empty_panel());
        assert_eq!(descs.len(), 3);
        assert_eq!(descs[0].id.index, row::ADD);
        assert!(descs[0].enabled);
        assert_eq!(descs[1].id.index, row::DELETE);
        assert!(!descs[1].enabled, "nothing to delete");
        assert_eq!(descs[2].id.index, row::LEADER);
        assert!(descs[2].enabled, "the leader key exists without bindings");
    }

    #[test]
    fn describes_list_fields_buttons_and_leader_in_reading_order() {
        let mut sp = panel_with_bindings(2);
        sp.selected_key_index = 1;
        let descs = keybindings_widget_descs(&sp);
        assert_eq!(descs.len(), 2 + 2 + 3);
        assert_eq!(descs[0].id.index, row::LIST_BASE);
        assert_eq!(
            descs[1].kind,
            WidgetKind::ListItem { selected: true },
            "entry 1 is selected"
        );
        assert_eq!(descs[2].id.index, row::FIELD_KEY);
        assert_eq!(descs[3].id.index, row::FIELD_ACTION);
        assert_eq!(descs[4].id.index, row::ADD);
        assert_eq!(descs[5].id.index, row::DELETE);
        assert_eq!(descs[6].id.index, row::LEADER);
    }

    #[test]
    fn the_key_field_is_a_key_capture_that_reflects_record_mode() {
        let mut sp = panel_with_bindings(1);
        let descs = keybindings_widget_descs(&sp);
        let key = descs.iter().find(|d| d.id.index == row::FIELD_KEY).unwrap();
        assert_eq!(
            key.kind,
            WidgetKind::KeyCapture {
                value: "ctrl+0".to_string(),
                recording: false,
            }
        );

        sp.key_field_focus = 1;
        assert!(sp.begin_key_record());
        let descs = keybindings_widget_descs(&sp);
        let key = descs.iter().find(|d| d.id.index == row::FIELD_KEY).unwrap();
        assert_eq!(
            key.kind,
            WidgetKind::KeyCapture {
                value: "ctrl+0".to_string(),
                recording: true,
            }
        );
    }

    #[test]
    fn a_text_mode_edit_turns_the_key_field_into_a_text_widget() {
        let mut sp = panel_with_bindings(1);
        sp.key_field_focus = 1;
        assert!(sp.begin_key_record());
        sp.toggle_key_edit_mode(); // Record -> Text
        sp.key_field_insert_char('x');
        let descs = keybindings_widget_descs(&sp);
        let key = descs.iter().find(|d| d.id.index == row::FIELD_KEY).unwrap();
        let WidgetKind::Text { editing, caret, .. } = &key.kind else {
            panic!("Text-mode editing must expose a text widget");
        };
        assert!(editing);
        assert!(caret.is_some());
    }

    #[test]
    fn the_action_field_cycles_and_shows_its_position() {
        let sp = panel_with_bindings(1);
        let descs = keybindings_widget_descs(&sp);
        let action = descs
            .iter()
            .find(|d| d.id.index == row::FIELD_ACTION)
            .unwrap();
        let WidgetKind::Cycle { value } = &action.kind else {
            panic!("action must be a cycler");
        };
        assert!(
            value.starts_with(&format!("{} (1/", KEYBINDING_ACTIONS[0])),
            "position shown: {value}"
        );
    }

    #[test]
    fn an_action_outside_the_fixed_list_marks_the_field_invalid() {
        let mut sp = panel_with_bindings(1);
        let action_field = |sp: &SettingsPanel| {
            keybindings_widget_descs(sp)
                .into_iter()
                .find(|d| d.id.index == row::FIELD_ACTION)
                .expect("action field")
        };
        assert!(!action_field(&sp).invalid, "a known action is valid");

        sp.keybindings[0].action = "typo_not_an_action".to_string();
        let invalid = action_field(&sp);
        assert!(invalid.invalid, "an unknown action must read as invalid");
        let WidgetKind::Cycle { value } = &invalid.kind else {
            panic!("action must be a cycler");
        };
        assert!(
            value.contains("typo_not_an_action"),
            "the offending value stays visible: {value}"
        );
    }

    #[test]
    fn the_leader_row_shows_the_stored_key_or_the_edit_buffer() {
        let mut sp = empty_panel();
        sp.leader_key = "ctrl+a".to_string();
        let descs = keybindings_widget_descs(&sp);
        let leader = descs.iter().find(|d| d.id.index == row::LEADER).unwrap();
        assert_eq!(
            leader.kind,
            WidgetKind::Text {
                value: "ctrl+a".to_string(),
                editing: false,
                caret: None,
            }
        );

        sp.key_field_focus = 5;
        assert!(sp.begin_leader_key_edit());
        sp.leader_key_insert_char('b');
        let descs = keybindings_widget_descs(&sp);
        let leader = descs.iter().find(|d| d.id.index == row::LEADER).unwrap();
        let WidgetKind::Text { editing, caret, .. } = &leader.kind else {
            panic!("leader must stay a text widget");
        };
        assert!(editing);
        assert!(caret.is_some());
    }

    #[test]
    fn the_field_focus_counter_maps_onto_widget_indices_as_identity() {
        let mut sp = panel_with_bindings(1);
        for focus in 1u8..=5 {
            sp.key_field_focus = focus;
            let descs = keybindings_widget_descs(&sp);
            let focused: Vec<u16> = descs
                .iter()
                .filter(|d| d.focused)
                .map(|d| d.id.index)
                .collect();
            assert_eq!(focused, vec![focus as u16], "focus={focus}");
        }
    }

    #[test]
    fn only_the_windowed_slice_of_a_long_list_gets_a_rect() {
        let mut sp = panel_with_bindings(20);
        sp.selected_key_index = 0;
        let specs = build_keybindings_widgets(&sp, &geometry());
        let rect_of = |index: u16| {
            specs
                .iter()
                .find(|s| s.id().index == index)
                .unwrap_or_else(|| panic!("widget {index} missing"))
                .rect
        };
        assert!(rect_of(row::LIST_BASE).h > 0.0);
        assert_eq!(rect_of(row::LIST_BASE + 10).h, 0.0);
        assert!(rect_of(row::FIELD_KEY).h > 0.0);
        assert!(rect_of(row::LEADER).h > 0.0);
    }

    #[test]
    fn the_leader_row_sits_below_the_button_row() {
        let sp = panel_with_bindings(2);
        let specs = build_keybindings_widgets(&sp, &geometry());
        let of = |index: u16| specs.iter().find(|s| s.id().index == index).unwrap();
        assert_eq!(of(row::ADD).rect.y, of(row::DELETE).rect.y);
        assert!(of(row::LEADER).rect.y > of(row::ADD).rect.y);
    }

    #[test]
    fn activating_a_list_entry_selects_it_and_returns_focus_to_the_list() {
        let mut sp = panel_with_bindings(3);
        sp.key_field_focus = 2;
        assert!(apply_keybindings_action(
            &mut sp,
            row::LIST_BASE + 2,
            WidgetAction::Activate
        ));
        assert_eq!(sp.selected_key_index, 2);
        assert_eq!(sp.key_field_focus, 0);
    }

    #[test]
    fn clicking_the_key_field_starts_recording_and_set_text_writes_directly() {
        let mut sp = panel_with_bindings(1);
        assert!(apply_keybindings_action(
            &mut sp,
            row::FIELD_KEY,
            WidgetAction::Activate
        ));
        assert_eq!(sp.key_field_focus, 1);
        assert!(sp.is_key_recording());

        sp.cancel_key_edit();
        assert!(apply_keybindings_action(
            &mut sp,
            row::FIELD_KEY,
            WidgetAction::SetText("alt+z".to_string())
        ));
        assert_eq!(sp.keybindings[0].key, "alt+z");
    }

    #[test]
    fn the_action_cycler_steps_and_rejects_unknown_direct_values() {
        let mut sp = panel_with_bindings(1);
        let before = sp.keybindings[0].action.clone();
        assert!(apply_keybindings_action(
            &mut sp,
            row::FIELD_ACTION,
            WidgetAction::Next
        ));
        assert_ne!(sp.keybindings[0].action, before);
        assert!(apply_keybindings_action(
            &mut sp,
            row::FIELD_ACTION,
            WidgetAction::Prev
        ));
        assert_eq!(sp.keybindings[0].action, before);
        assert!(!apply_keybindings_action(
            &mut sp,
            row::FIELD_ACTION,
            WidgetAction::SetText("no_such_action".to_string())
        ));
    }

    #[test]
    fn add_appends_and_delete_needs_something_to_delete() {
        let mut sp = empty_panel();
        assert!(apply_keybindings_action(
            &mut sp,
            row::ADD,
            WidgetAction::Activate
        ));
        assert_eq!(sp.keybindings.len(), 1);

        assert!(apply_keybindings_action(
            &mut sp,
            row::DELETE,
            WidgetAction::Activate
        ));
        assert!(sp.key_delete_dialog_open);

        let mut empty = empty_panel();
        assert!(!apply_keybindings_action(
            &mut empty,
            row::DELETE,
            WidgetAction::Activate
        ));
    }

    #[test]
    fn the_leader_row_opens_its_editor_and_accepts_set_text() {
        let mut sp = empty_panel();
        assert!(apply_keybindings_action(
            &mut sp,
            row::LEADER,
            WidgetAction::Activate
        ));
        assert_eq!(sp.key_field_focus, 5);
        assert!(sp.leader_key_editing.is_some());

        sp.cancel_leader_key_edit();
        assert!(apply_keybindings_action(
            &mut sp,
            row::LEADER,
            WidgetAction::SetText("ctrl+space".to_string())
        ));
        assert_eq!(sp.leader_key, "ctrl+space");
        assert!(sp.dirty);
    }

    #[test]
    fn out_of_range_and_mismatched_actions_are_refused() {
        let mut sp = panel_with_bindings(1);
        assert!(!apply_keybindings_action(
            &mut sp,
            row::LIST_BASE + 9,
            WidgetAction::Activate
        ));
        assert!(!apply_keybindings_action(
            &mut sp,
            row::FIELD_KEY,
            WidgetAction::SetValue(1.0)
        ));
        assert!(!apply_keybindings_action(
            &mut sp,
            0,
            WidgetAction::Activate
        ));
        // Field actions need a binding to act on.
        let mut empty = empty_panel();
        assert!(!apply_keybindings_action(
            &mut empty,
            row::FIELD_KEY,
            WidgetAction::Activate
        ));
    }
}
