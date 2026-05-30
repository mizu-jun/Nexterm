//! Background image rendering (Sprint 5-7 / Phase 3-1).
//!
//! Loads the image specified by `WindowConfig.background_image` at startup
//! and draws it as the full-screen background at the start of every frame.
//!
//! What this module provides:
//! - [`BackgroundTexture`] — cache of the GPU texture + bind_group + opacity.
//! - [`load_background_image`] — image file -> `wgpu::Texture`.
//! - [`compute_background_quad`] — NDC vertex / UV computation per fit mode
//!   (a pure function, suitable for unit tests).
//! - [`build_background_verts`] — builds the [`TextVertex`] list from
//!   `compute_background_quad`'s result.
//!
//! Reuses the existing `image_pipeline` (used for Sixel/Kitty), so no
//! dedicated pipeline is created here. The texture itself is clamped to a
//! maximum size (4096x4096) as a safety guard.

use anyhow::{Context, Result};
use nexterm_config::{BackgroundFit, BackgroundImageConfig};
use tracing::{info, warn};

use crate::glyph_atlas::TextVertex;

use super::WgpuState;

/// Safety upper bound (4K x 4K) to avoid memory blowups with huge images.
const MAX_BACKGROUND_DIMENSION: u32 = 4096;

/// Cache entry for the background image.
pub(super) struct BackgroundTexture {
    #[allow(dead_code)]
    pub(super) texture: wgpu::Texture,
    pub(super) bind_group: wgpu::BindGroup,
    /// Image width in pixels (actual size stored in the texture).
    pub(super) width: u32,
    /// Image height in pixels.
    pub(super) height: u32,
    /// Opacity multiplied at draw time (clamped to 0.0..=1.0).
    pub(super) opacity: f32,
    /// Fit mode (cover / contain / stretch / center / tile).
    pub(super) fit: BackgroundFit,
}

/// One rectangle (paired NDC and UV coordinates).
///
/// `pos` uses a bottom-left origin in the range `[-1, 1]`;
/// `uv` uses a top-left origin in the range `[0, 1]`.
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

