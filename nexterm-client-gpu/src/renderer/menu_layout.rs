//! Where a context menu's rows are, and how wide it is (UI/UX v3 N-4a).
//!
//! Five places needed one number and had three transcriptions of it plus two
//! constants that were not it. The builder sized the panel from the widest
//! label and hint; both placement sites re-derived that formula to clamp the
//! menu against the window edge; and both hit-tests — hover and click — used a
//! flat `18.0 * cell_w` under a comment asserting they matched the drawn width.
//!
//! They did not. Every menu the app builds is wider than 18 cells (27 to 40 at
//! `en`, more at `ja`), so the overhang was dead to the mouse: clicking a row on
//! its hint, or anywhere right of its label, dismissed the menu without acting.
//! On the block menu that was the right 55 % of every row.
//!
//! This module is where the geometry comes from now. N-4a moves all five sites
//! onto it while [`menu_width`] still counts display cells, so the fix is
//! reviewable as a fix: nothing about the rendered menu changes, and the dead
//! zone disappears. N-4b swaps the body of `menu_width` for `measure_run` and
//! leaves the callers alone.

use crate::state::{ContextMenu, ContextMenuAction, ContextMenuItem};
use crate::vertex_util::visual_width;

/// Horizontal padding around a row's text, in cells.
///
/// The builder documented this as "left padding (0.9) + gap (2) + right
/// padding (1.5)" and then added **5**, not the 4.4 those three sum to. The
/// extra 0.6 of a cell has been in every menu the app has ever drawn, and N-4a
/// is the phase that changes no pixels, so it is preserved exactly and named
/// rather than quietly corrected. Whether the padding should be 4.4 is a
/// design question for whoever wants to ask it.
const PAD_CELLS: usize = 5;

/// Narrowest menu worth drawing, in cells.
///
/// Unreachable in practice — the narrowest menu `state/menus.rs` builds is 27
/// cells — but kept because it costs nothing and a future menu of two short
/// items would want it. Its unreachability is why the hit-test's flat 18 could
/// only ever be *too narrow*, never too wide, and so why the defect this module
/// fixes was one-directional.
const MIN_CELLS: f32 = 16.0;

/// Width of the menu panel in pixels.
///
/// Takes items rather than a `ContextMenu` because both placement sites need
/// the width *before* there is a menu to place: they build a throwaway at the
/// origin, measure it, then rebuild it at the clamped position.
pub(crate) fn menu_width(items: &[ContextMenuItem], cell_w: f32) -> f32 {
    let max_label = items
        .iter()
        .map(|i| visual_width(&i.label))
        .max()
        .unwrap_or(8);
    let max_hint = items
        .iter()
        .map(|i| visual_width(&i.hint))
        .max()
        .unwrap_or(0);
    ((max_label + max_hint + PAD_CELLS) as f32).max(MIN_CELLS) * cell_w
}

/// Height of the menu panel in pixels.
///
/// Every row is one cell tall, separators included — which is why a separator
/// can be pointed at, and why [`item_at`] has to refuse it explicitly rather
/// than relying on it occupying no space.
pub(crate) fn menu_height(items: &[ContextMenuItem], cell_h: f32) -> f32 {
    items.len() as f32 * cell_h
}

/// Top edge of row `i`.
pub(crate) fn row_y(menu_y: f32, i: usize, cell_h: f32) -> f32 {
    menu_y + i as f32 * cell_h
}

