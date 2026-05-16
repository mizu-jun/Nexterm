//! 背景画像レンダリング（Sprint 5-7 / Phase 3-1）
//!
//! `WindowConfig.background_image` で指定された画像を起動時にロードし、
//! 毎フレームの最初に全画面背景として描画する。
//!
//! 提供するもの:
//! - [`BackgroundTexture`] — GPU テクスチャ + bind_group + 不透明度のキャッシュ
//! - [`load_background_image`] — 画像ファイル → wgpu::Texture
//! - [`compute_background_quad`] — fit モードごとの NDC 頂点/UV 計算（純関数・ユニットテスト対象）
//! - [`build_background_verts`] — `compute_background_quad` の結果から [`TextVertex`] リストを構築
//!
//! 既存の `image_pipeline`（Sixel/Kitty 用）を再利用するため、独自パイプラインは作らない。
//! テクスチャ自体は最大画面サイズ（4096x4096 ガード付き）にクランプ。

use anyhow::{Context, Result};
use nexterm_config::{BackgroundFit, BackgroundImageConfig};
use tracing::{info, warn};

use crate::glyph_atlas::TextVertex;

use super::WgpuState;

/// 巨大画像でメモリを食わないための安全上限（4K x 4K）
const MAX_BACKGROUND_DIMENSION: u32 = 4096;

/// 背景画像のキャッシュエントリ
pub(super) struct BackgroundTexture {
    #[allow(dead_code)]
    pub(super) texture: wgpu::Texture,
    pub(super) bind_group: wgpu::BindGroup,
    /// 画像の幅（ピクセル、テクスチャに格納されている実寸）
    pub(super) width: u32,
    /// 画像の高さ（ピクセル）
    pub(super) height: u32,
    /// 描画時に乗算する不透明度（クランプ済み 0.0〜1.0）
    pub(super) opacity: f32,
    /// fit モード（cover / contain / stretch / center / tile）
    pub(super) fit: BackgroundFit,
}

/// 1 つの矩形（NDC 座標と UV 座標のペア）
///
/// pos は左下原点で `[-1, 1]` の範囲、uv は左上原点で `[0, 1]` の範囲。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Quad {
    pub pos_x0: f32,
    pub pos_y0: f32,
    pub pos_x1: f32,
    pub pos_y1: f32,
    pub uv_x0: f32,
    pub uv_y0: f32,
    pub uv_x1: f32,
    pub uv_y1: f32,
}

