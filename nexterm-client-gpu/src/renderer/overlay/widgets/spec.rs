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
//! 3. The AccessKit tree builder, via [`WidgetDesc`] — the semantic half,
//!    which carries no geometry because a screen reader needs none.
//!
//! Everything in this file is pure: no GPU handles, no font state. That keeps
//! layout and hit-testing fully unit-testable.

/// An axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct WidgetRect {
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
///
/// `index` is 16-bit rather than 8-bit because the list-shaped categories
/// (Ssh, Keybindings, Profiles) index one widget per list entry, and those
/// lists are user-populated with no upper bound. A `u8` would silently stop
/// addressing entries past the 256th with no diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WidgetId {
    /// Owning settings category.
    pub category: u8,
    /// Position within the category.
    pub index: u16,
}

impl WidgetId {
    /// Build an id.
    pub fn new(category: u8, index: u16) -> Self {
        Self { category, index }
    }

    /// Flatten to a single integer, used as the AccessKit `NodeId` offset.
    ///
    /// Injective, so two widgets can never collide on one node id. The widest
    /// value is `0xFF_FFFF`, well inside the 100M-wide node-id range reserved
    /// for `SETTINGS_WIDGET_BASE`.
    pub fn as_u32(&self) -> u32 {
        ((self.category as u32) << 16) | self.index as u32
    }

    /// Inverse of [`Self::as_u32`].
    ///
    /// Returns `None` when the value carries bits above the packed category
    /// byte, i.e. it was never produced by `as_u32`.
    pub fn from_u32(raw: u32) -> Option<Self> {
        if raw > 0xFF_FFFF {
            return None;
        }
        Some(Self::new((raw >> 16) as u8, (raw & 0xFFFF) as u16))
    }
}

/// What a widget is, and the value it currently shows.
///
/// The payload is owned rather than borrowed: specs are rebuilt every frame
/// and the values are short display strings, so the allocation is not worth
/// designing around.
///
/// The enum is deliberately complete rather than grown tab by tab: it is the
/// vocabulary the whole layer is written against. Every arm now has both a
/// producer (some `settings_<tab>.rs`) and a painter in `draw_widget`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WidgetKind {
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
    ///
    /// The real value and range are carried rather than a pre-normalised
    /// fraction, because a screen reader announces the actual number and
    /// steps by `step`. The drawing code derives the fraction via
    /// [`WidgetKind::slider_fraction`].
    Slider {
        /// Current value.
        value: f32,
        /// Lowest value the control accepts.
        min: f32,
        /// Highest value the control accepts.
        max: f32,
        /// Increment applied by one step.
        step: f32,
        /// Human-readable value shown next to the track.
        display: String,
    },
    /// Free-text field.
    Text {
        /// Current content.
        value: String,
        /// Whether the field is being edited (shows a caret).
        editing: bool,
        /// Caret position as a byte offset into `value`, when editing.
        ///
        /// Carried rather than assumed to be end-of-string because the edit
        /// buffers behind these fields (`TextInputState`) support Home/End and
        /// arrow-key movement, and place IME preedit text at the caret. A
        /// `None` while editing means "put it at the end".
        caret: Option<usize>,
    },
    /// Discrete numeric value stepped with ←/→, e.g. the Ssh port.
    ///
    /// Drawn like a [`WidgetKind::Cycle`] (`< value >`), but announced as a
    /// spin button so a screen reader exposes the numeric range and can set
    /// the value directly — a cycler's ComboBox role would lose both.
    SpinButton {
        /// Current value.
        value: f32,
        /// Lowest value the control accepts.
        min: f32,
        /// Highest value the control accepts.
        max: f32,
        /// Increment applied by one step.
        step: f32,
        /// Human-readable value shown between the chevrons.
        display: String,
    },
    /// Key-combination capture, as used by the Keybindings tab.
    ///
    /// Distinct from [`WidgetKind::Text`] because recording is not text entry:
    /// the control swallows the next key press instead of appending it, and a
    /// screen reader has to announce that difference.
    KeyCapture {
        /// Current combination, e.g. `ctrl+shift+p`.
        value: String,
        /// Whether the control is waiting for a key press.
        recording: bool,
    },
    /// A colour swatch, e.g. a theme preview dot.
    Swatch {
        /// Fill colour.
        color: [f32; 4],
        /// Whether this swatch is the active choice.
        selected: bool,
    },
    /// One entry of a list, spanning the row rather than sitting in a control
    /// column. Selection is list state, not focus: the keyboard can rest on a
    /// different row than the one that is selected.
    ListItem {
        /// Whether this entry is the list's current selection.
        selected: bool,
    },
    /// A push button, e.g. the Add / Delete pair under a list.
    ///
    /// Carries no value; activating it is the whole interaction. A disabled
    /// button (no entry to delete) is expressed with `WidgetDesc::enabled`.
    Button {
        /// Whether the action destroys data, which the visuals warn about.
        destructive: bool,
    },
}

