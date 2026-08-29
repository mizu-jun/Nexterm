//! Widget specs for the Ssh settings category (UI/UX v3 phase P1c).
//!
//! The full list-shaped form: a windowed host list, a five-field edit panel
//! for the selected host, and an Add/Delete button pair. Widget indices are
//! chosen so the existing `focused_widget_index` counter (1..=5 fields, 6 Add,
//! 7 Delete) maps onto them as the identity; list entries live at
//! `LIST_BASE + i`. The delete-confirmation dialog is a modal over the whole
//! panel, not a settings row, and deliberately stays outside this module.

use crate::settings_panel::SettingsPanel;

use super::super::settings::layout::{LIST_ROW_PITCH, ListWindow, MAX_LIST_ROWS, list_window};
use super::action::WidgetAction;
use super::geometry::TabGeometry;
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Ssh category.
pub(crate) const SSH_CATEGORY: u8 = 4;

/// Widget indices. 1..=7 mirror `focused_widget_index` exactly (0 there means
/// "the list", whose entries are addressed via [`row::LIST_BASE`]` + i`).
pub(crate) mod row {
    /// `name` text field.
    pub const FIELD_NAME: u16 = 1;
    /// `host` text field.
    pub const FIELD_HOST: u16 = 2;
    /// `port` spin button.
    pub const FIELD_PORT: u16 = 3;
    /// `username` text field.
    pub const FIELD_USERNAME: u16 = 4;
    /// `auth_type` cycler.
    pub const FIELD_AUTH: u16 = 5;
    /// Add-host button.
    pub const ADD: u16 = 6;
    /// Delete-host button (disabled while the list is empty).
    pub const DELETE: u16 = 7;
    /// First list entry.
    pub const LIST_BASE: u16 = 8;
}

/// Offset of the first list row below the content top, in cell heights.
const LIST_TOP: f32 = 1.5;
/// Gap between the windowed list block and the edit-panel header.
const FIELDS_GAP: f32 = 0.6;
/// Offset of the first field row below the edit-panel header.
const FIELD_FIRST: f32 = 1.3;
/// Vertical pitch of the field rows.
const FIELD_PITCH: f32 = 1.1;
/// Gap between the last field and the note line.
const NOTE_GAP: f32 = 0.4;
/// Gap between the note line and the button row.
const BUTTONS_GAP: f32 = 1.5;
/// Button width/height and the gap between Add and Delete, in cells.
const BTN_W_CELLS: f32 = 24.0;
const BTN_H_CELLS: f32 = 1.4;
const BTN_GAP_CELLS: f32 = 2.0;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;
/// Button-row offset below the content top while the list is empty.
const EMPTY_BUTTONS_TOP: f32 = 4.0;

/// The bounded window over the host list for the current selection. Shared
/// with the renderer, which draws the range-indicator row from it.
pub(in crate::renderer) fn ssh_list_window(sp: &SettingsPanel) -> ListWindow {
    list_window(sp.ssh_hosts.len(), sp.selected_host_index, MAX_LIST_ROWS)
}

/// Y position of the edit-panel header: right below the windowed host list.
/// Shared with the renderer's remaining prose so the two cannot drift apart.
pub(crate) fn ssh_fields_top(sp: &SettingsPanel, content_top: f32, cell_h: f32) -> f32 {
    content_top + cell_h * (LIST_TOP + ssh_list_window(sp).block_rows() + FIELDS_GAP)
}

/// Y position of the edit-hint note line below the five fields.
pub(crate) fn ssh_note_y(sp: &SettingsPanel, content_top: f32, cell_h: f32) -> f32 {
    ssh_fields_top(sp, content_top, cell_h) + cell_h * (FIELD_FIRST + 5.0 * FIELD_PITCH + NOTE_GAP)
}

/// Y position of the Add/Delete button row.
fn buttons_y(sp: &SettingsPanel, content_top: f32, cell_h: f32) -> f32 {
    if sp.ssh_hosts.is_empty() {
        content_top + cell_h * EMPTY_BUTTONS_TOP
    } else {
        ssh_note_y(sp, content_top, cell_h) + cell_h * BUTTONS_GAP
    }
}

