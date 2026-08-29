//! Hover cross-fade between two items of one model (UI/UX v3 P3b2).
//!
//! A hover weight is a scalar, not a layer: three of the four hover models
//! in this client interpolate more than one property (a fill, an accent
//! line, a text colour), and two of them compute the hovered colour by
//! brightening the resting one. So this type answers "how hovered is this
//! id, right now" and each draw site lerps its own appearance — unlike
//! `SurfaceMotion`, whose consumers fade a whole surface's vertices.
//!
//! One pointer means one transition **per model**, not globally: moving from
//! a settings row to a tab starts a tab-bar transition while the widget
//! layer's is still fading out. Each model therefore owns its own
//! `HoverTransition`.
//!
//! The logical hover state (`SettingsPanel.hover_widget`,
//! `ContextMenu.hovered`) stays the truth for tooltips, hit-testing and
//! accessibility. It cannot also carry the transition: it goes `None` the
//! moment the pointer leaves, which is exactly when the fade-out must still
//! be running.

use std::time::Instant;

use nexterm_config::AnimationsConfig;

use super::{Curve, Timed, duration};

/// A cross-fade between the previously hovered item and the current one.
///
/// **Two timers, not one.** The obvious form is a single `Timed` with the
/// outgoing item at `1 - progress` and the incoming one at `progress`, so
/// the pair always sums to 1. That is wrong: the invariant only holds when
/// the outgoing item was already at weight 1. Enter row A and, 50 ms later
/// while A is still at 0.5, move to row B — a single timer makes B *jump* to
/// 0.5 on the frame the pointer crosses the boundary. Sweeping down a list
/// crosses boundaries faster than 100 ms routinely, so that form pops on
/// exactly the gesture hover exists to support.
///
/// With two timers the outgoing item decays from the weight it actually held
/// and the incoming one rises from the weight *it* actually held — 0
/// normally, or its partly-decayed value when the pointer comes back to it.
/// The pair does not sum to 1 mid-handoff, which is correct: at that instant
/// neither row is fully hovered.
///
/// One slot is a real limitation: only one item fades out at a time, so
/// sweeping across five rows drops the three intermediate ones to 0 as each
/// is replaced, leaving a trail that cuts off rather than one that fades. A
/// fixed-capacity `id → Timed` map would fix it behind an unchanged
/// `weight()`; a single slot is bounded and matches what the design chose.
// Consumed by the two overlay models in Tasks 3 and 4.
#[derive(Debug, Clone, Copy)]
pub struct HoverTransition<Id> {
    /// The item fading out, and the weight it held when it started to.
    from: Option<(Id, f32)>,
    from_anim: Timed,
    /// The item fading in.
    to: Option<Id>,
    to_anim: Timed,
}

impl<Id> Default for HoverTransition<Id> {
    fn default() -> Self {
        // Both animations are born finished; with `from` and `to` both
        // `None`, every id weighs 0 regardless. `Instant::now()` here is an
        // arbitrary start: at a zero duration, `Timed::raw_progress` and
        // `Timed::resuming_at` short-circuit without reading `start`, so any
        // instant is equally harmless.
        let zero = Timed::new(Instant::now(), 0, Curve::EasyEase);
        Self {
            from: None,
            from_anim: zero,
            to: None,
            to_anim: zero,
        }
    }
}

// Consumed by the two overlay models in Tasks 3 and 4.
impl<Id: Copy + PartialEq> HoverTransition<Id> {
    /// Point the transition at `to`, resuming from whatever is on screen.
    ///
    /// Idempotent while `to` is unchanged, because the caller is a
    /// pointer-motion handler that fires far more often than the hovered id
    /// changes; restarting the fade on every frame of a slow drag across one
    /// row would freeze it near 0.
    pub fn retarget(&mut self, to: Option<Id>, now: Instant, anim: &AnimationsConfig) {
        if self.to == to {
            return;
        }
        let ms = anim.scaled_duration_ms(duration::FASTER);
        // Read both weights off the screen *before* overwriting any field.
        let outgoing = self.to.map(|id| (id, self.weight(id, now)));
        let incoming_from = to.map_or(0.0, |id| self.weight(id, now));

        self.from = outgoing;
        self.from_anim = Timed::new(now, ms, Curve::EasyEase);
        self.to = to;
        self.to_anim = Timed::resuming_at(now, incoming_from, ms, Curve::EasyEase);
    }

