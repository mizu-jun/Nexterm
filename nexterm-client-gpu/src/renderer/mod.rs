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
    connection::{Connection, ConnectionExt},
    font::FontManager,
    state::{ClientState, ContextMenu, ContextMenuAction},
};

// サブモジュールは main.rs で宣言済み（crate ルート）
use crate::glyph_atlas::{BgVertex, GlyphAtlas, GlyphKey, TextVertex};
use crate::key_map::{
    config_key_matches, physical_to_proto_key, proto_modifiers, winit_code_to_char,
};
use crate::shaders::{BG_SHADER, IMAGE_SHADER, TEXT_SHADER};
use crate::vertex_util::{add_px_rect, add_string_verts, grid_to_text, open_url, visual_width};

// ---- サブモジュール（Sprint 2-1 Phase A: vertex builders 抽出）----
mod grid_verts;
mod overlay_verts;
mod ui_verts;

// ---- シェーダーファイル監視 ----

/// カスタムシェーダーファイルを監視するウォッチャーを起動する。
///
/// 設定にシェーダーパスがある場合のみ監視を開始する。
/// ファイルが変更されると `()` を受信チャネルに送信する。
fn start_shader_watcher(
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

// ---- アプリケーション本体 ----

/// GPU アプリケーション（winit EventLoop に渡す）
pub struct NextermApp {
    config: Config,
    state: ClientState,
    font: FontManager,
}

impl NextermApp {
    pub async fn new(config: Config) -> Result<Self> {
        let font = FontManager::new(
            &config.font.family,
            config.font.size,
            &config.font.font_fallbacks,
            1.0,
            config.font.ligatures,
        );
        let mut state = ClientState::new(80, 24, config.scrollback_lines);
        // 設定ファイルのホスト一覧をホストマネージャに渡す
        state.host_manager = crate::host_manager::HostManager::new(config.hosts.clone());
        // 設定ファイルの Lua マクロ一覧をマクロピッカーに渡す
        state.macro_picker = crate::macro_picker::MacroPicker::new(config.macros.clone());
        // 設定パネルを設定値で初期化する
        state.settings_panel = crate::settings_panel::SettingsPanel::new(&config);
        Ok(Self {
            config,
            state,
            font,
        })
    }

    pub fn into_event_handler(
        self,
        config_rx: Option<tokio::sync::mpsc::Receiver<Config>>,
        config_watcher: Option<notify::RecommendedWatcher>,
        status_eval: Option<StatusBarEvaluator>,
        server_handle: tokio::task::JoinHandle<()>,
        update_rx: tokio::sync::watch::Receiver<Option<String>>,
    ) -> EventHandler {
        // カスタムシェーダーファイルが設定されていれば監視を開始する
        let (shader_reload_rx, _shader_watcher) = start_shader_watcher(&self.config.gpu);

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
            shader_reload_rx,
            _shader_watcher,
            last_tab_click: None,
            server_handle,
            pixel_scroll_accumulator: 0.0,
            update_rx,
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
    /// シェーダーファイル変更通知チャネル（Some = カスタムシェーダー監視中）
    shader_reload_rx: Option<tokio::sync::mpsc::Receiver<()>>,
    /// シェーダーファイル監視ウォッチャー
    _shader_watcher: Option<notify::RecommendedWatcher>,
    /// タブのダブルクリック検出用（最終クリック時刻とペイン ID）
    last_tab_click: Option<(Instant, u32)>,
    /// 内部サーバータスクのハンドル（ウィンドウ終了時に abort する）
    server_handle: tokio::task::JoinHandle<()>,
    /// タッチパッド精密スクロール（PixelDelta）の積算バッファ
    pixel_scroll_accumulator: f64,
    /// 更新チェッカーからの通知受信チャネル（Some(version) = 新バージョンあり）
    update_rx: tokio::sync::watch::Receiver<Option<String>>,
}

/// 設定パネルに対するマウスヒットテスト結果
enum SettingsPanelHit {
    /// パネル外をクリック → パネルを閉じる
    Outside,
    /// タイトルバーエリア（ドラッグ移動等の将来拡張用）
    TitleBar,
    /// サイドバーカテゴリをクリック
    Category(usize),
    /// スライダーをクリック/ドラッグ
    Slider {
        slider_type: crate::settings_panel::SliderType,
        track_x: f32,
        track_w: f32,
        #[allow(dead_code)]
        min: f32,
        #[allow(dead_code)]
        max: f32,
    },
    /// テーマカラードット
    ThemeColor(usize),
    /// パネル内の空白エリア（何もしない）
    PanelBackground,
}

impl EventHandler {
    /// 設定パネルに対するマウスヒットテストを実行する
    fn hit_test_settings_panel(&self, cx: f32, cy: f32) -> SettingsPanelHit {
        use crate::settings_panel::{SettingsCategory, SliderType};

        let sp = &self.app.state.settings_panel;
        if !sp.is_open {
            return SettingsPanelHit::Outside;
        }
        let (sw, sh) = match self.wgpu_state.as_ref() {
            Some(w) => (
                w.surface_config.width as f32,
                w.surface_config.height as f32,
            ),
            None => return SettingsPanelHit::Outside,
        };
        let cell_w = self.app.font.cell_width();
        let cell_h = self.app.font.cell_height();

        // パネル寸法 (build_settings_panel_verts と同じ式)
        let panel_w = (sw * 0.72).min(sw - cell_w * 4.0);
        let panel_h = (sh * 0.75).min(sh - cell_h * 4.0);
        let px = (sw - panel_w) / 2.0;
        let eased = sp.eased_progress();
        let slide_offset = (1.0 - eased) * 16.0;
        let py = (sh - panel_h) / 2.0 + slide_offset;

        let sidebar_w = cell_w * 18.0;
        let content_x = px + sidebar_w;
        let content_w = panel_w - sidebar_w;
        let content_inner_x = content_x + cell_w;

        // パネル外 → 閉じる
        if cx < px || cx > px + panel_w || cy < py || cy > py + panel_h {
            return SettingsPanelHit::Outside;
        }

        // タイトルバー
        let title_h = cell_h * 1.4;
        if cy < py + title_h {
            return SettingsPanelHit::TitleBar;
        }

        // サイドバーカテゴリ
        let sidebar_top = py + title_h;
        let cat_item_h = cell_h * 1.3;
        if cx < px + sidebar_w {
            let rel_y = cy - sidebar_top;
            if rel_y >= 0.0 {
                let cat_idx = (rel_y / cat_item_h) as usize;
                if cat_idx < SettingsCategory::ALL.len() {
                    return SettingsPanelHit::Category(cat_idx);
                }
            }
            return SettingsPanelHit::PanelBackground;
        }

        // コンテンツ領域ヒットテスト
        let content_top = py + title_h + cell_h * 0.5;
        let bar_w = content_w - cell_w * 3.0;

        match &sp.category {
            SettingsCategory::Font => {
                // フォントサイズスライダー
                let bar_y = content_top + cell_h * 4.2;
                if cy >= bar_y - cell_h * 0.5
                    && cy <= bar_y + cell_h
                    && cx >= content_inner_x
                    && cx <= content_inner_x + bar_w
                {
                    return SettingsPanelHit::Slider {
                        slider_type: SliderType::FontSize,
                        track_x: content_inner_x,
                        track_w: bar_w,
                        min: 8.0,
                        max: 32.0,
                    };
                }
            }
            SettingsCategory::Theme => {
                // テーマカラードット
                let dot_y = content_top + cell_h * 2.5;
                let dot_gap = (content_w - cell_w * 2.0) / 9.0;
                let dot_size = cell_w * 1.2;
                if cy >= dot_y && cy <= dot_y + cell_h {
                    for i in 0..9_usize {
                        let dot_x = content_inner_x + i as f32 * dot_gap;
                        if cx >= dot_x && cx <= dot_x + dot_size {
                            return SettingsPanelHit::ThemeColor(i);
                        }
                    }
                }
            }
            SettingsCategory::Window => {
                // 不透明度スライダー
                let bar_y = content_top + cell_h * 2.4;
                if cy >= bar_y - cell_h * 0.5
                    && cy <= bar_y + cell_h
                    && cx >= content_inner_x
                    && cx <= content_inner_x + bar_w
                {
                    return SettingsPanelHit::Slider {
                        slider_type: SliderType::WindowOpacity,
                        track_x: content_inner_x,
                        track_w: bar_w,
                        min: 0.1,
                        max: 1.0,
                    };
                }
            }
            _ => {}
        }

        SettingsPanelHit::PanelBackground
    }
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
            .with_title("Nexterm")
            .with_inner_size(PhysicalSize::new(1280u32, 800u32))
            .with_transparent(transparent)
            .with_decorations(decorations);

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("Failed to create window"),
        );

        // アプリケーションアイコンを設定する
        {
            let icon_bytes = include_bytes!("../../../assets/nexterm-source.png");
            if let Ok(img) = image::load_from_memory(icon_bytes) {
                let rgba = img.into_rgba8();
                let (iw, ih) = (rgba.width(), rgba.height());
                if let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), iw, ih) {
                    window.set_window_icon(Some(icon));
                }
            }
        }

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
            self.app.config.font.ligatures,
        );

        // Acrylic（すりガラス）背景を適用する（Windows 11 のみ有効）
        #[cfg(windows)]
        crate::platform::apply_acrylic_blur(&window);

        // wgpu を非同期で初期化する（tokio runtime が必要）
        let wgpu_state = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(WgpuState::new(Arc::clone(&window), &self.app.config.gpu))
        })
        .expect("Failed to initialize wgpu");

        let mut atlas =
            GlyphAtlas::new_with_config(&wgpu_state.device, self.app.config.gpu.atlas_size);

        // ASCII 印字可能文字（0x20-0x7E）をグリフアトラスに事前ロードする。
        // 初回のキーストローク遅延を排除し、起動直後からスムーズな描画を実現する。
        for ch in ' '..='~' {
            for bold in [false, true] {
                let key = GlyphKey {
                    ch,
                    bold,
                    italic: false,
                    wide: false,
                };
                let (w, h, pixels) =
                    self.app
                        .font
                        .rasterize_char(ch, bold, false, [220, 220, 220, 255], false);
                if w > 0 && h > 0 {
                    atlas.get_or_insert(key, &pixels, w, h, &wgpu_state.queue);
                }
            }
        }

        // ウィンドウサイズからセル数を計算してステートを初期化する
        // タブバー（上部）とステータスバー（下部1セル）を除いた領域でセル数を計算する
        let size = window.inner_size();
        let cell_h_init = self.app.font.cell_height();
        let tab_bar_h_init = if self.app.config.tab_bar.enabled {
            self.app.config.tab_bar.height as f32
        } else {
            0.0
        };
        let status_bar_h_init = cell_h_init;
        let pad_x = self.app.config.window.padding_x as f32;
        let pad_y = self.app.config.window.padding_y as f32;
        let cols = ((size.width as f32 - pad_x * 2.0) / self.app.font.cell_width()).max(1.0) as u16;
        let rows = ((size.height as f32 - tab_bar_h_init - status_bar_h_init - pad_y * 2.0)
            / cell_h_init)
            .max(1.0) as u16;
        self.app.state.resize(cols, rows);

        self.window = Some(Arc::clone(&window));
        self.atlas = Some(atlas);
        self.wgpu_state = Some(wgpu_state);

        // サーバーに接続してデフォルトセッションにアタッチする
        let conn = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                match Connection::connect_gpu().await {
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
            && let Ok(new_config) = rx.try_recv()
        {
            info!(
                "Config reloaded: font={} {}pt",
                new_config.font.family, new_config.font.size
            );
            // フォントサイズ変更時はグリフアトラスも再生成する
            let font_changed = self.app.config.font != new_config.font;
            self.app.config = new_config;
            if font_changed {
                self.app.font = crate::font::FontManager::new(
                    &self.app.config.font.family,
                    self.app.config.font.size,
                    &self.app.config.font.font_fallbacks,
                    self.scale_factor,
                    self.app.config.font.ligatures,
                );
                let atlas_size = self.app.config.gpu.atlas_size;
                if let Some(wgpu) = &self.wgpu_state {
                    self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
                }
            }
            had_messages = true;
        }

        // カスタムシェーダーファイルの変更をポーリングしてパイプラインを再構築する
        if let Some(rx) = &mut self.shader_reload_rx
            && rx.try_recv().is_ok()
        {
            // チャネルをドレインして複数イベントを 1 回にまとめる
            while rx.try_recv().is_ok() {}
            if let Some(wgpu) = &mut self.wgpu_state {
                wgpu.reload_shader_pipelines(&self.app.config.gpu);
            }
            had_messages = true;
        }

        // ステータスバーを 1 秒ごとに再評価してキャッシュを更新する
        if self.app.config.status_bar.enabled
            && self.last_status_eval.elapsed() >= Duration::from_secs(1)
            && let Some(eval) = &self.status_eval
        {
            let ctx = nexterm_config::WidgetContext {
                session_name: Some("main".to_string()),
                pane_id: self.app.state.focused_pane_id,
            };
            let sep = &self.app.config.status_bar.separator;
            self.app.state.status_bar_text =
                eval.evaluate_with_context(&self.app.config.status_bar.widgets, &ctx, sep);
            self.app.state.status_bar_right_text =
                eval.evaluate_with_context(&self.app.config.status_bar.right_widgets, &ctx, sep);
            self.last_status_eval = Instant::now();
            had_messages = true;
        }

        // 更新チェッカーからの通知をポーリングしてバナーを表示する
        if self.update_rx.has_changed().unwrap_or(false)
            && let Some(ver) = self.update_rx.borrow_and_update().clone()
            && self.app.state.update_banner.is_none()
        {
            self.app.state.update_banner = Some(ver);
            had_messages = true;
        }

        if had_messages && let Some(w) = &self.window {
            w.request_redraw();
        }

        // 設定パネルの開閉アニメーションを進める（60fps 想定で約 8フレーム = 0.13秒）
        let sp = &mut self.app.state.settings_panel;
        if sp.is_open && sp.open_progress < 1.0 {
            sp.open_progress = (sp.open_progress + 0.15).min(1.0);
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
                // IPC 接続を先にドロップしてチャネルを閉じる（Windows でのハング防止）
                self.connection = None;
                // サーバータスクを abort してからイベントループを終了する
                self.server_handle.abort();
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                let cell_h_r = self.app.font.cell_height();
                let tab_bar_h_r = if self.app.config.tab_bar.enabled {
                    self.app.config.tab_bar.height as f32
                } else {
                    0.0
                };
                let pad_x_r = self.app.config.window.padding_x as f32;
                let pad_y_r = self.app.config.window.padding_y as f32;
                let cols = ((size.width as f32 - pad_x_r * 2.0) / self.app.font.cell_width())
                    .max(1.0) as u16;
                let rows = ((size.height as f32 - tab_bar_h_r - cell_h_r - pad_y_r * 2.0)
                    / cell_h_r)
                    .max(1.0) as u16;
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
                    self.app.config.font.ligatures,
                );
                // スケール変更でグリフが無効化されるためアトラスを再生成する
                let atlas_size = self.app.config.gpu.atlas_size;
                if let Some(wgpu) = &self.wgpu_state {
                    self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
                }
                // DPI 変更後のセルサイズ変更に合わせて cols/rows を再計算してサーバーに通知する
                if let Some(win) = &self.window {
                    let size = win.inner_size();
                    let cell_h_sf = self.app.font.cell_height();
                    let tab_bar_h_sf = if self.app.config.tab_bar.enabled {
                        self.app.config.tab_bar.height as f32
                    } else {
                        0.0
                    };
                    let pad_x_sf = self.app.config.window.padding_x as f32;
                    let pad_y_sf = self.app.config.window.padding_y as f32;
                    let cols = ((size.width as f32 - pad_x_sf * 2.0) / self.app.font.cell_width())
                        .max(1.0) as u16;
                    let rows = ((size.height as f32 - tab_bar_h_sf - cell_h_sf - pad_y_sf * 2.0)
                        / cell_h_sf)
                        .max(1.0) as u16;
                    self.app.state.resize(cols, rows);
                    if let Some(conn) = &self.connection {
                        let _ = conn.send_tx.try_send(ClientToServer::Resize { cols, rows });
                    }
                }
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            // マウスカーソル位置を追跡する（ドラッグ中は選択範囲を更新する）
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some((position.x, position.y));
                let cell_w = self.app.font.cell_width() as f64;
                let cell_h = self.app.font.cell_height() as f64;
                let tab_bar_h_f64 = if self.app.config.tab_bar.enabled {
                    self.app.config.tab_bar.height as f64
                } else {
                    0.0_f64
                };
                let col = (position.x / cell_w) as u16;
                let row = ((position.y - tab_bar_h_f64).max(0.0) / cell_h) as u16;
                if self.app.state.mouse_sel.is_dragging {
                    self.app.state.mouse_sel.update(col, row);
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                    // ドラッグ中もマウスモーションをレポートする（ボタン0=左ドラッグ）
                    if let Some(conn) = &self.connection {
                        let _ = conn.send_tx.try_send(ClientToServer::MouseReport {
                            button: 0,
                            col,
                            row,
                            pressed: true,
                            motion: true,
                        });
                    }
                }

                // 設定パネルのスライダーをドラッグ中の場合、値をリアルタイム更新する
                {
                    let fx = position.x as f32;
                    let sp = &mut self.app.state.settings_panel;
                    if let Some(drag) = &sp.drag_slider.clone() {
                        use crate::settings_panel::SliderType;
                        match drag.slider_type {
                            SliderType::FontSize => {
                                sp.set_font_size_from_slider(fx, drag.track_x, drag.track_w);
                            }
                            SliderType::WindowOpacity => {
                                sp.set_opacity_from_slider(fx, drag.track_x, drag.track_w);
                            }
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                    }
                }

                // コンテキストメニューが開いている場合はホバー項目を更新する
                if let Some(menu) = &mut self.app.state.context_menu {
                    let cw = self.app.font.cell_width();
                    let ch = self.app.font.cell_height();
                    let menu_w = 18.0 * cw;
                    let fx = position.x as f32;
                    let fy = position.y as f32;
                    let mut new_hovered = None;
                    if fx >= menu.x && fx <= menu.x + menu_w {
                        for (i, _item) in menu.items.iter().enumerate() {
                            let item_y = menu.y + i as f32 * ch;
                            if fy >= item_y && fy < item_y + ch {
                                new_hovered = Some(i);
                                break;
                            }
                        }
                    }
                    if menu.hovered != new_hovered {
                        menu.hovered = new_hovered;
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
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
                    let cell_w_ctx = self.app.font.cell_width() as f64;
                    let cell_h_ctx = self.app.font.cell_height() as f64;
                    let profile_list: Vec<(String, String)> = self
                        .app
                        .config
                        .profiles
                        .iter()
                        .map(|p| (p.name.clone(), p.icon.clone()))
                        .collect();
                    let tmp = ContextMenu::new_default(0.0, 0.0, &profile_list);
                    let item_count = tmp.items.len();
                    // メニュー幅を描画側と同じロジックで計算する
                    let max_label = tmp
                        .items
                        .iter()
                        .map(|i| visual_width(&i.label))
                        .max()
                        .unwrap_or(8);
                    let max_hint = tmp
                        .items
                        .iter()
                        .map(|i| visual_width(&i.hint))
                        .max()
                        .unwrap_or(0);
                    let menu_w_px = ((max_label + max_hint + 5) as f64).max(16.0) * cell_w_ctx;
                    let menu_h_px = item_count as f64 * cell_h_ctx;

                    // ウィンドウ内に収まるように位置をクランプする
                    let win_w = self
                        .window
                        .as_ref()
                        .map(|w| w.inner_size().width as f64)
                        .unwrap_or(800.0);
                    let win_h = self
                        .window
                        .as_ref()
                        .map(|w| w.inner_size().height as f64)
                        .unwrap_or(600.0);
                    let menu_x = (px).min(win_w - menu_w_px).max(0.0) as f32;
                    let menu_y = (py).min(win_h - menu_h_px).max(0.0) as f32;

                    self.app.state.context_menu =
                        Some(ContextMenu::new_default(menu_x, menu_y, &profile_list));
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            // 左ボタン押下: タブバークリック判定 + 選択開始 + マウスレポート
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } => {
                if let Some((px, py)) = self.cursor_position {
                    // 設定パネルが開いている場合はヒットテストを先に実行する
                    if self.app.state.settings_panel.is_open {
                        let hit = self.hit_test_settings_panel(px as f32, py as f32);
                        use crate::settings_panel::SliderType;
                        match hit {
                            SettingsPanelHit::Outside => {
                                // パネル外クリック → パネルを閉じる
                                self.app.state.settings_panel.close();
                            }
                            SettingsPanelHit::Category(idx) => {
                                // サイドバーカテゴリをクリック → カテゴリ切り替え
                                if let Some(cat) =
                                    crate::settings_panel::SettingsCategory::ALL.get(idx)
                                {
                                    self.app.state.settings_panel.category = cat.clone();
                                }
                            }
                            SettingsPanelHit::Slider {
                                slider_type,
                                track_x,
                                track_w,
                                min: _,
                                max: _,
                            } => {
                                // スライダーをクリック → 即時値を反映してドラッグ状態を開始する
                                let fx = px as f32;
                                let sp = &mut self.app.state.settings_panel;
                                match slider_type {
                                    SliderType::FontSize => {
                                        sp.set_font_size_from_slider(fx, track_x, track_w)
                                    }
                                    SliderType::WindowOpacity => {
                                        sp.set_opacity_from_slider(fx, track_x, track_w)
                                    }
                                }
                                sp.drag_slider = Some(crate::settings_panel::SliderDrag {
                                    slider_type,
                                    track_x,
                                    track_w,
                                    min_val: if matches!(slider_type, SliderType::FontSize) {
                                        8.0
                                    } else {
                                        0.1
                                    },
                                    max_val: if matches!(slider_type, SliderType::FontSize) {
                                        32.0
                                    } else {
                                        1.0
                                    },
                                });
                            }
                            SettingsPanelHit::ThemeColor(idx) => {
                                // テーマカラードットをクリック → スキーム切り替え
                                self.app.state.settings_panel.scheme_index = idx;
                                self.app.state.settings_panel.dirty = true;
                            }
                            SettingsPanelHit::TitleBar | SettingsPanelHit::PanelBackground => {
                                // その他のパネル内クリック → 何もしない
                            }
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                        return; // 設定パネルが開いている間はターミナルにクリックを伝えない
                    }

                    let cell_w = self.app.font.cell_width() as f64;
                    let cell_h = self.app.font.cell_height() as f64;
                    let tab_bar_h_f64 = if self.app.config.tab_bar.enabled {
                        self.app.config.tab_bar.height as f64
                    } else {
                        0.0_f64
                    };

                    // タブバーエリア（py < tab_bar_h）のクリックを処理する
                    if self.app.config.tab_bar.enabled && py < tab_bar_h_f64 {
                        let px_f32 = px as f32;
                        // 設定ボタンのクリック判定
                        let hit_settings = self
                            .app
                            .state
                            .settings_tab_rect
                            .map(|(x0, x1)| px_f32 >= x0 && px_f32 < x1)
                            .unwrap_or(false);
                        if hit_settings {
                            self.app.state.settings_panel.is_open =
                                !self.app.state.settings_panel.is_open;
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                        } else {
                            // タブクリックでペインフォーカスを切り替える
                            let hit_pane = self
                                .app
                                .state
                                .tab_hit_rects
                                .iter()
                                .find(|&(_, &(x0, x1))| px_f32 >= x0 && px_f32 < x1)
                                .map(|(&id, _)| id);
                            if let Some(pane_id) = hit_pane {
                                let now = Instant::now();
                                // ダブルクリック判定（300ms 以内に同一ペインを再クリック）
                                let is_double_click = self
                                    .last_tab_click
                                    .map(|(t, id)| {
                                        id == pane_id
                                            && now.duration_since(t) < Duration::from_millis(300)
                                    })
                                    .unwrap_or(false);

                                if is_double_click {
                                    // ダブルクリック → タブ名変更モードへ
                                    let current_name = self
                                        .app
                                        .state
                                        .panes
                                        .get(&pane_id)
                                        .map(|p| p.title.clone())
                                        .filter(|t| !t.is_empty())
                                        .unwrap_or_else(|| format!("pane:{}", pane_id));
                                    self.app
                                        .state
                                        .settings_panel
                                        .begin_tab_rename(pane_id, &current_name);
                                    self.last_tab_click = None;
                                } else {
                                    self.last_tab_click = Some((now, pane_id));
                                    if self.app.state.focused_pane_id != Some(pane_id)
                                        && let Some(conn) = &self.connection
                                    {
                                        let _ = conn
                                            .send_tx
                                            .try_send(ClientToServer::FocusPane { pane_id });
                                    }
                                }
                            }
                        }
                        return; // タブバー内のクリックはターミナルに伝えない
                    }

                    let col = (px / cell_w) as u16;
                    let row = ((py - tab_bar_h_f64).max(0.0) / cell_h) as u16;
                    self.app.state.mouse_sel.begin(col, row);
                    // マウスレポーティングが有効なら PTY にイベントを送信する
                    if let Some(conn) = &self.connection {
                        let _ = conn.send_tx.try_send(ClientToServer::MouseReport {
                            button: 0,
                            col,
                            row,
                            pressed: true,
                            motion: false,
                        });
                    }
                }
            }

            // 左ボタンリリース: 選択確定 → クリップボードコピー or フォーカス切替
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } => {
                // 設定パネルのスライダードラッグを終了して設定を保存する
                if self.app.state.settings_panel.drag_slider.take().is_some() {
                    let _ = self.app.state.settings_panel.save_to_toml();
                    self.app.state.settings_panel.dirty = false;
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }

                // コンテキストメニューが開いている場合はクリックで処理する
                if let Some((px, py)) = self.cursor_position
                    && let Some(menu) = self.app.state.context_menu.take()
                {
                    let cell_w = self.app.font.cell_width();
                    let cell_h = self.app.font.cell_height();
                    // 描画幅と同じ値を使用する（ここを変えると描画とクリック判定がずれる）
                    let menu_w = 18.0 * cell_w;
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
                    let tab_bar_h_f64 = if self.app.config.tab_bar.enabled {
                        self.app.config.tab_bar.height as f64
                    } else {
                        0.0_f64
                    };
                    let click_col = (px / cell_w) as u16;
                    let click_row = ((py - tab_bar_h_f64).max(0.0) / cell_h) as u16;

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
                                    let line: String = row
                                        [col_start.min(row.len())..col_end.min(row.len())]
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
                            && let Ok(mut clipboard) = arboard::Clipboard::new()
                        {
                            let _ = clipboard.set_text(text);
                        }
                        // 選択後はリターン（ペインフォーカス切替を行わない）
                        return;
                    }

                    // 選択なし（単純クリック）: Ctrl+クリックで URL を開く
                    if self.modifiers.control_key()
                        && let Some(url) = self.find_url_at(click_col, click_row)
                    {
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
                        && let Some(conn) = &self.connection
                    {
                        let _ = conn.send_tx.try_send(ClientToServer::FocusPane { pane_id });
                    }
                }
            }

            // マウスホイールでスクロールバックをスクロールする
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => {
                        // Windows タッチパッドは PixelDelta を送る。
                        // 積算してセル高さ分溜まったら1行スクロールし、端数は次回に持ち越す。
                        self.pixel_scroll_accumulator += p.y;
                        let cell_h = self.app.font.cell_height() as f64;
                        let lines = (self.pixel_scroll_accumulator / cell_h) as i32;
                        self.pixel_scroll_accumulator -= lines as f64 * cell_h;
                        lines
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
                        && !self.modifiers.control_key()
                    {
                        for ch in t.chars() {
                            self.app.state.push_search_char(ch);
                        }
                    }
                    // Escape / Enter は handle_key で処理する
                    if let PhysicalKey::Code(code) = physical_key
                        && matches!(code, WKeyCode::Escape | WKeyCode::Enter)
                    {
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

                // 設定パネルのフォントファミリー入力中は文字をフィールドに追加する
                if !consumed
                    && self.app.state.settings_panel.is_open
                    && self.app.state.settings_panel.font_family_editing
                {
                    if let Some(ref t) = text
                        && !self.modifiers.control_key()
                        && !self.modifiers.alt_key()
                    {
                        for ch in t.chars() {
                            self.app.state.settings_panel.push_font_family_char(ch);
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                        return;
                    }
                    // テキストがない場合（矢印キー等）もサーバーへは転送しない
                    return;
                }

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
                if let (Some(wgpu), Some(atlas)) = (&mut self.wgpu_state, &mut self.atlas)
                    && let Err(e) = wgpu.render(
                        &mut self.app.state,
                        &mut self.app.font,
                        atlas,
                        &self.app.config.tab_bar,
                        &self.app.config.colors,
                        self.app.config.gpu.fps_limit,
                        self.app.config.window.background_opacity,
                        &self.app.config.cursor_style,
                        self.app.config.window.padding_x as f32,
                        self.app.config.window.padding_y as f32,
                    )
                {
                    warn!("Render error: {}", e);
                }

                // GlyphAtlas の動的拡張: 満杯になったら 2 倍サイズで再生成する
                // 借用競合を避けるため atlas を一時的に取り出して処理する
                if let Some(mut atlas) = self.atlas.take() {
                    if atlas.needs_grow
                        && let Some(wgpu) = &self.wgpu_state
                    {
                        atlas = atlas.grow(&wgpu.device);
                    }
                    self.atlas = Some(atlas);
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
                && let Some(conn) = &self.connection
            {
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
                self.app
                    .state
                    .macro_picker
                    .reload(self.app.config.macros.clone());
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
                self.app
                    .state
                    .host_manager
                    .reload(self.app.config.hosts.clone());
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
                    if !ft.host_name.is_empty()
                        && !ft.local_path.is_empty()
                        && !ft.remote_path.is_empty()
                    {
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

        // タブ名変更モード中のキー処理（全キーを消費）
        if self.app.state.settings_panel.tab_rename_editing.is_some() {
            match code {
                WKeyCode::Escape => {
                    self.app.state.settings_panel.cancel_tab_rename();
                }
                WKeyCode::Enter => {
                    let rename_id = self.app.state.settings_panel.tab_rename_editing;
                    let new_name = self.app.state.settings_panel.tab_rename_text.clone();
                    self.app.state.settings_panel.cancel_tab_rename();
                    if let (Some(window_id), Some(conn)) = (rename_id, &self.connection)
                        && !new_name.is_empty()
                    {
                        let _ = conn.send_tx.try_send(ClientToServer::RenameWindow {
                            window_id,
                            name: new_name,
                        });
                    }
                }
                WKeyCode::Backspace => {
                    self.app.state.settings_panel.pop_tab_rename_char();
                }
                _ => {
                    // 英字・数字・記号を入力する
                    if let Some(ch) = winit_code_to_char(code) {
                        let ch = if self.modifiers.shift_key() {
                            ch.to_uppercase().next().unwrap_or(ch)
                        } else {
                            ch
                        };
                        self.app.state.settings_panel.push_tab_rename_char(ch);
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
            let editing = self.app.state.settings_panel.font_family_editing;
            match code {
                WKeyCode::Escape => {
                    if editing {
                        // 編集モードを終了する（変更を破棄せず入力モードだけ終了）
                        self.app.state.settings_panel.font_family_editing = false;
                    } else {
                        self.app.state.settings_panel.close();
                    }
                }
                WKeyCode::Enter => {
                    if editing {
                        // 編集モードを確定する
                        self.app.state.settings_panel.font_family_editing = false;
                    } else {
                        let _ = self.app.state.settings_panel.save_to_toml();
                        self.app.state.settings_panel.close();
                    }
                }
                WKeyCode::Backspace if editing => {
                    self.app.state.settings_panel.pop_font_family_char();
                }
                // F キーで Font カテゴリのフォントファミリー編集モードをトグルする
                WKeyCode::KeyF if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Font {
                        self.app.state.settings_panel.font_family_editing = true;
                    }
                }
                WKeyCode::Tab | WKeyCode::ArrowDown if !editing => {
                    self.app.state.settings_panel.next_category();
                }
                WKeyCode::ArrowUp if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    match &self.app.state.settings_panel.category {
                        SettingsCategory::Font => {
                            self.app.state.settings_panel.increase_font_size()
                        }
                        SettingsCategory::Window => {
                            self.app.state.settings_panel.increase_opacity()
                        }
                        _ => self.app.state.settings_panel.prev_category(),
                    }
                }
                WKeyCode::ArrowRight if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    match &self.app.state.settings_panel.category {
                        SettingsCategory::Theme => self.app.state.settings_panel.next_scheme(),
                        SettingsCategory::Startup => self.app.state.settings_panel.next_language(),
                        _ => {}
                    }
                }
                WKeyCode::ArrowLeft if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    match &self.app.state.settings_panel.category {
                        SettingsCategory::Theme => self.app.state.settings_panel.prev_scheme(),
                        SettingsCategory::Startup => self.app.state.settings_panel.prev_language(),
                        _ => {}
                    }
                }
                // Space: Startup カテゴリの auto_check_update トグル
                WKeyCode::Space if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Startup {
                        let sp = &mut self.app.state.settings_panel;
                        sp.auto_check_update = !sp.auto_check_update;
                    }
                }
                WKeyCode::BracketRight if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Theme {
                        self.app.state.settings_panel.next_scheme();
                    }
                }
                WKeyCode::BracketLeft if !editing => {
                    use crate::settings_panel::SettingsCategory;
                    if self.app.state.settings_panel.category == SettingsCategory::Theme {
                        self.app.state.settings_panel.prev_scheme();
                    }
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

        // 更新通知バナーが表示中のとき: Esc で閉じる、Enter でブラウザを開く
        if self.app.state.update_banner.is_some() {
            match code {
                WKeyCode::Escape => {
                    self.app.state.update_banner = None;
                    return true;
                }
                WKeyCode::Enter => {
                    crate::platform::open_releases_url();
                    self.app.state.update_banner = None;
                    return true;
                }
                _ => {}
            }
        }

        // ホストマネージャが開いているときのナビゲーション（全キーを消費）
        // パスワードモーダルが開いている場合は専用処理
        if self.app.state.host_manager.password_modal.is_some() {
            match code {
                WKeyCode::Escape => {
                    self.app.state.host_manager.password_modal = None;
                }
                WKeyCode::Tab => {
                    // OS キーチェーン保存フラグの切り替え（Sprint 3-2 後半）
                    if let Some(m) = &mut self.app.state.host_manager.password_modal {
                        m.toggle_remember();
                    }
                }
                WKeyCode::Backspace => {
                    if let Some(m) = &mut self.app.state.host_manager.password_modal {
                        m.pop_char();
                    }
                }
                WKeyCode::Enter => {
                    if let Some(m) = &mut self.app.state.host_manager.password_modal {
                        let host = m.host.clone();
                        let password = m.take_password();
                        self.app.state.host_manager.password_modal = None;
                        self.app.state.host_manager.record_connection(&host);
                        self.connect_ssh_host_with_password(&host, password);
                    }
                }
                _ => {
                    if let Some(ch) = winit_code_to_char(code)
                        && let Some(m) = &mut self.app.state.host_manager.password_modal
                    {
                        m.push_char(ch);
                    }
                }
            }
            return true;
        }

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
                        if host.auth_type == "password" {
                            // パスワード認証ホストはモーダルを開いてから接続する
                            self.app.state.host_manager.password_modal =
                                Some(crate::host_manager::PasswordModal::new(host));
                        } else {
                            self.app.state.host_manager.record_connection(&host);
                            self.connect_ssh_host_new_tab(&host);
                        }
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
        if self.app.state.search.is_active {
            match code {
                // Enter: 次のマッチへ / Shift+Enter: 前のマッチへ
                WKeyCode::Enter => {
                    if shift {
                        self.app.state.search_prev();
                    } else {
                        self.app.state.search_next();
                    }
                    return true;
                }
                // N: 前のマッチへ（vim 慣習）
                WKeyCode::KeyN if shift => {
                    self.app.state.search_prev();
                    return true;
                }
                _ => {}
            }
        }

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

        // OSC 8 ハイパーリンクを優先チェックする
        for span in &pane.grid.hyperlinks {
            if span.row == row && col >= span.col_start && col < span.col_end {
                return Some(span.url.clone());
            }
        }

        // テキストパターンから URL を動的検出する
        let cells = pane.grid.rows.get(row as usize)?;
        let urls = detect_urls_in_row(row, cells);
        urls.into_iter()
            .find(|u| u.contains(col, row))
            .map(|u| u.url)
    }

    /// コピーモードのキー入力を処理する（true = 消費済み）
    fn handle_copy_mode_key(&mut self, code: WKeyCode) -> bool {
        // 検索入力中は専用ハンドラに委譲する
        if self.app.state.copy_mode.search_query.is_some() {
            return self.handle_copy_mode_search_key(code);
        }

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
            // $: 行末へ移動
            WKeyCode::Digit4 => {
                // Shift+4 = '$' として扱う（WKeyCode には Dollar がないため）
                cm.cursor_col = max_col;
            }
            // w: 次の単語の先頭へ移動
            WKeyCode::KeyW => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_next_word_start(col, row, max_col, max_row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // b: 前の単語の先頭へ移動
            WKeyCode::KeyB => {
                let (col, row) = (cm.cursor_col, cm.cursor_row);
                if let Some((nc, nr)) = self.find_prev_word_start(col, row) {
                    let cm = &mut self.app.state.copy_mode;
                    cm.cursor_col = nc;
                    cm.cursor_row = nr;
                }
            }
            // v: 選択開始/終了をトグル
            WKeyCode::KeyV => {
                cm.toggle_selection();
            }
            // y / Y: y=選択テキストをヤンク、Y=行全体をヤンク
            WKeyCode::KeyY => {
                if self.modifiers.shift_key() {
                    self.yank_current_line();
                } else {
                    self.yank_selection();
                }
            }
            // /: インクリメンタル検索モードへ
            WKeyCode::Slash => {
                self.app.state.copy_mode.search_query = Some(String::new());
            }
            // n: 次の検索結果へ
            WKeyCode::KeyN => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_query
                    .clone()
                    .unwrap_or_default();
                if !q.is_empty() {
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    if let Some((nc, nr)) = self.search_forward(&q, col + 1, row, max_col, max_row)
                    {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// 検索入力中のキー処理（true = 消費済み）
    fn handle_copy_mode_search_key(&mut self, code: WKeyCode) -> bool {
        match code {
            // Escape: 検索をキャンセルして通常コピーモードへ
            WKeyCode::Escape => {
                self.app.state.copy_mode.search_query = None;
            }
            // Enter: 検索確定して最初のマッチへジャンプ
            WKeyCode::Enter => {
                let q = self
                    .app
                    .state
                    .copy_mode
                    .search_query
                    .clone()
                    .unwrap_or_default();
                self.app.state.copy_mode.search_query = None;
                if !q.is_empty() {
                    let max_col = self.app.state.cols.saturating_sub(1);
                    let max_row = self.app.state.rows.saturating_sub(1);
                    let (col, row) = (
                        self.app.state.copy_mode.cursor_col,
                        self.app.state.copy_mode.cursor_row,
                    );
                    if let Some((nc, nr)) = self.search_forward(&q, col, row, max_col, max_row) {
                        self.app.state.copy_mode.cursor_col = nc;
                        self.app.state.copy_mode.cursor_row = nr;
                        // 最後の検索クエリを保存して n キーで再利用できるようにする
                        self.app.state.copy_mode.search_query = Some(q);
                    }
                }
            }
            // Backspace: クエリの末尾を削除
            WKeyCode::Backspace => {
                if let Some(ref mut q) = self.app.state.copy_mode.search_query {
                    q.pop();
                }
            }
            _ => return false,
        }
        true
    }

    /// 次の単語の先頭位置を返す（見つからなければ None）
    fn find_next_word_start(
        &self,
        col: u16,
        row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as usize;
        let mut r = row as usize;

        // 現在位置が単語文字なら単語の終わりまでスキップ
        if let Some(cells) = pane.grid.rows.get(r) {
            while c < cells.len() && !cells[c].ch.is_whitespace() {
                c += 1;
            }
        }
        // 次の単語の先頭（空白をスキップ）
        loop {
            if let Some(cells) = pane.grid.rows.get(r) {
                while c < cells.len() {
                    if !cells[c].ch.is_whitespace() {
                        return Some((c as u16, r as u16));
                    }
                    c += 1;
                }
            }
            // 次の行へ
            if r >= max_row as usize {
                break;
            }
            r += 1;
            c = 0;
        }
        Some((max_col, max_row))
    }

    /// 前の単語の先頭位置を返す（見つからなければ None）
    fn find_prev_word_start(&self, col: u16, row: u16) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let mut c = col as isize - 1;
        let mut r = row as isize;

        // 現在位置の直前が空白ならスキップ
        loop {
            if c < 0 {
                if r <= 0 {
                    return Some((0, 0));
                }
                r -= 1;
                c = pane
                    .grid
                    .rows
                    .get(r as usize)
                    .map(|row| row.len() as isize - 1)
                    .unwrap_or(0);
            }
            if let Some(cells) = pane.grid.rows.get(r as usize)
                && c < cells.len() as isize
                && !cells[c as usize].ch.is_whitespace()
            {
                break;
            }
            c -= 1;
        }
        // 単語の先頭までスキップ
        loop {
            if c <= 0 {
                return Some((0, r as u16));
            }
            if let Some(cells) = pane.grid.rows.get(r as usize) {
                if c - 1 < cells.len() as isize && cells[(c - 1) as usize].ch.is_whitespace() {
                    break;
                }
            } else {
                break;
            }
            c -= 1;
        }
        Some((c as u16, r as u16))
    }

    /// 前方検索: クエリに最初にマッチする (col, row) を返す
    fn search_forward(
        &self,
        query: &str,
        start_col: u16,
        start_row: u16,
        max_col: u16,
        max_row: u16,
    ) -> Option<(u16, u16)> {
        let pane = self.app.state.focused_pane()?;
        let rows_total = (max_row + 1) as usize;

        for dr in 0..rows_total {
            let r = ((start_row as usize) + dr) % rows_total;
            let cells = pane.grid.rows.get(r)?;
            let row_str: String = cells.iter().map(|c| c.ch).collect();
            let col_start = if dr == 0 { start_col as usize } else { 0 };
            let search_in = if col_start < row_str.len() {
                &row_str[col_start..]
            } else {
                continue;
            };
            if let Some(offset) = search_in.find(query) {
                let found_col = (col_start + offset).min(max_col as usize) as u16;
                return Some((found_col, r as u16));
            }
        }
        None
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
                && let Ok(mut clipboard) = arboard::Clipboard::new()
            {
                let _ = clipboard.set_text(text);
            }
        }
        self.app.state.copy_mode.exit();
    }

    /// カーソル行全体をクリップボードにコピーしてコピーモードを終了する（Y キー）
    fn yank_current_line(&mut self) {
        let row_idx = self.app.state.copy_mode.cursor_row as usize;
        let text = if let Some(pane) = self.app.state.focused_pane() {
            pane.grid
                .rows
                .get(row_idx)
                .map(|row| row.iter().map(|c| c.ch).collect::<String>())
                .unwrap_or_default()
        } else {
            String::new()
        };
        if !text.is_empty()
            && let Ok(mut clipboard) = arboard::Clipboard::new()
        {
            let _ = clipboard.set_text(text);
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
        self.app.font = crate::font::FontManager::new(
            &self.app.config.font.family,
            new_size,
            &self.app.config.font.font_fallbacks,
            self.scale_factor,
            self.app.config.font.ligatures,
        );
        let atlas_size = self.app.config.gpu.atlas_size;
        if let Some(wgpu) = &self.wgpu_state {
            self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
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
        self.app.font = crate::font::FontManager::new(
            &self.app.config.font.family,
            default_size,
            &self.app.config.font.font_fallbacks,
            self.scale_factor,
            self.app.config.font.ligatures,
        );
        let atlas_size = self.app.config.gpu.atlas_size;
        if let Some(wgpu) = &self.wgpu_state {
            self.atlas = Some(GlyphAtlas::new_with_config(&wgpu.device, atlas_size));
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
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::SetBroadcast { enabled: true });
                }
            }
            "SetBroadcastOff" => {
                if let Some(conn) = &self.connection {
                    let _ = conn
                        .send_tx
                        .try_send(ClientToServer::SetBroadcast { enabled: false });
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
                        let target = layouts
                            .iter()
                            .filter(|l| l.pane_id != focused)
                            .map(|l| l.pane_id)
                            .min_by_key(|&id| if id > focused { id - focused } else { u32::MAX })
                            .or_else(|| {
                                layouts.iter().map(|l| l.pane_id).find(|&id| id != focused)
                            });
                        if let Some(target_id) = target {
                            let _ = conn.send_tx.try_send(ClientToServer::SwapPane {
                                target_pane_id: target_id,
                            });
                        }
                    }
                }
            }
            "SwapPanePrev" => {
                if let Some(conn) = &self.connection {
                    let layouts: Vec<_> = self.app.state.pane_layouts.values().collect();
                    if layouts.len() >= 2 {
                        let focused = self.app.state.focused_pane_id.unwrap_or(0);
                        let target = layouts
                            .iter()
                            .filter(|l| l.pane_id != focused)
                            .map(|l| l.pane_id)
                            .min_by_key(|&id| if id < focused { focused - id } else { u32::MAX })
                            .or_else(|| {
                                layouts.iter().map(|l| l.pane_id).find(|&id| id != focused)
                            });
                        if let Some(target_id) = target {
                            let _ = conn.send_tx.try_send(ClientToServer::SwapPane {
                                target_pane_id: target_id,
                            });
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
                self.app
                    .state
                    .host_manager
                    .reload(self.app.config.hosts.clone());
                self.app.state.host_manager.open();
            }
            "ShowMacroPicker" => {
                self.app
                    .state
                    .macro_picker
                    .reload(self.app.config.macros.clone());
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
                    let (port, baud_rate, data_bits, stop_bits, parity) =
                        if let Some(cfg) = serial_cfg {
                            (
                                cfg.port,
                                cfg.baud_rate,
                                cfg.data_bits,
                                cfg.stop_bits,
                                cfg.parity,
                            )
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
                    && let Some(conn) = &self.connection
                {
                    let _ = conn.send_tx.try_send(ClientToServer::PasteText { text });
                }
            }
            ContextMenuAction::SelectAll => {
                // グリッド全体のテキストをクリップボードにコピーする
                if let Some(pane) = self.app.state.focused_pane() {
                    let text = grid_to_text(pane);
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
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
            ContextMenuAction::InlineSearch => {
                self.app.state.start_search();
            }
            ContextMenuAction::OpenSettings => {
                self.app.state.settings_panel.open();
            }
            ContextMenuAction::OpenProfile { profile_name } => {
                // プロファイルのシェル設定でペインを新規分割する
                if let Some(prof) = self
                    .app
                    .config
                    .profiles
                    .iter()
                    .find(|p| &p.name == profile_name)
                    && let Some(shell) = &prof.shell
                    && let Some(conn) = &self.connection
                {
                    // まず垂直分割してから ConnectSsh の代わりにシェルパスを環境変数で渡す
                    // （現時点では SplitVertical で新ペインを開き、プロファイル設定はログとして記録）
                    let _ = conn.send_tx.try_send(ClientToServer::SplitVertical);
                    info!(
                        "プロファイル '{}' のシェル '{}' で起動を要求",
                        profile_name, shell.program
                    );
                }
            }
            ContextMenuAction::Separator => {
                // セパレーターはクリック不可のため何もしない
            }
        }
    }

    /// HostConfig から ConnectSsh メッセージを送信する（現在のペインに接続）
    #[allow(dead_code)]
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

    /// パスワード付きで新しいタブを開いて ConnectSsh メッセージを送信する（パスワード認証ホスト用）
    ///
    /// HIGH H-6: パスワード文字列は `Zeroizing<String>` で受け取り、
    /// IPC 送信後に drop されてメモリゼロクリアされる。
    fn connect_ssh_host_with_password(
        &self,
        host: &nexterm_config::HostConfig,
        password: zeroize::Zeroizing<String>,
    ) {
        let Some(conn) = &self.connection else { return };
        let _ = conn.send_tx.try_send(ClientToServer::NewWindow);
        // IPC bincode シリアライズ時に String が必要なので一時 clone する。
        // 旧 password は関数終了時に drop されゼロクリアされる。
        // TODO(future): IPC 経由で平文パスワードを送る代わりに OS keyring に
        // 一時保存する設計に変更する（HIGH H-6 の根本対策）。
        let pwd_string: Option<String> = if password.is_empty() {
            None
        } else {
            Some((*password).clone())
        };
        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password: pwd_string,
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
            && !text_str.is_empty()
        {
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

// wgpu::util のためのインポート
use wgpu::util::DeviceExt;

// ---- 画像テクスチャエントリ ----

/// GPU 画像テクスチャのキャッシュエントリ
struct ImageEntry {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}
