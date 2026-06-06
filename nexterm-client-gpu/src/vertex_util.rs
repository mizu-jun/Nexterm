//! Vertex-buffer helpers — utilities for generating rect/text/image vertices.

use tracing::info;
use unicode_width::UnicodeWidthChar;

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, GlyphKey, TextVertex};

/// Return the display width of a string in cells (CJK full-width characters count as 2).
pub(crate) fn visual_width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(1)).sum()
}

/// Push four background vertices for the NDC rectangle (and the corresponding triangle indices).
pub(crate) fn add_rect_verts(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: [f32; 4],
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    let base = bg_verts.len() as u16;
    bg_verts.extend_from_slice(&[
        BgVertex {
            position: [x0, y0],
            color,
        },
        BgVertex {
            position: [x1, y0],
            color,
        },
        BgVertex {
            position: [x1, y1],
            color,
        },
        BgVertex {
            position: [x0, y1],
            color,
        },
    ]);
    bg_idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Convert a pixel rectangle into NDC and push it onto the background vertex buffer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_px_rect(
    px: f32,
    py: f32,
    pw: f32,
    ph: f32,
    color: [f32; 4],
    sw: f32,
    sh: f32,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    let x0 = px / sw * 2.0 - 1.0;
    let y0 = 1.0 - py / sh * 2.0;
    let x1 = (px + pw) / sw * 2.0 - 1.0;
    let y1 = 1.0 - (py + ph) / sh * 2.0;
    add_rect_verts(x0, y0, x1, y1, color, bg_verts, bg_idx);
}

/// Draw a pixel-space rectangle with simulated rounded corners via a three-rect cross.
///
/// Approximates corner cutoffs at `radius` pixels without any shader changes.
/// Accurate enough at ≤ 8 px radius for dialog/overlay chrome.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_rounded_px_rect(
    px: f32,
    py: f32,
    pw: f32,
    ph: f32,
    color: [f32; 4],
    radius: f32,
    sw: f32,
    sh: f32,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    let r = radius.min(pw * 0.5).min(ph * 0.5);
    // Center vertical band — full height minus the two corner caps.
    add_px_rect(px, py + r, pw, ph - 2.0 * r, color, sw, sh, bg_verts, bg_idx);
    if r > 0.0 {
        // Top horizontal strip (inset by radius on both sides).
        add_px_rect(px + r, py, pw - 2.0 * r, r, color, sw, sh, bg_verts, bg_idx);
        // Bottom horizontal strip.
        add_px_rect(px + r, py + ph - r, pw - 2.0 * r, r, color, sw, sh, bg_verts, bg_idx);
    }
}

/// Append a cursor rectangle (per the configured cursor style) to the background vertex buffer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_cursor(
    style: &nexterm_config::CursorStyle,
    cx: f32,
    cy: f32,
    cell_w: f32,
    cell_h: f32,
    sw: f32,
    sh: f32,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    match style {
        nexterm_config::CursorStyle::Block => {
            add_px_rect(
                cx,
                cy,
                cell_w,
                cell_h,
                [1.0, 1.0, 1.0, 0.35],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }
        nexterm_config::CursorStyle::Beam => {
            // 2 px wide vertical bar.
            add_px_rect(
                cx,
                cy,
                2.0,
                cell_h,
                [1.0, 1.0, 1.0, 0.9],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }
        nexterm_config::CursorStyle::Underline => {
            // 2 px tall underline at the bottom of the cell.
            add_px_rect(
                cx,
                cy + cell_h - 2.0,
                cell_w,
                2.0,
                [1.0, 1.0, 1.0, 0.9],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }
    }
}

/// Append a single character to the text vertex buffer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_char_verts(
    ch: char,
    px: f32,
    py: f32,
    fg: [f32; 4],
    bold: bool,
    is_wide: bool,
    sw: f32,
    sh: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    if ch == ' ' {
        return;
    }
    // Set the wide-character flag correctly so the glyph atlas cache key matches.
    let key = GlyphKey {
        ch,
        bold,
        italic: false,
        wide: is_wide,
    };
    let fg_u8 = [
        (fg[0] * 255.0) as u8,
        (fg[1] * 255.0) as u8,
        (fg[2] * 255.0) as u8,
        255u8,
    ];
    let (gw, gh, pixels) = font.rasterize_char(ch, bold, false, fg_u8, is_wide);
    if gw == 0 || gh == 0 || pixels.is_empty() {
        return;
    }
    let rect = atlas.get_or_insert(key, &pixels, gw, gh, queue);
    let tx0 = px / sw * 2.0 - 1.0;
    let ty0 = 1.0 - py / sh * 2.0;
    let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
    let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
    let base = text_verts.len() as u16;
    text_verts.extend_from_slice(&[
        TextVertex {
            position: [tx0, ty0],
            uv: rect.uv_min,
            color: fg,
        },
        TextVertex {
            position: [tx1, ty0],
            uv: [rect.uv_max[0], rect.uv_min[1]],
            color: fg,
        },
        TextVertex {
            position: [tx1, ty1],
            uv: rect.uv_max,
            color: fg,
        },
        TextVertex {
            position: [tx0, ty1],
            uv: [rect.uv_min[0], rect.uv_max[1]],
            color: fg,
        },
    ]);
    text_idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Append a string to the text vertex buffer.
///
/// Each glyph is placed at the correct pixel position taking the Unicode column
/// width (full-width = 2, half-width = 1) into account. CJK full-width characters
/// (Japanese / Chinese / Korean) are rendered correctly.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_string_verts(
    text: &str,
    px: f32,
    py: f32,
    fg: [f32; 4],
    bold: bool,
    sw: f32,
    sh: f32,
    cell_w: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    let mut x_offset = 0.0f32;
    for ch in text.chars() {
        // Use the Unicode column width (full-width = 2, half-width = 1) for advance.
        let char_display_width = UnicodeWidthChar::width(ch).unwrap_or(1);
        let is_wide = char_display_width >= 2;
        add_char_verts(
            ch,
            px + x_offset,
            py,
            fg,
            bold,
            is_wide,
            sw,
            sh,
            font,
            atlas,
            queue,
            text_verts,
            text_idx,
        );
        x_offset += char_display_width as f32 * cell_w;
    }
}

/// Open a URL in the default browser (cross-platform).
pub(crate) fn open_url(url: &str) {
    info!("Opening URL: {}", url);
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// Convert a pane's grid contents into plain text (used by Ctrl+Shift+C copy).
pub(crate) fn grid_to_text(pane: &crate::state::PaneState) -> String {
    let mut lines = Vec::with_capacity(pane.grid.rows.len());
    for row in &pane.grid.rows {
        let line: String = row.iter().map(|c| c.ch).collect();
        // Strip trailing spaces from each row.
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}
