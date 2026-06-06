//! Body of a single-frame render.
//!
//! Extracted from `renderer/mod.rs`:
//! - `impl WgpuState { fn render }` — FPS limiting, surface acquisition,
//!   grid/overlay vertex construction → upload → main pass (bg + text)
//!   → image pass, in that order.

use std::time::{Duration, Instant};

use anyhow::Result;
use wgpu::util::DeviceExt;

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::WgpuState;
use super::background_pass::build_background_verts;
use super::image::build_image_verts;

impl WgpuState {
    /// Render a single frame.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
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
        // Sprint 5-7 / UI-1-4: reference to the full config for the key-hint overlay
        config: &nexterm_config::Config,
    ) -> Result<()> {
        // FPS limiting: skip if the time elapsed since the previous frame is less than 1/fps
        if fps_limit > 0 {
            let frame_duration = Duration::from_secs_f64(1.0 / fps_limit as f64);
            if self.last_frame_at.elapsed() < frame_duration {
                return Ok(());
            }
        }
        self.last_frame_at = Instant::now();

        // Phase 4 (UI/UX modernization): advance spring-physics animations once per frame.
        let frame_now = Instant::now();
        let anim_enabled = config.animations.scaled_duration_ms(1) > 0;
        state.animations.tick(frame_now, anim_enabled);

        // Clear the atlas reset flag at the start of the frame.
        // Even if the atlas was reset on the previous frame, this frame will redraw
        // using the correct UVs.
        atlas.cleared_this_frame = false;

        // Derive the palette from the color scheme (every frame; cost is small)
        let scheme_palette: Option<nexterm_config::SchemePalette> = match color_scheme {
            nexterm_config::ColorScheme::Builtin(s) => Some(s.palette()),
            nexterm_config::ColorScheme::Custom(p) => {
                // Convert the Custom palette into a SchemePalette
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
        // Compute design tokens from the active palette (cheap; runs every frame).
        let tokens = if let Some(p) = scheme_palette.as_ref() {
            nexterm_config::DesignTokens::from_palette(p)
        } else {
            nexterm_config::DesignTokens::default()
        };
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

        // Tab bar height (when enabled): used as the y-offset for the terminal content
        let tab_bar_h = if tab_bar_cfg.enabled {
            tab_bar_cfg.height as f32
        } else {
            0.0
        };
        // Effective offset accounting for padding (the origin for grid rendering)
        let _grid_offset_x = padding_x;
        let grid_offset_y = tab_bar_h + padding_y;

        let mut bg_verts: Vec<BgVertex> = Vec::new();
        let mut bg_idx: Vec<u16> = Vec::new();
        let mut text_verts: Vec<TextVertex> = Vec::new();
        let mut text_idx: Vec<u16> = Vec::new();

        // When layout information is available, draw all panes in their split positions
        if !state.pane_layouts.is_empty() {
            // Render each pane inside its layout rectangle
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
            // Draw the pane border lines
            self.build_border_verts(
                state,
                sw,
                sh,
                cell_w,
                cell_h,
                tab_bar_h,
                &tokens,
                &mut bg_verts,
                &mut bg_idx,
            );
        } else if let Some(pane) = state.focused_pane() {
            // Fallback: no layout information yet (e.g. immediately after connect)
            if pane.scroll_offset > 0 {
                // ---- Scrollback display mode ----
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
                // ---- Normal grid display ----
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

        // ---- Pane number overlay (when display_panes_mode is enabled) ----
        if state.display_panes_mode {
            let mut sorted_pane_ids: Vec<u32> = state.pane_layouts.keys().copied().collect();
            sorted_pane_ids.sort();
            for (number, pane_id) in sorted_pane_ids.iter().enumerate() {
                if let Some(layout) = state.pane_layouts.get(pane_id) {
                    let px = layout.col_offset as f32 * cell_w;
                    let py = layout.row_offset as f32 * cell_h + tab_bar_h;
                    let badge_w = cell_w * 2.0;
                    let badge_h = cell_h;
                    // Yellow background badge
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
                    // Pane number text (1-based)
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
            // When no layout information is available (fallback: focused pane only)
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

        // ---- Pane fade-in overlay (Sprint 5-7 / Phase 3-2) ----
        // Overlay a translucent white over newly added panes, fading out with ease-out.
        // Since this is alpha-blended on top of existing cell backgrounds, we append
        // additional vertices.
        {
            let fade_duration = config.animations.scaled_duration_ms(250);
            if fade_duration > 0 && !state.pane_layouts.is_empty() {
                let now = std::time::Instant::now();
                let layout_ids: Vec<u32> = state.pane_layouts.keys().copied().collect();
                for pane_id in layout_ids {
                    let raw = state
                        .animations
                        .pane_fade_in_progress(pane_id, now, fade_duration);
                    if raw >= 1.0 {
                        continue;
                    }
                    let eased = crate::animations::ease_out_cubic(raw);
                    // Fade from initial alpha=0.35 to 0.0
                    let overlay_alpha = 0.35 * (1.0 - eased);
                    if let Some(layout) = state.pane_layouts.get(&pane_id) {
                        let px = layout.col_offset as f32 * cell_w;
                        let py = layout.row_offset as f32 * cell_h + grid_offset_y;
                        let pw = layout.cols as f32 * cell_w;
                        let ph = layout.rows as f32 * cell_h;
                        add_px_rect(
                            px,
                            py,
                            pw,
                            ph,
                            [1.0, 1.0, 1.0, overlay_alpha],
                            sw,
                            sh,
                            &mut bg_verts,
                            &mut bg_idx,
                        );
                    }
                }
            }
        }

        // ---- Tab bar (when enabled in the config) ----
        if tab_bar_cfg.enabled {
            self.build_tab_bar_verts(
                state,
                tab_bar_cfg,
                &config.animations,
                &tokens,
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

        // ---- Status line (always shown) ----
        self.build_status_verts(
            state,
            sw,
            sh,
            cell_w,
            cell_h,
            &tokens,
            font,
            atlas,
            &mut bg_verts,
            &mut bg_idx,
            &mut text_verts,
            &mut text_idx,
        );

        // ---- Search bar (when active) ----
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

        // ---- Quick Select overlay (when active) ----
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

        // ---- SFTP file transfer dialog (when open) ----
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

        // ---- Lua macro picker (when open) ----
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

        // ---- Host manager (when open) ----
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

        // ---- Command palette (when open) ----
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

        // ---- Settings panel (opened with Ctrl+,) ----
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

        // ---- Context menu (on right-click) ----
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

        // ---- Update notification banner (top of the screen) ----
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

        // ---- Offline-mode banner (Sprint 5-14 / v1.7.8 — P2-1) ----
        // Visible while the client cannot reach the embedded server. Stacks
        // vertically with `update_banner`. Auto-clears on the next successful
        // `try_connect`, so unlike the other banners it has no key dismissal.
        if state.offline_banner_since.is_some() {
            self.build_offline_banner_verts(
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

        // ---- Server error banner (Sprint 5-12 Phase 1) ----
        // Surfaces PTY launch failures (e.g. PowerShell not found) and config load errors
        // at the top of the screen. Stacks vertically with `update_banner`. Closed with Esc.
        if state.error_banner.is_some() {
            self.build_error_banner_verts(
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

        // ---- Consent dialog (Sprint 4-1: sensitive-operation confirmation modal) ----
        // Appended last so it is rendered on top
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

        // ---- Window-close confirmation dialog (Sprint 5-9 Phase 4-6) ----
        // Shown when `close_action = "prompt"` detects a foreground process.
        // Like the sensitive-operation consent dialog, it is layered on top.
        if state.close_window_dialog.is_some() {
            self.build_close_window_dialog_verts(
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

        // ---- Key-hint overlay (Sprint 5-7 / UI-1-4) ----
        // After the Leader key is pressed alone, show the list of prefix bindings at the
        // bottom of the screen for 2 seconds.
        self.build_key_hint_verts(
            state,
            config,
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

        // ---- IME preedit overlay (text being composed) ----
        if let Some(ref preedit) = state.ime_preedit
            && let Some(pane) = state.focused_pane()
        {
            let px = pane.cursor_col as f32 * cell_w;
            let py = (pane.cursor_row + 1) as f32 * cell_h;
            // Preedit background (slightly brighter gray)
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
            // Underline (yellow)
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
            // Preedit text
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

        // ---- Upload to GPU buffers (write_buffer overwrites the reused buffers) ----
        // Reallocate only when capacity is insufficient (doubling the size)
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

        // ---- Clear pass (fill with the palette background color) ----
        // When there is a background image, complete the clear before drawing,
        // so this is an independent pass.
        {
            let clear_bg = scheme_palette.as_ref().map(|p| p.bg).unwrap_or([0, 0, 0]);
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_bg[0] as f64 / 255.0,
                            g: clear_bg[1] as f64 / 255.0,
                            b: clear_bg[2] as f64 / 255.0,
                            // Reflect the `background_opacity` setting in alpha (transparent terminal support)
                            a: background_opacity as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        // ---- Background image pass (Sprint 5-7 / Phase 3-1) ----
        // Drawn after clear and before bg_verts, only when a background image is configured.
        // Reuses `image_pipeline` (no dedicated pipeline).
        if let Some(ref bg_img) = self.background {
            let (bg_verts_img, bg_idx_img) = build_background_verts(
                sw,
                sh,
                bg_img.width,
                bg_img.height,
                &bg_img.fit,
                bg_img.opacity,
            );
            if !bg_idx_img.is_empty() {
                let vbuf = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("bg_image_vbuf"),
                        contents: bytemuck::cast_slice(&bg_verts_img),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                let ibuf = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("bg_image_ibuf"),
                        contents: bytemuck::cast_slice(&bg_idx_img),
                        usage: wgpu::BufferUsages::INDEX,
                    });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("background_image_pass"),
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
                pass.set_bind_group(0, &bg_img.bind_group, &[]);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..bg_idx_img.len() as u32, 0, 0..1);
            }
        }

        // ---- Main render pass (cell backgrounds + text) ----
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Preserve the clear + background image results, then draw cell backgrounds and text on top
                        load: wgpu::LoadOp::Load,
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

        // ---- Image render pass (overlay placed images) ----
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
}
