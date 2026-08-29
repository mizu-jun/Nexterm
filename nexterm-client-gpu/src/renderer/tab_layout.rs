//! How wide a tab is (UI/UX v3 N-3a).
//!
//! The tab bar sizes a tab, records its click region, places its close and
//! tear-out buttons, and scales its accent underline and progress bar from a
//! single number. Until N-3 that number was `label.chars().count() * cell_w` —
//! a count of *characters*, while `add_string_verts` advanced by *display
//! width*. The two disagree by a factor of two for CJK, so a Japanese tab title
//! drew past its own pill and the overhanging half of the label belonged, for
//! click purposes, to the next tab.
//!
//! This module is where that number comes from now: [`tab_width`] measures the
//! label the same way the drawing pass will, and [`fit_tab_width`] — pure, so
//! the clamp and the floor are testable without a device — decides whether what
//! is left of the strip can hold it.
//!
//! N-3a shipped the two functions; N-3b made `ui_verts::build_tab_bar_verts`
//! use them, which is when the seven consumers of the width — pill, click
//! region, accent underline, top highlight, progress bar, close and tear-out
//! buttons — started following a correct one.

use crate::font::FontManager;
use crate::vertex_util::measure_run;

/// The glyph a truncated label ends with.
///
/// A tab must always have room for this plus its padding, which is the floor
/// the maintainer signed off on 2026-08-30 (spec §7): a tab too narrow to show
/// even "there was more text here" is not worth the strip space, and the rule
/// describes itself instead of naming a pixel count that a HiDPI display would
/// falsify.
pub(crate) const ELLIPSIS: &str = "…";

/// Narrowest tab worth drawing: an ellipsis plus the padding either side.
///
/// Measured rather than assumed, for the same reason everything else in N-3 is:
/// the ellipsis is a glyph like any other, and on a font that lacks it the
/// advance is zero — in which case the floor is just the padding, and a tab
/// that small still draws nothing legible but costs nothing either.
pub(crate) fn min_tab_width(
    style: &nexterm_config::TypeStyle,
    padding: f32,
    font: &mut FontManager,
) -> f32 {
    measure_run(ELLIPSIS, style, font) + padding * 2.0
}

/// Fit a tab of `content_w` into the room left in the strip.
///
/// Returns the width to draw, or `None` when the strip cannot hold even
/// `min_w` — the caller stops laying out tabs at that point, which is what the
/// old `label_w < cell_w * 2.0` break did.
///
/// Pure: the caller measures. `content_w` is the measured label (plus any icon
/// drawn beside it), `padding` is applied once on each side, and the result is
/// clamped to `room_left` so a tab never spills out of the strip — the clamp
/// the character-count formula also had, and the only part of it that was
/// right.
pub(crate) fn fit_tab_width(
    content_w: f32,
    padding: f32,
    room_left: f32,
    min_w: f32,
) -> Option<f32> {
    let floor = min_w.max(0.0);
    if room_left < floor || room_left <= 0.0 {
        return None;
    }
    let wanted = content_w.max(0.0) + padding * 2.0;
    Some(wanted.clamp(floor, room_left))
}

