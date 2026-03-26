//! wgpu + winit レンダラー
//!
//! 描画パイプライン:
//!   1. ターミナルセルの背景色を頂点バッファで描画（カラーパス）
//!   2. cosmic-text でグリフをラスタライズし、グリフアトラスに書き込む
//!   3. グリフアトラスからサンプリングしてテキストを描画（テキストパス）

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use nexterm_config::{Config, StatusBarEvaluator};
use nexterm_proto::ClientToServer;
use nexterm_proto::KeyCode as ProtoKeyCode;
use tracing::{debug, info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode as WKeyCode, ModifiersState, PhysicalKey},
    window::{Window, WindowId},
};

use crate::{connection::Connection, font::FontManager, state::ClientState};

// ---- 頂点型 ----

/// 背景矩形用の頂点（位置 + 色）
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct BgVertex {
    /// NDC 座標 [-1, 1]
    position: [f32; 2],
    /// RGBA 色 [0, 1]
    color: [f32; 4],
}

/// テキスト用の頂点（位置 + UV + 色）
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct TextVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

// ---- グリフアトラス ----

/// グリフキャッシュのキー
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GlyphKey {
    ch: char,
    bold: bool,
    italic: bool,
}

/// グリフアトラス内の矩形
#[derive(Debug, Clone, Copy)]
struct GlyphRect {
    /// アトラス内の UV 座標（左上・右下）
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    /// グリフのピクセルサイズ
    width: u32,
    height: u32,
}

/// グリフアトラス（全グリフを 1 枚のテクスチャに詰め込む）
struct GlyphAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    /// アトラスの寸法（正方形）
    size: u32,
    /// 次に書き込む列
    cursor_x: u32,
    /// 次に書き込む行の Y 座標
    cursor_y: u32,
    /// 現在行の最大高さ
    row_height: u32,
    /// キャッシュ済みグリフ
    cache: HashMap<GlyphKey, GlyphRect>,
}

impl GlyphAtlas {
    const SIZE: u32 = 2048;

    fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: Self::SIZE,
                height: Self::SIZE,
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

        Self {
            texture,
            view,
            sampler,
            size: Self::SIZE,
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            cache: HashMap::new(),
        }
    }

    /// グリフをアトラスに追加する（既存なら再利用）
    fn get_or_insert(
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

        // アトラスが満杯の場合は原点に戻す（簡易 LRU の代わり）
        if self.cursor_y + height > self.size {
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.row_height = 0;
            self.cache.clear();
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
        self.cache.insert(key, rect);
        rect
    }
}

// ---- wgpu コアステート ----

/// wgpu の初期化済み状態
struct WgpuState {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    bg_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    text_bind_group_layout: wgpu::BindGroupLayout,
    /// 画像レンダリングパイプライン
    image_pipeline: wgpu::RenderPipeline,
    /// 画像用サンプラー
    image_sampler: wgpu::Sampler,
    /// 画像テクスチャキャッシュ（image_id → ImageEntry）
    image_textures: HashMap<u32, ImageEntry>,
}

