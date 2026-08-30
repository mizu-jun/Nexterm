//! Where a context menu's rows are, and how wide it is (UI/UX v3 N-4a, N-4b).
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
//! This module is where the geometry comes from now. N-4a moved all five sites
//! onto it while [`menu_width`] still counted display cells, so the fix was
//! reviewable as a fix: nothing about the rendered menu changed, and the dead
//! zone disappeared.
//!
//! N-4b then swapped counting for measuring. A label is measured at `body` and
//! a hint at `caption` — the steps they are *drawn* at — so the panel is sized
//! by the same numbers `add_run_verts` will advance by. This is what makes the
//! menu correct for a proportional fallback face and for CJK, rather than
//! merely self-consistent: `visual_width` answered 2 for every full-width
//! character regardless of what the font actually did with it.
//!
//! Padding stays in cells (see [`PAD_CELLS`]). Only the text contribution is
//! measured.

use crate::font::FontManager;
use crate::state::{ContextMenu, ContextMenuAction, ContextMenuItem};
use crate::vertex_util::measure_run;

/// Horizontal padding around a row's text, in cells.
///
/// The builder documented this as "left padding (0.9) + gap (2) + right
/// padding (1.5)" and then added **5**, not the 4.4 those three sum to. The
/// extra 0.6 of a cell has been in every menu the app has ever drawn, and N-4a
/// is the phase that changes no pixels, so it is preserved exactly and named
/// rather than quietly corrected. Whether the padding should be 4.4 is a
/// design question for whoever wants to ask it.
///
/// Still in *cells* after N-4b: the text is measured, the spacing around it is
/// not. Padding that tracked the ramp would change the menu's proportions,
/// which is a design change rather than the measurement fix N-4 is.
const PAD_CELLS: f32 = 5.0;

/// Narrowest menu worth drawing, in cells.
///
/// Unreachable in practice — the narrowest menu `state/menus.rs` builds is 27
/// cells — but kept because it costs nothing and a future menu of two short
/// items would want it. Its unreachability is why the hit-test's flat 18 could
/// only ever be *too narrow*, never too wide, and so why the defect this module
/// fixes was one-directional.
const MIN_CELLS: f32 = 16.0;

/// The ramp step a row's label is drawn at (UI/UX v3 N-4b, spec §2 D6).
pub(crate) fn label_style() -> nexterm_config::TypeStyle {
    nexterm_config::MetricTokens::default().type_ramp.body
}

/// The ramp step a row's key hint is drawn at.
///
/// `caption` is specified as "key hints, secondary metadata" in `metrics.rs`,
/// which is exactly what this column is.
pub(crate) fn hint_style() -> nexterm_config::TypeStyle {
    nexterm_config::MetricTokens::default().type_ramp.caption
}