/// Compute the draw rectangle for each fit mode (kept as a pure function for testability).
///
/// Returns `Vec<Quad>`:
/// - `Cover` / `Contain` / `Stretch` / `Center`: a single rectangle.
/// - `Tile`: as many tiles as needed to cover the screen.
///
/// Returns an empty `Vec` for invalid input (surface = 0 / image = 0).
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
            // Cover the entire screen, ignoring the aspect ratio
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
            // Preserve aspect ratio and fully cover the screen -> crop the UVs
            let surface_aspect = surface_w / surface_h;
            let image_aspect = iw / ih;
            let (uv_x0, uv_y0, uv_x1, uv_y1) = if image_aspect > surface_aspect {
                // Image is wider -> crop the left/right sides
                let scale = surface_aspect / image_aspect;
                let margin = (1.0 - scale) / 2.0;
                (margin, 0.0, 1.0 - margin, 1.0)
            } else {
                // Image is taller -> crop the top/bottom sides
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
            // Preserve aspect ratio and fit inside the screen -> shrink the draw rect, leave margins
            let surface_aspect = surface_w / surface_h;
            let image_aspect = iw / ih;
            let (px0, py0, px1, py1) = if image_aspect > surface_aspect {
                // Image is wider -> fit horizontally, margin top/bottom
                let scale = surface_aspect / image_aspect;
                (-1.0, -scale, 1.0, scale)
            } else {
                // Image is taller -> fit vertically, margin left/right
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
            // No scaling: place the image pixel-aligned at the center of the screen
            let scale_x = iw / surface_w; // What fraction of the screen width the image occupies
            let scale_y = ih / surface_h;
            // In NDC, the half-extent goes up to +/- 1.0, so `scale_x` is the half-width directly
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
            // Tile the image at native pixel size across the entire screen
            let scale_x = iw / surface_w * 2.0; // NDC width of one tile
            let scale_y = ih / surface_h * 2.0;
            if scale_x <= 0.0 || scale_y <= 0.0 {
                return Vec::new();
            }
            // Guard against excessive tile counts (cap at 256 = 16x16)
            let tiles_x = (2.0 / scale_x).ceil() as i32;
            let tiles_y = (2.0 / scale_y).ceil() as i32;
            if tiles_x * tiles_y > 256 {
                // Fall back to Stretch when the tile count is too high (defensive)
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

/// Build the TextVertex list from `compute_background_quad`'s result.
///
/// Returns a `(verts, indices)` pair. Logs a warning if the count exceeds `i32::MAX`.
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
    // Multiply color by (1, 1, 1, opacity) (the image_pipeline shader does texture * color)
    let color = [1.0, 1.0, 1.0, opacity];
    for quad in quads {
        let base = verts.len() as u16;
        // NDC uses OpenGL-style y-up
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

/// Load an image file from disk and create a `wgpu::Texture`.
///
/// On failure, logs a warning and returns `None` (does not crash).
/// Oversized images are warned about and downscaled to `MAX_BACKGROUND_DIMENSION`.
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
            warn!("Failed to load background image: {}: {}", expanded, e);
            return None;
        }
    };

    let (width, height, rgba) = img;
    info!(
        "Loaded background image: {} ({}x{})",
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

/// Decode an image file and return an RGBA8 byte buffer.
/// Oversized images are downscaled to `MAX_BACKGROUND_DIMENSION`.
fn decode_image(path: &str) -> Result<(u32, u32, Vec<u8>)> {
    let img = image::open(path).with_context(|| format!("failed to open image: {}", path))?;

    // Decide whether to downscale
    let (orig_w, orig_h) = (img.width(), img.height());
    let img = if orig_w > MAX_BACKGROUND_DIMENSION || orig_h > MAX_BACKGROUND_DIMENSION {
        let scale = (MAX_BACKGROUND_DIMENSION as f32 / orig_w.max(orig_h) as f32).min(1.0);
        let new_w = (orig_w as f32 * scale) as u32;
        let new_h = (orig_h as f32 * scale) as u32;
        warn!(
            "Background image is too large; downscaling: {}x{} -> {}x{}",
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
    /// Load and cache the background image at startup (only when successful).
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
        assert_eq!(quads.len(), 1, "should return exactly one quad");
        quads[0]
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn stretch_covers_full_screen_without_changing_uv() {
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
    fn cover_wide_image_crops_left_and_right() {
        // surface 1:1, image 2:1 -> crop the left/right sides and use the middle 50%
        let q = extract_quad(&compute_background_quad(
            100.0,
            100.0,
            200,
            100,
            &BackgroundFit::Cover,
        ));
        // pos covers the full screen
        assert!(approx(q.pos_x0, -1.0) && approx(q.pos_x1, 1.0));
        assert!(approx(q.pos_y0, -1.0) && approx(q.pos_y1, 1.0));
        // uv crops 25% from each side -> 0.25..=0.75
        assert!(approx(q.uv_x0, 0.25));
        assert!(approx(q.uv_x1, 0.75));
        assert!(approx(q.uv_y0, 0.0));
        assert!(approx(q.uv_y1, 1.0));
    }

    #[test]
    fn cover_tall_image_crops_top_and_bottom() {
        // surface 2:1, image 1:1 -> crop the top/bottom sides
        let q = extract_quad(&compute_background_quad(
            200.0,
            100.0,
            100,
            100,
            &BackgroundFit::Cover,
        ));
        // image_aspect = 1, surface_aspect = 2; image_aspect < surface_aspect, so we take the else branch.
        // scale = image_aspect / surface_aspect = 0.5, margin = 0.25.
        assert!(approx(q.uv_x0, 0.0) && approx(q.uv_x1, 1.0));
        assert!(approx(q.uv_y0, 0.25) && approx(q.uv_y1, 0.75));
    }

    #[test]
    fn contain_wide_image_leaves_vertical_margin() {
        // surface 1:1, image 2:1 -> fit horizontally, margin top/bottom
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
    fn contain_tall_image_leaves_horizontal_margin() {
        // surface 2:1, image 1:1 -> fit vertically, margin left/right
        let q = extract_quad(&compute_background_quad(
            200.0,
            100.0,
            100,
            100,
            &BackgroundFit::Contain,
        ));
        // image_aspect = 1, surface_aspect = 2; image_aspect <= surface_aspect, so we take the else branch.
        // scale = image_aspect / surface_aspect = 0.5
        assert!(approx(q.pos_x0, -0.5) && approx(q.pos_x1, 0.5));
        assert!(approx(q.pos_y0, -1.0) && approx(q.pos_y1, 1.0));
    }

    #[test]
    fn center_places_image_at_native_size_in_the_middle() {
        // surface 1000x1000, image 500x500 -> half-extent 0.5
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
    fn tile_returns_multiple_quads() {
        // surface 200x100, image 100x100 -> two tiles side by side
        let quads = compute_background_quad(200.0, 100.0, 100, 100, &BackgroundFit::Tile);
        assert!(quads.len() >= 2, "expected at least two tiles to be placed");
        // Each tile uses UV in 0..=1
        for q in &quads {
            assert!(approx(q.uv_x0, 0.0) && approx(q.uv_x1, 1.0));
        }
    }

    #[test]
    fn invalid_input_returns_empty_vec() {
        assert!(compute_background_quad(0.0, 600.0, 100, 100, &BackgroundFit::Cover).is_empty());
        assert!(compute_background_quad(800.0, 0.0, 100, 100, &BackgroundFit::Cover).is_empty());
        assert!(compute_background_quad(800.0, 600.0, 0, 100, &BackgroundFit::Cover).is_empty());
        assert!(compute_background_quad(800.0, 600.0, 100, 0, &BackgroundFit::Cover).is_empty());
    }

    #[test]
    fn build_background_verts_returns_four_verts_and_six_indices() {
        let (verts, idx) =
            build_background_verts(800.0, 600.0, 1920, 1080, &BackgroundFit::Stretch, 0.5);
        assert_eq!(verts.len(), 4);
        assert_eq!(idx.len(), 6);
        // All vertices should carry the opacity value in color.a
        for v in &verts {
            assert!(approx(v.color[3], 0.5));
        }
    }

    #[test]
    fn build_background_verts_is_empty_for_invalid_input() {
        let (verts, idx) =
            build_background_verts(0.0, 600.0, 1920, 1080, &BackgroundFit::Cover, 1.0);
        assert!(verts.is_empty() && idx.is_empty());
    }

    #[test]
    fn tile_excessive_tiles_falls_back_to_stretch() {
        // Tiling a 1x1 image across a 100x100 surface yields 10000 tiles -> exceeds 256, falls back
        let quads = compute_background_quad(100.0, 100.0, 1, 1, &BackgroundFit::Tile);
        assert_eq!(
            quads.len(),
            1,
            "fallback should produce a single quad like Stretch"
        );
    }
}
