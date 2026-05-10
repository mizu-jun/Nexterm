//! wgpu + winit レンダラー
//!
//! 描画パイプライン:
//!   1. ターミナルセルの背景色を頂点バッファで描画（カラーパス）
//!   2. cosmic-text でグリフをラスタライズし、グリフアトラスに書き込む
//!   3. グリフアトラスからサンプリングしてテキストを描画（テキストパス）
//!
//! サブモジュール:
//! - `glyph_atlas`  — GlyphAtlas + 頂点型 (BgVertex, TextVertex)
//! - `shaders`      — WGSL シェーダー定数
//! - `color_util`   — ANSI 色変換・16 進カラー解析
//! - `key_map`      — winit ↔ proto キーコード変換
//! - `vertex_util`  — 頂点バッファ生成ヘルパー

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{info, warn};
use winit::{dpi::PhysicalSize, window::Window};

use crate::font::FontManager;
use crate::state::ClientState;

// サブモジュールは main.rs で宣言済み（crate ルート）
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::shaders::{BG_SHADER, IMAGE_SHADER, TEXT_SHADER};
use crate::vertex_util::{add_px_rect, add_string_verts};

// ---- サブモジュール（Sprint 2-1 Phase A: vertex builders 抽出）----
mod grid_verts;
mod overlay_verts;
mod ui_verts;

// ---- サブモジュール（Sprint 2-1 Phase B/C: app + event_handler + input_handler 抽出）----
mod app;
mod event_handler;
mod input_handler;

pub use app::NextermApp;
pub use event_handler::EventHandler;

// ---- シェーダーファイル監視 ----