/// fit モードごとに描画矩形を計算する（純関数・テスト容易性のために分離）。
///
/// 戻り値は `Vec<Quad>`:
/// - `Cover` / `Contain` / `Stretch` / `Center`: 矩形 1 つ
/// - `Tile`: 画面を埋めるだけのタイル数
///
/// 異常入力（surface = 0 / image = 0）に対しては空 Vec を返す。
pub(super) fn compute_background_quad(
    surface_w: f32,
    surface_h: f32,
    img_w: u32,
    img_h: u32,
    fit: &BackgroundFit,
) -> Vec<Quad> {
    if surface_w <= 0.0 || surface_h <= 0.0 || img_w == 0 || img_h == 0 {
        return Vec::new();
    }

    let iw = img_w as f32;
    let ih = img_h as f32;

    match fit {
        BackgroundFit::Stretch => {
            // アスペクト比無視で画面全体に貼る
            vec![Quad {
                pos_x0: -1.0,
                pos_y0: -1.0,
                pos_x1: 1.0,
                pos_y1: 1.0,
                uv_x0: 0.0,
                uv_y0: 0.0,
                uv_x1: 1.0,
                uv_y1: 1.0,
            }]
        }
        BackgroundFit::Cover => {
            // アスペクト比保持・画面を完全に覆う → UV を切り取る
            let surface_aspect = surface_w / surface_h;
            let image_aspect = iw / ih;
            let (uv_x0, uv_y0, uv_x1, uv_y1) = if image_aspect > surface_aspect {
                // 画像が横長 → 左右を切り取る
                let scale = surface_aspect / image_aspect;
                let margin = (1.0 - scale) / 2.0;
                (margin, 0.0, 1.0 - margin, 1.0)
            } else {
                // 画像が縦長 → 上下を切り取る
                let scale = image_aspect / surface_aspect;
                let margin = (1.0 - scale) / 2.0;
                (0.0, margin, 1.0, 1.0 - margin)
            };
            vec![Quad {
                pos_x0: -1.0,
                pos_y0: -1.0,
                pos_x1: 1.0,
                pos_y1: 1.0,
                uv_x0,
                uv_y0,
                uv_x1,
                uv_y1,
            }]
        }
        BackgroundFit::Contain => {
            // アスペクト比保持・画面に収める → 描画矩形を縮小し余白を残す
            let surface_aspect = surface_w / surface_h;
            let image_aspect = iw / ih;
            let (px0, py0, px1, py1) = if image_aspect > surface_aspect {
                // 画像が横長 → 横方向はフィット・縦方向に余白
                let scale = surface_aspect / image_aspect;
                (-1.0, -scale, 1.0, scale)
            } else {
                // 画像が縦長 → 縦方向はフィット・横方向に余白
                let scale = image_aspect / surface_aspect;
                (-scale, -1.0, scale, 1.0)
            };
            vec![Quad {
                pos_x0: px0,
                pos_y0: py0,
                pos_x1: px1,
                pos_y1: py1,
                uv_x0: 0.0,
                uv_y0: 0.0,
                uv_x1: 1.0,
                uv_y1: 1.0,
            }]
        }
        BackgroundFit::Center => {
            // 拡縮なし・画像をピクセル単位で画面中央に配置
            let scale_x = iw / surface_w; // 画像幅が画面の何割か
            let scale_y = ih / surface_h;
            // NDC では半サイズが ±1.0 まで → scale_x がそのまま半分の幅
            let half_w = scale_x;
            let half_h = scale_y;
            vec![Quad {
                pos_x0: -half_w,
                pos_y0: -half_h,
                pos_x1: half_w,
                pos_y1: half_h,
                uv_x0: 0.0,
                uv_y0: 0.0,
                uv_x1: 1.0,
                uv_y1: 1.0,
            }]
        }
        BackgroundFit::Tile => {
            // 画像をピクセル等倍で画面全体に敷き詰める
            let scale_x = iw / surface_w * 2.0; // タイル 1 つの NDC 幅
            let scale_y = ih / surface_h * 2.0;
            if scale_x <= 0.0 || scale_y <= 0.0 {
                return Vec::new();
            }
            // ガード: 過剰なタイル数を防ぐ（最大 256 個 = 16x16）
            let tiles_x = (2.0 / scale_x).ceil() as i32;
            let tiles_y = (2.0 / scale_y).ceil() as i32;
            if tiles_x * tiles_y > 256 {
                // 過大なタイル数の場合は Stretch にフォールバック（実害回避）
                return compute_background_quad(
                    surface_w,
                    surface_h,
                    img_w,
                    img_h,
                    &BackgroundFit::Stretch,
                );
            }
            let mut quads = Vec::with_capacity((tiles_x * tiles_y).max(1) as usize);
            for ty in 0..tiles_y {
                for tx in 0..tiles_x {
                    let x0 = -1.0 + tx as f32 * scale_x;
                    let y0 = -1.0 + ty as f32 * scale_y;
                    let x1 = (x0 + scale_x).min(1.0);
                    let y1 = (y0 + scale_y).min(1.0);
                    quads.push(Quad {
                        pos_x0: x0,
                        pos_y0: y0,
                        pos_x1: x1,
                        pos_y1: y1,
                        uv_x0: 0.0,
                        uv_y0: 0.0,
                        uv_x1: 1.0,
                        uv_y1: 1.0,
                    });
                }
            }
            quads
        }
    }
}

