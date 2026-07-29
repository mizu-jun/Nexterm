//! Security category: consent-policy cyclers + byte-cap fields.
//!
//! 7 fields: 4 consent-policy cyclers (`< allow/deny/prompt >`) followed by
//! 3 decimal byte-cap inputs (see `SettingsPanel::SECURITY_FIELD_COUNT`).
//! Row geometry mirrors `settings_panel_hit.rs::SecurityRow` exactly.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;

use super::layout::compute_row_layout;
use super::row::{MIN_TEXT_CONTRAST, draw_label_control_row, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_security_tab(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    content_top: f32,
    content_inner_x: f32,
    content_w: f32,
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
    let layout = compute_row_layout(content_w, cell_w);
    let focus = sp.security_field_focus;

    // P2-B: search collapse — only matching rows render, compacted to the
    // top. The hit-test derives Y from the same list.
    let visible = sp.visible_security_rows();
    for (slot, &row) in visible.iter().enumerate() {
        let i = row as u8;
        let y = content_top + cell_h * (0.5 + slot as f32 * 1.4);
        let is_focused = focus == i;
        let label_color = if is_focused {
            tokens.text_primary
        } else {
            tokens.text_secondary
        };
        let value = if let Some(policy) = sp.security_policy_at(i) {
            format!("< {} >", SettingsPanel::consent_display_label(policy))
        } else if is_focused && let Some(state) = sp.security_field_editing.as_ref() {
            format!("{}|", state.buffer)
        } else {
            sp.security_bytes_at(i).unwrap_or(0).to_string()
        };
        draw_label_control_row(
            sp,
            tokens,
            content_inner_x,
            y,
            cell_h * 1.2,
            &layout,
            &SettingsPanel::security_field_label(i),
            &value,
            is_focused,
            tokens.surface_2,
            label_color,
            label_color,
            sw,
            sh,
            cell_w,
            cell_h,
            font,
            atlas,
            queue,
            bg_verts,
            bg_idx,
            text_verts,
            text_idx,
        );
    }

    // Footer note: the plugin_read policy has no synchronous prompt path
    // yet, so `prompt` currently behaves as `deny`.
    let note_y = content_top + cell_h * (0.5 + SettingsPanel::SECURITY_FIELD_COUNT as f32 * 1.4);
    super::row::draw_description_rows(
        &nexterm_i18n::fl!("settings-security-note"),
        content_inner_x,
        note_y,
        cell_h,
        (content_w / cell_w).floor() as usize,
        ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST),
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