impl WidgetKind {
    /// Whether this kind reacts to input at all.
    pub fn is_interactive(&self) -> bool {
        !matches!(self, WidgetKind::Label)
    }

    /// Position of a slider's value within its range, normalised to `[0, 1]`.
    ///
    /// Returns 0 for every other kind, and for a degenerate range, so the
    /// thumb never lands off the track.
    pub fn slider_fraction(&self) -> f32 {
        match self {
            WidgetKind::Slider {
                value, min, max, ..
            } if max > min => ((value - min) / (max - min)).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// A control's semantics, independent of where it is drawn.
///
/// This is what the AccessKit tree builder consumes: a screen reader needs
/// identity, role, label, value and state — never pixels. [`WidgetSpec`] is
/// this plus the layout for one frame, so the two can never describe
/// different sets of controls.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WidgetDesc {
    /// Stable identity.
    pub id: WidgetId,
    /// Kind and current value.
    pub kind: WidgetKind,
    /// Localised label shown in the left column and announced by a reader.
    pub label: String,
    /// Whether keyboard focus is on this widget.
    pub focused: bool,
    /// Whether the widget accepts input right now.
    pub enabled: bool,
    /// Whether the current value fails validation — a configured value outside
    /// the set the control accepts, e.g. a typo'd keybinding action. The row
    /// stays interactive (so the value can be corrected) but is drawn in the
    /// error colour and announced as invalid, which is what makes a bad value
    /// visible without opening the file it came from.
    pub invalid: bool,
    /// Whether this row's label matches the active sidebar search query.
    /// Matching rows are drawn in the accent colour so they stand out while
    /// the list is being filtered.
    pub search_match: bool,
    /// Optional hint, shown as a tooltip and announced as a description.
    pub tooltip: Option<String>,
}

impl WidgetDesc {
    /// Describe a widget with the common defaults (enabled, unfocused, no
    /// tooltip).
    pub fn new(id: WidgetId, kind: WidgetKind, label: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            label: label.into(),
            focused: false,
            enabled: true,
            invalid: false,
            search_match: false,
            tooltip: None,
        }
    }

    /// Mark the current value as failing validation.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Mark the row as matching the active search query.
    pub fn search_match(mut self, matched: bool) -> Self {
        self.search_match = matched;
        self
    }

    /// Mark the widget focused.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Attach a hint.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// The current value as a plain string, for readers and for search.
    ///
    /// Returns `None` for kinds that carry no value of their own.
    pub fn value_text(&self) -> Option<String> {
        match &self.kind {
            WidgetKind::Label => None,
            WidgetKind::Toggle { on } => Some(if *on { "on" } else { "off" }.to_string()),
            WidgetKind::Cycle { value } => Some(value.clone()),
            WidgetKind::Slider { display, .. } => Some(display.clone()),
            WidgetKind::SpinButton { display, .. } => Some(display.clone()),
            WidgetKind::Text { value, .. } => Some(value.clone()),
            WidgetKind::KeyCapture { value, .. } => Some(value.clone()),
            WidgetKind::Swatch { selected, .. } => Some(
                if *selected {
                    "selected"
                } else {
                    "not selected"
                }
                .to_string(),
            ),
            // A list entry's label is its value, and a button has none at all;
            // repeating the label here would make a reader say it twice.
            WidgetKind::ListItem { .. } | WidgetKind::Button { .. } => None,
        }
    }

    /// Place this widget for one frame.
    pub fn place(self, rect: WidgetRect, control_rect: WidgetRect) -> WidgetSpec {
        WidgetSpec {
            desc: self,
            rect,
            control_rect,
            hovered: false,
        }
    }
}

/// One control, fully described for this frame: semantics plus layout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WidgetSpec {
    /// Identity, role, label and state.
    pub desc: WidgetDesc,
    /// Full row rectangle — the focus highlight and the hit region.
    pub rect: WidgetRect,
    /// The interactive control itself, inside `rect`.
    pub control_rect: WidgetRect,
    /// Whether the pointer is over this widget.
    ///
    /// Dead as of UI/UX v3 P3b2a: `draw_row_background` now reads the hover
    /// *weight* instead. Removed, with the nine `.hovered(...)` call sites,
    /// in the follow-up task.
    #[allow(dead_code)]
    pub hovered: bool,
}

