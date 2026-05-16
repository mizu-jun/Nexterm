//! 画像レンダリング（Sixel / Kitty Graphics プロトコルで配置された画像）
//!
//! `renderer/mod.rs` から抽出した:
//! - `ImageEntry` — GPU 画像テクスチャのキャッシュエントリ
//! - `build_image_verts` — 配置済み画像の TextVertex リスト構築
//! - `ensure_image_texture` — 画像テクスチャをキャッシュに登録（初回のみ）

use crate::glyph_atlas::TextVertex;

use super::WgpuState;

/// GPU 画像テクスチャのキャッシュエントリ
pub(super) struct ImageEntry {
    #[allow(dead_code)]
    pub(super) texture: wgpu::Texture,
    pub(super) bind_group: wgpu::BindGroup,
}

/// 配置済み画像の TextVertex リストを構築する
pub(super) fn build_image_verts(
    img: &crate::state::PlacedImage,
    sw: f32,
    sh: f32,
    cell_w: f32,
    cell_h: f32,
) -> (Vec<TextVertex>, Vec<u16>) {
    let px = img.col as f32 * cell_w;
    let py = img.row as f32 * cell_h;
    let pw = img.width as f32;
    let ph = img.height as f32;

    let x0 = px / sw * 2.0 - 1.0;
    let y0 = 1.0 - py / sh * 2.0;
    let x1 = (px + pw) / sw * 2.0 - 1.0;
    let y1 = 1.0 - (py + ph) / sh * 2.0;

    let white = [1.0f32; 4];
    let verts = vec![
        TextVertex {
            position: [x0, y0],
            uv: [0.0, 0.0],
            color: white,
        },
        TextVertex {
            position: [x1, y0],
            uv: [1.0, 0.0],
            color: white,
        },
        TextVertex {
            position: [x1, y1],
            uv: [1.0, 1.0],
            color: white,
        },
        TextVertex {
            position: [x0, y1],
            uv: [0.0, 1.0],
            color: white,
        },
    ];
    let idx = vec![0u16, 1, 2, 0, 2, 3];
    (verts, idx)
}

impl WgpuState {
    /// 画像テクスチャをキャッシュに登録する（初回のみ作成）
    pub(super) fn ensure_image_texture(&mut self, id: u32, img: &crate::state::PlacedImage) {
        if self.image_textures.contains_key(&id) {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("img_texture"),
            size: wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img.rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(img.width * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("img_bind_group"),
            layout: &self.text_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                },
            ],
        });
        self.image_textures.insert(
            id,
            ImageEntry {
                texture,
                bind_group,
            },
        );
    }
}