impl WgpuState {
    async fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // SAFETY: surface は window と同じ Arc で管理されているため安全
        let surface = instance.create_surface(window)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("wgpu アダプターが見つかりません"))?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("nexterm_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // ---- 背景矩形パイプライン ----
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bg_shader"),
            source: wgpu::ShaderSource::Wgsl(BG_SHADER.into()),
        });

        let bg_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("bg_pipeline_layout"),
                bind_group_layouts: &[],
                push_constant_ranges: &[],
            });

        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg_pipeline"),
            layout: Some(&bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bg_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BgVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &bg_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ---- テキストパイプライン ----
        let text_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("text_bind_group_layout"),
                entries: &[
                    // グリフアトラステクスチャ
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // サンプラー
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text_shader"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER.into()),
        });

        let text_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("text_pipeline_layout"),
                bind_group_layouts: &[&text_bind_group_layout],
                push_constant_ranges: &[],
            });

        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text_pipeline"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x4
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // ---- 画像レンダリングパイプライン ----
        let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image_shader"),
            source: wgpu::ShaderSource::Wgsl(IMAGE_SHADER.into()),
        });
        let image_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("image_pipeline_layout"),
                bind_group_layouts: &[&text_bind_group_layout],
                push_constant_ranges: &[],
            });
        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image_pipeline"),
            layout: Some(&image_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &image_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x4
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &image_shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            bg_pipeline,
            text_pipeline,
            text_bind_group_layout,
            image_pipeline,
            image_sampler,
            image_textures: HashMap::new(),
        })
    }

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// 1フレームを描画する
    fn render(
        &mut self,
        state: &ClientState,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
    ) -> Result<()> {
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        let sw = self.surface_config.width as f32;
        let sh = self.surface_config.height as f32;
        let cell_w = font.cell_width();
        let cell_h = font.cell_height();

        let mut bg_verts: Vec<BgVertex> = Vec::new();
        let mut bg_idx: Vec<u16> = Vec::new();
        let mut text_verts: Vec<TextVertex> = Vec::new();
        let mut text_idx: Vec<u16> = Vec::new();

        // レイアウト情報がある場合は全ペインを分割表示する
        if !state.pane_layouts.is_empty() {
            // 各ペインをレイアウト矩形に従って描画する
            let layout_ids: Vec<u32> = state.pane_layouts.keys().copied().collect();
            for pane_id in layout_ids {
                let is_focused = state.focused_pane_id == Some(pane_id);
                if let (Some(layout), Some(pane)) = (
                    state.pane_layouts.get(&pane_id),
                    state.panes.get(&pane_id),
                ) {
                    if pane.scroll_offset > 0 && is_focused {
                        self.build_scrollback_verts_in_rect(
                            pane, layout, sw, sh, cell_w, cell_h, font, atlas,
                            &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
                        );
                    } else {
                        self.build_grid_verts_in_rect(
                            pane, layout, is_focused, sw, sh, cell_w, cell_h, font, atlas,
                            &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
                        );
                    }
                }
            }
            // ペイン境界線を描画する
            self.build_border_verts(state, sw, sh, cell_w, cell_h, &mut bg_verts, &mut bg_idx);
        } else if let Some(pane) = state.focused_pane() {
            // フォールバック: レイアウト情報なし（接続直後など）
            if pane.scroll_offset > 0 {
                // ---- スクロールバック表示モード ----
                self.build_scrollback_verts(
                    pane, sw, sh, cell_w, cell_h, font, atlas,
                    &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
                );
            } else {
                // ---- 通常グリッド表示 ----
                self.build_grid_verts(
                    pane, sw, sh, cell_w, cell_h, font, atlas,
                    &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
                );
            }
        }

        // ---- ステータスライン（常時表示） ----
        self.build_status_verts(
            state, sw, sh, cell_w, cell_h, font, atlas,
            &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
        );

        // ---- 検索バー（アクティブ時） ----
        if state.search.is_active {
            self.build_search_verts(
                state, sw, sh, cell_w, cell_h, font, atlas,
                &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
            );
        }

        // ---- コマンドパレット（オープン時） ----
        if state.palette.is_open {
            self.build_palette_verts(
                state, sw, sh, cell_w, cell_h, font, atlas,
                &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
            );
        }

        // ---- GPU バッファへアップロード ----
        let buf_bg_v = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bg_vertex_buffer"),
            contents: bytemuck::cast_slice(if bg_verts.is_empty() { &[BgVertex { position: [0.0;2], color: [0.0;4] }] } else { &bg_verts }),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let buf_bg_i = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bg_index_buffer"),
            contents: bytemuck::cast_slice(if bg_idx.is_empty() { &[0u16] } else { &bg_idx }),
            usage: wgpu::BufferUsages::INDEX,
        });
        let buf_txt_v = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("text_vertex_buffer"),
            contents: bytemuck::cast_slice(if text_verts.is_empty() { &[TextVertex { position: [0.0;2], uv: [0.0;2], color: [0.0;4] }] } else { &text_verts }),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let buf_txt_i = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("text_index_buffer"),
            contents: bytemuck::cast_slice(if text_idx.is_empty() { &[0u16] } else { &text_idx }),
            usage: wgpu::BufferUsages::INDEX,
        });

        let text_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text_bind_group"),
            layout: &self.text_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });

        // ---- メインレンダーパス（背景 + テキスト） ----
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05, g: 0.05, b: 0.05, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if !bg_idx.is_empty() {
                pass.set_pipeline(&self.bg_pipeline);
                pass.set_vertex_buffer(0, buf_bg_v.slice(..));
                pass.set_index_buffer(buf_bg_i.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..bg_idx.len() as u32, 0, 0..1);
            }
            if !text_idx.is_empty() {
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, &text_bind_group, &[]);
                pass.set_vertex_buffer(0, buf_txt_v.slice(..));
                pass.set_index_buffer(buf_txt_i.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..text_idx.len() as u32, 0, 0..1);
            }
        }

        // ---- 画像レンダーパス（配置済み画像をオーバーレイ） ----
        if let Some(pane) = state.focused_pane() {
            for (id, img) in &pane.images {
                self.ensure_image_texture(*id, img);
            }
            for (id, img) in &pane.images {
                if let Some(entry) = self.image_textures.get(id) {
                    let (img_verts, img_idx) = build_image_verts(img, sw, sh, cell_w, cell_h);
                    let img_vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("img_vbuf"),
                        contents: bytemuck::cast_slice(&img_verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                    let img_ibuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("img_ibuf"),
                        contents: bytemuck::cast_slice(&img_idx),
                        usage: wgpu::BufferUsages::INDEX,
                    });
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("image_render_pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(&self.image_pipeline);
                        pass.set_bind_group(0, &entry.bind_group, &[]);
                        pass.set_vertex_buffer(0, img_vbuf.slice(..));
                        pass.set_index_buffer(img_ibuf.slice(..), wgpu::IndexFormat::Uint16);
                        pass.draw_indexed(0..img_idx.len() as u32, 0, 0..1);
                    }
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    /// グリッドコンテンツの頂点を構築する
    fn build_grid_verts(
        &self,
        pane: &crate::state::PaneState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let grid = &pane.grid;
        for row in 0..grid.height as usize {
            for col in 0..grid.width as usize {
                let Some(cell) = grid.get(col as u16, row as u16) else { continue };
                let px = col as f32 * cell_w;
                let py = row as f32 * cell_h;
                let bg = resolve_color(&cell.bg, false);
                add_px_rect(px, py, cell_w, cell_h, bg, sw, sh, bg_verts, bg_idx);
                if cell.ch == ' ' { continue; }
                let fg = resolve_color(&cell.fg, true);
                let fg_u8 = [
                    (fg[0] * 255.0) as u8, (fg[1] * 255.0) as u8,
                    (fg[2] * 255.0) as u8, (fg[3] * 255.0) as u8,
                ];
                let key = GlyphKey { ch: cell.ch, bold: cell.attrs.is_bold(), italic: cell.attrs.is_italic() };
                let (gw, gh, pixels) = font.rasterize_char(cell.ch, cell.attrs.is_bold(), cell.attrs.is_italic(), fg_u8);
                if gw == 0 || gh == 0 || pixels.is_empty() { continue; }
                let rect = atlas.get_or_insert(key, &pixels, gw, gh, &self.queue);
                let tx0 = px / sw * 2.0 - 1.0;
                let ty0 = 1.0 - py / sh * 2.0;
                let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
                let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
                let base = text_verts.len() as u16;
                text_verts.extend_from_slice(&[
                    TextVertex { position: [tx0, ty0], uv: rect.uv_min, color: fg },
                    TextVertex { position: [tx1, ty0], uv: [rect.uv_max[0], rect.uv_min[1]], color: fg },
                    TextVertex { position: [tx1, ty1], uv: rect.uv_max, color: fg },
                    TextVertex { position: [tx0, ty1], uv: [rect.uv_min[0], rect.uv_max[1]], color: fg },
                ]);
                text_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
            }
        }

        // カーソル矩形（半透明の白いオーバーレイ）
        let cx = pane.cursor_col as f32 * cell_w;
        let cy = pane.cursor_row as f32 * cell_h;
        add_px_rect(cx, cy, cell_w, cell_h, [1.0, 1.0, 1.0, 0.35], sw, sh, bg_verts, bg_idx);
    }

    /// スクロールバックコンテンツの頂点を構築する
    fn build_scrollback_verts(
        &self,
        pane: &crate::state::PaneState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let visible_rows = (sh / cell_h) as usize;
        let offset = pane.scroll_offset;

        for visual_row in 0..visible_rows {
            let sb_row = offset + visual_row;
            let Some(line) = pane.scrollback.get(sb_row) else { continue };
            let py = visual_row as f32 * cell_h;
            for (col, cell) in line.iter().enumerate() {
                let px = col as f32 * cell_w;
                // スクロールバック行は背景を少し暗くする
                let bg = resolve_color(&cell.bg, false);
                let dim_bg = [bg[0] * 0.75, bg[1] * 0.75, bg[2] * 0.75, 1.0];
                add_px_rect(px, py, cell_w, cell_h, dim_bg, sw, sh, bg_verts, bg_idx);
                if cell.ch == ' ' { continue; }
                let fg = resolve_color(&cell.fg, true);
                let fg_u8 = [
                    (fg[0] * 255.0) as u8, (fg[1] * 255.0) as u8,
                    (fg[2] * 255.0) as u8, (fg[3] * 255.0) as u8,
                ];
                let key = GlyphKey { ch: cell.ch, bold: cell.attrs.is_bold(), italic: false };
                let (gw, gh, pixels) = font.rasterize_char(cell.ch, cell.attrs.is_bold(), false, fg_u8);
                if gw == 0 || gh == 0 || pixels.is_empty() { continue; }
                let rect = atlas.get_or_insert(key, &pixels, gw, gh, &self.queue);
                let tx0 = px / sw * 2.0 - 1.0;
                let ty0 = 1.0 - py / sh * 2.0;
                let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
                let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
                let base = text_verts.len() as u16;
                text_verts.extend_from_slice(&[
                    TextVertex { position: [tx0, ty0], uv: rect.uv_min, color: fg },
                    TextVertex { position: [tx1, ty0], uv: [rect.uv_max[0], rect.uv_min[1]], color: fg },
                    TextVertex { position: [tx1, ty1], uv: rect.uv_max, color: fg },
                    TextVertex { position: [tx0, ty1], uv: [rect.uv_min[0], rect.uv_max[1]], color: fg },
                ]);
                text_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
            }
        }
    }

    /// マルチペイン用: レイアウト矩形内にグリッドを描画する
    #[allow(clippy::too_many_arguments)]
    fn build_grid_verts_in_rect(
        &self,
        pane: &crate::state::PaneState,
        layout: &nexterm_proto::PaneLayout,
        is_focused: bool,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let off_x = layout.col_offset as f32 * cell_w;
        let off_y = layout.row_offset as f32 * cell_h;
        // 非フォーカスペインを少し暗く表示する
        let dim = if is_focused { 1.0f32 } else { 0.70f32 };
        let grid = &pane.grid;

        for row in 0..layout.rows.min(grid.height) as usize {
            for col in 0..layout.cols.min(grid.width) as usize {
                let Some(cell) = grid.get(col as u16, row as u16) else { continue };
                let px = off_x + col as f32 * cell_w;
                let py = off_y + row as f32 * cell_h;
                let bg = resolve_color(&cell.bg, false);
                let bg = [bg[0] * dim, bg[1] * dim, bg[2] * dim, 1.0];
                add_px_rect(px, py, cell_w, cell_h, bg, sw, sh, bg_verts, bg_idx);
                if cell.ch == ' ' { continue; }
                let fg = resolve_color(&cell.fg, true);
                let fg = [fg[0] * dim, fg[1] * dim, fg[2] * dim, fg[3]];
                let fg_u8 = [
                    (fg[0] * 255.0) as u8, (fg[1] * 255.0) as u8,
                    (fg[2] * 255.0) as u8, (fg[3] * 255.0) as u8,
                ];
                let key = GlyphKey { ch: cell.ch, bold: cell.attrs.is_bold(), italic: cell.attrs.is_italic() };
                let (gw, gh, pixels) = font.rasterize_char(cell.ch, cell.attrs.is_bold(), cell.attrs.is_italic(), fg_u8);
                if gw == 0 || gh == 0 || pixels.is_empty() { continue; }
                let rect = atlas.get_or_insert(key, &pixels, gw, gh, &self.queue);
                let tx0 = px / sw * 2.0 - 1.0;
                let ty0 = 1.0 - py / sh * 2.0;
                let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
                let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
                let base = text_verts.len() as u16;
                text_verts.extend_from_slice(&[
                    TextVertex { position: [tx0, ty0], uv: rect.uv_min, color: fg },
                    TextVertex { position: [tx1, ty0], uv: [rect.uv_max[0], rect.uv_min[1]], color: fg },
                    TextVertex { position: [tx1, ty1], uv: rect.uv_max, color: fg },
                    TextVertex { position: [tx0, ty1], uv: [rect.uv_min[0], rect.uv_max[1]], color: fg },
                ]);
                text_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
            }
        }

        // カーソル（フォーカスペインのみ）
        if is_focused {
            let cx = off_x + pane.cursor_col as f32 * cell_w;
            let cy = off_y + pane.cursor_row as f32 * cell_h;
            add_px_rect(cx, cy, cell_w, cell_h, [1.0, 1.0, 1.0, 0.35], sw, sh, bg_verts, bg_idx);
        }
    }

    /// マルチペイン用: レイアウト矩形内にスクロールバックを描画する
    #[allow(clippy::too_many_arguments)]
    fn build_scrollback_verts_in_rect(
        &self,
        pane: &crate::state::PaneState,
        layout: &nexterm_proto::PaneLayout,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let off_x = layout.col_offset as f32 * cell_w;
        let off_y = layout.row_offset as f32 * cell_h;
        let offset = pane.scroll_offset;

        for visual_row in 0..layout.rows as usize {
            let sb_row = offset + visual_row;
            let Some(line) = pane.scrollback.get(sb_row) else { continue };
            let py = off_y + visual_row as f32 * cell_h;
            for (col, cell) in line.iter().enumerate().take(layout.cols as usize) {
                let px = off_x + col as f32 * cell_w;
                let bg = resolve_color(&cell.bg, false);
                let dim_bg = [bg[0] * 0.75, bg[1] * 0.75, bg[2] * 0.75, 1.0];
                add_px_rect(px, py, cell_w, cell_h, dim_bg, sw, sh, bg_verts, bg_idx);
                if cell.ch == ' ' { continue; }
                let fg = resolve_color(&cell.fg, true);
                let fg_u8 = [
                    (fg[0] * 255.0) as u8, (fg[1] * 255.0) as u8,
                    (fg[2] * 255.0) as u8, (fg[3] * 255.0) as u8,
                ];
                let key = GlyphKey { ch: cell.ch, bold: cell.attrs.is_bold(), italic: false };
                let (gw, gh, pixels) = font.rasterize_char(cell.ch, cell.attrs.is_bold(), false, fg_u8);
                if gw == 0 || gh == 0 || pixels.is_empty() { continue; }
                let rect = atlas.get_or_insert(key, &pixels, gw, gh, &self.queue);
                let tx0 = px / sw * 2.0 - 1.0;
                let ty0 = 1.0 - py / sh * 2.0;
                let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
                let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
                let base = text_verts.len() as u16;
                text_verts.extend_from_slice(&[
                    TextVertex { position: [tx0, ty0], uv: rect.uv_min, color: fg },
                    TextVertex { position: [tx1, ty0], uv: [rect.uv_max[0], rect.uv_min[1]], color: fg },
                    TextVertex { position: [tx1, ty1], uv: rect.uv_max, color: fg },
                    TextVertex { position: [tx0, ty1], uv: [rect.uv_min[0], rect.uv_max[1]], color: fg },
                ]);
                text_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
            }
        }
    }

    /// ペイン境界線を描画する
    fn build_border_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
    ) {
        if state.pane_layouts.len() <= 1 {
            return;
        }
        let border_color = [0.35, 0.35, 0.42, 1.0];
        let focused_border = [0.30, 0.55, 0.90, 1.0];

        for layout in state.pane_layouts.values() {
            let px = layout.col_offset as f32 * cell_w;
            let py = layout.row_offset as f32 * cell_h;
            let pw = layout.cols as f32 * cell_w;
            let ph = layout.rows as f32 * cell_h;
            let is_focused = state.focused_pane_id == Some(layout.pane_id);
            let color = if is_focused { focused_border } else { border_color };

            // 右隣にペインがあれば垂直境界線を描画する
            let right_col = layout.col_offset + layout.cols + 1;
            if state.pane_layouts.values().any(|o| {
                o.pane_id != layout.pane_id && o.col_offset == right_col
            }) {
                add_px_rect(px + pw, py, cell_w, ph, color, sw, sh, bg_verts, bg_idx);
            }

            // 下隣にペインがあれば水平境界線を描画する
            let bottom_row = layout.row_offset + layout.rows + 1;
            if state.pane_layouts.values().any(|o| {
                o.pane_id != layout.pane_id && o.row_offset == bottom_row
            }) {
                add_px_rect(px, py + ph, pw, cell_h, color, sw, sh, bg_verts, bg_idx);
            }
        }
    }

    /// ステータスライン頂点を構築する（ウィンドウ最下行）
    fn build_status_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let py = sh - cell_h;
        // ステータスライン背景（濃い青）
        add_px_rect(0.0, py, sw, cell_h, [0.12, 0.20, 0.35, 1.0], sw, sh, bg_verts, bg_idx);

        // テキスト: ペイン情報 + アクティビティ通知
        let pane_id = state.focused_pane_id.unwrap_or(0);
        let activity_ids = state.active_pane_ids();
        let status = if activity_ids.is_empty() {
            format!(" nexterm | pane:{}", pane_id)
        } else {
            let ids: Vec<String> = activity_ids.iter().map(|id| id.to_string()).collect();
            format!(" nexterm | pane:{} | activity:{}", pane_id, ids.join(","))
        };

        add_string_verts(
            &status, 0.0, py,
            [0.9, 0.9, 0.9, 1.0], false,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );

        // Lua ステータスバーウィジェットテキストを右側に表示する
        if !state.status_bar_text.is_empty() {
            let widget_text = format!(" {} ", state.status_bar_text);
            let right_px = sw - widget_text.chars().count() as f32 * cell_w;
            add_string_verts(
                &widget_text, right_px, py,
                [0.4, 0.9, 0.6, 1.0], false,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }

        // スクロールバック中はインジケーターをウィジェットの左に表示する
        if let Some(pane) = state.focused_pane() {
            if pane.scroll_offset > 0 {
                let scroll_text = format!(" ↑{} ", pane.scroll_offset);
                let widget_extra =
                    if state.status_bar_text.is_empty() { 0.0 } else {
                        (state.status_bar_text.chars().count() as f32 + 2.0) * cell_w
                    };
                let right_px = sw - scroll_text.chars().count() as f32 * cell_w - widget_extra;
                add_string_verts(
                    &scroll_text, right_px, py,
                    [1.0, 0.85, 0.2, 1.0], true,
                    sw, sh, cell_w, font, atlas, &self.queue,
                    text_verts, text_idx,
                );
            }
        }
    }

    /// 検索バー頂点を構築する（ウィンドウ下部のオーバーレイ）
    fn build_search_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        // ステータスラインの 1 行上に検索バーを表示する
        let py = sh - cell_h * 2.0;
        add_px_rect(0.0, py, sw, cell_h, [0.1, 0.1, 0.1, 1.0], sw, sh, bg_verts, bg_idx);

        let match_info = if let Some(idx) = state.search.current_match {
            format!(" SEARCH: {}  (match:{})", state.search.query, idx)
        } else if state.search.query.is_empty() {
            " SEARCH: ".to_string()
        } else {
            format!(" SEARCH: {}  (no match)", state.search.query)
        };
        add_string_verts(
            &match_info, 0.0, py,
            [0.3, 1.0, 0.5, 1.0], false,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );
    }

    /// コマンドパレット頂点を構築する（中央フローティング）
    fn build_palette_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let palette = &state.palette;
        let items = palette.filtered();
        let palette_cols: f32 = 40.0;
        let palette_rows = (items.len() + 2).min(12) as f32;  // クエリ行 + 最大10アイテム + マージン

        let pw = palette_cols * cell_w;
        let ph = palette_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パレット背景（ダークグレー）
        add_px_rect(px, py, pw, ph, [0.15, 0.15, 0.18, 1.0], sw, sh, bg_verts, bg_idx);
        // 外枠（やや明るい）
        add_px_rect(px, py, pw, 2.0, [0.4, 0.6, 1.0, 1.0], sw, sh, bg_verts, bg_idx);

        // クエリ行
        let query_text = format!("> {}", palette.query);
        add_string_verts(
            &query_text, px + cell_w, py + cell_h * 0.1,
            [1.0, 1.0, 1.0, 1.0], false,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );

        // アクション一覧
        for (i, action) in items.iter().enumerate().take(palette_rows as usize - 1) {
            let item_py = py + cell_h * (i as f32 + 1.2);
            if i == palette.selected {
                // 選択行ハイライト
                add_px_rect(px + 2.0, item_py, pw - 4.0, cell_h, [0.25, 0.40, 0.65, 1.0], sw, sh, bg_verts, bg_idx);
            }
            let prefix = if i == palette.selected { "> " } else { "  " };
            let label = format!("{}{}", prefix, action.label);
            let fg = if i == palette.selected {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.75, 0.75, 0.78, 1.0]
            };
            add_string_verts(
                &label, px + cell_w, item_py,
                fg, i == palette.selected,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }
    }

    /// 画像テクスチャをキャッシュに登録する（初回のみ作成）
    fn ensure_image_texture(&mut self, id: u32, img: &crate::state::PlacedImage) {
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
        self.image_textures.insert(id, ImageEntry { texture, bind_group });
    }
}