    /// Hover weight for `id` in `[0, 1]`.
    pub fn weight(&self, id: Id, now: Instant) -> f32 {
        if self.to == Some(id) {
            return self.to_anim.progress(now);
        }
        if let Some((from_id, held)) = self.from
            && from_id == id
        {
            return held * (1.0 - self.from_anim.progress(now));
        }
        0.0
    }

    /// Whether another frame is needed.
    pub fn is_active(&self, now: Instant) -> bool {
        (self.to.is_some() && !self.to_anim.is_done(now))
            || (self.from.is_some() && !self.from_anim.is_done(now))
    }

    /// The item currently being hovered, as far as the transition knows.
    ///
    /// Only available in test builds. All four hover models (settings rows,
    /// context menu, tab bar, window buttons) read `weight` directly in
    /// production; this method exists to let tests pin that `retarget`
    /// actually moved the target, a property the weight assertions alone do
    /// not cover.
    #[cfg(test)]
    pub fn target(&self) -> Option<Id> {
        self.to
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::AnimationsConfig;
    use std::time::Duration;

    fn on() -> AnimationsConfig {
        AnimationsConfig::default()
    }

    fn off() -> AnimationsConfig {
        let mut cfg = AnimationsConfig::default();
        cfg.enabled = nexterm_config::AnimationsEnabled::No;
        cfg
    }

    /// 100 ms is `duration::FASTER`, the constant both P3b2 models use.
    const MS: u64 = 100;

    #[test]
    fn a_fresh_transition_weighs_nothing() {
        let h: HoverTransition<u32> = HoverTransition::default();
        let now = Instant::now();
        assert!(h.weight(1, now).abs() < 1e-4);
        assert!(!h.is_active(now));
        assert_eq!(h.target(), None);
    }

    #[test]
    fn entering_an_item_fades_it_in() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(7), t0, &on());
        assert_eq!(h.target(), Some(7));
        assert!(h.weight(7, t0).abs() < 1e-3);
        assert!(h.is_active(t0));
        let done = t0 + Duration::from_millis(MS);
        assert!((h.weight(7, done) - 1.0).abs() < 1e-3);
        assert!(!h.is_active(done));
    }

    /// The cross-fade: the item being left and the item being entered are
    /// complementary at every instant, and nothing else weighs anything.
    #[test]
    fn moving_between_items_cross_fades_them() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &on());
        let settled = t0 + Duration::from_millis(MS);
        h.retarget(Some(2), settled, &on());

        let mid = settled + Duration::from_millis(50);
        let (w1, w2) = (h.weight(1, mid), h.weight(2, mid));
        assert!(
            w1 > 0.1 && w1 < 0.9,
            "outgoing item should be mid-fade: {w1}"
        );
        assert!(
            w2 > 0.1 && w2 < 0.9,
            "incoming item should be mid-fade: {w2}"
        );
        assert!((w1 + w2 - 1.0).abs() < 1e-3, "must be complementary");
        assert!(h.weight(3, mid).abs() < 1e-4, "untouched item weighs 0");

        let done = settled + Duration::from_millis(MS);
        assert!(h.weight(1, done).abs() < 1e-3);
        assert!((h.weight(2, done) - 1.0).abs() < 1e-3);
    }

    /// Leaving the model entirely still fades the last item out — this is why
    /// the transition cannot live on the logical hover state, which goes
    /// `None` the moment the pointer leaves.
    #[test]
    fn leaving_fades_the_last_item_out() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &on());
        let settled = t0 + Duration::from_millis(MS);
        h.retarget(None, settled, &on());
        assert_eq!(h.target(), None);

        let mid = settled + Duration::from_millis(50);
        let w = h.weight(1, mid);
        assert!(w > 0.1 && w < 0.9, "must still be drawn while fading: {w}");
        assert!(h.is_active(mid));

        let done = settled + Duration::from_millis(MS);
        assert!(h.weight(1, done).abs() < 1e-3);
        assert!(!h.is_active(done));
    }

    /// Retargeting to the same id must not restart the fade — `retarget` is
    /// called from a per-motion handler that fires far more often than the
    /// hovered id changes.
    #[test]
    fn retargeting_the_same_item_is_a_no_op() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(7), t0, &on());
        let mid = t0 + Duration::from_millis(50);
        let before = h.weight(7, mid);
        h.retarget(Some(7), mid, &on());
        let after = h.weight(7, mid);
        assert!(
            (after - before).abs() < 1e-4,
            "fade restarted: {before} -> {after}"
        );
        // And it still finishes on the original schedule.
        assert!((h.weight(7, t0 + Duration::from_millis(MS)) - 1.0).abs() < 1e-3);
    }

    /// The defect the two-timer design exists to prevent. A single `Timed`
    /// with the pair summing to 1 makes the *incoming* item jump to whatever
    /// the outgoing one held — here 0.5 — the instant the pointer crosses
    /// the boundary. Sweeping a list crosses boundaries faster than the
    /// 100 ms fade, so that jump is the common case, not the corner.
    #[test]
    fn interrupting_mid_fade_jumps_neither_item() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &on());
        let mid = t0 + Duration::from_millis(50);
        let out_before = h.weight(1, mid);
        assert!(
            out_before > 0.1 && out_before < 0.9,
            "the test needs item 1 genuinely mid-fade: {out_before}"
        );

        h.retarget(Some(2), mid, &on());

        let out_after = h.weight(1, mid);
        assert!(
            (out_after - out_before).abs() < 1e-3,
            "the outgoing item jumped: {out_before} -> {out_after}"
        );
        let in_after = h.weight(2, mid);
        assert!(
            in_after.abs() < 1e-3,
            "the incoming item must start from nothing, not from the \
             outgoing item's weight: {in_after}"
        );
    }

    /// Coming back to the item that is still fading out must resume it, not
    /// restart it from 0 — the pointer wobbling on a row boundary is the
    /// gesture this covers.
    #[test]
    fn returning_to_a_fading_item_resumes_it() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &on());
        let settled = t0 + Duration::from_millis(100);
        h.retarget(Some(2), settled, &on());

        let mid = settled + Duration::from_millis(50);
        let held = h.weight(1, mid);
        assert!(
            held > 0.1 && held < 0.9,
            "item 1 should be mid-decay: {held}"
        );

        h.retarget(Some(1), mid, &on());
        let resumed = h.weight(1, mid);
        assert!(
            (resumed - held).abs() < 5e-2,
            "returning restarted the fade: {held} -> {resumed}"
        );
        // And it climbs back to 1 rather than stalling.
        assert!((h.weight(1, mid + Duration::from_millis(100)) - 1.0).abs() < 1e-2);
    }

    /// The reduced-motion path.
    #[test]
    fn disabled_animations_snap() {
        let mut h: HoverTransition<u32> = HoverTransition::default();
        let t0 = Instant::now();
        h.retarget(Some(1), t0, &off());
        assert!((h.weight(1, t0) - 1.0).abs() < 1e-4);
        assert!(!h.is_active(t0));
        h.retarget(Some(2), t0, &off());
        assert!(h.weight(1, t0).abs() < 1e-4);
        assert!((h.weight(2, t0) - 1.0).abs() < 1e-4);
        assert!(!h.is_active(t0));
    }
}
