//! Blocks category: enabled / border width / status badge toggles.
//!
//! Rows 0..=2 are clickable (see `settings_panel_hit.rs::BlocksRow`); row 3
//! is a static hint with no hit zone. Fully editable via mouse click —
//! `mouse.rs`'s `BlocksRow` handler toggles/cycles the value and immediately
//! calls `save_to_toml()`, so changes persist without a separate Save step.
//! Direct edits to `nexterm.toml` also hot-reload.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::add_string_verts;

use super::layout::compute_row_layout;
use super::row::{MIN_TEXT_CONTRAST, draw_label_control_row, ensure_readable};

#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_blocks_tab(
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

    let rows: [(String, String); 3] = [
        (
            nexterm_i18n::fl!("settings-blocks-enabled"),
            if sp.blocks_enabled { "[ON ]" } else { "[OFF]" }.to_string(),
        ),
        (
            nexterm_i18n::fl!("settings-blocks-border-width"),
            format!("[{}]", sp.blocks_border_width_px),
        ),
        (
            nexterm_i18n::fl!("settings-blocks-status-badge"),
            if sp.blocks_show_exit_code_badge {
                "[ON ]"
            } else {
                "[OFF]"
            }
            .to_string(),
        ),
    ];
    // P2-B: search collapse — only matching rows render, compacted to the
    // top. The hit-test derives Y from the same list.
    let visible = sp.visible_blocks_rows();
    for (slot, &row) in visible.iter().enumerate() {
        let (label, value) = &rows[row];
        // Row geometry mirrors `settings_panel_hit.rs::BlocksRow` exactly —
        // keep both in sync.
        let y = content_top + cell_h * (0.5 + slot as f32 * 1.6);
        let value_color = match value.as_str() {
            "[ON ]" => tokens.semantic_success,
            "[OFF]" => ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST),
            _ => tokens.text_primary,
        };
        draw_label_control_row(
            sp,
            tokens,
            content_inner_x,
            y,
            cell_h * 1.2,
            &layout,
            label,
            value,
            false,
            tokens.surface_2,
            tokens.text_secondary,
            value_color,
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

    // Tip (final row, no hit zone) — placed after the visible rows so it
    // moves up with the collapsed layout.
    let tip_y = content_top + cell_h * (0.5 + visible.len() as f32 * 1.6);
    add_string_verts(
        &nexterm_i18n::fl!("settings-blocks-tip"),
        content_inner_x,
        tip_y,
        ensure_readable(tokens.text_muted, tokens.surface_2, MIN_TEXT_CONTRAST),
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
