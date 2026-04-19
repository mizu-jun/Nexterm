//! 頂点バッファヘルパー — 矩形・テキスト・画像の頂点生成ユーティリティ

use tracing::info;
use unicode_width::UnicodeWidthChar;

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, GlyphKey, TextVertex};

/// 文字列の表示幅をセル単位で返す（CJK 全角文字は 2 として計算）
pub(crate) fn visual_width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(1)).sum()
}

/// NDC 矩形の背景頂点4つを追加する（三角形インデックスも追加）
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

/// ピクセル矩形を NDC に変換して背景頂点バッファに追加する
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

/// 1文字をテキスト頂点バッファに追加する
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
    // 全角文字フラグを正しく設定してグリフアトラスのキャッシュキーを一致させる
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

/// 文字列をテキスト頂点バッファに追加する
///
/// Unicode 文字幅（全角=2, 半角=1）を考慮して正しいピクセル位置に各グリフを配置する。
/// 日本語・中国語・韓国語などの全角文字も正しく描画される。
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
        // Unicode 幅（全角=2, 半角=1）を取得して文字送り量を決定する
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

/// URL をデフォルトブラウザで開く（プラットフォーム対応）
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

/// ペインのグリッド内容をプレーンテキストに変換する（Ctrl+Shift+C コピー用）
pub(crate) fn grid_to_text(pane: &crate::state::PaneState) -> String {
    let mut lines = Vec::with_capacity(pane.grid.rows.len());
    for row in &pane.grid.rows {
        let line: String = row.iter().map(|c| c.ch).collect();
        // 行末の空白を除去して返す
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}
