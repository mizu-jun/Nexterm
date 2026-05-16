//! GPU 頂点バッファのアップロード（再利用バッファ + 容量超過時の再確保）
//!
//! `renderer/mod.rs` から抽出した:
//! - `upload_bg_verts` — 背景頂点バッファのアップロード
//! - `upload_txt_verts` — テキスト頂点バッファのアップロード
//!
//! 各バッファは初期容量を `new()` 時に確保し、頂点数がそれを超える場合は
//! 2 倍サイズで再確保する。アップロードは `queue.write_buffer` で行い、
//! `staging_belt` 等の中間バッファは経由しない。

use crate::glyph_atlas::{BgVertex, TextVertex};

use super::WgpuState;

impl WgpuState {
    /// 背景頂点・インデックスデータを再利用バッファへアップロードする
    ///
    /// バッファ容量が不足する場合は 2 倍に拡張して再確保する。
    pub(super) fn upload_bg_verts(&mut self, verts: &[BgVertex], idx: &[u16]) {
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
