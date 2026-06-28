//! Phase 5b (UI/UX v2): smooth cursor motion.
//!
//! When `CursorConfig.smooth_motion = true`, the cursor interpolates
//! between cells over a brief duration (default 80 ms, ease-out cubic)
//! instead of snapping. The motion is driven by per-pane state stored
//! in the renderer (`WgpuState.cursor_motion`); this module owns only
//! the pure helpers (`ease_out_cubic`, `interpolate`, `animation_t`)
//! and the `CursorMotionState` struct.
//!
//! Behaviour notes:
//! - When `smooth_motion = false`, callers skip this module entirely
//!   and pass the server-reported cell directly. The cache key picks
//!   up a fixed quantized value so cache hits are preserved.
//! - When the server reports a new cursor cell mid-animation, the
//!   in-progress interpolated position is snapshotted as the new
//!   `prev` so the motion stays continuous (no jump back to the old
//!   cell).
//! - The duration is intentionally short (80 ms) so cursor latency
//!   stays imperceptible during typing while still smoothing the jump
//!   from one prompt line to the next.

use std::time::Instant;

/// Default duration for cursor motion interpolation in milliseconds.
/// 80 ms keeps the cursor responsive (~5 frames at 60 Hz) while still
/// being long enough to perceive as a glide rather than a snap.
pub const CURSOR_MOTION_DURATION_MS: u32 = 80;

/// Per-pane cursor motion state held by the renderer. `prev_col_f` /
/// `prev_row_f` are floating-point because they can hold the
/// mid-interpolation position when a new target arrives before the
/// previous animation finishes.
#[derive(Debug, Clone, Copy)]
pub struct CursorMotionState {
    /// Position to interpolate from. Floating because mid-animation
    /// targeting captures the visible position rather than the
    /// integer cell.
    pub prev_col_f: f32,
    pub prev_row_f: f32,
    /// Position to interpolate toward. Stored as integers because the
    /// server always reports cell-aligned positions.
    pub target_col: u16,
    pub target_row: u16,
    /// When the current animation started. The animation completes
    /// `CURSOR_MOTION_DURATION_MS` later (or when a new target arrives
    /// and resets the timer).
    pub transition_start: Instant,
}

impl CursorMotionState {
    /// Initialise a motion state with `prev == target` so the first
    /// frame is a no-op (cursor visible immediately at its starting
    /// position, no glide-in from nowhere).
    pub fn new(col: u16, row: u16, now: Instant) -> Self {
        Self {
            prev_col_f: col as f32,
            prev_row_f: row as f32,
            target_col: col,
            target_row: row,
            transition_start: now,
        }
    }
}

/// Standard ease-out cubic: `1 - (1 - t)^3`. Used so the cursor
/// decelerates as it approaches the new cell — initial movement is
/// fast (matches the user's intent) and the settling phase is gentle.
/// Inputs outside `[0, 1]` are not clamped here; callers are expected
/// to pass a clamped `animation_t`.
pub fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

/// Animation progress in `[0, 1]` given elapsed milliseconds and a
/// total duration. `duration_ms == 0` returns `1.0` immediately so the
/// caller can disable motion by setting the duration to zero.
pub fn animation_t(elapsed_ms: u64, duration_ms: u32) -> f32 {
    if duration_ms == 0 {
        return 1.0;
    }
    let raw = elapsed_ms as f32 / duration_ms as f32;
    raw.clamp(0.0, 1.0)
}

/// Linear interpolation between `prev` and `target` weighted by an
/// already-eased `t`. Kept as a separate function so tests can pin the
/// arithmetic without dragging in `Instant`.
pub fn interpolate(prev: f32, target: f32, t_eased: f32) -> f32 {
    prev + (target - prev) * t_eased
}

/// Apply a new server-reported cursor position to the motion state.
///
/// - When the target is unchanged, the existing animation continues.
/// - When the target moves, snapshot the *current visible position*
///   (computed from the existing animation progress) as the new `prev`,
///   set the new target, and reset the timer.
///
/// Returns the updated state.
pub fn update_target(
    state: CursorMotionState,
    new_col: u16,
    new_row: u16,
    now: Instant,
    duration_ms: u32,
) -> CursorMotionState {
    if state.target_col == new_col && state.target_row == new_row {
        return state;
    }
    let elapsed_ms = now.duration_since(state.transition_start).as_millis() as u64;
    let t = animation_t(elapsed_ms, duration_ms);
    let eased = ease_out_cubic(t);
    let visible_col = interpolate(state.prev_col_f, state.target_col as f32, eased);
    let visible_row = interpolate(state.prev_row_f, state.target_row as f32, eased);
    CursorMotionState {
        prev_col_f: visible_col,
        prev_row_f: visible_row,
        target_col: new_col,
        target_row: new_row,
        transition_start: now,
    }
}

/// Current visible cursor position derived from the motion state.
/// Returns `(col_f32, row_f32)` in cell coordinates (0-relative
/// inside the pane). Callers convert to pixels by multiplying by the
/// cell metrics.
pub fn visible_position(state: CursorMotionState, now: Instant, duration_ms: u32) -> (f32, f32) {
    let elapsed_ms = now.duration_since(state.transition_start).as_millis() as u64;
    let t = animation_t(elapsed_ms, duration_ms);
    let eased = ease_out_cubic(t);
    let col = interpolate(state.prev_col_f, state.target_col as f32, eased);
    let row = interpolate(state.prev_row_f, state.target_row as f32, eased);
    (col, row)
}

