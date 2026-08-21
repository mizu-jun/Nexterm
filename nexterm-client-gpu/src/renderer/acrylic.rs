//! In-app acrylic capture bookkeeping and GPU resources (UI/UX v3 P2b).
//! `AcrylicCaptureState` and the Kawase tap-offset functions below are pure
//! state — no wgpu types — so the invalidation rules are unit-testable
//! without a GPU. `AcrylicState` further down owns the actual offscreen
//! textures, blur pipelines and bind group, and therefore cannot be
//! behavior-tested here (see `resource_shape_tests` for what *can* be
//! checked without a `wgpu::Device`).

use bytemuck::{Pod, Zeroable};

/// Tracks when the offscreen `scene_color` capture needs to be redone.
/// The capture is a frozen snapshot taken once per overlay-open
/// transition; it is intentionally *not* refreshed every frame while an
/// overlay stays open (see the design spec's "Non-goals").
#[derive(Debug, Default)]
pub(crate) struct AcrylicCaptureState {
    last_overlay_open_count: usize,
    generation: u64,
    captured_generation: Option<u64>,
    captured_while_open: bool,
}

impl AcrylicCaptureState {
    /// Call once per frame with how many overlays are currently open.
    pub(crate) fn note_overlay_open_count(&mut self, count: usize) {
        let was_open = self.last_overlay_open_count > 0;
        let now_open = count > 0;
        if !was_open && now_open {
            // 0 -> N transition: force a fresh capture.
            self.captured_generation = None;
        }
        self.last_overlay_open_count = count;
    }

    /// Call whenever the window resizes or the DPI scale changes.
    pub(crate) fn note_resize(&mut self) {
        self.generation += 1;
    }

    /// Whether the caller should (re-)capture `scene_color` and re-run the
    /// blur chain this frame. `overlay_open` must match what
    /// `note_overlay_open_count` was last told.
    pub(crate) fn is_dirty(&self, overlay_open: bool) -> bool {
        overlay_open && self.captured_generation != Some(self.generation)
    }

    /// Call after a capture + blur pass has run this frame.
    pub(crate) fn mark_captured(&mut self) {
        self.captured_generation = Some(self.generation);
        self.captured_while_open = true;
        let _ = self.captured_while_open; // silence unused-field lint until Task 7 reads it
    }
}

/// The 4 diagonal taps of one dual-Kawase downsample pass, in texels.
/// Mirrors the WGSL `kawase_downsample` entry point in `shaders.rs` —
/// keep the two in sync.
#[allow(dead_code)] // CPU-side spec for the WGSL shader's tap math; exercised only by its own unit tests (see Task 5).
pub(crate) fn kawase_downsample_offsets(texel_size: (f32, f32)) -> [(f32, f32); 4] {
    let (hx, hy) = (texel_size.0 * 0.5, texel_size.1 * 0.5);
    [(-hx, -hy), (hx, -hy), (-hx, hy), (hx, hy)]
}