/// 配置済み画像の TextVertex リストを構築する
fn build_image_verts(
    img: &crate::state::PlacedImage,
    sw: f32, sh: f32, cell_w: f32, cell_h: f32,
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
        TextVertex { position: [x0, y0], uv: [0.0, 0.0], color: white },
        TextVertex { position: [x1, y0], uv: [1.0, 0.0], color: white },
        TextVertex { position: [x1, y1], uv: [1.0, 1.0], color: white },
        TextVertex { position: [x0, y1], uv: [0.0, 1.0], color: white },
    ];
    let idx = vec![0u16, 1, 2, 0, 2, 3];
    (verts, idx)
}

/// nexterm-proto の Color を RGBA [0, 1] に変換する
fn resolve_color(color: &nexterm_proto::Color, is_fg: bool) -> [f32; 4] {
    use nexterm_proto::Color;
    match color {
        Color::Default => {
            if is_fg {
                [0.85, 0.85, 0.85, 1.0] // デフォルト前景: ライトグレー
            } else {
                [0.05, 0.05, 0.05, 1.0] // デフォルト背景: ほぼ黒
            }
        }
        Color::Rgb(r, g, b) => {
            [*r as f32 / 255.0, *g as f32 / 255.0, *b as f32 / 255.0, 1.0]
        }
        Color::Indexed(n) => ansi_256_to_rgb(*n),
    }
}

