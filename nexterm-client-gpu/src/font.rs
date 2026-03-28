//! フォント管理 — cosmic-text によるグリフレンダリング

use cosmic_text::{Attrs, Buffer, Color, FontSystem, Metrics, Shaping, SwashCache};

/// フォントシステムのラッパー
pub struct FontManager {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub metrics: Metrics,
}

impl FontManager {
    /// 指定フォント設定でフォントマネージャーを作成する
    ///
    /// `family` はプライマリフォントファミリー名。
    /// `fallbacks` はグリフが見つからない場合に順番に試行するフォントファミリーリスト。
    /// `cosmic-text` の `FontSystem` はシステムフォントをすべてロード済みのため、
    /// `fallbacks` に列挙されたフォントがシステムにインストールされていれば
    /// 自動的にフォールバック候補として使用される。
    pub fn new(family: &str, size_pt: f32, fallbacks: &[String]) -> Self {
        let mut font_system = FontSystem::new();

        // プライマリフォントを monospace エイリアスとして登録する
        // （Family::Monospace を使用する場合のデフォルト解決先になる）
        font_system.db_mut().set_monospace_family(family);

        if !fallbacks.is_empty() {
            tracing::debug!(
                "フォントフォールバックチェーン: {} -> {}",
                family,
                fallbacks.join(" -> ")
            );
        }

        // セル高さ = size_pt * (4/3) px（72dpi 基準の概算）
        let line_height = size_pt * 4.0 / 3.0 * 1.2;
        let metrics = Metrics::new(size_pt * 4.0 / 3.0, line_height);

        Self {
            font_system,
            swash_cache: SwashCache::new(),
            metrics,
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
        buffer.set_size(&mut self.font_system, Some(self.metrics.font_size * 2.0), Some(self.metrics.line_height));
        buffer.shape_until_scroll(&mut self.font_system, false);

        // セルサイズを計算する
        let cell_w = (self.metrics.font_size * 0.6).ceil() as u32;
        let cell_h = self.metrics.line_height.ceil() as u32;
        let mut pixels = vec![0u8; (cell_w * cell_h * 4) as usize];

        let color = Color::rgba(fg[0], fg[1], fg[2], fg[3]);

        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            color,
            |x, y, w, h, c| {
                // バッファ範囲内にクリップして書き込む
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

    /// 1セルの幅（ピクセル）を返す
    pub fn cell_width(&self) -> f32 {
        self.metrics.font_size * 0.6
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
        let fm = FontManager::new("monospace", 14.0, &[]);
        assert!(fm.cell_width() > 0.0);
        assert!(fm.cell_height() > 0.0);
    }

    #[test]
    fn セルサイズが正の値を持つ() {
        let fm = FontManager::new("monospace", 16.0, &[]);
        // 16pt → cell_w ≈ 9.6px, cell_h ≈ 25.6px
        assert!(fm.cell_width() > 5.0);
        assert!(fm.cell_height() > 10.0);
    }

    #[test]
    fn フォールバックチェーン付きで生成できる() {
        let fallbacks = vec!["Noto Sans CJK JP".to_string(), "Noto Color Emoji".to_string()];
        let fm = FontManager::new("JetBrains Mono", 14.0, &fallbacks);
        assert!(fm.cell_width() > 0.0);
        assert!(fm.cell_height() > 0.0);
    }
}