/// The 8 taps of one dual-Kawase upsample pass (4 axis-aligned taps at
/// 2 texels, weight 1; 4 diagonal taps at 1 texel, weight 2 — applied by
/// the shader, not encoded here). Mirrors `kawase_upsample` in
/// `shaders.rs`.
#[allow(dead_code)] // CPU-side spec for the WGSL shader's tap math; exercised only by its own unit tests (see Task 5).
pub(crate) fn kawase_upsample_offsets(texel_size: (f32, f32)) -> [(f32, f32); 8] {
    let (x, y) = texel_size;
    [
        (-2.0 * x, 0.0),
        (2.0 * x, 0.0),
        (0.0, -2.0 * y),
        (0.0, 2.0 * y),
        (-x, y),
        (x, y),
        (-x, -y),
        (x, -y),
    ]
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct BlurParamsUniform {
    pub texel_size: [f32; 2],
    pub _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct AcrylicUniformData {
    pub tint: [f32; 4],
    pub viewport_size: [f32; 2],
    pub strength: f32,
    pub _pad: f32,
}

/// One level of the Kawase ping-pong chain: a texture, its view, and the
/// bind group that samples the *previous* level into it.
struct BlurLevel {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl BlurLevel {
    fn create(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        label: &str,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            width: width.max(1),
            height: height.max(1),
        }
    }
}

/// Offscreen resources for the in-app acrylic material (UI/UX v3 P2b).
/// Created once at `WgpuState::new` with a 1x1 placeholder so `bg_pipeline`
/// always has a valid bind group, resized (recreated) whenever the surface
/// size changes.
///
/// Deliberately does *not* own an `acrylic_bind_group_layout` field: that
/// layout is created once on `WgpuState` (Task 4) and shared by reference
/// with `bg_pipeline_layout`, `shader_reload.rs`, and this state's
/// `acrylic_bind_group` — a second, independently-created layout with the
/// same entries would be a distinct wgpu object and incompatible with bind
/// groups built against the original.
pub(crate) struct AcrylicState {
    format: wgpu::TextureFormat,
    scene_color: BlurLevel,
    half_res: BlurLevel,
    quarter_res: BlurLevel,
    blurred_result: BlurLevel,
    sampler: wgpu::Sampler,
    blur_bind_group_layout: wgpu::BindGroupLayout,
    blur_pipeline_layout: wgpu::PipelineLayout,
    downsample_pipeline: wgpu::RenderPipeline,
    upsample_pipeline: wgpu::RenderPipeline,
    /// Bound to `bg_pipeline`'s group 0 every frame; points at
    /// `blurred_result` once a capture has run, or at a 1x1 transparent
    /// placeholder before the first capture / when the feature is off.
    acrylic_bind_group: wgpu::BindGroup,
    acrylic_uniform_buf: wgpu::Buffer,
    placeholder_texture: wgpu::Texture,
    placeholder_view: wgpu::TextureView,
}

impl AcrylicState {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        acrylic_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("acrylic_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let placeholder_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("acrylic_placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let placeholder_view =
            placeholder_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let acrylic_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("acrylic_uniform"),
            size: std::mem::size_of::<AcrylicUniformData>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let acrylic_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acrylic_sample_bind_group"),
            layout: acrylic_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&placeholder_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(
                        acrylic_uniform_buf.as_entire_buffer_binding(),
                    ),
                },
            ],
        });

        let blur_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("kawase_blur_bind_group_layout"),
                entries: &[
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kawase_blur_pipeline_layout"),
            bind_group_layouts: &[&blur_bind_group_layout],
            push_constant_ranges: &[],
        });

        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kawase_blur_shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::KAWASE_BLUR_SHADER.into()),
        });

        let make_blur_pipeline = |entry_point: &str, label: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&blur_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &blur_shader,
                    entry_point: "vs_fullscreen",
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &blur_shader,
                    entry_point,
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let downsample_pipeline = make_blur_pipeline("fs_downsample", "kawase_downsample_pipeline");
        let upsample_pipeline = make_blur_pipeline("fs_upsample", "kawase_upsample_pipeline");

        Self {
            format,
            scene_color: BlurLevel::create(device, format, 1, 1, "acrylic_scene_color"),
            half_res: BlurLevel::create(device, format, 1, 1, "acrylic_half_res"),
            quarter_res: BlurLevel::create(device, format, 1, 1, "acrylic_quarter_res"),
            blurred_result: BlurLevel::create(device, format, 1, 1, "acrylic_blurred_result"),
            sampler,
            blur_bind_group_layout,
            blur_pipeline_layout,
            downsample_pipeline,
            upsample_pipeline,
            acrylic_bind_group,
            acrylic_uniform_buf,
            placeholder_texture,
            placeholder_view,
        }
    }

    /// Recreate the offscreen chain at the new surface size. Cheap to call
    /// unconditionally on every resize — it only reallocates when the size
    /// actually changed.
    ///
    /// Also rebuilds `acrylic_bind_group`: it was built once in `new()`
    /// (or by the previous call to this function) pointing at that
    /// generation's `blurred_result` view. Recreating the `BlurLevel`s
    /// without rebuilding the bind group would leave it referencing a
    /// dropped/stale `TextureView` while the shader keeps sampling through
    /// it — a wgpu validation error at best, silently-wrong rendering at
    /// worst.
    pub(crate) fn ensure_size(
        &mut self,
        device: &wgpu::Device,
        acrylic_bind_group_layout: &wgpu::BindGroupLayout,
        width: u32,
        height: u32,
    ) {
        if self.scene_color.width == width && self.scene_color.height == height {
            return;
        }
        self.scene_color =
            BlurLevel::create(device, self.format, width, height, "acrylic_scene_color");
        self.half_res = BlurLevel::create(
            device,
            self.format,
            (width / 2).max(1),
            (height / 2).max(1),
            "acrylic_half_res",
        );
        self.quarter_res = BlurLevel::create(
            device,
            self.format,
            (width / 4).max(1),
            (height / 4).max(1),
            "acrylic_quarter_res",
        );
        self.blurred_result =
            BlurLevel::create(device, self.format, width, height, "acrylic_blurred_result");

        self.acrylic_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("acrylic_sample_bind_group"),
            layout: acrylic_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.blurred_result.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(
                        self.acrylic_uniform_buf.as_entire_buffer_binding(),
                    ),
                },
            ],
        });
    }
}

