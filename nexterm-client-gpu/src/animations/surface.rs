//! The shared open/close timer pair for overlay surfaces (UI/UX v3 P3b).
//!
//! P3a proved this state machine on the settings panel by hand. P3b needs it
//! on eleven surfaces, so the logic is lifted here verbatim — including its
//! two ordering rules:
//!
//! 1. Read the value already on screen **before** overwriting either field;
//!    `progress` derives it from them, so touching one first loses it.
//! 2. Starting a close passes `1.0 - visibility`, because `closing` counts
//!    up while visibility counts down.
//!
//! The surface's own openness (`is_open`, or a live `Option`) remains the
//! truth for input routing and the AccessKit tree and still flips the
//! instant the user acts. This type is the renderer's permission to keep
//! drawing for a few more frames.

use std::time::Instant;

use nexterm_config::AnimationsConfig;

use super::{Curve, Timed};

/// One surface's entrance and exit animations.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceMotion {
    /// Entrance. `Some` from the moment the surface opens; its progress is
    /// the surface's visibility until it closes.
    open_anim: Option<Timed>,
    /// Exit — **render-only**. Retired by [`SurfaceMotion::retire`] once done.
    closing: Option<Timed>,
}

impl SurfaceMotion {
    /// Start the entrance, resuming from whatever is on screen.
    ///
    /// `base_ms` is an unscaled `duration::*` constant; the reduced-motion
    /// scaling happens here so no caller can forget it.
    pub fn open(&mut self, now: Instant, anim: &AnimationsConfig, base_ms: u32, curve: Curve) {
        let ms = anim.scaled_duration_ms(base_ms);
        let resume_from = self.closing.is_some().then(|| self.progress(now));
        self.closing = None;
        self.open_anim = Some(match resume_from {
            Some(v) => Timed::resuming_at(now, v, ms, curve),
            None => Timed::new(now, ms, curve),
        });
    }

    /// Start the exit from whatever is on screen.
    pub fn close(&mut self, now: Instant, anim: &AnimationsConfig, base_ms: u32, curve: Curve) {
        let ms = anim.scaled_duration_ms(base_ms);
        let visibility = self.progress(now);
        self.open_anim = None;
        self.closing = Some(Timed::resuming_at(now, 1.0 - visibility, ms, curve));
    }

    /// Visibility in `[0, 1]`: 0 hidden, 1 fully shown.
    pub fn progress(&self, now: Instant) -> f32 {
        if let Some(closing) = self.closing {
            return 1.0 - closing.progress(now);
        }
        self.open_anim.map_or(0.0, |a| a.progress(now))
    }

    /// Whether the renderer should draw the surface at all.
    pub fn is_visible(&self) -> bool {
        self.open_anim.is_some() || self.closing.is_some()
    }

    /// Whether another frame is needed.
    pub fn is_active(&self, now: Instant) -> bool {
        self.closing.is_some_and(|c| !c.is_done(now))
            || self.open_anim.is_some_and(|a| !a.is_done(now))
    }

    /// Drop a finished exit animation, so the surface stops being drawn.
    ///
    /// A finished *entrance* is deliberately kept: it is the surface's
    /// visibility for as long as it stays open.
    pub fn retire(&mut self, now: Instant) {
        if self.closing.is_some_and(|c| c.is_done(now)) {
            self.closing = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::AnimationsConfig;
    use std::time::{Duration, Instant};

    fn on() -> AnimationsConfig {
        AnimationsConfig::default()
    }

    fn off() -> AnimationsConfig {
        let mut cfg = AnimationsConfig::default();
        cfg.enabled = nexterm_config::AnimationsEnabled::No;
        cfg
    }

    fn open_it(m: &mut SurfaceMotion, now: Instant, anim: &AnimationsConfig) {
        m.open(now, anim, 300, Curve::DecelerateMax);
    }

    fn close_it(m: &mut SurfaceMotion, now: Instant, anim: &AnimationsConfig) {
        m.close(now, anim, 150, Curve::AccelerateMax);
    }

    #[test]
    fn a_fresh_motion_is_invisible() {
        let m = SurfaceMotion::default();
        let now = Instant::now();
        assert!(!m.is_visible());
        assert!(!m.is_active(now));
        assert!(m.progress(now).abs() < 1e-4);
    }

    #[test]
    fn open_runs_from_0_to_1_over_the_entrance_duration() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        assert!(m.is_visible());
        assert!(m.progress(t0).abs() < 1e-3);
        assert!((m.progress(t0 + Duration::from_millis(300)) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn close_keeps_the_surface_visible_while_it_fades() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        let opened = t0 + Duration::from_millis(300);
        close_it(&mut m, opened, &on());
        assert!(m.is_visible(), "the renderer must keep drawing it");
        assert!(m.progress(opened) > 0.9);
        let done = opened + Duration::from_millis(150);
        assert!(m.progress(done).abs() < 1e-3);
    }

    #[test]
    fn reopening_mid_fade_is_continuous() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        let opened = t0 + Duration::from_millis(300);
        close_it(&mut m, opened, &on());
        let mid = opened + Duration::from_millis(75);
        let before = m.progress(mid);
        open_it(&mut m, mid, &on());
        let after = m.progress(mid);
        assert!(
            (after - before).abs() < 5e-2,
            "value jumped on reopen: {before} -> {after}"
        );
    }

    #[test]
    fn retire_drops_a_finished_exit_and_hides_the_surface() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        let opened = t0 + Duration::from_millis(300);
        close_it(&mut m, opened, &on());
        let mid = opened + Duration::from_millis(75);
        m.retire(mid);
        assert!(m.is_visible(), "an unfinished exit must survive retire");
        let done = opened + Duration::from_millis(150);
        m.retire(done);
        assert!(!m.is_visible());
        assert!(!m.is_active(done));
    }

    #[test]
    fn is_active_is_false_once_the_entrance_has_finished() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &on());
        assert!(m.is_active(t0));
        assert!(!m.is_active(t0 + Duration::from_millis(300)));
        assert!(m.is_visible(), "finished entrance still means shown");
    }

    /// The reduced-motion path: `scaled_duration_ms` returns 0, so both
    /// transitions are finished the moment they start.
    #[test]
    fn disabled_animations_open_and_close_instantly() {
        let mut m = SurfaceMotion::default();
        let t0 = Instant::now();
        open_it(&mut m, t0, &off());
        assert!((m.progress(t0) - 1.0).abs() < 1e-4);
        assert!(!m.is_active(t0));
        close_it(&mut m, t0, &off());
        assert!(m.progress(t0).abs() < 1e-4);
        assert!(!m.is_active(t0));
        m.retire(t0);
        assert!(!m.is_visible());
    }
}
