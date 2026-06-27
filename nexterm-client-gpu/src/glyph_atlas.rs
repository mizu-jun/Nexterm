//! Glyph atlas — texture cache for GPU text rendering.

use std::num::NonZeroUsize;

use bytemuck::{Pod, Zeroable};
use lru::LruCache;

// ---- Vertex types ----

/// Background-quad vertex (position + color + optional SDF rounded-rect data).
///
/// When `corner_radius == 0.0` the shader takes its flat-rect fast path and
/// returns `color` unmodified, so legacy callers using `add_px_rect` pay no
/// fragment-shader cost beyond a single branch. `rect_center` and
/// `rect_half_size` are in **framebuffer pixel coordinates** (y-down,
/// origin = top-left), matching WGSL's `@builtin(position).xy` in the
/// fragment stage so no uniform/push-constant is needed.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct BgVertex {
    /// NDC coordinates in [-1, 1].
    pub position: [f32; 2],
    /// RGBA color in [0, 1].
    pub color: [f32; 4],
    /// Pixel-space rectangle centre (SDF). Unused when `corner_radius == 0`.
    pub rect_center: [f32; 2],
    /// Pixel-space rectangle half-extents (SDF). Unused when `corner_radius == 0`.
    pub rect_half_size: [f32; 2],
    /// Corner radius in pixels. `0.0` disables the SDF and produces a flat rect.
    pub corner_radius: f32,
}

/// Text vertex (position + UV + color).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct TextVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

// ---- Glyph atlas ----

/// Cache key for a single-character glyph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    pub ch: char,
    pub bold: bool,
    pub italic: bool,
    pub wide: bool,
}

/// Cache key for a ligature glyph (per-row shaping).
///
/// `col` is the grid column, `text` is the entire chunk text. Ligatures are
/// context-dependent, so the surrounding text is part of the cache key too.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LigatureKey {
    pub col: usize,
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// fg color packed into a u32 for hashing ([r, g, b, a] → u32).
    pub fg_packed: u32,
}

/// A rectangle inside the glyph atlas.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GlyphRect {
    /// UV coordinates inside the atlas (top-left and bottom-right).
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Glyph size in pixels.
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
}

/// Glyph atlas (packs every glyph into a single texture).
pub(crate) struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    /// Current atlas dimensions (square).
    pub size: u32,
    /// Maximum atlas size (resolved from the config).
    size_max: u32,
    /// Next column to write to.
    cursor_x: u32,
    /// Y coordinate of the next row to write to.
    cursor_y: u32,
    /// Maximum height in the current row.
    row_height: u32,
    /// Cached single-glyphs — LRU evicts stale entries.
    pub cache: LruCache<GlyphKey, GlyphRect>,
    /// Cached ligature glyphs (per-row shaping) — LRU evicts stale entries.
    pub ligature_cache: LruCache<LigatureKey, GlyphRect>,
    /// True if the atlas was reset within this frame.
    /// Indicates that a redraw is required next frame (prevents UV mismatch).
    pub cleared_this_frame: bool,
    /// True when the atlas needs to grow (the next frame calls `grow()`).
    pub needs_grow: bool,
    /// Font cell width hint for proportional LRU sizing (0 = use default 8×8).
    cell_w_hint: u32,
    /// Font cell height hint for proportional LRU sizing (0 = use default 8×8).
    cell_h_hint: u32,
}

impl GlyphAtlas {
    /// Initial texture size at startup: 1024×1024 = 4 MB.
    const SIZE_INIT: u32 = 1024;
    /// Default maximum texture size: 2048×2048 = 16 MB.
    const SIZE_MAX_DEFAULT: u32 = 2048;

    /// Build using the configured `atlas_size`.
    /// - `atlas_size` becomes the maximum; the initial size is half of it
    ///   (clamped to at least 1024).
    pub fn new_with_config(device: &wgpu::Device, atlas_size: u32) -> Self {
        let max = atlas_size.max(Self::SIZE_INIT);
        let init = (max / 2).max(Self::SIZE_INIT);
        Self::new_with_max(device, init, max)
    }

