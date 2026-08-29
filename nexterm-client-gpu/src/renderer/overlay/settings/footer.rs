//! The settings footer's link row (UI/UX v3 P4c).
//!
//! The footer carries two right-aligned links — `↗ Open config.toml` and
//! `↺ Reset category` — and until P4c their geometry existed **twice**: the
//! builder in the parent module computed a width from `visual_width * cell_w`
//! and the hit-test in `event_handler/settings_panel_hit.rs` recomputed the
//! same expression, label formatting included. Two copies of an arithmetic
//! that has to agree is the shape P1's widget layer and P6's `bar_rects` were
//! each built to remove, and it is what blocked the links from moving to the
//! P4b type ramp: a proportional label's width is not a multiple of `cell_w`,
//! so the mirrored formula would have gone quietly wrong rather than loudly.
//!
//! This module owns the labels, the ramp step, the measurement and the rects.
//! [`place_links`] is pure — it takes widths rather than measuring — so the
//! right-alignment and the padding are unit-testable without a device, and
//! [`footer_links`] is the one door both the renderer and the hit-test use.

use crate::font::FontManager;
use crate::renderer::overlay::widgets::spec::WidgetRect;
use crate::vertex_util::measure_run;

/// Gap between the two links, in cells.
///
/// Matches the `cell_w * 3.0` the two mirrored copies each open-coded, so
/// P4c is not also a spacing change.
const LINK_GAP_CELLS: f32 = 3.0;

/// Padding around a link's label inside its clickable rect, in cells.
///
/// The pre-P4c hit-test gave the open link one cell of slack on its left (its
/// rect ran to the panel edge) and the reset link one cell on its right. One
/// constant for both keeps the two links equally forgiving to click.
const LINK_PAD_CELLS: f32 = 1.0;

/// One footer link: the text to draw and the rect that is both its slot and
/// its click target.
pub(in crate::renderer) struct FooterLink {
    /// The rect the link occupies. The label draws at `rect.x`, vertically
    /// centred; a click anywhere inside it activates the link.
    pub rect: WidgetRect,
    /// The localised label, glyph included.
    pub label: String,
}

/// The footer's link row for one frame.
pub(in crate::renderer) struct FooterLinks {
    /// `↗ Open config.toml`. Always present.
    pub open: FooterLink,
    /// `↺ Reset category` — absent for the list-based categories (SSH /
    /// Keybindings / Profiles), where a reset would delete user data.
    pub reset: Option<FooterLink>,
}

/// The ramp step the footer links draw at.
///
/// Body: they are ordinary UI labels, the same class as a settings row's
/// label, and reading as one size with the rows above them is the point.
pub(in crate::renderer) fn link_style() -> nexterm_config::TypeStyle {
    // Deliberately unscaled: `FontManager::chrome_metrics` owns the DPI
    // conversion, and handing it a `scaled()` ramp would double-scale it.
    nexterm_config::MetricTokens::default().type_ramp.body
}

/// What the open-config link says, without its glyph.
///
/// The AccessKit node announces this rather than [`open_label`]: `↗` is a
/// visual affordance, and a screen reader reading "north east arrow" before
/// every activation is noise (UI/UX v3 P4d).
pub(crate) fn open_text() -> String {
    nexterm_i18n::fl!("settings-open-config-file")
}

/// What the reset link says, without its glyph. See [`open_text`].
pub(crate) fn reset_text() -> String {
    nexterm_i18n::fl!("settings-reset-category")
}

/// The `↗ Open config.toml` label.
pub(in crate::renderer) fn open_label() -> String {
    format!("↗ {}", open_text())
}

/// The `↺ Reset category` label.
pub(in crate::renderer) fn reset_label() -> String {
    format!("↺ {}", reset_text())
}

/// Place the two links, right-aligned inside the footer, from their measured
/// widths.
///
/// Pure: the caller measures. That split is what lets the alignment be tested
/// without a `FontManager`, and it is the same shape `place_tooltip` took in
/// P4b for the same reason.
pub(in crate::renderer) fn place_links(
    panel_x: f32,
    panel_w: f32,
    footer_y: f32,
    footer_h: f32,
    cell_w: f32,
    open_w: f32,
    reset_w: Option<f32>,
) -> (WidgetRect, Option<WidgetRect>) {
    let pad = cell_w * LINK_PAD_CELLS;
    let open_x = panel_x + panel_w - open_w - pad;
    let open = WidgetRect::new(open_x, footer_y, open_w + pad, footer_h);
    let reset = reset_w.map(|w| {
        let x = open_x - cell_w * LINK_GAP_CELLS - w;
        WidgetRect::new(x, footer_y, w + pad, footer_h)
    });
    (open, reset)
}

