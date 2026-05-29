//! UI animation foundation (Sprint 5-7 / Phase 3-2).
//!
//! Records the timestamps of events such as tab switches and pane additions and
//! answers `progress(now)` queries from the renderer with a value in [0.0, 1.0].
//!
//! Design notes:
//! - All timing math lives in [`AnimationManager`]; the renderer is a pure consumer.
//! - Event recording is idempotent (re-registering the same id overwrites the
//!   timestamp).
//! - The duration is the effective value resolved from the config (via
//!   `AnimationsConfig::scaled_duration_ms`). When it is 0 ms, progress is always
//!   1.0 (i.e. an instant update).
//! - Easing functions are pure functions kept separate for testability.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Cubic ease-out. Maps `t ∈ [0, 1]` to `[0, 1]`.
///
/// Accelerates at the start and decelerates near the end for a natural feel.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

/// Linear (identity).
///
/// Currently unused but exposed for future animations such as cursor blink.
#[allow(dead_code)]
pub fn linear(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// Compute `progress ∈ [0, 1]` from a start time, the current time, and a duration.
///
/// - When `duration_ms == 0`, the result is always `1.0` (animations disabled).
/// - When `elapsed ≥ duration`, the result is `1.0`.
/// - Otherwise `elapsed / duration`.
pub fn compute_progress(start: Instant, now: Instant, duration_ms: u32) -> f32 {
    if duration_ms == 0 {
        return 1.0;
    }
    let elapsed_ms = now.saturating_duration_since(start).as_millis() as f32;
    (elapsed_ms / duration_ms as f32).clamp(0.0, 1.0)
}

/// Manager that owns every running animation.
///
/// Call `record_*` when an event happens to stash the timestamp; the renderer
/// queries `tab_switch_progress(now)` / `pane_fade_in_progress(id, now)` for
/// the progress.
#[derive(Debug, Default)]
pub struct AnimationManager {
    /// Most-recent tab-switch animation start time plus the destination pane_id.
    tab_switch: Option<TabSwitchState>,
    /// Pane fade-in animations (pane_id → start time).
    pane_fade_ins: HashMap<u32, Instant>,
}

#[derive(Debug, Clone, Copy)]
struct TabSwitchState {
    /// Target pane_id.
    to_pane: u32,
    /// When the switch started.
    started_at: Instant,
}

impl AnimationManager {
    /// Build a fresh [`AnimationManager`] (everything empty).
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a tab-switch event (call right when the focus pane changes).
    ///
    /// Recording the same pane_id twice resets the animation (the most recent
    /// timestamp wins).
    pub fn record_tab_switch(&mut self, to_pane: u32, now: Instant) {
        self.tab_switch = Some(TabSwitchState {
            to_pane,
            started_at: now,
        });
    }

    /// Record a pane-addition event.
    pub fn record_pane_added(&mut self, pane_id: u32, now: Instant) {
        self.pane_fade_ins.insert(pane_id, now);
    }

    /// Discard the matching animation state when a pane is removed.
    pub fn record_pane_removed(&mut self, pane_id: u32) {
        self.pane_fade_ins.remove(&pane_id);
        if let Some(ref s) = self.tab_switch
            && s.to_pane == pane_id
        {
            self.tab_switch = None;
        }
    }

    /// Clean up expired animation state (to prevent leaks).
    ///
    /// Intended to be called at the end of a frame or during idle. Removes any
    /// state older than `duration_ms`. There is no explicit cleanup hook today,
    /// but because the progress getter returns 1.0 once the duration elapses,
    /// leftover entries never cause incorrect rendering. Re-enable this once
    /// `record_*` is called frequently enough for the map to grow noticeably.
    #[allow(dead_code)]
    pub fn cleanup_expired(&mut self, now: Instant, duration_ms: u32) {
        if duration_ms == 0 {
            self.tab_switch = None;
            self.pane_fade_ins.clear();
            return;
        }
        let dur = Duration::from_millis(duration_ms as u64);
        if let Some(ref s) = self.tab_switch
            && now.saturating_duration_since(s.started_at) > dur
        {
            self.tab_switch = None;
        }
        self.pane_fade_ins
            .retain(|_, started_at| now.saturating_duration_since(*started_at) <= dur);
    }

    /// Return the tab-switch progress (`[0, 1]`).
    ///
    /// Returns `1.0` when there is no active switch (i.e. nothing to animate).
    pub fn tab_switch_progress(&self, now: Instant, duration_ms: u32) -> f32 {
        match self.tab_switch {
            Some(s) => compute_progress(s.started_at, now, duration_ms),
            None => 1.0,
        }
    }

    /// Return the fade-in progress for the specified pane (`[0, 1]`).
    ///
    /// Returns `1.0` when no record exists (fully visible).
    pub fn pane_fade_in_progress(&self, pane_id: u32, now: Instant, duration_ms: u32) -> f32 {
        match self.pane_fade_ins.get(&pane_id) {
            Some(started_at) => compute_progress(*started_at, now, duration_ms),
            None => 1.0,
        }
    }

    /// Return the pane_id of the most recent tab switch (only while it is active).
    pub fn current_tab_switch_target(&self) -> Option<u32> {
        self.tab_switch.as_ref().map(|s| s.to_pane)
    }

    /// Whether any animation is currently active (useful for redraw scheduling).
    ///
    /// Currently unused because we redraw every frame, but planned for the idle
    /// redraw-skipping optimisation.
    #[allow(dead_code)]
    pub fn has_active_animation(&self, now: Instant, duration_ms: u32) -> bool {
        if duration_ms == 0 {
            return false;
        }
        if self.tab_switch_progress(now, duration_ms) < 1.0 {
            return true;
        }
        self.pane_fade_ins
            .values()
            .any(|started_at| compute_progress(*started_at, now, duration_ms) < 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    // ---- easing functions ----

    #[test]
    fn ease_out_cubic_is_0_and_1_at_endpoints() {
        assert!(approx(ease_out_cubic(0.0), 0.0));
        assert!(approx(ease_out_cubic(1.0), 1.0));
    }

    #[test]
    fn ease_out_cubic_is_monotonically_increasing() {
        let v00 = ease_out_cubic(0.0);
        let v25 = ease_out_cubic(0.25);
        let v50 = ease_out_cubic(0.5);
        let v75 = ease_out_cubic(0.75);
        let v100 = ease_out_cubic(1.0);
        assert!(v00 < v25 && v25 < v50 && v50 < v75 && v75 < v100);
    }

    #[test]
    fn ease_out_cubic_exceeds_linear_near_middle() {
        // ease-out is faster in the first half, so at t=0.3 it is ahead of linear.
        assert!(ease_out_cubic(0.3) > linear(0.3));
    }

    #[test]
    fn ease_out_cubic_clamps_out_of_range_inputs() {
        assert!(approx(ease_out_cubic(-1.0), 0.0));
        assert!(approx(ease_out_cubic(2.0), 1.0));
    }

    #[test]
    fn linear_is_identity() {
        assert!(approx(linear(0.0), 0.0));
        assert!(approx(linear(0.5), 0.5));
        assert!(approx(linear(1.0), 1.0));
        assert!(approx(linear(-0.5), 0.0));
        assert!(approx(linear(1.5), 1.0));
    }

    // ---- compute_progress ----

    #[test]
    fn compute_progress_with_duration_0_is_always_1() {
        let now = Instant::now();
        assert!(approx(compute_progress(now, now, 0), 1.0));
        let later = now + Duration::from_secs(60);
        assert!(approx(compute_progress(now, later, 0), 1.0));
    }

    #[test]
    fn compute_progress_with_zero_elapsed_is_0() {
        let now = Instant::now();
        assert!(approx(compute_progress(now, now, 200), 0.0));
    }

    #[test]
    fn compute_progress_at_half_duration_is_0_5() {
        let start = Instant::now();
        let now = start + Duration::from_millis(100);
        assert!(approx(compute_progress(start, now, 200), 0.5));
    }

    #[test]
    fn compute_progress_beyond_duration_clamps_to_1() {
        let start = Instant::now();
        let now = start + Duration::from_millis(500);
        assert!(approx(compute_progress(start, now, 200), 1.0));
    }

    // ---- AnimationManager ----

    #[test]
    fn a_new_manager_holds_nothing() {
        let mgr = AnimationManager::new();
        let now = Instant::now();
        // Every pane is considered fully visible.
        assert!(approx(mgr.tab_switch_progress(now, 200), 1.0));
        assert!(approx(mgr.pane_fade_in_progress(1, now, 200), 1.0));
        assert!(!mgr.has_active_animation(now, 200));
    }

    #[test]
    fn record_tab_switch_starts_progress_at_0() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(approx(mgr.tab_switch_progress(now, 200), 0.0));
        assert_eq!(mgr.current_tab_switch_target(), Some(7));
    }

    #[test]
    fn record_pane_added_starts_progress_at_0() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_pane_added(42, now);
        assert!(approx(mgr.pane_fade_in_progress(42, now, 200), 0.0));
        // Other panes are unaffected.
        assert!(approx(mgr.pane_fade_in_progress(43, now, 200), 1.0));
    }

    #[test]
    fn record_pane_removed_clears_fade_in() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_pane_added(42, now);
        mgr.record_pane_removed(42);
        // After removal the progress falls back to 1.0.
        assert!(approx(mgr.pane_fade_in_progress(42, now, 200), 1.0));
    }

    #[test]
    fn record_pane_removed_also_clears_matching_tab_switch() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        mgr.record_pane_removed(7);
        assert_eq!(mgr.current_tab_switch_target(), None);
        // Switching to a different pane is unaffected by removing 7.
        mgr.record_tab_switch(8, now);
        mgr.record_pane_removed(7);
        assert_eq!(mgr.current_tab_switch_target(), Some(8));
    }

    #[test]
    fn cleanup_expired_drops_expired_entries() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        mgr.record_pane_added(42, now);

        // Clean up at 2.5× the duration.
        let later = now + Duration::from_millis(500);
        mgr.cleanup_expired(later, 200);

        assert_eq!(mgr.current_tab_switch_target(), None);
        assert!(approx(mgr.pane_fade_in_progress(42, later, 200), 1.0));
    }

    #[test]
    fn cleanup_expired_with_duration_0_drops_everything() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        mgr.record_pane_added(42, now);
        mgr.cleanup_expired(now, 0);
        assert_eq!(mgr.current_tab_switch_target(), None);
        assert!(mgr.pane_fade_ins.is_empty());
    }

    #[test]
    fn has_active_animation_decision() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(mgr.has_active_animation(now, 200));

        // After the duration elapses, returns false.
        let later = now + Duration::from_millis(300);
        assert!(!mgr.has_active_animation(later, 200));

        // duration_ms = 0 always returns false.
        assert!(!mgr.has_active_animation(now, 0));
    }

    #[test]
    fn record_pane_added_overwrites_the_same_id() {
        let mut mgr = AnimationManager::new();
        let t0 = Instant::now();
        mgr.record_pane_added(42, t0);
        // Re-register 100 ms later → the start time is replaced.
        let t1 = t0 + Duration::from_millis(100);
        mgr.record_pane_added(42, t1);
        // At t1 the progress restarts at 0.0.
        assert!(approx(mgr.pane_fade_in_progress(42, t1, 200), 0.0));
    }
}
