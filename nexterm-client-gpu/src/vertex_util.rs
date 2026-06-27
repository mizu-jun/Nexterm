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
///
/// Flat-rect path: SDF fields are zeroed, so the bg shader takes its
/// `corner_radius <= 0` early-return and produces output identical to the
/// pre-v2 (pre-Sprint-5-15) renderer.
pub(crate) fn add_rect_verts(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: [f32; 4],
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    push_rect_verts_with_sdf(
        x0,
        y0,
        x1,
        y1,
        color,
        [0.0, 0.0],
        [0.0, 0.0],
        0.0,
        bg_verts,
        bg_idx,
    );
}

/// Inner helper that fills every `BgVertex` field. Used by both the flat
/// [`add_rect_verts`] and the rounded [`add_px_rounded_rect_sdf`].
#[allow(clippy::too_many_arguments)]
fn push_rect_verts_with_sdf(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: [f32; 4],
    rect_center: [f32; 2],
    rect_half_size: [f32; 2],
    corner_radius: f32,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    let base = bg_verts.len() as u16;
    let make = |position| BgVertex {
        position,
        color,
        rect_center,
        rect_half_size,
        corner_radius,
    };
    bg_verts.extend_from_slice(&[
        make([x0, y0]),
        make([x1, y0]),
        make([x1, y1]),
        make([x0, y1]),
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

/// Pixel-space rounded rectangle drawn via the SDF path of the bg shader
/// (Sprint 5-15 / UI/UX Modernization v2 Phase 1).
///
/// Produces sub-pixel-AA rounded corners with a single drawcall. Prefer this
/// over the legacy [`add_rounded_px_rect`] (a CPU-side three-rect cross that
/// leaves square holes at the corners) whenever the result is visible chrome.
/// Passing `radius == 0.0` falls through to a flat rect, matching
/// [`add_px_rect`] byte-for-byte.
///
/// Initially unused — Phase 2 (tab pills) and later phases will be the first
/// callers. The `dead_code` suppression keeps the Phase 1 landing warning-free.
#[allow(clippy::too_many_arguments, dead_code)]
pub(crate) fn add_px_rounded_rect_sdf(
    px: f32,
    py: f32,
    pw: f32,
    ph: f32,
    radius: f32,
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
    // Clamp the radius to half the shortest side. A negative radius collapses
    // to zero so the shader takes the flat path instead of producing garbage.
    let r = radius.max(0.0).min(pw * 0.5).min(ph * 0.5);
    let rect_center = [px + pw * 0.5, py + ph * 0.5];
    let rect_half_size = [pw * 0.5, ph * 0.5];
    push_rect_verts_with_sdf(
        x0,
        y0,
        x1,
        y1,
        color,
        rect_center,
        rect_half_size,
        r,
        bg_verts,
        bg_idx,
    );
}

/// Signed distance from `point` to a rounded rectangle (in pixels).
///
/// Pure helper mirroring the WGSL `fs_main` math in
/// [`crate::shaders::BG_SHADER`]; lets us unit-test the SDF formula without
/// a GPU. Negative inside, zero on the edge, positive outside.
#[allow(dead_code)]
pub(crate) fn signed_rect_distance(
    point: [f32; 2],
    rect_center: [f32; 2],
    rect_half_size: [f32; 2],
    corner_radius: f32,
) -> f32 {
    let dx = (point[0] - rect_center[0]).abs() - rect_half_size[0] + corner_radius;
    let dy = (point[1] - rect_center[1]).abs() - rect_half_size[1] + corner_radius;
    let outside_len = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside_d = dx.max(dy).min(0.0);
    outside_len + inside_d - corner_radius
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
    add_px_rect(
        px,
        py + r,
        pw,
        ph - 2.0 * r,
        color,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    if r > 0.0 {
        // Top horizontal strip (inset by radius on both sides).
        add_px_rect(px + r, py, pw - 2.0 * r, r, color, sw, sh, bg_verts, bg_idx);
        // Bottom horizontal strip.
        add_px_rect(
            px + r,
            py + ph - r,
            pw - 2.0 * r,
            r,
            color,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    // ---- signed_rect_distance ----

    #[test]
    fn sdf_center_is_negative_min_half_size() {
        // A point at the rect centre is `half_size_min` units inside the edge
        // (for a square, exactly `-half_size`).
        let d = signed_rect_distance([0.0, 0.0], [0.0, 0.0], [10.0, 10.0], 4.0);
        assert!(approx(d, -10.0), "centre distance was {}", d);
    }

    #[test]
    fn sdf_zero_on_rounded_corner_arc() {
        // The rounded corner arc sits at radius `r` from the inset corner
        // centre `(half_size - r, half_size - r)`. Pick a 45° point on that
        // arc; the SDF must report distance 0.
        let half = 10.0;
        let r = 4.0;
        let arc_centre = half - r; // 6.0
        // 45° on the arc: arc_centre + r * cos(45°)
        let p = arc_centre + r * std::f32::consts::FRAC_1_SQRT_2;
        let d = signed_rect_distance([p, p], [0.0, 0.0], [half, half], r);
        assert!(approx(d, 0.0), "arc point distance was {}", d);
    }

    #[test]
    fn sdf_positive_outside() {
        // A point well outside the rect.
        let d = signed_rect_distance([15.0, 15.0], [0.0, 0.0], [10.0, 10.0], 4.0);
        // Expected: sqrt((15-10+4)^2 + (15-10+4)^2) - 4 = sqrt(162) - 4
        let expected = (162.0_f32).sqrt() - 4.0;
        assert!(approx(d, expected), "got {}, expected {}", d, expected);
    }

    #[test]
    fn sdf_zero_on_straight_edge() {
        // Mid-edge point (no corner influence). For a rect at origin with
        // half_size=10, the point (10, 0) sits exactly on the right edge.
        let d = signed_rect_distance([10.0, 0.0], [0.0, 0.0], [10.0, 10.0], 4.0);
        assert!(approx(d, 0.0), "edge distance was {}", d);
    }

    #[test]
    fn sdf_zero_radius_is_axis_aligned_box() {
        // With r=0 the SDF degenerates into the axis-aligned box distance.
        let d = signed_rect_distance([12.0, 0.0], [0.0, 0.0], [10.0, 10.0], 0.0);
        assert!(approx(d, 2.0), "non-rounded box distance was {}", d);
    }

    // ---- add_rect_verts / add_px_rounded_rect_sdf ----

    #[test]
    fn flat_rect_zeroes_sdf_fields() {
        // Legacy `add_rect_verts` must produce vertices with all SDF fields at
        // zero so the shader takes its flat-path early-return.
        let mut v = Vec::new();
        let mut i = Vec::new();
        add_rect_verts(-0.5, 0.5, 0.5, -0.5, [1.0, 0.0, 0.0, 1.0], &mut v, &mut i);
        assert_eq!(v.len(), 4);
        for vert in &v {
            assert_eq!(vert.rect_center, [0.0, 0.0]);
            assert_eq!(vert.rect_half_size, [0.0, 0.0]);
            assert_eq!(vert.corner_radius, 0.0);
        }
        // Index triangulation is unchanged.
        assert_eq!(i, vec![0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn rounded_helper_populates_pixel_space_sdf_metadata() {
        // 800×600 screen, rect at (100, 50) with size 200×40, radius 8.
        let mut v = Vec::new();
        let mut i = Vec::new();
        add_px_rounded_rect_sdf(
            100.0,
            50.0,
            200.0,
            40.0,
            8.0,
            [0.1, 0.2, 0.3, 1.0],
            800.0,
            600.0,
            &mut v,
            &mut i,
        );
        assert_eq!(v.len(), 4);
        for vert in &v {
            assert_eq!(vert.rect_center, [200.0, 70.0]);
            assert_eq!(vert.rect_half_size, [100.0, 20.0]);
            assert_eq!(vert.corner_radius, 8.0);
        }
    }

    #[test]
    fn rounded_helper_clamps_radius_to_half_min_side() {
        // A 100×20 rect has min half-side 10. A requested radius of 50 must
        // be clamped to 10 to keep the SDF well-defined.
        let mut v = Vec::new();
        let mut i = Vec::new();
        add_px_rounded_rect_sdf(
            0.0, 0.0, 100.0, 20.0, 50.0, [1.0; 4], 800.0, 600.0, &mut v, &mut i,
        );
        assert_eq!(v.first().map(|x| x.corner_radius), Some(10.0));
    }

    #[test]
    fn rounded_helper_clamps_negative_radius_to_zero() {
        // A negative radius must collapse to zero so the shader takes the
        // flat path rather than producing garbage.
        let mut v = Vec::new();
        let mut i = Vec::new();
        add_px_rounded_rect_sdf(
            0.0, 0.0, 100.0, 20.0, -3.0, [1.0; 4], 800.0, 600.0, &mut v, &mut i,
        );
        assert_eq!(v.first().map(|x| x.corner_radius), Some(0.0));
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
