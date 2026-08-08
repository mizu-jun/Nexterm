//! Pointer-dwell tracking for settings-panel tooltips (UI/UX v3 P1b).
//!
//! Tooltips appear only after the pointer has rested on one control for a
//! moment, so sweeping the mouse across the panel does not flash a trail of
//! them. The state lives here, next to the rest of the panel state; the
//! renderer owns placement and drawing.

use std::time::Instant;

/// How long the pointer must rest on a control before its tooltip appears.
pub const TOOLTIP_DELAY_MS: u128 = 500;

/// The control the pointer is resting on, and since when.
///
/// Identified by the same `(category, index)` pair as the renderer's
/// `WidgetId`, kept as plain integers so the panel state does not depend on
/// renderer-internal types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverDwell {
    /// `SettingsCategory::ALL` index of the owning category.
    pub category: u8,
    /// Widget index within that category.
    pub index: u8,
    /// When the pointer arrived.
    pub since: Instant,
}

impl HoverDwell {
    /// Record the pointer being over `(category, index)` at `now`.
    ///
    /// Staying on the same control keeps the running timer, so small
    /// movements within a row do not restart the dwell; moving to a different
    /// control resets it.
    pub fn enter(previous: Option<Self>, category: u8, index: u8, now: Instant) -> Self {
        match previous {
            Some(prev) if prev.category == category && prev.index == index => prev,
            _ => Self {
                category,
                index,
                since: now,
            },
        }
    }

    /// Whether the dwell has lasted long enough to show the tooltip.
    pub fn is_ready(&self, now: Instant) -> bool {
        now.duration_since(self.since).as_millis() >= TOOLTIP_DELAY_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn delay() -> Duration {
        Duration::from_millis(TOOLTIP_DELAY_MS as u64)
    }

    #[test]
    fn dwell_must_elapse_before_the_tooltip_is_ready() {
        let now = Instant::now();
        let d = HoverDwell::enter(None, 2, 0, now);
        assert!(!d.is_ready(now));
        assert!(!d.is_ready(now + delay() - Duration::from_millis(1)));
        assert!(d.is_ready(now + delay()));
    }

    #[test]
    fn staying_on_one_control_keeps_the_timer_running() {
        let start = Instant::now();
        let first = HoverDwell::enter(None, 2, 0, start);
        let later = start + Duration::from_millis(400);
        let same = HoverDwell::enter(Some(first), 2, 0, later);
        assert_eq!(same.since, start, "the timer must not restart");
        assert!(same.is_ready(start + delay()));
    }

    #[test]
    fn moving_to_another_control_restarts_the_timer() {
        let start = Instant::now();
        let first = HoverDwell::enter(None, 2, 0, start);
        let later = start + Duration::from_millis(400);
        let moved = HoverDwell::enter(Some(first), 2, 1, later);
        assert_eq!(moved.since, later);
        assert!(!moved.is_ready(later));
    }

    #[test]
    fn moving_to_another_category_restarts_the_timer() {
        let start = Instant::now();
        let first = HoverDwell::enter(None, 2, 0, start);
        let later = start + Duration::from_millis(400);
        let moved = HoverDwell::enter(Some(first), 3, 0, later);
        assert_eq!(moved.since, later);
    }
}
