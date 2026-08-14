//! Vertex builder for the settings panel (Ctrl+,).
//!
//! Split by responsibility (Phase B2, settings-panel 2-column layout
//! overhaul):
//!   - [`mod@self`] (this file): entry point (`build_settings_panel_verts`),
//!     panel chrome (scrim/title bar/bottom bar), and the scroll/scissor
//!     merge machinery inherited from Phase B1.
//!   - [`layout`]: pure label/control column-width math shared by every
//!     category (`compute_row_layout`).
//!   - [`row`]: common row builders (section header / label+control row /
//!     wrapped description) built on top of `layout`.
//!   - [`sidebar`]: left sidebar (category search + list).
//!   - `font_tab` / `theme_tab` / `window_tab` / `startup_tab` / `ssh_tab` /
//!     `keybindings_tab` / `profiles_tab` / `blocks_tab` / `security_tab`:
//!     one file per [`crate::settings_panel::SettingsCategory`] variant.

use super::util::draw_overlay_panel;
use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::state::ClientState;
use crate::vertex_util::{add_px_rect, add_px_rounded_rect_sdf, add_string_verts};

use super::super::WgpuState;

mod blocks_tab;
mod font_tab;
mod keybindings_tab;
// `pub(super)`: the sibling `widgets` module lays its controls out on the
// same label/control column split.
pub(super) mod layout;
mod profiles_tab;
// `pub(super)` so the sibling `widgets` module can reuse the contrast helper
// (`ensure_readable`) instead of duplicating the WCAG correction.
pub(super) mod row;
mod security_tab;
mod sidebar;
mod ssh_tab;
mod startup_tab;
mod theme_tab;
mod window_tab;

/// Phase B1: layout metrics produced by [`WgpuState::build_settings_panel_verts`]
/// for the scrollable content area.
///
/// The render function only has a shared `&ClientState`, so it cannot write
/// the freshly-measured content height back into `SettingsPanel::scroll`
/// itself. Instead it returns the measurement here; the caller
/// (`render_frame.rs`, which holds `&mut ClientState`) applies it to
/// `state.settings_panel.scroll` and also uses `scissor` / the index ranges
/// to clip the scrollable content to its viewport when drawing.
pub(in crate::renderer) struct SettingsPanelScrollMetrics {
    /// Measured height of the current category's content, in pixels.
    pub content_h_px: f32,
    /// Visible height of the content viewport, in pixels.
    pub viewport_h_px: f32,
    /// Physical-pixel scissor rect `(x, y, w, h)` for the content area, or
    /// `None` when there is nothing to clip (zero-sized viewport).
    pub scissor: Option<(u32, u32, u32, u32)>,
    /// `[start, end)` range in `bg_idx` occupied by the scrollable content
    /// (as opposed to the panel chrome / sidebar, which is drawn outside
    /// this range and is never scissored).
    pub bg_range: (usize, usize),
    /// `[start, end)` range in `text_idx` occupied by the scrollable content.
    pub text_range: (usize, usize),
}

