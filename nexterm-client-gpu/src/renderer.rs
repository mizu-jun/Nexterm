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
    event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow},
    keyboard::{KeyCode as WKeyCode, ModifiersState, PhysicalKey},
    window::{Window, WindowId},
};

use crate::{
    connection::Connection,
    font::FontManager,
    state::{ClientState, ContextMenu, ContextMenuAction},
};

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
    #[allow(dead_code)]
    width: u32,
    #[allow(dead_code)]
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
            .ok_or_else(|| anyhow::anyhow!("{}", nexterm_i18n::fl!("gpu-adapter-not-found")))?;

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
            // 透過合成のために PreMultiplied を優先する（非対応時は最初のモードにフォールバック）
            alpha_mode: surface_caps
                .alpha_modes
                .iter()
                .copied()
                .find(|m| *m == wgpu::CompositeAlphaMode::PreMultiplied)
                .unwrap_or(surface_caps.alpha_modes[0]),
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
        tab_bar_cfg: &nexterm_config::TabBarConfig,
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
                            pane, layout, is_focused, &state.mouse_sel,
                            sw, sh, cell_w, cell_h, font, atlas,
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
                    pane, &state.mouse_sel, sw, sh, cell_w, cell_h, font, atlas,
                    &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
                );
            }
        }

        // ---- ペイン番号オーバーレイ（display_panes_mode 有効時） ----
        if state.display_panes_mode {
            let mut sorted_pane_ids: Vec<u32> = state.pane_layouts.keys().copied().collect();
            sorted_pane_ids.sort();
            for (number, pane_id) in sorted_pane_ids.iter().enumerate() {
                if let Some(layout) = state.pane_layouts.get(pane_id) {
                    let px = layout.col_offset as f32 * cell_w;
                    let py = layout.row_offset as f32 * cell_h;
                    let badge_w = cell_w * 2.0;
                    let badge_h = cell_h;
                    // 黄色背景バッジ
                    add_px_rect(px, py, badge_w, badge_h, [0.9, 0.75, 0.0, 0.90], sw, sh, &mut bg_verts, &mut bg_idx);
                    // ペイン番号テキスト（1 始まり）
                    let label = (number + 1).to_string();
                    add_string_verts(
                        &label, px + 2.0, py,
                        [0.0, 0.0, 0.0, 1.0], true,
                        sw, sh, cell_w, font, atlas, &self.queue,
                        &mut text_verts, &mut text_idx,
                    );
                }
            }
            // レイアウト情報がない場合（フォールバック: フォーカスペインのみ）
            if state.pane_layouts.is_empty()
                && let Some(focused_id) = state.focused_pane_id {
                    add_px_rect(0.0, 0.0, cell_w * 2.0, cell_h, [0.9, 0.75, 0.0, 0.90], sw, sh, &mut bg_verts, &mut bg_idx);
                    let label = focused_id.to_string();
                    add_string_verts(
                        &label, 2.0, 0.0,
                        [0.0, 0.0, 0.0, 1.0], true,
                        sw, sh, cell_w, font, atlas, &self.queue,
                        &mut text_verts, &mut text_idx,
                    );
                }
        }

        // ---- タブバー（設定で有効な場合）----
        if tab_bar_cfg.enabled {
            self.build_tab_bar_verts(
                state, tab_bar_cfg, sw, sh, cell_w, cell_h, font, atlas,
                &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
            );
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

        // ---- Quick Select オーバーレイ（アクティブ時） ----
        if state.quick_select.is_active {
            self.build_quick_select_verts(
                state, sw, sh, cell_w, cell_h, font, atlas,
                &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
            );
        }

        // ---- SFTP ファイル転送ダイアログ（オープン時） ----
        if state.file_transfer.is_open {
            self.build_file_transfer_verts(
                state, sw, sh, cell_w, cell_h, font, atlas,
                &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
            );
        }

        // ---- Lua マクロピッカー（オープン時） ----
        if state.macro_picker.is_open {
            self.build_macro_picker_verts(
                state, sw, sh, cell_w, cell_h, font, atlas,
                &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
            );
        }

        // ---- ホストマネージャ（オープン時） ----
        if state.host_manager.is_open {
            self.build_host_manager_verts(
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

        // ---- 設定パネル（Ctrl+, でオープン） ----
        if state.settings_panel.is_open {
            self.build_settings_panel_verts(
                state, sw, sh, cell_w, cell_h, font, atlas,
                &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
            );
        }

        // ---- コンテキストメニュー（右クリック時） ----
        if let Some(ref menu) = state.context_menu {
            self.build_context_menu_verts(
                menu, sw, sh, cell_w, cell_h, font, atlas,
                &mut bg_verts, &mut bg_idx, &mut text_verts, &mut text_idx,
            );
        }

        // ---- IME プリエディットオーバーレイ（変換中テキスト） ----
        if let Some(ref preedit) = state.ime_preedit
            && let Some(pane) = state.focused_pane() {
                let px = pane.cursor_col as f32 * cell_w;
                let py = (pane.cursor_row + 1) as f32 * cell_h;
                // プリエディット背景（やや明るいグレー）
                let text_width = preedit.chars().count() as f32 * cell_w;
                add_px_rect(px, py, text_width.max(cell_w), cell_h, [0.25, 0.25, 0.30, 0.90], sw, sh, &mut bg_verts, &mut bg_idx);
                // アンダーライン（黄色）
                add_px_rect(px, py + cell_h - 2.0, text_width.max(cell_w), 2.0, [1.0, 0.85, 0.2, 1.0], sw, sh, &mut bg_verts, &mut bg_idx);
                // プリエディットテキスト
                add_string_verts(
                    preedit, px, py,
                    [1.0, 1.0, 0.6, 1.0], false,
                    sw, sh, cell_w, font, atlas, &self.queue,
                    &mut text_verts, &mut text_idx,
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
                            r: 0.0, g: 0.0, b: 0.0, a: 0.0,
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
    #[allow(clippy::too_many_arguments)]
    fn build_grid_verts(
        &self,
        pane: &crate::state::PaneState,
        mouse_sel: &crate::state::MouseSelection,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        // 選択ハイライト色（半透明の青）
        const SEL_COLOR: [f32; 4] = [0.25, 0.55, 1.0, 0.40];

        let grid = &pane.grid;
        for row in 0..grid.height as usize {
            for col in 0..grid.width as usize {
                let Some(cell) = grid.get(col as u16, row as u16) else { continue };
                let px = col as f32 * cell_w;
                let py = row as f32 * cell_h;
                let bg = resolve_color(&cell.bg, false);
                add_px_rect(px, py, cell_w, cell_h, bg, sw, sh, bg_verts, bg_idx);
                // 選択ハイライトオーバーレイ
                if mouse_sel.contains(col as u16, row as u16) {
                    add_px_rect(px, py, cell_w, cell_h, SEL_COLOR, sw, sh, bg_verts, bg_idx);
                }
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
    #[allow(clippy::too_many_arguments)]
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
        mouse_sel: &crate::state::MouseSelection,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        // 選択ハイライト色（半透明の青）
        const SEL_COLOR: [f32; 4] = [0.25, 0.55, 1.0, 0.40];

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
                // 選択ハイライトオーバーレイ（フォーカスペインのみ）
                if is_focused && mouse_sel.contains(col as u16, row as u16) {
                    add_px_rect(px, py, cell_w, cell_h, SEL_COLOR, sw, sh, bg_verts, bg_idx);
                }
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
    #[allow(clippy::too_many_arguments)]
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

    /// タブバー頂点を構築する（ウィンドウ最上行、WezTerm スタイル）
    #[allow(clippy::too_many_arguments)]
    fn build_tab_bar_verts(
        &self,
        state: &ClientState,
        cfg: &nexterm_config::TabBarConfig,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let bar_h = cfg.height as f32;
        let bar_y = 0.0_f32;

        // タブバー全体の背景（非アクティブ色）
        let inactive_bg = hex_to_rgba(&cfg.inactive_tab_bg, 1.0);
        add_px_rect(0.0, bar_y, sw, bar_h, inactive_bg, sw, sh, bg_verts, bg_idx);

        // フォーカスペインの ID で「アクティブタブ」を表示する
        let focused_id = state.focused_pane_id.unwrap_or(0);
        let active_bg = hex_to_rgba(&cfg.active_tab_bg, 1.0);
        let text_fg = [0.95, 0.95, 0.95, 1.0];
        let inactive_fg = [0.65, 0.65, 0.65, 1.0];

        let mut x_offset = 0.0_f32;
        let padding = cell_w;
        let sep = &cfg.separator;

        // ペイン ID 順にタブを並べる
        let mut pane_ids: Vec<u32> = state.pane_layouts.keys().copied().collect();
        pane_ids.sort();

        for (i, &pane_id) in pane_ids.iter().enumerate() {
            let is_active = pane_id == focused_id;
            let label = format!(" pane:{} ", pane_id);
            let label_w = label.chars().count() as f32 * cell_w + padding * 2.0;
            let tab_bg = if is_active { active_bg } else { inactive_bg };

            // タブ背景
            add_px_rect(x_offset, bar_y, label_w, bar_h, tab_bg, sw, sh, bg_verts, bg_idx);

            // タブラベル（垂直中央揃え）
            let text_y = bar_y + (bar_h - cell_h) / 2.0;
            let fg = if is_active { text_fg } else { inactive_fg };
            add_string_verts(
                &label, x_offset + padding, text_y, fg, is_active,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );

            x_offset += label_w;

            // セパレータ（最後のタブの後は不要）
            if i + 1 < pane_ids.len() {
                let next_is_active = pane_ids[i + 1] == focused_id;
                // セパレータの色: 現在タブの背景から次のタブへの遷移を表現する
                let sep_bg = if is_active || next_is_active { active_bg } else { inactive_bg };
                let sep_w = cell_w;
                add_px_rect(x_offset, bar_y, sep_w, bar_h, sep_bg, sw, sh, bg_verts, bg_idx);
                add_string_verts(
                    sep, x_offset, text_y, text_fg, false,
                    sw, sh, cell_w, font, atlas, &self.queue,
                    text_verts, text_idx,
                );
                x_offset += sep_w;
            }
        }
    }

    /// ステータスライン頂点を構築する（ウィンドウ最下行）
    #[allow(clippy::too_many_arguments)]
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

        // 右端インジケーター群（右から左へ積み上げる）
        let mut right_offset = if state.status_bar_text.is_empty() { 0.0 } else {
            (state.status_bar_text.chars().count() as f32 + 2.0) * cell_w
        };

        // ズームインジケーター（[Z] ラベルを黄色で表示）
        if state.is_zoomed {
            let zoom_text = " [Z] ";
            right_offset += zoom_text.chars().count() as f32 * cell_w;
            let right_px = sw - right_offset;
            add_string_verts(
                zoom_text, right_px, py,
                [1.0, 0.85, 0.2, 1.0], true,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }

        // スクロールバック中はインジケーターをウィジェットの左に表示する
        if let Some(pane) = state.focused_pane()
            && pane.scroll_offset > 0 {
                let scroll_text = format!(" ↑{} ", pane.scroll_offset);
                let right_px = sw - scroll_text.chars().count() as f32 * cell_w - right_offset;
                add_string_verts(
                    &scroll_text, right_px, py,
                    [1.0, 0.85, 0.2, 1.0], true,
                    sw, sh, cell_w, font, atlas, &self.queue,
                    text_verts, text_idx,
                );
            }
    }

    /// 検索バー頂点を構築する（ウィンドウ下部のオーバーレイ）
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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

    /// 設定パネル頂点を構築する（Ctrl+, でオープン）
    ///
    /// タブ 0=Font, 1=Colors, 2=Window のパネルを表示する。
    #[allow(clippy::too_many_arguments)]
    fn build_settings_panel_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let sp = &state.settings_panel;
        if !sp.is_open {
            return;
        }

        let panel_cols: f32 = 44.0;
        let panel_rows: f32 = 10.0;
        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景
        add_px_rect(px, py, pw, ph, [0.12, 0.13, 0.16, 0.97], sw, sh, bg_verts, bg_idx);
        // 上端のアクセント線
        add_px_rect(px, py, pw, 2.0, [0.4, 0.7, 1.0, 1.0], sw, sh, bg_verts, bg_idx);

        // タイトル
        add_string_verts(
            "Settings", px + cell_w, py + cell_h * 0.1,
            [1.0, 1.0, 1.0, 1.0], true,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );

        // タブバー
        let tab_labels = ["[Font]", "[Colors]", "[Window]"];
        for (i, label) in tab_labels.iter().enumerate() {
            let tab_x = px + cell_w * (i as f32 * 10.0 + 12.0);
            let tab_y = py + cell_h * 0.1;
            let is_active = sp.tab == i;
            if is_active {
                add_px_rect(tab_x - cell_w * 0.3, tab_y, label.len() as f32 * cell_w + cell_w * 0.6, cell_h, [0.25, 0.45, 0.75, 1.0], sw, sh, bg_verts, bg_idx);
            }
            let fg = if is_active { [1.0, 1.0, 1.0, 1.0] } else { [0.6, 0.65, 0.7, 1.0] };
            add_string_verts(
                label, tab_x, tab_y,
                fg, is_active,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }

        // タブ区切り線
        add_px_rect(px, py + cell_h * 1.2, pw, 1.0, [0.30, 0.30, 0.35, 1.0], sw, sh, bg_verts, bg_idx);

        // タブコンテンツ
        let content_y = py + cell_h * 1.5;
        match sp.tab {
            0 => {
                // Font タブ
                let family_line = format!("  Font: {}", sp.font_family);
                add_string_verts(
                    &family_line, px + cell_w, content_y,
                    [0.8, 0.85, 0.9, 1.0], false,
                    sw, sh, cell_w, font, atlas, &self.queue,
                    text_verts, text_idx,
                );
                let size_line = format!("  Size: {:.1}pt   (↑/↓ to change)", sp.font_size);
                add_string_verts(
                    &size_line, px + cell_w, content_y + cell_h * 1.2,
                    [0.9, 0.95, 1.0, 1.0], false,
                    sw, sh, cell_w, font, atlas, &self.queue,
                    text_verts, text_idx,
                );
            }
            1 => {
                // Colors タブ
                let scheme_line = format!("  Scheme: {}   (←/→ to change)", sp.scheme_name());
                add_string_verts(
                    &scheme_line, px + cell_w, content_y,
                    [0.9, 0.95, 1.0, 1.0], false,
                    sw, sh, cell_w, font, atlas, &self.queue,
                    text_verts, text_idx,
                );
                // スキームドット（5個）
                let dot_y = content_y + cell_h * 1.4;
                let schemes_colors: [[f32; 4]; 5] = [
                    [0.15, 0.15, 0.18, 1.0], // dark
                    [0.95, 0.95, 0.92, 1.0], // light
                    [0.10, 0.10, 0.20, 1.0], // tokyonight
                    [0.00, 0.17, 0.21, 1.0], // solarized
                    [0.28, 0.26, 0.22, 1.0], // gruvbox
                ];
                for (i, &col) in schemes_colors.iter().enumerate() {
                    let dot_x = px + cell_w * (2.0 + i as f32 * 3.0);
                    let is_sel = sp.scheme_index == i;
                    let dot_size = if is_sel { cell_w * 1.4 } else { cell_w };
                    if is_sel {
                        add_px_rect(dot_x - 2.0, dot_y - 2.0, dot_size + 4.0, cell_h + 4.0, [0.4, 0.7, 1.0, 1.0], sw, sh, bg_verts, bg_idx);
                    }
                    add_px_rect(dot_x, dot_y, dot_size, cell_h, col, sw, sh, bg_verts, bg_idx);
                }
            }
            _ => {
                // Window タブ
                let opacity_line = format!("  Opacity: {:.2}   (↑/↓ to change)", sp.opacity);
                add_string_verts(
                    &opacity_line, px + cell_w, content_y,
                    [0.9, 0.95, 1.0, 1.0], false,
                    sw, sh, cell_w, font, atlas, &self.queue,
                    text_verts, text_idx,
                );
                // 不透明度バー
                let bar_y = content_y + cell_h * 1.4;
                let bar_w = pw - cell_w * 4.0;
                add_px_rect(px + cell_w * 2.0, bar_y, bar_w, cell_h * 0.4, [0.30, 0.30, 0.35, 1.0], sw, sh, bg_verts, bg_idx);
                add_px_rect(px + cell_w * 2.0, bar_y, bar_w * sp.opacity, cell_h * 0.4, [0.4, 0.7, 1.0, 1.0], sw, sh, bg_verts, bg_idx);
            }
        }

        // ボトムヒント
        let hint_y = py + ph - cell_h * 1.1;
        add_px_rect(px, hint_y, pw, 1.0, [0.30, 0.30, 0.35, 1.0], sw, sh, bg_verts, bg_idx);
        add_string_verts(
            "  Enter=save  Esc=cancel  Tab=next tab",
            px + cell_w, hint_y + cell_h * 0.1,
            [0.5, 0.55, 0.60, 1.0], false,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );
    }

    /// SFTP ファイル転送ダイアログ頂点を構築する
    ///
    /// ホスト名・ローカルパス・リモートパスの 3 フィールドを入力する。
    #[allow(clippy::too_many_arguments)]
    fn build_file_transfer_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let ft = &state.file_transfer;
        let panel_cols: f32 = 56.0;
        let panel_rows: f32 = 7.0; // タイトル + ホスト + ローカル + リモート + ヒント

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景（深青緑）
        let bg_color = if ft.mode == "upload" {
            [0.05, 0.15, 0.20, 0.96]
        } else {
            [0.05, 0.20, 0.12, 0.96]
        };
        add_px_rect(px, py, pw, ph, bg_color, sw, sh, bg_verts, bg_idx);
        let accent = if ft.mode == "upload" { [0.2, 0.8, 1.0, 1.0] } else { [0.2, 1.0, 0.6, 1.0] };
        add_px_rect(px, py, pw, 2.0, accent, sw, sh, bg_verts, bg_idx);

        // タイトル
        let title = if ft.mode == "upload" { "SFTP Upload  (Tab=next, Enter=send, Esc=cancel)" }
                    else { "SFTP Download  (Tab=next, Enter=send, Esc=cancel)" };
        add_string_verts(
            title, px + cell_w, py + cell_h * 0.1,
            accent, true,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );

        let field_labels = ["Host:", "Local:", "Remote:"];
        let field_values = [&ft.host_name, &ft.local_path, &ft.remote_path];

        for (i, (label, value)) in field_labels.iter().zip(field_values.iter()).enumerate() {
            let row_y = py + cell_h * (i as f32 * 1.5 + 1.3);
            let is_active = i == ft.field;

            // フィールド背景（アクティブは明るく）
            let field_bg = if is_active { [0.15, 0.25, 0.35, 1.0] } else { [0.10, 0.15, 0.20, 1.0] };
            add_px_rect(px + cell_w * 8.0, row_y, pw - cell_w * 9.0, cell_h, field_bg, sw, sh, bg_verts, bg_idx);

            // ラベル
            add_string_verts(
                label, px + cell_w, row_y,
                if is_active { [0.9, 0.95, 1.0, 1.0] } else { [0.6, 0.65, 0.7, 1.0] }, is_active,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );

            // 入力値 + カーソル
            let display = if is_active { format!("{}_", value) } else { format!("{}", value) };
            add_string_verts(
                &display, px + cell_w * 8.5, row_y,
                if is_active { [1.0, 1.0, 0.8, 1.0] } else { [0.8, 0.85, 0.8, 1.0] }, false,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }
    }

    /// Lua マクロピッカー頂点を構築する（中央フローティングリスト）
    ///
    /// 定義済みマクロを一覧表示し、Enter で実行する。
    #[allow(clippy::too_many_arguments)]
    fn build_macro_picker_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let mp = &state.macro_picker;
        let items = mp.filtered();
        let panel_cols: f32 = 50.0;
        let panel_rows = (items.len() + 3).min(14) as f32;

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景（深紫系）
        add_px_rect(px, py, pw, ph, [0.12, 0.08, 0.20, 0.96], sw, sh, bg_verts, bg_idx);
        // 上端アクセント線（紫/ピンク）
        add_px_rect(px, py, pw, 2.0, [0.7, 0.3, 1.0, 1.0], sw, sh, bg_verts, bg_idx);

        // タイトル行
        add_string_verts(
            "Lua Macros", px + cell_w, py + cell_h * 0.1,
            [0.8, 0.5, 1.0, 1.0], true,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );

        // クエリ行
        let query_text = format!("> {}", mp.query);
        add_string_verts(
            &query_text, px + cell_w, py + cell_h * 1.1,
            [1.0, 1.0, 1.0, 1.0], false,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );

        // マクロ一覧
        for (i, mac) in items.iter().enumerate().take(panel_rows as usize - 2) {
            let item_py = py + cell_h * (i as f32 + 2.2);
            let is_selected = i == mp.selected;
            if is_selected {
                add_px_rect(px + 2.0, item_py, pw - 4.0, cell_h, [0.35, 0.15, 0.50, 1.0], sw, sh, bg_verts, bg_idx);
            }
            let prefix = if is_selected { "> " } else { "  " };
            let desc = if mac.description.is_empty() { &mac.lua_fn } else { &mac.description };
            let label = format!("{}{:<22} {}", prefix, mac.name, desc);
            let fg = if is_selected { [0.95, 0.8, 1.0, 1.0] } else { [0.70, 0.60, 0.78, 1.0] };
            add_string_verts(
                &label, px + cell_w, item_py, fg, is_selected,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }

        // 空マクロ時のヒント
        if items.is_empty() {
            add_string_verts(
                "  (no macros in config)", px + cell_w, py + cell_h * 2.2,
                [0.5, 0.5, 0.5, 1.0], false,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }
    }

    /// ホストマネージャ頂点を構築する（中央フローティングリスト）
    ///
    /// コマンドパレットと同様のレイアウトで SSH ホスト一覧を表示する。
    #[allow(clippy::too_many_arguments)]
    fn build_host_manager_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let hm = &state.host_manager;
        let items = hm.filtered();
        let panel_cols: f32 = 52.0;
        let panel_rows = (items.len() + 3).min(14) as f32; // タイトル + クエリ + 最大12項目

        let pw = panel_cols * cell_w;
        let ph = panel_rows * cell_h;
        let px = (sw - pw) / 2.0;
        let py = (sh - ph) / 2.0;

        // パネル背景（深めの紺）
        add_px_rect(px, py, pw, ph, [0.08, 0.12, 0.22, 0.96], sw, sh, bg_verts, bg_idx);
        // 上端アクセント線（緑系）
        add_px_rect(px, py, pw, 2.0, [0.2, 0.8, 0.5, 1.0], sw, sh, bg_verts, bg_idx);

        // タイトル行
        add_string_verts(
            "SSH Hosts", px + cell_w, py + cell_h * 0.1,
            [0.2, 0.9, 0.6, 1.0], true,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );

        // クエリ行
        let query_text = format!("> {}", hm.query);
        add_string_verts(
            &query_text, px + cell_w, py + cell_h * 1.1,
            [1.0, 1.0, 1.0, 1.0], false,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );

        // ホスト一覧（タイトル+クエリ = 2行分オフセット）
        for (i, host) in items.iter().enumerate().take(panel_rows as usize - 2) {
            let item_py = py + cell_h * (i as f32 + 2.2);
            let is_selected = i == hm.selected;
            if is_selected {
                add_px_rect(px + 2.0, item_py, pw - 4.0, cell_h, [0.15, 0.45, 0.30, 1.0], sw, sh, bg_verts, bg_idx);
            }
            // 表示フォーマット: "> name  user@host:port"
            let prefix = if is_selected { "> " } else { "  " };
            let label = format!("{}{:<20} {}@{}:{}", prefix, host.name, host.username, host.host, host.port);
            let fg = if is_selected { [0.9, 1.0, 0.9, 1.0] } else { [0.70, 0.75, 0.72, 1.0] };
            add_string_verts(
                &label, px + cell_w, item_py, fg, is_selected,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }

        // 空ホスト時のヒント
        if items.is_empty() {
            add_string_verts(
                "  (no hosts in config)", px + cell_w, py + cell_h * 2.2,
                [0.5, 0.5, 0.5, 1.0], false,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }
    }

    /// Quick Select オーバーレイ頂点を構築する
    ///
    /// 各マッチ位置にラベル（a, b, ..., aa, ...）を黄色背景で描画する。
    #[allow(clippy::too_many_arguments)]
    fn build_quick_select_verts(
        &self,
        state: &ClientState,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let qs = &state.quick_select;
        if !qs.is_active {
            return;
        }

        // フォーカスペインのオフセットを取得する
        let (pane_x, pane_y) = if let Some(pid) = state.focused_pane_id {
            if let Some(layout) = state.pane_layouts.get(&pid) {
                (layout.col_offset as f32 * cell_w, layout.row_offset as f32 * cell_h)
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };

        for m in &qs.matches {
            let lx = pane_x + m.col_start as f32 * cell_w;
            let ly = pane_y + m.row as f32 * cell_h;
            let label_w = m.label.len() as f32 * cell_w;

            // マッチ全体をセミ透明ハイライト
            let match_w = (m.col_end - m.col_start) as f32 * cell_w;
            add_px_rect(lx, ly, match_w, cell_h, [0.9, 0.85, 0.2, 0.25], sw, sh, bg_verts, bg_idx);

            // ラベル背景（黄色）
            let is_partial_match = !qs.typed_label.is_empty() && m.label.starts_with(&qs.typed_label);
            let bg_color = if is_partial_match {
                [1.0, 0.6, 0.0, 0.95]
            } else {
                [0.9, 0.85, 0.1, 0.92]
            };
            add_px_rect(lx, ly, label_w, cell_h, bg_color, sw, sh, bg_verts, bg_idx);

            // ラベルテキスト（黒）
            add_string_verts(
                &m.label, lx, ly,
                [0.05, 0.05, 0.05, 1.0], true,
                sw, sh, cell_w, font, atlas, &self.queue,
                text_verts, text_idx,
            );
        }

        // 入力中ラベルを画面上部に表示する
        let typed = format!("Quick Select: {}_", qs.typed_label);
        add_px_rect(0.0, 0.0, typed.len() as f32 * cell_w + cell_w, cell_h, [0.15, 0.15, 0.18, 0.92], sw, sh, bg_verts, bg_idx);
        add_string_verts(
            &typed, cell_w * 0.5, 0.0,
            [1.0, 0.85, 0.2, 1.0], true,
            sw, sh, cell_w, font, atlas, &self.queue,
            text_verts, text_idx,
        );
    }

    /// コンテキストメニュー頂点を構築する（右クリック時のポップアップ）
    #[allow(clippy::too_many_arguments)]
    fn build_context_menu_verts(
        &self,
        menu: &ContextMenu,
        sw: f32, sh: f32, cell_w: f32, cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>, bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>, text_idx: &mut Vec<u16>,
    ) {
        let menu_w = 8.0 * cell_w;
        let menu_h = menu.items.len() as f32 * cell_h;

        // メニュー全体の背景（濃いグレー、半透明）
        add_px_rect(menu.x, menu.y, menu_w, menu_h, [0.15, 0.15, 0.18, 0.95], sw, sh, bg_verts, bg_idx);
        // 上端のアクセント線
        add_px_rect(menu.x, menu.y, menu_w, 2.0, [0.4, 0.6, 1.0, 1.0], sw, sh, bg_verts, bg_idx);

        for (i, item) in menu.items.iter().enumerate() {
            let item_y = menu.y + i as f32 * cell_h;
            // 項目区切り線（最初以外）
            if i > 0 {
                add_px_rect(menu.x, item_y, menu_w, 1.0, [0.30, 0.30, 0.35, 1.0], sw, sh, bg_verts, bg_idx);
            }
            add_string_verts(
                &item.label, menu.x + cell_w * 0.5, item_y,
                [0.9, 0.9, 0.9, 1.0], false,
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
    if (16..=231).contains(&n) {
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

/// Windows 11 の Acrylic（すりガラス）効果をウィンドウに適用する
///
/// DwmSetWindowAttribute で DWMWA_SYSTEMBACKDROP_TYPE = 4 (DWMWCP_ACRYLIC) を指定する。
/// Windows 10 や旧バージョンでは API が存在しないため何も起きない。
#[cfg(windows)]
fn apply_acrylic_blur(window: &winit::window::Window) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
    // raw-window-handle 0.6 の hwnd は NonZeroIsize (= isize)。
    // windows-sys 0.59 では HWND = *mut c_void なので isize から変換する。
    let hwnd = h.hwnd.get() as *mut ::core::ffi::c_void;

    // DWMWA_SYSTEMBACKDROP_TYPE = 38; 4 = DWMWCP_ACRYLIC（Windows 11 22H2+）
    let backdrop_type: u32 = 4;
    // SAFETY: hwnd は winit から取得した有効なウィンドウハンドル。
    //         DwmSetWindowAttribute は失敗しても戻り値を無視して続行する。
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            38,
            &backdrop_type as *const _ as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

/// GPU アプリケーション（winit EventLoop に渡す）
pub struct NextermApp {
    config: Config,
    state: ClientState,
    font: FontManager,
}

impl NextermApp {
    pub async fn new(config: Config) -> Result<Self> {
        let font = FontManager::new(&config.font.family, config.font.size, &config.font.font_fallbacks, 1.0);
        let mut state = ClientState::new(80, 24, config.scrollback_lines);
        // 設定ファイルのホスト一覧をホストマネージャに渡す
        state.host_manager = crate::host_manager::HostManager::new(config.hosts.clone());
        // 設定ファイルの Lua マクロ一覧をマクロピッカーに渡す
        state.macro_picker = crate::macro_picker::MacroPicker::new(config.macros.clone());
        // 設定パネルを設定値で初期化する
        state.settings_panel = crate::settings_panel::SettingsPanel::new(&config);
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
            scale_factor: 1.0,
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
    /// ディスプレイの DPI スケール係数（winit より取得）
    scale_factor: f32,
}

impl ApplicationHandler for EventHandler {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        // PTY 出力を 16ms ごとにポーリングする（約 60fps）
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(16),
        ));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // ウィンドウを作成する（設定に従って透過・ぼかし・装飾を適用する）
        use nexterm_config::WindowDecorations;
        let win_cfg = &self.app.config.window;
        let transparent = win_cfg.background_opacity < 1.0;
        let decorations = !matches!(win_cfg.decorations, WindowDecorations::None);

        let attrs = Window::default_attributes()
            .with_title("nexterm")
            .with_inner_size(PhysicalSize::new(1280u32, 800u32))
            .with_transparent(transparent)
            .with_decorations(decorations);

        let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create window"));

        // IME 入力を有効にする
        window.set_ime_allowed(true);

        // DPI スケール係数を取得し、フォントを実スケールで再生成する
        let scale_factor = window.scale_factor() as f32;
        self.scale_factor = scale_factor;
        self.app.font = FontManager::new(
            &self.app.config.font.family,
            self.app.config.font.size,
            &self.app.config.font.font_fallbacks,
            scale_factor,
        );

        // Acrylic（すりガラス）背景を適用する（Windows 11 のみ有効）
        #[cfg(windows)]
        apply_acrylic_blur(&window);

        // wgpu を非同期で初期化する（tokio runtime が必要）
        let wgpu_state = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(WgpuState::new(Arc::clone(&window)))
        })
        .expect("Failed to initialize wgpu");

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
                        info!("Connected to nexterm server");
                        Some(conn)
                    }
                    Err(e) => {
                        warn!("Failed to connect to server (offline mode): {}", e);
                        None
                    }
                }
            })
        });
        self.connection = conn;

        info!("wgpu renderer initialized");
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

        // BEL を受信していればウィンドウ注目要求を発行する
        if self.app.state.pending_bell {
            self.app.state.pending_bell = false;
            if let Some(w) = &self.window {
                w.request_user_attention(Some(winit::window::UserAttentionType::Informational));
            }
        }

        // 設定ホットリロードをポーリングする（最新の設定を適用する）
        if let Some(rx) = &mut self.config_rx
            && let Ok(new_config) = rx.try_recv() {
                info!("Config reloaded: font={} {}pt", new_config.font.family, new_config.font.size);
                // フォントサイズ変更時はグリフアトラスも再生成する
                let font_changed = self.app.config.font != new_config.font;
                self.app.config = new_config;
                if font_changed {
                    self.app.font = crate::font::FontManager::new(
                        &self.app.config.font.family,
                        self.app.config.font.size,
                        &self.app.config.font.font_fallbacks,
                        self.scale_factor,
                    );
                    if let Some(wgpu) = &self.wgpu_state {
                        self.atlas = Some(GlyphAtlas::new(&wgpu.device));
                    }
                }
                had_messages = true;
            }

        // Lua ステータスバーを 1 秒ごとに再評価してキャッシュを更新する
        if self.app.config.status_bar.enabled
            && !self.app.config.status_bar.widgets.is_empty()
            && self.last_status_eval.elapsed() >= Duration::from_secs(1)
            && let Some(eval) = &self.status_eval {
                self.app.state.status_bar_text =
                    eval.evaluate_widgets(&self.app.config.status_bar.widgets);
                self.last_status_eval = Instant::now();
                had_messages = true;
            }

        if had_messages
            && let Some(w) = &self.window {
                w.request_redraw();
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

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor as f32;
                self.app.font = crate::font::FontManager::new(
                    &self.app.config.font.family,
                    self.app.config.font.size,
                    &self.app.config.font.font_fallbacks,
                    self.scale_factor,
                );
                // スケール変更でグリフが無効化されるためアトラスをクリアする
                if let Some(wgpu) = &self.wgpu_state {
                    self.atlas = Some(GlyphAtlas::new(&wgpu.device));
                }
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            // マウスカーソル位置を追跡する（ドラッグ中は選択範囲を更新する）
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some((position.x, position.y));
                if self.app.state.mouse_sel.is_dragging {
                    let cell_w = self.app.font.cell_width() as f64;
                    let cell_h = self.app.font.cell_height() as f64;
                    let col = (position.x / cell_w) as u16;
                    let row = (position.y / cell_h) as u16;
                    self.app.state.mouse_sel.update(col, row);
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            // 右ボタン押下: コンテキストメニューを開く
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state: ElementState::Pressed,
                ..
            } => {
                if let Some((px, py)) = self.cursor_position {
                    self.app.state.context_menu =
                        Some(ContextMenu::new_default(px as f32, py as f32));
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            // 左ボタン押下: 選択開始
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } => {
                if let Some((px, py)) = self.cursor_position {
                    let cell_w = self.app.font.cell_width() as f64;
                    let cell_h = self.app.font.cell_height() as f64;
                    let col = (px / cell_w) as u16;
                    let row = (py / cell_h) as u16;
                    self.app.state.mouse_sel.begin(col, row);
                }
            }

            // 左ボタンリリース: 選択確定 → クリップボードコピー or フォーカス切替
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } => {
                // コンテキストメニューが開いている場合はクリックで処理する
                if let Some((px, py)) = self.cursor_position
                    && let Some(menu) = self.app.state.context_menu.take() {
                        let cell_w = self.app.font.cell_width();
                        let cell_h = self.app.font.cell_height();
                        let menu_w = 8.0 * cell_w;
                        let fx = px as f32;
                        let fy = py as f32;
                        if fx >= menu.x && fx <= menu.x + menu_w {
                            for (i, item) in menu.items.iter().enumerate() {
                                let item_y = menu.y + i as f32 * cell_h;
                                if fy >= item_y && fy < item_y + cell_h {
                                    self.execute_context_menu_action(&item.action);
                                    break;
                                }
                            }
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                        return;
                    }

                if let Some((px, py)) = self.cursor_position {
                    let cell_w = self.app.font.cell_width() as f64;
                    let cell_h = self.app.font.cell_height() as f64;
                    let click_col = (px / cell_w) as u16;
                    let click_row = (py / cell_h) as u16;

                    // ドラッグ選択を終了して選択テキストをコピーする
                    self.app.state.mouse_sel.update(click_col, click_row);
                    self.app.state.mouse_sel.finish();

                    if let Some(((sc, sr), (ec, er))) = self.app.state.mouse_sel.normalized() {
                        // 選択範囲があればテキストを抽出してクリップボードにコピーする
                        let text = if let Some(pane) = self.app.state.focused_pane() {
                            let mut lines = Vec::new();
                            for row_idx in sr..=er {
                                if let Some(row) = pane.grid.rows.get(row_idx as usize) {
                                    let col_start = if row_idx == sr { sc as usize } else { 0 };
                                    let col_end = if row_idx == er {
                                        (ec + 1) as usize
                                    } else {
                                        row.len()
                                    };
                                    let line: String =
                                        row[col_start.min(row.len())..col_end.min(row.len())]
                                            .iter()
                                            .map(|c| c.ch)
                                            .collect();
                                    lines.push(line.trim_end().to_string());
                                }
                            }
                            lines.join("\n")
                        } else {
                            String::new()
                        };

                        if !text.is_empty()
                            && let Ok(mut clipboard) = arboard::Clipboard::new() {
                                let _ = clipboard.set_text(text);
                            }
                        // 選択後はリターン（ペインフォーカス切替を行わない）
                        return;
                    }

                    // 選択なし（単純クリック）: Ctrl+クリックで URL を開く
                    if self.modifiers.control_key()
                        && let Some(url) = self.find_url_at(click_col, click_row) {
                            open_url(&url);
                            return;
                        }

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
                    if let Some(pane_id) = target_pane
                        && self.app.state.focused_pane_id != Some(pane_id)
                            && let Some(conn) = &self.connection {
                                let _ = conn
                                    .send_tx
                                    .try_send(ClientToServer::FocusPane { pane_id });
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
                    } else if let Some(ref t) = text
                        && !self.modifiers.control_key() {
                            for ch in t.chars() {
                                self.app.state.push_search_char(ch);
                            }
                        }
                    // Escape / Enter は handle_key で処理する
                    if let PhysicalKey::Code(code) = physical_key
                        && matches!(code, WKeyCode::Escape | WKeyCode::Enter) {
                            self.handle_key(code, event_loop);
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

            // IME イベントを処理する（日本語・中国語などの入力に対応）
            WindowEvent::Ime(ime_event) => {
                match ime_event {
                    Ime::Enabled => {
                        // IME が有効になった（特別な処理は不要）
                    }
                    Ime::Preedit(text, _cursor_range) => {
                        // 変換中テキストを state に保存して再描画する
                        if text.is_empty() {
                            self.app.state.ime_preedit = None;
                        } else {
                            self.app.state.ime_preedit = Some(text);
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                    }
                    Ime::Commit(text) => {
                        // 確定テキストをプリエディットクリア + PTY 送信
                        self.app.state.ime_preedit = None;
                        if let Some(conn) = &self.connection {
                            let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                    }
                    Ime::Disabled => {
                        self.app.state.ime_preedit = None;
                    }
                }
                // IME カーソルエリアをフォーカスペインのカーソル位置に更新する
                if let Some(pane) = self.app.state.focused_pane() {
                    let cell_w = self.app.font.cell_width();
                    let cell_h = self.app.font.cell_height();
                    let ime_x = pane.cursor_col as f32 * cell_w;
                    let ime_y = (pane.cursor_row + 1) as f32 * cell_h;
                    if let Some(w) = &self.window {
                        w.set_ime_cursor_area(
                            winit::dpi::PhysicalPosition::new(ime_x as i32, ime_y as i32),
                            winit::dpi::PhysicalSize::new(cell_w as u32, cell_h as u32),
                        );
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if let (Some(wgpu), Some(atlas)) =
                    (&mut self.wgpu_state, &mut self.atlas)
                    && let Err(e) =
                        wgpu.render(&self.app.state, &mut self.app.font, atlas, &self.app.config.tab_bar)
                    {
                        warn!("Render error: {}", e);
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
            if let Ok(mut clipboard) = arboard::Clipboard::new()
                && let Ok(text) = clipboard.get_text()
                    && let Some(conn) = &self.connection {
                        let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
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

        // Ctrl+Shift+U: SFTP アップロードダイアログを開く
        if ctrl && shift && code == WKeyCode::KeyU {
            self.app.state.file_transfer.open_upload();
            return true;
        }

        // Ctrl+Shift+D: SFTP ダウンロードダイアログを開く
        if ctrl && shift && code == WKeyCode::KeyD {
            self.app.state.file_transfer.open_download();
            return true;
        }

        // Ctrl+Shift+M: Lua マクロピッカーのトグル
        if ctrl && shift && code == WKeyCode::KeyM {
            if self.app.state.macro_picker.is_open {
                self.app.state.macro_picker.close();
            } else {
                self.app.state.macro_picker.reload(self.app.config.macros.clone());
                self.app.state.macro_picker.open();
            }
            return true;
        }

        // Ctrl+Shift+H: ホストマネージャのトグル
        if ctrl && shift && code == WKeyCode::KeyH {
            if self.app.state.host_manager.is_open {
                self.app.state.host_manager.close();
            } else {
                // 設定ホスト一覧を最新にリロードしてから開く
                self.app.state.host_manager.reload(self.app.config.hosts.clone());
                self.app.state.host_manager.open();
            }
            return true;
        }

        // Ctrl+,: 設定パネルをトグルする
        if ctrl && code == WKeyCode::Comma {
            if self.app.state.settings_panel.is_open {
                self.app.state.settings_panel.close();
            } else {
                self.app.state.settings_panel.open();
            }
            return true;
        }

        // Ctrl+F: スクロールバック検索を開始する
        if ctrl && code == WKeyCode::KeyF {
            self.app.state.start_search();
            return true;
        }

        // Ctrl+[ : コピーモードを開始する（tmux 互換）
        if ctrl && code == WKeyCode::BracketLeft {
            if !self.app.state.copy_mode.is_active {
                let (col, row) = self
                    .app
                    .state
                    .focused_pane()
                    .map(|p| (p.cursor_col, p.cursor_row))
                    .unwrap_or((0, 0));
                self.app.state.copy_mode.enter(col, row);
            }
            return true;
        }

        // コピーモード中のキー処理
        if self.app.state.copy_mode.is_active {
            return self.handle_copy_mode_key(code);
        }

        // Quick Select モード中のキー処理
        if self.app.state.quick_select.is_active {
            return self.handle_quick_select_key(code);
        }

        // ファイル転送ダイアログが開いているときのキー処理（全キーを消費）
        if self.app.state.file_transfer.is_open {
            match code {
                WKeyCode::Escape => self.app.state.file_transfer.close(),
                WKeyCode::Tab | WKeyCode::ArrowDown => self.app.state.file_transfer.next_field(),
                WKeyCode::ArrowUp => self.app.state.file_transfer.prev_field(),
                WKeyCode::Backspace => {
                    self.app.state.file_transfer.current_field_mut().pop();
                }
                WKeyCode::Enter => {
                    let ft = &self.app.state.file_transfer;
                    if !ft.host_name.is_empty() && !ft.local_path.is_empty() && !ft.remote_path.is_empty() {
                        let msg = if ft.mode == "upload" {
                            ClientToServer::SftpUpload {
                                host_name: ft.host_name.clone(),
                                local_path: ft.local_path.clone(),
                                remote_path: ft.remote_path.clone(),
                            }
                        } else {
                            ClientToServer::SftpDownload {
                                host_name: ft.host_name.clone(),
                                remote_path: ft.remote_path.clone(),
                                local_path: ft.local_path.clone(),
                            }
                        };
                        if let Some(conn) = &self.connection {
                            let _ = conn.send_tx.try_send(msg);
                        }
                        self.app.state.file_transfer.close();
                    }
                }
                _ => {
                    if let Some(ch) = winit_code_to_char(code) {
                        self.app.state.file_transfer.current_field_mut().push(ch);
                    }
                }
            }
            return true;
        }

        // マクロピッカーが開いているときのナビゲーション（全キーを消費）
        if self.app.state.macro_picker.is_open {
            match code {
                WKeyCode::ArrowDown => self.app.state.macro_picker.select_next(),
                WKeyCode::ArrowUp => self.app.state.macro_picker.select_prev(),
                WKeyCode::Escape => self.app.state.macro_picker.close(),
                WKeyCode::Backspace => self.app.state.macro_picker.pop_char(),
                WKeyCode::Enter => {
                    if let Some(mac) = self.app.state.macro_picker.selected_macro() {
                        let fn_name = mac.lua_fn.clone();
                        let display_name = mac.name.clone();
                        self.app.state.macro_picker.close();
                        if let Some(conn) = &self.connection {
                            let _ = conn.send_tx.try_send(ClientToServer::RunMacro {
                                macro_fn: fn_name,
                                display_name,
                            });
                        }
                    }
                }
                _ => {
                    if let Some(ch) = winit_code_to_char(code) {
                        self.app.state.macro_picker.push_char(ch);
                    }
                }
            }
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

        // Escape: 検索・パレット・ホストマネージャを閉じる
        if code == WKeyCode::Escape {
            if self.app.state.settings_panel.is_open {
                self.app.state.settings_panel.close();
                return true;
            } else if self.app.state.palette.is_open {
                self.app.state.palette.close();
                return true;
            } else if self.app.state.host_manager.is_open {
                self.app.state.host_manager.close();
                return true;
            } else if self.app.state.macro_picker.is_open {
                self.app.state.macro_picker.close();
                return true;
            } else if self.app.state.file_transfer.is_open {
                self.app.state.file_transfer.close();
                return true;
            } else if self.app.state.search.is_active {
                self.app.state.end_search();
                return true;
            }
            // パレット・検索が開いていなければ PTY に転送する
            return false;
        }

        // 設定パネルが開いているときのナビゲーション（全キーを消費）
        if self.app.state.settings_panel.is_open {
            match code {
                WKeyCode::Tab | WKeyCode::ArrowRight => self.app.state.settings_panel.next_tab(),
                WKeyCode::ArrowLeft => self.app.state.settings_panel.prev_tab(),
                WKeyCode::ArrowUp => {
                    match self.app.state.settings_panel.tab {
                        0 => self.app.state.settings_panel.increase_font_size(),
                        2 => self.app.state.settings_panel.increase_opacity(),
                        _ => {}
                    }
                }
                WKeyCode::ArrowDown => {
                    match self.app.state.settings_panel.tab {
                        0 => self.app.state.settings_panel.decrease_font_size(),
                        2 => self.app.state.settings_panel.decrease_opacity(),
                        _ => {}
                    }
                }
                WKeyCode::BracketRight => {
                    if self.app.state.settings_panel.tab == 1 {
                        self.app.state.settings_panel.next_scheme();
                    }
                }
                WKeyCode::BracketLeft => {
                    if self.app.state.settings_panel.tab == 1 {
                        self.app.state.settings_panel.prev_scheme();
                    }
                }
                WKeyCode::Enter => {
                    let _ = self.app.state.settings_panel.save_to_toml();
                    self.app.state.settings_panel.close();
                }
                WKeyCode::Escape => {
                    self.app.state.settings_panel.close();
                }
                _ => {}
            }
            return true;
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

        // ホストマネージャが開いているときのナビゲーション（全キーを消費）
        if self.app.state.host_manager.is_open {
            match code {
                WKeyCode::ArrowDown => self.app.state.host_manager.select_next(),
                WKeyCode::ArrowUp => self.app.state.host_manager.select_prev(),
                WKeyCode::Escape => self.app.state.host_manager.close(),
                WKeyCode::Backspace => self.app.state.host_manager.pop_char(),
                WKeyCode::Enter => {
                    if let Some(host) = self.app.state.host_manager.selected_host() {
                        let host = host.clone();
                        self.app.state.host_manager.close();
                        self.connect_ssh_host_new_tab(&host);
                    }
                }
                _ => {
                    if let Some(ch) = winit_code_to_char(code) {
                        self.app.state.host_manager.push_char(ch);
                    }
                }
            }
            return true;
        }

        // 検索モードの特殊キー
        if self.app.state.search.is_active
            && code == WKeyCode::Enter {
                self.app.state.search_next();
                return true;
            }
            // 他のキーは消費しない（上の search.is_active ブロックで処理済み）

        // Ctrl++（Equal / Plus）: フォントサイズを大きくする
        if ctrl && (code == WKeyCode::Equal || code == WKeyCode::NumpadAdd) {
            self.change_font_size(1.0);
            return true;
        }

        // Ctrl+- : フォントサイズを小さくする
        if ctrl && (code == WKeyCode::Minus || code == WKeyCode::NumpadSubtract) {
            self.change_font_size(-1.0);
            return true;
        }

        // Ctrl+0 : フォントサイズをデフォルトに戻す
        if ctrl && code == WKeyCode::Digit0 {
            self.reset_font_size();
            return true;
        }

        // 設定ファイルのカスタムキーバインドをチェックする
        if self.check_config_keybindings(code, event_loop) {
            return true;
        }

        false
    }

    /// クリック座標 (col, row) に URL があれば返す
    fn find_url_at(&self, col: u16, row: u16) -> Option<String> {
        use crate::state::detect_urls_in_row;
        let pane = self.app.state.focused_pane()?;
        let cells = pane.grid.rows.get(row as usize)?;
        let urls = detect_urls_in_row(row, cells);
        urls.into_iter().find(|u| u.contains(col, row)).map(|u| u.url)
    }

    /// コピーモードのキー入力を処理する（true = 消費済み）
    fn handle_copy_mode_key(&mut self, code: WKeyCode) -> bool {
        let cm = &mut self.app.state.copy_mode;
        let max_col = self.app.state.cols.saturating_sub(1);
        let max_row = self.app.state.rows.saturating_sub(1);

        match code {
            // q / Escape: コピーモードを終了する
            WKeyCode::KeyQ | WKeyCode::Escape => {
                cm.exit();
            }
            // h / Left: 左移動
            WKeyCode::KeyH | WKeyCode::ArrowLeft => {
                cm.cursor_col = cm.cursor_col.saturating_sub(1);
            }
            // l / Right: 右移動
            WKeyCode::KeyL | WKeyCode::ArrowRight => {
                if cm.cursor_col < max_col {
                    cm.cursor_col += 1;
                }
            }
            // j / Down: 下移動
            WKeyCode::KeyJ | WKeyCode::ArrowDown => {
                if cm.cursor_row < max_row {
                    cm.cursor_row += 1;
                }
            }
            // k / Up: 上移動
            WKeyCode::KeyK | WKeyCode::ArrowUp => {
                cm.cursor_row = cm.cursor_row.saturating_sub(1);
            }
            // 0: 行頭へ移動
            WKeyCode::Digit0 => {
                cm.cursor_col = 0;
            }
            // v: 選択開始/終了をトグル
            WKeyCode::KeyV => {
                cm.toggle_selection();
            }
            // y: 選択テキストをクリップボードにコピーして終了
            WKeyCode::KeyY => {
                self.yank_selection();
            }
            _ => return false,
        }
        true
    }

    /// 選択範囲のテキストをクリップボードにコピーしてコピーモードを終了する
    fn yank_selection(&mut self) {
        let cm = &self.app.state.copy_mode;
        if let Some(((sc, sr), (ec, er))) = cm.normalized_selection() {
            // グリッドから選択テキストを抽出する
            let text = if let Some(pane) = self.app.state.focused_pane() {
                let mut lines = Vec::new();
                for row_idx in sr..=er {
                    if let Some(row) = pane.grid.rows.get(row_idx as usize) {
                        let col_start = if row_idx == sr { sc as usize } else { 0 };
                        let col_end = if row_idx == er {
                            (ec + 1) as usize
                        } else {
                            row.len()
                        };
                        let line: String = row[col_start.min(row.len())..col_end.min(row.len())]
                            .iter()
                            .map(|c| c.ch)
                            .collect();
                        lines.push(line);
                    }
                }
                lines.join("\n")
            } else {
                String::new()
            };

            if !text.is_empty()
                && let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
        }
        self.app.state.copy_mode.exit();
    }

    /// フォントサイズを delta pt だけ変更してグリフアトラスを再生成する
    fn change_font_size(&mut self, delta: f32) {
        let new_size = (self.app.config.font.size + delta).clamp(6.0, 72.0);
        if (new_size - self.app.config.font.size).abs() < f32::EPSILON {
            return;
        }
        self.app.config.font.size = new_size;
        self.app.font =
            crate::font::FontManager::new(&self.app.config.font.family, new_size, &self.app.config.font.font_fallbacks, self.scale_factor);
        if let Some(wgpu) = &self.wgpu_state {
            self.atlas = Some(GlyphAtlas::new(&wgpu.device));
        }
        info!("Font size changed to {}pt", new_size);
    }

    /// Quick Select モードのキー入力を処理する（true = 消費済み）
    fn handle_quick_select_key(&mut self, code: WKeyCode) -> bool {
        match code {
            WKeyCode::Escape => {
                self.app.state.quick_select.exit();
                return true;
            }
            WKeyCode::Backspace => {
                self.app.state.quick_select.typed_label.pop();
                return true;
            }
            _ => {}
        }

        // アルファベットキーをラベル入力として受け取る
        if let Some(ch) = winit_code_to_char(code) {
            self.app.state.quick_select.typed_label.push(ch);

            // マッチが確定したらクリップボードにコピーして終了
            if let Some(m) = self.app.state.quick_select.accept() {
                let text = m.text.clone();
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(text);
                }
                self.app.state.quick_select.exit();
            }
        }

        true
    }

    /// フォントサイズを設定ファイルの初期値に戻す
    fn reset_font_size(&mut self) {
        // 設定ファイルの初期値は config 生成時のサイズを参照する手段がないため
        // 慣例の 14pt をデフォルトとして使用する
        let default_size = nexterm_config::Config::default().font.size;
        self.app.config.font.size = default_size;
        self.app.font =
            crate::font::FontManager::new(&self.app.config.font.family, default_size, &self.app.config.font.font_fallbacks, self.scale_factor);
        if let Some(wgpu) = &self.wgpu_state {
            self.atlas = Some(GlyphAtlas::new(&wgpu.device));
        }
        info!("Font size reset to {}pt", default_size);
    }

    fn execute_action(&mut self, action: &str, event_loop: &ActiveEventLoop) {
        match action {
            "Quit" => event_loop.exit(),
            "SearchScrollback" => self.app.state.start_search(),
            "SplitVertical" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitVertical);
                }
            }
            "SplitHorizontal" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitHorizontal);
                }
            }
            "FocusNextPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::FocusNextPane);
                }
            }
            "FocusPrevPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::FocusPrevPane);
                }
            }
            "ClosePane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ClosePane);
                }
            }
            "NewWindow" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::NewWindow);
                }
            }
            "Detach" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::Detach);
                }
            }
            "CommandPalette" => {
                self.app.state.toggle_palette();
            }
            "SetBroadcastOn" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SetBroadcast { enabled: true });
                }
            }
            "SetBroadcastOff" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SetBroadcast { enabled: false });
                }
            }
            "ToggleZoom" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ToggleZoom);
                }
            }
            "QuickSelect" => {
                if let Some(pane) = self.app.state.focused_pane() {
                    let rows = pane.grid.rows.clone();
                    self.app.state.quick_select.enter(&rows);
                }
            }
            "SwapPaneNext" => {
                // フォーカスペインの次のペイン ID を取得してスワップする
                if let Some(conn) = &self.connection {
                    // 現在フォーカスペインの隣ペインを pane_layouts から探す
                    let layouts: Vec<_> = self.app.state.pane_layouts.values().collect();
                    if layouts.len() >= 2 {
                        let focused = self.app.state.focused_pane_id.unwrap_or(0);
                        // focused 以外で pane_id が最も近い（次の）ペインを選ぶ
                        let target = layouts.iter()
                            .filter(|l| l.pane_id != focused)
                            .map(|l| l.pane_id)
                            .min_by_key(|&id| if id > focused { id - focused } else { u32::MAX })
                            .or_else(|| layouts.iter().map(|l| l.pane_id).find(|&id| id != focused));
                        if let Some(target_id) = target {
                            let _ = conn.send_tx.try_send(ClientToServer::SwapPane { target_pane_id: target_id });
                        }
                    }
                }
            }
            "SwapPanePrev" => {
                if let Some(conn) = &self.connection {
                    let layouts: Vec<_> = self.app.state.pane_layouts.values().collect();
                    if layouts.len() >= 2 {
                        let focused = self.app.state.focused_pane_id.unwrap_or(0);
                        let target = layouts.iter()
                            .filter(|l| l.pane_id != focused)
                            .map(|l| l.pane_id)
                            .min_by_key(|&id| if id < focused { focused - id } else { u32::MAX })
                            .or_else(|| layouts.iter().map(|l| l.pane_id).find(|&id| id != focused));
                        if let Some(target_id) = target {
                            let _ = conn.send_tx.try_send(ClientToServer::SwapPane { target_pane_id: target_id });
                        }
                    }
                }
            }
            "BreakPane" => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::BreakPane);
                }
            }
            "ShowSettings" => {
                self.app.state.settings_panel.open();
            }
            "ShowHostManager" => {
                self.app.state.host_manager.reload(self.app.config.hosts.clone());
                self.app.state.host_manager.open();
            }
            "ShowMacroPicker" => {
                self.app.state.macro_picker.reload(self.app.config.macros.clone());
                self.app.state.macro_picker.open();
            }
            "SftpUploadDialog" => {
                self.app.state.file_transfer.open_upload();
            }
            "SftpDownloadDialog" => {
                self.app.state.file_transfer.open_download();
            }
            "ConnectSerialPrompt" => {
                // 設定ファイルのシリアルポート一覧からデフォルト（先頭）エントリで接続する
                // 設定がない場合は一般的なデフォルト値を使用する
                if let Some(conn) = &self.connection {
                    let serial_cfg = self.app.config.serial_ports.first().cloned();
                    let (port, baud_rate, data_bits, stop_bits, parity) = if let Some(cfg) = serial_cfg {
                        (cfg.port, cfg.baud_rate, cfg.data_bits, cfg.stop_bits, cfg.parity)
                    } else {
                        // プラットフォームデフォルト
                        #[cfg(unix)]
                        let default_port = "/dev/ttyUSB0".to_string();
                        #[cfg(windows)]
                        let default_port = "COM1".to_string();
                        (default_port, 115200, 8, 1, "none".to_string())
                    };
                    let _ = conn.send_tx.try_send(ClientToServer::ConnectSerial {
                        port,
                        baud_rate,
                        data_bits,
                        stop_bits,
                        parity,
                    });
                }
            }
            _ => debug!("Execute action: {}", action),
        }
    }

    /// コンテキストメニューのアクションを実行する
    fn execute_context_menu_action(&mut self, action: &ContextMenuAction) {
        match action {
            ContextMenuAction::Copy => {
                // フォーカスペインの可視グリッドをクリップボードにコピーする
                if let Some(pane) = self.app.state.focused_pane() {
                    let text = grid_to_text(pane);
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                }
            }
            ContextMenuAction::Paste => {
                if let Ok(mut clipboard) = arboard::Clipboard::new()
                    && let Ok(text) = clipboard.get_text()
                        && let Some(conn) = &self.connection {
                            let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                        }
            }
            ContextMenuAction::SplitVertical => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitVertical);
                }
            }
            ContextMenuAction::SplitHorizontal => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::SplitHorizontal);
                }
            }
            ContextMenuAction::ClosePane => {
                if let Some(conn) = &self.connection {
                    let _ = conn.send_tx.try_send(ClientToServer::ClosePane);
                }
            }
        }
    }

    /// HostConfig から ConnectSsh メッセージを送信する（現在のペインに接続）
    fn connect_ssh_host(&self, host: &nexterm_config::HostConfig) {
        let Some(conn) = &self.connection else { return };
        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password: None,
            key_path: host.key_path.clone(),
            remote_forwards: host.forward_remote.clone(),
            x11_forward: host.x11_forward,
            x11_trusted: host.x11_trusted,
        });
    }

    /// HostConfig から新しいタブを開いて ConnectSsh メッセージを送信する
    fn connect_ssh_host_new_tab(&self, host: &nexterm_config::HostConfig) {
        let Some(conn) = &self.connection else { return };
        // 先に新しいウィンドウ（タブ）を作成してから SSH 接続を要求する
        let _ = conn.send_tx.try_send(ClientToServer::NewWindow);
        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password: None,
            key_path: host.key_path.clone(),
            remote_forwards: host.forward_remote.clone(),
            x11_forward: host.x11_forward,
            x11_trusted: host.x11_trusted,
        });
    }

    /// 設定のキーバインド一覧から一致するものを探してアクションを実行する
    /// 消費した場合は true を返す
    fn check_config_keybindings(&mut self, code: WKeyCode, event_loop: &ActiveEventLoop) -> bool {
        // config.keys を走査してマッチするバインドを探す
        let bindings = self.app.config.keys.clone();
        for binding in &bindings {
            if config_key_matches(&binding.key, code, self.modifiers) {
                let action = binding.action.clone();
                self.execute_action(&action, event_loop);
                return true;
            }
        }
        false
    }

    /// キー入力をサーバーの PTY に転送する
    fn forward_key_to_server(&self, physical_key: PhysicalKey, text: Option<&str>) {
        let Some(conn) = &self.connection else { return };
        let mods = proto_modifiers(self.modifiers);
        let ctrl = self.modifiers.control_key();

        // Ctrl 非押下でテキストがある場合はテキスト入力として送信する
        if !ctrl
            && let Some(text_str) = text
                && !text_str.is_empty() {
                    for ch in text_str.chars() {
                        let _ = conn.send_tx.try_send(ClientToServer::KeyEvent {
                            code: ProtoKeyCode::Char(ch),
                            modifiers: mods,
                        });
                    }
                    return;
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
/// URL をデフォルトブラウザで開く（プラットフォーム対応）
fn open_url(url: &str) {
    info!("Opening URL: {}", url);
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

fn grid_to_text(pane: &crate::state::PaneState) -> String {
    let mut lines = Vec::with_capacity(pane.grid.rows.len());
    for row in &pane.grid.rows {
        let line: String = row.iter().map(|c| c.ch).collect();
        // 行末の空白を除去して返す
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

/// `#rrggbb` 形式の16進カラー文字列を `[f32; 4]` RGBA に変換する
fn hex_to_rgba(hex: &str, alpha: f32) -> [f32; 4] {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(hex.get(0..2).unwrap_or("80"), 16).unwrap_or(128) as f32 / 255.0;
    let g = u8::from_str_radix(hex.get(2..4).unwrap_or("80"), 16).unwrap_or(128) as f32 / 255.0;
    let b = u8::from_str_radix(hex.get(4..6).unwrap_or("80"), 16).unwrap_or(128) as f32 / 255.0;
    [r, g, b, alpha]
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
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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

/// 設定キー文字列（例: "ctrl+shift+p", "ctrl+b d"）と winit キーイベントを照合する
///
/// フォーマット: 修飾キー（ctrl/shift/alt/meta）と最終キー文字を `+` で区切る。
/// スペース区切りのプレフィックス（tmux 風: "ctrl+b d"）は先頭の修飾シーケンス + 末尾の単一文字として扱う。
fn config_key_matches(key_str: &str, code: WKeyCode, mods: ModifiersState) -> bool {
    // スペース区切りで最後のトークンをメインキーとして扱う（tmux プレフィックス互換は未実装 → 最後トークンのみ比較）
    let last_token = key_str.split_whitespace().last().unwrap_or(key_str);

    // `+` で分割して修飾キーとメインキーを取得する
    let parts: Vec<&str> = last_token.split('+').collect();
    if parts.is_empty() {
        return false;
    }

    let mut need_ctrl = false;
    let mut need_shift = false;
    let mut need_alt = false;
    let mut need_meta = false;
    let main_key = parts.last().expect("parts は split() で少なくとも1要素ある");

    for part in &parts[..parts.len() - 1] {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => need_ctrl = true,
            "shift" => need_shift = true,
            "alt" | "option" => need_alt = true,
            "meta" | "super" | "cmd" | "command" => need_meta = true,
            _ => {}
        }
    }

    // 修飾キーが一致しなければ false
    if need_ctrl != mods.control_key() { return false; }
    if need_shift != mods.shift_key() { return false; }
    if need_alt != mods.alt_key() { return false; }
    if need_meta != mods.super_key() { return false; }

    // メインキー文字列を winit KeyCode に変換して比較する
    key_str_to_keycode(main_key) == Some(code)
}

/// キー文字列を winit の KeyCode に変換する（簡易実装）
fn key_str_to_keycode(s: &str) -> Option<WKeyCode> {
    // 1 文字の場合は英数字として処理する
    if s.len() == 1 {
        let ch = s.chars().next().expect("s.len() == 1 なので必ず1文字ある");
        return char_to_keycode(ch);
    }
    // 特殊キー名
    match s.to_lowercase().as_str() {
        "enter" | "return" => Some(WKeyCode::Enter),
        "backspace" => Some(WKeyCode::Backspace),
        "delete" | "del" => Some(WKeyCode::Delete),
        "escape" | "esc" => Some(WKeyCode::Escape),
        "tab" => Some(WKeyCode::Tab),
        "space" => Some(WKeyCode::Space),
        "up" => Some(WKeyCode::ArrowUp),
        "down" => Some(WKeyCode::ArrowDown),
        "left" => Some(WKeyCode::ArrowLeft),
        "right" => Some(WKeyCode::ArrowRight),
        "home" => Some(WKeyCode::Home),
        "end" => Some(WKeyCode::End),
        "pageup" => Some(WKeyCode::PageUp),
        "pagedown" => Some(WKeyCode::PageDown),
        "insert" => Some(WKeyCode::Insert),
        "f1" => Some(WKeyCode::F1),
        "f2" => Some(WKeyCode::F2),
        "f3" => Some(WKeyCode::F3),
        "f4" => Some(WKeyCode::F4),
        "f5" => Some(WKeyCode::F5),
        "f6" => Some(WKeyCode::F6),
        "f7" => Some(WKeyCode::F7),
        "f8" => Some(WKeyCode::F8),
        "f9" => Some(WKeyCode::F9),
        "f10" => Some(WKeyCode::F10),
        "f11" => Some(WKeyCode::F11),
        "f12" => Some(WKeyCode::F12),
        _ => None,
    }
}

/// 1文字を winit の KeyCode に変換する
fn char_to_keycode(ch: char) -> Option<WKeyCode> {
    match ch {
        'a' | 'A' => Some(WKeyCode::KeyA),
        'b' | 'B' => Some(WKeyCode::KeyB),
        'c' | 'C' => Some(WKeyCode::KeyC),
        'd' | 'D' => Some(WKeyCode::KeyD),
        'e' | 'E' => Some(WKeyCode::KeyE),
        'f' | 'F' => Some(WKeyCode::KeyF),
        'g' | 'G' => Some(WKeyCode::KeyG),
        'h' | 'H' => Some(WKeyCode::KeyH),
        'i' | 'I' => Some(WKeyCode::KeyI),
        'j' | 'J' => Some(WKeyCode::KeyJ),
        'k' | 'K' => Some(WKeyCode::KeyK),
        'l' | 'L' => Some(WKeyCode::KeyL),
        'm' | 'M' => Some(WKeyCode::KeyM),
        'n' | 'N' => Some(WKeyCode::KeyN),
        'o' | 'O' => Some(WKeyCode::KeyO),
        'p' | 'P' => Some(WKeyCode::KeyP),
        'q' | 'Q' => Some(WKeyCode::KeyQ),
        'r' | 'R' => Some(WKeyCode::KeyR),
        's' | 'S' => Some(WKeyCode::KeyS),
        't' | 'T' => Some(WKeyCode::KeyT),
        'u' | 'U' => Some(WKeyCode::KeyU),
        'v' | 'V' => Some(WKeyCode::KeyV),
        'w' | 'W' => Some(WKeyCode::KeyW),
        'x' | 'X' => Some(WKeyCode::KeyX),
        'y' | 'Y' => Some(WKeyCode::KeyY),
        'z' | 'Z' => Some(WKeyCode::KeyZ),
        '0' => Some(WKeyCode::Digit0),
        '1' => Some(WKeyCode::Digit1),
        '2' => Some(WKeyCode::Digit2),
        '3' => Some(WKeyCode::Digit3),
        '4' => Some(WKeyCode::Digit4),
        '5' => Some(WKeyCode::Digit5),
        '6' => Some(WKeyCode::Digit6),
        '7' => Some(WKeyCode::Digit7),
        '8' => Some(WKeyCode::Digit8),
        '9' => Some(WKeyCode::Digit9),
        '%' => Some(WKeyCode::Digit5),    // Shift+5 = %
        '"' => Some(WKeyCode::Quote),
        '\'' => Some(WKeyCode::Quote),
        '[' => Some(WKeyCode::BracketLeft),
        ']' => Some(WKeyCode::BracketRight),
        '\\' => Some(WKeyCode::Backslash),
        '/' => Some(WKeyCode::Slash),
        '-' => Some(WKeyCode::Minus),
        '=' => Some(WKeyCode::Equal),
        ',' => Some(WKeyCode::Comma),
        '.' => Some(WKeyCode::Period),
        ';' => Some(WKeyCode::Semicolon),
        '`' => Some(WKeyCode::Backquote),
        _ => None,
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

    #[test]
    fn hex_to_rgba_変換() {
        let c = hex_to_rgba("#ae8b2d", 1.0);
        assert!((c[0] - 0xae as f32 / 255.0).abs() < 1e-3);
        assert!((c[1] - 0x8b as f32 / 255.0).abs() < 1e-3);
        assert!((c[2] - 0x2d as f32 / 255.0).abs() < 1e-3);
        assert_eq!(c[3], 1.0);
    }

    #[test]
    fn hex_to_rgba_ハッシュなし() {
        let c = hex_to_rgba("ffffff", 0.5);
        assert_eq!(c, [1.0, 1.0, 1.0, 0.5]);
    }
}