/// Width of the menu panel in pixels.
///
/// Takes items rather than a `ContextMenu` because both placement sites need
/// the width *before* there is a menu to place: they build a throwaway at the
/// origin, measure it, then rebuild it at the clamped position.
///
/// Labels and hints are measured at their **own** ramp steps because they are
/// drawn at them. Measuring both as `body` would place the hint column off by
/// the difference between the two sizes — the same class of mistake as
/// measuring in cells and drawing in pixels, one level finer.
pub(crate) fn menu_width(items: &[ContextMenuItem], cell_w: f32, font: &mut FontManager) -> f32 {
    let label_style = label_style();
    let hint_style = hint_style();
    let max_label = items
        .iter()
        .map(|i| measure_run(&i.label, &label_style, font))
        .fold(0.0_f32, f32::max);
    let max_hint = items
        .iter()
        .map(|i| measure_run(&i.hint, &hint_style, font))
        .fold(0.0_f32, f32::max);
    (max_label + max_hint + PAD_CELLS * cell_w).max(MIN_CELLS * cell_w)
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
    font: &mut FontManager,
) -> Option<usize> {
    if cell_h <= 0.0 || y < menu.y {
        return None;
    }
    let w = menu_width(&menu.items, cell_w, font);
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

    /// CI's font stack answers the same advance for every character (N-3 spec
    /// §6 — this devcontainer resolves `fc-list :lang=ja` to zero faces), so
    /// nothing below may assert that CJK measures wider than Latin. The tests
    /// assert font-independent properties instead.
    fn font() -> FontManager {
        FontManager::new("monospace", 14.0, &[], 1.0, true)
    }

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

    /// The two maxima are independent: the widest label and the widest hint
    /// need not belong to the same row, and the panel has to hold both.
    #[test]
    fn the_width_is_the_widest_label_plus_the_widest_hint() {
        let mut f = font();
        let label = measure_run("Set block name", &label_style(), &mut f);
        let hint = measure_run("Ctrl+Shift+L", &hint_style(), &mut f);

        let items = vec![item("Copy", "Ctrl+Shift+L"), item("Set block name", "")];
        assert!(
            (menu_width(&items, CELL_W, &mut f) - (label + hint + PAD_CELLS * CELL_W)).abs() < 1e-3
        );
    }

    /// N-4b measures the *text*; the spacing around it stays in cells, so the
    /// menu's proportions do not shift with the ramp.
    #[test]
    fn the_padding_is_five_cells_and_scales_with_the_cell() {
        let mut f = font();
        // Long enough that neither call lands on the floor — two floored
        // widths differ by the floor, not by the padding, which is what a
        // short label measured here instead.
        let items = vec![item("Copy this block to the clipboard", "")];
        let narrow = menu_width(&items, 4.0, &mut f);
        let wide = menu_width(&items, 8.0, &mut f);
        assert!(
            narrow > MIN_CELLS * 4.0 && wide > MIN_CELLS * 8.0,
            "the label must clear the floor for this to measure padding: \
             {narrow} / {wide}"
        );
        assert!(
            (wide - narrow - PAD_CELLS * 4.0).abs() < 1e-3,
            "doubling the cell adds exactly five cells of padding: {narrow} → {wide}"
        );
    }

    /// A hint is drawn at `caption` and a label at `body`, so they must be
    /// measured at those steps. Sizing the hint column as `body` would place
    /// it off by the difference between the two.
    #[test]
    fn a_hint_is_measured_at_its_own_ramp_step() {
        let mut f = font();
        let text = "Ctrl+Shift+L";
        let as_body = measure_run(text, &label_style(), &mut f);
        let as_caption = measure_run(text, &hint_style(), &mut f);
        assert!(
            as_caption < as_body,
            "caption (12px) must measure narrower than body (14px): {as_caption} vs {as_body}"
        );

        let items = vec![item("", text)];
        let w = menu_width(&items, CELL_W, &mut f);
        assert!(
            (w - (as_caption + PAD_CELLS * CELL_W)).abs() < 1e-3 || w == MIN_CELLS * CELL_W,
            "the hint contributes its caption width, not its body width"
        );
    }

    /// G-width in the shape N-3 established: the width a menu is *sized* by
    /// comes from the same `measure_run` the drawing pass uses, so the two
    /// cannot disagree whatever the font reports for any glyph. Phrased as an
    /// equality between paths, never as a claim about CJK metrics.
    #[test]
    fn the_panel_contains_the_runs_that_will_be_drawn_in_it() {
        let mut f = font();
        for (label, hint) in [
            ("Copy", "Ctrl+C"),
            ("Collapse / expand block", "Ctrl+Shift+L"),
            ("このウィンドウだけ閉じる", ""),
            ("Split Horizontal", "Ctrl+B  \""),
        ] {
            let items = vec![item(label, hint)];
            let w = menu_width(&items, CELL_W, &mut f);
            let drawn = measure_run(label, &label_style(), &mut f)
                + measure_run(hint, &hint_style(), &mut f);
            assert!(
                w >= drawn,
                "{label:?} + {hint:?} draw {drawn} wide in a {w}-wide panel"
            );
        }
    }

    /// No glyph measures zero, so a longer label is never a narrower menu —
    /// the monotonicity `visual_width` gave for free and a measured path has
    /// to be checked for.
    #[test]
    fn a_longer_label_never_narrows_the_menu() {
        let mut f = font();
        let short = menu_width(&[item("Copy", "")], CELL_W, &mut f);
        let long = menu_width(
            &[item("Copy this block to the clipboard", "")],
            CELL_W,
            &mut f,
        );
        assert!(long > short, "{short} → {long}");
    }

    #[test]
    fn a_narrow_menu_is_widened_to_the_floor() {
        let mut f = font();
        assert_eq!(
            menu_width(&[item("Ok", "")], CELL_W, &mut f),
            MIN_CELLS * CELL_W
        );
        assert_eq!(menu_width(&[], CELL_W, &mut f), MIN_CELLS * CELL_W);
    }

    /// G-menu-agree, the invariant `mouse.rs` claimed in a comment and broke on
    /// the line below it: every x the panel is drawn across responds, and no x
    /// outside it does.
    #[test]
    fn the_region_that_responds_is_the_region_that_is_drawn() {
        let mut f = font();
        for items in [
            vec![item("Copy", "Ctrl+C")],
            vec![item("Collapse / expand block", "Ctrl+Shift+L")],
            vec![item("このウィンドウだけ閉じる", "")],
            vec![item("Ok", "")], // hits the floor
        ] {
            let m = menu(100.0, 50.0, items);
            let w = menu_width(&m.items, CELL_W, &mut f);
            let mid_y = 50.0 + CELL_H * 0.5;

            assert_eq!(item_at(&m, 100.0, mid_y, CELL_W, CELL_H, &mut f), Some(0));
            assert_eq!(
                item_at(&m, 100.0 + w, mid_y, CELL_W, CELL_H, &mut f),
                Some(0)
            );
            assert_eq!(
                item_at(&m, 100.0 + w + 0.5, mid_y, CELL_W, CELL_H, &mut f),
                None
            );
            assert_eq!(item_at(&m, 99.5, mid_y, CELL_W, CELL_H, &mut f), None);
        }
    }

    /// The 18-cell constant in miniature: a menu wider than 18 cells used to
    /// answer `None` past that column while drawing rows all the way across.
    #[test]
    fn a_wide_menu_responds_past_the_eighteenth_cell() {
        let mut f = font();
        let m = menu(
            0.0,
            0.0,
            vec![item("Collapse / expand block", "Ctrl+Shift+L")],
        );
        assert!(menu_width(&m.items, CELL_W, &mut f) > 18.0 * CELL_W);
        assert_eq!(
            item_at(&m, 18.5 * CELL_W, CELL_H * 0.5, CELL_W, CELL_H, &mut f),
            Some(0),
            "the hint column has to be clickable; it is where the dead zone was"
        );
    }

    #[test]
    fn rows_are_addressed_top_to_bottom_and_end_with_the_last_item() {
        let mut f = font();
        let m = menu(
            0.0,
            100.0,
            vec![item("a", ""), item("b", ""), item("c", "")],
        );
        assert_eq!(item_at(&m, 0.0, 100.0, CELL_W, CELL_H, &mut f), Some(0));
        assert_eq!(
            item_at(&m, 0.0, 100.0 + CELL_H, CELL_W, CELL_H, &mut f),
            Some(1)
        );
        assert_eq!(
            item_at(&m, 0.0, 100.0 + CELL_H * 2.9, CELL_W, CELL_H, &mut f),
            Some(2)
        );
        assert_eq!(
            item_at(&m, 0.0, 100.0 + CELL_H * 3.0, CELL_W, CELL_H, &mut f),
            None
        );
    }

    /// A click above the menu must not wrap onto its last row — the failure a
    /// `(y - menu.y) / cell_h` cast produces if the negative is not refused
    /// before it becomes a `usize`.
    #[test]
    fn a_point_above_the_menu_does_not_wrap_onto_a_row() {
        let mut f = font();
        let m = menu(0.0, 100.0, vec![item("a", ""), item("b", "")]);
        assert_eq!(item_at(&m, 0.0, 99.0, CELL_W, CELL_H, &mut f), None);
        assert_eq!(item_at(&m, 0.0, 0.0, CELL_W, CELL_H, &mut f), None);
        assert_eq!(item_at(&m, 0.0, -50.0, CELL_W, CELL_H, &mut f), None);
    }

    #[test]
    fn a_separator_is_not_a_row_the_mouse_can_have() {
        let mut f = font();
        let m = menu(0.0, 0.0, vec![item("a", ""), separator(), item("b", "")]);
        assert_eq!(
            item_at(&m, 0.0, CELL_H * 0.5, CELL_W, CELL_H, &mut f),
            Some(0)
        );
        assert_eq!(item_at(&m, 0.0, CELL_H * 1.5, CELL_W, CELL_H, &mut f), None);
        assert_eq!(
            item_at(&m, 0.0, CELL_H * 2.5, CELL_W, CELL_H, &mut f),
            Some(2)
        );
    }

    #[test]
    fn a_degenerate_cell_height_yields_no_row() {
        let mut f = font();
        let m = menu(0.0, 0.0, vec![item("a", "")]);
        assert_eq!(item_at(&m, 0.0, 0.0, CELL_W, 0.0, &mut f), None);
        assert_eq!(item_at(&m, 0.0, 0.0, CELL_W, -16.0, &mut f), None);
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

    /// G-i18n (menu half): `new_default`'s eight hard-coded English labels are
    /// gone. A menu that renders half in the user's language and half in
    /// English reads as a rendering fault, and the width of a translated label
    /// is not bounded by its source's.
    #[test]
    fn the_context_menu_holds_no_untranslated_label() {
        let src = include_str!("../state/menus.rs");
        for literal in [
            "\"Copy\"",
            "\"Paste\"",
            "\"Select All\"",
            "\"Split Vertical\"",
            "\"Split Horizontal\"",
            "\"Close Pane\"",
            "\"Search...\"",
            "\"Settings...\"",
        ] {
            assert!(
                !src.contains(literal),
                "menus.rs builds a menu item from the literal {literal}; \
                 user-facing strings go through fl! and all eight locales"
            );
        }
    }
}
