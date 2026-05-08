//! グリフアトラス — GPU テキスト描画用テクスチャキャッシュ

use std::num::NonZeroUsize;

use bytemuck::{Pod, Zeroable};
use lru::LruCache;

// ---- 頂点型 ----

/// 背景矩形用の頂点（位置 + 色）
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct BgVertex {
    /// NDC 座標 [-1, 1]
    pub position: [f32; 2],
    /// RGBA 色 [0, 1]
    pub color: [f32; 4],
}

/// テキスト用の頂点（位置 + UV + 色）
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct TextVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

// ---- グリフアトラス ----

/// グリフキャッシュのキー（1文字単位）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    pub ch: char,
    pub bold: bool,
    pub italic: bool,
    pub wide: bool,
}

/// リガチャグリフキャッシュのキー（行単位シェーピング用）
///
/// `col` はグリッド列インデックス、`text` はチャンク全体の文字列。
/// リガチャは文脈依存のため、前後の文字列も含めてキャッシュキーにする。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LigatureKey {
    pub col: usize,
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// fg 色を u32 にパックして比較する（[r,g,b,a] → u32）
    pub fg_packed: u32,
}

/// グリフアトラス内の矩形
#[derive(Debug, Clone, Copy)]
pub(crate) struct GlyphRect {
    /// アトラス内の UV 座標（左上・右下）
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// グリフのピクセルサイズ
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
}

/// グリフアトラス（全グリフを 1 枚のテクスチャに詰め込む）
pub(crate) struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    /// アトラスの現在の寸法（正方形）
    pub size: u32,
    /// アトラスの最大サイズ（設定値から決定）
    size_max: u32,
    /// 次に書き込む列
    cursor_x: u32,
    /// 次に書き込む行の Y 座標
    cursor_y: u32,
    /// 現在行の最大高さ
    row_height: u32,
    /// キャッシュ済みグリフ（1文字単位）— LRU で古いエントリを自動削除
    pub cache: LruCache<GlyphKey, GlyphRect>,
    /// キャッシュ済みリガチャグリフ（行単位シェーピング）— LRU で古いエントリを自動削除
    pub ligature_cache: LruCache<LigatureKey, GlyphRect>,
    /// フレーム内でアトラスをリセットした場合 true
    /// 次フレームで再描画が必要なことを示す（UV 不整合防止）
    pub cleared_this_frame: bool,
    /// サイズアップが必要な場合 true（次フレームで grow() を呼ぶ）
    pub needs_grow: bool,
}

impl GlyphAtlas {
    /// 起動時の初期テクスチャサイズ: 1024×1024 = 4MB
    const SIZE_INIT: u32 = 1024;
    /// テクスチャのデフォルト最大サイズ: 2048×2048 = 16MB
    const SIZE_MAX_DEFAULT: u32 = 2048;

    /// atlas_size 設定値を使って初期化する
    /// - `atlas_size` を最大サイズとして使用し、初期サイズはその半分（最低 1024）
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

    /// 指定サイズでアトラスを生成する（動的拡張用）
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

        // LRU キャパシティ: size*size を最小グリフ面積 (8×8) で割った値を上限とする
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
        }
    }

    /// アトラスを拡張する（サイズを 2 倍にするか、最大サイズですでにあればリセット）。
    /// 呼び出し後は UV キャッシュが無効になるため cleared_this_frame が true になる。
    pub fn grow(self, device: &wgpu::Device) -> Self {
        let size_max = self.size_max;
        let new_size = (self.size * 2).min(size_max);
        if new_size > self.size {
            tracing::debug!("GlyphAtlas 拡張: {}→{}", self.size, new_size);
        }
        // キャッシュは無効化されるため新しいアトラスを作成する
        let mut atlas = Self::with_size(device, new_size);
        atlas.size_max = size_max;
        atlas.cleared_this_frame = true;
        atlas
    }

    /// グリフをアトラスに追加する（既存なら再利用）
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

        // 行末で折り返し
        if self.cursor_x + width > self.size {
            self.cursor_y += self.row_height + 1;
            self.cursor_x = 0;
            self.row_height = 0;
        }

        // アトラスが満杯の場合: サイズが MAX 未満なら拡張シグナルを立てる。
        // 最大サイズの場合はキャッシュをリセットして原点から再開する。
        // cleared_this_frame = true をセットし、次フレームでの再描画を促す。
        // これにより「クリア前に書いたUV」と「クリア後に上書きされた内容」の
        // 不整合（グリフ化け）を防ぐ。
        if self.cursor_y + height > self.size {
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.row_height = 0;
            self.cache.clear();
            self.cleared_this_frame = true;
            if self.size < self.size_max {
                // 次フレームで grow() を呼んでテクスチャを拡張する
                self.needs_grow = true;
            }
        }

        // テクスチャへ書き込む
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

    /// リガチャグリフをアトラスに追加する（既存なら再利用）
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
