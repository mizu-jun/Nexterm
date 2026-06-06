//! Font size shortcuts (Ctrl+= / Ctrl+- / Ctrl+0)
//!
//! Extracted from `input_handler.rs`:
//! - `change_font_size` — increment/decrement by delta pt
//! - `reset_font_size` — reset to the config-file default

use tracing::info;

use super::EventHandler;
use crate::glyph_atlas::GlyphAtlas;

impl EventHandler {
    /// Change the font size by delta pt and rebuild the glyph atlas
    pub(super) fn change_font_size(&mut self, delta: f32) {
        let new_size = (self.app.config.font.size + delta).clamp(6.0, 72.0);
        if (new_size - self.app.config.font.size).abs() < f32::EPSILON {
            return;
        }
        self.app.config.font.size = new_size;
        self.app.font = crate::font::FontManager::new(
            &self.app.config.font.family,
            new_size,
            &self.app.config.font.font_fallbacks,
            self.scale_factor,
            self.app.config.font.ligatures,
        );
        let atlas_size = self.app.config.gpu.atlas_size;
        if let Some(wgpu) = &self.wgpu_state {
            let mut atlas = GlyphAtlas::new_with_config(&wgpu.device, atlas_size);
            atlas.update_capacity_hint(
                self.app.font.cell_width() as u32,
                self.app.font.cell_height() as u32,
            );
            self.atlas = Some(atlas);
        }
        info!("Font size changed to {}pt", new_size);
    }

    /// Reset the font size to the config-file default
    pub(super) fn reset_font_size(&mut self) {
        // There is no way to recover the original size used when the config was
        // generated, so we fall back to the conventional 14pt default.
        let default_size = nexterm_config::Config::default().font.size;
        self.app.config.font.size = default_size;
        self.app.font = crate::font::FontManager::new(
            &self.app.config.font.family,
            default_size,
            &self.app.config.font.font_fallbacks,
            self.scale_factor,
            self.app.config.font.ligatures,
        );
        let atlas_size = self.app.config.gpu.atlas_size;
        if let Some(wgpu) = &self.wgpu_state {
            let mut atlas = GlyphAtlas::new_with_config(&wgpu.device, atlas_size);
            atlas.update_capacity_hint(
                self.app.font.cell_width() as u32,
                self.app.font.cell_height() as u32,
            );
            self.atlas = Some(atlas);
        }
        info!("Font size reset to {}pt", default_size);
    }
}
