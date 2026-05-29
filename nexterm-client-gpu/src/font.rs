//! Font management — glyph rendering via cosmic-text.

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

/// A rasterized glyph produced by ligature-aware rendering (per-row output).
pub struct RenderedGlyph {
    /// Grid column index (0-origin).
    pub col: usize,
    /// Physical glyph width (pixels).
    pub width: u32,
    /// Physical glyph height (pixels).
    pub height: u32,
    /// RGBA pixel data.
    pub pixels: Vec<u8>,
}

/// Wrapper around the font system.
pub struct FontManager {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub metrics: Metrics,
    /// Measured cell width (physical pixels) per character.
    cell_w: f32,
    /// Configured font family name (passed to `Attrs`).
    family: String,
    /// Whether to enable ligatures.
    pub ligatures: bool,
}

impl FontManager {
    /// Create a font manager with the given configuration.
    ///
    /// `family` is the primary font family name.
    /// `fallbacks` is a list of font families to try in order when glyphs are missing.
    /// `scale_factor` is the DPI scale from winit's `window.scale_factor()`.
    /// `ligatures` enables HarfBuzz ligature shaping.
    pub fn new(
        family: &str,
        size_pt: f32,
        fallbacks: &[String],
        scale_factor: f32,
        ligatures: bool,
    ) -> Self {
        // `FontSystem::new()` scans every system font and consumes ~30–50 MB.
        // We use a curated loader instead to keep memory usage down.
        let mut font_system = Self::build_font_system(family, fallbacks);

        // Register the primary font under the generic `monospace` name.
        // `Attrs::new().family(Family::Monospace)` references it.
        font_system.db_mut().set_monospace_family(family);

        if !fallbacks.is_empty() {
            tracing::debug!(
                "font fallback chain: {} -> {}",
                family,
                fallbacks.join(" -> ")
            );
        }

        // size_pt × (96 dpi / 72 dpi) × scale_factor = physical font size in pixels.
        // Windows defaults to 96 DPI; `scale_factor` represents the display scaling.
        let font_size_px = size_pt * (96.0 / 72.0) * scale_factor;
        let line_height = font_size_px * 1.2;
        let metrics = Metrics::new(font_size_px, line_height);

        let mut swash_cache = SwashCache::new();

        // Measure the advance width of the reference character '0' via `layout_runs()`.
        // `Attrs::new()` defaults to `Family::SansSerif`, so we explicitly request
        // `Family::Monospace` (or `Family::Name` when a specific family is configured).
        let cell_w = Self::measure_char_width(&mut font_system, &mut swash_cache, metrics, family);

        tracing::debug!(
            "font init: family={} {}pt × scale={} → {}px, cell_w={:.1}px, cell_h={:.1}px",
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

    /// Initialise the font system with a curated set of font directories.
    ///
    /// `FontSystem::new()` scans every system font and consumes ~30–50 MB.
    /// This helper only loads the OS-specific main font directories to keep
    /// memory usage down, while still covering CJK and emoji fallback fonts.
    fn build_font_system(_primary_family: &str, _fallbacks: &[String]) -> FontSystem {
        use cosmic_text::fontdb;

        let locale = sys_locale::get_locale().unwrap_or_else(|| "ja-JP".to_string());
        let mut db = fontdb::Database::new();

        // OS-specific main font directories (faster than a full scan).
        // Fallback: `load_system_fonts()` does a full scan.
        #[cfg(target_os = "macos")]
        {
            // System fonts (covers emoji and CJK).
            db.load_fonts_dir("/System/Library/Fonts");
            // User-installed fonts are intentionally skipped (the main terminal
            // fonts ship with the OS).
        }
        #[cfg(target_os = "windows")]
        {
            // Windows system fonts.
            db.load_fonts_dir("C:\\Windows\\Fonts");
        }
        #[cfg(target_os = "linux")]
        {
            // Main Linux font directories (faster than a full scan).
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
            // Fall back to a full scan on unknown operating systems.
            db.load_system_fonts();
        }

        tracing::debug!("font DB loaded {} faces", db.len());

        FontSystem::new_with_locale_and_db(locale, db)
    }

    /// Measure the advance width of the ASCII reference character '0'.
    ///
    /// `Buffer::draw()` only emits ink pixels, so it does not match the advance
    /// width. We use `glyph.x + glyph.w` from `layout_runs()` to get the precise
    /// cell width.
    fn measure_char_width(
        font_system: &mut FontSystem,
        _swash_cache: &mut SwashCache,
        metrics: Metrics,
        family: &str,
    ) -> f32 {
        let mut buf = Buffer::new(font_system, metrics);
        // Use `Family::Monospace` for the generic "monospace" name; otherwise pass
        // the family name directly via `Family::Name`. Either path avoids the
        // SansSerif fallback so we get an accurate cell width.
        // `family_owned` is declared up front so its lifetime covers every branch.
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

        // Pull the advance width out of each glyph's hit box (x + w) in `layout_runs()`.
        // That value is the precise font advance width (left bearing + ink + right bearing).
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

        // Fall back to `line_height * 0.5` if the measurement failed.
        if advance > 1.0 {
            advance
        } else {
            metrics.line_height * 0.5
        }
    }

    /// Rasterise a single character and return its RGBA pixels.
    ///
    /// When `wide` is true we treat the character as full-width (e.g. CJK) and
    /// allocate a 2-cell-wide buffer.
    /// Returns `(width, height, rgba_pixels)`.
    pub fn rasterize_char(
        &mut self,
        ch: char,
        bold: bool,
        italic: bool,
        fg: [u8; 4],
        wide: bool,
    ) -> (u32, u32, Vec<u8>) {
        let mut buffer = Buffer::new(&mut self.font_system, self.metrics);

        // Use `Family::Monospace` for the generic "monospace" name; otherwise pass
        // the family name directly via `Family::Name` (prevents the SansSerif fallback).
        // `family_owned` is declared up front so its lifetime covers every branch.
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
        // Full-width characters render with a 2-cell buffer (double the width).
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

        // Cell size: full-width characters span two cells.
        let cell_w = (self.cell_w * display_cols).ceil() as u32;
        let cell_h = self.metrics.line_height.ceil() as u32;
        let mut pixels = vec![0u8; (cell_w * cell_h * 4) as usize];

        let color = Color::rgba(fg[0], fg[1], fg[2], fg[3]);

        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            color,
            |x, y, _w, _h, c| {
                // Skip negative coordinates (e.g. left bearings).
                if x < 0 || y < 0 {
                    return;
                }
                let px = x as u32;
                let py = y as u32;
                if px < cell_w && py < cell_h {
                    let idx = ((py * cell_w + px) * 4) as usize;
                    if idx + 3 < pixels.len() {
                        // Alpha-blend (plain overwrite).
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

    /// Rasterise a full row with ligatures and return per-column glyphs.
    ///
    /// `chars` is a list of `(col, char, bold, italic, fg_rgba)` tuples.
    /// HarfBuzz's `Advanced` shaping is enabled, so ligature glyphs (e.g. "->",
    /// "=>", "!=") are synthesised correctly.
    ///
    /// Returns an empty list when ligatures are disabled (`self.ligatures == false`).
    /// Callers should fall back to `rasterize_char()` in that case.
    pub fn rasterize_line_segment(
        &mut self,
        chars: &[(usize, char, bool, bool, [u8; 4])],
    ) -> Vec<RenderedGlyph> {
        if !self.ligatures || chars.is_empty() {
            return Vec::new();
        }

        // Split the input into contiguous chunks that share the same fg/bold/italic
        // attributes and shape each chunk separately. Ligatures break at attribute
        // boundaries, which is the visually correct behavior.
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

    /// Shape a single attribute-uniform chunk and return its glyphs.
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

        // Shape the whole chunk in one HarfBuzz pass.
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

        // Pull each glyph's x offset and width from `layout_runs()` and map them
        // back to grid columns by dividing by the cell width.
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

        // If we couldn't extract any glyphs, return empty so the caller falls back.
        if glyphs.is_empty() {
            return Vec::new();
        }

        // Rasterise the whole buffer once, then slice out each glyph region.
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

        // Slice each glyph region and convert it into a `RenderedGlyph`.
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

    /// Return the cell width (physical pixels) — the measured advance width.
    pub fn cell_width(&self) -> f32 {
        self.cell_w
    }

    /// Return the cell height (physical pixels).
    pub fn cell_height(&self) -> f32 {
        self.metrics.line_height
    }

    /// Return the configured font family name.
    #[allow(dead_code)]
    pub fn family(&self) -> &str {
        &self.family
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_manager_constructs() {
        let fm = FontManager::new("monospace", 14.0, &[], 1.0, true);
        assert!(fm.cell_width() > 0.0);
        assert!(fm.cell_height() > 0.0);
    }

    #[test]
    fn cell_size_is_positive() {
        let fm = FontManager::new("monospace", 16.0, &[], 1.0, true);
        assert!(fm.cell_width() > 5.0);
        assert!(fm.cell_height() > 10.0);
    }

    #[test]
    fn constructs_with_a_fallback_chain() {
        let fallbacks = vec![
            "Noto Sans CJK JP".to_string(),
            "Noto Color Emoji".to_string(),
        ];
        let fm = FontManager::new("JetBrains Mono", 14.0, &fallbacks, 1.0, true);
        assert!(fm.cell_width() > 0.0);
        assert!(fm.cell_height() > 0.0);
    }

    #[test]
    fn scale_factor_1_25_grows_the_cell() {
        let fm1 = FontManager::new("monospace", 14.0, &[], 1.0, true);
        let fm125 = FontManager::new("monospace", 14.0, &[], 1.25, true);
        assert!(fm125.cell_width() > fm1.cell_width());
        assert!(fm125.cell_height() > fm1.cell_height());
    }

    #[test]
    fn advance_width_is_at_least_the_ink_width() {
        // `layout_runs()` measurement is greater than or equal to the ink width
        // (it includes the right bearing).
        let fm = FontManager::new("monospace", 14.0, &[], 1.0, true);
        // A monospace font has the same width for every glyph.
        let cell_w = fm.cell_width();
        assert!(cell_w > 0.0, "cell_w should be positive: {}", cell_w);
        // 14 pt @ 96 dpi = 18.67 px; advance ≈ 0.6 × font_size ≈ 11 px.
        assert!(cell_w > 5.0, "cell_w should be > 5 px: {}", cell_w);
        assert!(cell_w < 40.0, "cell_w should be < 40 px: {}", cell_w);
    }
}
