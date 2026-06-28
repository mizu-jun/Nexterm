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

use super::PaneRenderCache;
use super::WgpuState;
use super::background_pass::build_background_verts;
use super::image::{ImageEntry, build_image_verts};

impl WgpuState {
    /// Phase 5b (UI/UX v2): advance the per-pane cursor motion state
    /// and return the visible cursor position + the cache-key
    /// quantization for the current frame.
    ///
    /// `smooth_motion = false` short-circuits to the integer cell
    /// position so the cache key is deterministic and the rendered
    /// output is byte-identical to the pre-Phase-5b build.
    pub(super) fn sample_cursor_motion(
        &mut self,
        pane_id: u32,
        cursor_col: u16,
        cursor_row: u16,
        smooth_motion: bool,
        now: Instant,
    ) -> (f32, f32, (u32, u32)) {
        use crate::cursor_motion::{
            CURSOR_MOTION_DURATION_MS, CursorMotionState, quantize_visible, update_target,
            visible_position,
        };

        if !smooth_motion {
            // Reset motion state so re-enabling smooth_motion later
            // does not glide from a stale position.
            self.cursor_motion.remove(&pane_id);
            let col = cursor_col as f32;
            let row = cursor_row as f32;
            return (col, row, (quantize_visible(col), quantize_visible(row)));
        }

        let entry = self
            .cursor_motion
            .entry(pane_id)
            .or_insert_with(|| CursorMotionState::new(cursor_col, cursor_row, now));
        *entry = update_target(
            *entry,
            cursor_col,
            cursor_row,
            now,
            CURSOR_MOTION_DURATION_MS,
        );
        let (vcol, vrow) = visible_position(*entry, now, CURSOR_MOTION_DURATION_MS);
        (vcol, vrow, (quantize_visible(vcol), quantize_visible(vrow)))
    }
}