/// カスタムシェーダーファイルを監視するウォッチャーを起動する。
///
/// 設定にシェーダーパスがある場合のみ監視を開始する。
/// ファイルが変更されると `()` を受信チャネルに送信する。
pub(super) fn start_shader_watcher(
    gpu_cfg: &nexterm_config::GpuConfig,
) -> (
    Option<tokio::sync::mpsc::Receiver<()>>,
    Option<notify::RecommendedWatcher>,
) {
    use notify::{Event, RecursiveMode, Watcher};

    let paths: Vec<std::path::PathBuf> = [
        gpu_cfg.custom_bg_shader.as_deref(),
        gpu_cfg.custom_text_shader.as_deref(),
    ]
    .iter()
    .flatten()
    .map(|p| std::path::PathBuf::from(shellexpand::tilde(p).as_ref()))
    .filter(|p| p.exists())
    .collect();

    if paths.is_empty() {
        return (None, None);
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<()>(1);

    let mut watcher = match notify::recommended_watcher(move |result: notify::Result<Event>| {
        if let Ok(event) = result {
            use notify::EventKind::*;
            if matches!(event.kind, Modify(_) | Create(_)) {
                info!("シェーダーファイルの変更を検知しました。パイプラインを再構築します。");
                let _ = tx.blocking_send(());
            }
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            warn!("シェーダーウォッチャーの起動に失敗しました: {}", e);
            return (None, None);
        }
    };

    for path in &paths {
        if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
            warn!(
                "シェーダーファイルの監視に失敗しました: {}: {}",
                path.display(),
                e
            );
        } else {
            info!("シェーダーファイルを監視中: {}", path.display());
        }
    }

    (Some(rx), Some(watcher))
}

// ---- wgpu コアステート ----

/// wgpu の初期化済み状態
struct WgpuState {
    device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
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
    // ---- フレーム間再利用バッファ（毎フレームの GPU アロケーションを回避）----
    /// 背景頂点バッファ（VERTEX | COPY_DST、容量超過時は再確保）
    buf_bg_v: wgpu::Buffer,
    /// 背景インデックスバッファ
    buf_bg_i: wgpu::Buffer,
    /// テキスト頂点バッファ
    buf_txt_v: wgpu::Buffer,
    /// テキストインデックスバッファ
    buf_txt_i: wgpu::Buffer,
    /// 背景頂点バッファの現在容量（BgVertex 単位）
    bg_v_cap: u64,
    /// 背景インデックスバッファの現在容量（u16 単位）
    bg_i_cap: u64,
    /// テキスト頂点バッファの現在容量（TextVertex 単位）
    txt_v_cap: u64,
    /// テキストインデックスバッファの現在容量（u16 単位）
    txt_i_cap: u64,
    /// 最後にフレームを描画した時刻（FPS 制限用）
    last_frame_at: Instant,
}

impl WgpuState {
    async fn new(window: Arc<Window>, gpu_cfg: &nexterm_config::GpuConfig) -> Result<Self> {
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

        let present_mode = match gpu_cfg.present_mode {
            nexterm_config::PresentModeConfig::Fifo => wgpu::PresentMode::Fifo,
            nexterm_config::PresentModeConfig::Mailbox => wgpu::PresentMode::Mailbox,
            nexterm_config::PresentModeConfig::Auto => wgpu::PresentMode::AutoVsync,
        };

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

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// 1フレームを描画する
    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        state: &mut ClientState,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        tab_bar_cfg: &nexterm_config::TabBarConfig,
        color_scheme: &nexterm_config::ColorScheme,
        fps_limit: u32,
        background_opacity: f32,
        cursor_style: &nexterm_config::CursorStyle,
        padding_x: f32,
        padding_y: f32,
    ) -> Result<()> {
        // FPS 制限: 前フレームからの経過時間が 1/fps より短い場合はスキップ
        if fps_limit > 0 {
            let frame_duration = Duration::from_secs_f64(1.0 / fps_limit as f64);
            if self.last_frame_at.elapsed() < frame_duration {
                return Ok(());
            }
        }
        self.last_frame_at = Instant::now();

        // フレーム開始時にアトラスのリセットフラグをクリアする
        // （前フレームでリセットされていても、このフレームで正しいUVを使って再描画する）
        atlas.cleared_this_frame = false;

        // カラースキームからパレットを導出する（毎フレーム; コストは小さい）
        let scheme_palette: Option<nexterm_config::SchemePalette> = match color_scheme {
            nexterm_config::ColorScheme::Builtin(s) => Some(s.palette()),
            nexterm_config::ColorScheme::Custom(p) => {
                // Custom パレットを SchemePalette に変換
                let parse_hex = |s: &str| -> [u8; 3] {
                    let s = s.trim_start_matches('#');
                    let v = u32::from_str_radix(s, 16).unwrap_or(0);
                    [
                        ((v >> 16) & 0xFF) as u8,
                        ((v >> 8) & 0xFF) as u8,
                        (v & 0xFF) as u8,
                    ]
                };
                let mut ansi = [[0u8; 3]; 16];
                for (i, hex) in p.ansi.iter().enumerate().take(16) {
                    ansi[i] = parse_hex(hex);
                }
                Some(nexterm_config::SchemePalette {
                    fg: parse_hex(&p.foreground),
                    bg: parse_hex(&p.background),
                    ansi,
                })
            }
        };
        let palette_ref = scheme_palette.as_ref();
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

        // タブバー高さ（有効時のみ）: ターミナルコンテンツのy-offsetとして使用
        let tab_bar_h = if tab_bar_cfg.enabled {
            tab_bar_cfg.height as f32
        } else {
            0.0
        };
        // パディングを加味した実効オフセット（グリッド描画の基点）
        let _grid_offset_x = padding_x;
        let grid_offset_y = tab_bar_h + padding_y;

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
                if let (Some(layout), Some(pane)) =
                    (state.pane_layouts.get(&pane_id), state.panes.get(&pane_id))
                {
                    if pane.scroll_offset > 0 && is_focused {
                        self.build_scrollback_verts_in_rect(
                            pane,
                            layout,
                            sw,
                            sh,
                            cell_w,
                            cell_h,
                            grid_offset_y,
                            font,
                            atlas,
                            palette_ref,
                            &mut bg_verts,
                            &mut bg_idx,
                            &mut text_verts,
                            &mut text_idx,
                        );
                    } else {
                        self.build_grid_verts_in_rect(
                            pane,
                            layout,
                            is_focused,
                            &state.mouse_sel,
                            sw,
                            sh,
                            cell_w,
                            cell_h,
                            grid_offset_y,
                            font,
                            atlas,
                            palette_ref,
                            cursor_style,
                            &mut bg_verts,
                            &mut bg_idx,
                            &mut text_verts,
                            &mut text_idx,
                        );
                    }
                }
            }
            // ペイン境界線を描画する
            self.build_border_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                tab_bar_h,
                &mut bg_verts,
                &mut bg_idx,
            );
        } else if let Some(pane) = state.focused_pane() {
            // フォールバック: レイアウト情報なし（接続直後など）
            if pane.scroll_offset > 0 {
                // ---- スクロールバック表示モード ----
                self.build_scrollback_verts(
                    pane,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    grid_offset_y,
                    font,
                    atlas,
                    palette_ref,
                    &mut bg_verts,
                    &mut bg_idx,
                    &mut text_verts,
                    &mut text_idx,
                );
            } else {
                // ---- 通常グリッド表示 ----
                self.build_grid_verts(
                    pane,
                    &state.mouse_sel,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    grid_offset_y,
                    font,
                    atlas,
                    palette_ref,
                    cursor_style,
                    &mut bg_verts,
                    &mut bg_idx,
                    &mut text_verts,
                    &mut text_idx,
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
                    let py = layout.row_offset as f32 * cell_h + tab_bar_h;
                    let badge_w = cell_w * 2.0;
                    let badge_h = cell_h;
                    // 黄色背景バッジ
                    add_px_rect(
                        px,
                        py,
                        badge_w,
                        badge_h,
                        [0.9, 0.75, 0.0, 0.90],
                        sw,
                        sh,
                        &mut bg_verts,
                        &mut bg_idx,
                    );
                    // ペイン番号テキスト（1 始まり）
                    let label = (number + 1).to_string();
                    add_string_verts(
                        &label,
                        px + 2.0,
                        py,
                        [0.0, 0.0, 0.0, 1.0],
                        true,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        &mut text_verts,
                        &mut text_idx,
                    );
                }
            }
            // レイアウト情報がない場合（フォールバック: フォーカスペインのみ）
            if state.pane_layouts.is_empty()
                && let Some(focused_id) = state.focused_pane_id
            {
                add_px_rect(
                    0.0,
                    tab_bar_h,
                    cell_w * 2.0,
                    cell_h,
                    [0.9, 0.75, 0.0, 0.90],
                    sw,
                    sh,
                    &mut bg_verts,
                    &mut bg_idx,
                );
                let label = focused_id.to_string();
                add_string_verts(
                    &label,
                    2.0,
                    tab_bar_h,
                    [0.0, 0.0, 0.0, 1.0],
                    true,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    &mut text_verts,
                    &mut text_idx,
                );
            }
        }

        // ---- タブバー（設定で有効な場合）----
        if tab_bar_cfg.enabled {
            self.build_tab_bar_verts(
                state,
                tab_bar_cfg,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- ステータスライン（常時表示） ----
        self.build_status_verts(
            state,
            sw,
            sh,
            cell_w,
            cell_h,
            font,
            atlas,
            &mut bg_verts,
            &mut bg_idx,
            &mut text_verts,
            &mut text_idx,
        );

        // ---- 検索バー（アクティブ時） ----
        if state.search.is_active {
            self.build_search_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- Quick Select オーバーレイ（アクティブ時） ----
        if state.quick_select.is_active {
            self.build_quick_select_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- SFTP ファイル転送ダイアログ（オープン時） ----
        if state.file_transfer.is_open {
            self.build_file_transfer_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- Lua マクロピッカー（オープン時） ----
        if state.macro_picker.is_open {
            self.build_macro_picker_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- ホストマネージャ（オープン時） ----
        if state.host_manager.is_open {
            self.build_host_manager_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }
        if state.host_manager.password_modal.is_some() {
            self.build_password_modal_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- コマンドパレット（オープン時） ----
        if state.palette.is_open {
            self.build_palette_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- 設定パネル（Ctrl+, でオープン） ----
        if state.settings_panel.is_open {
            self.build_settings_panel_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- コンテキストメニュー（右クリック時） ----
        if let Some(ref menu) = state.context_menu {
            self.build_context_menu_verts(
                menu,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- 更新通知バナー（画面上部） ----
        if state.update_banner.is_some() {
            self.build_update_banner_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- 同意ダイアログ（Sprint 4-1: 機密操作確認モーダル）----
        // 最前面表示するため最後に追加する
        if state.pending_consent.is_some() {
            self.build_consent_dialog_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                font,
                atlas,
                &mut bg_verts,
                &mut bg_idx,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- IME プリエディットオーバーレイ（変換中テキスト） ----
        if let Some(ref preedit) = state.ime_preedit
            && let Some(pane) = state.focused_pane()
        {
            let px = pane.cursor_col as f32 * cell_w;
            let py = (pane.cursor_row + 1) as f32 * cell_h;
            // プリエディット背景（やや明るいグレー）
            let text_width = preedit.chars().count() as f32 * cell_w;
            add_px_rect(
                px,
                py,
                text_width.max(cell_w),
                cell_h,
                [0.25, 0.25, 0.30, 0.90],
                sw,
                sh,
                &mut bg_verts,
                &mut bg_idx,
            );
            // アンダーライン（黄色）
            add_px_rect(
                px,
                py + cell_h - 2.0,
                text_width.max(cell_w),
                2.0,
                [1.0, 0.85, 0.2, 1.0],
                sw,
                sh,
                &mut bg_verts,
                &mut bg_idx,
            );
            // プリエディットテキスト
            add_string_verts(
                preedit,
                px,
                py,
                [1.0, 1.0, 0.6, 1.0],
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                &mut text_verts,
                &mut text_idx,
            );
        }

        // ---- GPU バッファへアップロード（再利用バッファへ write_buffer で上書き）----
        // 容量不足の場合のみ新規確保する（2倍に拡張）
        self.upload_bg_verts(&bg_verts, &bg_idx);
        self.upload_txt_verts(&text_verts, &text_idx);

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
            // パレット背景色でクリアして黒い余白が残らないようにする
            let clear_bg = scheme_palette.as_ref().map(|p| p.bg).unwrap_or([0, 0, 0]);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_bg[0] as f64 / 255.0,
                            g: clear_bg[1] as f64 / 255.0,
                            b: clear_bg[2] as f64 / 255.0,
                            // background_opacity 設定値を alpha に反映（透過ターミナル対応）
                            a: background_opacity as f64,
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
                pass.set_vertex_buffer(0, self.buf_bg_v.slice(..));
                pass.set_index_buffer(self.buf_bg_i.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..bg_idx.len() as u32, 0, 0..1);
            }
            if !text_idx.is_empty() {
                pass.set_pipeline(&self.text_pipeline);
                pass.set_bind_group(0, &text_bind_group, &[]);
                pass.set_vertex_buffer(0, self.buf_txt_v.slice(..));
                pass.set_index_buffer(self.buf_txt_i.slice(..), wgpu::IndexFormat::Uint16);
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
                    let img_vbuf =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("img_vbuf"),
                                contents: bytemuck::cast_slice(&img_verts),
                                usage: wgpu::BufferUsages::VERTEX,
                            });
                    let img_ibuf =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
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

    /// 背景頂点・インデックスデータを再利用バッファへアップロードする
    ///
    /// バッファ容量が不足する場合は 2 倍に拡張して再確保する。
    fn upload_bg_verts(&mut self, verts: &[BgVertex], idx: &[u16]) {
        let v_bytes = bytemuck::cast_slice(verts);
        let i_bytes = bytemuck::cast_slice(idx);

        // 容量不足なら再確保
        if verts.len() as u64 > self.bg_v_cap {
            self.bg_v_cap = (verts.len() as u64 * 2).max(self.bg_v_cap);
            self.buf_bg_v = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bg_vertex_buffer"),
                size: self.bg_v_cap * std::mem::size_of::<BgVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if idx.len() as u64 > self.bg_i_cap {
            self.bg_i_cap = (idx.len() as u64 * 2).max(self.bg_i_cap);
            self.buf_bg_i = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bg_index_buffer"),
                size: self.bg_i_cap * std::mem::size_of::<u16>() as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        if !v_bytes.is_empty() {
            self.queue.write_buffer(&self.buf_bg_v, 0, v_bytes);
        }
        if !i_bytes.is_empty() {
            self.queue.write_buffer(&self.buf_bg_i, 0, i_bytes);
        }
    }

    /// テキスト頂点・インデックスデータを再利用バッファへアップロードする
    fn upload_txt_verts(&mut self, verts: &[TextVertex], idx: &[u16]) {
        let v_bytes = bytemuck::cast_slice(verts);
        let i_bytes = bytemuck::cast_slice(idx);

        if verts.len() as u64 > self.txt_v_cap {
            self.txt_v_cap = (verts.len() as u64 * 2).max(self.txt_v_cap);
            self.buf_txt_v = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("text_vertex_buffer"),
                size: self.txt_v_cap * std::mem::size_of::<TextVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if idx.len() as u64 > self.txt_i_cap {
            self.txt_i_cap = (idx.len() as u64 * 2).max(self.txt_i_cap);
            self.buf_txt_i = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("text_index_buffer"),
                size: self.txt_i_cap * std::mem::size_of::<u16>() as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        if !v_bytes.is_empty() {
            self.queue.write_buffer(&self.buf_txt_v, 0, v_bytes);
        }
        if !i_bytes.is_empty() {
            self.queue.write_buffer(&self.buf_txt_i, 0, i_bytes);
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
        self.image_textures.insert(
            id,
            ImageEntry {
                texture,
                bind_group,
            },
        );
    }

    /// カスタムシェーダーを再読み込みし bg/text パイプラインを再構築する。
    ///
    /// シェーダーファイルに構文エラーがあっても既存パイプラインは維持され、
    /// ログに警告を出してフォールバックする。
    fn reload_shader_pipelines(&mut self, gpu_cfg: &nexterm_config::GpuConfig) {
        let format = self.surface_config.format;

        // 背景シェーダー読み込み
        let bg_src: std::borrow::Cow<'static, str> =
            if let Some(ref path) = gpu_cfg.custom_bg_shader {
                let expanded = shellexpand::tilde(path).into_owned();
                match std::fs::read_to_string(&expanded) {
                    Ok(s) => {
                        info!(
                            "シェーダーホットリロード: 背景シェーダーを再読み込みしました: {}",
                            expanded
                        );
                        std::borrow::Cow::Owned(s)
                    }
                    Err(e) => {
                        warn!(
                            "背景シェーダーの再読み込みに失敗しました（既存を維持）: {}",
                            e
                        );
                        return;
                    }
                }
            } else {
                std::borrow::Cow::Borrowed(BG_SHADER)
            };

        // テキストシェーダー読み込み
        let text_src: std::borrow::Cow<'static, str> =
            if let Some(ref path) = gpu_cfg.custom_text_shader {
                let expanded = shellexpand::tilde(path).into_owned();
                match std::fs::read_to_string(&expanded) {
                    Ok(s) => {
                        info!(
                            "シェーダーホットリロード: テキストシェーダーを再読み込みしました: {}",
                            expanded
                        );
                        std::borrow::Cow::Owned(s)
                    }
                    Err(e) => {
                        warn!(
                            "テキストシェーダーの再読み込みに失敗しました（既存を維持）: {}",
                            e
                        );
                        return;
                    }
                }
            } else {
                std::borrow::Cow::Borrowed(TEXT_SHADER)
            };

        // 背景パイプラインを再構築する
        let bg_shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("bg_shader_hot"),
                source: wgpu::ShaderSource::Wgsl(bg_src),
            });
        let bg_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("bg_pipeline_layout"),
                bind_group_layouts: &[],
                push_constant_ranges: &[],
            });
        self.bg_pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("bg_pipeline"),
                layout: Some(&bg_layout),
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
                        // アルファブレンディングを有効化（画像オーバーレイパイプラインも同様）
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

        // テキストパイプラインを再構築する
        let text_shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("text_shader_hot"),
                source: wgpu::ShaderSource::Wgsl(text_src),
            });
        let text_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("text_pipeline_layout"),
                bind_group_layouts: &[&self.text_bind_group_layout],
                push_constant_ranges: &[],
            });
        self.text_pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text_pipeline"),
            layout: Some(&text_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TextVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4],
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
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        info!("シェーダーホットリロード完了");
    }
}

/// 配置済み画像の TextVertex リストを構築する
fn build_image_verts(
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

// wgpu::util のためのインポート
use wgpu::util::DeviceExt;

// ---- 画像テクスチャエントリ ----

/// GPU 画像テクスチャのキャッシュエントリ
struct ImageEntry {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}