#[cfg(test)]
mod resource_shape_tests {
    use super::*;

    #[test]
    fn blur_uniform_matches_wgsl_struct_layout() {
        // BlurParams in KAWASE_BLUR_SHADER is { texel_size: vec2<f32>, _pad: vec2<f32> }
        // = 16 bytes, satisfying wgpu's 16-byte uniform alignment without
        // an explicit #[repr(align(16))].
        assert_eq!(std::mem::size_of::<BlurParamsUniform>(), 16);
    }

    #[test]
    fn acrylic_uniform_matches_wgsl_struct_layout() {
        // AcrylicUniform in BG_SHADER is { tint: vec4<f32>, viewport_size: vec2<f32>, strength: f32, _pad: f32 } = 32 bytes.
        assert_eq!(std::mem::size_of::<AcrylicUniformData>(), 32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_from_zero_to_one_overlay_is_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(0);
        assert!(!state.is_dirty(false));
        state.note_overlay_open_count(1);
        assert!(state.is_dirty(true));
    }

    #[test]
    fn staying_open_with_more_overlays_is_not_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        assert!(!state.is_dirty(true));
        state.note_overlay_open_count(2);
        assert!(!state.is_dirty(true));
    }

    #[test]
    fn resize_while_open_marks_dirty() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        assert!(!state.is_dirty(true));
        state.note_resize();
        assert!(state.is_dirty(true));
    }

    #[test]
    fn resize_while_closed_does_not_force_a_capture() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(0);
        state.note_resize();
        assert!(!state.is_dirty(false));
    }

    #[test]
    fn closing_and_reopening_recaptures() {
        let mut state = AcrylicCaptureState::default();
        state.note_overlay_open_count(1);
        state.mark_captured();
        state.note_overlay_open_count(0);
        state.note_overlay_open_count(1);
        assert!(state.is_dirty(true));
    }
}

#[cfg(test)]
mod offset_tests {
    use super::*;

    #[test]
    fn downsample_offsets_are_symmetric_half_texel() {
        let offsets = kawase_downsample_offsets((2.0, 4.0));
        // half-texel = (1.0, 2.0); four diagonal corners.
        assert_eq!(
            offsets,
            [(-1.0, -2.0), (1.0, -2.0), (-1.0, 2.0), (1.0, 2.0)]
        );
    }

    #[test]
    fn upsample_offsets_are_symmetric_full_and_double_texel() {
        let offsets = kawase_upsample_offsets((2.0, 4.0));
        assert_eq!(offsets.len(), 8);
        // The four axis-aligned double-distance taps come first by
        // construction, then the four single-distance diagonal taps.
        assert_eq!(offsets[0], (-4.0, 0.0));
        assert_eq!(offsets[4], (-2.0, 4.0));
    }
}
