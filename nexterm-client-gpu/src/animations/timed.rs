//! Time-based, curve-eased animations (UI/UX v3 P3a).
//!
//! A `Timed` is a value, not a running object: it stores when it started,
//! how long it lasts and which curve shapes it, and answers questions about
//! any instant you hand it. Nothing ticks it. That keeps every consumer
//! testable without a clock and makes an animation cheap to copy around.
//!
//! Springs (`SpringState` in the parent module) stay the tool for motion
//! that must be interruptible mid-flight by a new target, such as the tab
//! accent. `Timed` is for transitions with a known start, end and duration.
//!
//! `raw_progress` computes elapsed time itself (via `Duration::as_secs_f32`)
//! instead of delegating to `compute_progress`, which truncates to whole
//! milliseconds via `Duration::as_millis()`. That truncation is invisible
//! for `new()`, whose `start` is always the caller's own `Instant`, but
//! `resuming_at()` must *reconstruct* a `start` whose elapsed time
//! reproduces an arbitrary fractional value, and a curve with a steep tail
//! (e.g. `AccelerateMax`, whose derivative is largest near `t = 1`) turns a
//! sub-millisecond rounding error into a multi-percent progress error at a
//! typical 200 ms duration. Keeping elapsed time in full `Duration`
//! precision on both ends of that round trip removes the amplification.

use std::time::{Duration, Instant};

use super::curve::Curve;

/// One time-based animation.
#[derive(Debug, Clone, Copy)]
pub struct Timed {
    start: Instant,
    duration_ms: u32,
    curve: Curve,
}

impl Timed {
    /// Start an animation at `start`.
    ///
    /// Pass a `duration_ms` that already went through
    /// `AnimationsConfig::scaled_duration_ms`, so a user who turned
    /// animations off gets 0 here and the animation is born finished.
    pub fn new(start: Instant, duration_ms: u32, curve: Curve) -> Self {
        Self {
            start,
            duration_ms,
            curve,
        }
    }

    /// Build an animation that already holds `value` at `now`.
    ///
    /// This is how an interruption is expressed: read whatever value is on
    /// screen, then ask for an animation that continues from there instead
    /// of snapping back to 0. Continuity of *value* is guaranteed; the
    /// speed may change abruptly, which is the normal look of a reversed
    /// transition.
    pub fn resuming_at(now: Instant, value: f32, duration_ms: u32, curve: Curve) -> Self {
        if duration_ms == 0 {
            return Self::new(now, 0, curve);
        }
        let elapsed_secs = (duration_ms as f32 / 1000.0) * curve.invert(value);
        let elapsed = Duration::try_from_secs_f32(elapsed_secs).unwrap_or(Duration::ZERO);
        let start = now.checked_sub(elapsed).unwrap_or(now);
        Self::new(start, duration_ms, curve)
    }

    /// Elapsed fraction in `[0, 1]`, before easing.
    pub fn raw_progress(&self, now: Instant) -> f32 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        let elapsed_ms = now.saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        (elapsed_ms / self.duration_ms as f32).clamp(0.0, 1.0)
    }

    /// Eased progress in `[0, 1]` — the value a consumer should animate on.
    pub fn progress(&self, now: Instant) -> f32 {
        self.curve.eval(self.raw_progress(now))
    }

    /// Whether the animation has reached its end and needs no more frames.
    pub fn is_done(&self, now: Instant) -> bool {
        self.raw_progress(now) >= 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn progress_runs_from_0_to_1_over_the_duration() {
        let t0 = Instant::now();
        let a = Timed::new(t0, 200, Curve::Linear);
        assert!(a.progress(t0).abs() < 1e-4);
        assert!((a.progress(t0 + Duration::from_millis(100)) - 0.5).abs() < 1e-3);
        assert!((a.progress(t0 + Duration::from_millis(200)) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn progress_stays_at_1_past_the_duration() {
        let t0 = Instant::now();
        let a = Timed::new(t0, 200, Curve::DecelerateMax);
        assert!((a.progress(t0 + Duration::from_secs(60)) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn is_done_flips_at_the_duration() {
        let t0 = Instant::now();
        let a = Timed::new(t0, 200, Curve::DecelerateMax);
        assert!(!a.is_done(t0));
        assert!(!a.is_done(t0 + Duration::from_millis(199)));
        assert!(a.is_done(t0 + Duration::from_millis(200)));
    }

    /// The reduced-motion path: `AnimationsConfig::scaled_duration_ms`
    /// returns 0 when animations are disabled or the intensity is "off", and
    /// a zero-duration `Timed` must be finished before it is ever queried.
    #[test]
    fn a_zero_duration_animation_is_finished_immediately() {
        let t0 = Instant::now();
        let a = Timed::new(t0, 0, Curve::DecelerateMax);
        assert!((a.progress(t0) - 1.0).abs() < 1e-4);
        assert!(a.is_done(t0));
    }

    #[test]
    fn easing_makes_progress_differ_from_raw_progress() {
        let t0 = Instant::now();
        let mid = t0 + Duration::from_millis(100);
        let a = Timed::new(t0, 200, Curve::DecelerateMax);
        assert!((a.raw_progress(mid) - 0.5).abs() < 1e-3);
        assert!(a.progress(mid) > a.raw_progress(mid) + 0.05);
    }

    #[test]
    fn resuming_at_starts_from_the_requested_value() {
        let now = Instant::now();
        for curve in [
            Curve::Linear,
            Curve::DecelerateMax,
            Curve::AccelerateMax,
            Curve::EasyEase,
        ] {
            for i in 0..=10 {
                let v = i as f32 / 10.0;
                let a = Timed::resuming_at(now, v, 200, curve);
                assert!(
                    (a.progress(now) - v).abs() < 2e-2,
                    "{curve:?}: asked for {v}, got {}",
                    a.progress(now)
                );
            }
        }
    }

    #[test]
    fn resuming_at_finishes_within_the_remaining_duration() {
        let now = Instant::now();
        let a = Timed::resuming_at(now, 0.5, 200, Curve::Linear);
        assert!(!a.is_done(now));
        assert!(a.is_done(now + Duration::from_millis(200)));
    }

    #[test]
    fn resuming_a_zero_duration_animation_is_finished_immediately() {
        let now = Instant::now();
        let a = Timed::resuming_at(now, 0.3, 0, Curve::DecelerateMax);
        assert!(a.is_done(now));
        assert!((a.progress(now) - 1.0).abs() < 1e-4);
    }
}