impl WgpuState {
    /// Build vertices for the settings panel (opens with Ctrl+,)
    ///
    /// Displays the panel for tab 0=Font, 1=Colors, 2=Window.
    ///
    /// Returns [`SettingsPanelScrollMetrics`] when the panel is open so the
    /// caller can feed the measured content height back into
    /// `SettingsPanel::scroll` and scissor-clip the scrollable region
    /// (Phase B1: scroll + GPU clipping).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::renderer) fn build_settings_panel_verts(
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
    ) -> Option<SettingsPanelScrollMetrics> {
        use crate::settings_panel::SettingsCategory;

        let sp = &state.settings_panel;
        if !sp.is_open {
            return None;
        }

        // Open/close animation: smoothly via ease-out cubic
        let eased = sp.eased_progress();

        // Note: the historical straight-alpha vs `PreMultiplied`-surface
        // mismatch (issue #35, "Phase B3 known issue") was fixed in UI/UX v3
        // P0 — shaders now emit premultiplied alpha and every pipeline blends
        // with `PREMULTIPLIED_ALPHA_BLENDING`. Keeping the settings panel's
        // own surfaces (scrim once visible, panel/sidebar/title/bottom-bar
        // backgrounds) fully opaque remains a deliberate readability choice
        // on translucent terminals, no longer a blending workaround.
        //
        // Full-screen scrim behind the panel, so translucent terminal
        // backgrounds (`window.background_opacity < 1.0`, Acrylic) don't
        // make the settings panel read as non-modal. `surface_0` is the
        // deepest background in the active scheme, so the scrim matches the
        // panel's color family in both light and dark themes.
        //
        // The scrim is the one part of this panel allowed to fade (it sits
        // *behind* the panel, never under any panel text), but only up to
        // a point: past `eased > 0.02` (the panel is meaningfully visible)
        // it snaps to a fixed, mostly-opaque alpha rather than continuing
        // to track `eased` all the way to 1.0. This keeps the terminal
        // behind it reliably dimmed as soon as the panel appears, instead
        // of being only faintly dimmed for most of the open animation.
        const SCRIM_ALPHA: f32 = 0.72; // >= 0.55 floor (Phase B3 mitigation)
        let scrim = tokens.surface_0;
        let scrim_alpha = if eased > 0.02 { SCRIM_ALPHA } else { eased };
        let scrim_color = [scrim[0], scrim[1], scrim[2], scrim_alpha];
        add_px_rect(0.0, 0.0, sw, sh, scrim_color, sw, sh, bg_verts, bg_idx);

        // Panel size (with left sidebar)
        let panel_w = (sw * 0.72).min(sw - cell_w * 4.0);
        let panel_h = (sh * 0.75).min(sh - cell_h * 4.0);
        let base_x = (sw - panel_w) / 2.0;
        // Slide-up: start 16px below and ease into the resting position
        let slide_offset = (1.0 - eased) * 16.0;
        let base_y = (sh - panel_h) / 2.0 + slide_offset;
        // Apply the cumulative title-bar drag offset and clamp it so the
        // panel cannot be flung off-screen. `title_h` matches the value
        // computed below; passing it here lets the clamp keep at least the
        // title bar reachable on the bottom edge.
        let title_h_for_clamp = cell_h * 1.4;
        let (px, py) = crate::settings_panel::clamp_panel_position(
            base_x,
            base_y,
            panel_w,
            panel_h,
            sw,
            sh,
            title_h_for_clamp,
            sp.drag_offset,
        );

        // Sidebar width / content area (reserve 18 cells to fit Japanese category names)
        let sidebar_w = cell_w * 18.0;
        let content_x = px + sidebar_w;
        let content_w = panel_w - sidebar_w;

        // Panel chrome: drop-shadow + border ring + rounded background via shared helper.
        draw_overlay_panel(
            px, py, panel_w, panel_h, tokens, 4.0, 6.0, sw, sh, bg_verts, bg_idx,
        );

        // Title bar (tokens.surface_3, opaque)
        let title_h = cell_h * 1.4;
        add_px_rect(
            px,
            py,
            panel_w,
            title_h,
            tokens.surface_3,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Top accent line of the title bar (3px, accent_primary)
        let ap = tokens.accent_primary;
        add_px_rect(px, py, panel_w, 3.0, ap, sw, sh, bg_verts, bg_idx);
        // Inner 1px faint glow
        add_px_rect(
            px,
            py + 3.0,
            panel_w,
            1.0,
            [ap[0], ap[1], ap[2], 0.25],
            sw,
            sh,
            bg_verts,
            bg_idx,
        );

        // Title. `text_secondary` (fg at 0.78 alpha) drawn over `surface_3`
        // falls short of the 4.5:1 contrast floor for some themes (Phase B3
        // contrast audit) — nudge it up via `ensure_readable` rather than
        // hard-coding `text_primary`, so it still tracks the scheme's own
        // secondary-text tone wherever that already clears the bar.
        let title_color = row::ensure_readable(
            tokens.text_secondary,
            tokens.surface_3,
            row::MIN_TEXT_CONTRAST,
        );
        add_string_verts(
            &nexterm_i18n::fl!("settings-panel-title"),
            px + cell_w * 0.5,
            py + cell_h * 0.2,
            title_color,
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
        // Close-button hint
        let close_text = "Esc";
        let close_x = px + panel_w - close_text.len() as f32 * cell_w - cell_w;
        add_string_verts(
            close_text,
            close_x,
            py + cell_h * 0.2,
            tokens.accent_primary,
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

        // Sidebar (background + separator + search input + category list)
        let sidebar_top = py + title_h;
        let sidebar_h = panel_h - title_h - cell_h * 1.5;
        sidebar::draw_sidebar(
            sp,
            tokens,
            px,
            sidebar_top,
            sidebar_w,
            sidebar_h,
            sw,
            sh,
            cell_w,
            cell_h,
            font,
            atlas,
            &self.queue,
            bg_verts,
            bg_idx,
            text_verts,
            text_idx,
        );

        // Metric tokens for the widget layer. The panel does not have
        // `UiConfig` plumbed through yet, and the migrated widgets only read
        // `radius.control`, which is the config-independent Fluent value — so
        // the defaults are exact here. Plumbing follows when the panel's own
        // surfaces move onto `radius.surface`.
        let metrics = nexterm_config::MetricTokens::default();

        // Content area
        let content_top = py + title_h + cell_h * 0.5;
        let content_inner_x = content_x + cell_w;
        // The content viewport ends where the footer (bottom bar) begins —
        // see the `bottom_y` computation below, which this must stay in
        // sync with.
        let viewport_bottom = py + panel_h - cell_h * 1.5;
        let viewport_h_px = (viewport_bottom - content_top).max(0.0);

        // Phase B1 (scroll + GPU scissor clipping): the per-category content
        // is generated into local buffers rather than directly into
        // `bg_verts` / `text_verts`. This lets the content be uniformly
        // scrolled (translated by `-offset_px`) and its extent measured
        // without the per-category drawing code needing to know about
        // scrolling at all.
        let offset_px = sp.scroll.offset_px;
        let mut content_bg_verts: Vec<BgVertex> = Vec::new();
        let mut content_bg_idx: Vec<u16> = Vec::new();
        let mut content_text_verts: Vec<TextVertex> = Vec::new();
        let mut content_text_idx: Vec<u16> = Vec::new();
        {
            let bg_verts = &mut content_bg_verts;
            let bg_idx = &mut content_bg_idx;
            let text_verts = &mut content_text_verts;
            let text_idx = &mut content_text_idx;

            match &sp.category {
                SettingsCategory::Font => font_tab::draw_font_tab(
                    sp,
                    tokens,
                    &metrics,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
                SettingsCategory::Theme => theme_tab::draw_theme_tab(
                    sp,
                    tokens,
                    &metrics,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
                SettingsCategory::Window => window_tab::draw_window_tab(
                    sp,
                    tokens,
                    &metrics,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
                SettingsCategory::Profiles => profiles_tab::draw_profiles_tab(
                    sp,
                    tokens,
                    &metrics,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
                SettingsCategory::Startup => startup_tab::draw_startup_tab(
                    sp,
                    tokens,
                    &metrics,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
                SettingsCategory::Ssh => ssh_tab::draw_ssh_tab(
                    sp,
                    tokens,
                    &metrics,
                    px,
                    py,
                    panel_w,
                    panel_h,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
                SettingsCategory::Keybindings => keybindings_tab::draw_keybindings_tab(
                    sp,
                    tokens,
                    px,
                    py,
                    panel_w,
                    panel_h,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
                SettingsCategory::Blocks => blocks_tab::draw_blocks_tab(
                    sp,
                    tokens,
                    &metrics,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
                SettingsCategory::Security => security_tab::draw_security_tab(
                    sp,
                    tokens,
                    &metrics,
                    content_top,
                    content_inner_x,
                    content_w,
                    sw,
                    sh,
                    cell_w,
                    cell_h,
                    font,
                    atlas,
                    &self.queue,
                    bg_verts,
                    bg_idx,
                    text_verts,
                    text_idx,
                ),
            }
        }

        // Measure the content just generated (before scrolling is applied),
        // scroll it, and merge it into the real vertex buffers as a
        // contiguous, scissor-clippable range.
        //
        // `BgVertex` / `TextVertex` positions are already NDC (see
        // `add_px_rect`'s `1.0 - py / sh * 2.0`), so the bottom-most pixel
        // extent is recovered from the *smallest* y (NDC y decreases
        // downward). Scanning both buffers means content built from either
        // rects or bare text (no background) is measured correctly.
        let min_y_ndc = content_bg_verts
            .iter()
            .map(|v| v.position[1])
            .chain(content_text_verts.iter().map(|v| v.position[1]))
            .fold(f32::INFINITY, f32::min);
        let content_h_px = if min_y_ndc.is_finite() {
            let content_bottom_px = sh * (1.0 - min_y_ndc) / 2.0;
            (content_bottom_px - content_top).max(0.0)
        } else {
            0.0
        };

        // Scroll: shift every content vertex up by `offset_px` pixels, i.e.
        // by `+2*offset_px/sh` in NDC (NDC y grows upward while pixel y
        // grows downward, so scrolling the content *down* — larger
        // `offset_px` — moves it *up* on screen).
        let delta_ndc = offset_px / sh * 2.0;
        for v in content_bg_verts.iter_mut() {
            v.position[1] += delta_ndc;
        }
        for v in content_text_verts.iter_mut() {
            v.position[1] += delta_ndc;
        }

        // Merge into the real buffers, rebasing indices onto the outer
        // vertex arrays, and remember the `[start, end)` ranges so the
        // caller can scissor exactly this span when drawing.
        let bg_range_start = bg_idx.len();
        let bg_base = bg_verts.len() as u16;
        bg_verts.extend(content_bg_verts);
        bg_idx.extend(content_bg_idx.iter().map(|i| i + bg_base));
        let bg_range_end = bg_idx.len();

        let text_range_start = text_idx.len();
        let text_base = text_verts.len() as u16;
        text_verts.extend(content_text_verts);
        text_idx.extend(content_text_idx.iter().map(|i| i + text_base));
        let text_range_end = text_idx.len();

        // UI/UX v3 P1b: tooltips draw into the *outer*, unscissored buffers so
        // one anchored to a row near the viewport edge is not clipped in half.
        // The anchor is the widget rect translated by the scroll offset.
        theme_tab::draw_theme_tooltip(
            sp,
            tokens,
            &metrics,
            content_top - offset_px,
            content_inner_x,
            content_w,
            sw,
            sh,
            cell_w,
            cell_h,
            font,
            atlas,
            &self.queue,
            bg_verts,
            bg_idx,
            text_verts,
            text_idx,
        );

        // Scrollbar: fixed to the content viewport (not scrolled with the
        // content), drawn only when the content overflows. Track spans the
        // full viewport height; thumb size/position reflect the scroll
        // ratio, with a minimum thumb height so it stays grabbable.
        let measured = crate::settings_panel::ScrollState {
            offset_px,
            content_h_px,
            viewport_h_px,
        };
        let max_offset = measured.max_offset();
        if measured.is_scrollable() && viewport_h_px > 0.0 {
            let bar_w = (cell_w * 0.35).max(3.0);
            let bar_x = px + panel_w - bar_w - cell_w * 0.25;
            add_px_rounded_rect_sdf(
                bar_x,
                content_top,
                bar_w,
                viewport_h_px,
                bar_w * 0.5,
                [
                    tokens.text_muted[0],
                    tokens.text_muted[1],
                    tokens.text_muted[2],
                    0.08,
                ],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            let thumb_h = (viewport_h_px * viewport_h_px / content_h_px).max(cell_h * 1.5);
            let thumb_travel = (viewport_h_px - thumb_h).max(0.0);
            let scroll_ratio = (offset_px / max_offset).clamp(0.0, 1.0);
            let thumb_y = content_top + thumb_travel * scroll_ratio;
            add_px_rounded_rect_sdf(
                bar_x,
                thumb_y,
                bar_w,
                thumb_h.min(viewport_h_px),
                bar_w * 0.5,
                [
                    tokens.accent_primary[0],
                    tokens.accent_primary[1],
                    tokens.accent_primary[2],
                    0.35,
                ],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }

        // Physical-pixel scissor rect for the content viewport, clamped to
        // the surface bounds (wgpu panics on an out-of-surface scissor rect).
        let scissor = {
            let x = content_x.max(0.0).min(sw);
            let y = content_top.max(0.0).min(sh);
            let w = content_w.min(sw - x).max(0.0);
            let h = viewport_h_px.min(sh - y).max(0.0);
            if w > 0.0 && h > 0.0 {
                Some((x as u32, y as u32, w as u32, h as u32))
            } else {
                None
            }
        };

        let scroll_metrics = SettingsPanelScrollMetrics {
            content_h_px,
            viewport_h_px,
            scissor,
            bg_range: (bg_range_start, bg_range_end),
            text_range: (text_range_start, text_range_end),
        };

        // Bottom bar (Save / Cancel)
        let bottom_y = py + panel_h - cell_h * 1.5;
        add_px_rect(
            px,
            bottom_y,
            panel_w,
            1.0,
            tokens.surface_1,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_px_rect(
            px,
            bottom_y + 1.0,
            panel_w,
            cell_h * 1.5 - 1.0,
            tokens.surface_0,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
        add_string_verts(
            &nexterm_i18n::fl!("settings-panel-footer-hint"),
            px + cell_w * 0.5,
            bottom_y + cell_h * 0.3,
            row::ensure_readable(tokens.text_muted, tokens.surface_0, row::MIN_TEXT_CONTRAST),
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

        // P4 (WT-like UX): right-aligned "Open config.toml" link (WT's
        // "Open JSON file" equivalent). The hit-test in
        // `settings_panel_hit.rs` mirrors this exact geometry.
        let open_label = format!("↗ {}", nexterm_i18n::fl!("settings-open-config-file"));
        let open_label_w = crate::vertex_util::visual_width(&open_label) as f32 * cell_w;
        let link_color = row::ensure_readable(
            tokens.accent_primary,
            tokens.surface_0,
            row::MIN_TEXT_CONTRAST,
        );
        add_string_verts(
            &open_label,
            px + panel_w - open_label_w - cell_w,
            bottom_y + cell_h * 0.3,
            link_color,
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

        // P2-A (WT-like UX): "reset category to defaults" link, left of the
        // open-config link. Hidden for the list-based categories (SSH /
        // Keybindings / Profiles) where a reset would delete user data.
        // The hit-test mirrors this geometry too.
        if sp.category_resettable() {
            let reset_label = format!("↺ {}", nexterm_i18n::fl!("settings-reset-category"));
            let reset_label_w = crate::vertex_util::visual_width(&reset_label) as f32 * cell_w;
            add_string_verts(
                &reset_label,
                px + panel_w - open_label_w - cell_w * 3.0 - reset_label_w,
                bottom_y + cell_h * 0.3,
                link_color,
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

        // Phase B3: the open/close animation is expressed purely through
        // `slide_offset` (position) above — there used to also be a
        // translucent "fade-in" wash drawn over the entire panel here,
        // which made the panel (and any text already drawn on it) read as
        // half-transparent for most of the open animation (worsened, at the
        // time, by the since-fixed blending mismatch noted above).
        // The panel/sidebar/title/bottom-bar backgrounds are already drawn
        // fully opaque regardless of `eased`, so removing the fade wash is
        // enough to keep the panel opaque and legible throughout the
        // animation; the slide-up motion still communicates "opening".

        Some(scroll_metrics)
    }
}
