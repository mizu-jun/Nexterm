//! Widget specs for the Theme settings category — the first tab migrated
//! onto the shared widget layer (UI/UX v3 phase P1b).
//!
//! This is the single definition of the Theme tab. [`theme_widget_descs`] is
//! the semantic list the AccessKit tree is built from;
//! [`build_theme_widgets`] is that list plus geometry, shared by the renderer
//! (`overlay/settings/theme_tab.rs`) and the mouse hit-test
//! (`event_handler/settings_panel_hit.rs`). The "keep both in sync" comment
//! the old hit-test carried is gone: there is only one copy to keep.
//!
//! Later tabs get sibling modules here as they are migrated.

use crate::settings_panel::SettingsPanel;

use super::action::WidgetAction;
use super::geometry::TabGeometry;
use super::spec::{WidgetDesc, WidgetId, WidgetKind, WidgetRect, WidgetSpec};

/// `SettingsCategory::ALL` index of the Theme category.
pub(crate) const THEME_CATEGORY: u8 = 2;
/// Widget index of the color-scheme cycler.
pub(crate) const THEME_SCHEME: u16 = 0;
/// Widget index of the "follow system theme" toggle.
pub(crate) const THEME_FOLLOW_SYSTEM: u16 = 1;
/// First widget index of the nine color-scheme swatches. The gap above the
/// row indices leaves room for future rows without renumbering the swatches.
pub(crate) const THEME_SWATCH_BASE: u16 = 10;

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
pub(crate) fn swatch_names() -> [&'static str; 9] {
    let mut out = [""; 9];
    for (i, (name, _)) in SWATCHES.iter().enumerate() {
        out[i] = name;
    }
    out
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
pub(crate) fn swatch_y(g: &TabGeometry) -> f32 {
    g.content_top + g.cell_h * 2.5
}

/// Y position of the "follow system theme" row's text.
fn follow_row_y(g: &TabGeometry) -> f32 {
    swatch_y(g) + g.cell_h * 2.8
}

/// Horizontal pitch between swatches.
pub(crate) fn swatch_gap(g: &TabGeometry) -> f32 {
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

/// Describe every control of the Theme tab, without laying it out.
///
/// This is the tab's single semantic definition: the AccessKit tree builder
/// consumes it directly, and [`build_theme_widgets`] is this list plus
/// geometry.
///
/// Order matters: the swatches come last so they win the hit-test where they
/// overlap their surrounding row (see `spec::hit_test`).
pub(crate) fn theme_widget_descs(sp: &SettingsPanel) -> Vec<WidgetDesc> {
    let focus = sp.focused_widget_index;
    let mut out = Vec::with_capacity(SWATCHES.len() + 2);

    out.push(
        WidgetDesc::new(
            WidgetId::new(THEME_CATEGORY, THEME_SCHEME),
            WidgetKind::Cycle {
                value: sp.scheme_name().to_string(),
            },
            nexterm_i18n::fl!("settings-theme-label"),
        )
        .focused(focus == THEME_SCHEME)
        .tooltip(nexterm_i18n::fl!("settings-theme-tip")),
    );

    out.push(
        WidgetDesc::new(
            WidgetId::new(THEME_CATEGORY, THEME_FOLLOW_SYSTEM),
            WidgetKind::Toggle {
                on: sp.colors_follow_system,
            },
            nexterm_i18n::fl!("settings-theme-follow-system-label"),
        )
        .focused(focus == THEME_FOLLOW_SYSTEM)
        .tooltip(nexterm_i18n::fl!("settings-theme-follow-system-tip")),
    );

    for (i, (name, color)) in SWATCHES.iter().enumerate() {
        out.push(
            WidgetDesc::new(
                WidgetId::new(THEME_CATEGORY, THEME_SWATCH_BASE + i as u16),
                WidgetKind::Swatch {
                    color: *color,
                    selected: sp.scheme_index == i,
                },
                *name,
            )
            .tooltip(*name),
        );
    }

    out
}

/// Lay the Theme tab out for this frame.
pub(crate) fn build_theme_widgets(sp: &SettingsPanel, g: &TabGeometry) -> Vec<WidgetSpec> {
    let layout = super::super::settings::layout::compute_row_layout(g.content_w, g.cell_w);
    // The pointer is over at most one widget of this category.
    let hovered = sp
        .hover_widget
        .filter(|h| h.category == THEME_CATEGORY)
        .map(|h| h.index);

    let row = |text_y: f32| {
        let rect = row_rect(g, text_y);
        let control = WidgetRect::new(
            g.content_inner_x + layout.control_x_off,
            rect.y,
            layout.control_w,
            rect.h,
        );
        (rect, control)
    };
    let dot_y = swatch_y(g);
    let gap = swatch_gap(g);
    let dot_w = g.cell_w * 1.2;

    theme_widget_descs(sp)
        .into_iter()
        .map(|desc| {
            let matched = sp.label_matches_search(&desc.label);
            let desc = desc.search_match(matched);
            let index = desc.id.index;
            let (rect, control) = if let Some(i) = swatch_index_of(desc.id) {
                let dot =
                    WidgetRect::new(g.content_inner_x + i as f32 * gap, dot_y, dot_w, g.cell_h);
                (dot, dot)
            } else if index == THEME_FOLLOW_SYSTEM {
                row(follow_row_y(g))
            } else {
                row(scheme_row_y(g))
            };
            desc.place(rect, control).hovered(hovered == Some(index))
        })
        .collect()
}

/// Apply `action` to the Theme widget at `index`.
///
/// Returns whether anything changed, matching the convention
/// `dispatch_settings_action` uses. This is the same state transition the
/// mouse and keyboard paths perform, so a screen reader and a click stay in
/// agreement.
pub(crate) fn apply_theme_action(sp: &mut SettingsPanel, index: u16, action: WidgetAction) -> bool {
    // Nothing in this tab is numeric or typed.
    if matches!(action, WidgetAction::SetValue(_) | WidgetAction::SetText(_)) {
        return false;
    }
    if let Some(i) = swatch_index_of(WidgetId::new(THEME_CATEGORY, index)) {
        // A swatch is a choice, not a stepper: every action selects it.
        sp.scheme_index = i;
        sp.dirty = true;
        sp.theme_hover_preview = None;
        return true;
    }
    match (index, action) {
        (THEME_SCHEME, WidgetAction::Activate | WidgetAction::Next) => {
            sp.next_scheme();
            true
        }
        (THEME_SCHEME, WidgetAction::Prev) => {
            sp.prev_scheme();
            true
        }
        // A toggle has two states, so stepping either way flips it — the same
        // behaviour the Left/Right keys already have. A numeric SetValue is
        // meaningless on a toggle, so it is refused rather than treated as a
        // flip.
        (THEME_FOLLOW_SYSTEM, _) => {
            sp.toggle_colors_follow_system();
            true
        }
        _ => false,
    }
}

/// Map a widget index back to the swatch it represents.
pub(crate) fn swatch_index_of(id: WidgetId) -> Option<usize> {
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
            .filter(|s| matches!(s.kind(), WidgetKind::Swatch { .. }))
            .count();
        assert_eq!(swatches, 9);
    }

    #[test]
    fn every_widget_id_is_unique() {
        let specs = build_theme_widgets(&panel(), &geometry());
        let ids: std::collections::HashSet<_> = specs.iter().map(|s| s.id()).collect();
        assert_eq!(ids.len(), specs.len());
    }

    #[test]
    fn the_focused_counter_selects_exactly_one_row() {
        let mut sp = panel();
        sp.focused_widget_index = THEME_FOLLOW_SYSTEM;
        let specs = build_theme_widgets(&sp, &geometry());
        let focused: Vec<_> = specs.iter().filter(|s| s.focused()).collect();
        assert_eq!(focused.len(), 1);
        assert_eq!(focused[0].id().index, THEME_FOLLOW_SYSTEM);
    }

    #[test]
    fn the_toggle_mirrors_colors_follow_system() {
        let mut sp = panel();
        sp.colors_follow_system = true;
        let specs = build_theme_widgets(&sp, &geometry());
        let toggle = specs
            .iter()
            .find(|s| s.id().index == THEME_FOLLOW_SYSTEM)
            .expect("the follow-system toggle must exist");
        assert_eq!(*toggle.kind(), WidgetKind::Toggle { on: true });
    }

    #[test]
    fn the_selected_swatch_matches_scheme_index() {
        let mut sp = panel();
        sp.scheme_index = 4;
        let specs = build_theme_widgets(&sp, &geometry());
        let selected: Vec<_> = specs
            .iter()
            .filter(|s| matches!(s.kind(), WidgetKind::Swatch { selected: true, .. }))
            .collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(swatch_index_of(selected[0].id()), Some(4));
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
                .find(|s| swatch_index_of(s.id()) == Some(i))
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
        for s in specs.iter().filter(|s| swatch_index_of(s.id()).is_none()) {
            let overlaps = s.rect.y < strip_bottom && s.rect.y + s.rect.h > strip_top;
            assert!(!overlaps, "row {:?} overlaps the swatch strip", s.id());
        }
    }

    #[test]
    fn swatch_names_match_the_swatch_count() {
        assert_eq!(swatch_names().len(), SWATCHES.len());
        assert_eq!(swatch_names()[0], "dark");
    }
}
