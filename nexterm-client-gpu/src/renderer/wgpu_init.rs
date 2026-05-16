//! wgpu 初期化・サーフェスリサイズ・PresentMode 選択
//!
//! `renderer/mod.rs` から抽出した:
//! - `impl WgpuState { async fn new }` — wgpu インスタンス・adapter・device・
//!   サーフェス・パイプライン（bg / text / image）・再利用バッファの初期化
//! - `impl WgpuState { fn resize }` — サーフェスサイズ更新
//! - `select_present_mode` — `gpu.present_mode` 設定とアダプタ対応モードから決定
//! - `present_mode_tests` — `select_present_mode` のユニットテスト

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use tracing::{info, warn};
use winit::{dpi::PhysicalSize, window::Window};

use crate::glyph_atlas::{BgVertex, TextVertex};
use crate::shaders::{BG_SHADER, IMAGE_SHADER, TEXT_SHADER};

use super::WgpuState;

impl WgpuState {
    pub(super) async fn new(
        window: Arc<Window>,
        gpu_cfg: &nexterm_config::GpuConfig,
    ) -> Result<Self> {
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

        // Sprint 5-3 / C3: 希望のモードがアダプタ非対応の場合、Fifo にフォールバックする。
        // Fifo は WebGPU 仕様上すべてのアダプタでサポートが保証されている。
        let present_mode = select_present_mode(&gpu_cfg.present_mode, &surface_caps.present_modes);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
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

        // ---- カスタムシェーダーの読み込み ----
        // gpu.custom_bg_shader / gpu.custom_text_shader が設定されている場合はファイルから読み込む。
        // 読み込み失敗時はビルトインシェーダーにフォールバックする。
        let bg_shader_src: std::borrow::Cow<'static, str> = if let Some(ref path) =
            gpu_cfg.custom_bg_shader
        {
            let expanded = shellexpand::tilde(path).into_owned();
            match std::fs::read_to_string(&expanded) {
                Ok(s) => {
                    info!("カスタム背景シェーダーを読み込みました: {}", expanded);
                    std::borrow::Cow::Owned(s)
                }
                Err(e) => {
                    warn!(
                        "カスタム背景シェーダーの読み込みに失敗しました（ビルトインを使用）: {}: {}",
                        expanded, e
                    );
                    std::borrow::Cow::Borrowed(BG_SHADER)
                }
            }
        } else {
            std::borrow::Cow::Borrowed(BG_SHADER)
        };

        let text_shader_src: std::borrow::Cow<'static, str> = if let Some(ref path) =
            gpu_cfg.custom_text_shader
        {
            let expanded = shellexpand::tilde(path).into_owned();
            match std::fs::read_to_string(&expanded) {
                Ok(s) => {
                    info!("カスタムテキストシェーダーを読み込みました: {}", expanded);
                    std::borrow::Cow::Owned(s)
                }
                Err(e) => {
                    warn!(
                        "カスタムテキストシェーダーの読み込みに失敗しました（ビルトインを使用）: {}: {}",
                        expanded, e
                    );
                    std::borrow::Cow::Borrowed(TEXT_SHADER)
                }
            }
        } else {
            std::borrow::Cow::Borrowed(TEXT_SHADER)
        };

        // ---- 背景矩形パイプライン ----
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bg_shader"),
            source: wgpu::ShaderSource::Wgsl(bg_shader_src),
        });

        let bg_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
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
                    // アルファブレンディングを有効化してグラスモーフィズム（半透明UI）を実現する
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
            source: wgpu::ShaderSource::Wgsl(text_shader_src),
        });

        let text_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
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

        // ---- 再利用バッファの初期確保 ----
        // 初期容量: 背景 8192 頂点・32768 インデックス（典型的な 80x24 ターミナルで十分）
        const INIT_BG_V: u64 = 8192;
        const INIT_BG_I: u64 = 32768;
        const INIT_TXT_V: u64 = 16384;
        const INIT_TXT_I: u64 = 65536;

        let buf_bg_v = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg_vertex_buffer"),
            size: INIT_BG_V * std::mem::size_of::<BgVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_bg_i = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg_index_buffer"),
            size: INIT_BG_I * std::mem::size_of::<u16>() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_txt_v = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text_vertex_buffer"),
            size: INIT_TXT_V * std::mem::size_of::<TextVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let buf_txt_i = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text_index_buffer"),
            size: INIT_TXT_I * std::mem::size_of::<u16>() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
            buf_bg_v,
            buf_bg_i,
            buf_txt_v,
            buf_txt_i,
            bg_v_cap: INIT_BG_V,
            bg_i_cap: INIT_BG_I,
            txt_v_cap: INIT_TXT_V,
            txt_i_cap: INIT_TXT_I,
            last_frame_at: Instant::now(),
        })
    }

    pub(super) fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }
}

