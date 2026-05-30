//! GPU vertex-buffer uploads (reused buffers, reallocated on overflow).
//!
//! Extracted from `renderer/mod.rs`:
//! - `upload_bg_verts` — upload of the background vertex buffer.
//! - `upload_txt_verts` — upload of the text vertex buffer.
//!
//! Each buffer reserves its initial capacity in `new()`; if the vertex count
//! exceeds it, the buffer is reallocated at double the size. Uploads use
//! `queue.write_buffer` directly without going through `staging_belt` or
//! other intermediate buffers.

use crate::glyph_atlas::{BgVertex, TextVertex};

use super::WgpuState;

impl WgpuState {
    /// Upload background vertex / index data into the reused buffers.
    ///
    /// When the buffer capacity is insufficient, double it and reallocate.
    pub(super) fn upload_bg_verts(&mut self, verts: &[BgVertex], idx: &[u16]) {
        let v_bytes = bytemuck::cast_slice(verts);
        let i_bytes = bytemuck::cast_slice(idx);

        // Reallocate if capacity is insufficient
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

    /// Upload text vertex / index data into the reused buffers.
    pub(super) fn upload_txt_verts(&mut self, verts: &[TextVertex], idx: &[u16]) {
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
}
