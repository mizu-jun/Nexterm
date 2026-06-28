//! wgpu + winit renderer.
//!
//! Rendering pipeline:
//!   1. Draw terminal cell backgrounds from a vertex buffer (color pass).
//!   2. Rasterize glyphs with cosmic-text and write them into the glyph atlas.
//!   3. Sample the glyph atlas to draw text (text pass).
//!
//! Vertex-builder submodules:
//! - `grid_verts` — grid / scrollback / borders.
//! - `overlay` — tab bar / status / search bar / various overlays.
//! - `ui_verts` — context menu / consent dialog / update banner.
//!
//! Runtime submodules:
//! - `app` — `NextermApp`.
//! - `event_handler` — winit `ApplicationHandler`.
//! - `input_handler` — key input dispatch.
//!
//! wgpu internal submodules (split out in Sprint 5-6):
//! - `wgpu_init` — `WgpuState::new` / `resize` / `select_present_mode`.
//! - `render_frame` — `WgpuState::render`.
//! - `gpu_buffers` — upload of background / text vertex buffers.
//! - `image` — image textures and vertex construction.
//! - `shader_reload` — hot reload of custom shaders.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use nexterm_proto::PaneLayout;
use tracing::{info, warn};

use crate::state::{ContextMenu, CopyModeState, SearchState};

// ---- Vertex-builder submodules (Sprint 2-1 Phase A) ----
// Sprint 5-4 / A2: overlay_verts.rs (1,958 lines) was further split into the overlay/ subdirectory.
mod grid_verts;
mod overlay;
mod ui_verts;

// ---- Runtime submodules (Sprint 2-1 Phase B/C) ----
mod app;
mod event_handler;
mod input_handler;

// ---- wgpu internal submodules (file-level split in Sprint 5-6) ----
mod background_pass;
mod gpu_buffers;
mod image;
mod render_frame;
mod shader_reload;
mod wgpu_init;

pub use app::NextermApp;
pub use event_handler::{EventHandler, UserEvent};

use background_pass::BackgroundTexture;
use image::ImageEntry;

use crate::glyph_atlas::{BgVertex, TextVertex};
use crate::state::MouseSelection;

/// Cached CPU-side vertex data for a single pane's grid render (C4 partial-redraw).
///
/// Built when a pane's content is dirty; reused every subsequent frame until the
/// content changes, a layout parameter changes, or the glyph atlas is reset.
/// Indices are 0-relative (i.e. pane-local). `render_frame` shifts them by the
/// current combined buffer offset when appending.
pub(super) struct PaneRenderCache {
    // Layout / display parameters active when the cache was built
    pub(super) col_offset: u16,
    pub(super) row_offset: u16,
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) sw_bits: u32,
    pub(super) sh_bits: u32,
    pub(super) cell_w_bits: u32,
    pub(super) cell_h_bits: u32,
    pub(super) grid_offset_y_bits: u32,
    pub(super) was_focused: bool,
    pub(super) cursor_style: nexterm_config::CursorStyle,
    /// Phase 5 (UI/UX v2): cursor visibility (blink phase) at cache-fill
    /// time. Included so a blink-tick invalidates the cache and the cursor
    /// actually disappears/appears between frames.
    pub(super) cursor_visible: bool,
    /// Phase 5b (UI/UX v2): quantized visible cursor position so
    /// mid-animation frames invalidate the cache. Format is
    /// `(quantize_visible(col), quantize_visible(row))` — see
    /// `cursor_motion::quantize_visible`.
    pub(super) cursor_visual_q: (u32, u32),
    pub(super) mouse_sel_start: (u16, u16),
    pub(super) mouse_sel_end: (u16, u16),
    pub(super) mouse_sel_dragging: bool,
    // Cached vertex data (pane-local, 0-relative indices)
    pub(super) bg_verts: Vec<BgVertex>,
    pub(super) bg_idx: Vec<u16>,
    pub(super) text_verts: Vec<TextVertex>,
    pub(super) text_idx: Vec<u16>,
}

