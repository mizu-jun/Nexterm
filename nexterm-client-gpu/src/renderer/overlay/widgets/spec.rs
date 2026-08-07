//! `WidgetSpec` — the immediate-mode descriptor every chrome control is
//! rebuilt from, once per frame.
//!
//! Before this module a settings row was defined three times over: once in the
//! per-tab draw code, once in `settings_panel_hit.rs` (whose comments had to
//! say "keep both in sync"), and once in `accessibility.rs`. A `WidgetSpec`
//! carries everything all three readers need, so the geometry is computed in
//! exactly one place and the other readers consume it.
//!
//! Readers:
//! 1. [`super::draw::draw_widget`] — visuals.
//! 2. [`hit_test`] — mouse routing.
//! 3. The AccessKit tree builder — wired up in a follow-up, from the same
//!    label / kind / focus data the other two already read.
//!
//! Everything in this file is pure: no GPU handles, no font state. That keeps
//! layout and hit-testing fully unit-testable.

/// An axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(in crate::renderer) struct WidgetRect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl WidgetRect {
    /// Build a rectangle.
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Whether `(px, py)` lies inside the rectangle (edges inclusive).
    ///
    /// A rectangle with a non-positive extent contains nothing, so a
    /// collapsed or not-yet-measured widget can never swallow a click.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        self.w > 0.0
            && self.h > 0.0
            && px >= self.x
            && px <= self.x + self.w
            && py >= self.y
            && py <= self.y + self.h
    }

    /// Centre point.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// Stable identity of a widget, unique across the whole settings panel.
///
/// `category` is the `SettingsCategory::ALL` index the widget belongs to and
/// `index` is its position within that category. The pair is stable across
/// frames even when rows are collapsed by the search filter, which is what
/// lets focus survive a re-layout — and what will let the seven per-tab
/// focus counters collapse into a single `focused_widget_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::renderer) struct WidgetId {
    /// Owning settings category.
    pub category: u8,
    /// Position within the category.
    pub index: u8,
}

impl WidgetId {
    /// Build an id.
    pub fn new(category: u8, index: u8) -> Self {
        Self { category, index }
    }
}

/// What a widget is, and the value it currently shows.
///
/// The payload is owned rather than borrowed: specs are rebuilt every frame
/// and the values are short display strings, so the allocation is not worth
/// designing around.
///
/// The enum is deliberately complete rather than grown tab by tab: it is the
/// vocabulary the whole layer is written against, and `draw_widget` already
/// paints every arm. `Label`, `Slider` and `Text` have no producer until the
/// Window tab migrates.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(in crate::renderer) enum WidgetKind {
    /// Non-interactive text row (section headers, notes).
    Label,
    /// Boolean, drawn as a Fluent pill switch.
    Toggle {
        /// Current state.
        on: bool,
    },
    /// Discrete options stepped with ←/→, drawn with chevron affordances.
    Cycle {
        /// Display text of the current option.
        value: String,
    },
    /// Continuous value, drawn as a track with a thumb.
    Slider {
        /// Normalised position in `[0, 1]`.
        fraction: f32,
        /// Human-readable value shown next to the track.
        display: String,
    },
    /// Free-text field.
    Text {
        /// Current content.
        value: String,
        /// Whether the field is being edited (shows a caret).
        editing: bool,
    },
    /// A colour swatch, e.g. a theme preview dot.
    Swatch {
        /// Fill colour.
        color: [f32; 4],
        /// Whether this swatch is the active choice.
        selected: bool,
    },
}

impl WidgetKind {
    /// Whether this kind reacts to input at all.
    pub fn is_interactive(&self) -> bool {
        !matches!(self, WidgetKind::Label)
    }
}

/// One control, fully described for this frame.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::renderer) struct WidgetSpec {
    /// Stable identity.
    pub id: WidgetId,
    /// Kind and current value.
    pub kind: WidgetKind,
    /// Localised label shown in the left column.
    pub label: String,
    /// Full row rectangle — the focus highlight and the hit region.
    pub rect: WidgetRect,
    /// The interactive control itself, inside `rect`.
    pub control_rect: WidgetRect,
    /// Whether keyboard focus is on this widget.
    pub focused: bool,
    /// Whether the pointer is over this widget.
    pub hovered: bool,
    /// Whether the widget accepts input right now.
    pub enabled: bool,
    /// Optional tooltip text, shown after a hover dwell.
    pub tooltip: Option<String>,
}