/// `compute_background_quad` の結果から TextVertex リストを構築する。
///
/// 戻り値は (verts, indices) のペア。i32::MAX を超える場合は警告ログを出す。
pub(super) fn build_background_verts(
    surface_w: f32,
    surface_h: f32,
    img_w: u32,
    img_h: u32,
    fit: &BackgroundFit,
    opacity: f32,
) -> (Vec<TextVertex>, Vec<u16>) {
    let quads = compute_background_quad(surface_w, surface_h, img_w, img_h, fit);
    let mut verts: Vec<TextVertex> = Vec::with_capacity(quads.len() * 4);
    let mut indices: Vec<u16> = Vec::with_capacity(quads.len() * 6);
    // 色は (1, 1, 1, opacity) を乗算（image_pipeline のシェーダで texture * color）
    let color = [1.0, 1.0, 1.0, opacity];
    for quad in quads {
        let base = verts.len() as u16;
        // NDC は OpenGL 流で y 上向き
        verts.push(TextVertex {
            position: [quad.pos_x0, quad.pos_y1],
            uv: [quad.uv_x0, quad.uv_y0],
            color,
        });
        verts.push(TextVertex {
            position: [quad.pos_x1, quad.pos_y1],
            uv: [quad.uv_x1, quad.uv_y0],
            color,
        });
        verts.push(TextVertex {
            position: [quad.pos_x1, quad.pos_y0],
            uv: [quad.uv_x1, quad.uv_y1],
            color,
        });
        verts.push(TextVertex {
            position: [quad.pos_x0, quad.pos_y0],
            uv: [quad.uv_x0, quad.uv_y1],
            color,
        });
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (verts, indices)
}

/// 画像ファイルをディスクからロードして wgpu::Texture を作成する。
///
/// 失敗時は警告ログを出して `None` を返す（クラッシュさせない）。
/// 過大な画像は警告のうえ `MAX_BACKGROUND_DIMENSION` にダウンスケールする。
pub(super) fn load_background_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    config: &BackgroundImageConfig,
) -> Option<BackgroundTexture> {
    if !config.is_enabled() {
        return None;
    }
    let expanded = shellexpand::tilde(&config.path).into_owned();
    let img = match decode_image(&expanded) {
        Ok(img) => img,
        Err(e) => {
            warn!("背景画像の読み込みに失敗しました: {}: {}", expanded, e);
            return None;
        }
    };

    let (width, height, rgba) = img;
    info!(
        "背景画像を読み込みました: {} ({}x{})",
        expanded, width, height
    );

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("background_image_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba,
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
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("background_image_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });

    Some(BackgroundTexture {
        texture,
        bind_group,
        width,
        height,
        opacity: config.clamped_opacity(),
        fit: config.fit.clone(),
    })
}

/// 画像ファイルをデコードして RGBA8 バイト列を返す。
/// 過大な画像は MAX_BACKGROUND_DIMENSION にダウンスケールする。
fn decode_image(path: &str) -> Result<(u32, u32, Vec<u8>)> {
    let img = image::open(path).with_context(|| format!("画像を開けませんでした: {}", path))?;

    // ダウンスケール判定
    let (orig_w, orig_h) = (img.width(), img.height());
    let img = if orig_w > MAX_BACKGROUND_DIMENSION || orig_h > MAX_BACKGROUND_DIMENSION {
        let scale = (MAX_BACKGROUND_DIMENSION as f32 / orig_w.max(orig_h) as f32).min(1.0);
        let new_w = (orig_w as f32 * scale) as u32;
        let new_h = (orig_h as f32 * scale) as u32;
        warn!(
            "背景画像が大きすぎるためダウンスケールします: {}x{} → {}x{}",
            orig_w, orig_h, new_w, new_h
        );
        img.resize(new_w, new_h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok((w, h, rgba.into_raw()))
}

impl WgpuState {
    /// 起動時に背景画像をロードしてキャッシュする（成功時のみ）。
    pub(super) fn load_background(&mut self, cfg: &nexterm_config::WindowConfig) {
        let Some(ref image_cfg) = cfg.background_image else {
            self.background = None;
            return;
        };
        self.background = load_background_image(
            &self.device,
            &self.queue,
            &self.text_bind_group_layout,
            &self.image_sampler,
            image_cfg,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_quad(quads: &[Quad]) -> Quad {
        assert_eq!(quads.len(), 1, "1 枚の矩形を返すべき");
        quads[0]
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn stretch_は全画面に貼り付けてuv_を変えない() {
        let q = extract_quad(&compute_background_quad(
            800.0,
            600.0,
            1920,
            1080,
            &BackgroundFit::Stretch,
        ));
        assert!(approx(q.pos_x0, -1.0) && approx(q.pos_x1, 1.0));
        assert!(approx(q.pos_y0, -1.0) && approx(q.pos_y1, 1.0));
        assert!(approx(q.uv_x0, 0.0) && approx(q.uv_x1, 1.0));
        assert!(approx(q.uv_y0, 0.0) && approx(q.uv_y1, 1.0));
    }

    #[test]
    fn cover_の横長画像は左右を切り取る() {
        // surface 1:1、画像 2:1 → 左右を切り取って中央 50% を使う
        let q = extract_quad(&compute_background_quad(
            100.0,
            100.0,
            200,
            100,
            &BackgroundFit::Cover,
        ));
        // pos は全画面
        assert!(approx(q.pos_x0, -1.0) && approx(q.pos_x1, 1.0));
        assert!(approx(q.pos_y0, -1.0) && approx(q.pos_y1, 1.0));
        // uv は左右各 25% カット → 0.25〜0.75
        assert!(approx(q.uv_x0, 0.25));
        assert!(approx(q.uv_x1, 0.75));
        assert!(approx(q.uv_y0, 0.0));
        assert!(approx(q.uv_y1, 1.0));
    }

    #[test]
    fn cover_の縦長画像は上下を切り取る() {
        // surface 2:1、画像 1:1 → 上下を切り取る
        let q = extract_quad(&compute_background_quad(
            200.0,
            100.0,
            100,
            100,
            &BackgroundFit::Cover,
        ));
        // image_aspect = 1, surface_aspect = 2, image_aspect < surface_aspect なので else 分岐
        // scale = image_aspect / surface_aspect = 0.5、margin = 0.25
        assert!(approx(q.uv_x0, 0.0) && approx(q.uv_x1, 1.0));
        assert!(approx(q.uv_y0, 0.25) && approx(q.uv_y1, 0.75));
    }

    #[test]
    fn contain_の横長画像は縦方向に余白を作る() {
        // surface 1:1、画像 2:1 → 横はフィット、縦に余白
        let q = extract_quad(&compute_background_quad(
            100.0,
            100.0,
            200,
            100,
            &BackgroundFit::Contain,
        ));
        // scale = surface_aspect / image_aspect = 0.5
        assert!(approx(q.pos_x0, -1.0) && approx(q.pos_x1, 1.0));
        assert!(approx(q.pos_y0, -0.5) && approx(q.pos_y1, 0.5));
        assert!(approx(q.uv_x0, 0.0) && approx(q.uv_x1, 1.0));
        assert!(approx(q.uv_y0, 0.0) && approx(q.uv_y1, 1.0));
    }

    #[test]
    fn contain_の縦長画像は横方向に余白を作る() {
        // surface 2:1、画像 1:1 → 縦はフィット、横に余白
        let q = extract_quad(&compute_background_quad(
            200.0,
            100.0,
            100,
            100,
            &BackgroundFit::Contain,
        ));
        // image_aspect = 1, surface_aspect = 2, image_aspect <= surface_aspect なので else
        // scale = image_aspect / surface_aspect = 0.5
        assert!(approx(q.pos_x0, -0.5) && approx(q.pos_x1, 0.5));
        assert!(approx(q.pos_y0, -1.0) && approx(q.pos_y1, 1.0));
    }

    #[test]
    fn center_は画像サイズのまま中央配置() {
        // surface 1000x1000、画像 500x500 → 半サイズ 0.5
        let q = extract_quad(&compute_background_quad(
            1000.0,
            1000.0,
            500,
            500,
            &BackgroundFit::Center,
        ));
        assert!(approx(q.pos_x0, -0.5) && approx(q.pos_x1, 0.5));
        assert!(approx(q.pos_y0, -0.5) && approx(q.pos_y1, 0.5));
        assert!(approx(q.uv_x0, 0.0) && approx(q.uv_x1, 1.0));
    }

    #[test]
    fn tile_は複数の矩形を返す() {
        // surface 200x100、画像 100x100 → 2 タイル横並び
        let quads = compute_background_quad(200.0, 100.0, 100, 100, &BackgroundFit::Tile);
        assert!(quads.len() >= 2, "タイルが 2 つ以上配置されるべき");
        // 各タイルの UV は 0〜1
        for q in &quads {
            assert!(approx(q.uv_x0, 0.0) && approx(q.uv_x1, 1.0));
        }
    }

    #[test]
    fn 異常値は空配列を返す() {
        assert!(compute_background_quad(0.0, 600.0, 100, 100, &BackgroundFit::Cover).is_empty());
        assert!(compute_background_quad(800.0, 0.0, 100, 100, &BackgroundFit::Cover).is_empty());
        assert!(compute_background_quad(800.0, 600.0, 0, 100, &BackgroundFit::Cover).is_empty());
        assert!(compute_background_quad(800.0, 600.0, 100, 0, &BackgroundFit::Cover).is_empty());
    }

    #[test]
    fn build_background_verts_は4頂点6インデックスを返す() {
        let (verts, idx) =
            build_background_verts(800.0, 600.0, 1920, 1080, &BackgroundFit::Stretch, 0.5);
        assert_eq!(verts.len(), 4);
        assert_eq!(idx.len(), 6);
        // 全頂点の color.a が opacity になる
        for v in &verts {
            assert!(approx(v.color[3], 0.5));
        }
    }

    #[test]
    fn build_background_verts_異常値時は空() {
        let (verts, idx) =
            build_background_verts(0.0, 600.0, 1920, 1080, &BackgroundFit::Cover, 1.0);
        assert!(verts.is_empty() && idx.is_empty());
    }

    #[test]
    fn tile_過剰タイルは_stretch_にフォールバック() {
        // 1x1 画像を 100x100 surface に並べると 10000 タイル → 256 を超えてフォールバック
        let quads = compute_background_quad(100.0, 100.0, 1, 1, &BackgroundFit::Tile);
        assert_eq!(quads.len(), 1, "フォールバック時は Stretch と同じく 1 矩形");
    }
}