impl PaneRenderCache {
    /// Returns true when all layout/display parameters match and the cache can be reused.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn key_matches(
        &self,
        layout: &nexterm_proto::PaneLayout,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        grid_offset_y: f32,
        is_focused: bool,
        cursor_style: &nexterm_config::CursorStyle,
        cursor_visible: bool,
        cursor_visual_q: (u32, u32),
        mouse_sel: &MouseSelection,
    ) -> bool {
        self.col_offset == layout.col_offset
            && self.row_offset == layout.row_offset
            && self.cols == layout.cols
            && self.rows == layout.rows
            && self.sw_bits == sw.to_bits()
            && self.sh_bits == sh.to_bits()
            && self.cell_w_bits == cell_w.to_bits()
            && self.cell_h_bits == cell_h.to_bits()
            && self.grid_offset_y_bits == grid_offset_y.to_bits()
            && self.was_focused == is_focused
            && &self.cursor_style == cursor_style
            && self.cursor_visible == cursor_visible
            && self.cursor_visual_q == cursor_visual_q
            && self.mouse_sel_start == mouse_sel.start
            && self.mouse_sel_end == mouse_sel.end
            && self.mouse_sel_dragging == mouse_sel.is_dragging
    }
}

// ---- Shader file watcher ----

/// Start a watcher for custom shader files.
///
/// Only starts watching when a shader path is configured. When the file
/// changes, sends `()` on the receiver channel.
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
                info!("Detected shader file change. Rebuilding pipelines.");
                let _ = tx.blocking_send(());
            }
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            warn!("Failed to start shader watcher: {}", e);
            return (None, None);
        }
    };

    for path in &paths {
        if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
            warn!("Failed to watch shader file: {}: {}", path.display(), e);
        } else {
            info!("Watching shader file: {}", path.display());
        }
    }

    (Some(rx), Some(watcher))
}

// ---- wgpu core state ----

/// Initialized wgpu state.
///
/// All fields are accessed directly from the renderer submodules
/// (`wgpu_init` / `render_frame` / `gpu_buffers` / `image` / `shader_reload`).
///
/// Visibility `pub(super)` is matched to `ClientWindow.wgpu`'s visibility
/// introduced in Sprint 5-8 Phase 4-1 Step 1.2, so parent modules such as
/// `EventHandler` can also reference it.
pub(super) struct WgpuState {
    device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    bg_pipeline: wgpu::RenderPipeline,
    text_pipeline: wgpu::RenderPipeline,
    text_bind_group_layout: wgpu::BindGroupLayout,
    /// Image rendering pipeline.
    image_pipeline: wgpu::RenderPipeline,
    /// Sampler used for images.
    image_sampler: wgpu::Sampler,
    /// Image texture cache (`image_id` -> `ImageEntry`).
    image_textures: HashMap<u32, ImageEntry>,
    /// OSC 66 text-sizing texture cache (`(text, scale_num, scale_den)` -> `(ImageEntry, w, h)`).
    text_size_textures: HashMap<(String, u8, u8), (ImageEntry, u32, u32)>,
    /// Background image (Sprint 5-7 / Phase 3-1). Loaded only when `WindowConfig.background_image` is set.
    background: Option<BackgroundTexture>,
    // ---- Frame-to-frame reused buffers (avoids per-frame GPU allocations) ----
    /// Background vertex buffer (VERTEX | COPY_DST; reallocated on overflow).
    buf_bg_v: wgpu::Buffer,
    /// Background index buffer.
    buf_bg_i: wgpu::Buffer,
    /// Text vertex buffer.
    buf_txt_v: wgpu::Buffer,
    /// Text index buffer.
    buf_txt_i: wgpu::Buffer,
    /// Current capacity of the background vertex buffer (in `BgVertex` units).
    bg_v_cap: u64,
    /// Current capacity of the background index buffer (in `u16` units).
    bg_i_cap: u64,
    /// Current capacity of the text vertex buffer (in `TextVertex` units).
    txt_v_cap: u64,
    /// Current capacity of the text index buffer (in `u16` units).
    txt_i_cap: u64,
    /// Timestamp of the last frame draw (used for FPS limiting).
    last_frame_at: Instant,
    /// Phase 5 (UI/UX v2): wall-clock reference for the cursor blink phase.
    /// Seeded at `WgpuState::new()`; `draw_cursor` queries
    /// `cursor_blink_start.elapsed().as_millis()` against `CursorConfig`'s
    /// blink interval to decide whether the cursor is currently shown.
    pub(super) cursor_blink_start: Instant,
    /// Per-pane CPU vertex cache (C4 partial-redraw optimization).
    ///
    /// Keyed by `pane_id`. Entries are invalidated when the pane's content changes
    /// (`content_dirty = true`), when layout parameters change, or when the glyph
    /// atlas is cleared mid-frame (`atlas.cleared_this_frame = true`).
    pane_cache: HashMap<u32, PaneRenderCache>,
    /// Phase 5b (UI/UX v2): per-pane cursor motion state. Drives smooth
    /// interpolation between cells when `CursorConfig.smooth_motion = true`.
    /// Keyed by `pane_id`; entries are lazily created on first sight of a
    /// pane and updated each frame from the server-reported cursor cell.
    pub(super) cursor_motion: HashMap<u32, crate::cursor_motion::CursorMotionState>,
}

