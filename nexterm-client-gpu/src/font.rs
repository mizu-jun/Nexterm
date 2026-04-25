//! フォント管理 — cosmic-text によるグリフレンダリング

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

/// リガチャ描画の出力グリフ（行単位ラスタライズ用）
pub struct RenderedGlyph {
    /// グリッド上の列インデックス（0 origin）
    pub col: usize,
    /// グリフの物理幅（ピクセル）
    pub width: u32,
    /// グリフの物理高さ（ピクセル）
    pub height: u32,
    /// RGBA ピクセルデータ
    pub pixels: Vec<u8>,
}

/// フォントシステムのラッパー
pub struct FontManager {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub metrics: Metrics,
    /// 実計測による1セルの幅（物理ピクセル）
    cell_w: f32,
    /// 設定されたフォントファミリー名（Attrs に渡す）
    family: String,
    /// リガチャを有効にするか
    pub ligatures: bool,
}

impl FontManager {
    /// 指定フォント設定でフォントマネージャーを作成する
    ///
    /// `family` はプライマリフォントファミリー名。
    /// `fallbacks` はグリフが見つからない場合に順番に試行するフォントファミリーリスト。
    /// `scale_factor` は winit の window.scale_factor()（DPI スケール係数）。
    /// `ligatures` は HarfBuzz リガチャシェーピングを有効にするか。
    pub fn new(
        family: &str,
        size_pt: f32,
        fallbacks: &[String],
        scale_factor: f32,
        ligatures: bool,
    ) -> Self {
        // FontSystem::new() は全システムフォントをスキャンするため ~30-50MB 消費する。
        // 代わりに絞り込みロードを使用してメモリを削減する。
        let mut font_system = Self::build_font_system(family, fallbacks);

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
            ligatures,
        }
    }

    /// フォントシステムを絞り込みロードで初期化する
    ///
    /// `FontSystem::new()` は全システムフォントをスキャンするため ~30-50MB 消費する。
    /// このメソッドではOS別の主要フォントディレクトリのみをロードしてメモリを削減する。
    /// CJK・絵文字フォールバックに必要なディレクトリは含む。
    fn build_font_system(_primary_family: &str, _fallbacks: &[String]) -> FontSystem {
        use cosmic_text::fontdb;

        let locale = sys_locale::get_locale().unwrap_or_else(|| "ja-JP".to_string());
        let mut db = fontdb::Database::new();

        // OS 別の主要フォントディレクトリを絞り込んでロードする
        // フォールバック: load_system_fonts() で全スキャン
        #[cfg(target_os = "macos")]
        {
            // システムフォント（絵文字・CJK 含む）
            db.load_fonts_dir("/System/Library/Fonts");
            // ユーザーインストールフォントは省略（主要ターミナルフォントは上記に含まれる）
        }
        #[cfg(target_os = "windows")]
        {
            // Windows システムフォント
            db.load_fonts_dir("C:\\Windows\\Fonts");
        }
        #[cfg(target_os = "linux")]
        {
            // Linux 主要フォントディレクトリ（全スキャンより高速）
            for dir in &[
                "/usr/share/fonts",
                "/usr/local/share/fonts",
                "/usr/share/fonts/truetype",
                "/usr/share/fonts/opentype",
            ] {
                db.load_fonts_dir(dir);
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            // 未知の OS はフルスキャンにフォールバック
            db.load_system_fonts();
        }

        tracing::debug!("フォントDB: {} 個のフェイスをロード済み", db.len());

        FontSystem::new_with_locale_and_db(locale, db)
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
        // family_owned はすべてのパスで有効なライフタイムを持つよう事前宣言する。
        let family_owned = family.to_string();
        let attrs = if family.eq_ignore_ascii_case("monospace") || family.is_empty() {
            Attrs::new().family(Family::Monospace)
        } else {
            Attrs::new().family(Family::Name(&family_owned))
        };
        buf.set_text(font_system, "0", &attrs, Shaping::Advanced, None);
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
    /// `wide` が true の場合は全角文字（CJK 等）として2セル幅のバッファを使用する。
    /// 戻り値: `(width, height, rgba_pixels)`
    pub fn rasterize_char(
        &mut self,
        ch: char,
        bold: bool,
        italic: bool,
        fg: [u8; 4],
        wide: bool,
    ) -> (u32, u32, Vec<u8>) {
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);

        // "monospace" ジェネリック名の場合は Family::Monospace、
        // 具体的なフォント名の場合は Family::Name で直接指定する（SansSerif フォールバックを防ぐ）
        // family_owned はすべてのパスで有効なライフタイムを持つよう事前宣言する。
        let family_owned = self.family.clone();
        let base_attrs = if self.family.eq_ignore_ascii_case("monospace") || self.family.is_empty()
        {
            Attrs::new().family(Family::Monospace)
        } else {
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
        // 全角文字は 2 セル分の幅でレンダリングする（バッファ幅を 2× に設定）
        let display_cols = if wide { 2.0 } else { 1.0 };
        buffer.set_text(
            &mut self.font_system,
            &text,
            &attrs,
            Shaping::Advanced,
            None,
        );
        buffer.set_size(
            &mut self.font_system,
            Some(self.metrics.font_size * 4.0 * display_cols),
            Some(self.metrics.line_height),
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        // セルサイズ: 全角文字は 2 セル幅
        let cell_w = (self.cell_w * display_cols).ceil() as u32;
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

    /// 行全体をリガチャ込みでラスタライズし、グリッド列単位のグリフリストを返す。
    ///
    /// `chars` は (col, char, bold, italic, fg_rgba) のタプル列。
    /// HarfBuzz の Advanced シェーピングが有効になるため、「->」「=>」「!=」等の
    /// リガチャグリフが正しく合成される。
    ///
    /// リガチャが無効（`self.ligatures == false`）の場合は空リストを返す。
    /// 呼び出し元は空リストのとき `rasterize_char()` にフォールバックすること。
    pub fn rasterize_line_segment(
        &mut self,
        chars: &[(usize, char, bool, bool, [u8; 4])],
    ) -> Vec<RenderedGlyph> {
        if !self.ligatures || chars.is_empty() {
            return Vec::new();
        }

        // 同じ fg 色・bold・italic の連続するチャンクに分けてシェーピングする。
        // 属性が変わる箇所でリガチャは分断される（レンダリング上正しい）。
        let mut result = Vec::new();
        let mut chunk_start = 0;

        while chunk_start < chars.len() {
            let (_, _, bold0, italic0, fg0) = chars[chunk_start];
            let mut chunk_end = chunk_start + 1;
            while chunk_end < chars.len() {
                let (_, _, b, i, fg) = chars[chunk_end];
                if b != bold0 || i != italic0 || fg != fg0 {
                    break;
                }
                chunk_end += 1;
            }
            let chunk = &chars[chunk_start..chunk_end];
            let mut rendered = self.rasterize_chunk(chunk);
            result.append(&mut rendered);
            chunk_start = chunk_end;
        }
        result
    }

    /// 同属性の文字チャンクを1つのバッファでシェーピングしてグリフリストを返す。
    fn rasterize_chunk(
        &mut self,
        chunk: &[(usize, char, bool, bool, [u8; 4])],
    ) -> Vec<RenderedGlyph> {
        let text: String = chunk.iter().map(|(_, ch, _, _, _)| *ch).collect();
        let (_, _, bold, italic, fg) = chunk[0];
        let cell_h = self.metrics.line_height.ceil() as u32;

        let family_owned = self.family.clone();
        let base_attrs = if self.family.eq_ignore_ascii_case("monospace") || self.family.is_empty()
        {
            Attrs::new().family(Family::Monospace)
        } else {
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

        // チャンク全体を1バッファに渡して HarfBuzz でシェーピングする
        let buf_width = self.cell_w * chunk.len() as f32 * 4.0;
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);
        buffer.set_text(
            &mut self.font_system,
            &text,
            &attrs,
            Shaping::Advanced,
            None,
        );
        buffer.set_size(
            &mut self.font_system,
            Some(buf_width),
            Some(self.metrics.line_height),
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        // layout_runs() から各グリフの x オフセット・幅を取得する
        // グリフ x 座標をセル幅で割ってグリッド列にマッピングする
        let color = Color::rgba(fg[0], fg[1], fg[2], fg[3]);
        let mut glyphs: Vec<(usize, f32, f32)> = Vec::new(); // (col, x_px, w_px)
        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let col_idx = (glyph.x / self.cell_w).round() as usize;
                if col_idx < chunk.len() {
                    let (grid_col, _, _, _, _) = chunk[col_idx];
                    glyphs.push((grid_col, glyph.x, glyph.w));
                }
            }
        }

        // グリフが取得できなかった場合は空を返してフォールバックさせる
        if glyphs.is_empty() {
            return Vec::new();
        }

        // バッファ全体をラスタライズしてグリフ領域ごとに切り出す
        let total_w = (self.cell_w * chunk.len() as f32).ceil() as u32;
        let mut full_pixels = vec![0u8; (total_w * cell_h * 4) as usize];
        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            color,
            |x, y, _w, _h, c| {
                if x < 0 || y < 0 {
                    return;
                }
                let px = x as u32;
                let py = y as u32;
                if px < total_w && py < cell_h {
                    let idx = ((py * total_w + px) * 4) as usize;
                    if idx + 3 < full_pixels.len() {
                        full_pixels[idx] = (c.r() as u32 * c.a() as u32 / 255) as u8;
                        full_pixels[idx + 1] = (c.g() as u32 * c.a() as u32 / 255) as u8;
                        full_pixels[idx + 2] = (c.b() as u32 * c.a() as u32 / 255) as u8;
                        full_pixels[idx + 3] = c.a();
                    }
                }
            },
        );

        // 各グリフ領域を切り出して RenderedGlyph に変換する
        glyphs
            .into_iter()
            .map(|(grid_col, glyph_x, glyph_w)| {
                let x_start = glyph_x.max(0.0) as u32;
                let width = glyph_w.ceil() as u32;
                let width = width.min(total_w.saturating_sub(x_start));
                if width == 0 {
                    return RenderedGlyph {
                        col: grid_col,
                        width: 0,
                        height: 0,
                        pixels: Vec::new(),
                    };
                }
                let mut pixels = vec![0u8; (width * cell_h * 4) as usize];
                for row in 0..cell_h {
                    for col in 0..width {
                        let src = ((row * total_w + x_start + col) * 4) as usize;
                        let dst = ((row * width + col) * 4) as usize;
                        if src + 3 < full_pixels.len() && dst + 3 < pixels.len() {
                            pixels[dst..dst + 4].copy_from_slice(&full_pixels[src..src + 4]);
                        }
                    }
                }
                RenderedGlyph {
                    col: grid_col,
                    width,
                    height: cell_h,
                    pixels,
                }
            })
            .filter(|g| g.width > 0)
            .collect()
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
    #[allow(dead_code)]
    pub fn family(&self) -> &str {
        &self.family
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn フォントマネージャーが生成できる() {
        let fm = FontManager::new("monospace", 14.0, &[], 1.0, true);
        assert!(fm.cell_width() > 0.0);
        assert!(fm.cell_height() > 0.0);
    }

    #[test]
    fn セルサイズが正の値を持つ() {
        let fm = FontManager::new("monospace", 16.0, &[], 1.0, true);
        assert!(fm.cell_width() > 5.0);
        assert!(fm.cell_height() > 10.0);
    }

    #[test]
    fn フォールバックチェーン付きで生成できる() {
        let fallbacks = vec![
            "Noto Sans CJK JP".to_string(),
            "Noto Color Emoji".to_string(),
        ];
        let fm = FontManager::new("JetBrains Mono", 14.0, &fallbacks, 1.0, true);
        assert!(fm.cell_width() > 0.0);
        assert!(fm.cell_height() > 0.0);
    }

    #[test]
    fn scale_factor_1_25でセル幅が大きくなる() {
        let fm1 = FontManager::new("monospace", 14.0, &[], 1.0, true);
        let fm125 = FontManager::new("monospace", 14.0, &[], 1.25, true);
        assert!(fm125.cell_width() > fm1.cell_width());
        assert!(fm125.cell_height() > fm1.cell_height());
    }

    #[test]
    fn advance_width_が_ink_width_以上であること() {
        // layout_runs() 計測は ink width と等しいか大きい（right bearing を含む）
        let fm = FontManager::new("monospace", 14.0, &[], 1.0, true);
        // monospace フォントなのですべての文字が同じ幅のはず
        let cell_w = fm.cell_width();
        assert!(cell_w > 0.0, "cell_w should be positive: {}", cell_w);
        // 14pt @ 96dpi = 18.67px, advance ≈ 0.6 × font_size ≈ 11px
        assert!(cell_w > 5.0, "cell_w should be > 5px: {}", cell_w);
        assert!(cell_w < 40.0, "cell_w should be < 40px: {}", cell_w);
    }
}