/// ANSI 256 色パレット → RGBA [0, 1]
fn ansi_256_to_rgb(n: u8) -> [f32; 4] {
    // 基本 16 色（簡易実装）
    const BASIC: [[f32; 3]; 16] = [
        [0.0, 0.0, 0.0],       // 0: black
        [0.502, 0.0, 0.0],     // 1: red
        [0.0, 0.502, 0.0],     // 2: green
        [0.502, 0.502, 0.0],   // 3: yellow
        [0.0, 0.0, 0.502],     // 4: blue
        [0.502, 0.0, 0.502],   // 5: magenta
        [0.0, 0.502, 0.502],   // 6: cyan
        [0.753, 0.753, 0.753], // 7: white
        [0.502, 0.502, 0.502], // 8: bright black
        [1.0, 0.0, 0.0],       // 9: bright red
        [0.0, 1.0, 0.0],       // 10: bright green
        [1.0, 1.0, 0.0],       // 11: bright yellow
        [0.0, 0.0, 1.0],       // 12: bright blue
        [1.0, 0.0, 1.0],       // 13: bright magenta
        [0.0, 1.0, 1.0],       // 14: bright cyan
        [1.0, 1.0, 1.0],       // 15: bright white
    ];

    if (n as usize) < BASIC.len() {
        let c = BASIC[n as usize];
        return [c[0], c[1], c[2], 1.0];
    }

    // 216 色キューブ（16〜231）
    if n >= 16 && n <= 231 {
        let idx = n - 16;
        let b = (idx % 6) as f32 / 5.0;
        let g = ((idx / 6) % 6) as f32 / 5.0;
        let r = ((idx / 36) % 6) as f32 / 5.0;
        return [r, g, b, 1.0];
    }

    // グレースケール（232〜255）
    let grey = (n - 232) as f32 / 23.0;
    [grey, grey, grey, 1.0]
}

