//! Widget specs for the Theme settings category — the first tab migrated
//! onto the shared widget layer (UI/UX v3 phase P1b).
//!
//! This is the single definition of the Theme tab's geometry. The renderer
//! (`overlay/settings/theme_tab.rs`) and the mouse hit-test
//! (`event_handler/settings_panel_hit.rs`) both call [`build_theme_widgets`],
//! so the "keep both in sync" comment the old hit-test carried is gone: there
//! is only one copy to keep.
//!
//! Later tabs get sibling modules here as they are migrated.

use crate::settings_panel::SettingsPanel;

use super::spec::{WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Theme category.
pub(in crate::renderer) const THEME_CATEGORY: u8 = 2;
/// Widget index of the color-scheme cycler.
pub(in crate::renderer) const THEME_SCHEME: u8 = 0;
/// Widget index of the "follow system theme" toggle.
pub(in crate::renderer) const THEME_FOLLOW_SYSTEM: u8 = 1;
/// First widget index of the nine color-scheme swatches. The gap above the
/// row indices leaves room for future rows without renumbering the swatches.
pub(in crate::renderer) const THEME_SWATCH_BASE: u8 = 10;

/// The nine built-in schemes previewed as swatches, with the representative
/// background color drawn in each chip.
const SWATCHES: [(&str, [f32; 4]); 9] = [
    ("dark", [0.15, 0.15, 0.18, 1.0]),
    ("light", [0.95, 0.95, 0.92, 1.0]),
    ("tokyonight", [0.10, 0.10, 0.20, 1.0]),
    ("solarized", [0.00, 0.17, 0.21, 1.0]),
    ("gruvbox", [0.28, 0.26, 0.22, 1.0]),
    ("catppuccin", [0.19, 0.17, 0.23, 1.0]),
    ("dracula", [0.16, 0.13, 0.23, 1.0]),
    ("nord", [0.18, 0.20, 0.25, 1.0]),
    ("onedark", [0.16, 0.18, 0.22, 1.0]),
];

/// Display names of the swatches, in the same order as [`SWATCHES`].
pub(in crate::renderer) fn swatch_names() -> [&'static str; 9] {
    let mut out = [""; 9];
    for (i, (name, _)) in SWATCHES.iter().enumerate() {
        out[i] = name;
    }
    out
}

/// The panel geometry a tab needs to lay its widgets out, in physical pixels.
#[derive(Debug, Clone, Copy)]
pub(in crate::renderer) struct TabGeometry {
    /// Top of the category content area.
    pub content_top: f32,
    /// Left edge of the content area's inner padding.
    pub content_inner_x: f32,
    /// Width of the content area.
    pub content_w: f32,
    /// Character cell width.
    pub cell_w: f32,
    /// Character cell height.
    pub cell_h: f32,
}

/// Row height as a multiple of the cell height, matching the highlight
/// rectangle the pre-migration Theme tab drew.
const ROW_H: f32 = 1.2;
/// Vertical offset of a row box above its text baseline.
const ROW_LIFT: f32 = 0.1;
/// Left overhang of a row box past the content inner edge.
const ROW_BLEED: f32 = 0.3;

/// Y position of the color-scheme row's text.
fn scheme_row_y(g: &TabGeometry) -> f32 {
    g.content_top + g.cell_h
}

/// Y position of the swatch strip.
pub(in crate::renderer) fn swatch_y(g: &TabGeometry) -> f32 {
    g.content_top + g.cell_h * 2.5
}

/// Y position of the "follow system theme" row's text.
fn follow_row_y(g: &TabGeometry) -> f32 {
    swatch_y(g) + g.cell_h * 2.8
}

/// Horizontal pitch between swatches.
pub(in crate::renderer) fn swatch_gap(g: &TabGeometry) -> f32 {
    (g.content_w - g.cell_w * 2.0) / SWATCHES.len() as f32
}

/// Full-width row rectangle for a row whose text sits at `text_y`.
fn row_rect(g: &TabGeometry, text_y: f32) -> WidgetRect {
    WidgetRect::new(
        g.content_inner_x - g.cell_w * ROW_BLEED,
        text_y - g.cell_h * ROW_LIFT,
        (g.content_w - g.cell_w * (ROW_BLEED + 0.4)).max(0.0),
        g.cell_h * ROW_H,
    )
}

/// Build every widget of the Theme tab for this frame.
///
/// Order matters: the swatches come last so they win the hit-test where they
/// overlap their surrounding row (see `spec::hit_test`).
pub(in crate::renderer) fn build_theme_widgets(
    sp: &SettingsPanel,
    g: &TabGeometry,
) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    let focus = sp.theme_field_focus;
    // The pointer is over at most one widget of this category.
    let hovered = sp
        .hover_widget
        .filter(|h| h.category == THEME_CATEGORY)
        .map(|h| h.index);
    let mut out = Vec::with_capacity(SWATCHES.len() + 2);

    // Row 0 — color-scheme cycler.
    let y = scheme_row_y(g);
    let rect = row_rect(g, y);
    out.push(
        WidgetSpec::new(
            WidgetId::new(THEME_CATEGORY, THEME_SCHEME),
            WidgetKind::Cycle {
                value: sp.scheme_name().to_string(),
            },
            nexterm_i18n::fl!("settings-theme-label"),
            rect,
            WidgetRect::new(
                g.content_inner_x + layout.control_x_off,
                rect.y,
                layout.control_w,
                rect.h,
            ),
        )
        .focused(focus == THEME_SCHEME)
        .hovered(hovered == Some(THEME_SCHEME))
        .tooltip(nexterm_i18n::fl!("settings-theme-tip")),
    );

    // Row 1 — follow-system toggle.
    let y = follow_row_y(g);
    let rect = row_rect(g, y);
    out.push(
        WidgetSpec::new(
            WidgetId::new(THEME_CATEGORY, THEME_FOLLOW_SYSTEM),
            WidgetKind::Toggle {
                on: sp.colors_follow_system,
            },
            nexterm_i18n::fl!("settings-theme-follow-system-label"),
            rect,
            WidgetRect::new(
                g.content_inner_x + layout.control_x_off,
                rect.y,
                layout.control_w,
                rect.h,
            ),
        )
        .focused(focus == THEME_FOLLOW_SYSTEM)
        .hovered(hovered == Some(THEME_FOLLOW_SYSTEM))
        .tooltip(nexterm_i18n::fl!("settings-theme-follow-system-tip")),
    );

    // Swatches — drawn (and hit-tested) on top of the strip.
    let dot_y = swatch_y(g);
    let gap = swatch_gap(g);
    let dot_w = g.cell_w * 1.2;
    for (i, (name, color)) in SWATCHES.iter().enumerate() {
        let dot = WidgetRect::new(g.content_inner_x + i as f32 * gap, dot_y, dot_w, g.cell_h);
        out.push(
            WidgetSpec::new(
                WidgetId::new(THEME_CATEGORY, THEME_SWATCH_BASE + i as u8),
                WidgetKind::Swatch {
                    color: *color,
                    selected: sp.scheme_index == i,
                },
                *name,
                dot,
                dot,
            )
            .hovered(hovered == Some(THEME_SWATCH_BASE + i as u8))
            .tooltip(*name),
        );
    }

    out
}

