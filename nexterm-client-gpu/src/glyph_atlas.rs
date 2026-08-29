//! Glyph atlas — texture cache for GPU text rendering.

use std::num::NonZeroUsize;

use bytemuck::{Pod, Zeroable};
use lru::LruCache;

use crate::icons;

/// Largest pixel size a chrome icon is drawn at (the 16/20/24 px steps).
const MAX_ICON_PX: u64 = 24;
/// How many of those steps one icon can occupy in the cache at once.
const ICON_SIZE_STEPS: u64 = 3;
/// Atlas area reserved for chrome icons, excluded from the cell-based LRU
/// capacity. Roughly 3% of a 1024² atlas — small enough not to matter to
/// terminal glyph caching, honest enough that the capacity is not a fiction.
const ICON_RESERVED_AREA: u64 =
    icons::ALL_ICONS.len() as u64 * ICON_SIZE_STEPS * MAX_ICON_PX * MAX_ICON_PX;

// The reservation is only defensible while it stays negligible. If the icon set
// ever grows enough to claim a meaningful share of a 1024² atlas, "reserve and
// ignore" needs revisiting rather than silently shrinking the glyph cache.
const _: () = assert!(
    ICON_RESERVED_AREA * 20 < 1024 * 1024,
    "the chrome icon set now claims over 5% of a 1024x1024 atlas"
);

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
    /// Penumbra half-width in pixels (UI/UX v3 P2a). `> 0.0` widens the SDF
    /// edge fade from the 1 px AA into a soft drop shadow; the quad must be
    /// grown by the same amount (see `add_px_soft_shadow_sdf`).
    pub shadow_softness: f32,
    /// Outline band width in pixels (UI/UX v3 P2a). `> 0.0` paints only a
    /// stroke hugging the inside of the rect edge instead of a fill.
    pub stroke_width: f32,
    /// Acrylic blend factor in `0.0..=1.0` (UI/UX v3 P2b). `0.0` (the
    /// default for every non-overlay vertex) draws the flat `color` as
    /// today; `> 0.0` mixes in the blurred/tinted `scene_color` sample by
    /// this amount. Only overlay panel fills ever set this to non-zero.
    pub acrylic_mix: f32,
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

/// Which font a cached glyph was rasterised from.
///
/// This is part of [`GlyphKey`] rather than an implementation detail because
/// the bundled icon font (UI/UX v3 P4a) occupies the Private Use Area from
/// `U+F101` upward, which sits *inside* the `U+E000`–`U+F8FF` Nerd Font range
/// `tab_icons.rs` draws terminal-content icons from. Without this
/// discriminant, `U+F101` from the icon font and `U+F101` from a user's Nerd
/// Font would share one cache slot and whichever rasterised first would win.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FontRole {
    /// The user-configured terminal font, rasterised at the cell size.
    Terminal,
    /// The bundled chrome icon font, rasterised at an explicit pixel size.
    Icon,
    /// The terminal font, rasterised at a chrome type-ramp size rather than at
    /// the cell (UI/UX v3 P4b). Same face as [`Self::Terminal`], different
    /// size — which is exactly why it needs its own discriminant: `A` at the
    /// cell size and `A` at Caption 12 are different bitmaps.
    Chrome,
}

/// Cache key for a single-character glyph.
///
/// The fields are private: construct through [`GlyphKey::terminal`] or
/// [`GlyphKey::icon`] so that a new call site cannot forget to say which font
/// it means.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    ch: char,
    bold: bool,
    italic: bool,
    wide: bool,
    role: FontRole,
    /// Rasterised size in whole pixels. Always 0 for [`FontRole::Terminal`],
    /// whose size is the cell and therefore already implied by the atlas
    /// invalidation that follows any font or DPI change. Icons carry their
    /// size so that the same glyph at 16 px and at 20 px can coexist.
    size_px: u16,
}

impl GlyphKey {
    /// A glyph from the terminal font, at the current cell size.
    pub fn terminal(ch: char, bold: bool, italic: bool, wide: bool) -> Self {
        Self {
            ch,
            bold,
            italic,
            wide,
            role: FontRole::Terminal,
            size_px: 0,
        }
    }

    /// A chrome icon from the bundled icon font, at an explicit pixel size.
    ///
    /// `size_px` is quantised to whole pixels by the caller's cast so that a
    /// window resize does not generate an unbounded key space.
    pub fn icon(ch: char, size_px: u16) -> Self {
        Self {
            ch,
            bold: false,
            italic: false,
            wide: false,
            role: FontRole::Icon,
            size_px,
        }
    }

    /// A glyph from the terminal font at a chrome type-ramp size.
    ///
    /// `wide` is absent by design: chrome runs advance by the glyph's measured
    /// width, not by cell columns, so the full-width flag that the grid needs
    /// has nothing to key here.
    pub fn chrome(ch: char, size_px: u16, bold: bool) -> Self {
        Self {
            ch,
            bold,
            italic: false,
            wide: false,
            role: FontRole::Chrome,
            size_px,
        }
    }
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
        // Audit round 3 (R1): use u64 math so a large configured atlas size cannot
        // overflow the u32 multiplication (debug panic / release wrap).
        let atlas_sq = (size as u64).saturating_mul(size as u64);
        let lru_cap = NonZeroUsize::new((atlas_sq / 64).max(256).min(usize::MAX as u64) as usize)
            .unwrap_or(NonZeroUsize::MIN);

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
        // Audit round 3 (R1): guard against u32 overflow in atlas_size * atlas_size
        // and cell_w * cell_h (atlas_size is user-configurable via gpu.atlas_size).
        let atlas_sq = (atlas_size as u64).saturating_mul(atlas_size as u64);
        let cell_area = (cell_w as u64).saturating_mul(cell_h as u64).max(1);
        // UI/UX v3 P4a: the cache is no longer all cell-sized entries — chrome
        // icons are rasterised at 16/20/24 px regardless of the cell. Rather
        // than pessimise the formula to the largest possible entry (which would
        // cut the capacity ~8x for a small terminal font and evict terminal
        // glyphs that fit fine), reserve the area the *bounded* icon set can
        // occupy and divide what is left by the cell. The icon set is a
        // compile-time list, so this reservation cannot drift from reality.
        let usable = atlas_sq.saturating_sub(ICON_RESERVED_AREA);
        let cap = (usable / cell_area).max(256).min(usize::MAX as u64) as usize;
        NonZeroUsize::new(cap).unwrap_or(NonZeroUsize::MIN)
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
    use super::{FontRole, GlyphAtlas, GlyphKey, ICON_RESERVED_AREA};