// ---- WGSL シェーダー ----

const BG_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

/// 画像レンダリング用シェーダー（テクスチャ RGBA をそのまま出力）
const IMAGE_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}
@group(0) @binding(0) var img_texture: texture_2d<f32>;
@group(0) @binding(1) var img_sampler: sampler;
@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(img_texture, img_sampler, in.uv);
}
"#;

const TEXT_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@group(0) @binding(0) var glyph_texture: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(glyph_texture, glyph_sampler, in.uv).a;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

// ---- アプリケーション本体 ----

/// GPU アプリケーション（winit EventLoop に渡す）
pub struct NextermApp {
    config: Config,
    state: ClientState,
    font: FontManager,
}

impl NextermApp {
    pub async fn new(config: Config) -> Result<Self> {
        let font = FontManager::new(&config.font.family, config.font.size);
        let state = ClientState::new(80, 24, config.scrollback_lines);
        Ok(Self { config, state, font })
    }

    pub fn into_event_handler(
        self,
        config_rx: Option<tokio::sync::mpsc::Receiver<Config>>,
        config_watcher: Option<notify::RecommendedWatcher>,
        status_eval: Option<StatusBarEvaluator>,
    ) -> EventHandler {
        EventHandler {
            app: self,
            wgpu_state: None,
            atlas: None,
            window: None,
            modifiers: ModifiersState::empty(),
            connection: None,
            cursor_position: None,
            config_rx,
            _config_watcher: config_watcher,
            status_eval,
            last_status_eval: Instant::now(),
        }
    }
}

/// winit のイベントハンドラ
pub struct EventHandler {
    app: NextermApp,
    wgpu_state: Option<WgpuState>,
    atlas: Option<GlyphAtlas>,
    window: Option<Arc<Window>>,
    modifiers: ModifiersState,
    /// サーバーとの IPC 接続
    connection: Option<Connection>,
    /// マウスカーソル位置（ピクセル）
    cursor_position: Option<(f64, f64)>,
    /// 設定ホットリロード受信チャネル
    config_rx: Option<tokio::sync::mpsc::Receiver<Config>>,
    /// ファイル監視ウォッチャー（Drop されると停止するため保持する）
    _config_watcher: Option<notify::RecommendedWatcher>,
    /// Lua ステータスバー評価器
    status_eval: Option<StatusBarEvaluator>,
    /// ステータスバーの最終評価時刻
    last_status_eval: Instant,
}

