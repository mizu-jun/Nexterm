//! Text-entry controls: the free-text field and the key-combination capture.

use crate::font::FontManager;
use crate::glyph_atlas::GlyphAtlas;
use crate::vertex_util::{add_px_rounded_rect_sdf, add_string_verts, truncate_to_width};

use super::super::super::settings::row::{MIN_TEXT_CONTRAST, ensure_readable};
use super::super::spec::WidgetSpec;
use super::{WidgetSink, WidgetTheme, text_baseline};

/// Glyph used for the insertion point.
const CARET: char = '│';
/// Marker shown while a key capture is armed.
const RECORDING_MARK: &str = "◉ ";

/// Bordered input box; a caret is shown at the insertion point while editing.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_text_field(
    spec: &WidgetSpec,
    value: &str,
    editing: bool,
    caret: Option<usize>,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    draw_field_box(spec, theme.tokens.border_default, theme, sink);

    let shown = if editing {
        with_caret(value, caret)
    } else {
        value.to_string()
    };
    draw_field_text(spec, &shown, editing, theme, font, atlas, queue, sink);
}

/// Key-combination capture. While recording, the box is outlined in the accent
/// colour and prefixed with a marker so it is obvious the next key press will
/// be swallowed rather than typed.
///
/// The prose that explains recording lives in the widget's tooltip, which the
/// visual tooltip and the AccessKit description both read — keeping localised
/// strings out of the drawing layer.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_key_capture(
    spec: &WidgetSpec,
    value: &str,
    recording: bool,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    let border = if recording {
        theme.tokens.accent_primary
    } else {
        theme.tokens.border_default
    };
    draw_field_box(spec, border, theme, sink);

    let shown = if recording {
        format!("{RECORDING_MARK}{value}")
    } else {
        value.to_string()
    };
    draw_field_text(spec, &shown, recording, theme, font, atlas, queue, sink);
}

/// Insert a caret marker into `value` at `caret`.
///
/// Falls back to the end of the string when the offset is absent or lands
/// mid-character: the edit buffer and this renderer can disagree for one frame
/// after a multi-byte edit, and a panicking slice would be a poor trade for a
/// caret that is briefly one character off.
fn with_caret(value: &str, caret: Option<usize>) -> String {
    let at = match caret {
        Some(c) if c <= value.len() && value.is_char_boundary(c) => c,
        _ => value.len(),
    };
    let mut s = String::with_capacity(value.len() + CARET.len_utf8());
    s.push_str(&value[..at]);
    s.push(CARET);
    s.push_str(&value[at..]);
    s
}

/// Border + face of an input box.
///
/// Split from the text so the geometry can be exercised without font state.
fn draw_field_box(
    spec: &WidgetSpec,
    border: [f32; 4],
    theme: &WidgetTheme<'_>,
    sink: &mut WidgetSink<'_>,
) {
    let r = theme.metrics.radius.control;
    add_px_rounded_rect_sdf(
        spec.control_rect.x - 1.0,
        spec.control_rect.y - 1.0,
        spec.control_rect.w + 2.0,
        spec.control_rect.h + 2.0,
        r + 1.0,
        [border[0], border[1], border[2], 1.0],
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
    add_px_rounded_rect_sdf(
        spec.control_rect.x,
        spec.control_rect.y,
        spec.control_rect.w,
        spec.control_rect.h,
        r,
        theme.tokens.surface_1,
        theme.sw,
        theme.sh,
        sink.bg_verts,
        sink.bg_idx,
    );
}

/// Contents of an input box, contrast-corrected against its face.
#[allow(clippy::too_many_arguments)]
fn draw_field_text(
    spec: &WidgetSpec,
    shown: &str,
    emphasised: bool,
    theme: &WidgetTheme<'_>,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    sink: &mut WidgetSink<'_>,
) {
    let color = ensure_readable(
        if spec.enabled() {
            theme.tokens.text_primary
        } else {
            theme.tokens.text_muted
        },
        theme.tokens.surface_1,
        MIN_TEXT_CONTRAST,
    );
    let inner = (spec.control_rect.w - theme.cell_w).max(0.0);
    let text = truncate_to_width(shown, inner, theme.cell_w);
    add_string_verts(
        &text,
        spec.control_rect.x + theme.cell_w * 0.5,
        text_baseline(spec.rect, theme),
        color,
        emphasised,
        theme.sw,
        theme.sh,
        theme.cell_w,
        font,
        atlas,
        queue,
        sink.text_verts,
        sink.text_idx,
    );
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::renderer::overlay::widgets::spec::WidgetKind;

    #[test]
    fn the_caret_lands_at_the_requested_offset() {
        assert_eq!(with_caret("abcd", Some(2)), format!("ab{CARET}cd"));
        assert_eq!(with_caret("abcd", Some(0)), format!("{CARET}abcd"));
        assert_eq!(with_caret("abcd", Some(4)), format!("abcd{CARET}"));
    }

    #[test]
    fn a_missing_or_out_of_range_caret_falls_back_to_the_end() {
        assert_eq!(with_caret("abcd", None), format!("abcd{CARET}"));
        assert_eq!(with_caret("abcd", Some(99)), format!("abcd{CARET}"));
    }

    #[test]
    fn a_caret_inside_a_multibyte_character_does_not_panic() {
        // Byte offset 1 is mid-character in "あ"; the caret slides to the end
        // rather than slicing through the code point.
        assert_eq!(with_caret("あい", Some(1)), format!("あい{CARET}"));
        // A valid boundary between the two characters still works.
        assert_eq!(with_caret("あい", Some(3)), format!("あ{CARET}い"));
    }

    #[test]
    fn a_field_box_is_a_border_plus_a_face() {
        let spec = spec_at(WidgetKind::Text {
            value: "x".into(),
            editing: false,
            caret: None,
        });
        assert_eq!(
            bg_quads(|t, s| draw_field_box(&spec, [0.0, 0.0, 0.0, 1.0], t, s)),
            2
        );
    }
}
