//! UI animation foundation (Sprint 5-7 / Phase 3-2; Phase 4 spring physics).
//!
//! ## Architecture
//!
//! **Time-based animations** (pane fade-in white overlay): progress is computed
//! from a start `Instant` via `compute_progress`.
//!
//! **Spring-based animations** (tab accent line, pane dim overlay): a
//! mass-spring-damper ODE drives each `SpringState`, ticked once per frame via
//! `AnimationManager::tick`. Springs give a physically plausible feel — a
//! slightly underdamped tab accent gives a subtle snap, while the pane dim uses
//! near-critical damping for a smooth fade.
//!
//! Design notes:
//! - All timing math lives in [`AnimationManager`]; the renderer is a pure consumer.
//! - `tick(now, enabled)` must be called once at the top of every rendered frame.
//!   When `enabled = false` (animations disabled in config), every spring snaps to
//!   its target instantly.
//! - Easing functions are kept for the time-based fade-in overlay.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Maximum alpha for the dim overlay on non-focused panes.
pub const MAX_DIM_ALPHA: f32 = 0.06;

// ---------------------------------------------------------------------------
// Easing helpers (kept for the pane fade-in overlay in render_frame.rs)
// ---------------------------------------------------------------------------

/// Cubic ease-out. Maps `t ∈ [0, 1]` to `[0, 1]`.
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

/// Linear (identity).
#[allow(dead_code)]
pub fn linear(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// Compute `progress ∈ [0, 1]` from a start time, the current time, and a duration.
///
/// - When `duration_ms == 0`, the result is always `1.0` (animations disabled).
/// - When `elapsed ≥ duration`, the result is `1.0`.
pub fn compute_progress(start: Instant, now: Instant, duration_ms: u32) -> f32 {
    if duration_ms == 0 {
        return 1.0;
    }
    let elapsed_ms = now.saturating_duration_since(start).as_millis() as f32;
    (elapsed_ms / duration_ms as f32).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Spring physics
// ---------------------------------------------------------------------------

/// A single degree-of-freedom mass-spring-damper (mass = 1).
///
/// ODE: `a = -k*(x - target) - c*v`  (semi-implicit Euler integration)
///
/// Damping ratio `ζ = c / (2√k)`:
/// - Tab accent: `k=460, c=28` → ζ ≈ 0.65 (snappy, ~5 % overshoot)
/// - Pane dim:   `k=280, c=32` → ζ ≈ 0.96 (near-critical, smooth fade)
/// - Pane fade:  `k=200, c=28` → ζ ≈ 0.99 (smooth entrance)
#[derive(Debug, Clone, Copy)]
pub struct SpringState {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    stiffness: f32,
    damping: f32,
}

impl SpringState {
    fn new(initial: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            value: initial,
            velocity: 0.0,
            target: initial,
            stiffness,
            damping,
        }
    }

    /// Preset for the tab accent-line animation (starts fully visible).
    pub fn tab_accent() -> Self {
        Self::new(1.0, 460.0, 28.0)
    }

    /// Preset for per-pane dim overlay (starts invisible).
    pub fn pane_dim() -> Self {
        Self::new(0.0, 280.0, 32.0)
    }

    /// Advance the spring by `dt` seconds (semi-implicit Euler).
    ///
    /// `dt` is clamped to 50 ms to prevent explosive behaviour after long pauses
    /// (e.g. when the window was minimised).
    pub fn tick(&mut self, dt: f32) {
        let dt = dt.min(0.05);
        let a = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
        self.velocity += a * dt;
        self.value += self.velocity * dt;
    }

    /// Returns `true` when the spring has effectively come to rest.
    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 0.001 && self.velocity.abs() < 0.01
    }

    /// Immediately set `value = target` and zero the velocity.
    pub fn snap(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }
}

// ---------------------------------------------------------------------------
// AnimationManager
// ---------------------------------------------------------------------------

/// Manages every running animation.
///
/// Call `tick(now, enabled)` **once at the top of each rendered frame** before
/// querying any progress values.
///
/// Call `record_*` when an event happens; the renderer queries the state via
/// `tab_accent_progress()`, `pane_dim_alpha()`, and `pane_fade_in_progress()`.
#[derive(Debug)]
pub struct AnimationManager {
    // --- spring-based ---
    /// Tab accent-line spring (resets to 0 → target 1 on each tab switch).
    tab_accent: SpringState,
    /// Per-pane dim overlay springs. Target = 0.0 for focused, MAX_DIM_ALPHA otherwise.
    pane_dims: HashMap<u32, SpringState>,
    /// Wall-clock time of the last `tick` call (used to compute `dt`).
    last_tick: Option<Instant>,