impl ApplicationHandler for EventHandler {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        // PTY 出力を 16ms ごとにポーリングする（約 60fps）
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(16),
        ));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ウィンドウを作成する
        let attrs = Window::default_attributes()
            .with_title("nexterm")
            .with_inner_size(PhysicalSize::new(1280u32, 800u32));

        let window = Arc::new(event_loop.create_window(attrs).expect("ウィンドウ作成失敗"));

        // wgpu を非同期で初期化する（tokio runtime が必要）
        let wgpu_state = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(WgpuState::new(Arc::clone(&window)))
        })
        .expect("wgpu 初期化失敗");

        let atlas = GlyphAtlas::new(&wgpu_state.device);

        // ウィンドウサイズからセル数を計算してステートを初期化する
        let size = window.inner_size();
        let cols = (size.width as f32 / self.app.font.cell_width()).max(1.0) as u16;
        let rows = (size.height as f32 / self.app.font.cell_height()).max(1.0) as u16;
        self.app.state.resize(cols, rows);

        self.window = Some(Arc::clone(&window));
        self.atlas = Some(atlas);
        self.wgpu_state = Some(wgpu_state);

        // サーバーに接続してデフォルトセッションにアタッチする
        let conn = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                match Connection::connect().await {
                    Ok(conn) => {
                        // セッションにアタッチ → 実際のサイズを通知
                        let _ = conn.send_tx.try_send(ClientToServer::Attach {
                            session_name: "main".to_string(),
                        });
                        let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
                        info!("nexterm サーバーに接続しました");
                        Some(conn)
                    }
                    Err(e) => {
                        warn!("サーバーへの接続に失敗しました（オフラインモード）: {}", e);
                        None
                    }
                }
            })
        });
        self.connection = conn;

        info!("wgpu レンダラーを初期化しました");
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // サーバーからのメッセージをポーリングして状態を更新する
        let mut had_messages = false;
        if let Some(conn) = &mut self.connection {
            while let Ok(msg) = conn.recv_rx.try_recv() {
                self.app.state.apply_server_message(msg);
                had_messages = true;
            }
        }

        // 設定ホットリロードをポーリングする（最新の設定を適用する）
        if let Some(rx) = &mut self.config_rx {
            if let Ok(new_config) = rx.try_recv() {
                info!("設定を再ロードしました: フォント={} {}pt", new_config.font.family, new_config.font.size);
                // フォントサイズ変更時はグリフアトラスも再生成する
                let font_changed = self.app.config.font != new_config.font;
                self.app.config = new_config;
                if font_changed {
                    self.app.font = crate::font::FontManager::new(
                        &self.app.config.font.family,
                        self.app.config.font.size,
                    );
                    if let Some(wgpu) = &self.wgpu_state {
                        self.atlas = Some(GlyphAtlas::new(&wgpu.device));
                    }
                }
                had_messages = true;
            }
        }

        // Lua ステータスバーを 1 秒ごとに再評価してキャッシュを更新する
        if self.app.config.status_bar.enabled
            && !self.app.config.status_bar.widgets.is_empty()
            && self.last_status_eval.elapsed() >= Duration::from_secs(1)
        {
            if let Some(eval) = &self.status_eval {
                self.app.state.status_bar_text =
                    eval.evaluate_widgets(&self.app.config.status_bar.widgets);
                self.last_status_eval = Instant::now();
                had_messages = true;
            }
        }

        if had_messages {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                let cols = (size.width as f32 / self.app.font.cell_width()).max(1.0) as u16;
                let rows = (size.height as f32 / self.app.font.cell_height()).max(1.0) as u16;
                if let Some(wgpu) = &mut self.wgpu_state {
                    wgpu.resize(size);
                }
                self.app.state.resize(cols, rows);
                // サーバーにリサイズを通知する
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
                }
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            // マウスカーソル位置を追跡する
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some((position.x, position.y));
            }

            // 左クリックでペインフォーカスを切り替える
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } => {
                if let Some((px, py)) = self.cursor_position {
                    let cell_w = self.app.font.cell_width() as f64;
                    let cell_h = self.app.font.cell_height() as f64;
                    let click_col = (px / cell_w) as u16;
                    let click_row = (py / cell_h) as u16;
                    // クリック座標が含まれるペインを探してフォーカスを移動する
                    let target_pane = self
                        .app
                        .state
                        .pane_layouts
                        .values()
                        .find(|l| {
                            click_col >= l.col_offset
                                && click_col < l.col_offset + l.cols
                                && click_row >= l.row_offset
                                && click_row < l.row_offset + l.rows
                        })
                        .map(|l| l.pane_id);
                    if let Some(pane_id) = target_pane {
                        if self.app.state.focused_pane_id != Some(pane_id) {
                            if let Some(conn) = &self.connection {
                                let _ = conn
                                    .send_tx
                                    .try_send(ClientToServer::FocusPane { pane_id });
                            }
                        }
                    }
                }
            }

            // マウスホイールでスクロールバックをスクロールする
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => {
                        (p.y / self.app.font.cell_height() as f64 * 3.0) as i32
                    }
                };
                if lines > 0 {
                    self.app.state.scroll_up(lines as usize);
                } else if lines < 0 {
                    self.app.state.scroll_down((-lines) as usize);
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key,
                        state: ElementState::Pressed,
                        text,
                        ..
                    },
                ..
            } => {
                // 検索モードの文字入力を処理する（PTY には転送しない）
                if self.app.state.search.is_active {
                    if matches!(physical_key, PhysicalKey::Code(WKeyCode::Backspace)) {
                        self.app.state.pop_search_char();
                    } else if let Some(ref t) = text {
                        if !self.modifiers.control_key() {
                            for ch in t.chars() {
                                self.app.state.push_search_char(ch);
                            }
                        }
                    }
                    // Escape / Enter は handle_key で処理する
                    if let PhysicalKey::Code(code) = physical_key {
                        if matches!(code, WKeyCode::Escape | WKeyCode::Enter) {
                            self.handle_key(code, event_loop);
                        }
                    }
                    return;
                }

                // ローカル操作（パレット・検索開始など）をチェックする
                let consumed = if let PhysicalKey::Code(code) = physical_key {
                    self.handle_key(code, event_loop)
                } else {
                    false
                };

                // ローカルで消費されなかった場合はサーバーへ転送する
                if !consumed {
                    self.forward_key_to_server(physical_key, text.as_deref());
                }
            }

            WindowEvent::RedrawRequested => {
                if let (Some(wgpu), Some(atlas)) =
                    (&mut self.wgpu_state, &mut self.atlas)
                {
                    if let Err(e) =
                        wgpu.render(&self.app.state, &mut self.app.font, atlas)
                    {
                        warn!("描画エラー: {}", e);
                    }
                }
            }

            _ => {}
        }

        // 毎フレーム再描画をリクエストする
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl EventHandler {
    /// キーを処理してローカルで消費した場合は true を返す
    fn handle_key(&mut self, code: WKeyCode, event_loop: &ActiveEventLoop) -> bool {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        // Ctrl+Shift+V: クリップボードからペーストする
        if ctrl && shift && code == WKeyCode::KeyV {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                if let Ok(text) = clipboard.get_text() {
                    if let Some(conn) = &self.connection {
                        let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                    }
                }
            }
            return true;
        }

        // Ctrl+Shift+C: フォーカスペインの可視グリッドをクリップボードにコピーする
        if ctrl && shift && code == WKeyCode::KeyC {
            if let Some(pane) = self.app.state.focused_pane() {
                let text = grid_to_text(pane);
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
            }
            return true;
        }

        // Ctrl+Shift+P: コマンドパレットのトグル
        if ctrl && shift && code == WKeyCode::KeyP {
            if self.app.state.palette.is_open {
                self.app.state.palette.close();
            } else {
                self.app.state.palette.open();
            }
            return true;
        }

        // Ctrl+F: スクロールバック検索を開始する
        if ctrl && code == WKeyCode::KeyF {
            self.app.state.start_search();
            return true;
        }

        // PageUp / PageDown: スクロールバックをスクロールする
        if code == WKeyCode::PageUp {
            let scroll_lines = self.app.state.rows as usize / 2;
            self.app.state.scroll_up(scroll_lines);
            return true;
        }
        if code == WKeyCode::PageDown {
            let scroll_lines = self.app.state.rows as usize / 2;
            self.app.state.scroll_down(scroll_lines);
            return true;
        }

        // Escape: 検索・パレットを閉じる
        if code == WKeyCode::Escape {
            if self.app.state.palette.is_open {
                self.app.state.palette.close();
                return true;
            } else if self.app.state.search.is_active {
                self.app.state.end_search();
                return true;
            }
            // パレット・検索が開いていなければ PTY に転送する
            return false;
        }

        // パレットが開いているときのナビゲーション（全キーを消費）
        if self.app.state.palette.is_open {
            match code {
                WKeyCode::ArrowDown => self.app.state.palette.select_next(),
                WKeyCode::ArrowUp => self.app.state.palette.select_prev(),
                WKeyCode::Enter => {
                    if let Some(action) = self.app.state.palette.selected_action() {
                        let action_id = action.action.clone();
                        self.app.state.palette.close();
                        self.execute_action(&action_id, event_loop);
                    }
                }
                _ => {}
            }
            return true;
        }

        // 検索モードの特殊キー
        if self.app.state.search.is_active {
            if code == WKeyCode::Enter {
                self.app.state.search_next();
                return true;
            }
            // 他のキーは消費しない（上の search.is_active ブロックで処理済み）
        }

        false
    }

    fn execute_action(&mut self, action: &str, event_loop: &ActiveEventLoop) {
        match action {
            "Quit" => event_loop.exit(),
            "SearchScrollback" => self.app.state.start_search(),
            _ => debug!("アクション実行: {}", action),
        }
    }

    /// キー入力をサーバーの PTY に転送する
    fn forward_key_to_server(&self, physical_key: PhysicalKey, text: Option<&str>) {
        let Some(conn) = &self.connection else { return };
        let mods = proto_modifiers(self.modifiers);
        let ctrl = self.modifiers.control_key();

        // Ctrl 非押下でテキストがある場合はテキスト入力として送信する
        if !ctrl {
            if let Some(text_str) = text {
                if !text_str.is_empty() {
                    for ch in text_str.chars() {
                        let _ = conn.send_tx.try_send(ClientToServer::KeyEvent {
                            code: ProtoKeyCode::Char(ch),
                            modifiers: mods,
                        });
                    }
                    return;
                }
            }
        }

        // 特殊キーおよび Ctrl キーシーケンス
        if let Some(key_code) = physical_to_proto_key(physical_key, self.modifiers) {
            let _ = conn.send_tx.try_send(ClientToServer::KeyEvent {
                code: key_code,
                modifiers: mods,
            });
        }
    }
}