// ---- Multi OS-window skeleton (Sprint 5-8 Phase 4-1 Step 1.2) ----

/// Aggregates per-OS-window display state (Sprint 5-8 Phase 4-1 Step 1.3).
///
/// Defines, in parallel, the fields currently held inside `ClientState` that
/// are **candidates for per-OS-window scope**. Actual wiring (threading
/// through the event-handler arguments and migrating fields out of
/// `ClientState`) is done incrementally from Step 1.4 onward, so this struct
/// is currently never referenced even when instantiated (`dead_code` allow
/// is retained).
///
/// The parallel-definition approach follows the plan
/// ([[project_sprint5_7_phase4_plan]] Sprint 5-8 section) so that
/// `ClientState` responsibility splits can land without any
/// non-compilable interim period.
///
/// Fields (migrated from `ClientState` starting in Step 1.4):
/// - `focused_server_window_id`: server-window ID this OS window has focused.
/// - `pane_layouts`: pane layout info shown here (duplicated for per-window rendering).
/// - `copy_mode`: copy-mode (Vim-style text selection) state.
/// - `search`: incremental search state.
/// - `context_menu`: the context menu opened by right-click.
/// - `hovered_tab_id`: ID of the tab currently hovered in the tab bar.
#[allow(dead_code)]
pub(super) struct PerWindowViewState {
    pub(super) focused_server_window_id: u32,
    pub(super) pane_layouts: HashMap<u32, PaneLayout>,
    pub(super) copy_mode: CopyModeState,
    pub(super) search: SearchState,
    pub(super) context_menu: Option<ContextMenu>,
    pub(super) hovered_tab_id: Option<u32>,
}

impl Default for PerWindowViewState {
    fn default() -> Self {
        Self {
            focused_server_window_id: 0,
            pane_layouts: HashMap::new(),
            copy_mode: CopyModeState::new(),
            search: SearchState::new(),
            context_menu: None,
            hovered_tab_id: None,
        }
    }
}

/// Pair type bound to one OS window (Sprint 5-8 Phase 4-1 Step 1.2 skeleton).
///
/// Currently only a single window exists, but from Phase 4-2 onward
/// `EventHandler.windows: HashMap<WindowId, ClientWindow>` will hold
/// multiple OS windows.
///
/// During the transition (Step 1.2..1.3) this is held in parallel with the
/// existing `EventHandler.window` / `EventHandler.wgpu_state` fields and
/// will be gradually consolidated from Step 1.3 onward.
///
/// Sprint 5-11-2 Step 2-3: each OS window owns an independent AccessKit
/// adapter. Platform a11y adapters are managed per window, so additional
/// windows need a node tree independent from the main window's
/// (`EventHandler::accesskit_adapter` is still kept for the main window in
/// Step 2-3, and this field is added for additional windows).
#[allow(dead_code)]
pub(super) struct ClientWindow {
    /// winit native window.
    pub(super) window: Arc<winit::window::Window>,
    /// wgpu render state.
    pub(super) wgpu: WgpuState,
    /// Per-OS-window display state (detailed fields to be added in Step 1.3).
    pub(super) view_state: PerWindowViewState,
    /// AccessKit platform adapter (Sprint 5-11-2 Step 2-3).
    ///
    /// Each OS window holds its own independent adapter. Screen readers
    /// can manage a separate tree per window, so additional windows also
    /// receive `InitialTreeRequested` and return
    /// `build_tree_from_state(&self.app.state)`.
    pub(super) accesskit_adapter: accesskit_winit::Adapter,
}

#[cfg(test)]
mod client_window_tests {
    use super::*;

    #[test]
    fn per_window_view_state_default() {
        // Step 1.3 expanded `PerWindowViewState` from a unit struct into the
        // current full struct. Verify that the `Default` impl produces
        // initial values for the per-OS-window candidate fields that match
        // the existing behavior in `ClientState`.
        let view = PerWindowViewState::default();
        assert_eq!(view.focused_server_window_id, 0);
        assert!(view.pane_layouts.is_empty());
        assert!(view.context_menu.is_none());
        assert!(view.hovered_tab_id.is_none());
        // Invariants about the initial states of `copy_mode` / `search`
        // themselves are covered by tests in those modules.
    }
}