/// Sprint 5-3 / C3: 設定値とアダプタの対応モードから実際の `wgpu::PresentMode` を決定する。
///
/// - 希望が `Fifo`: 常に `Fifo`（WebGPU 仕様上必ずサポートされる）
/// - 希望が `Mailbox`: サポートされていれば `Mailbox`、それ以外は `Fifo` にフォールバック
/// - 希望が `Auto`: `AutoVsync` がサポートされていればそれ、なければ `Fifo`
///
/// この関数は単純なため、surface に依存せずスライスを受け取ってユニットテスト可能にしている。
fn select_present_mode(
    desired: &nexterm_config::PresentModeConfig,
    supported: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
    match desired {
        nexterm_config::PresentModeConfig::Fifo => wgpu::PresentMode::Fifo,
        nexterm_config::PresentModeConfig::Mailbox => {
            if supported.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                tracing::info!(
                    "present_mode=mailbox は本アダプタでサポートされていません。fifo にフォールバックします。"
                );
                wgpu::PresentMode::Fifo
            }
        }
        nexterm_config::PresentModeConfig::Auto => {
            if supported.contains(&wgpu::PresentMode::AutoVsync) {
                wgpu::PresentMode::AutoVsync
            } else if supported.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::Fifo
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod present_mode_tests {
    use super::*;
    use nexterm_config::PresentModeConfig;

    #[test]
    fn fifo_is_always_fifo() {
        // Fifo は supported に関わらず常に Fifo
        assert_eq!(
            select_present_mode(&PresentModeConfig::Fifo, &[wgpu::PresentMode::Mailbox]),
            wgpu::PresentMode::Fifo
        );
        assert_eq!(
            select_present_mode(&PresentModeConfig::Fifo, &[]),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn mailbox_uses_mailbox_when_supported() {
        assert_eq!(
            select_present_mode(
                &PresentModeConfig::Mailbox,
                &[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox]
            ),
            wgpu::PresentMode::Mailbox
        );
    }

    #[test]
    fn mailbox_falls_back_to_fifo_when_unsupported() {
        assert_eq!(
            select_present_mode(&PresentModeConfig::Mailbox, &[wgpu::PresentMode::Fifo]),
            wgpu::PresentMode::Fifo
        );
        // supported が空でも Fifo に落ちる
        assert_eq!(
            select_present_mode(&PresentModeConfig::Mailbox, &[]),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn auto_prefers_auto_vsync_then_mailbox_then_fifo() {
        // AutoVsync 対応時はそれを優先
        assert_eq!(
            select_present_mode(
                &PresentModeConfig::Auto,
                &[wgpu::PresentMode::AutoVsync, wgpu::PresentMode::Mailbox]
            ),
            wgpu::PresentMode::AutoVsync
        );
        // AutoVsync なしなら Mailbox
        assert_eq!(
            select_present_mode(
                &PresentModeConfig::Auto,
                &[wgpu::PresentMode::Mailbox, wgpu::PresentMode::Fifo]
            ),
            wgpu::PresentMode::Mailbox
        );
        // どちらもなければ Fifo
        assert_eq!(
            select_present_mode(&PresentModeConfig::Auto, &[wgpu::PresentMode::Fifo]),
            wgpu::PresentMode::Fifo
        );
    }
}