/// winit の物理キーを nexterm_proto の KeyCode に変換する
fn physical_to_proto_key(key: PhysicalKey, mods: ModifiersState) -> Option<ProtoKeyCode> {
    let ctrl = mods.control_key();
    let PhysicalKey::Code(code) = key else { return None };

    match code {
        WKeyCode::Enter => Some(ProtoKeyCode::Enter),
        WKeyCode::Backspace => Some(ProtoKeyCode::Backspace),
        WKeyCode::Delete => Some(ProtoKeyCode::Delete),
        WKeyCode::Escape => Some(ProtoKeyCode::Escape),
        WKeyCode::Tab => {
            if mods.shift_key() {
                Some(ProtoKeyCode::BackTab)
            } else {
                Some(ProtoKeyCode::Tab)
            }
        }
        WKeyCode::ArrowUp => Some(ProtoKeyCode::Up),
        WKeyCode::ArrowDown => Some(ProtoKeyCode::Down),
        WKeyCode::ArrowLeft => Some(ProtoKeyCode::Left),
        WKeyCode::ArrowRight => Some(ProtoKeyCode::Right),
        WKeyCode::Home => Some(ProtoKeyCode::Home),
        WKeyCode::End => Some(ProtoKeyCode::End),
        WKeyCode::PageUp => Some(ProtoKeyCode::PageUp),
        WKeyCode::PageDown => Some(ProtoKeyCode::PageDown),
        WKeyCode::Insert => Some(ProtoKeyCode::Insert),
        WKeyCode::F1 => Some(ProtoKeyCode::F(1)),
        WKeyCode::F2 => Some(ProtoKeyCode::F(2)),
        WKeyCode::F3 => Some(ProtoKeyCode::F(3)),
        WKeyCode::F4 => Some(ProtoKeyCode::F(4)),
        WKeyCode::F5 => Some(ProtoKeyCode::F(5)),
        WKeyCode::F6 => Some(ProtoKeyCode::F(6)),
        WKeyCode::F7 => Some(ProtoKeyCode::F(7)),
        WKeyCode::F8 => Some(ProtoKeyCode::F(8)),
        WKeyCode::F9 => Some(ProtoKeyCode::F(9)),
        WKeyCode::F10 => Some(ProtoKeyCode::F(10)),
        WKeyCode::F11 => Some(ProtoKeyCode::F(11)),
        WKeyCode::F12 => Some(ProtoKeyCode::F(12)),
        // Ctrl+文字: text が None のケース（OS がテキストを生成しない場合）
        c if ctrl => winit_code_to_char(c).map(ProtoKeyCode::Char),
        _ => None,
    }
}

