//! Vertex builder for the key-hint overlay (Sprint 5-7 / UI-1-4).
//!
//! Equivalent to WezTerm's `show_active_key_table`. For two seconds after a
//! lone Leader press, a semi-transparent overlay at the bottom of the screen
//! shows the list of `<leader> ...` or `ctrl+b ...` style bindings.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_run_verts, measure_run};

use super::super::WgpuState;
use nexterm_config::SurfaceLevel;

/// Banner size from the measured columns (UI/UX v3 N-7b).
///
/// Pure so the box can be tested without a GPU, in the shape `place_links`
/// and `place_tooltip` established. `content_w` is the key column plus the
/// widest action; the banner is as wide as whichever of that and the header
/// needs more, and never wider than the window.
fn banner_size(
    header_w: f32,
    content_w: f32,
    rows: usize,
    header_h: f32,
    row_h: f32,
    pad: f32,
    sw: f32,
) -> (f32, f32) {
    let w = (header_w.max(content_w) + pad * 2.0).min(sw);
    let h = header_h + rows as f32 * row_h + pad * 2.0;
    (w, h)
}

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
        // `cell_w` survives as the DPI-scaled unit the banner's padding is
        // expressed in — it is spacing, not a text measurement. The terminal
        // row height used to be a parameter too; N-7b took it away, because
        // row pitch now comes from the ramp's line box.
        cell_w: f32,
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

        // UI/UX v3 N-7b: the banner is chrome, so every width in it is
        // measured. It used to align its two columns with a left-pad format
        // spec eight characters wide and a fixed ten-cell jump to the second
        // column — padding that only lines up in a monospace font, at a size
        // this banner is not drawn at. Action names come from `config.keys`,
        // so they are arbitrary user text.
        let ramp = nexterm_config::MetricTokens::default().type_ramp;
        let (header_style, key_style, action_style) = (ramp.body_strong, ramp.body, ramp.body);
        let (_, header_h, _) = font.chrome_metrics(&header_style);
        let (_, row_h, _) = font.chrome_metrics(&key_style);

        let header = nexterm_i18n::fl!("key-hint-header").replace("{leader}", leader);

        // The key column is as wide as its widest key, plus one space of
        // gutter before the arrow.
        let gutter = measure_run("  ", &key_style, font);
        let key_col_w = hints
            .iter()
            .map(|(key, _)| measure_run(key, &key_style, font))
            .fold(0.0f32, f32::max)
            + gutter;
        let actions: Vec<String> = hints
            .iter()
            .map(|(_, action)| format!("→ {action}"))
            .collect();
        let action_w = actions
            .iter()
            .map(|action| measure_run(action, &action_style, font))
            .fold(0.0f32, f32::max);
        let header_w = measure_run(&header, &header_style, font);

        let pad = cell_w * 0.6;
        let (banner_w, banner_h) = banner_size(
            header_w,
            key_col_w + action_w,
            hints.len(),
            header_h,
            row_h,
            pad,
            sw,
        );
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
        add_run_verts(
            &header,
            &header_style,
            bx + pad,
            by + pad,
            tokens.text_on(SurfaceLevel::S0).primary,
            sw,
            sh,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Each entry. The indent is the `pad` offset, not a leading space
        // inside the string — a space is text, and text that exists only to
        // push other text over stops working the moment it is measured.
        let key_fg = tokens.text_on(SurfaceLevel::S0).primary;
        let action_fg = tokens.text_on(SurfaceLevel::S0).secondary;
        let key_x = bx + pad;
        for (i, ((key, _), action)) in hints.iter().zip(&actions).enumerate() {
            let row_y = by + pad + header_h + (i as f32 * row_h);
            add_run_verts(
                key,
                &key_style,
                key_x,
                row_y,
                key_fg,
                sw,
                sh,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
            add_run_verts(
                action,
                &action_style,
                key_x + key_col_w,
                row_y,
                action_fg,
                sw,
                sh,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::banner_size;

    /// The banner takes its width from whichever of the header and the two
    /// content columns is wider — the header used to be free to overflow a
    /// box sized at a flat `80 * cell_w`.
    #[test]
    fn the_wider_of_header_and_content_sets_the_width() {
        let (wide_header, _) = banner_size(400.0, 100.0, 3, 20.0, 18.0, 6.0, 1000.0);
        assert_eq!(wide_header, 412.0);
        let (wide_content, _) = banner_size(100.0, 400.0, 3, 20.0, 18.0, 6.0, 1000.0);
        assert_eq!(wide_content, 412.0);
    }

    /// A banner never grows past the window, however long an action name is.
    #[test]
    fn the_window_is_the_ceiling() {
        let (w, _) = banner_size(0.0, 5_000.0, 1, 20.0, 18.0, 6.0, 800.0);
        assert_eq!(w, 800.0);
    }

    /// Height is the header's line box plus one row's line box per entry —
    /// both from the ramp, neither from `cell_h`.
    #[test]
    fn height_follows_the_ramp_line_boxes() {
        let (_, h) = banner_size(0.0, 0.0, 4, 20.0, 18.0, 6.0, 1000.0);
        assert_eq!(h, 20.0 + 4.0 * 18.0 + 12.0);
    }

    /// An empty banner is never drawn (the builder returns early), but the
    /// size must still degrade to just the header rather than going negative.
    #[test]
    fn zero_rows_leaves_the_header_box() {
        let (_, h) = banner_size(0.0, 0.0, 0, 20.0, 18.0, 6.0, 1000.0);
        assert_eq!(h, 32.0);
    }

    /// UI/UX v3 N-7b: nothing in this banner is placed by counting cells.
    #[test]
    fn the_key_hint_banner_draws_no_text_on_the_cell_path() {
        let src = include_str!("key_hint.rs");
        let body = src
            .split("#[cfg(test)]")
            .next()
            .expect("the file has a body before its tests");
        assert!(
            body.contains("add_run_verts") && body.contains("measure_run"),
            "the gates below would pass vacuously"
        );
        assert!(
            !body.contains("add_string_verts"),
            "the key-hint banner draws text on the cell path again"
        );
        assert!(
            !body.contains("{:<"),
            "the key-hint banner pads a column with spaces again; character \
             padding only aligns in a monospace font, and this banner is not \
             drawn at the cell size"
        );
        // `cell_w` stays, as the DPI-scaled unit the padding is expressed in.
        // `cell_h` does not: row pitch is a line box, and a banner drawn at a
        // ramp size has no business stepping by the terminal's row height.
        assert!(
            !body.contains("cell_h"),
            "the key-hint banner steps its rows by the terminal cell height \
             again; the pitch comes from the ramp's line box"
        );
        assert!(
            !body.contains("Leader:"),
            "the key-hint header is an English literal again; it is `fl!`-backed"
        );
    }
}
