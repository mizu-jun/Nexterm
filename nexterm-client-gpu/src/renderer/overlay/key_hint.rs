//! Vertex builder for the key-hint overlay (Sprint 5-7 / UI-1-4).
//!
//! Equivalent to WezTerm's `show_active_key_table`. For two seconds after a
//! lone Leader press, a semi-transparent overlay at the bottom of the screen
//! shows the list of `<leader> ...` or `ctrl+b ...` style bindings.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::super::WgpuState;

impl WgpuState {
    /// Build vertices for the key-hint overlay.
    ///
    /// Drawn when `state.key_hint_visible_until` is `Some(time)` and the time
    /// is still in the future. Draws a semi-transparent banner at the bottom
    /// of the screen and lists the trailing key + action name for each
    /// prefix-style binding from `config.keys` (entries starting with
    /// `leader_key` or containing `<leader>`).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_key_hint_verts(
        &self,
        state: &ClientState,
        cfg: &nexterm_config::Config,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // Visibility check: deadline is Some and still in the future
        let visible = match state.key_hint_visible_until {
            Some(t) => std::time::Instant::now() < t,
            None => false,
        };
        if !visible {
            return;
        }

        // Extract bindings to display:
        //   1. Starts with `<leader> ...` (uses leader explicitly)
        //   2. Starts with `<leader_key> ...` (e.g. `ctrl+b ...`) (tmux-compatible legacy form)
        let leader = &cfg.leader_key;
        let mut hints: Vec<(String, String)> = Vec::new();
        for binding in &cfg.keys {
            let key = &binding.key;
            // Only space-separated entries with 2+ tokens (prefix-style)
            let tokens: Vec<&str> = key.split_whitespace().collect();
            if tokens.len() < 2 {
                continue;
            }
            let first = tokens[0];
            let is_leader_prefix =
                first == "<leader>" || first.eq_ignore_ascii_case(leader.as_str());
            if !is_leader_prefix {
                continue;
            }
            // Trailing key (join everything past the first token)
            let rest = tokens[1..].join(" ");
            hints.push((rest, binding.action.clone()));
        }
        // Deduplicate and cap at 12 entries for display
        hints.dedup_by(|a, b| a.0 == b.0);
        hints.truncate(12);

        if hints.is_empty() {
            return;
        }

        // Banner height: header row + one row per entry
        let lines = (hints.len() as f32) + 1.0;
        let pad = cell_w * 0.6;
        let banner_h = lines * cell_h + pad * 2.0;
        let banner_w = sw.min(80.0 * cell_w);
        let bx = (sw - banner_w) / 2.0;
        let by = sh - banner_h - pad;

        // Background (semi-transparent dark navy derived from surface_0)
        let bg_color = {
            let [r, g, b, _] = tokens.surface_0;
            [r, g, b, 0.92]
        };
        add_px_rect(
            bx, by, banner_w, banner_h, bg_color, sw, sh, bg_verts, bg_idx,
        );
        // Top accent line
        add_px_rect(
            bx,
            by,
            banner_w,
            2.0,
            tokens.accent_muted,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Header
        let header = format!(" Leader: {} — press the next key ", leader);
        add_string_verts(
            &header,
            bx + pad,
            by + pad,
            tokens.text_primary,
            true,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Each entry
        let key_fg = tokens.text_primary;
        let action_fg = tokens.text_secondary;
        for (i, (key, action)) in hints.iter().enumerate() {
            let row_y = by + pad + cell_h + (i as f32 * cell_h);
            let key_x = bx + pad;
            add_string_verts(
                &format!(" {:<8}", key),
                key_x,
                row_y,
                key_fg,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
            add_string_verts(
                &format!(" → {}", action),
                key_x + 10.0 * cell_w,
                row_y,
                action_fg,
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }
}
