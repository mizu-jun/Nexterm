//! One-shot press feedback (UI/UX v3 P3b3).
//!
//! Three of this client's four pointer models commit their action on
//! mouse-*down*: by the frame the button comes up the tab has switched, the
//! panel row has toggled, or the window is gone. A held "pressed" state has
//! no window to live in, so press is a pulse — full weight at the press
//! instant, zero 100 ms later, independent of the button ever coming up.
//!
//! One `Timed`, where `HoverTransition` needs two. That type's second timer
//! exists so a hand-off decays the outgoing item from the weight it actually
//! held; a press has no hand-off. Pressing a second control simply replaces
//! the first, which is correct: the abandoned control is no longer where the
//! user is looking.

use std::time::Instant;

use nexterm_config::AnimationsConfig;

use super::{Curve, Timed, duration};

/// A decaying press highlight for at most one item of one model.
#[derive(Debug, Clone, Copy)]
pub struct PressPulse<Id> {
    /// The item that was pressed, if any.
    id: Option<Id>,
    /// Runs 0 → 1 while the pulse decays 1 → 0.
    anim: Timed,
}

impl<Id> Default for PressPulse<Id> {
    fn default() -> Self {
        // Born finished. With `id` at `None` every id weighs 0 regardless,
        // so the arbitrary start instant is never read (`Timed` at a zero
        // duration short-circuits before touching it).
        Self {
            id: None,
            anim: Timed::new(Instant::now(), 0, Curve::EasyEase),
        }
    }
}

impl<Id: Copy + PartialEq> PressPulse<Id> {
    /// Fire a pulse on `id`, replacing whatever was decaying before.
    pub fn press(&mut self, id: Id, now: Instant, anim: &AnimationsConfig) {
        self.id = Some(id);
        self.anim = Timed::new(
            now,
            anim.scaled_duration_ms(duration::FASTER),
            Curve::EasyEase,
        );
    }

    /// Press weight for `id` in `[0, 1]`: 1 at the press instant, 0 once the
    /// pulse has run out.
    pub fn weight(&self, id: Id, now: Instant) -> f32 {
        if self.id == Some(id) {
            1.0 - self.anim.progress(now)
        } else {
            0.0
        }
    }

    /// Whether another frame is needed.
    pub fn is_active(&self, now: Instant) -> bool {
        self.id.is_some() && !self.anim.is_done(now)
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
        AnimationsConfig {
            enabled: false,
            ..AnimationsConfig::default()
        }
    }

    /// 100 ms is `duration::FASTER`, the constant every press site uses.
    const MS: u64 = 100;

    #[test]
    fn a_fresh_pulse_weighs_nothing() {
        let p: PressPulse<u32> = PressPulse::default();
        let now = Instant::now();
        assert!(p.weight(1, now).abs() < 1e-4);
        assert!(!p.is_active(now));
    }

    #[test]
    fn a_press_starts_full_and_decays_to_nothing() {
        let mut p: PressPulse<u32> = PressPulse::default();
        let t0 = Instant::now();
        p.press(7, t0, &on());
        assert!((p.weight(7, t0) - 1.0).abs() < 1e-3);
        assert!(p.is_active(t0));
        assert!(p.weight(8, t0).abs() < 1e-4, "another id weighs nothing");
        let done = t0 + Duration::from_millis(MS);
        assert!(p.weight(7, done).abs() < 1e-3);
        assert!(!p.is_active(done));
    }

    /// One slot: pressing a second control drops the first immediately.
    /// Unlike hover there is no hand-off to preserve — the user's attention
    /// has moved, and the abandoned pulse must not keep requesting frames.
    #[test]
    fn a_second_press_replaces_the_first() {
        let mut p: PressPulse<u32> = PressPulse::default();
        let t0 = Instant::now();
        p.press(1, t0, &on());
        let mid = t0 + Duration::from_millis(50);
        assert!(p.weight(1, mid) > 0.1, "first pulse is mid-decay");
        p.press(2, mid, &on());
        assert!(p.weight(1, mid).abs() < 1e-4);
        assert!((p.weight(2, mid) - 1.0).abs() < 1e-3);
    }

    /// Deliberately NOT idempotent, unlike `HoverTransition::retarget`: the
    /// caller fires once per click, not once per pointer-motion frame, so a
    /// double-click must pulse twice rather than continue the first decay.
    #[test]
    fn pressing_the_same_id_again_restarts_the_decay() {
        let mut p: PressPulse<u32> = PressPulse::default();
        let t0 = Instant::now();
        p.press(3, t0, &on());
        let mid = t0 + Duration::from_millis(80);
        assert!(p.weight(3, mid) < 0.5, "decayed most of the way");
        p.press(3, mid, &on());
        assert!((p.weight(3, mid) - 1.0).abs() < 1e-3, "restarted at full");
    }

    /// The config gate. With animations off `scaled_duration_ms` returns 0,
    /// so the pulse is already finished on the frame it is fired and no site
    /// ever renders a pressed appearance.
    #[test]
    fn disabled_animations_never_show_a_pulse() {
        let mut p: PressPulse<u32> = PressPulse::default();
        let t0 = Instant::now();
        p.press(5, t0, &off());
        assert!(p.weight(5, t0).abs() < 1e-4);
        assert!(!p.is_active(t0));
    }
}