impl WidgetSpec {
    /// Build a spec with the common defaults (enabled, unfocused, no tooltip).
    pub fn new(
        id: WidgetId,
        kind: WidgetKind,
        label: impl Into<String>,
        rect: WidgetRect,
        control_rect: WidgetRect,
    ) -> Self {
        Self {
            id,
            kind,
            label: label.into(),
            rect,
            control_rect,
            focused: false,
            hovered: false,
            enabled: true,
            tooltip: None,
        }
    }

    /// Mark the widget focused.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Mark the widget hovered.
    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Attach a tooltip.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
}

/// Find the widget under `(px, py)`.
///
/// Disabled and non-interactive widgets are skipped so a section header can
/// never steal a click from the row beneath it. Later specs win on overlap,
/// matching painter's order: a swatch drawn on top of its row is hit first.
pub(in crate::renderer) fn hit_test(specs: &[WidgetSpec], px: f32, py: f32) -> Option<WidgetId> {
    specs
        .iter()
        .rev()
        .find(|s| s.enabled && s.kind.is_interactive() && s.rect.contains(px, py))
        .map(|s| s.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(index: u8, kind: WidgetKind, y: f32) -> WidgetSpec {
        WidgetSpec::new(
            WidgetId::new(1, index),
            kind,
            format!("row {index}"),
            WidgetRect::new(0.0, y, 200.0, 20.0),
            WidgetRect::new(100.0, y, 100.0, 20.0),
        )
    }

    fn toggle(index: u8, y: f32) -> WidgetSpec {
        row(index, WidgetKind::Toggle { on: false }, y)
    }

    #[test]
    fn rect_contains_includes_edges() {
        let r = WidgetRect::new(10.0, 20.0, 100.0, 30.0);
        assert!(r.contains(10.0, 20.0));
        assert!(r.contains(110.0, 50.0));
        assert!(r.contains(60.0, 35.0));
        assert!(!r.contains(9.9, 35.0));
        assert!(!r.contains(60.0, 50.1));
    }

    #[test]
    fn collapsed_rect_contains_nothing() {
        // A search-collapsed row must not swallow clicks.
        let r = WidgetRect::new(10.0, 20.0, 0.0, 30.0);
        assert!(!r.contains(10.0, 25.0));
        let r = WidgetRect::new(10.0, 20.0, 100.0, 0.0);
        assert!(!r.contains(50.0, 20.0));
    }

    #[test]
    fn rect_center_is_the_midpoint() {
        let (cx, cy) = WidgetRect::new(10.0, 20.0, 100.0, 30.0).center();
        assert_eq!((cx, cy), (60.0, 35.0));
    }

    #[test]
    fn labels_are_not_interactive() {
        assert!(!WidgetKind::Label.is_interactive());
        assert!(WidgetKind::Toggle { on: true }.is_interactive());
        assert!(
            WidgetKind::Swatch {
                color: [0.0; 4],
                selected: false
            }
            .is_interactive()
        );
    }

    #[test]
    fn hit_test_finds_the_row_under_the_cursor() {
        let specs = vec![toggle(0, 0.0), toggle(1, 20.0), toggle(2, 40.0)];
        assert_eq!(hit_test(&specs, 50.0, 25.0), Some(WidgetId::new(1, 1)));
        assert_eq!(hit_test(&specs, 50.0, 45.0), Some(WidgetId::new(1, 2)));
        assert_eq!(hit_test(&specs, 50.0, 500.0), None);
        assert_eq!(hit_test(&specs, 500.0, 5.0), None);
    }

    #[test]
    fn hit_test_skips_labels_and_disabled_widgets() {
        let mut specs = vec![row(0, WidgetKind::Label, 0.0), toggle(1, 20.0)];
        specs[1].enabled = false;
        assert_eq!(hit_test(&specs, 50.0, 5.0), None);
        assert_eq!(hit_test(&specs, 50.0, 25.0), None);
    }

    #[test]
    fn hit_test_prefers_the_widget_drawn_last_on_overlap() {
        // A swatch drawn on top of its row must win the click.
        let mut swatch = row(
            9,
            WidgetKind::Swatch {
                color: [1.0; 4],
                selected: false,
            },
            0.0,
        );
        swatch.rect = WidgetRect::new(20.0, 0.0, 10.0, 20.0);
        let specs = vec![toggle(0, 0.0), swatch];
        assert_eq!(hit_test(&specs, 25.0, 10.0), Some(WidgetId::new(1, 9)));
        assert_eq!(hit_test(&specs, 60.0, 10.0), Some(WidgetId::new(1, 0)));
    }

    #[test]
    fn builder_methods_set_the_interaction_flags() {
        let w = toggle(0, 0.0).focused(true).hovered(true).tooltip("hint");
        assert!(w.focused);
        assert!(w.hovered);
        assert_eq!(w.tooltip.as_deref(), Some("hint"));
    }
}