/// winit のキーコードを英小文字に変換する（Ctrl シーケンス用）
fn winit_code_to_char(code: WKeyCode) -> Option<char> {
    match code {
        WKeyCode::KeyA => Some('a'),
        WKeyCode::KeyB => Some('b'),
        WKeyCode::KeyC => Some('c'),
        WKeyCode::KeyD => Some('d'),
        WKeyCode::KeyE => Some('e'),
        WKeyCode::KeyF => Some('f'),
        WKeyCode::KeyG => Some('g'),
        WKeyCode::KeyH => Some('h'),
        WKeyCode::KeyI => Some('i'),
        WKeyCode::KeyJ => Some('j'),
        WKeyCode::KeyK => Some('k'),
        WKeyCode::KeyL => Some('l'),
        WKeyCode::KeyM => Some('m'),
        WKeyCode::KeyN => Some('n'),
        WKeyCode::KeyO => Some('o'),
        WKeyCode::KeyP => Some('p'),
        WKeyCode::KeyQ => Some('q'),
        WKeyCode::KeyR => Some('r'),
        WKeyCode::KeyS => Some('s'),
        WKeyCode::KeyT => Some('t'),
        WKeyCode::KeyU => Some('u'),
        WKeyCode::KeyV => Some('v'),
        WKeyCode::KeyW => Some('w'),
        WKeyCode::KeyX => Some('x'),
        WKeyCode::KeyY => Some('y'),
        WKeyCode::KeyZ => Some('z'),
        _ => None,
    }
}

/// winit の ModifiersState を nexterm_proto の Modifiers に変換する
fn proto_modifiers(state: ModifiersState) -> nexterm_proto::Modifiers {
    let mut bits = 0u8;
    if state.shift_key() {
        bits |= nexterm_proto::Modifiers::SHIFT;
    }
    if state.control_key() {
        bits |= nexterm_proto::Modifiers::CTRL;
    }
    if state.alt_key() {
        bits |= nexterm_proto::Modifiers::ALT;
    }
    if state.super_key() {
        bits |= nexterm_proto::Modifiers::META;
    }
    nexterm_proto::Modifiers(bits)
}

// wgpu::util のためのインポート
use wgpu::util::DeviceExt;

// ---- 画像テクスチャエントリ ----

/// GPU 画像テクスチャのキャッシュエントリ
struct ImageEntry {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

// ---- ヘルパー関数 ----

/// ペインのグリッド内容をプレーンテキストに変換する（Ctrl+Shift+C コピー用）
fn grid_to_text(pane: &crate::state::PaneState) -> String {
    let mut lines = Vec::with_capacity(pane.grid.rows.len());
    for row in &pane.grid.rows {
        let line: String = row.iter().map(|c| c.ch).collect();
        // 行末の空白を除去して返す
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

/// NDC 矩形の背景頂点4つを追加する（三角形インデックスも追加）
fn add_rect_verts(
    x0: f32, y0: f32, x1: f32, y1: f32,
    color: [f32; 4],
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    let base = bg_verts.len() as u16;
    bg_verts.extend_from_slice(&[
        BgVertex { position: [x0, y0], color },
        BgVertex { position: [x1, y0], color },
        BgVertex { position: [x1, y1], color },
        BgVertex { position: [x0, y1], color },
    ]);
    bg_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
}

/// ピクセル矩形を NDC に変換して背景頂点バッファに追加する
fn add_px_rect(
    px: f32, py: f32, pw: f32, ph: f32,
    color: [f32; 4],
    sw: f32, sh: f32,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) {
    let x0 = px / sw * 2.0 - 1.0;
    let y0 = 1.0 - py / sh * 2.0;
    let x1 = (px + pw) / sw * 2.0 - 1.0;
    let y1 = 1.0 - (py + ph) / sh * 2.0;
    add_rect_verts(x0, y0, x1, y1, color, bg_verts, bg_idx);
}

/// 1文字をテキスト頂点バッファに追加する
fn add_char_verts(
    ch: char,
    px: f32, py: f32,
    fg: [f32; 4],
    bold: bool,
    sw: f32, sh: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    if ch == ' ' {
        return;
    }
    let key = GlyphKey { ch, bold, italic: false };
    let fg_u8 = [
        (fg[0] * 255.0) as u8,
        (fg[1] * 255.0) as u8,
        (fg[2] * 255.0) as u8,
        255u8,
    ];
    let (gw, gh, pixels) = font.rasterize_char(ch, bold, false, fg_u8);
    if gw == 0 || gh == 0 || pixels.is_empty() {
        return;
    }
    let rect = atlas.get_or_insert(key, &pixels, gw, gh, queue);
    let tx0 = px / sw * 2.0 - 1.0;
    let ty0 = 1.0 - py / sh * 2.0;
    let tx1 = (px + gw as f32) / sw * 2.0 - 1.0;
    let ty1 = 1.0 - (py + gh as f32) / sh * 2.0;
    let base = text_verts.len() as u16;
    text_verts.extend_from_slice(&[
        TextVertex { position: [tx0, ty0], uv: rect.uv_min, color: fg },
        TextVertex { position: [tx1, ty0], uv: [rect.uv_max[0], rect.uv_min[1]], color: fg },
        TextVertex { position: [tx1, ty1], uv: rect.uv_max, color: fg },
        TextVertex { position: [tx0, ty1], uv: [rect.uv_min[0], rect.uv_max[1]], color: fg },
    ]);
    text_idx.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
}

/// 文字列をテキスト頂点バッファに追加する（各文字 cell_w 幅で等幅配置）
#[allow(clippy::too_many_arguments)]
fn add_string_verts(
    text: &str,
    px: f32, py: f32,
    fg: [f32; 4],
    bold: bool,
    sw: f32, sh: f32,
    cell_w: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    for (i, ch) in text.chars().enumerate() {
        add_char_verts(
            ch, px + i as f32 * cell_w, py, fg, bold,
            sw, sh, font, atlas, queue, text_verts, text_idx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi256_基本16色が変換できる() {
        let black = ansi_256_to_rgb(0);
        assert_eq!(black, [0.0, 0.0, 0.0, 1.0]);
        let white = ansi_256_to_rgb(15);
        assert_eq!(white, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn ansi256_グレースケールが変換できる() {
        let grey = ansi_256_to_rgb(232);
        assert_eq!(grey, [0.0, 0.0, 0.0, 1.0]);
        let bright = ansi_256_to_rgb(255);
        assert_eq!(bright, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn デフォルト色の解決() {
        let fg = resolve_color(&nexterm_proto::Color::Default, true);
        assert!(fg[0] > 0.5); // 前景は明るい
        let bg = resolve_color(&nexterm_proto::Color::Default, false);
        assert!(bg[0] < 0.5); // 背景は暗い
    }
}
