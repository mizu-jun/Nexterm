//! フォント管理 — cosmic-text によるグリフレンダリング

use cosmic_text::{Attrs, Buffer, Color, FontSystem, Metrics, Shaping, SwashCache};

/// フォントシステムのラッパー
pub struct FontManager {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub metrics: Metrics,
    /// 実計測による1セルの幅（ピクセル）
    cell_w: f32,
}

impl FontManager {
    /// 指定フォント設定でフォントマネージャーを作成する
    ///
    /// `family` はプライマリフォントファミリー名。
    /// `fallbacks` はグリフが見つからない場合に順番に試行するフォントファミリーリスト。
    /// `scale_factor` は winit の window.scale_factor()（DPI スケール係数）。
    pub fn new(family: &str, size_pt: f32, fallbacks: &[String], scale_factor: f32) -> Self {
        let mut font_system = FontSystem::new();

        // プライマリフォントを monospace エイリアスとして登録する
        font_system.db_mut().set_monospace_family(family);

        if !fallbacks.is_empty() {
            tracing::debug!(
                "フォントフォールバックチェーン: {} -> {}",
                family,
                fallbacks.join(" -> ")
            );
        }

        // size_pt × (96dpi/72dpi) × scale_factor = 物理ピクセルでのフォントサイズ
        // Windows 標準は 96 DPI、scale_factor がディスプレイのスケーリングを表す
        let font_size_px = size_pt * (96.0 / 72.0) * scale_factor;
        let line_height = font_size_px * 1.2;
        let metrics = Metrics::new(font_size_px, line_height);

        let mut swash_cache = SwashCache::new();

        // ASCII 基準文字 '0' を実際に描画してセル幅を計測する（0.6 係数廃止）
        let cell_w = Self::measure_char_width(&mut font_system, &mut swash_cache, metrics);

        tracing::debug!(
            "フォント初期化: {}pt × scale={} → {}px, cell_w={}px, cell_h={}px",
            size_pt,
            scale_factor,
            font_size_px,
            cell_w,
            line_height
        );

        Self {
            font_system,
            swash_cache,
            metrics,
            cell_w,
        }
    }

    /// ASCII 基準文字 '0' をラスタライズして実際の advance width を計測する
    fn measure_char_width(
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        metrics: Metrics,
    ) -> f32 {
        let mut buf = Buffer::new(font_system, metrics);
        buf.set_text(font_system, "0", Attrs::new(), Shaping::Advanced);
        buf.set_size(
            font_system,
            Some(metrics.font_size * 4.0),
            Some(metrics.line_height),
        );
        buf.shape_until_scroll(font_system, false);

        // 描画された最大 x + w をアドバンス幅とする
        let mut max_x: i32 = 0;
        buf.draw(
            font_system,
            swash_cache,
            Color::rgba(255, 255, 255, 255),
            |x, _y, w, _h, _c| {
                max_x = max_x.max(x + w as i32);
            },
        );

        // 計測失敗の場合は line_height * 0.5 を安全策として返す
        let measured = max_x as f32;
        if measured > 1.0 {
            measured
        } else {
            metrics.line_height * 0.5
        }
    }

    /// 1文字のグリフをラスタライズして RGBA ピクセル列を返す
    ///
    /// 戻り値: `(width, height, rgba_pixels)`
    pub fn rasterize_char(
        &mut self,
        ch: char,
        bold: bool,
        italic: bool,
        fg: [u8; 4],
    ) -> (u32, u32, Vec<u8>) {
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);

        let attrs = Attrs::new()
            .weight(if bold {
                cosmic_text::Weight::BOLD
            } else {
                cosmic_text::Weight::NORMAL
            })
            .style(if italic {
                cosmic_text::Style::Italic
            } else {
                cosmic_text::Style::Normal
            });

        let text = ch.to_string();
        buffer.set_text(&mut self.font_system, &text, attrs, Shaping::Advanced);
        buffer.set_size(
            &mut self.font_system,
            Some(self.metrics.font_size * 4.0),
            Some(self.metrics.line_height),
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        // セルサイズは実計測値を使う
        let cell_w = self.cell_w.ceil() as u32;
        let cell_h = self.metrics.line_height.ceil() as u32;
        let mut pixels = vec![0u8; (cell_w * cell_h * 4) as usize];

        let color = Color::rgba(fg[0], fg[1], fg[2], fg[3]);

        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            color,
            |x, y, _w, _h, c| {
                // 負の座標（left bearing 等）はスキップ
                if x < 0 || y < 0 {
                    return;
                }
                let px = x as u32;
                let py = y as u32;
                if px < cell_w && py < cell_h {
                    let idx = ((py * cell_w + px) * 4) as usize;
                    if idx + 3 < pixels.len() {
                        // アルファブレンド（単純上書き）
                        pixels[idx] = (c.r() as u32 * c.a() as u32 / 255) as u8;
                        pixels[idx + 1] = (c.g() as u32 * c.a() as u32 / 255) as u8;
                        pixels[idx + 2] = (c.b() as u32 * c.a() as u32 / 255) as u8;
                        pixels[idx + 3] = c.a();
                    }
                }
            },
        );

        (cell_w, cell_h, pixels)
    }

    /// 1セルの幅（ピクセル）を返す — 実計測値
    pub fn cell_width(&self) -> f32 {
        self.cell_w
    }

    /// 1セルの高さ（ピクセル）を返す
    pub fn cell_height(&self) -> f32 {
        self.metrics.line_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn フォントマネージャーが生成できる() {
        let fm = FontManager::new("monospace", 14.0, &[], 1.0);
        assert!(fm.cell_width() > 0.0);
        assert!(fm.cell_height() > 0.0);
    }

    #[test]
    fn セルサイズが正の値を持つ() {
        let fm = FontManager::new("monospace", 16.0, &[], 1.0);
        assert!(fm.cell_width() > 5.0);
        assert!(fm.cell_height() > 10.0);
    }

    #[test]
    fn フォールバックチェーン付きで生成できる() {
        let fallbacks = vec![
            "Noto Sans CJK JP".to_string(),
            "Noto Color Emoji".to_string(),
        ];
        let fm = FontManager::new("JetBrains Mono", 14.0, &fallbacks, 1.0);
        assert!(fm.cell_width() > 0.0);
        assert!(fm.cell_height() > 0.0);
    }

    #[test]
    fn scale_factor_1_25でセル幅が大きくなる() {
        let fm1 = FontManager::new("monospace", 14.0, &[], 1.0);
        let fm125 = FontManager::new("monospace", 14.0, &[], 1.25);
        assert!(fm125.cell_width() > fm1.cell_width());
        assert!(fm125.cell_height() > fm1.cell_height());
    }
}