    fn new_with_max(device: &wgpu::Device, init_size: u32, max_size: u32) -> Self {
        let mut atlas = Self::with_size(device, init_size);
        atlas.size_max = max_size;
        atlas
    }

    /// Construct the atlas at the requested size (used for dynamic growth).
    pub fn with_size(device: &wgpu::Device, size: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // LRU capacity upper bound: size*size divided by the smallest glyph area (8×8).
        let lru_cap = NonZeroUsize::new(((size * size) / 64).max(256) as usize)
            .expect("lru capacity is non-zero");

        Self {
            texture,
            view,
            sampler,
            size,
            size_max: Self::SIZE_MAX_DEFAULT,
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            cache: LruCache::new(lru_cap),
            ligature_cache: LruCache::new(lru_cap),
            cleared_this_frame: false,
            needs_grow: false,
            cell_w_hint: 0,
            cell_h_hint: 0,
        }
    }

    /// Grow the atlas (double the size, or reset if it is already at the max).
    /// After this call the UV cache is invalid, so `cleared_this_frame` becomes true.
    pub fn grow(self, device: &wgpu::Device) -> Self {
        let size_max = self.size_max;
        let cell_w_hint = self.cell_w_hint;
        let cell_h_hint = self.cell_h_hint;
        let new_size = (self.size * 2).min(size_max);
        if new_size > self.size {
            tracing::debug!("growing GlyphAtlas: {}→{}", self.size, new_size);
        }
        // The cache is invalidated, so build a fresh atlas.
        let mut atlas = Self::with_size(device, new_size);
        atlas.size_max = size_max;
        atlas.cleared_this_frame = true;
        if cell_w_hint > 0 && cell_h_hint > 0 {
            atlas.update_capacity_hint(cell_w_hint, cell_h_hint);
        }
        atlas
    }

    /// Compute the LRU capacity from actual font cell dimensions.
    ///
    /// This is a pure function that can be called without a GPU device, making
    /// it straightforward to unit-test independently.
    fn lru_cap_from_cell(atlas_size: u32, cell_w: u32, cell_h: u32) -> NonZeroUsize {
        let cell_area = (cell_w as u64 * cell_h as u64).max(1) as u32;
        NonZeroUsize::new(((atlas_size * atlas_size) / cell_area).max(256) as usize)
            .expect("lru capacity is non-zero")
    }

    /// Update LRU capacity based on the actual font cell dimensions.
    ///
    /// Call this once after atlas creation and again whenever the font changes
    /// (size, DPI, font face) so the LRU matches how many glyphs the atlas
    /// texture can actually hold.
    pub fn update_capacity_hint(&mut self, cell_w: u32, cell_h: u32) {
        let cap = Self::lru_cap_from_cell(self.size, cell_w, cell_h);
        self.cache.resize(cap);
        self.ligature_cache.resize(cap);
        self.cell_w_hint = cell_w;
        self.cell_h_hint = cell_h;
        tracing::debug!(
            cell_w,
            cell_h,
            capacity = cap.get(),
            "GlyphAtlas: LRU capacity updated from font metrics"
        );
    }