    #[test]
    fn lru_cap_default_formula_less_the_icon_reservation() {
        // 8×8 glyph. UI/UX v3 P4a subtracts the bounded icon-set area from the
        // usable atlas before dividing, so this is the old `(size*size)/64`
        // minus that reservation — not the old number.
        let cap = GlyphAtlas::lru_cap_from_cell(1024, 8, 8);
        assert_eq!(
            cap.get(),
            ((1024 * 1024 - ICON_RESERVED_AREA) / 64) as usize
        );
    }

    #[test]
    fn lru_cap_realistic_font() {
        // 14pt / 96 DPI → typical cell ~11×22 px
        let cap = GlyphAtlas::lru_cap_from_cell(1024, 11, 22);
        // For an 11×22 cell the area-based capacity is far above the 256 floor,
        // so no clamp is needed here; the floor itself is covered by
        // `lru_cap_floor_at_256`.
        let expected = ((1024u64 * 1024 - ICON_RESERVED_AREA) / (11u64 * 22)) as usize;
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
    fn lru_cap_grows_quadratically_with_atlas_size() {
        // Capacity scales with atlas *area*, so doubling the side roughly
        // quadruples it. "Roughly" is deliberate: P4a subtracts a constant
        // icon reservation before dividing, so the relationship is no longer
        // exactly 4x — a bigger atlas amortises the fixed reservation and
        // lands slightly above it. This assertion was `== cap1 * 4` and is
        // updated on purpose, not relaxed to hide a regression.
        let cap1 = GlyphAtlas::lru_cap_from_cell(1024, 16, 32).get();
        let cap2 = GlyphAtlas::lru_cap_from_cell(2048, 16, 32).get();
        assert!(cap2 > cap1 * 4, "{cap2} should exceed 4x {cap1}");
        assert!(cap2 < cap1 * 41 / 10, "{cap2} should stay near 4x {cap1}");
    }

    #[test]
    fn lru_cap_survives_overflowing_atlas_size() {
        // Audit round 3 (R1): atlas_size is user-configurable; a value whose
        // square overflows u32 (>= 65536) must not panic or wrap. It should
        // simply yield a large, valid capacity via saturating u64 math.
        let cap = GlyphAtlas::lru_cap_from_cell(100_000, 8, 8);
        assert!(cap.get() >= 256);
        // 100_000^2 / 64 must be computed in u64, not a wrapped u32.
        let expected = ((100_000u64 * 100_000 - ICON_RESERVED_AREA) / 64) as usize;
        assert_eq!(cap.get(), expected);
    }

    #[test]
    fn lru_cap_survives_an_atlas_smaller_than_the_icon_reservation() {
        // A tiny atlas_size makes the reservation exceed the whole atlas. The
        // saturating subtraction must floor at zero rather than wrap, leaving
        // the 256 floor to answer.
        let cap = GlyphAtlas::lru_cap_from_cell(16, 8, 8);
        assert_eq!(cap.get(), 256);
    }

    #[test]
    fn terminal_and_icon_glyphs_do_not_share_a_cache_slot() {
        // The regression this guards: Fluent's PUA codepoints sit inside the
        // Nerd Font range, so the same char can legitimately arrive from both
        // fonts. They must be distinct cache entries.
        let ch = '\u{f101}';
        assert_ne!(
            GlyphKey::terminal(ch, false, false, false),
            GlyphKey::icon(ch, 16)
        );
    }

    #[test]
    fn icon_glyphs_are_keyed_by_size() {
        let ch = '\u{f101}';
        assert_ne!(GlyphKey::icon(ch, 16), GlyphKey::icon(ch, 20));
        assert_eq!(GlyphKey::icon(ch, 16), GlyphKey::icon(ch, 16));
    }

    #[test]
    fn terminal_keys_ignore_size_and_keep_their_style_flags() {
        // Terminal glyphs are always at the cell size, so size must not enter
        // the key; the style flags still must.
        let a = GlyphKey::terminal('A', false, false, false);
        assert_eq!(a.size_px, 0);
        assert_eq!(a.role, FontRole::Terminal);
        assert_ne!(a, GlyphKey::terminal('A', true, false, false));
        assert_ne!(a, GlyphKey::terminal('A', false, true, false));
        assert_ne!(a, GlyphKey::terminal('A', false, false, true));
    }

    #[test]
    fn lru_cap_handles_zero_cell_dimensions() {
        // Degenerate cell dimensions must not divide by zero.
        let cap = GlyphAtlas::lru_cap_from_cell(1024, 0, 0);
        assert!(cap.get() >= 256);
    }
}
