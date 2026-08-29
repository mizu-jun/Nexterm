//! Row builders shared by the settings-panel categories that have not moved
//! onto the widget layer yet, plus the contrast helper every category uses.
//!
//! UI/UX v3 P1b/P1c retired `draw_label_control_row` and
//! `search_label_color`: migrated tabs describe their rows as `WidgetSpec`s,
//! and `draw_widget` owns the label/control layout and the search highlight.
//! What remains:
//!   - [`draw_section_header`]: a bold, full-width heading line.
//!   - [`draw_description_rows`]: a muted, word-wrapped hint/description
//!     block below a row.
//!
//! All three truncate their text to the column widths in [`RowLayout`] via
//! [`truncate_to_width`] / [`wrap_text`] so long labels/values/descriptions
//! can no longer overflow the content area.
//!
//! UI/UX v3 P5d retired `ensure_readable` from here. It raised alpha and
//! nothing else, so it could only fix translucency — and P5's measurement
//! showed most failures were hue/luminance clashes it returned unchanged.
//! Text now takes an already-corrected colour from
//! `DesignTokens::text_on(level)`, and the few grounds that are not a surface
//! token (this module's danger-button fill among them) go through
//! [`crate::color_util::readable_on`], which is `contrast_correct`.

use crate::font::FontManager;
use crate::glyph_atlas::{GlyphAtlas, TextVertex};
use crate::vertex_util::{add_run_verts, add_string_verts, truncate_run_to_width};

use super::super::util::{danger_fill, wrap_text};
use nexterm_config::SurfaceLevel;

/// Fill / label pair for a destructive-confirmation button.
///
/// UI/UX v3 (G11 follow-up): the Ssh and Keybindings delete dialogs each
/// hand-mixed their own reds and had already drifted apart — `[0.498, 0.196,
/// 0.196]` against `[0.486, 0.180, 0.180]` for the focused fill, and two
/// different resting treatments (a dark red against `surface_1`). Both now
/// derive from `semantic_error`.
///
/// The fills come from [`danger_fill`], which is shared with the close-window
/// dialog: this is the focused/unfocused pair a *settings* delete button
/// wants, where red means "focused" rather than "dangerous".
///
/// The two states take their label from different places, and the reason is
/// measurable rather than aesthetic. The focused fill is red enough that the
/// scheme's own foreground lands near 3:1 on it, so the label comes from
/// [`crate::color_util::on_surface_text`], which picks whichever extreme the
/// fill actually contrasts with. The resting fill is barely tinted, so
/// `text_primary` reads comfortably on it and keeps the button in the same
/// type colour as the rest of the panel. Both are pinned by the tests below.
pub(in crate::renderer) fn danger_button_colors(
    tokens: &nexterm_config::DesignTokens,
    focused: bool,
) -> ([f32; 4], [f32; 4]) {
    if focused {
        let bg = danger_fill(tokens, 0.55);
        (bg, crate::color_util::on_surface_text(bg))
    } else {
        let bg = danger_fill(tokens, 0.18);
        (
            bg,
            crate::color_util::readable_on(tokens.text_on(SurfaceLevel::S2).primary, bg),
        )
    }
}

/// Draw a section header line (no control column, not truncated to a control
/// width since it spans the full content width).
///
/// UI/UX v3 P4b: drawn at the ramp's Subtitle step — `metrics.rs` names that
/// step "section headers inside a panel", which is exactly this. It used to be
/// the terminal cell size with the bold flag set, so the headings had the same
/// size as the rows beneath them and were distinguished by weight alone.
#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_section_header(
    text: &str,
    x: f32,
    y: f32,
    content_w: f32,
    color: [f32; 4],
    sw: f32,
    sh: f32,
    metrics: &nexterm_config::MetricTokens,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    let style = metrics.type_ramp.subtitle;
    let truncated = truncate_run_to_width(text, &style, content_w, font);
    add_run_verts(
        &truncated, &style, x, y, color, sw, sh, font, atlas, queue, text_verts, text_idx,
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
            contrast: nexterm_config::ContrastTarget::Aa,
        };
        nexterm_config::DesignTokens::from_palette(&palette)
    }

    fn contrast_of(color: [f32; 4], bg: [f32; 4]) -> f32 {
        let bg_rgb = [bg[0], bg[1], bg[2]];
        let effective = crate::color_util::composite_over(color, bg_rgb);
        crate::color_util::contrast_ratio(effective, bg_rgb)
    }

    // UI/UX v3 P5b removed two tests from here, and P5d a third. The first two
    // asserted things about `ensure_readable` and about the uncorrected muted
    // token; neither colour is reachable any more, and `nexterm-config`'s
    // `every_builtin_scheme_meets_the_text_floor_on_every_surface` covers every
    // scheme × surface × token, so a hand-listed subset here would only be a
    // second place to forget to update. The third,
    // `ensure_readable_is_a_no_op_when_already_readable`, moved with the helper
    // it tested: `contrast_correct`'s own early return is pinned in
    // `nexterm-config`.

    /// UI/UX v3 (G11 follow-up): the Ssh and Keybindings delete dialogs used
    /// to hand-mix their own reds. Whatever the scheme, the confirmation
    /// label must clear the panel's floor in both states — and on the focused
    /// fill that is not automatic: the scheme's own foreground only reaches
    /// ~3:1 there, which is why the label comes from `on_surface_text`
    /// instead. Measured: 4.65 (Tokyo Night, white label) and 5.00 (Gruvbox
    /// Light, dark label) focused; 6.55 at rest in both.
    #[test]
    fn danger_button_labels_clear_the_contrast_floor() {
        // UI/UX v3 P5d: the resting label is the one live caller of
        // `color_util::readable_on`, because its ground is a blended
        // `danger_fill` rather than a surface token — the case the token layer
        // cannot pre-correct. So sweep every built-in scheme here, plus the
        // out-of-tree Gruvbox Light palette for a light ground.
        let mut cases: Vec<(String, nexterm_config::DesignTokens)> =
            nexterm_config::BuiltinScheme::all()
                .iter()
                .map(|s| {
                    (
                        s.display_name().to_string(),
                        nexterm_config::DesignTokens::from_palette(&s.palette()),
                    )
                })
                .collect();
        cases.push(("gruvbox_light".to_string(), gruvbox_light_tokens()));

        for (name, tokens) in cases {
            for focused in [true, false] {
                let (bg, fg) = danger_button_colors(&tokens, focused);
                let cr = contrast_of(fg, bg);
                assert!(
                    cr >= nexterm_config::MIN_TEXT_CONTRAST,
                    "{name} focused={focused}: label only reached {cr}"
                );
            }
        }
    }

    /// The focused state has to read as the destructive one. Redness is
    /// measured as the red channel's lead over the other two, because a light
    /// scheme's resting fill is brighter overall — comparing raw channels
    /// would call it the redder of the two.
    #[test]
    fn danger_button_focus_state_is_the_redder_one() {
        let redness = |c: [f32; 4]| c[0] - (c[1] + c[2]) / 2.0;
        for tokens in [tokyo_night_tokens(), gruvbox_light_tokens()] {
            let (focused_bg, _) = danger_button_colors(&tokens, true);
            let (resting_bg, _) = danger_button_colors(&tokens, false);
            assert!(
                redness(focused_bg) > redness(resting_bg),
                "focused {focused_bg:?} is not redder than resting {resting_bg:?}"
            );
        }
    }
}