/// Describe every control of the Ssh tab, without laying it out.
///
/// The Add/Delete buttons are always present (a reader can reach Add on an
/// empty list; Delete reports disabled). The list and the five fields exist
/// only while there is a host to show — the empty state is prose in the
/// renderer.
pub(crate) fn ssh_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    let empty = sp.ssh_hosts.is_empty();
    let mut descs = Vec::with_capacity(sp.ssh_hosts.len() + 7);
    if !empty {
        let sel = sp.selected_host_index.min(sp.ssh_hosts.len() - 1);
        for (i, host) in sp.ssh_hosts.iter().enumerate() {
            let mut item = WidgetDesc::new(
                WidgetId::new(SSH_CATEGORY, row::LIST_BASE + i as u16),
                WidgetKind::ListItem { selected: sel == i },
                host.label(),
            )
            .focused(sp.focused_widget_index == 0 && sel == i);
            // A reader hears the auth method as the entry's description,
            // matching the retired hand-written nodes.
            if !host.auth_type.is_empty() {
                item = item.tooltip(format!("Auth: {}", host.auth_type));
            }
            descs.push(item);
        }

        // While a GUI edit is in flight on the focused field, expose the
        // buffer (and its caret) instead of the stored value, so the reader
        // and the renderer both track keystrokes live.
        let host = &sp.ssh_hosts[sel];
        let text_kind = |field: u16, stored: &str| -> WidgetKind {
            match sp.ssh_field_editing.as_ref() {
                Some(state) if sp.focused_widget_index == field => WidgetKind::Text {
                    value: state.display_string(),
                    editing: true,
                    caret: Some(state.display_cursor()),
                },
                _ => WidgetKind::Text {
                    value: stored.to_string(),
                    editing: false,
                    caret: None,
                },
            }
        };
        let field = |index: u16, kind: WidgetKind, label: String| {
            WidgetDesc::new(WidgetId::new(SSH_CATEGORY, index), kind, label)
                .focused(sp.focused_widget_index == index)
        };
        descs.push(field(
            row::FIELD_NAME,
            text_kind(1, &host.name),
            nexterm_i18n::fl!("settings-ssh-field-name"),
        ));
        descs.push(field(
            row::FIELD_HOST,
            text_kind(2, &host.host),
            nexterm_i18n::fl!("settings-ssh-field-host"),
        ));
        descs.push(field(
            row::FIELD_PORT,
            WidgetKind::SpinButton {
                value: host.port as f32,
                min: 1.0,
                max: 65535.0,
                step: 1.0,
                display: host.port.to_string(),
            },
            nexterm_i18n::fl!("settings-ssh-field-port"),
        ));
        descs.push(field(
            row::FIELD_USERNAME,
            text_kind(4, &host.username),
            nexterm_i18n::fl!("settings-ssh-field-username"),
        ));
        descs.push(field(
            row::FIELD_AUTH,
            WidgetKind::Cycle {
                value: host.auth_type.clone(),
            },
            nexterm_i18n::fl!("settings-ssh-field-auth-type"),
        ));
    }
    descs.push(
        WidgetDesc::new(
            WidgetId::new(SSH_CATEGORY, row::ADD),
            WidgetKind::Button { destructive: false },
            nexterm_i18n::fl!("settings-ssh-add"),
        )
        .focused(sp.focused_widget_index == 6),
    );
    let delete_label = if empty {
        nexterm_i18n::fl!("settings-ssh-delete-disabled")
    } else {
        nexterm_i18n::fl!("settings-ssh-delete")
    };
    let mut delete = WidgetDesc::new(
        WidgetId::new(SSH_CATEGORY, row::DELETE),
        WidgetKind::Button { destructive: true },
        delete_label,
    )
    .focused(!empty && sp.focused_widget_index == 7);
    delete.enabled = !empty;
    descs.push(delete);
    descs
}

/// Lay the Ssh tab out for this frame.
///
/// List entries outside the bounded window collapse to a zero rect — absent
/// for `hit_test` and `draw_widget`, still described for the AccessKit tree.
pub(crate) fn build_ssh_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    let w = ssh_list_window(sp);
    let fields_top = ssh_fields_top(sp, g.content_top, g.cell_h);
    let btn_y = buttons_y(sp, g.content_top, g.cell_h);

    let x = g.content_inner_x - g.cell_w * ROW_BLEED;
    let row_w = (g.content_w - g.cell_w * (ROW_BLEED + 0.4)).max(0.0);

    ssh_widget_descs(sp)
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
            } else {
                let fi = (index - row::FIELD_NAME) as f32;
                let y = fields_top + g.cell_h * (FIELD_FIRST + fi * FIELD_PITCH);
                let rect = WidgetRect::new(x, y - g.cell_h * 0.1, row_w, g.cell_h);
                let control = WidgetRect::new(
                    g.content_inner_x + layout.control_x_off,
                    rect.y,
                    layout.control_w,
                    rect.h,
                );
                (rect, control)
            };
            desc.place(rect, control)
        })
        .collect()
}

