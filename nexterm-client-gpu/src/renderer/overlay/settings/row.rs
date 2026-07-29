//! Common row builders shared by every settings-panel category.
//!
//! Rather than a trait-based abstraction, these are plain functions
//! (Simplicity First) covering the three recurring shapes:
//!   - [`draw_section_header`]: a bold, full-width heading line.
//!   - [`draw_label_control_row`]: a label (left column) + value/control
//!     (right column) row, with an optional focus-highlight background
//!     spanning the full row.
//!   - [`draw_description_rows`]: a muted, word-wrapped hint/description
//!     block below a row.
//!
//! All three truncate their text to the column widths in [`RowLayout`] via
//! [`truncate_to_width`] / [`wrap_text`] so long labels/values/descriptions
//! can no longer overflow the content area.
//!
//! [`ensure_readable`] additionally covers the settings-panel contrast audit
//! (Phase B3): tokens like `text_muted` carry a fixed alpha tuned for
//! readability against an opaque UI in general, but once alpha-blended
//! into a specific panel surface (e.g. `surface_2` for body content,
//! `surface_3` for the title bar) the effective on-screen color can land
//! under the project's 4.5:1 contrast floor. Callers pass the exact
//! background the text is drawn over so the check reflects reality rather
//! than a generic assumption.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::vertex_util::{add_px_rect, add_string_verts, truncate_to_width};

use super::super::util::wrap_text;
use super::layout::RowLayout;

/// WCAG 2.x contrast floor used throughout the settings panel (project
/// accessibility guideline: see `CLAUDE.md` "UI/UX Guidelines").
pub(in crate::renderer) const MIN_TEXT_CONTRAST: f32 = 4.5;

/// Bump `color`'s alpha (hue/RGB unchanged) until it reaches at least
/// `min_ratio` WCAG contrast against `bg`, an opaque panel surface color.
/// Returns `color` unchanged when it already clears the bar. Alpha is
/// capped at 1.0 (fully opaque); if even that fails to reach `min_ratio`
/// (a hue/luminance clash rather than a translucency problem), the fully
/// opaque color is returned as the best achievable result.
pub(in crate::renderer) fn ensure_readable(
    color: [f32; 4],
    bg: [f32; 4],
    min_ratio: f32,
) -> [f32; 4] {
    let bg_rgb = [bg[0], bg[1], bg[2]];
    let mut alpha = color[3];
    loop {
        let candidate = [color[0], color[1], color[2], alpha];
        let effective = crate::color_util::composite_over(candidate, bg_rgb);
        if crate::color_util::contrast_ratio(effective, bg_rgb) >= min_ratio || alpha >= 1.0 {
            return candidate;
        }
        alpha = (alpha + 0.02).min(1.0);
    }
}

/// P2-A (WT-like UX): label colour override while a field-level search
/// query is active. Rows whose rendered label fuzzy-matches the query pop
/// out in the accent colour (contrast-corrected); everything else keeps
/// its `base` colour. With an idle search this is a pass-through.
pub(in crate::renderer) fn search_label_color(
    sp: &crate::settings_panel::SettingsPanel,
    label: &str,
    base: [f32; 4],
    tokens: &nexterm_config::DesignTokens,
) -> [f32; 4] {
    if sp.label_matches_search(label) {
        ensure_readable(tokens.accent_primary, tokens.surface_2, MIN_TEXT_CONTRAST)
    } else {
        base
    }
}

/// Draw a section header line (bold, no control column, not truncated to a
/// control width since it spans the full content width).
#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_section_header(
    text: &str,
    x: f32,
    y: f32,
    content_w: f32,
    color: [f32; 4],
    sw: f32,
    sh: f32,
    cell_w: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    let truncated = truncate_to_width(text, content_w, cell_w);
    add_string_verts(
        &truncated, x, y, color, true, sw, sh, cell_w, font, atlas, queue, text_verts, text_idx,
    );
}

