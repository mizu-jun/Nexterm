//! Hot-reload of custom shaders.
//!
//! Extracted from `renderer/mod.rs`:
//! - `reload_shader_pipelines` — reload custom shaders and rebuild the bg / text pipelines.

use tracing::{info, warn};

use crate::glyph_atlas::{BgVertex, TextVertex};
use crate::shaders::{BG_SHADER, TEXT_SHADER};

use super::WgpuState;

impl WgpuState {
    /// Reload custom shaders and rebuild the bg / text pipelines.
    ///
    /// If the shader file contains syntax errors, the existing pipelines are kept,
    /// a warning is logged, and the previous pipelines remain in use.
    pub(super) fn reload_shader_pipelines(&mut self, gpu_cfg: &nexterm_config::GpuConfig) {
        let format = self.surface_config.format;

        // Load background shader
        let bg_src: std::borrow::Cow<'static, str> =
            if let Some(ref path) = gpu_cfg.custom_bg_shader {
                let expanded = shellexpand::tilde(path).into_owned();
                match std::fs::read_to_string(&expanded) {
                    Ok(s) => {
                        info!(
                            "Shader hot reload: reloaded background shader: {}",
                            expanded
                        );
                        std::borrow::Cow::Owned(s)
                    }
                    Err(e) => {
                        warn!(
                            "Failed to reload background shader (keeping the existing one): {}",
                            e
                        );
                        return;
                    }
                }
            } else {
                std::borrow::Cow::Borrowed(BG_SHADER)
            };

        // Load text shader
        let text_src: std::borrow::Cow<'static, str> =
            if let Some(ref path) = gpu_cfg.custom_text_shader {
                let expanded = shellexpand::tilde(path).into_owned();
                match std::fs::read_to_string(&expanded) {
                    Ok(s) => {
                        info!("Shader hot reload: reloaded text shader: {}", expanded);
                        std::borrow::Cow::Owned(s)
                    }
                    Err(e) => {
                        warn!(
                            "Failed to reload text shader (keeping the existing one): {}",
                            e
                        );
                        return;
                    }
                }
            } else {
                std::borrow::Cow::Borrowed(TEXT_SHADER)
            };

        // Rebuild the background pipeline
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
                        // Enable alpha blending (the image overlay pipeline does the same)
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

        // Rebuild the text pipeline
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

        info!("Shader hot reload complete");
    }
}