/// Map a widget index back to the swatch it represents.
pub(in crate::renderer) fn swatch_index_of(id: WidgetId) -> Option<usize> {
    if id.category != THEME_CATEGORY || id.index < THEME_SWATCH_BASE {
        return None;
    }
    let i = (id.index - THEME_SWATCH_BASE) as usize;
    (i < SWATCHES.len()).then_some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> TabGeometry {
        TabGeometry {
            content_top: 100.0,
            content_inner_x: 200.0,
            content_w: 600.0,
            cell_w: 10.0,
            cell_h: 20.0,
        }
    }

    fn panel() -> SettingsPanel {
        SettingsPanel::default()
    }

    #[test]
    fn builds_two_rows_and_nine_swatches() {
        let specs = build_theme_widgets(&panel(), &geometry());
        assert_eq!(specs.len(), 11);
        let swatches = specs
            .iter()
            .filter(|s| matches!(s.kind, WidgetKind::Swatch { .. }))
            .count();
        assert_eq!(swatches, 9);
    }

    #[test]
    fn every_widget_id_is_unique() {
        let specs = build_theme_widgets(&panel(), &geometry());
        let ids: std::collections::HashSet<_> = specs.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), specs.len());
    }

    #[test]
    fn the_focused_counter_selects_exactly_one_row() {
        let mut sp = panel();
        sp.theme_field_focus = THEME_FOLLOW_SYSTEM;
        let specs = build_theme_widgets(&sp, &geometry());
        let focused: Vec<_> = specs.iter().filter(|s| s.focused).collect();
        assert_eq!(focused.len(), 1);
        assert_eq!(focused[0].id.index, THEME_FOLLOW_SYSTEM);
    }

    #[test]
    fn the_toggle_mirrors_colors_follow_system() {
        let mut sp = panel();
        sp.colors_follow_system = true;
        let specs = build_theme_widgets(&sp, &geometry());
        let toggle = specs
            .iter()
            .find(|s| s.id.index == THEME_FOLLOW_SYSTEM)
            .expect("the follow-system toggle must exist");
        assert_eq!(toggle.kind, WidgetKind::Toggle { on: true });
    }

    #[test]
    fn the_selected_swatch_matches_scheme_index() {
        let mut sp = panel();
        sp.scheme_index = 4;
        let specs = build_theme_widgets(&sp, &geometry());
        let selected: Vec<_> = specs
            .iter()
            .filter(|s| matches!(s.kind, WidgetKind::Swatch { selected: true, .. }))
            .collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(swatch_index_of(selected[0].id), Some(4));
    }

    #[test]
    fn swatch_geometry_matches_the_pre_migration_layout() {
        // Regression guard: the strip must not shift when the hit-test stops
        // computing these numbers on its own.
        let g = geometry();
        let specs = build_theme_widgets(&panel(), &g);
        let gap = (g.content_w - g.cell_w * 2.0) / 9.0;
        for i in 0..9usize {
            let s = specs
                .iter()
                .find(|s| swatch_index_of(s.id) == Some(i))
                .expect("swatch present");
            assert!((s.rect.x - (g.content_inner_x + i as f32 * gap)).abs() < 0.001);
            assert!((s.rect.y - (g.content_top + g.cell_h * 2.5)).abs() < 0.001);
            assert!((s.rect.w - g.cell_w * 1.2).abs() < 0.001);
        }
    }

    #[test]
    fn hit_testing_a_swatch_returns_that_swatch() {
        let g = geometry();
        let specs = build_theme_widgets(&panel(), &g);
        let gap = swatch_gap(&g);
        let x = g.content_inner_x + 3.0 * gap + 2.0;
        let y = swatch_y(&g) + 5.0;
        let hit = super::super::spec::hit_test(&specs, x, y).expect("swatch hit");
        assert_eq!(swatch_index_of(hit), Some(3));
    }

    #[test]
    fn hit_testing_the_scheme_row_returns_the_cycler() {
        let g = geometry();
        let specs = build_theme_widgets(&panel(), &g);
        // Well right of the swatch strip's first chip, on the scheme row.
        let hit = super::super::spec::hit_test(&specs, g.content_inner_x + 400.0, scheme_row_y(&g))
            .expect("scheme row hit");
        assert_eq!(hit.index, THEME_SCHEME);
    }

    #[test]
    fn swatch_index_of_rejects_foreign_ids() {
        assert_eq!(swatch_index_of(WidgetId::new(THEME_CATEGORY, 0)), None);
        assert_eq!(swatch_index_of(WidgetId::new(0, THEME_SWATCH_BASE)), None);
        assert_eq!(
            swatch_index_of(WidgetId::new(THEME_CATEGORY, THEME_SWATCH_BASE + 9)),
            None
        );
    }

    #[test]
    fn rows_do_not_overlap_the_swatch_strip() {
        let g = geometry();
        let specs = build_theme_widgets(&panel(), &g);
        let strip_top = swatch_y(&g);
        let strip_bottom = strip_top + g.cell_h;
        for s in specs.iter().filter(|s| swatch_index_of(s.id).is_none()) {
            let overlaps = s.rect.y < strip_bottom && s.rect.y + s.rect.h > strip_top;
            assert!(!overlaps, "row {:?} overlaps the swatch strip", s.id);
        }
    }

    #[test]
    fn swatch_names_match_the_swatch_count() {
        assert_eq!(swatch_names().len(), SWATCHES.len());
        assert_eq!(swatch_names()[0], "dark");
    }
}