/// Measure and place the footer's links.
///
/// The one door: the renderer draws what this returns and the hit-test tests
/// against it, so the drawn glyphs and the click target come from a single
/// measurement by construction.
pub(in crate::renderer) fn footer_links(
    panel_x: f32,
    panel_w: f32,
    footer_y: f32,
    footer_h: f32,
    cell_w: f32,
    resettable: bool,
    font: &mut FontManager,
) -> FooterLinks {
    let style = link_style();
    let open_text = open_label();
    let open_w = measure_run(&open_text, &style, font);
    let reset_text = resettable.then(reset_label);
    let reset_w = reset_text
        .as_ref()
        .map(|text| measure_run(text, &style, font));

    let (open_rect, reset_rect) = place_links(
        panel_x, panel_w, footer_y, footer_h, cell_w, open_w, reset_w,
    );
    FooterLinks {
        open: FooterLink {
            rect: open_rect,
            label: open_text,
        },
        reset: reset_text
            .zip(reset_rect)
            .map(|(label, rect)| FooterLink { rect, label }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL_X: f32 = 100.0;
    const PANEL_W: f32 = 800.0;
    const FOOTER_Y: f32 = 500.0;
    const FOOTER_H: f32 = 30.0;
    const CELL_W: f32 = 10.0;

    fn place(open_w: f32, reset_w: Option<f32>) -> (WidgetRect, Option<WidgetRect>) {
        place_links(
            PANEL_X, PANEL_W, FOOTER_Y, FOOTER_H, CELL_W, open_w, reset_w,
        )
    }

    #[test]
    fn the_open_link_hugs_the_panels_right_edge() {
        let (open, reset) = place(120.0, None);
        assert!(reset.is_none());
        assert_eq!(open.x, PANEL_X + PANEL_W - 120.0 - CELL_W);
        assert_eq!(open.x + open.w, PANEL_X + PANEL_W);
        assert_eq!(open.y, FOOTER_Y);
        assert_eq!(open.h, FOOTER_H);
    }

    /// The reset link sits left of the open link with a fixed gap, and the two
    /// rects must not overlap — an overlap would route one link's clicks to
    /// the other, which is exactly the failure a single source prevents.
    #[test]
    fn the_reset_link_sits_left_of_the_open_link_without_touching_it() {
        let (open, reset) = place(120.0, Some(90.0));
        let reset = reset.expect("a resettable category has a reset link");
        assert!(reset.x + reset.w <= open.x, "{reset:?} overlaps {open:?}");
        assert_eq!(open.x - reset.x - 90.0, CELL_W * LINK_GAP_CELLS);
        assert_eq!(reset.y, FOOTER_Y);
    }

    /// A proportional label's width is not a multiple of `cell_w`, which is
    /// the whole reason the mirrored formula had to go: the placement must
    /// carry the fractional width through rather than round it to cells.
    #[test]
    fn a_fractional_label_width_is_carried_through_unrounded() {
        let (open, _) = place(117.5, None);
        assert_eq!(open.x, PANEL_X + PANEL_W - 117.5 - CELL_W);
        assert_eq!(open.w, 117.5 + CELL_W);
    }

    /// Both links get the same slack, so neither is harder to hit than the
    /// other — the pre-P4c pair gave the open link its cell on the left and
    /// the reset link its cell on the right.
    #[test]
    fn both_links_get_the_same_padding() {
        let (open, reset) = place(120.0, Some(120.0));
        assert_eq!(open.w, reset.expect("reset link").w);
    }

    /// A category with nothing to reset must not leave a phantom click target
    /// where the link would have been.
    #[test]
    fn a_non_resettable_category_has_no_reset_rect() {
        assert!(place(120.0, None).1.is_none());
    }

    /// The structural gate, in the shape P6's `bar_rects` gate takes: neither
    /// the builder nor the hit-test may reconstruct a label or a width of its
    /// own. If one comes back, the two have a way to disagree again and the
    /// links silently drift from their click targets.
    #[test]
    fn neither_the_builder_nor_the_hit_test_rebuilds_the_links() {
        let builder = include_str!("mod.rs");
        let hit = include_str!("../../event_handler/settings_panel_hit.rs");

        for (name, src) in [("settings/mod.rs", builder), ("settings_panel_hit.rs", hit)] {
            assert_eq!(
                src.matches("footer_links(").count(),
                1,
                "{name} does not take its footer links from exactly one call"
            );
            assert!(
                !src.contains("settings-open-config-file"),
                "{name} rebuilds the open-config label; footer::open_label owns that"
            );
            assert!(
                !src.contains("settings-reset-category"),
                "{name} rebuilds the reset label; footer::reset_label owns that"
            );
        }
    }

    /// P4c's other half: the dialog button boxes are sized from the same
    /// `measure_run` their labels are drawn with. They carry no click target —
    /// consent and close-window dialogs are keyboard- and AccessKit-driven —
    /// so this is the whole of the change, and a `visual_width` box would put
    /// the ramp's glyphs back in a cell-sized slot.
    #[test]
    fn the_dialog_buttons_are_sized_from_the_run_they_draw() {
        let src = include_str!("../dialog.rs");
        assert!(
            !src.contains("visual_width(btn)") && !src.contains("visual_width(label)"),
            "dialog.rs sizes a button from cells again; measure_run owns that"
        );
    }
}
