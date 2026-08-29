//! Sprint 2-1 Phase A: UI vertex builders for borders, the tab bar, and the status line.
//!
//! Six UI vertex-builder methods extracted from `renderer.rs`.

use crate::color_util::with_alpha;
use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{
    add_icon_verts, add_px_rect, add_px_rounded_rect_sdf, add_run_verts, add_string_verts,
    icon_size_for_slot,
};

use super::WgpuState;
use super::overlay::infobar;
use nexterm_config::SurfaceLevel;

/// Shared chrome for the one-line notification banners (update / offline / error).
///
/// Draws:
/// 1. A full-width background: `surface_2` tinted 18 % toward `accent`.
/// 2. A 1 px bottom divider using `border_subtle`.
/// 3. A 4 px left accent bar in the full `accent` colour.
///
/// Returns the opaque ground it painted. UI/UX v3 P5b: a banner is the one
/// piece of chrome whose ground is neither a surface token nor a plain fill —
/// it is a tint between the two — so its labels cannot take a `text_on(..)`
/// colour as-is. Callers correct against this value instead of approximating
/// it with `S2`.
#[allow(clippy::too_many_arguments)]
fn draw_banner_bg(
    bx: f32,
    by: f32,
    bw: f32,
    bh: f32,
    accent: [f32; 4],
    tokens: &nexterm_config::DesignTokens,
    sw: f32,
    sh: f32,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
) -> [f32; 3] {
    let [r2, g2, b2, _] = tokens.surface_2;
    let [ra, ga, ba, _] = accent;
    let bg = [
        r2 * 0.82 + ra * 0.18,
        g2 * 0.82 + ga * 0.18,
        b2 * 0.82 + ba * 0.18,
        0.97,
    ];
    add_px_rect(bx, by, bw, bh, bg, sw, sh, bg_verts, bg_idx);
    add_px_rect(
        bx,
        by + bh - 1.0,
        bw,
        1.0,
        tokens.border_subtle,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    add_px_rect(bx, by, 4.0, bh, accent, sw, sh, bg_verts, bg_idx);
    // The alpha is 0.97 over an already-opaque chrome layer; the 3 % of
    // whatever is behind it cannot move the ratio meaningfully, and treating
    // the tint as opaque keeps the correction independent of draw order.
    [bg[0], bg[1], bg[2]]
}

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
        tokens: &nexterm_config::DesignTokens,
        // Phase 6 (UI/UX v2): consulted for `inactive_pane_hsb`.
        config: &nexterm_config::Config,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
    ) {
        if state.pane_layouts.len() <= 1 {
            return;
        }
        // Phase 3 (UI/UX modernization): pane border & focus visualization.
        //   - Non-focused adjacent borders: 2px `border_subtle` (quiet).
        //   - Focused adjacent borders:    3px `accent_primary` (clearly lifted).
        //   - Non-focused panes get a flat dim overlay (alpha 0.06 black) so the
        //     focused pane stands out without a halo on its own frame.
        let border_color = tokens.border_subtle;
        let focus_color = tokens.accent_primary;
        let border_w = 2.0_f32;
        let focus_border_w = 3.0_f32;
        // 1) Dim non-focused panes (only meaningful when >=2 panes).
        // Phase 4 (UI/UX modernization): alpha is spring-animated via AnimationManager.
        // Phase 6 (UI/UX v2): overlay colour + alpha are now derived from
        // [`InactivePaneHsbConfig`] instead of the hard-coded black-alpha
        // pair, so the user can dial saturation / brightness in `config.toml`.
        // The spring still drives the transition; we pass it in as
        // `animation_t` (normalised to [0, 1] from MAX_DIM_ALPHA).
        // Phase 6b (UI/UX v2): when `hsb.is_active()`, the per-cell HSB
        // transform inside `build_grid_verts_in_rect` already produces
        // the dimmed look (and a real hue shift). Drawing the legacy
        // grey-alpha overlay on top of it would double-dim, so this
        // path is now skipped entirely when HSB is active. The overlay
        // is retained only as a no-op safety net in case the Phase-6b
        // CPU transform ever needs to be temporarily bypassed.
        let hsb = &config.inactive_pane_hsb;
        if hsb.is_active() {
            // Per-cell transform handles the inactive look; nothing to
            // overlay here.
        }

        // 2) Adjacent borders. The focused pane's edges get accent_primary at 3px,
        //    everything else stays at the subtle 2px divider.
        for layout in state.pane_layouts.values() {
            let px = layout.col_offset as f32 * cell_w;
            let py = layout.row_offset as f32 * cell_h + tab_bar_h;
            let pw = layout.cols as f32 * cell_w;
            let ph = layout.rows as f32 * cell_h;
            let is_focused = state.focused_pane_id == Some(layout.pane_id);
            let (color, w) = if is_focused {
                (focus_color, focus_border_w)
            } else {
                (border_color, border_w)
            };

            // Right neighbor → vertical border on the right edge.
            let right_col = layout.col_offset + layout.cols + 1;
            if state
                .pane_layouts
                .values()
                .any(|o| o.pane_id != layout.pane_id && o.col_offset == right_col)
            {
                add_px_rect(px + pw, py, w, ph, color, sw, sh, bg_verts, bg_idx);
            }

            // Bottom neighbor → horizontal border on the bottom edge.
            let bottom_row = layout.row_offset + layout.rows + 1;
            if state
                .pane_layouts
                .values()
                .any(|o| o.pane_id != layout.pane_id && o.row_offset == bottom_row)
            {
                add_px_rect(px, py + ph, pw, w, color, sw, sh, bg_verts, bg_idx);
            }

            // Left neighbor → vertical border on the left edge (focused only;
            // the neighbor draws the divider in the unfocused case).
            if is_focused && layout.col_offset > 0 {
                let left_col = layout.col_offset.saturating_sub(1);
                if state.pane_layouts.values().any(|o| {
                    o.pane_id != layout.pane_id && o.col_offset + o.cols + 1 == left_col + 1
                }) {
                    add_px_rect(px - w, py, w, ph, color, sw, sh, bg_verts, bg_idx);
                }
            }

            // Top neighbor → horizontal border on the top edge (focused only).
            if is_focused && layout.row_offset > 0 {
                let top_row = layout.row_offset.saturating_sub(1);
                if state.pane_layouts.values().any(|o| {
                    o.pane_id != layout.pane_id && o.row_offset + o.rows + 1 == top_row + 1
                }) {
                    add_px_rect(px, py - w, pw, w, color, sw, sh, bg_verts, bg_idx);
                }
            }
        }
    }

    /// Build the tab-bar vertices (top row of the window, WezTerm-style).
    ///
    /// Sprint 5-15 / UI/UX Modernization v2 Phase 2a: tab backgrounds are
    /// drawn with the SDF rounded-rect helper so the active and inactive tabs
    /// render as proper pills. The radius comes from `ui_cfg.chrome_radius()`;
    /// `0.0` reproduces the flat-rect look from earlier builds.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_tab_bar_verts(
        &mut self,
        state: &mut ClientState,
        cfg: &nexterm_config::TabBarConfig,
        _animations_cfg: &nexterm_config::AnimationsConfig,
        ui_cfg: &nexterm_config::UiConfig,
        tokens: &nexterm_config::DesignTokens,
        // Custom title bar (`window.decorations = "notitle"`): draw the
        // minimize / maximize / close buttons at the right edge.
        custom_titlebar: bool,
        is_maximized: bool,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        now: std::time::Instant,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let bar_h = cfg.height as f32;
        let bar_y = 0.0_f32;
        // Phase 2 (UI/UX modernization): browser-style pill tabs.
        //   - Bottom accent line: 2px (reduced from 3px) for a quieter look.
        //   - Top highlight line: 2px on the active tab for a "lifted" feel.
        //   - 4px transparent gap between tabs replaces the vertical divider.
        let accent_h = 2.0_f32;
        let top_highlight_h = 2.0_f32;
        const TAB_GAP_PX: f32 = 4.0;

        // Resolve each color: user override (Some) takes priority, otherwise use the token.
        let inactive_bg =
            nexterm_config::resolve_color(cfg.inactive_tab_bg.as_deref(), tokens.tab_inactive_bg);
        add_px_rect(0.0, bar_y, sw, bar_h, inactive_bg, sw, sh, bg_verts, bg_idx);
        // Divider line at the bottom of the tab bar
        add_px_rect(
            0.0,
            bar_y + bar_h - 1.0,
            sw,
            1.0,
            tokens.border_subtle,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Render the "active tab" based on the focused pane ID
        let focused_id = state.focused_pane_id.unwrap_or(0);
        let active_bg =
            nexterm_config::resolve_color(cfg.active_tab_bg.as_deref(), tokens.tab_active_bg);
        let activity_bg =
            nexterm_config::resolve_color(cfg.activity_tab_bg.as_deref(), tokens.tab_activity_bg);
        let accent_color = nexterm_config::resolve_color(
            cfg.active_accent_color.as_deref(),
            tokens.accent_primary,
        );
        // Text colors derived from tokens
        let text_fg = tokens.text_on(SurfaceLevel::S2).primary;
        let dim = cfg.inactive_text_brightness.clamp(0.2, 1.0);
        let inactive_fg = [dim, dim, dim, 1.0];

        let padding = cell_w;
        let sep = cfg.separator.clone();

        // Custom title bar: the window buttons occupy the true right edge
        // (outside the Settings pill, where every native title bar puts
        // them), so reserve their width before anything else. Widths are
        // `cell_w`-relative, which keeps them DPI-scaled for free.
        let window_button_w = 4.0 * cell_w;
        let window_buttons_w = if custom_titlebar {
            3.0 * window_button_w
        } else {
            0.0
        };
        // Reserve the right-edge settings-button width first (fixed width to avoid emoji width drift)
        let settings_label = " * Settings ";
        let settings_w = 12.0 * cell_w;
        // Sprint 5-15 / Phase 2b: optional `+` new-tab button left of Settings.
        let new_tab_w = if cfg.show_new_tab_button {
            4.0 * cell_w
        } else {
            0.0
        };
        // P1 (WT-like UX): `▾` profile-dropdown button rendered right of `+`.
        let dropdown_w = if cfg.show_new_tab_button {
            3.0 * cell_w
        } else {
            0.0
        };
        let tab_area_w = sw - window_buttons_w - settings_w - new_tab_w - dropdown_w;

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
        // Phase 2 (UI/UX modernization): clear close `×` button hit regions every frame
        state.tab_close_hit_rects.clear();
        // Sprint 5-15 / Phase 2b: clear the new-tab `+` button hit region every frame
        state.new_tab_hit_rect = None;
        // P1 (WT-like UX): clear the new-tab `▾` dropdown button hit region too
        state.new_tab_dropdown_hit_rect = None;

        let mut x_offset = 0.0_f32;
        let text_y = bar_y + (bar_h - cell_h) / 2.0;
        // UI/UX v3 N-3b: tab labels draw at the chrome ramp, and their width
        // comes from `tab_layout` rather than from a character count. Body for
        // an inactive tab, Body Strong for the active one — the distinction
        // the cell path drew with its `bold` flag.
        let tab_ramp = nexterm_config::MetricTokens::default().type_ramp;
        let (_size, tab_line_h, _bold) = font.chrome_metrics(&tab_ramp.body);
        let tab_text_y = bar_y + (bar_h - tab_line_h) / 2.0;

        for (i, &pane_id) in pane_ids.iter().enumerate() {
            let is_active = pane_id == focused_id;
            let is_hovered = state.hovered_tab_id == Some(pane_id);
            // Pick up the activity flag, the title, and the Phase 2c
            // foreground process name (drives the Nerd Font glyph).
            let (has_activity, raw_title, process_name, pane_progress) = state
                .panes
                .get(&pane_id)
                .map(|p| {
                    (
                        p.has_activity,
                        p.title.clone(),
                        p.process_name.clone(),
                        p.progress,
                    )
                })
                .unwrap_or((false, String::new(), None, None));

            // Tab label: show the OSC title if any; otherwise the pane number.
            //
            // N-3b: the 24-character cap is gone. Twenty-four characters is 24
            // cells of Latin or 48 of Japanese, so it never bounded the drawn
            // width; the label is cut to the room the strip actually has,
            // below, by `truncate_run_to_width`.
            let base_label = if raw_title.is_empty() {
                format!("pane:{}", pane_id)
            } else {
                raw_title.clone()
            };
            // Phase 2c: the Nerd Font glyph for the foreground process, when
            // (a) the user opted in via `tab_bar.show_process_icon` and (b) the
            // glyph map has an entry for it. Unknown processes get nothing —
            // no fallback glyph, because the absence is signal.
            //
            // N-3c: it is drawn beside the label rather than prepended to it,
            // and on the *cell* path. It is a Nerd Font codepoint from the
            // user's terminal font, and `icons.rs` warns that the bundled
            // chrome-icon subset occupies the same Private Use Area — so
            // drawing it through the icon path would resolve a Fluent icon in
            // its place. The cell path is where it has always come from, and it
            // boxes a glyph to a whole cell instead of to its advance, which is
            // what keeps an overhanging icon from being clipped.
            let process_glyph = if cfg.show_process_icon {
                process_name
                    .as_deref()
                    .and_then(crate::tab_icons::glyph_for_process)
            } else {
                None
            };
            let icon_w = process_glyph
                .map(|glyph| crate::vertex_util::visual_width(glyph) as f32 * cell_w)
                .unwrap_or(0.0);
            // Tab number prefix (Windows Terminal style): prepends `[N]` when the option is on
            let numbered = if cfg.show_tab_number {
                format!("[{}] {}", i + 1, base_label)
            } else {
                base_label
            };
            // The cell path wrapped every label in spaces because `padding`
            // alone did not read as padding at cell precision. It does now, and
            // measured spaces would be counted twice, so they go.
            let label = if has_activity && !is_active {
                format!("{} ●", numbered)
            } else {
                numbered
            };

            // N-3b: measure the label, never count it. The tab is cut to the
            // room left, then sized from what is actually drawn, so the pill,
            // the click region (`tab_hit_rects`), the accent underline, the
            // progress bar and both hover buttons all follow one correct
            // number instead of a character count that disagreed with the
            // drawing pass for every full-width glyph.
            let tab_style = if is_active {
                tab_ramp.body_strong
            } else {
                tab_ramp.body
            };
            let room_left = tab_area_w - x_offset;
            let label = crate::vertex_util::truncate_run_to_width(
                &label,
                &tab_style,
                (room_left - padding * 2.0 - icon_w).max(0.0),
                font,
            );
            let Some(label_w) = crate::renderer::tab_layout::tab_width(
                &label, &tab_style, icon_w, padding, room_left, font,
            ) else {
                break; // no more room to draw additional tabs
            };

            // Decide the tab background color:
            //   1. Active -> active_bg
            //   2. Inactive but has activity -> activity_bg (from config)
            //   3. Hovered -> brightened inactive_bg, cross-faded (P3b2b)
            //   4. Normal -> inactive_bg
            let tab_bg = if is_active {
                active_bg
            } else if has_activity {
                activity_bg
            } else if cfg.hover_highlight {
                // The quad is always drawn, so the *colour* moves — alpha
                // scaling would fade the tab out of the bar instead of into
                // its hover tint.
                let hovered_bg = [
                    (inactive_bg[0] + 0.06).min(1.0),
                    (inactive_bg[1] + 0.06).min(1.0),
                    (inactive_bg[2] + 0.08).min(1.0),
                    inactive_bg[3],
                ];
                // UI/UX v3 P3b3: press raises the weight before dimming it —
                // a click that lands inside the hover fade's first 100 ms
                // would otherwise have almost no fill to dim.
                let press = state.tab_press.weight(pane_id, now);
                let w = state.tab_hover.weight(pane_id, now).max(press);
                crate::color_util::press_fill(
                    crate::color_util::lerp_rgba(inactive_bg, hovered_bg, w),
                    press,
                )
            } else {
                inactive_bg
            };

            // Tab background — Sprint 5-15 / Phase 2a: pill-shaped via the
            // SDF rounded-rect helper. Radius is bounded by the tab height so
            // pills never become full ellipses on a tall tab bar.
            let tab_radius = ui_cfg.chrome_radius().min(bar_h * 0.5);
            add_px_rounded_rect_sdf(
                x_offset, bar_y, label_w, bar_h, tab_radius, tab_bg, sw, sh, bg_verts, bg_idx,
            );
            // Draw the accent line (config color) under the active tab.
            // Sprint 5-7 / Phase 3-2: just after a tab switch, fade the accent line in
            // with ease-out and expand it horizontally (can be suppressed by reduced-motion
            // settings).
            if is_active {
                // Phase 4 (UI/UX modernization): spring-physics drives the accent line.
                let progress = state.animations.tab_accent_progress();
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
                // Phase 2 (UI/UX modernization): pill-style top highlight on the active tab.
                // A muted accent line at the top edge gives a subtle "lifted" feel without
                // requiring true rounded corners (which would need a custom shader).
                let mut top_hi = accent_color;
                top_hi[3] = accent_color[3] * 0.45 * progress;
                add_px_rect(
                    accent_x,
                    bar_y,
                    accent_w,
                    top_highlight_h,
                    top_hi,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // OSC 9;4 progress indicator: a thin bar just above the accent
            // line, colored by state. Drawn for active and inactive tabs
            // alike so long-running jobs stay visible across tab switches.
            if let Some((color, frac)) = progress_indicator_style(pane_progress, tokens) {
                add_px_rect(
                    x_offset,
                    bar_y + bar_h - accent_h - 3.0,
                    label_w * frac,
                    2.0,
                    color,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
            }

            // Tab label (vertically centred on the run's own line box), with
            // the process icon in the cell-wide slot reserved before it.
            let fg = if is_active { text_fg } else { inactive_fg };
            if let Some(glyph) = process_glyph {
                add_string_verts(
                    glyph,
                    x_offset + padding,
                    text_y,
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
            }
            add_run_verts(
                &label,
                &tab_style,
                x_offset + padding + icon_w,
                tab_text_y,
                fg,
                sw,
                sh,
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
            // Phase 2 (UI/UX modernization): require room for both the
            // tear-out and the close button. Minimum tab width raised from 4 to
            // 6 cells.
            let hover_btn_min_width = cell_w * 6.0;
            if is_hovered && state.tab_drag.is_none() && label_w >= hover_btn_min_width {
                let btn_size = cell_w; // a 1-cell-wide square
                let btn_y = bar_y + (bar_h - cell_h) / 2.0;
                // Close button at the far right, tear-out one slot to its left.
                let close_x = x_offset + label_w - padding - btn_size;
                let tearout_x = close_x - btn_size;
                // UI/UX v3 P4a: both buttons draw from the bundled icon font.
                // The slots are unchanged — `mouse.rs` hit-tests the same
                // squares it did when these were `↗` and `×` glyphs.
                let btn_icon_size = icon_size_for_slot(font.icon_px(16.0), btn_size, cell_h, 0.2);
                add_icon_verts(
                    crate::icons::TAB_TEAR_OUT,
                    tearout_x,
                    btn_y,
                    btn_size,
                    cell_h,
                    btn_icon_size,
                    fg,
                    sw,
                    sh,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                add_icon_verts(
                    crate::icons::TAB_CLOSE,
                    close_x,
                    btn_y,
                    btn_size,
                    cell_h,
                    btn_icon_size,
                    fg,
                    sw,
                    sh,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                // Hit regions: pad slightly to favor clickability
                let pad = cell_w * 0.25;
                state
                    .tab_tearout_hit_rects
                    .insert(pane_id, (tearout_x - pad, tearout_x + btn_size + pad));
                state
                    .tab_close_hit_rects
                    .insert(pane_id, (close_x - pad, close_x + btn_size + pad));
            }

            x_offset += label_w;

            // Phase 2 (UI/UX modernization): a 4px transparent gap separates tabs
            // visually instead of a 1px vertical divider line. The gap lets the tab
            // bar background show through, giving the tabs a discrete pill feel.
            if i + 1 < pane_ids.len() {
                x_offset += TAB_GAP_PX;
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
                let ghost_radius = ui_cfg.chrome_radius().min(bar_h * 0.5);
                add_px_rounded_rect_sdf(
                    ghost_x,
                    bar_y,
                    ghost_w,
                    bar_h,
                    ghost_radius,
                    ghost_bg,
                    sw,
                    sh,
                    bg_verts,
                    bg_idx,
                );
                // Ghost label (the original tab name)
                let ghost_title = state
                    .panes
                    .get(&drag.pane_id)
                    .map(|p| p.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| format!("pane:{}", drag.pane_id));
                // N-3b: the ghost carries the same defect its tab did — it
                // was cut to 24 characters and drawn on the cell path inside a
                // pill whose width comes from `tab_hit_rects`. It is cut to
                // that pill now, and drawn as a run like the tab it copies.
                let ghost_style = tab_ramp.body_strong;
                let ghost_label = crate::vertex_util::truncate_run_to_width(
                    &ghost_title,
                    &ghost_style,
                    (ghost_w - padding * 2.0).max(0.0),
                    font,
                );
                add_run_verts(
                    &ghost_label,
                    &ghost_style,
                    ghost_x + padding,
                    tab_text_y,
                    text_fg,
                    sw,
                    sh,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
            }
        }

        // Sprint 5-15 / Phase 2b: new-tab `+` pill (placed just before the
        // Settings button). The renderer registers `state.new_tab_hit_rect`
        // every frame; `event_handler/mouse.rs` consumes it and dispatches a
        // `NewPane` IPC on left-click.
        if cfg.show_new_tab_button && new_tab_w > 0.0 {
            let new_tab_x = sw - window_buttons_w - settings_w - dropdown_w - new_tab_w;
            let new_tab_bg = [
                inactive_bg[0] + 0.04,
                inactive_bg[1] + 0.04,
                inactive_bg[2] + 0.06,
                1.0,
            ];
            let new_tab_radius = ui_cfg.chrome_radius().min(bar_h * 0.5);
            add_px_rounded_rect_sdf(
                new_tab_x,
                bar_y,
                new_tab_w,
                bar_h,
                new_tab_radius,
                new_tab_bg,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            // `+` glyph centred within the pill.
            let plus_label = " + ";
            let plus_text_x =
                new_tab_x + (new_tab_w - plus_label.chars().count() as f32 * cell_w).max(0.0) * 0.5;
            add_string_verts(
                plus_label,
                plus_text_x,
                text_y,
                tokens.text_on(SurfaceLevel::S2).secondary,
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
            state.new_tab_hit_rect = Some((new_tab_x, new_tab_x + new_tab_w));

            // P1 (WT-like UX): the profile-dropdown pill right of `+`.
            // Clicking it opens `ContextMenu::new_tab_dropdown` (profiles +
            // WSL distros); the hit rect is consumed by `mouse.rs`.
            let dropdown_x = sw - window_buttons_w - settings_w - dropdown_w;
            add_px_rounded_rect_sdf(
                dropdown_x,
                bar_y,
                dropdown_w,
                bar_h,
                new_tab_radius,
                new_tab_bg,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            // UI/UX v3 P4a: the chevron comes from the bundled icon font and
            // centres in the pill it already occupied.
            add_icon_verts(
                crate::icons::CHEVRON_DOWN,
                dropdown_x,
                text_y,
                dropdown_w,
                cell_h,
                icon_size_for_slot(font.icon_px(16.0), dropdown_w, cell_h, 0.2),
                tokens.text_on(SurfaceLevel::S2).secondary,
                sw,
                sh,
                font,
                atlas,
                &self.queue,
                text_verts,
                text_idx,
            );
            state.new_tab_dropdown_hit_rect = Some((dropdown_x, dropdown_x + dropdown_w));
        }

        // Right edge: settings button (left of the window buttons when the
        // custom title bar is active).
        let settings_x = sw - window_buttons_w - settings_w;
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
        let settings_radius = ui_cfg.chrome_radius().min(bar_h * 0.5);
        add_px_rounded_rect_sdf(
            settings_x,
            bar_y,
            settings_w,
            bar_h,
            settings_radius,
            settings_bg,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        let settings_fg = if settings_open {
            text_fg
        } else {
            tokens.text_on(SurfaceLevel::S2).secondary
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
        state.settings_tab_rect = Some((settings_x, settings_x + settings_w));

        // Custom title bar: minimize / maximize / close buttons at the far
        // right. They sit flat on the bar (no permanent fill); the hovered
        // button gets a fill — the close button the semantic error colour,
        // Windows Terminal-style. Hit rects are re-registered every frame
        // like every other tab-bar control.
        state.window_minimize_hit_rect = None;
        state.window_maximize_hit_rect = None;
        state.window_close_hit_rect = None;
        if custom_titlebar {
            use crate::state::WindowButton;
            let radius = ui_cfg.chrome_radius().min(bar_h * 0.5);
            // UI/UX v3 P4a: caption glyphs come from the bundled icon font.
            // These were the most visible place the chrome borrowed the user's
            // terminal font, so they move with everything else (decision D-1 in
            // the phase's design spec); the shapes match the Windows 11 caption
            // set — a rule, a square, overlapping squares, a cross.
            let buttons = [
                (WindowButton::Minimize, crate::icons::WINDOW_MINIMIZE),
                (
                    WindowButton::Maximize,
                    if is_maximized {
                        crate::icons::WINDOW_RESTORE
                    } else {
                        crate::icons::WINDOW_MAXIMIZE
                    },
                ),
                (WindowButton::Close, crate::icons::WINDOW_CLOSE),
            ];
            for (i, &(button, glyph)) in buttons.iter().enumerate() {
                let bx = sw - (3 - i) as f32 * window_button_w;
                let hover = state.window_button_hover.weight(button, now);
                // UI/UX v3 P3b3: the fill is additive, so press has to raise
                // the weight before `press_fill` dims and strengthens it.
                let press = state.window_button_press.weight(button, now);
                let fill_w = hover.max(press);
                // The fill is an additive layer — absent when not hovered —
                // so its alpha carries the fade and nothing is emitted at 0.
                if fill_w > 0.0 {
                    let bg = if button == WindowButton::Close {
                        tokens.semantic_error
                    } else {
                        [
                            (inactive_bg[0] + 0.08).min(1.0),
                            (inactive_bg[1] + 0.08).min(1.0),
                            (inactive_bg[2] + 0.10).min(1.0),
                            1.0,
                        ]
                    };
                    add_px_rounded_rect_sdf(
                        bx,
                        bar_y,
                        window_button_w,
                        bar_h,
                        radius,
                        crate::color_util::press_fill([bg[0], bg[1], bg[2], bg[3] * fill_w], press),
                        sw,
                        sh,
                        bg_verts,
                        bg_idx,
                    );
                }
                // The glyph is an opaque swap between two tokens, so the
                // colour itself moves. UI/UX v3 P3b3: this reads `hover`
                // alone, never `fill_w` / `press` — see
                // `window_button_glyph_color`'s own doc comment for why.
                let fg = window_button_glyph_color(
                    hover,
                    tokens.text_on(SurfaceLevel::S2).secondary,
                    tokens.text_on(SurfaceLevel::S2).primary,
                );
                add_icon_verts(
                    glyph,
                    bx,
                    text_y,
                    window_button_w,
                    cell_h,
                    icon_size_for_slot(font.icon_px(16.0), window_button_w, cell_h, 0.2),
                    fg,
                    sw,
                    sh,
                    font,
                    atlas,
                    &self.queue,
                    text_verts,
                    text_idx,
                );
                let rect = Some((bx, bx + window_button_w));
                match button {
                    WindowButton::Minimize => state.window_minimize_hit_rect = rect,
                    WindowButton::Maximize => state.window_maximize_hit_rect = rect,
                    WindowButton::Close => state.window_close_hit_rect = rect,
                }
            }
        }

        // When renaming a tab, display an inline edit field at the tab's position.
        if let Some(rename_id) = state.settings_panel.tab_rename_editing
            && let Some(&(tx0, tx1)) = state.tab_hit_rects.get(&rename_id)
        {
            let edit_w = (tx1 - tx0).min(tab_area_w - tx0);
            // Edit field background.
            add_px_rect(
                tx0,
                bar_y,
                edit_w,
                bar_h,
                tokens.surface_3,
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
                tokens.accent_primary,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            // Text + cursor (append `|` at the end).
            // UI/UX v3 (G11): the edit text follows the scheme's foreground
            // instead of a hard-coded white, which had no contrast against the
            // pale `surface_3` fill of a light scheme.
            let edit_text = format!(" {}|", state.settings_panel.tab_rename_text);
            add_string_verts(
                &edit_text,
                tx0 + padding,
                text_y,
                tokens.text_on(SurfaceLevel::S3).primary,
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
        tokens: &nexterm_config::DesignTokens,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let py = sh - cell_h;

        // Zone 1: full-width background (surface_1) + top divider.
        add_px_rect(
            0.0,
            py,
            sw,
            cell_h,
            tokens.surface_1,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_px_rect(
            0.0,
            py,
            sw,
            1.0,
            tokens.border_subtle,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Zone 2: icon area — accent_primary at 25 % alpha behind the "N" glyph.
        let icon_zone_w = cell_w * 3.0;
        let icon_bg = {
            let [r, g, b, _] = tokens.accent_primary;
            [r, g, b, 0.25]
        };
        add_px_rect(
            0.0,
            py,
            icon_zone_w,
            cell_h,
            icon_bg,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_string_verts(
            " N ",
            0.0,
            py,
            tokens.text_on_accent,
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

        // Zone 3: pane info text, starting just after the icon zone.
        let pane_id = state.focused_pane_id.unwrap_or(0);
        let activity_ids = state.active_pane_ids();
        let pane_count = state.pane_layouts.len();
        let info = if activity_ids.is_empty() {
            format!(" nexterm │ pane {}/{}", pane_id, pane_count)
        } else {
            let ids: Vec<String> = activity_ids.iter().map(|id| id.to_string()).collect();
            format!(
                " nexterm │ pane {}/{} │ ●{}",
                pane_id,
                pane_count,
                ids.join(",")
            )
        };
        add_string_verts(
            &info,
            icon_zone_w,
            py,
            tokens.text_on(SurfaceLevel::S1).secondary,
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

        // Zone 4: left widget (status_bar_text), rendered after the info block.
        if !state.status_bar_text.is_empty() {
            let info_w = (1 + info.chars().count()) as f32 * cell_w;
            let left_x = icon_zone_w + info_w;
            let left_text = format!("│ {} ", state.status_bar_text);
            add_string_verts(
                &left_text,
                left_x,
                py,
                tokens.text_on(SurfaceLevel::S1).muted,
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

        // Zone 5 (right edge): right widget, stacked indicators.
        // Source: prefer status_bar_right_text, fall back to status_bar_text when
        // status_bar_text is not also being shown on the left.
        let right_widget_src = if !state.status_bar_right_text.is_empty() {
            &state.status_bar_right_text
        } else if state.status_bar_text.is_empty() {
            &state.status_bar_text
        } else {
            // status_bar_text is already shown on the left; don't duplicate on the right.
            ""
        };
        let right_widget_src = right_widget_src.to_owned();
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
                tokens.accent_muted,
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

        // Zoom indicator — semantic_warning colour.
        if state.is_zoomed {
            let zoom_text = " [Z] ";
            right_offset += zoom_text.chars().count() as f32 * cell_w;
            let right_px = sw - right_offset;
            add_string_verts(
                zoom_text,
                right_px,
                py,
                tokens.semantic_warning,
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

        // Copy mode indicator — accent_primary colour.
        if state.copy_mode.is_active {
            use crate::state::ViMode;
            let mode_label = match state.copy_mode.vi_mode {
                ViMode::Normal => " COPY ",
                ViMode::Visual => " VISUAL ",
                ViMode::VisualLine => " V-LINE ",
            };
            right_offset += mode_label.chars().count() as f32 * cell_w;
            let right_px = sw - right_offset;
            add_string_verts(
                mode_label,
                right_px,
                py,
                tokens.accent_primary,
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

        // Scrollback position indicator — semantic_warning colour.
        if let Some(pane) = state.focused_pane()
            && pane.scroll_offset > 0
        {
            let scroll_text = format!(" ↑{} ", pane.scroll_offset);
            let right_px = sw - scroll_text.chars().count() as f32 * cell_w - right_offset;
            add_string_verts(
                &scroll_text,
                right_px,
                py,
                tokens.semantic_warning,
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
        tokens: &nexterm_config::DesignTokens,
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

        // Background from design tokens.
        add_px_rect(
            0.0,
            py,
            sw,
            cell_h,
            tokens.surface_2,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        // Top accent line (accent_primary, 2 px).
        add_px_rect(
            0.0,
            py,
            sw,
            2.0,
            tokens.accent_primary,
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
            tokens.text_on(SurfaceLevel::S2).primary,
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

        // Key hint at the far right.
        let hint = "Enter/↑ next  Shift+Enter/↑ prev  Esc close ";
        let hint_x = sw - hint.chars().count() as f32 * cell_w;
        add_string_verts(
            hint,
            hint_x.max(0.0),
            py,
            tokens.text_on(SurfaceLevel::S2).muted,
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

    /// Build the InfoBar stack vertices (UI/UX v3 P6b).
    ///
    /// One builder for what used to be three — update, offline and server
    /// error — each of which picked its own colour, re-derived its own `y` by
    /// testing the others, and emitted its own string. The colour now comes
    /// from the kind's severity, the message from `InfoBarKind::label`, and the
    /// geometry from `infobar::bar_rects`, which is the only place a bar's `y`
    /// is decided (G-single).
    ///
    /// `now` is passed rather than read so the offline bar's elapsed count is
    /// consistent with the rest of the frame.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_info_bar_verts(
        &self,
        state: &ClientState,
        tokens: &nexterm_config::DesignTokens,
        sw: f32,
        sh: f32,
        cell_w: f32,
        cell_h: f32,
        tab_bar_h: f32,
        now: std::time::Instant,
        font: &mut FontManager,
        atlas: &mut GlyphAtlas,
        bg_verts: &mut Vec<BgVertex>,
        bg_idx: &mut Vec<u16>,
        text_verts: &mut Vec<TextVertex>,
        text_idx: &mut Vec<u16>,
    ) {
        let bars = infobar::contiguous(&state.info_bars);
        let layout = infobar::bar_rects(&bars, tab_bar_h, cell_h, sw);
        let last_visible = layout.visible.len().saturating_sub(1);

        for (slot, (index, rect)) in layout.visible.iter().copied().enumerate() {
            let bar = &bars[index];
            // UI/UX v3 P6d: each bar fades independently, so the recorded
            // range is per bar rather than around the whole builder — one bar
            // can be leaving while the one under it is still arriving.
            let (bg_start, text_start) = (bg_verts.len(), text_verts.len());
            let banner_bg = draw_banner_bg(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                bar.kind.accent(tokens),
                tokens,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            let baseline_y = rect.y + (rect.h - cell_h) * 0.5;

            // The hint is drawn only for a bar `Esc` can actually dismiss, so
            // the offline bar no longer offers a key that does nothing.
            let hint = if bar.kind.is_dismissible() {
                "  [Esc]"
            } else {
                ""
            };
            // Bars past the cap are counted rather than drawn (G-cap), and the
            // bottom bar is where the count goes — it is the edge of the stack,
            // so "there is more below this" reads in the right place.
            let more = if slot == last_visible {
                layout
                    .more_label()
                    .map(|label| format!("  {label}"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            // Truncate to the room left by the hint. The error banner already
            // did this; the other two inherit it because a long message is a
            // property of the text, not of the kind. The count is part of the
            // budget rather than appended after it, so a long message cannot
            // push it off the edge.
            let max_chars = ((sw / cell_w) as usize)
                .saturating_sub(hint.chars().count() + more.chars().count() + 4)
                .max(8);
            let label: String = bar
                .kind
                .label(now)
                .chars()
                .take(max_chars)
                .chain(more.chars())
                .collect();
            add_string_verts(
                &label,
                cell_w * 1.2,
                baseline_y,
                nexterm_config::contrast_correct(
                    tokens.text_on(SurfaceLevel::S2).primary,
                    banner_bg,
                    nexterm_config::MIN_TEXT_CONTRAST,
                ),
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

            if !hint.is_empty() {
                let hint_x = sw - hint.len() as f32 * cell_w - cell_w;
                add_string_verts(
                    hint,
                    hint_x,
                    baseline_y,
                    nexterm_config::contrast_correct(
                        tokens.text_on(SurfaceLevel::S2).muted,
                        banner_bg,
                        nexterm_config::MIN_TEXT_CONTRAST,
                    ),
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

            crate::renderer::overlay::fade::apply_surface_fade(
                &mut bg_verts[bg_start..],
                &mut text_verts[text_start..],
                bar.visibility(now),
            );
        }
    }

    /// Build the Quick Select overlay vertices.
    ///
    /// At each match position, draw a label (a, b, ..., aa, ...) over a coloured background.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_quick_select_verts(
        &self,
        state: &ClientState,
        tokens: &nexterm_config::DesignTokens,
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

        // Derive per-use colours from tokens once.
        let [ar, ag, ab, _] = tokens.accent_activity;
        let [pr, pg, pb, _] = tokens.accent_primary;
        let [s1r, s1g, s1b, _] = tokens.surface_1;
        let match_tint = [ar, ag, ab, 0.25];
        let label_bg_normal = [ar, ag, ab, 0.92];
        let label_bg_active = [pr, pg, pb, 0.95];
        let hud_bg = [s1r, s1g, s1b, 0.92];

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
                lx, ly, match_w, cell_h, match_tint, sw, sh, bg_verts, bg_idx,
            );

            // Label background: accent_primary while the user is typing a prefix,
            // accent_activity otherwise.
            let is_partial_match =
                !qs.typed_label.is_empty() && m.label.starts_with(&qs.typed_label);
            let bg_color = if is_partial_match {
                label_bg_active
            } else {
                label_bg_normal
            };
            add_px_rect(lx, ly, label_w, cell_h, bg_color, sw, sh, bg_verts, bg_idx);

            // Label text on accent background.
            add_string_verts(
                &m.label,
                lx,
                ly,
                tokens.text_on_accent,
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

        // HUD at the top of the screen showing what has been typed so far.
        let typed = format!("Quick Select: {}_", qs.typed_label);
        add_px_rect(
            0.0,
            0.0,
            typed.len() as f32 * cell_w + cell_w,
            cell_h,
            hud_bg,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_string_verts(
            &typed,
            cell_w * 0.5,
            0.0,
            tokens.accent_activity,
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

/// Window-button glyph colour (UI/UX v3 P3b3). Pure so it can be
/// unit-tested without a GPU.
///
/// Takes `hover` only, on purpose — there is no `press` parameter here to
/// merge in by accident. The fill (`fill_w = hover.max(press)`, drawn
/// separately) is allowed to move on press because it is an additive
/// layer that is absent at weight 0; a foreground glyph swap is always
/// drawn, so moving it on press would tint the glyph on every click
/// regardless of hover, which the spec forbids.
fn window_button_glyph_color(
    hover: f32,
    text_secondary: [f32; 4],
    text_primary: [f32; 4],
) -> [f32; 4] {
    crate::color_util::lerp_rgba(text_secondary, text_primary, hover)
}

/// Maps an OSC 9;4 progress record to a tab-bar indicator `(color, width
/// fraction)`. Returns `None` when no indicator should be drawn. Pure so it
/// can be unit-tested without a GPU.
///
/// State semantics (ConEmu / Windows Terminal): 1 = normal (success),
/// 2 = error, 4 = paused (warning) — all sized by the percentage;
/// 3 = indeterminate — full-width dim bar (no meaningful percentage).
/// Hues come from the scheme-derived semantic tokens (UI/UX v3 G11); the
/// alpha stays an element-specific design value.
fn progress_indicator_style(
    progress: Option<(u8, u8)>,
    tokens: &nexterm_config::DesignTokens,
) -> Option<([f32; 4], f32)> {
    let (state, percent) = progress?;
    let frac = (percent.min(100) as f32) / 100.0;
    match state {
        1 => Some((with_alpha(tokens.semantic_success, 0.90), frac)),
        2 => Some((with_alpha(tokens.semantic_error, 0.90), frac)),
        3 => Some((
            with_alpha(tokens.text_on(SurfaceLevel::S1).muted, 0.60),
            1.0,
        )),
        4 => Some((with_alpha(tokens.semantic_warning, 0.90), frac)),
        _ => None,
    }
}

#[cfg(test)]
mod window_button_glyph_color_tests {
    use super::window_button_glyph_color;
    use nexterm_config::{DesignTokens, SurfaceLevel};

    /// The regression this pins: a press must never move the window
    /// buttons' glyph colour. `window_button_glyph_color` takes only
    /// `hover`, so there is no `press` value to pass in even at full
    /// press weight — at `hover == 0.0` the result must be exactly
    /// `text_secondary`, the unhovered colour.
    #[test]
    fn hover_at_zero_keeps_the_unhovered_glyph_colour() {
        let tokens = DesignTokens::default();
        let fg = window_button_glyph_color(
            0.0,
            tokens.text_on(SurfaceLevel::S2).secondary,
            tokens.text_on(SurfaceLevel::S2).primary,
        );
        assert_eq!(fg, tokens.text_on(SurfaceLevel::S2).secondary);
    }

    #[test]
    fn hover_at_one_reaches_the_hovered_glyph_colour() {
        let tokens = DesignTokens::default();
        let fg = window_button_glyph_color(
            1.0,
            tokens.text_on(SurfaceLevel::S2).secondary,
            tokens.text_on(SurfaceLevel::S2).primary,
        );
        assert_eq!(fg, tokens.text_on(SurfaceLevel::S2).primary);
    }
}

#[cfg(test)]
mod progress_indicator_tests {
    use super::progress_indicator_style;
    use nexterm_config::{DesignTokens, SurfaceLevel};

    fn tokens() -> DesignTokens {
        DesignTokens::default()
    }

    #[test]
    fn normal_progress_scales_with_the_percentage() {
        let (color, frac) = progress_indicator_style(Some((1, 50)), &tokens()).expect("indicator");
        assert!((frac - 0.5).abs() < 1e-6);
        assert!(color[1] > color[0], "normal state is green-dominant");
    }

    #[test]
    fn error_state_is_red_and_indeterminate_is_full_width() {
        let (color, _) = progress_indicator_style(Some((2, 30)), &tokens()).expect("indicator");
        assert!(color[0] > color[1], "error state is red-dominant");
        let (_, frac) = progress_indicator_style(Some((3, 5)), &tokens()).expect("indicator");
        assert!((frac - 1.0).abs() < 1e-6, "indeterminate ignores percent");
    }

    #[test]
    fn none_and_unknown_states_draw_nothing() {
        assert!(progress_indicator_style(None, &tokens()).is_none());
        assert!(progress_indicator_style(Some((0, 50)), &tokens()).is_none());
        assert!(progress_indicator_style(Some((9, 50)), &tokens()).is_none());
    }

    #[test]
    fn overlong_percentages_are_clamped() {
        let (_, frac) = progress_indicator_style(Some((1, 250)), &tokens()).expect("indicator");
        assert!((frac - 1.0).abs() < 1e-6);
    }

    #[test]
    fn indicator_hues_come_from_the_semantic_tokens() {
        // G11: pin each OSC 9;4 state to its token so the bar follows the
        // active scheme instead of a hard-coded palette. Alpha stays an
        // element-specific design value, so only the hue is compared.
        let t = tokens();
        for (state, expected) in [
            (1u8, t.semantic_success),
            (2, t.semantic_error),
            (3, t.text_on(SurfaceLevel::S1).muted),
            (4, t.semantic_warning),
        ] {
            let (color, _) = progress_indicator_style(Some((state, 50)), &t).expect("indicator");
            assert_eq!(color[..3], expected[..3], "hue for state {state}");
        }
    }
}