/// Draw a two-column label+control row.
///
/// When `focused` is true, a highlight rect is drawn spanning the full row
/// width (label column + gap + control column) before the text, matching
/// the existing focus-highlight visual language used throughout the panel.
#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_label_control_row(
    sp: &crate::settings_panel::SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    content_inner_x: f32,
    row_y: f32,
    row_h: f32,
    layout: &RowLayout,
    label: &str,
    value: &str,
    focused: bool,
    focus_bg: [f32; 4],
    label_color: [f32; 4],
    value_color: [f32; 4],
    sw: f32,
    sh: f32,
    cell_w: f32,
    cell_h: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    if focused {
        let row_w = layout.control_x_off + layout.control_w + cell_w * 0.6;
        add_px_rect(
            content_inner_x - cell_w * 0.3,
            row_y - cell_h * 0.1,
            row_w,
            row_h,
            focus_bg,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
    }

    // P2-A: matching rows pop out in the accent colour while searching.
    let label_color = search_label_color(sp, label, label_color, tokens);
    let label_text = truncate_to_width(label, layout.label_w, cell_w);
    add_string_verts(
        &label_text,
        content_inner_x,
        row_y,
        label_color,
        focused,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    let value_text = truncate_to_width(value, layout.control_w, cell_w);
    add_string_verts(
        &value_text,
        content_inner_x + layout.control_x_off,
        row_y,
        value_color,
        focused,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );
}

/// Draw a word-wrapped description/hint block starting at `(x, y)`, one
/// line per `line_h`. Returns the y position immediately below the last
/// line, so callers can stack further content beneath it.
#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_description_rows(
    text: &str,
    x: f32,
    y: f32,
    line_h: f32,
    max_cols: usize,
    color: [f32; 4],
    sw: f32,
    sh: f32,
    cell_w: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) -> f32 {
    let lines = wrap_text(text, max_cols);
    for (i, line) in lines.iter().enumerate() {
        add_string_verts(
            line,
            x,
            y + line_h * i as f32,
            color,
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            queue,
            text_verts,
            text_idx,
        );
    }
    y + line_h * lines.len().max(1) as f32
}

#[cfg(test)]
mod contrast_tests {
    use super::*;

    fn tokyo_night_tokens() -> nexterm_config::DesignTokens {
        nexterm_config::DesignTokens::default()
    }

    fn gruvbox_light_tokens() -> nexterm_config::DesignTokens {
        let palette = nexterm_config::SchemePalette {
            fg: [0x3C, 0x38, 0x36],
            bg: [0xFB, 0xF1, 0xC7],
            ansi: [
                [0xFB, 0xF1, 0xC7],
                [0xCC, 0x24, 0x1D],
                [0x98, 0x97, 0x1A],
                [0xD7, 0x99, 0x21],
                [0x45, 0x85, 0x88],
                [0xB1, 0x62, 0x86],
                [0x68, 0x9D, 0x6A],
                [0x7C, 0x6F, 0x64],
                [0x92, 0x83, 0x74],
                [0x9D, 0x00, 0x06],
                [0x79, 0x74, 0x0E],
                [0xB5, 0x76, 0x14],
                [0x07, 0x66, 0x78],
                [0x8F, 0x3F, 0x71],
                [0x42, 0x7B, 0x58],
                [0x3C, 0x38, 0x36],
            ],
        };
        nexterm_config::DesignTokens::from_palette(&palette)
    }

    fn contrast_of(color: [f32; 4], bg: [f32; 4]) -> f32 {
        let bg_rgb = [bg[0], bg[1], bg[2]];
        let effective = crate::color_util::composite_over(color, bg_rgb);
        crate::color_util::contrast_ratio(effective, bg_rgb)
    }

    /// Regression guard: the raw `text_muted` token, unmodified, does NOT
    /// clear the contrast floor against the panel's body-content surface;
    /// this is the bug `ensure_readable` exists to fix. If this assertion
    /// ever starts failing, `DesignTokens::text_muted` itself has changed
    /// and `ensure_readable`'s premise should be re-checked.
    #[test]
    fn raw_text_muted_fails_contrast_on_content_surface() {
        let tokens = tokyo_night_tokens();
        let cr = contrast_of(tokens.text_muted, tokens.surface_2);
        assert!(
            cr < MIN_TEXT_CONTRAST,
            "expected raw text_muted to fail, got {cr}"
        );
    }

    /// The representative (text color, background) pairs actually used
    /// throughout the settings panel must clear 4.5:1 once passed through
    /// `ensure_readable`, for both a dark and a light default theme.
    #[test]
    fn settings_panel_text_pairs_meet_contrast_after_ensure_readable() {
        for tokens in [tokyo_night_tokens(), gruvbox_light_tokens()] {
            let pairs = [
                (tokens.text_muted, tokens.surface_0),
                (tokens.text_muted, tokens.surface_1),
                (tokens.text_muted, tokens.surface_2),
                (tokens.text_secondary, tokens.surface_3),
            ];
            for (color, bg) in pairs {
                let adjusted = ensure_readable(color, bg, MIN_TEXT_CONTRAST);
                let cr = contrast_of(adjusted, bg);
                assert!(
                    cr >= MIN_TEXT_CONTRAST - 0.01,
                    "pair {:?} on {:?} only reached {cr}",
                    color,
                    bg
                );
            }
        }
    }

    /// A color that already clears the bar must be returned unchanged (no
    /// unnecessary alpha bump / no visual drift for already-readable text).
    #[test]
    fn ensure_readable_is_a_no_op_when_already_readable() {
        let tokens = tokyo_night_tokens();
        let already_fine = tokens.text_primary;
        let adjusted = ensure_readable(already_fine, tokens.surface_2, MIN_TEXT_CONTRAST);
        assert_eq!(adjusted, already_fine);
    }
}