/// Quantise a sub-cell position to an integer for the render-cache
/// key. Granularity of 16 sub-cells balances cache reuse against
/// visible smoothness — at 60 Hz cell-to-cell motion takes ~5 frames,
/// well under 16 distinct quantised positions, so the cache
/// invalidates each animation frame.
pub fn quantize_visible(pos: f32) -> u32 {
    (pos * 16.0).round() as i32 as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ease-out cubic must satisfy the basic boundary conditions —
    /// 0→0, 1→1, and monotone in between. Without these the cursor
    /// would either jump to its destination instantly or oscillate.
    #[test]
    fn ease_out_cubic_satisfies_endpoints() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-6);
        let mid = ease_out_cubic(0.5);
        assert!(
            mid > 0.5,
            "ease-out should be ahead of linear at t=0.5, got {}",
            mid
        );
    }

    /// `animation_t` must clamp at both ends and return exactly 1.0
    /// when the duration is zero (so callers can disable motion).
    #[test]
    fn animation_t_clamps_to_unit_range() {
        assert_eq!(animation_t(0, 100), 0.0);
        assert_eq!(animation_t(50, 100), 0.5);
        assert_eq!(animation_t(100, 100), 1.0);
        assert_eq!(animation_t(200, 100), 1.0);
        assert_eq!(animation_t(50, 0), 1.0);
    }

    /// Interpolation must hit the endpoints exactly so the cursor
    /// lands on the target cell when the animation completes (no
    /// off-by-half-pixel residual).
    #[test]
    fn interpolate_hits_endpoints_exactly() {
        assert_eq!(interpolate(0.0, 10.0, 0.0), 0.0);
        assert_eq!(interpolate(0.0, 10.0, 1.0), 10.0);
        assert!((interpolate(0.0, 10.0, 0.5) - 5.0).abs() < 1e-6);
    }

    /// A fresh motion state (`new`) must report its position as the
    /// integer cell with zero animation progress — the cursor must
    /// not glide in from elsewhere on first render.
    #[test]
    fn new_state_has_no_animation() {
        let now = Instant::now();
        let s = CursorMotionState::new(5, 3, now);
        let (col, row) = visible_position(s, now, CURSOR_MOTION_DURATION_MS);
        assert!((col - 5.0).abs() < 1e-6);
        assert!((row - 3.0).abs() < 1e-6);
    }

    /// When the server reports the same cell, the motion state must
    /// not restart — otherwise typing would re-trigger animation on
    /// every keystroke and the cursor would feel laggy.
    #[test]
    fn unchanged_target_preserves_state() {
        let now = Instant::now();
        let s = CursorMotionState::new(5, 3, now);
        let later = now + std::time::Duration::from_millis(10);
        let s2 = update_target(s, 5, 3, later, CURSOR_MOTION_DURATION_MS);
        // Timer must not have reset; prev unchanged.
        assert_eq!(s2.transition_start, s.transition_start);
        assert_eq!(s2.target_col, 5);
        assert_eq!(s2.target_row, 3);
    }

    /// A new target snapshots the visible position so motion stays
    /// continuous when retargeting mid-animation. Regression guard:
    /// without snapshotting, the cursor would jump back to the old
    /// integer cell before gliding to the new one.
    #[test]
    fn retargeting_snapshots_visible_position() {
        let now = Instant::now();
        let mut s = CursorMotionState::new(0, 0, now);
        // Move target to 10,0 — full target so we can measure.
        s = update_target(s, 10, 0, now, CURSOR_MOTION_DURATION_MS);
        // 40 ms in (half-way), retarget to 20,0.
        let half = now + std::time::Duration::from_millis(40);
        let s2 = update_target(s, 20, 0, half, CURSOR_MOTION_DURATION_MS);
        // The new prev must equal the visible position at t=40ms (which is > 5.0 due to ease-out cubic).
        assert!(
            s2.prev_col_f > 5.0 && s2.prev_col_f < 10.0,
            "expected prev to be the mid-animation visible pos, got {}",
            s2.prev_col_f
        );
        assert_eq!(s2.target_col, 20);
    }

    /// After the animation duration elapses, the visible position
    /// must equal the integer target — otherwise the cursor would
    /// drift permanently.
    #[test]
    fn animation_completes_at_target() {
        let now = Instant::now();
        let mut s = CursorMotionState::new(0, 0, now);
        s = update_target(s, 10, 5, now, CURSOR_MOTION_DURATION_MS);
        let done = now + std::time::Duration::from_millis(CURSOR_MOTION_DURATION_MS as u64 + 10);
        let (col, row) = visible_position(s, done, CURSOR_MOTION_DURATION_MS);
        assert!((col - 10.0).abs() < 1e-6);
        assert!((row - 5.0).abs() < 1e-6);
    }

    /// Quantisation must be stable around integer cell positions so
    /// the cache key is deterministic when the animation has settled.
    #[test]
    fn quantize_is_stable_at_integer_positions() {
        assert_eq!(quantize_visible(0.0), 0);
        assert_eq!(quantize_visible(1.0), 16);
        assert_eq!(quantize_visible(5.0), 80);
    }

    /// Different sub-cell positions must produce different quantised
    /// values so the cache invalidates each animation frame at 60 Hz
    /// (~5 frames spread across the 80 ms duration).
    #[test]
    fn quantize_distinguishes_sub_cell_steps() {
        let a = quantize_visible(5.0);
        let b = quantize_visible(5.0625); // 1/16th of a cell further.
        assert_ne!(a, b);
    }
}
