//! Shared helpers used by overlay rendering.

/// Extract the requesting pane ID from a consent-dialog kind
pub(super) fn pane_id_for(kind: &crate::state::ConsentKind) -> Option<u32> {
    use crate::state::ConsentKind;
    match kind {
        ConsentKind::OpenUrl(_) => None,
        ConsentKind::ClipboardWrite { source_pane, .. } => *source_pane,
        ConsentKind::Notification { source_pane, .. } => Some(*source_pane),
    }
}

/// Return the preview string for a consent-dialog kind
pub(super) fn preview_text(kind: &crate::state::ConsentKind) -> String {
    use crate::state::ConsentKind;
    match kind {
        ConsentKind::OpenUrl(url) => url.clone(),
        ConsentKind::ClipboardWrite { text, .. } => {
            // Replace control chars and newlines with spaces for safety
            let safe: String = text
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            // Truncate by byte length
            if safe.len() > 200 {
                let mut end = 200;
                while end > 0 && !safe.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &safe[..end])
            } else {
                safe
            }
        }
        ConsentKind::Notification { title, body, .. } => format!("{title}: {body}"),
    }
}

/// Wrap text to multiple lines at the given column width (CJK full-width chars count as 2 columns)
pub(super) fn wrap_text(s: &str, max_cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_cols = 0usize;
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if current_cols + w > max_cols && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_cols = 0;
        }
        current.push(c);
        current_cols += w;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Draw the shared chrome for every floating overlay panel:
/// drop-shadow → 1 px border ring → rounded filled background.
///
/// All colors are taken from `tokens` so the panel adapts to the active color scheme.
/// The border ring is drawn by overdrawing the background with a slightly larger rect
/// using `tokens.border_default` at reduced opacity.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_overlay_panel(
    px: f32,
    py: f32,
    pw: f32,
    ph: f32,
    tokens: &nexterm_config::DesignTokens,
    shadow_offset: f32,
    radius: f32,
    sw: f32,
    sh: f32,
    bg_verts: &mut Vec<crate::glyph_atlas::BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    use crate::vertex_util::add_rounded_px_rect;

    // 1. Drop shadow (solid dark, offset down-right).
    let shadow = [0.0f32, 0.0, 0.0, 0.55];
    add_rounded_px_rect(
        px + shadow_offset,
        py + shadow_offset,
        pw,
        ph,
        shadow,
        radius,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // 2. Border ring — 1 px wider on every side, tokens.border_default at ~18% alpha.
    let bd = tokens.border_default;
    let border_color = [bd[0], bd[1], bd[2], 0.18];
    add_rounded_px_rect(
        px - 1.0,
        py - 1.0,
        pw + 2.0,
        ph + 2.0,
        border_color,
        radius + 1.0,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // 3. Panel background — tokens.surface_2, fully opaque.
    let bg = tokens.surface_2;
    add_rounded_px_rect(px, py, pw, ph, bg, radius, sw, sh, bg_verts, bg_idx);
}