/// Width of one tab: its label measured at `style`, plus `icon_w` for a
/// process icon drawn beside it, fitted to the room left.
///
/// The measurement goes through [`measure_run`], the same function
/// `truncate_run_to_width` and `add_run_verts` use, so the width a tab is sized
/// by and the width its label is drawn at cannot disagree — whatever the font
/// reports for any particular glyph (spec §4, `G-width`).
pub(crate) fn tab_width(
    label: &str,
    style: &nexterm_config::TypeStyle,
    icon_w: f32,
    padding: f32,
    room_left: f32,
    font: &mut FontManager,
) -> Option<f32> {
    let content_w = measure_run(label, style, font) + icon_w.max(0.0);
    let min_w = min_tab_width(style, padding, font);
    fit_tab_width(content_w, padding, room_left, min_w)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PADDING: f32 = 10.0;
    const MIN: f32 = 25.0;

    #[test]
    fn a_tab_is_its_content_plus_padding_on_both_sides() {
        assert_eq!(fit_tab_width(100.0, PADDING, 1000.0, MIN), Some(120.0));
    }

    /// The clamp the character-count formula also had — the one part of it that
    /// was right. A tab never spills past the end of the strip.
    #[test]
    fn a_tab_never_exceeds_the_room_left() {
        assert_eq!(fit_tab_width(500.0, PADDING, 200.0, MIN), Some(200.0));
    }

    /// The floor, as signed off in spec §7: a strip that cannot hold an
    /// ellipsis and its padding holds no more tabs. This is what replaces
    /// `label_w < cell_w * 2.0`, which had no meaning once text stopped being
    /// measured in cells.
    #[test]
    fn a_strip_too_narrow_for_the_floor_draws_no_further_tab() {
        assert_eq!(fit_tab_width(100.0, PADDING, MIN - 0.5, MIN), None);
        assert_eq!(fit_tab_width(100.0, PADDING, MIN, MIN), Some(MIN));
    }

    /// A tab whose content is narrower than the floor still gets the floor, so
    /// a one-character title cannot produce a pill too small to click.
    #[test]
    fn a_tiny_label_is_widened_to_the_floor() {
        assert_eq!(fit_tab_width(0.0, 1.0, 1000.0, MIN), Some(MIN));
    }

    /// Degenerate inputs must not produce a negative or reversed rect: a
    /// collapsed strip draws nothing, and a negative content width is treated
    /// as empty rather than eating the padding.
    #[test]
    fn degenerate_inputs_are_refused_rather_than_inverted() {
        assert_eq!(fit_tab_width(100.0, PADDING, 0.0, MIN), None);
        assert_eq!(fit_tab_width(100.0, PADDING, -10.0, MIN), None);
        assert_eq!(fit_tab_width(-100.0, PADDING, 1000.0, 0.0), Some(20.0));
    }

    /// The floor is measured, not assumed. This runs on whatever font stack CI
    /// has — which answers the same advance for every character (spec §6) — so
    /// it asserts the shape of the rule, not a number: the floor is the
    /// padding plus something non-negative, and more padding moves it.
    #[test]
    fn the_floor_is_the_measured_ellipsis_plus_padding() {
        let mut font = FontManager::new("monospace", 14.0, &[], 1.0, true);
        let style = nexterm_config::MetricTokens::default().type_ramp.body;

        let narrow = min_tab_width(&style, 4.0, &mut font);
        let wide = min_tab_width(&style, 12.0, &mut font);

        assert!(narrow >= 8.0, "the floor includes its padding: {narrow}");
        assert!(
            (wide - narrow - 16.0).abs() < 1e-3,
            "extra padding must move the floor by exactly twice: {narrow} → {wide}"
        );
    }

    /// G-width in miniature: the width a tab is *sized* by comes from the same
    /// `measure_run` the drawing pass uses, so the two cannot disagree. Phrased
    /// as an equality between paths rather than as a claim about CJK metrics —
    /// CI's font stack has no real CJK face (spec §6), so the latter would pass
    /// for the wrong reason.
    #[test]
    fn a_tabs_width_contains_the_run_that_will_be_drawn_in_it() {
        let mut font = FontManager::new("monospace", 14.0, &[], 1.0, true);
        let style = nexterm_config::MetricTokens::default().type_ramp.body;

        for label in ["pane:1", "ビルド", "a very long tab title indeed"] {
            let drawn = measure_run(label, &style, &mut font);
            let width = tab_width(label, &style, 0.0, PADDING, 10_000.0, &mut font)
                .expect("a wide strip fits any tab");
            assert!(
                width >= drawn,
                "{label:?} is drawn {drawn} wide in a {width}-wide tab"
            );
        }
    }

    /// G-single: the tab bar takes its width from here and nowhere else. The
    /// formula this phase removed — a character count times the cell width —
    /// must not come back, and neither must a second call that could drift
    /// from the first.
    #[test]
    fn the_tab_bar_computes_a_tab_width_in_exactly_one_place() {
        let src = include_str!("ui_verts.rs");
        assert_eq!(
            src.matches("tab_layout::tab_width(").count(),
            1,
            "a tab's width is computed in exactly one place in ui_verts.rs"
        );
        assert!(
            !tab_region(src).contains("chars().count() as f32 * cell_w"),
            "the tab bar sizes something by counting characters again; \
             tab_width owns a tab's width"
        );
    }

    /// The part of the tab-bar builder that draws *tabs*: the per-tab loop
    /// plus the drag ghost, which is a copy of one.
    ///
    /// Bounded deliberately. The same function also draws the `+`, `▾` and
    /// Settings pills, which are fixed-width buttons sized from a config
    /// constant rather than from their label, and the status bar next door is
    /// column-oriented by design (P4 spec §5.2). A gate over the whole file —
    /// or even the whole function — would trip on surfaces N-3 leaves on the
    /// cell path on purpose.
    fn tab_region(src: &str) -> &str {
        let start = src
            .find("for (i, &pane_id) in pane_ids.iter().enumerate()")
            .expect("the per-tab loop exists");
        let rest = &src[start..];
        let end = rest
            .find("new-tab `+` pill")
            .expect("the `+` pill follows the tabs");
        &rest[..end]
    }

    /// The 24-character cap went with it: 24 characters is 24 cells of Latin or
    /// 48 of Japanese, so it never bounded the drawn width. Truncation is by
    /// width budget now.
    #[test]
    fn the_tab_label_is_no_longer_capped_by_character_count() {
        let src = include_str!("ui_verts.rs");
        assert!(
            !tab_region(src).contains("chars().take(24)"),
            "the tab bar caps a title by character count again — the ghost \
             tab carried the same cap and was migrated with it"
        );
    }

    /// A process icon widens the tab it sits in — the icon is drawn beside the
    /// label (spec D3), so its width has to be in the tab's.
    #[test]
    fn a_process_icon_widens_its_tab() {
        let mut font = FontManager::new("monospace", 14.0, &[], 1.0, true);
        let style = nexterm_config::MetricTokens::default().type_ramp.body;

        let bare = tab_width("build", &style, 0.0, PADDING, 10_000.0, &mut font);
        let with_icon = tab_width("build", &style, 16.0, PADDING, 10_000.0, &mut font);
        assert_eq!(
            with_icon.zip(bare).map(|(a, b)| a - b),
            Some(16.0),
            "the icon's width lands in the tab's, unrounded"
        );
    }
}
