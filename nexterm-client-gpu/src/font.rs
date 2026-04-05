//! フォント管理 — cosmic-text によるグリフレンダリング

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

/// フォントシステムのラッパー
pub struct FontManager {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub metrics: Metrics,
    /// 実計測による1セルの幅（物理ピクセル）
    cell_w: f32,
    /// 設定されたフォントファミリー名（Attrs に渡す）
    family: String,
}

impl FontManager {
    /// 指定フォント設定でフォントマネージャーを作成する
    ///
    /// `family` はプライマリフォントファミリー名。
    /// `fallbacks` はグリフが見つからない場合に順番に試行するフォントファミリーリスト。
    /// `scale_factor` は winit の window.scale_factor()（DPI スケール係数）。
    pub fn new(family: &str, size_pt: f32, fallbacks: &[String], scale_factor: f32) -> Self {
        let mut font_system = FontSystem::new();

        // プライマリフォントを monospace ジェネリックとして登録する。
        // Attrs::new().family(Family::Monospace) で参照される。
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

        // 基準文字 '0' の advance width（アドバンス幅）を layout_runs() で計測する。
        // Attrs::new() のデフォルトは Family::SansSerif なので Family::Monospace を明示する。
        // "monospace" 以外の名前が指定された場合は Family::Name で直接指定する。
        let cell_w = Self::measure_char_width(&mut font_system, &mut swash_cache, metrics, family);

        tracing::debug!(
            "フォント初期化: family={} {}pt × scale={} → {}px, cell_w={:.1}px, cell_h={:.1}px",
            family,
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
            family: family.to_string(),
        }
    }

    /// ASCII 基準文字 '0' のアドバンス幅を計測する
    ///
    /// `Buffer::draw()` はインクピクセルのみを提供するため advance width と一致しない。
    /// `layout_runs()` の `glyph.x + glyph.w` を使って正確なセル幅を取得する。
    fn measure_char_width(
        font_system: &mut FontSystem,
        _swash_cache: &mut SwashCache,
        metrics: Metrics,
        family: &str,
    ) -> f32 {
        let mut buf = Buffer::new(font_system, metrics);
        // "monospace" ジェネリック名の場合は Family::Monospace、
        // 具体的なフォント名の場合は Family::Name で直接指定する。
        // どちらも SansSerif フォールバックを防いで正確なセル幅を計測できる。
        let family_owned;
        let attrs = if family.eq_ignore_ascii_case("monospace") || family.is_empty() {
            Attrs::new().family(Family::Monospace)
        } else {
            family_owned = family.to_string();
            Attrs::new().family(Family::Name(&family_owned))
        };
        buf.set_text(font_system, "0", attrs, Shaping::Advanced);
        buf.set_size(
            font_system,
            Some(metrics.font_size * 4.0),
            Some(metrics.line_height),
        );
        buf.shape_until_scroll(font_system, false);

        // layout_runs() のグリフ hitbox（x + w）からアドバンス幅を取得する
        // これはフォントの正確な advance width（left bearing + ink + right bearing）
        let mut advance = 0.0f32;
        for run in buf.layout_runs() {
            for glyph in run.glyphs.iter() {
                advance = advance.max(glyph.x + glyph.w);
            }
        }

        tracing::debug!(
            "measure_char_width: advance={:.2}px (font_size={:.2}px)",
            advance,
            metrics.font_size
        );

        // 計測失敗の場合は line_height * 0.5 を安全策として返す
        if advance > 1.0 {
            advance
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

        // "monospace" ジェネリック名の場合は Family::Monospace、
        // 具体的なフォント名の場合は Family::Name で直接指定する（SansSerif フォールバックを防ぐ）
        let family_owned;
        let base_attrs = if self.family.eq_ignore_ascii_case("monospace") || self.family.is_empty()
        {
            Attrs::new().family(Family::Monospace)
        } else {
            family_owned = self.family.clone();
            Attrs::new().family(Family::Name(&family_owned))
        };
        let attrs = base_attrs
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

    /// 1セルの幅（物理ピクセル）を返す — advance width の実計測値
    pub fn cell_width(&self) -> f32 {
        self.cell_w
    }

    /// 1セルの高さ（物理ピクセル）を返す
    pub fn cell_height(&self) -> f32 {
        self.metrics.line_height
    }

    /// 設定されているフォントファミリー名を返す
    pub fn family(&self) -> &str {
        &self.family
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

    #[test]
    fn advance_width_が_ink_width_以上であること() {
        // layout_runs() 計測は ink width と等しいか大きい（right bearing を含む）
        let fm = FontManager::new("monospace", 14.0, &[], 1.0);
        // monospace フォントなのですべての文字が同じ幅のはず
        let cell_w = fm.cell_width();
        assert!(cell_w > 0.0, "cell_w should be positive: {}", cell_w);
        // 14pt @ 96dpi = 18.67px, advance ≈ 0.6 × font_size ≈ 11px
        assert!(cell_w > 5.0, "cell_w should be > 5px: {}", cell_w);
        assert!(cell_w < 40.0, "cell_w should be < 40px: {}", cell_w);
    }
}