    // --- time-based (backward-compat) ---
    /// Most-recent tab-switch start time + destination pane_id.
    tab_switch: Option<TabSwitchState>,
    /// Pane fade-in animations (pane_id → start time).
    pane_fade_ins: HashMap<u32, Instant>,
}

#[derive(Debug, Clone, Copy)]
struct TabSwitchState {
    to_pane: u32,
    started_at: Instant,
}

impl Default for AnimationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationManager {
    /// Build a fresh [`AnimationManager`].
    pub fn new() -> Self {
        Self {
            tab_accent: SpringState::tab_accent(),
            pane_dims: HashMap::new(),
            last_tick: None,
            tab_switch: None,
            pane_fade_ins: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Frame tick
    // -----------------------------------------------------------------------

    /// Advance all spring animations by the time elapsed since the last tick.
    ///
    /// Must be called **once per rendered frame**.  When `enabled = false` all
    /// springs snap to their targets immediately (reduced-motion / animations
    /// disabled).
    pub fn tick(&mut self, now: Instant, enabled: bool) {
        let dt = self
            .last_tick
            .map(|t| now.saturating_duration_since(t).as_secs_f32())
            .unwrap_or(0.0);
        self.last_tick = Some(now);

        if !enabled {
            self.tab_accent.snap();
            for s in self.pane_dims.values_mut() {
                s.snap();
            }
            return;
        }

        if dt > 0.0 {
            self.tab_accent.tick(dt);
            for s in self.pane_dims.values_mut() {
                s.tick(dt);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Event recording
    // -----------------------------------------------------------------------

    /// Record a tab-switch event (the focused pane just changed to `to_pane`).
    ///
    /// Resets the tab accent spring from 0 → 1 and stashes the time-based
    /// fallback state.
    pub fn record_tab_switch(&mut self, to_pane: u32, now: Instant) {
        self.tab_switch = Some(TabSwitchState {
            to_pane,
            started_at: now,
        });
        self.tab_accent.value = 0.0;
        self.tab_accent.velocity = 0.0;
        self.tab_accent.target = 1.0;
    }

    /// Update dim-overlay spring targets after a focus change.
    ///
    /// Call this whenever `focused_id` or the set of visible pane IDs changes.
    /// New panes get a fresh spring (starts at 0, targets MAX_DIM_ALPHA).
    /// Panes no longer in `all_ids` are pruned.
    pub fn record_focus_changed(&mut self, focused_id: u32, all_ids: &[u32]) {
        for &id in all_ids {
            let s = self
                .pane_dims
                .entry(id)
                .or_insert_with(SpringState::pane_dim);
            s.target = if id == focused_id { 0.0 } else { MAX_DIM_ALPHA };
        }
        let id_set: std::collections::HashSet<u32> = all_ids.iter().copied().collect();
        self.pane_dims.retain(|id, _| id_set.contains(id));
    }

    /// Record a pane-addition event (starts the white fade-in overlay).
    pub fn record_pane_added(&mut self, pane_id: u32, now: Instant) {
        self.pane_fade_ins.insert(pane_id, now);
    }

    /// Discard animation state when a pane is removed.
    pub fn record_pane_removed(&mut self, pane_id: u32) {
        self.pane_fade_ins.remove(&pane_id);
        self.pane_dims.remove(&pane_id);
        if let Some(ref s) = self.tab_switch
            && s.to_pane == pane_id
        {
            self.tab_switch = None;
        }
    }

    /// Remove expired time-based animation entries to prevent map growth.
    ///
    /// Spring-based entries are pruned via `record_pane_removed` / `record_focus_changed`.
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

    // -----------------------------------------------------------------------
    // Progress queries
    // -----------------------------------------------------------------------

    /// Current tab accent-line progress `[0, 1]`.
    ///
    /// Returns the spring's current value.  Starts at 1.0 when no animation
    /// has run yet; drops to ~0 on each tab switch and springs back to 1.0.
    pub fn tab_accent_progress(&self) -> f32 {
        self.tab_accent.value.clamp(0.0, 1.0)
    }

    /// Current dim alpha for `pane_id`.
    ///
    /// Returns `0.0` for unknown panes (no overlay) and the spring's current
    /// value otherwise.
    pub fn pane_dim_alpha(&self, pane_id: u32) -> f32 {
        self.pane_dims
            .get(&pane_id)
            .map(|s| s.value.clamp(0.0, 1.0))
            .unwrap_or(0.0)
    }

    /// Return the pane_id of the most recent tab switch (only while active).
    #[allow(dead_code)]
    pub fn current_tab_switch_target(&self) -> Option<u32> {
        self.tab_switch.as_ref().map(|s| s.to_pane)
    }

    /// Return the fade-in progress for the specified pane (`[0, 1]`).
    ///
    /// Returns `1.0` when no record exists (pane is fully visible).
    pub fn pane_fade_in_progress(&self, pane_id: u32, now: Instant, duration_ms: u32) -> f32 {
        match self.pane_fade_ins.get(&pane_id) {
            Some(started_at) => compute_progress(*started_at, now, duration_ms),
            None => 1.0,
        }
    }

    /// Time-based tab-switch progress (`[0, 1]`), kept for backward compatibility.
    ///
    /// Returns `1.0` when there is no active switch.
    #[allow(dead_code)]
    pub fn tab_switch_progress(&self, now: Instant, duration_ms: u32) -> f32 {
        match self.tab_switch {
            Some(s) => compute_progress(s.started_at, now, duration_ms),
            None => 1.0,
        }
    }

    /// Whether any animation is currently active (useful for redraw scheduling).
    #[allow(dead_code)]
    pub fn has_active_animation(&self, now: Instant, duration_ms: u32) -> bool {
        if duration_ms == 0 {
            return false;
        }
        if !self.tab_accent.is_settled() {
            return true;
        }
        if self.pane_dims.values().any(|s| !s.is_settled()) {
            return true;
        }
        self.pane_fade_ins
            .values()
            .any(|started_at| compute_progress(*started_at, now, duration_ms) < 1.0)
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Advance all springs by a fixed `dt` without a wall-clock `Instant`.
    ///
    /// Only available in test builds.
    #[cfg(test)]
    pub fn tick_by_dt(&mut self, dt: f32) {
        self.tab_accent.tick(dt);
        for s in self.pane_dims.values_mut() {
            s.tick(dt);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn approx_loose(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
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

    // ---- SpringState ----

    #[test]
    fn spring_at_rest_is_settled() {
        let s = SpringState::tab_accent();
        assert!(s.is_settled()); // starts at value=1.0, target=1.0
    }

    #[test]
    fn spring_snap_reaches_target() {
        let mut s = SpringState::tab_accent();
        s.value = 0.0;
        s.velocity = 5.0;
        s.target = 1.0;
        s.snap();
        assert!(approx(s.value, 1.0));
        assert!(approx(s.velocity, 0.0));
    }

    #[test]
    fn spring_tick_moves_toward_target() {
        let mut s = SpringState::new(0.0, 460.0, 28.0);
        s.target = 1.0;
        s.tick(0.016);
        assert!(s.value > 0.0 && s.value < 1.0);
    }

    #[test]
    fn spring_settles_after_many_ticks() {
        let mut s = SpringState::new(0.0, 460.0, 28.0);
        s.target = 1.0;
        for _ in 0..600 {
            s.tick(0.016);
        }
        assert!(s.is_settled());
        assert!(approx_loose(s.value, 1.0));
    }

    #[test]
    fn spring_clamps_dt_to_prevent_explosion() {
        let mut s = SpringState::new(0.0, 460.0, 28.0);
        s.target = 1.0;
        // A huge dt (e.g. 10 s after minimise) should not blow up.
        s.tick(10.0); // clamped to 0.05 internally
        // Value should remain finite and not exceed a reasonable bound.
        assert!(s.value.is_finite());
        assert!(s.value.abs() < 100.0);
    }

    // ---- AnimationManager ----

    #[test]
    fn a_new_manager_is_at_rest() {
        let mgr = AnimationManager::new();
        let now = Instant::now();
        assert!(approx(mgr.tab_accent_progress(), 1.0));
        assert!(approx(mgr.pane_dim_alpha(1), 0.0));
        assert!(approx(mgr.tab_switch_progress(now, 200), 1.0));
        assert!(approx(mgr.pane_fade_in_progress(1, now, 200), 1.0));
        assert!(!mgr.has_active_animation(now, 200));
    }

    #[test]
    fn record_tab_switch_resets_accent_spring_to_zero() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(approx(mgr.tab_accent_progress(), 0.0));
        assert_eq!(mgr.current_tab_switch_target(), Some(7));
    }

    #[test]
    fn record_tab_switch_time_based_progress_starts_at_0() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(approx(mgr.tab_switch_progress(now, 200), 0.0));
    }

    #[test]
    fn record_focus_changed_sets_dim_targets() {
        let mut mgr = AnimationManager::new();
        mgr.record_focus_changed(1, &[1, 2, 3]);
        assert!(approx(mgr.pane_dims[&1].target, 0.0));
        assert!(approx(mgr.pane_dims[&2].target, MAX_DIM_ALPHA));
        assert!(approx(mgr.pane_dims[&3].target, MAX_DIM_ALPHA));
    }

    #[test]
    fn record_focus_changed_removes_stale_panes() {
        let mut mgr = AnimationManager::new();
        mgr.record_focus_changed(1, &[1, 2, 3]);
        mgr.record_focus_changed(1, &[1, 2]); // pane 3 gone
        assert!(!mgr.pane_dims.contains_key(&3));
    }

    #[test]
    fn pane_dim_alpha_returns_zero_for_unknown_pane() {
        let mgr = AnimationManager::new();
        assert!(approx(mgr.pane_dim_alpha(99), 0.0));
    }

    #[test]
    fn tick_by_dt_advances_accent_spring() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(approx(mgr.tab_accent_progress(), 0.0));
        mgr.tick_by_dt(0.05); // 50 ms
        assert!(mgr.tab_accent_progress() > 0.01);
    }

    #[test]
    fn tick_snap_when_disabled() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        mgr.record_focus_changed(7, &[7, 8]);
        assert!(approx(mgr.tab_accent_progress(), 0.0));
        // Disabled tick snaps everything
        mgr.tick(now, false);
        assert!(approx(mgr.tab_accent_progress(), 1.0));
        assert!(approx(mgr.pane_dim_alpha(7), 0.0));
        assert!(approx(mgr.pane_dim_alpha(8), MAX_DIM_ALPHA));
    }

    #[test]
    fn record_pane_added_starts_fade_in_at_0() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_pane_added(42, now);
        assert!(approx(mgr.pane_fade_in_progress(42, now, 200), 0.0));
        assert!(approx(mgr.pane_fade_in_progress(43, now, 200), 1.0));
    }

    #[test]
    fn record_pane_removed_clears_all_state() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_pane_added(42, now);
        mgr.record_focus_changed(1, &[1, 42]);
        mgr.record_pane_removed(42);
        assert!(approx(mgr.pane_fade_in_progress(42, now, 200), 1.0));
        assert!(!mgr.pane_dims.contains_key(&42));
    }

    #[test]
    fn record_pane_removed_clears_matching_tab_switch() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        mgr.record_pane_removed(7);
        assert_eq!(mgr.current_tab_switch_target(), None);
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
    fn has_active_animation_true_while_spring_unsettled() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(mgr.has_active_animation(now, 200));
    }

    #[test]
    fn has_active_animation_false_after_spring_settles() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        for _ in 0..600 {
            mgr.tick_by_dt(0.016);
        }
        assert!(!mgr.has_active_animation(now, 200));
    }

    #[test]
    fn has_active_animation_false_when_animations_disabled() {
        let mut mgr = AnimationManager::new();
        let now = Instant::now();
        mgr.record_tab_switch(7, now);
        assert!(!mgr.has_active_animation(now, 0));
    }

    #[test]
    fn record_pane_added_overwrites_the_same_id() {
        let mut mgr = AnimationManager::new();
        let t0 = Instant::now();
        mgr.record_pane_added(42, t0);
        let t1 = t0 + Duration::from_millis(100);
        mgr.record_pane_added(42, t1);
        assert!(approx(mgr.pane_fade_in_progress(42, t1, 200), 0.0));
    }
}