/// Append cached (0-relative index) pane vertex data into the frame's main buffers.
///
/// Indices in `src_*_idx` are 0-relative (built against empty local vecs).
/// This function shifts them by the current lengths of `bg_verts` / `text_verts`
/// before extending, producing correct absolute indices for the frame buffer.
#[allow(clippy::too_many_arguments)]
fn append_pane_verts(
    src_bg_verts: &[BgVertex],
    src_bg_idx: &[u16],
    src_text_verts: &[TextVertex],
    src_text_idx: &[u16],
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    let bg_base = bg_verts.len() as u16;
    bg_verts.extend_from_slice(src_bg_verts);
    bg_idx.extend(src_bg_idx.iter().map(|i| i + bg_base));

    let txt_base = text_verts.len() as u16;
    text_verts.extend_from_slice(src_text_verts);
    text_idx.extend(src_text_idx.iter().map(|i| i + txt_base));
}

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

        // Phase 5 (UI/UX v2): cursor visibility for this frame. Computed once
        // here against the wall-clock so every `build_grid_verts*` invocation
        // below sees the same value (otherwise multi-pane layouts would show
        // mixed blink phases). When `blink_enabled = false`, this is always
        // `true` and `draw_cursor_with_visibility` becomes a no-op gate.
        let cursor_visible = config
            .cursor
            .is_visible_at(self.cursor_blink_start.elapsed().as_millis() as u64);

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

        // Sprint 5-15 / UI/UX Modernization v2 Phase 2b: with
        // `tab_bar.hide_when_single = true`, hide the bar (and reclaim its
        // height for the grid) when the active window has at most one pane.
        let tab_bar_visible =
            tab_bar_cfg.enabled && !(tab_bar_cfg.hide_when_single && state.pane_layouts.len() <= 1);
        let tab_bar_h = if tab_bar_visible {
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

        // ---- Background gradient (Phase 5 / UI-UX v2) ----
        // Mutually exclusive with the background-image pass: when an image is
        // configured the renderer skips this drawcall (the image fully covers
        // the screen anyway). Pushed onto `bg_verts` first so per-cell
        // backgrounds drawn later naturally layer over the gradient.
        if self.background.is_none()
            && let Some(ref grad) = config.window.gradient
            && grad.is_enabled()
            && let Some(from) = nexterm_config::parse_hex_color(&grad.from)
            && let Some(to) = nexterm_config::parse_hex_color(&grad.to)
        {
            crate::vertex_util::add_px_gradient_rect(
                0.0,
                0.0,
                sw,
                sh,
                from,
                to,
                grad.angle,
                sw,
                sh,
                &mut bg_verts,
                &mut bg_idx,
            );
        }

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
                        // Scrollback mode: no caching (historical rows change on scroll)
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
                        // Command-block overlay: drawn after the scrollback
                        // text so the border + badge sit on top.
                        self.build_block_overlay_verts_in_rect(
                            pane,
                            state.selected_block,
                            &config.blocks,
                            layout,
                            sw,
                            sh,
                            cell_w,
                            cell_h,
                            grid_offset_y,
                            font,
                            atlas,
                            &mut bg_verts,
                            &mut bg_idx,
                            &mut text_verts,
                            &mut text_idx,
                        );
                    } else {
                        // Phase 5b (UI/UX v2): update / sample the per-pane
                        // cursor motion state and derive the visible cursor
                        // position for this frame. When smooth motion is
                        // disabled, the visible position equals the
                        // server-reported cell exactly so the cache key is
                        // deterministic.
                        let (cursor_visual_col, cursor_visual_row, cursor_visual_q) = self
                            .sample_cursor_motion(
                                pane_id,
                                pane.cursor_col,
                                pane.cursor_row,
                                config.cursor.smooth_motion,
                                frame_now,
                            );

                        // C4: check whether the cached vertex data can be reused.
                        // A cache hit requires all of: content unchanged, layout
                        // parameters unchanged, and the atlas not yet reset this
                        // frame (a reset invalidates stored UV coordinates).
                        let cache_valid = !atlas.cleared_this_frame
                            && self.pane_cache.get(&pane_id).is_some_and(|c| {
                                !pane.content_dirty
                                    && c.key_matches(
                                        layout,
                                        sw,
                                        sh,
                                        cell_w,
                                        cell_h,
                                        grid_offset_y,
                                        is_focused,
                                        cursor_style,
                                        cursor_visible,
                                        cursor_visual_q,
                                        &state.mouse_sel,
                                    )
                            });

                        if cache_valid {
                            // Cache hit: append stored vertex data directly.
                            let c = self.pane_cache.get(&pane_id).unwrap();
                            append_pane_verts(
                                &c.bg_verts,
                                &c.bg_idx,
                                &c.text_verts,
                                &c.text_idx,
                                &mut bg_verts,
                                &mut bg_idx,
                                &mut text_verts,
                                &mut text_idx,
                            );
                        } else {
                            // Cache miss: rebuild into local vecs, then store and append.
                            let mut l_bg_v: Vec<BgVertex> = Vec::new();
                            let mut l_bg_i: Vec<u16> = Vec::new();
                            let mut l_txt_v: Vec<TextVertex> = Vec::new();
                            let mut l_txt_i: Vec<u16> = Vec::new();

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
                                cursor_visible,
                                cursor_visual_col,
                                cursor_visual_row,
                                &mut l_bg_v,
                                &mut l_bg_i,
                                &mut l_txt_v,
                                &mut l_txt_i,
                            );

                            // Append to frame buffers before moving into cache.
                            append_pane_verts(
                                &l_bg_v,
                                &l_bg_i,
                                &l_txt_v,
                                &l_txt_i,
                                &mut bg_verts,
                                &mut bg_idx,
                                &mut text_verts,
                                &mut text_idx,
                            );

                            // Store the bare grid in cache (without overlay),
                            // so selection changes don't invalidate it.
                            self.pane_cache.insert(
                                pane_id,
                                PaneRenderCache {
                                    col_offset: layout.col_offset,
                                    row_offset: layout.row_offset,
                                    cols: layout.cols,
                                    rows: layout.rows,
                                    sw_bits: sw.to_bits(),
                                    sh_bits: sh.to_bits(),
                                    cell_w_bits: cell_w.to_bits(),
                                    cell_h_bits: cell_h.to_bits(),
                                    grid_offset_y_bits: grid_offset_y.to_bits(),
                                    was_focused: is_focused,
                                    cursor_style: cursor_style.clone(),
                                    cursor_visible,
                                    cursor_visual_q,
                                    mouse_sel_start: state.mouse_sel.start,
                                    mouse_sel_end: state.mouse_sel.end,
                                    mouse_sel_dragging: state.mouse_sel.is_dragging,
                                    bg_verts: l_bg_v,
                                    bg_idx: l_bg_i,
                                    text_verts: l_txt_v,
                                    text_idx: l_txt_i,
                                },
                            );
                        }

                        // Command-block overlay for in-grid mode. Drawn
                        // outside the cache so selection changes refresh
                        // instantly without invalidating the cached grid.
                        self.build_block_overlay_verts_in_rect(
                            pane,
                            state.selected_block,
                            &config.blocks,
                            layout,
                            sw,
                            sh,
                            cell_w,
                            cell_h,
                            grid_offset_y,
                            font,
                            atlas,
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
                // Command-block left border + selection tint + status badge
                // over the scrollback view. No-op when disabled or when the
                // pane has no blocks recorded.
                self.build_block_overlay_verts(
                    pane,
                    state.selected_block,
                    &config.blocks,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    grid_offset_y,
                    font,
                    atlas,
                    &mut bg_verts,
                    &mut bg_idx,
                    &mut text_verts,
                    &mut text_idx,
                );
            } else {
                // ---- Normal grid display ----
                // Phase 5b: derive visible cursor position from the
                // motion state for the focused pane (fallback path
                // when no layout info yet).
                let fallback_pane_id = state.focused_pane_id.unwrap_or(0);
                let (cursor_visual_col, cursor_visual_row, _q) = self.sample_cursor_motion(
                    fallback_pane_id,
                    pane.cursor_col,
                    pane.cursor_row,
                    config.cursor.smooth_motion,
                    frame_now,
                );
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
                    cursor_visible,
                    cursor_visual_col,
                    cursor_visual_row,
                    &mut bg_verts,
                    &mut bg_idx,
                    &mut text_verts,
                    &mut text_idx,
                );
                // Same overlay over the live grid: the viewport top maps to
                // `scrollback.len()` so blocks whose end_row is None extend to
                // the bottom of the screen (running indicator stays visible).
                self.build_block_overlay_verts(
                    pane,
                    state.selected_block,
                    &config.blocks,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    grid_offset_y,
                    font,
                    atlas,
                    &mut bg_verts,
                    &mut bg_idx,
                    &mut text_verts,
                    &mut text_idx,
                );
            }
        }

        // C4: clear content_dirty on all panes now that vertex data has been built
        // for this frame.  New diffs arriving before the next frame will re-set the
        // flag, triggering a cache miss on the next render.
        for p in state.panes.values_mut() {
            p.content_dirty = false;
        }
        // C4: if the glyph atlas was reset during pane rendering (atlas overflow),
        // all cached UV coordinates are stale.  Evict every cache entry so the next
        // frame performs a full rebuild with correct UVs.
        if atlas.cleared_this_frame {
            self.pane_cache.clear();
        }

        // ---- Copy mode overlay (Vi-mode selection highlight + cursor block) ----
        // Drawn outside the pane vertex cache so it is always up-to-date.
        if state.copy_mode.is_active {
            const CM_SEL_COLOR: [f32; 4] = [0.40, 0.65, 1.0, 0.45];
            const CM_CURSOR_COLOR: [f32; 4] = [1.0, 1.0, 0.0, 0.60];

            // Resolve the focused pane's pixel origin and column count.
            let (pane_px, pane_py, pane_cols) = if let Some(pane_id) = state.focused_pane_id
                && let Some(layout) = state.pane_layouts.get(&pane_id)
            {
                (
                    layout.col_offset as f32 * cell_w,
                    layout.row_offset as f32 * cell_h + grid_offset_y,
                    layout.cols,
                )
            } else {
                (0.0_f32, grid_offset_y, (sw / cell_w) as u16)
            };

            use crate::state::ViMode;
            let vi_mode = state.copy_mode.vi_mode.clone();
            let cursor_col = state.copy_mode.cursor_col;
            let cursor_row = state.copy_mode.cursor_row;
            let sel_range = state.copy_mode.normalized_selection();
            let line_range = state.copy_mode.normalized_visual_line_range();

            match vi_mode {
                ViMode::VisualLine => {
                    if let Some((row_start, row_end)) = line_range {
                        for row in row_start..=row_end {
                            add_px_rect(
                                pane_px,
                                pane_py + row as f32 * cell_h,
                                pane_cols as f32 * cell_w,
                                cell_h,
                                CM_SEL_COLOR,
                                sw,
                                sh,
                                &mut bg_verts,
                                &mut bg_idx,
                            );
                        }
                    }
                }
                ViMode::Visual => {
                    if let Some(((sc, sr), (ec, er))) = sel_range {
                        for row in sr..=er {
                            let col_start = if row == sr { sc } else { 0 };
                            let col_end = if row == er {
                                ec
                            } else {
                                pane_cols.saturating_sub(1)
                            };
                            if col_end >= col_start {
                                add_px_rect(
                                    pane_px + col_start as f32 * cell_w,
                                    pane_py + row as f32 * cell_h,
                                    (col_end - col_start + 1) as f32 * cell_w,
                                    cell_h,
                                    CM_SEL_COLOR,
                                    sw,
                                    sh,
                                    &mut bg_verts,
                                    &mut bg_idx,
                                );
                            }
                        }
                    }
                }
                ViMode::Normal => {}
            }

            // Yellow block cursor drawn on top of any selection highlight.
            add_px_rect(
                pane_px + cursor_col as f32 * cell_w,
                pane_py + cursor_row as f32 * cell_h,
                cell_w,
                cell_h,
                CM_CURSOR_COLOR,
                sw,
                sh,
                &mut bg_verts,
                &mut bg_idx,
            );
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
        if tab_bar_visible {
            self.build_tab_bar_verts(
                state,
                tab_bar_cfg,
                &config.animations,
                &config.ui,
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

        // ---- Quick Select overlay (when active) ----
        if state.quick_select.is_active {
            self.build_quick_select_verts(
                state,
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

        // ---- SFTP file transfer dialog (when open) ----
        if state.file_transfer.is_open {
            self.build_file_transfer_verts(
                state,
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

        // ---- Lua macro picker (when open) ----
        if state.macro_picker.is_open {
            self.build_macro_picker_verts(
                state,
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

        // ---- Host manager (when open) ----
        if state.host_manager.is_open {
            self.build_host_manager_verts(
                state,
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
        if state.host_manager.password_modal.is_some() {
            self.build_password_modal_verts(
                state,
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
        // Phase 2c-4: block-name input modal.
        if state.block_name_modal.is_open {
            self.build_block_name_modal_verts(
                state,
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

        // ---- Command palette (when open) ----
        if state.palette.is_open {
            self.build_palette_verts(
                state,
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

        // ---- Settings panel (opened with Ctrl+,) ----
        if state.settings_panel.is_open {
            self.build_settings_panel_verts(
                state,
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

        // ---- Context menu (on right-click) ----
        if let Some(ref menu) = state.context_menu {
            self.build_context_menu_verts(
                menu,
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

        // ---- Update notification banner (top of the screen) ----
        if state.update_banner.is_some() {
            self.build_update_banner_verts(
                state,
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

        // ---- Offline-mode banner (Sprint 5-14 / v1.7.8 — P2-1) ----
        // Visible while the client cannot reach the embedded server. Stacks
        // vertically with `update_banner`. Auto-clears on the next successful
        // `try_connect`, so unlike the other banners it has no key dismissal.
        if state.offline_banner_since.is_some() {
            self.build_offline_banner_verts(
                state,
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

        // ---- Server error banner (Sprint 5-12 Phase 1) ----
        // Surfaces PTY launch failures (e.g. PowerShell not found) and config load errors
        // at the top of the screen. Stacks vertically with `update_banner`. Closed with Esc.
        if state.error_banner.is_some() {
            self.build_error_banner_verts(
                state,
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

        // ---- Consent dialog (Sprint 4-1: sensitive-operation confirmation modal) ----
        // Appended last so it is rendered on top
        if state.pending_consent.is_some() {
            self.build_consent_dialog_verts(
                state,
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

        // ---- Window-close confirmation dialog (Sprint 5-9 Phase 4-6) ----
        // Shown when `close_action = "prompt"` detects a foreground process.
        // Like the sensitive-operation consent dialog, it is layered on top.
        if state.close_window_dialog.is_some() {
            self.build_close_window_dialog_verts(
                state,
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

        // ---- Key-hint overlay (Sprint 5-7 / UI-1-4) ----
        // After the Leader key is pressed alone, show the list of prefix bindings at the
        // bottom of the screen for 2 seconds.
        self.build_key_hint_verts(
            state,
            config,
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

        // ---- Text-size overlay pass (OSC 66 / Kitty Text Sizing Protocol) ----
        if let Some(focused_id) = state.focused_pane_id
            && let Some(pane) = state.panes.get(&focused_id)
        {
            // Clone the list so we can mutably borrow `self` for GPU texture ops below.
            let text_sizes: Vec<_> = pane
                .text_sizes
                .iter()
                .map(|ts| {
                    (
                        ts.text.clone(),
                        ts.scale_num,
                        ts.scale_den,
                        ts.width_cells,
                        ts.col,
                        ts.row,
                    )
                })
                .collect();

            for (text, scale_num, scale_den, width_cells, col, row) in text_sizes {
                let cache_key = (text.clone(), scale_num, scale_den);
                if !self.text_size_textures.contains_key(&cache_key) {
                    let fg = [255u8, 255, 255, 255];
                    let (tw, th, rgba) =
                        font.rasterize_scaled_text(&text, scale_num, scale_den, fg);
                    let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("text_size_tex"),
                        size: wgpu::Extent3d {
                            width: tw,
                            height: th,
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
                            texture: &tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &rgba,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(tw * 4),
                            rows_per_image: None,
                        },
                        wgpu::Extent3d {
                            width: tw,
                            height: th,
                            depth_or_array_layers: 1,
                        },
                    );
                    let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
                    let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("text_size_bg"),
                        layout: &self.text_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&tex_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                            },
                        ],
                    });
                    self.text_size_textures.insert(
                        cache_key.clone(),
                        (
                            ImageEntry {
                                texture: tex,
                                bind_group,
                            },
                            tw,
                            th,
                        ),
                    );
                }

                if let Some((entry, tw, th)) = self.text_size_textures.get(&cache_key) {
                    let (tw, th) = (*tw, *th);
                    let px = col as f32 * cell_w + padding_x;
                    let py = row as f32 * cell_h + grid_offset_y;
                    let pw = if width_cells > 0 {
                        width_cells as f32 * cell_w
                    } else {
                        tw as f32
                    };
                    let ph = th as f32;
                    let x0 = px / sw * 2.0 - 1.0;
                    let y0 = 1.0 - py / sh * 2.0;
                    let x1 = (px + pw) / sw * 2.0 - 1.0;
                    let y1 = 1.0 - (py + ph) / sh * 2.0;
                    let white = [1.0f32; 4];
                    let ts_verts = [
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
                    let ts_idx = [0u16, 1, 2, 0, 2, 3];
                    let ts_vbuf =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("text_size_vbuf"),
                                contents: bytemuck::cast_slice(&ts_verts),
                                usage: wgpu::BufferUsages::VERTEX,
                            });
                    let ts_ibuf =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("text_size_ibuf"),
                                contents: bytemuck::cast_slice(&ts_idx),
                                usage: wgpu::BufferUsages::INDEX,
                            });
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("text_size_pass"),
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
                        pass.set_vertex_buffer(0, ts_vbuf.slice(..));
                        pass.set_index_buffer(ts_ibuf.slice(..), wgpu::IndexFormat::Uint16);
                        pass.draw_indexed(0..ts_idx.len() as u32, 0, 0..1);
                    }
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}
