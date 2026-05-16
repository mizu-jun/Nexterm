//! フォントサイズ変更ショートカット（Ctrl+= / Ctrl+- / Ctrl+0）
//!
//! `input_handler.rs` から抽出した:
//! - `change_font_size` — delta pt 分の増減
//! - `reset_font_size` — 設定ファイル初期値に戻す

use tracing::info;

use super::EventHandler;
use crate::glyph_atlas::GlyphAtlas;

impl EventHandler {
    /// フォントサイズを delta pt だけ変更してグリフアトラスを再生成する
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
            self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
        }
        info!("Font size changed to {}pt", new_size);
    }

    /// フォントサイズを設定ファイルの初期値に戻す
    pub(super) fn reset_font_size(&mut self) {
        // 設定ファイルの初期値は config 生成時のサイズを参照する手段がないため
        // 慣例の 14pt をデフォルトとして使用する
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
            self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
        }
        info!("Font size reset to {}pt", default_size);
    }
}