impl WidgetSpec {
    /// Stable identity.
    pub fn id(&self) -> WidgetId {
        self.desc.id
    }

    /// Kind and current value.
    pub fn kind(&self) -> &WidgetKind {
        &self.desc.kind
    }

    /// Whether keyboard focus is on this widget.
    pub fn focused(&self) -> bool {
        self.desc.focused
    }

    /// Whether the widget accepts input right now.
    pub fn enabled(&self) -> bool {
        self.desc.enabled
    }

    /// Mark the widget hovered.
    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }
}

/// Find the widget under `(px, py)`.
///
/// Disabled and non-interactive widgets are skipped so a section header can
/// never steal a click from the row beneath it. Later specs win on overlap,
/// matching painter's order: a swatch drawn on top of its row is hit first.
pub(crate) fn hit_test(specs: &[WidgetSpec], px: f32, py: f32) -> Option<WidgetId> {
    specs
        .iter()
        .rev()
        .find(|s| s.enabled() && s.kind().is_interactive() && s.rect.contains(px, py))
        .map(|s| s.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(index: u16, kind: WidgetKind, y: f32) -> WidgetSpec {
        WidgetDesc::new(WidgetId::new(1, index), kind, format!("row {index}")).place(
            WidgetRect::new(0.0, y, 200.0, 20.0),
            WidgetRect::new(100.0, y, 100.0, 20.0),
        )
    }

    fn toggle(index: u16, y: f32) -> WidgetSpec {
        row(index, WidgetKind::Toggle { on: false }, y)
    }

    #[test]
    fn a_widget_is_valid_until_marked_otherwise() {
        let desc = WidgetDesc::new(
            WidgetId::new(1, 0),
            WidgetKind::Cycle {
                value: "known".into(),
            },
            "action",
        );
        assert!(!desc.invalid);
        assert!(desc.clone().invalid(true).invalid);
        assert!(!desc.invalid(false).invalid);
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
    fn spin_buttons_are_interactive_and_expose_their_display_text() {
        // Added for the Ssh port field (UI/UX v3 P1c): announced as a spin
        // button with a numeric range, drawn like a cycler.
        let kind = WidgetKind::SpinButton {
            value: 22.0,
            min: 1.0,
            max: 65535.0,
            step: 1.0,
            display: "22".to_string(),
        };
        assert!(kind.is_interactive());
        let desc = WidgetDesc::new(WidgetId::new(4, 3), kind, "Port");
        assert_eq!(desc.value_text().as_deref(), Some("22"));
    }

    #[test]
    fn widget_ids_round_trip_through_their_packed_form() {
        for id in [
            WidgetId::new(0, 0),
            WidgetId::new(3, 13),
            // Past the old u8 index ceiling: a list-shaped category addresses
            // one widget per entry, and those lists have no upper bound.
            WidgetId::new(9, 256),
            WidgetId::new(u8::MAX, u16::MAX),
        ] {
            assert_eq!(WidgetId::from_u32(id.as_u32()), Some(id));
        }
    }

    #[test]
    fn packed_ids_are_injective_across_category_boundaries() {
        // The last index of one category must not collide with the first of
        // the next, which is what a too-narrow shift would cause.
        assert_ne!(
            WidgetId::new(1, u16::MAX).as_u32(),
            WidgetId::new(2, 0).as_u32()
        );
        assert!(WidgetId::from_u32(0x0100_0000).is_none());
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
        specs[1].desc.enabled = false;
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
        let desc = WidgetDesc::new(
            WidgetId::new(1, 0),
            WidgetKind::Toggle { on: false },
            "row 0",
        )
        .focused(true)
        .tooltip("hint");
        let w = desc
            .place(WidgetRect::default(), WidgetRect::default())
            .hovered(true);
        assert!(w.focused());
        assert!(w.hovered);
        assert_eq!(w.desc.tooltip.as_deref(), Some("hint"));
    }
}
