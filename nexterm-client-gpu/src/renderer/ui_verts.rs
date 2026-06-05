//! Sprint 2-1 Phase A: UI vertex builders for borders, the tab bar, and the status line.
//!
//! Six UI vertex-builder methods extracted from `renderer.rs`.

use crate::color_util::hex_to_rgba;
use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_string_verts};

use super::WgpuState;

impl WgpuState {
    /// Draw the pane border lines.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_border_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        tab_bar_h: f32,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
    ) {
        if state.pane_layouts.len() <= 1 {
            return;
        }
        // Tokyo Night: separator color #415068, focused border #7AA2F7
        let border_color = [0.255, 0.286, 0.408, 1.0];
        let focused_border = [0.478, 0.635, 0.969, 1.0];
        // Focused-pane border highlight (light blue, alpha 0.25)
        let focused_highlight = [0.478, 0.635, 0.969, 0.25];
        // Border is 2px wide for visibility
        let border_w = 2.0_f32;

        for layout in state.pane_layouts.values() {
            let px = layout.col_offset as f32 * cell_w;
            let py = layout.row_offset as f32 * cell_h + tab_bar_h;
            let pw = layout.cols as f32 * cell_w;
            let ph = layout.rows as f32 * cell_h;
            let is_focused = state.focused_pane_id == Some(layout.pane_id);

            // Draw a faint highlight border (2px) on the focused pane
            if is_focused && state.pane_layouts.len() > 1 {
                // Top edge
                add_px_rect(px, py, pw, 2.0, focused_highlight, sw, sh, bg_verts, bg_idx);
                // Bottom edge
                add_px_rect(
                    px,
                    py + ph - 2.0,
                    pw,
                    2.0,
                    focused_highlight,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // Left edge
                add_px_rect(px, py, 2.0, ph, focused_highlight, sw, sh, bg_verts, bg_idx);
                // Right edge
                add_px_rect(
                    px + pw - 2.0,
                    py,
                    2.0,
                    ph,
                    focused_highlight,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // If a pane sits to the right, draw a 1px vertical border
            let right_col = layout.col_offset + layout.cols + 1;
            let color = if is_focused {
                focused_border
            } else {
                border_color
            };
            if state
                .pane_layouts
                .values()
                .any(|o| o.pane_id != layout.pane_id && o.col_offset == right_col)
            {
                add_px_rect(px + pw, py, border_w, ph, color, sw, sh, bg_verts, bg_idx);
            }

            // If a pane sits below, draw a 1px horizontal border
            let bottom_row = layout.row_offset + layout.rows + 1;
            if state
                .pane_layouts
                .values()
                .any(|o| o.pane_id != layout.pane_id && o.row_offset == bottom_row)
            {
                add_px_rect(px, py + ph, pw, border_w, color, sw, sh, bg_verts, bg_idx);
            }
        }
    }

    /// Build the tab-bar vertices (top row of the window, WezTerm-style).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_tab_bar_verts(
        &mut self,
        state: &mut ClientState,
        cfg: &nexterm_config::TabBarConfig,
        animations_cfg: &nexterm_config::AnimationsConfig,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let bar_h = cfg.height as f32;
        let bar_y = 0.0_f32;
        // Height of the active-tab accent line (3px for better visibility)
        let accent_h = 3.0_f32;

        // Full-width tab-bar background (inactive color)
        let inactive_bg = hex_to_rgba(&cfg.inactive_tab_bg, 1.0);
        add_px_rect(0.0, bar_y, sw, bar_h, inactive_bg, sw, sh, bg_verts, bg_idx);
        // Divider line at the bottom of the tab bar (matches the border color)
        add_px_rect(
            0.0,
            bar_y + bar_h - 1.0,
            sw,
            1.0,
            [0.255, 0.286, 0.408, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Render the "active tab" based on the focused pane ID
        let focused_id = state.focused_pane_id.unwrap_or(0);
        let active_bg = hex_to_rgba(&cfg.active_tab_bg, 1.0);
        let activity_bg = hex_to_rgba(&cfg.activity_tab_bg, 1.0);
        let accent_color = hex_to_rgba(&cfg.active_accent_color, 1.0);
        // Text colors: active uses near-white; inactive is muted via the config value (Sprint 5-7 / UI-1-1)
        let text_fg = [0.97, 0.97, 0.97, 1.0];
        let dim = cfg.inactive_text_brightness.clamp(0.2, 1.0);
        let inactive_fg = [dim, dim, dim, 1.0];

        let padding = cell_w;
        let sep = cfg.separator.clone();

        // Reserve the right-edge settings-button width first (fixed width to avoid emoji width drift)
        let settings_label = " * Settings ";
        let settings_w = 12.0 * cell_w;
        let tab_area_w = sw - settings_w;

        // Sprint 5-7 / Phase 2-3: tab display order follows `ClientState.tab_order`
        // (the logical tab order produced by the server from `Window.pane_order`).
        // When `tab_order` is empty (e.g. immediately after connect), fall back to
        // ascending `pane_layouts` keys.
        let pane_ids: Vec<u32> = if state.tab_order.is_empty() {
            let mut v: Vec<u32> = state.pane_layouts.keys().copied().collect();
            v.sort();
            v
        } else {
            state
                .tab_order
                .iter()
                .copied()
                .filter(|id| state.pane_layouts.contains_key(id))
                .collect()
        };

        // Refresh the click-hit table every frame
        state.tab_hit_rects.clear();
        // Sprint 5-9 Phase 4-6: clear the tab tear-out `[↗]` button hit regions every frame, too
        state.tab_tearout_hit_rects.clear();

        let mut x_offset = 0.0_f32;
        let text_y = bar_y + (bar_h - cell_h) / 2.0;

        for (i, &pane_id) in pane_ids.iter().enumerate() {
            let is_active = pane_id == focused_id;
            let is_hovered = state.hovered_tab_id == Some(pane_id);
            // Pick up the activity flag and the title
            let (has_activity, raw_title) = state
                .panes
                .get(&pane_id)
                .map(|p| (p.has_activity, p.title.clone()))
                .unwrap_or((false, String::new()));

            // Tab label: show the OSC title if any; otherwise the pane number
            let base_label = if raw_title.is_empty() {
                format!("pane:{}", pane_id)
            } else {
                // Trim titles that are too long (max 24 chars)
                let truncated: String = raw_title.chars().take(24).collect();
                if raw_title.chars().count() > 24 {
                    format!("{}…", truncated)
                } else {
                    truncated
                }
            };
            // Tab number prefix (Windows Terminal style): prepends `[N]` when the option is on
            let numbered = if cfg.show_tab_number {
                format!("[{}] {}", i + 1, base_label)
            } else {
                base_label
            };
            let label = if has_activity && !is_active {
                format!(" {} ● ", numbered)
            } else {
                format!(" {} ", numbered)
            };
            let label_w =
                (label.chars().count() as f32 * cell_w + padding * 2.0).min(tab_area_w - x_offset); // don't spill out of the tab area

            if label_w < cell_w * 2.0 {
                break; // no more room to draw additional tabs
            }

            // Decide the tab background color:
            //   1. Active -> active_bg
            //   2. Inactive but has activity -> activity_bg (from config)
            //   3. Hovered -> brightened inactive_bg
            //   4. Normal -> inactive_bg
            let tab_bg = if is_active {
                active_bg
            } else if has_activity {
                activity_bg
            } else if is_hovered && cfg.hover_highlight {
                [
                    (inactive_bg[0] + 0.06).min(1.0),
                    (inactive_bg[1] + 0.06).min(1.0),
                    (inactive_bg[2] + 0.08).min(1.0),
                    inactive_bg[3],
                ]
            } else {
                inactive_bg
            };

            // Tab background
            add_px_rect(
                x_offset, bar_y, label_w, bar_h, tab_bg, sw, sh, bg_verts, bg_idx,
            );
            // Draw the accent line (config color) under the active tab.
            // Sprint 5-7 / Phase 3-2: just after a tab switch, fade the accent line in
            // with ease-out and expand it horizontally (can be suppressed by reduced-motion
            // settings).
            if is_active {
                let tab_switch_duration = animations_cfg.scaled_duration_ms(200);
                let progress = if tab_switch_duration == 0
                    || state.animations.current_tab_switch_target() != Some(pane_id)
                {
                    1.0
                } else {
                    let raw = state
                        .animations
                        .tab_switch_progress(std::time::Instant::now(), tab_switch_duration);
                    crate::animations::ease_out_cubic(raw)
                };
                let mut accent = accent_color;
                accent[3] = accent_color[3] * progress;
                // The underline grows outward from the center toward both ends
                let accent_w = label_w * progress;
                let accent_x = x_offset + (label_w - accent_w) / 2.0;
                add_px_rect(
                    accent_x,
                    bar_y + bar_h - accent_h,
                    accent_w,
                    accent_h,
                    accent,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // Tab label (vertically centered)
            let fg = if is_active { text_fg } else { inactive_fg };
            add_string_verts(
                &label,
                x_offset + padding,
                text_y,
                fg,
                is_active,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );

            // Record the click-hit range
            state
                .tab_hit_rects
                .insert(pane_id, (x_offset, x_offset + label_w));

            // Sprint 5-9 Phase 4-6: while the tab is hovered, draw the `[↗]` tear-out button.
            // Conditions:
            //   - hovered
            //   - not currently dragging a tab (drag conflicts with the ghost-tab render)
            //   - the tab is at least minimally wide (>= cell_w * 4)
            //
            // Button area: a square (about cell_w x cell_w) inset from the tab's right edge
            // by `padding`. A click fires the `DetachToNewWindow` path (hit-detected in
            // `mouse.rs`).
            let tearout_min_width = cell_w * 4.0;
            if is_hovered && state.tab_drag.is_none() && label_w >= tearout_min_width {
                let btn_size = cell_w; // a 1-cell-wide square
                let btn_x = x_offset + label_w - padding - btn_size;
                let btn_y = bar_y + (bar_h - cell_h) / 2.0;
                // Draw the U+2197 NORTH EAST ARROW (some fonts will substitute it)
                add_string_verts(
                    "↗",
                    btn_x,
                    btn_y,
                    fg,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // Make the hit region slightly larger around the button center (favors clickability)
                let hit_x0 = btn_x - cell_w * 0.25;
                let hit_x1 = btn_x + btn_size + cell_w * 0.25;
                state
                    .tab_tearout_hit_rects
                    .insert(pane_id, (hit_x0, hit_x1));
            }

            x_offset += label_w;

            // Vertical divider between tabs (1px, light accent color)
            if i + 1 < pane_ids.len() {
                // Hide the divider next to the active tab (the accent line is enough)
                if !is_active && pane_ids[i + 1] != focused_id {
                    let line_h = bar_h * 0.6; // 60% of the tab-bar height
                    let line_y = bar_y + (bar_h - line_h) / 2.0;
                    add_px_rect(
                        x_offset,
                        line_y,
                        1.0,
                        line_h,
                        [0.25, 0.28, 0.38, 0.50],
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                // Keep the separator-string rendering for backward compatibility (default is empty)
                if !sep.trim().is_empty() {
                    let sep_w = cell_w;
                    let sep_bg = if is_active { active_bg } else { inactive_bg };
                    add_px_rect(
                        x_offset, bar_y, sep_w, bar_h, sep_bg, sw, sh, bg_verts, bg_idx,
                    );
                    add_string_verts(
                        &sep,
                        x_offset,
                        text_y,
                        inactive_fg,
                        false,
                        sw,
                        sh,
                        cell_w,
                        font,
                        atlas,
                        &self.queue,
                        text_verts,
                        text_idx,
                    );
                    x_offset += sep_w;
                }
            }
        }

        // Sprint 5-7 / Phase 2-3: overlays drawn while a tab is being dragged
        //   1. A vertical indicator line at the left edge of the drag target (insertion position)
        //   2. A translucent ghost tab at the mouse cursor position
        if let Some(drag) = state.tab_drag.as_ref().filter(|d| d.committed) {
            // Insertion indicator: only when `hover_target` exists and differs from the dragged tab
            if let Some(target_id) = drag.hover_target
                && target_id != drag.pane_id
                && let Some(&(tx0, _tx1)) = state.tab_hit_rects.get(&target_id)
            {
                let indicator_w = 3.0;
                add_px_rect(
                    tx0,
                    bar_y,
                    indicator_w,
                    bar_h,
                    accent_color,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }
            // Ghost tab: draw the dragged tab's label translucently at the cursor position
            if let Some(&(orig_x0, orig_x1)) = state.tab_hit_rects.get(&drag.pane_id) {
                let ghost_w = orig_x1 - orig_x0;
                let ghost_x = (drag.current_x - ghost_w / 2.0)
                    .max(0.0)
                    .min(tab_area_w - ghost_w);
                // Translucent active color (alpha=0.65 so the tab beneath the drop target is visible)
                let ghost_bg = [active_bg[0], active_bg[1], active_bg[2], 0.65];
                add_px_rect(
                    ghost_x, bar_y, ghost_w, bar_h, ghost_bg, sw, sh, bg_verts, bg_idx,
                );
                // Ghost label (the original tab name)
                let ghost_title = state
                    .panes
                    .get(&drag.pane_id)
                    .map(|p| p.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| format!("pane:{}", drag.pane_id));
                let truncated: String = ghost_title.chars().take(24).collect();
                let ghost_label = format!(" {} ", truncated);
                add_string_verts(
                    &ghost_label,
                    ghost_x + padding,
                    text_y,
                    text_fg,
                    false,
                    sw,
                    sh,
                    cell_w,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
        }

        // Right edge: settings button
        let settings_x = sw - settings_w;
        let settings_open = state.settings_panel.is_open;
        let settings_bg = if settings_open {
            active_bg
        } else {
            // Slightly brighter than the inactive color to make it stand out
            [
                inactive_bg[0] + 0.05,
                inactive_bg[1] + 0.05,
                inactive_bg[2] + 0.08,
                1.0,
            ]
        };
        add_px_rect(
            settings_x,
            bar_y,
            settings_w,
            bar_h,
            settings_bg,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        let settings_fg = if settings_open {
            text_fg
        } else {
            [0.80, 0.80, 0.80, 1.0]
        };
        add_string_verts(
            settings_label,
            settings_x,
            text_y,
            settings_fg,
            settings_open,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
        // Record the click rectangle of the settings button.
        state.settings_tab_rect = Some((settings_x, sw));

        // When renaming a tab, display an inline edit field at the tab's position.
        if let Some(rename_id) = state.settings_panel.tab_rename_editing
            && let Some(&(tx0, tx1)) = state.tab_hit_rects.get(&rename_id)
        {
            let edit_w = (tx1 - tx0).min(tab_area_w - tx0);
            // Edit field background (dark accent color).
            add_px_rect(
                tx0,
                bar_y,
                edit_w,
                bar_h,
                [0.231, 0.259, 0.384, 1.0],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            // Thicken the bottom accent line to indicate edit mode.
            add_px_rect(
                tx0,
                bar_y + bar_h - accent_h * 2.0,
                edit_w,
                accent_h * 2.0,
                [0.478, 0.635, 0.969, 1.0],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            // Text + cursor (append `|` at the end).
            let edit_text = format!(" {}|", state.settings_panel.tab_rename_text);
            add_string_verts(
                &edit_text,
                tx0 + padding,
                text_y,
                [1.0, 1.0, 1.0, 1.0],
                true,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }

    /// Build the status line vertices (bottom row of the window).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_status_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let py = sh - cell_h;
        // Status line background (Tokyo Night: #1E2030).
        add_px_rect(
            0.0,
            py,
            sw,
            cell_h,
            [0.118, 0.125, 0.188, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // 1px divider line at the top of the status line (#2D3149).
        add_px_rect(
            0.0,
            py,
            sw,
            1.0,
            [0.176, 0.192, 0.286, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Text: N icon + session name + pane info.
        let pane_id = state.focused_pane_id.unwrap_or(0);
        let activity_ids = state.active_pane_ids();
        let pane_count = state.pane_layouts.len();
        let status = if activity_ids.is_empty() {
            format!(" N  nexterm | pane:{}/{}", pane_id, pane_count)
        } else {
            let ids: Vec<String> = activity_ids.iter().map(|id| id.to_string()).collect();
            format!(
                " N  nexterm | pane:{}/{} | ●{}",
                pane_id,
                pane_count,
                ids.join(",")
            )
        };

        // Tokyo Night text color #A9B1D6.
        add_string_verts(
            &status,
            0.0,
            py,
            [0.663, 0.694, 0.839, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Show the right-side widget (status_bar_right_text or legacy status_bar_text) at the far right.
        let right_widget_src = if !state.status_bar_right_text.is_empty() {
            &state.status_bar_right_text
        } else {
            &state.status_bar_text
        };
        let mut right_offset = 0.0f32;
        if !right_widget_src.is_empty() {
            let widget_text = format!(" {} ", right_widget_src);
            let text_w = widget_text.chars().count() as f32 * cell_w;
            right_offset = text_w;
            let right_px = sw - text_w;
            add_string_verts(
                &widget_text,
                right_px,
                py,
                [0.4, 0.9, 0.6, 1.0],
                false,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // Display the left-side widget (status_bar_text) if it is also set.
        // (Shown left-aligned, independent of right_widgets.)
        if !state.status_bar_right_text.is_empty() && !state.status_bar_text.is_empty() {
            let left_text = format!(" {} ", state.status_bar_text);
            let left_end = left_text.chars().count() as f32 * cell_w;
            // The left-side widget is rendered to the right of the `nexterm | pane:` text.
            let base_left = {
                let pane_id = state.focused_pane_id.unwrap_or(0);
                let activity_ids = state.active_pane_ids();
                let status = if activity_ids.is_empty() {
                    format!(" nexterm | pane:{}", pane_id)
                } else {
                    let ids: Vec<String> = activity_ids.iter().map(|id| id.to_string()).collect();
                    format!(" nexterm | pane:{} | activity:{}", pane_id, ids.join(","))
                };
                status.chars().count() as f32 * cell_w
            };
            let _ = left_end;
            let _ = base_left;
            // TODO: Extend the left-widget offset computation in the future.
        }

        // Right-edge indicators (stacked from right to left).

        // Zoom indicator (yellow `[Z]` label).
        if state.is_zoomed {
            let zoom_text = " [Z] ";
            right_offset += zoom_text.chars().count() as f32 * cell_w;
            let right_px = sw - right_offset;
            add_string_verts(
                zoom_text,
                right_px,
                py,
                [1.0, 0.85, 0.2, 1.0],
                true,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // While in scrollback, show the indicator to the left of the widgets.
        if let Some(pane) = state.focused_pane()
            && pane.scroll_offset > 0
        {
            let scroll_text = format!(" ↑{} ", pane.scroll_offset);
            let right_px = sw - scroll_text.chars().count() as f32 * cell_w - right_offset;
            add_string_verts(
                &scroll_text,
                right_px,
                py,
                [1.0, 0.85, 0.2, 1.0],
                true,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }
    }

    /// Build the search bar vertices (overlay at the bottom of the window).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_search_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        // Display the search bar one row above the status line.
        let py = sh - cell_h * 2.0;
        add_px_rect(
            0.0,
            py,
            sw,
            cell_h,
            [0.08, 0.10, 0.15, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // Draw a thin accent line along the top edge.
        add_px_rect(
            0.0,
            py,
            sw,
            2.0,
            [0.3, 0.7, 1.0, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Search query and cursor (always show `|` instead of blinking).
        let query_with_cursor = format!("{}|", state.search.query);
        let match_text = if let Some(idx) = state.search.current_match {
            format!("  ↑↓:{}", idx)
        } else if !state.search.query.is_empty() {
            "  (no match)".to_string()
        } else {
            String::new()
        };
        let label = format!(" / {}{}", query_with_cursor, match_text);
        add_string_verts(
            &label,
            0.0,
            py,
            [0.3, 1.0, 0.5, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Show key hint text at the far right.
        let hint = "Enter/↑ next  Shift+Enter/↑ prev  Esc close ";
        let hint_x = sw - hint.chars().count() as f32 * cell_w;
        add_string_verts(
            hint,
            hint_x.max(0.0),
            py,
            [0.55, 0.55, 0.55, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }

    /// Build the update-notification banner vertices (one-line bar at the top of the screen).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_update_banner_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let Some(ref version) = state.update_banner else {
            return;
        };

        // The banner spans the full screen width and is one row tall.
        let bar_h = cell_h * 1.4;
        let bar_y = 0.0;

        // Background (dark green).
        add_px_rect(
            0.0,
            bar_y,
            sw,
            bar_h,
            [0.05, 0.28, 0.18, 0.97],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // Left-edge accent line (bright green).
        add_px_rect(
            0.0,
            bar_y,
            4.0,
            bar_h,
            [0.15, 0.85, 0.45, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Notification text (uses the i18n key "update-available", substituting {version}).
        let raw = nexterm_i18n::fl!("update-available");
        let msg = raw.replace("{version}", version);
        add_string_verts(
            &msg,
            cell_w * 1.2,
            bar_y + (bar_h - cell_h) * 0.5,
            [0.88, 1.0, 0.88, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Right-side hint (press Esc to close).
        let hint = "  [Esc]";
        let hint_x = sw - hint.len() as f32 * cell_w - cell_w;
        add_string_verts(
            hint,
            hint_x,
            bar_y + (bar_h - cell_h) * 0.5,
            [0.55, 0.80, 0.55, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }

    /// Build the offline-mode banner vertices (one-line amber bar at the top of the screen).
    ///
    /// Sprint 5-14 / v1.7.8 — P2-1. Shown while the client is repeatedly failing
    /// to connect to the embedded server. Surfaces what was previously a silent
    /// blank window during cold start, especially on Windows where the
    /// `\\.\pipe\nexterm-<user>` named pipe may take >1 s to come up.
    /// Auto-clears as soon as the connection succeeds (no key dismissal).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_offline_banner_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let Some(started) = state.offline_banner_since else {
            return;
        };

        // The banner spans the full screen width and is one row tall. Stack it
        // below `update_banner` if present.
        let bar_h = cell_h * 1.4;
        let bar_y = if state.update_banner.is_some() {
            bar_h
        } else {
            0.0
        };

        // Background (amber / warning orange).
        add_px_rect(
            0.0,
            bar_y,
            sw,
            bar_h,
            [0.45, 0.28, 0.05, 0.97],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // Left-edge accent line (bright amber).
        add_px_rect(
            0.0,
            bar_y,
            4.0,
            bar_h,
            [0.95, 0.62, 0.15, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Format the seconds count for display.
        let elapsed_secs = started.elapsed().as_secs();
        let raw = nexterm_i18n::fl!("offline-banner-connecting");
        let msg = raw.replace("{seconds}", &elapsed_secs.to_string());
        add_string_verts(
            &msg,
            cell_w * 1.2,
            bar_y + (bar_h - cell_h) * 0.5,
            [1.0, 0.92, 0.78, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }

    /// Build the server error banner vertices (one-line bar at the top of the screen, just
    /// below `update_banner`).
    ///
    /// Sprint 5-12 Phase 1: surfaces `ServerToClient::Error` events such as PTY launch
    /// failures (e.g. PowerShell not found), config load errors, and pane split failures so
    /// the user notices them immediately, via a red bar. Dismissed with `Esc`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_error_banner_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let Some(ref message) = state.error_banner else {
            return;
        };

        // The banner spans the full screen width and is one row tall. Stack
        // below `update_banner` and `offline_banner` (Sprint 5-14 / v1.7.8)
        // when either is present.
        let bar_h = cell_h * 1.4;
        let mut bar_y = 0.0_f32;
        if state.update_banner.is_some() {
            bar_y += bar_h;
        }
        if state.offline_banner_since.is_some() {
            bar_y += bar_h;
        }

        // Background (dark red).
        add_px_rect(
            0.0,
            bar_y,
            sw,
            bar_h,
            [0.40, 0.08, 0.08, 0.97],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // Left-edge accent line (bright red).
        add_px_rect(
            0.0,
            bar_y,
            4.0,
            bar_h,
            [0.95, 0.30, 0.30, 1.0],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Show "Error: {message}" (uses the i18n key "error-banner-prefix").
        let prefix = nexterm_i18n::fl!("error-banner-prefix");
        let full = format!("{} {}", prefix, message);
        // Truncate to what fits in the screen width (reserve space for the [Esc] hint on the right).
        let hint = "  [Esc]";
        let max_chars = ((sw / cell_w) as usize)
            .saturating_sub(hint.chars().count() + 4)
            .max(8);
        let msg_display: String = full.chars().take(max_chars).collect();
        add_string_verts(
            &msg_display,
            cell_w * 1.2,
            bar_y + (bar_h - cell_h) * 0.5,
            [1.0, 0.92, 0.92, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );

        // Right-side hint (press Esc to close).
        let hint_x = sw - hint.len() as f32 * cell_w - cell_w;
        add_string_verts(
            hint,
            hint_x,
            bar_y + (bar_h - cell_h) * 0.5,
            [0.95, 0.70, 0.70, 1.0],
            false,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }

    /// Build the Quick Select overlay vertices.
    ///
    /// At each match position, draw a label (a, b, ..., aa, ...) over a yellow background.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_quick_select_verts(
        &self,
        state: &ClientState,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let qs = &state.quick_select;
        if !qs.is_active {
            return;
        }

        // Fetch the offset of the focused pane.
        let (pane_x, pane_y) = if let Some(pid) = state.focused_pane_id {
            if let Some(layout) = state.pane_layouts.get(&pid) {
                (
                    layout.col_offset as f32 * cell_w,
                    layout.row_offset as f32 * cell_h,
                )
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };

        for m in &qs.matches {
            let lx = pane_x + m.col_start as f32 * cell_w;
            let ly = pane_y + m.row as f32 * cell_h;
            let label_w = m.label.len() as f32 * cell_w;

            // Semi-transparent highlight covering the entire match.
            let match_w = (m.col_end - m.col_start) as f32 * cell_w;
            add_px_rect(
                lx,
                ly,
                match_w,
                cell_h,
                [0.9, 0.85, 0.2, 0.25],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );

            // Label background (yellow).
            let is_partial_match =
                !qs.typed_label.is_empty() && m.label.starts_with(&qs.typed_label);
            let bg_color = if is_partial_match {
                [1.0, 0.6, 0.0, 0.95]
            } else {
                [0.9, 0.85, 0.1, 0.92]
            };
            add_px_rect(lx, ly, label_w, cell_h, bg_color, sw, sh, bg_verts, bg_idx);

            // Label text (black).
            add_string_verts(
                &m.label,
                lx,
                ly,
                [0.05, 0.05, 0.05, 1.0],
                true,
                sw,
                sh,
                cell_w,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
        }

        // Show the in-progress label at the top of the screen.
        let typed = format!("Quick Select: {}_", qs.typed_label);
        add_px_rect(
            0.0,
            0.0,
            typed.len() as f32 * cell_w + cell_w,
            cell_h,
            [0.15, 0.15, 0.18, 0.92],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_string_verts(
            &typed,
            cell_w * 0.5,
            0.0,
            [1.0, 0.85, 0.2, 1.0],
            true,
            sw,
            sh,
            cell_w,
            font,
            atlas,
            &self.queue,
            text_verts,
            text_idx,
        );
    }
}
