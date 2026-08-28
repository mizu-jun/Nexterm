//! Easing helpers and time-based progress (Sprint 5-7 / Phase 3-2).
//!
//! Split out of `animations.rs` in UI/UX v3 P3a so the module stays within
//! the file-size guidance while `Curve` and `Timed` join it.

use std::time::Instant;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

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
}
