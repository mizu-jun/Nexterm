//! Left sidebar: category list + search input.

use crate::font::FontManager;
use crate::glyph_atlas::{BgVertex, GlyphAtlas, TextVertex};
use crate::settings_panel::SettingsPanel;
use crate::vertex_util::{
    add_icon_verts, add_px_rect, add_run_verts, add_string_verts, icon_size_for_slot,
    truncate_run_to_width, truncate_to_width,
};

/// Draw the sidebar background, separator, category-search input, and the
/// (possibly filtered) category list.
///
/// Sidebar width is kept as a fixed `cell_w * 18.0` (wide enough to fit the
/// longest translated category names); category labels are still truncated
/// defensively via [`truncate_to_width`] in case a future locale overflows it.
#[allow(clippy::too_many_arguments)]
pub(in crate::renderer) fn draw_sidebar(
    sp: &SettingsPanel,
    tokens: &nexterm_config::DesignTokens,
    metrics: &nexterm_config::MetricTokens,
    px: f32,
    sidebar_top: f32,
    sidebar_w: f32,
    sidebar_h: f32,
    sw: f32,
    sh: f32,
    cell_w: f32,
    cell_h: f32,
    font: &mut FontManager,
    atlas: &mut GlyphAtlas,
    queue: &wgpu::Queue,
    bg_verts: &mut Vec<BgVertex>,
    bg_idx: &mut Vec<u16>,
    text_verts: &mut Vec<TextVertex>,
    text_idx: &mut Vec<u16>,
) {
    // Sidebar background (tokens.surface_1, slightly darker than the panel)
    add_px_rect(
        px,
        sidebar_top,
        sidebar_w,
        sidebar_h,
        tokens.surface_1,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // Sidebar separator (faint accent color)
    let ap = tokens.accent_primary;
    add_px_rect(
        px + sidebar_w,
        sidebar_top,
        1.0,
        sidebar_h,
        [ap[0], ap[1], ap[2], 0.30],
        sw,
        sh,
        bg_verts,
        bg_idx,
    );

    // Category-search input box. Reserved at the top of the sidebar; pushes
    // the category list down by `search_h`. The hit-test in
    // `settings_panel_hit.rs` mirrors the same offset.
    let search_h = cell_h * 1.6;
    let search_pad = cell_w * 0.5;
    let search_box_y = sidebar_top + cell_h * 0.2;
    let search_box_h = cell_h * 1.1;
    let box_bg = if sp.search_focused {
        tokens.surface_2
    } else {
        tokens.surface_0
    };
    add_px_rect(
        px + search_pad,
        search_box_y,
        sidebar_w - search_pad * 2.0,
        search_box_h,
        box_bg,
        sw,
        sh,
        bg_verts,
        bg_idx,
    );
    if sp.search_focused {
        add_px_rect(
            px + search_pad,
            search_box_y + search_box_h - 2.0,
            sidebar_w - search_pad * 2.0,
            2.0,
            ap,
            sw,
            sh,
            bg_verts,
            bg_idx,
        );
    }
    // Search query / placeholder text. `/` is the activation hotkey;
    // surface it in the placeholder so the affordance is discoverable.
    let (search_text, search_fg) = if sp.search_query.is_empty() {
        if sp.search_focused {
            (
                nexterm_i18n::fl!("settings-search-placeholder-typing"),
                tokens.text_secondary,
            )
        } else {
            (
                nexterm_i18n::fl!("settings-search-placeholder-idle"),
                tokens.text_secondary,
            )
        }
    } else {
        let cursor = if sp.search_focused { "|" } else { "" };
        (
            format!("/ {}{}", sp.search_query, cursor),
            tokens.text_primary,
        )
    };
    let search_max_w = sidebar_w - search_pad * 2.0 - cell_w * 0.3;
    let search_text = truncate_to_width(&search_text, search_max_w, cell_w);
    add_string_verts(
        &search_text,
        px + search_pad + cell_w * 0.3,
        search_box_y + cell_h * 0.05,
        search_fg,
        false,
        sw,
        sh,
        cell_w,
        font,
        atlas,
        queue,
        text_verts,
        text_idx,
    );

    // Category list (rendered below the search input)
    let categories_top = sidebar_top + search_h;
    let cat_item_h = cell_h * 1.3;
    let label_max_w = sidebar_w - cell_w * 1.0;
    let visible_categories = sp.filtered_categories();
    for (i, cat) in visible_categories.iter().enumerate() {
        let item_y = categories_top + i as f32 * cat_item_h + cell_h * 0.3;
        let is_active = &sp.category == cat;
        if is_active {
            // Active item: token-driven selection background
            add_px_rect(
                px,
                item_y - cell_h * 0.15,
                sidebar_w,
                cat_item_h,
                tokens.tab_active_bg,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            // Left-edge indicator (3px + faint inner 1px)
            add_px_rect(
                px,
                item_y - cell_h * 0.15,
                3.0,
                cat_item_h,
                ap,
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
            add_px_rect(
                px + 3.0,
                item_y - cell_h * 0.15,
                1.0,
                cat_item_h,
                [ap[0], ap[1], ap[2], 0.35],
                sw,
                sh,
                bg_verts,
                bg_idx,
            );
        }
        // When a search query is active, append a `(N)` hit count derived
        // from `category_fields` so users can see at a glance which
        // category their field-level query landed in.
        let label = if sp.is_search_filtering() {
            let n = sp.field_hit_count(cat);
            if n > 0 {
                format!("{} ({})", cat.label(), n)
            } else {
                cat.label()
            }
        } else {
            cat.label()
        };
        let fg = if is_active {
            tokens.text_primary
        } else {
            tokens.text_secondary
        };
        // UI/UX v3 P4a: the icon is drawn from the bundled icon font into a
        // fixed leading column rather than concatenated into the label. The
        // column is sized off the row, not off the glyph, so no icon can push
        // the label sideways — the old string form gave `Aa` two cells and
        // every other icon one, which is why the labels never quite lined up.
        let icon_slot_x = px + cell_w * 0.5;
        let icon_slot_w = cell_w * 2.0;
        let icon_size = icon_size_for_slot(font.icon_px(16.0), icon_slot_w, cell_h, 0.15);
        add_icon_verts(
            cat.icon(),
            icon_slot_x,
            item_y,
            icon_slot_w,
            cell_h,
            icon_size,
            fg,
            sw,
            sh,
            font,
            atlas,
            queue,
            text_verts,
            text_idx,
        );
        // UI/UX v3 P4b: Body, or Body Strong for the active category — the
        // same distinction the cell path drew with its bold flag.
        let style = if is_active {
            metrics.type_ramp.body_strong
        } else {
            metrics.type_ramp.body
        };
        let text_x = icon_slot_x + icon_slot_w + cell_w * 0.5;
        let label = truncate_run_to_width(&label, &style, label_max_w - (text_x - px), font);
        let (_size, line_h, _bold) = font.chrome_metrics(&style);
        add_run_verts(
            &label,
            &style,
            text_x,
            item_y + (cell_h - line_h) * 0.5,
            fg,
            sw,
            sh,
            font,
            atlas,
            queue,
            text_verts,
            text_idx,
        );
    }
}