/// Apply an action to the Ssh widget at `index`.
///
/// One router for the mouse, the keyboard-independent AccessKit path, and any
/// future caller: activating a list entry selects it (and hands the focus
/// counter back to the list), text fields take SetText directly or open the
/// GUI edit buffer on click, the port steps or accepts a direct value, the
/// auth cycler cycles, Add appends + starts editing the name, and Delete
/// opens the confirmation dialog.
pub(crate) fn apply_ssh_action(sp: &mut SettingsPanel, index: u16, action: WidgetAction) -> bool {
    if index >= row::LIST_BASE {
        let i = (index - row::LIST_BASE) as usize;
        if i >= sp.ssh_hosts.len() || !matches!(action, WidgetAction::Activate) {
            return false;
        }
        sp.selected_host_index = i;
        sp.focused_widget_index = 0;
        return true;
    }
    // Everything below Add operates on the selected host.
    if sp.ssh_hosts.is_empty() && index != row::ADD {
        return false;
    }
    match index {
        row::FIELD_NAME | row::FIELD_HOST | row::FIELD_USERNAME => match action {
            WidgetAction::SetText(text) => {
                match index {
                    row::FIELD_NAME => sp.set_ssh_host_name(text),
                    row::FIELD_HOST => sp.set_ssh_host_host(text),
                    _ => sp.set_ssh_host_username(text),
                }
                sp.focused_widget_index = index;
                true
            }
            WidgetAction::Activate => {
                // A click focuses the field and opens the edit buffer,
                // matching the Security byte-cap fields.
                sp.focused_widget_index = index;
                sp.begin_ssh_field_edit()
            }
            _ => false,
        },
        row::FIELD_PORT => {
            match action {
                WidgetAction::Next => sp.increase_ssh_host_port(),
                WidgetAction::Prev => sp.decrease_ssh_host_port(),
                WidgetAction::SetValue(v) => sp.set_ssh_host_port_value(v),
                // A click only focuses: the port has no edit mode.
                WidgetAction::Activate => {}
                WidgetAction::SetText(_) => return false,
            }
            sp.focused_widget_index = 3;
            true
        }
        row::FIELD_AUTH => {
            match action {
                WidgetAction::Activate | WidgetAction::Next => sp.next_ssh_auth_type(),
                WidgetAction::Prev => sp.prev_ssh_auth_type(),
                _ => return false,
            }
            sp.focused_widget_index = 5;
            true
        }
        row::ADD if matches!(action, WidgetAction::Activate) => {
            sp.add_ssh_host();
            true
        }
        row::DELETE if matches!(action, WidgetAction::Activate) => {
            sp.open_ssh_delete_dialog();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_panel::{SettingsPanel, SshHostEntry};

    fn geometry() -> TabGeometry {
        TabGeometry {
            content_top: 100.0,
            content_inner_x: 200.0,
            content_w: 600.0,
            cell_w: 10.0,
            cell_h: 20.0,
        }
    }

    fn panel_with_hosts(n: usize) -> SettingsPanel {
        let mut sp = SettingsPanel::default();
        for i in 0..n {
            sp.ssh_hosts.push(SshHostEntry {
                name: format!("h{i}"),
                host: format!("host{i}.example"),
                port: 22,
                username: "u".to_string(),
                auth_type: "password".to_string(),
            });
        }
        sp
    }

    #[test]
    fn an_empty_list_exposes_only_the_buttons() {
        let descs = ssh_widget_descs(&SettingsPanel::default());
        assert_eq!(descs.len(), 2);
        assert_eq!(descs[0].id.index, row::ADD);
        assert!(descs[0].enabled);
        assert_eq!(descs[1].id.index, row::DELETE);
        assert!(!descs[1].enabled, "nothing to delete");
    }

    #[test]
    fn describes_list_fields_and_buttons_in_reading_order() {
        let mut sp = panel_with_hosts(3);
        sp.selected_host_index = 1;
        let descs = ssh_widget_descs(&sp);
        assert_eq!(descs.len(), 3 + 5 + 2);
        assert_eq!(descs[0].id.index, row::LIST_BASE);
        assert!(matches!(descs[2].kind, WidgetKind::ListItem { .. }));
        assert_eq!(descs[3].id.index, row::FIELD_NAME);
        assert_eq!(descs[7].id.index, row::FIELD_AUTH);
        assert_eq!(descs[8].id.index, row::ADD);
        assert_eq!(descs[9].id.index, row::DELETE);
        assert!(descs[9].enabled);
    }

    #[test]
    fn the_selected_entry_is_focused_while_the_list_owns_the_counter() {
        let mut sp = panel_with_hosts(3);
        sp.selected_host_index = 2;
        sp.focused_widget_index = 0;
        let descs = ssh_widget_descs(&sp);
        assert_eq!(descs[2].kind, WidgetKind::ListItem { selected: true });
        assert!(descs[2].focused);
        assert!(!descs[0].focused);
    }

    #[test]
    fn the_field_focus_counter_maps_onto_widget_indices_as_identity() {
        let mut sp = panel_with_hosts(1);
        for focus in 1u16..=7 {
            sp.focused_widget_index = focus;
            let descs = ssh_widget_descs(&sp);
            let focused: Vec<u16> = descs
                .iter()
                .filter(|d| d.focused)
                .map(|d| d.id.index)
                .collect();
            assert_eq!(focused, vec![focus], "focus={focus}");
        }
    }

    #[test]
    fn the_port_field_is_a_spin_button_with_the_full_port_range() {
        let mut sp = panel_with_hosts(1);
        sp.ssh_hosts[0].port = 2222;
        let descs = ssh_widget_descs(&sp);
        let port = descs
            .iter()
            .find(|d| d.id.index == row::FIELD_PORT)
            .unwrap();
        assert_eq!(
            port.kind,
            WidgetKind::SpinButton {
                value: 2222.0,
                min: 1.0,
                max: 65535.0,
                step: 1.0,
                display: "2222".to_string(),
            }
        );
    }

    #[test]
    fn an_in_flight_edit_shows_the_buffer_with_a_caret() {
        let mut sp = panel_with_hosts(1);
        sp.focused_widget_index = 1;
        assert!(sp.begin_ssh_field_edit());
        sp.ssh_field_insert_char('x');
        let descs = ssh_widget_descs(&sp);
        let name = descs
            .iter()
            .find(|d| d.id.index == row::FIELD_NAME)
            .unwrap();
        let WidgetKind::Text {
            value,
            editing,
            caret,
        } = &name.kind
        else {
            panic!("name must be a text field");
        };
        assert!(editing);
        assert!(value.ends_with('x'));
        assert!(caret.is_some());
    }

    #[test]
    fn only_the_windowed_slice_of_a_long_list_gets_a_rect() {
        let mut sp = panel_with_hosts(20);
        sp.selected_host_index = 0;
        let specs = build_ssh_widgets(&sp, &geometry());
        let rect_of = |index: u16| {
            specs
                .iter()
                .find(|s| s.id().index == index)
                .unwrap_or_else(|| panic!("widget {index} missing"))
                .rect
        };
        assert!(rect_of(row::LIST_BASE).h > 0.0, "windowed row drawn");
        assert_eq!(
            rect_of(row::LIST_BASE + 10).h,
            0.0,
            "row outside the window is collapsed"
        );
        // The edit panel sits right below the windowed list, not the full one.
        assert!(rect_of(row::FIELD_NAME).h > 0.0);
    }

    #[test]
    fn the_buttons_share_a_row_below_the_edit_panel() {
        let sp = panel_with_hosts(2);
        let specs = build_ssh_widgets(&sp, &geometry());
        let of = |index: u16| specs.iter().find(|s| s.id().index == index).unwrap();
        let auth = of(row::FIELD_AUTH);
        let add = of(row::ADD);
        let delete = of(row::DELETE);
        assert!(add.rect.y > auth.rect.y, "buttons sit below the fields");
        assert_eq!(add.rect.y, delete.rect.y, "Add and Delete share a row");
        assert!(delete.rect.x > add.rect.x + add.rect.w, "side by side");
    }

    #[test]
    fn activating_a_list_entry_selects_it_and_returns_focus_to_the_list() {
        let mut sp = panel_with_hosts(3);
        sp.focused_widget_index = 4;
        assert!(apply_ssh_action(
            &mut sp,
            row::LIST_BASE + 2,
            WidgetAction::Activate
        ));
        assert_eq!(sp.selected_host_index, 2);
        assert_eq!(sp.focused_widget_index, 0);
    }

    #[test]
    fn text_fields_accept_set_text_and_a_click_starts_editing() {
        let mut sp = panel_with_hosts(1);
        assert!(apply_ssh_action(
            &mut sp,
            row::FIELD_NAME,
            WidgetAction::SetText("new".to_string())
        ));
        assert_eq!(sp.ssh_hosts[0].name, "new");
        assert_eq!(sp.focused_widget_index, 1);
        assert!(sp.ssh_field_editing.is_none(), "SetText is a direct write");

        assert!(apply_ssh_action(
            &mut sp,
            row::FIELD_HOST,
            WidgetAction::Activate
        ));
        assert_eq!(sp.focused_widget_index, 2);
        assert!(sp.ssh_field_editing.is_some(), "a click starts GUI editing");
    }

    #[test]
    fn the_port_spin_button_steps_and_accepts_a_direct_value() {
        let mut sp = panel_with_hosts(1);
        assert!(apply_ssh_action(
            &mut sp,
            row::FIELD_PORT,
            WidgetAction::Next
        ));
        assert_eq!(sp.ssh_hosts[0].port, 23);
        assert!(apply_ssh_action(
            &mut sp,
            row::FIELD_PORT,
            WidgetAction::Prev
        ));
        assert_eq!(sp.ssh_hosts[0].port, 22);
        assert!(apply_ssh_action(
            &mut sp,
            row::FIELD_PORT,
            WidgetAction::SetValue(8022.0)
        ));
        assert_eq!(sp.ssh_hosts[0].port, 8022);
        assert_eq!(sp.focused_widget_index, 3);
    }

    #[test]
    fn the_auth_cycler_steps_in_both_directions() {
        let mut sp = panel_with_hosts(1);
        assert!(apply_ssh_action(
            &mut sp,
            row::FIELD_AUTH,
            WidgetAction::Activate
        ));
        assert_ne!(sp.ssh_hosts[0].auth_type, "password");
        assert!(apply_ssh_action(
            &mut sp,
            row::FIELD_AUTH,
            WidgetAction::Prev
        ));
        assert_eq!(sp.ssh_hosts[0].auth_type, "password");
        assert_eq!(sp.focused_widget_index, 5);
    }

    #[test]
    fn add_appends_a_host_and_starts_editing_its_name() {
        let mut sp = SettingsPanel::default();
        assert!(apply_ssh_action(&mut sp, row::ADD, WidgetAction::Activate));
        assert_eq!(sp.ssh_hosts.len(), 1);
        assert_eq!(sp.focused_widget_index, 1);
        assert!(sp.ssh_field_editing.is_some());
    }

    #[test]
    fn delete_opens_the_dialog_only_when_something_exists() {
        let mut sp = SettingsPanel::default();
        assert!(!apply_ssh_action(
            &mut sp,
            row::DELETE,
            WidgetAction::Activate
        ));
        assert!(!sp.ssh_delete_dialog_open);

        let mut sp = panel_with_hosts(1);
        assert!(apply_ssh_action(
            &mut sp,
            row::DELETE,
            WidgetAction::Activate
        ));
        assert!(sp.ssh_delete_dialog_open);
    }

    #[test]
    fn out_of_range_and_mismatched_actions_are_refused() {
        let mut sp = panel_with_hosts(1);
        assert!(!apply_ssh_action(
            &mut sp,
            row::LIST_BASE + 9,
            WidgetAction::Activate
        ));
        assert!(!apply_ssh_action(
            &mut sp,
            row::FIELD_NAME,
            WidgetAction::SetValue(1.0)
        ));
        assert!(!apply_ssh_action(
            &mut sp,
            row::FIELD_PORT,
            WidgetAction::SetText("x".to_string())
        ));
        // Index 0 is deliberately unassigned: the focus counter's 0 means
        // "the list", whose entries are addressed via LIST_BASE instead.
        assert!(!apply_ssh_action(&mut sp, 0, WidgetAction::Activate));
    }
}