    /// Add a glyph to the atlas (returns the existing entry when cached).
    pub fn get_or_insert(
        &mut self,
        key: GlyphKey,
        pixels: &[u8],
        width: u32,
        height: u32,
        queue: &wgpu::Queue,
    ) -> GlyphRect {
        if let Some(rect) = self.cache.get(&key) {
            return *rect;
        }

        // Wrap to the next row when we hit the right edge.
        if self.cursor_x + width > self.size {
            self.cursor_y += self.row_height + 1;
            self.cursor_x = 0;
            self.row_height = 0;
        }

        // Atlas full: if below the max, signal growth; otherwise reset the cache
        // and restart from the origin. Setting `cleared_this_frame = true` forces
        // a redraw next frame to avoid the "wrote a UV, then overwrote the slot"
        // mismatch that would otherwise produce garbled glyphs.
        if self.cursor_y + height > self.size {
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.row_height = 0;
            self.cache.clear();
            self.cleared_this_frame = true;
            if self.size < self.size_max {
                // Call `grow()` next frame to expand the texture.
                self.needs_grow = true;
            }
        }

        // Write the glyph into the texture.
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: self.cursor_x,
                    y: self.cursor_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let s = self.size as f32;
        let rect = GlyphRect {
            uv_min: [self.cursor_x as f32 / s, self.cursor_y as f32 / s],
            uv_max: [
                (self.cursor_x + width) as f32 / s,
                (self.cursor_y + height) as f32 / s,
            ],
            width,
            height,
        };

        self.cursor_x += width + 1;
        self.row_height = self.row_height.max(height);
        self.cache.put(key, rect);
        rect
    }

    /// Add a ligature glyph to the atlas (returns the existing entry when cached).
    pub fn get_or_insert_ligature(
        &mut self,
        key: LigatureKey,
        pixels: &[u8],
        width: u32,
        height: u32,
        queue: &wgpu::Queue,
    ) -> GlyphRect {
        if let Some(rect) = self.ligature_cache.get(&key) {
            return *rect;
        }

        if self.cursor_x + width > self.size {
            self.cursor_y += self.row_height + 1;
            self.cursor_x = 0;
            self.row_height = 0;
        }

        if self.cursor_y + height > self.size {
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.row_height = 0;
            self.cache.clear();
            self.ligature_cache.clear();
            self.cleared_this_frame = true;
            if self.size < self.size_max {
                self.needs_grow = true;
            }
        }

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: self.cursor_x,
                    y: self.cursor_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let s = self.size as f32;
        let rect = GlyphRect {
            uv_min: [self.cursor_x as f32 / s, self.cursor_y as f32 / s],
            uv_max: [
                (self.cursor_x + width) as f32 / s,
                (self.cursor_y + height) as f32 / s,
            ],
            width,
            height,
        };

        self.cursor_x += width + 1;
        self.row_height = self.row_height.max(height);
        self.ligature_cache.put(key, rect);
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::GlyphAtlas;

    #[test]
    fn lru_cap_default_formula_unchanged() {
        // 8×8 glyph → same result as the old formula `(size*size)/64`
        let cap = GlyphAtlas::lru_cap_from_cell(1024, 8, 8);
        assert_eq!(cap.get(), (1024 * 1024) / 64);
    }

    #[test]
    fn lru_cap_realistic_font() {
        // 14pt / 96 DPI → typical cell ~11×22 px
        let cap = GlyphAtlas::lru_cap_from_cell(1024, 11, 22);
        // For an 11×22 cell the area-based capacity is far above the 256 floor,
        // so no clamp is needed here; the floor itself is covered by
        // `lru_cap_floor_at_256`.
        let expected = ((1024u64 * 1024) / (11u64 * 22)) as usize;
        assert_eq!(cap.get(), expected);
        // Must be much smaller than the default 8×8 capacity
        let default_cap = GlyphAtlas::lru_cap_from_cell(1024, 8, 8).get();
        assert!(cap.get() < default_cap);
    }

    #[test]
    fn lru_cap_floor_at_256() {
        // Absurdly large cells should still return at least 256
        let cap = GlyphAtlas::lru_cap_from_cell(64, 64, 64);
        assert_eq!(cap.get(), 256);
    }

    #[test]
    fn lru_cap_doubles_with_atlas_size() {
        // Capacity should scale quadratically with atlas size
        let cap1 = GlyphAtlas::lru_cap_from_cell(1024, 16, 32).get();
        let cap2 = GlyphAtlas::lru_cap_from_cell(2048, 16, 32).get();
        assert_eq!(cap2, cap1 * 4);
    }
}