/// The row at `(x, y)`, or `None` outside the menu and on separators.
///
/// Both hit-tests call this, so the region that responds to the mouse is the
/// region [`menu_width`] draws, by construction rather than by a comment asking
/// the next maintainer to keep two numbers in step.
///
/// Separators return `None` at every x. They are drawn as a line in a full-height
/// row, `input_handler/action.rs` makes their action a documented no-op, and the
/// renderer skips their hover fill — so pointing at one has never done anything.
/// Saying so here means the hover path stops recording a hovered index the
/// renderer will ignore.
pub(crate) fn item_at(
    menu: &ContextMenu,
    x: f32,
    y: f32,
    cell_w: f32,
    cell_h: f32,
) -> Option<usize> {
    if cell_h <= 0.0 || y < menu.y {
        return None;
    }
    let w = menu_width(&menu.items, cell_w);
    if x < menu.x || x > menu.x + w {
        return None;
    }
    let i = ((y - menu.y) / cell_h) as usize;
    let item = menu.items.get(i)?;
    if matches!(item.action, ContextMenuAction::Separator) {
        return None;
    }
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_W: f32 = 8.0;
    const CELL_H: f32 = 16.0;

    fn item(label: &str, hint: &str) -> ContextMenuItem {
        ContextMenuItem {
            label: label.to_string(),
            hint: hint.to_string(),
            action: ContextMenuAction::Copy,
        }
    }

    fn separator() -> ContextMenuItem {
        ContextMenuItem {
            label: String::new(),
            hint: String::new(),
            action: ContextMenuAction::Separator,
        }
    }

    fn menu(x: f32, y: f32, items: Vec<ContextMenuItem>) -> ContextMenu {
        ContextMenu {
            x,
            y,
            items,
            hovered: None,
            hover_transition: Default::default(),
            press_pulse: Default::default(),
        }
    }

    /// The width is the widest label plus the widest hint plus the padding —
    /// the two maxima are independent, so the widest row need not be one item.
    #[test]
    fn the_width_is_the_widest_label_plus_the_widest_hint() {
        let items = vec![item("Copy", "Ctrl+Shift+L"), item("Set block name", "")];
        // 14 (label) + 12 (hint) + 5 = 31 cells
        assert_eq!(menu_width(&items, CELL_W), 31.0 * CELL_W);
    }

    /// Preserved deliberately: see `PAD_CELLS`. N-4a changes no pixels, so this
    /// pins the 5 rather than the 4.4 the padding comment implies.
    #[test]
    fn the_padding_is_five_cells_not_the_four_point_four_it_documents() {
        let items = vec![item("aaaaaaaaaaaaaaaaaaaa", "")]; // 20 cells
        assert_eq!(menu_width(&items, CELL_W), 25.0 * CELL_W);
    }

    #[test]
    fn a_narrow_menu_is_widened_to_the_floor() {
        assert_eq!(menu_width(&[item("Ok", "")], CELL_W), MIN_CELLS * CELL_W);
        assert_eq!(menu_width(&[], CELL_W), MIN_CELLS * CELL_W);
    }

    /// A full-width label costs two cells, as `add_string_verts` advances. The
    /// old tab bar counted `chars()` here and overflowed on CJK (N-3 §1.2);
    /// this path has always counted display width and keeps doing so until
    /// N-4b measures it instead.
    #[test]
    fn a_full_width_label_costs_two_cells_per_character() {
        let latin = menu_width(&[item("abcdefghijklmnopqrst", "")], CELL_W);
        let cjk = menu_width(&[item("あいうえおかきくけこ", "")], CELL_W);
        assert_eq!(latin, cjk, "ten CJK characters occupy twenty cells");
    }

    /// G-menu-agree, the invariant `mouse.rs` claimed in a comment and broke on
    /// the line below it: every x the panel is drawn across responds, and no x
    /// outside it does.
    #[test]
    fn the_region_that_responds_is_the_region_that_is_drawn() {
        for items in [
            vec![item("Copy", "Ctrl+C")],
            vec![item("Collapse / expand block", "Ctrl+Shift+L")],
            vec![item("このウィンドウだけ閉じる", "")],
            vec![item("Ok", "")], // hits the floor
        ] {
            let m = menu(100.0, 50.0, items);
            let w = menu_width(&m.items, CELL_W);
            let mid_y = 50.0 + CELL_H * 0.5;

            assert_eq!(item_at(&m, 100.0, mid_y, CELL_W, CELL_H), Some(0));
            assert_eq!(item_at(&m, 100.0 + w, mid_y, CELL_W, CELL_H), Some(0));
            assert_eq!(item_at(&m, 100.0 + w + 0.5, mid_y, CELL_W, CELL_H), None);
            assert_eq!(item_at(&m, 99.5, mid_y, CELL_W, CELL_H), None);
        }
    }

    /// The 18-cell constant in miniature: a menu wider than 18 cells used to
    /// answer `None` past that column while drawing rows all the way across.
    #[test]
    fn a_wide_menu_responds_past_the_eighteenth_cell() {
        let m = menu(
            0.0,
            0.0,
            vec![item("Collapse / expand block", "Ctrl+Shift+L")],
        );
        assert!(menu_width(&m.items, CELL_W) > 18.0 * CELL_W);
        assert_eq!(
            item_at(&m, 30.0 * CELL_W, CELL_H * 0.5, CELL_W, CELL_H),
            Some(0),
            "the hint column has to be clickable; it is where the dead zone was"
        );
    }

    #[test]
    fn rows_are_addressed_top_to_bottom_and_end_with_the_last_item() {
        let m = menu(
            0.0,
            100.0,
            vec![item("a", ""), item("b", ""), item("c", "")],
        );
        assert_eq!(item_at(&m, 0.0, 100.0, CELL_W, CELL_H), Some(0));
        assert_eq!(item_at(&m, 0.0, 100.0 + CELL_H, CELL_W, CELL_H), Some(1));
        assert_eq!(
            item_at(&m, 0.0, 100.0 + CELL_H * 2.9, CELL_W, CELL_H),
            Some(2)
        );
        assert_eq!(item_at(&m, 0.0, 100.0 + CELL_H * 3.0, CELL_W, CELL_H), None);
    }

    /// A click above the menu must not wrap onto its last row — the failure a
    /// `(y - menu.y) / cell_h` cast produces if the negative is not refused
    /// before it becomes a `usize`.
    #[test]
    fn a_point_above_the_menu_does_not_wrap_onto_a_row() {
        let m = menu(0.0, 100.0, vec![item("a", ""), item("b", "")]);
        assert_eq!(item_at(&m, 0.0, 99.0, CELL_W, CELL_H), None);
        assert_eq!(item_at(&m, 0.0, 0.0, CELL_W, CELL_H), None);
        assert_eq!(item_at(&m, 0.0, -50.0, CELL_W, CELL_H), None);
    }

    #[test]
    fn a_separator_is_not_a_row_the_mouse_can_have() {
        let m = menu(0.0, 0.0, vec![item("a", ""), separator(), item("b", "")]);
        assert_eq!(item_at(&m, 0.0, CELL_H * 0.5, CELL_W, CELL_H), Some(0));
        assert_eq!(item_at(&m, 0.0, CELL_H * 1.5, CELL_W, CELL_H), None);
        assert_eq!(item_at(&m, 0.0, CELL_H * 2.5, CELL_W, CELL_H), Some(2));
    }

    #[test]
    fn a_degenerate_cell_height_yields_no_row() {
        let m = menu(0.0, 0.0, vec![item("a", "")]);
        assert_eq!(item_at(&m, 0.0, 0.0, CELL_W, 0.0), None);
        assert_eq!(item_at(&m, 0.0, 0.0, CELL_W, -16.0), None);
    }

    #[test]
    fn the_height_is_one_cell_per_row_including_separators() {
        let items = vec![item("a", ""), separator(), item("b", "")];
        assert_eq!(menu_height(&items, CELL_H), 3.0 * CELL_H);
        assert_eq!(row_y(100.0, 2, CELL_H), 100.0 + 2.0 * CELL_H);
    }

    /// G-menu-width: the geometry lives here and the five sites read it. The
    /// formula must not be transcribed again, and the constant that broke the
    /// hit-test must not come back.
    #[test]
    fn no_other_file_computes_a_menu_width() {
        for (name, src) in [
            ("mouse.rs", include_str!("event_handler/mouse.rs")),
            ("dialog.rs", include_str!("overlay/dialog.rs")),
        ] {
            assert!(
                !src.contains("18.0 * cell_w"),
                "{name} hit-tests the menu against a hard-coded width again"
            );
            assert!(
                !src.contains("max_label + max_hint + 5"),
                "{name} transcribes the menu width formula again; menu_layout \
                 owns it"
            );
        }
    }

    /// Both hit-tests go through `item_at` rather than walking the rows
    /// themselves — the walk is where the width check lived, so a
    /// reintroduced loop would be a reintroduced second opinion about the
    /// menu's edges.
    #[test]
    fn both_mouse_paths_ask_this_module_which_row_was_hit() {
        let src = include_str!("event_handler/mouse.rs");
        assert_eq!(
            src.matches("menu_layout::item_at(").count(),
            2,
            "the hover path and the click path each call item_at exactly once"
        );
    }
}
