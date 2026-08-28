//! Fluent 2 motion curves and durations (UI/UX v3 P3a).
//!
//! Values are transcribed from the Fluent UI implementation repository —
//! `microsoft/fluentui`, `packages/tokens/src/global/curves.ts` and
//! `durations.ts`. The Fluent 2 design site documents motion qualitatively
//! and publishes no token values, so the implementation repo is the source
//! of truth here. Do not re-derive these by eye.
//!
//! All nine curves are defined even though P3a uses two: a partial copy of
//! an external table invites a later change to guess at a missing constant.
//! They are `const fn` data with no runtime cost.

/// Animation durations from the Fluent 2 token set, in milliseconds.
///
/// `dead_code` is allowed for the module as a whole: this is a verbatim
/// transcription of an external table, and the steps P3a does not consume
/// yet are consumed by P3b. Silencing them individually as they are picked
/// up would churn this file for no gain.
#[allow(dead_code)]
pub mod duration {
    /// Checkbox tick, toggle snap.
    pub const ULTRA_FAST: u32 = 50;
    /// Button press feedback.
    pub const FASTER: u32 = 100;
    /// Small control state changes.
    pub const FAST: u32 = 150;
    /// Panel slide, card expand.
    pub const NORMAL: u32 = 200;
    /// Slightly softer than `NORMAL`.
    pub const GENTLE: u32 = 250;
    /// Dialog entrance, page transition.
    pub const SLOW: u32 = 300;
    /// Large-surface movement.
    pub const SLOWER: u32 = 400;
    /// Full-screen morph.
    pub const ULTRA_SLOW: u32 = 500;
}

/// A Fluent 2 easing curve, expressed as a CSS-style cubic bezier with
/// `P0 = (0, 0)` and `P3 = (1, 1)`.
///
/// Accelerate curves start slow and leave quickly (use them for exits);
/// decelerate curves arrive quickly and settle (use them for entrances).
///
/// `dead_code` is allowed for the same reason as `duration` above: the
/// table is transcribed whole, and the variants P3a does not construct are
/// P3b's to construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Curve {
    /// No easing. Progress bars and spinners.
    Linear,
    AccelerateMax,
    AccelerateMid,
    AccelerateMin,
    DecelerateMax,
    DecelerateMid,
    DecelerateMin,
    EasyEaseMax,
    EasyEase,
}

impl Curve {
    /// The `(x1, y1, x2, y2)` control points, matching the CSS
    /// `cubic-bezier()` argument order.
    pub const fn control_points(self) -> (f32, f32, f32, f32) {
        match self {
            Curve::Linear => (0.0, 0.0, 1.0, 1.0),
            Curve::AccelerateMax => (0.9, 0.1, 1.0, 0.2),
            Curve::AccelerateMid => (1.0, 0.0, 1.0, 1.0),
            Curve::AccelerateMin => (0.8, 0.0, 0.78, 1.0),
            Curve::DecelerateMax => (0.1, 0.9, 0.2, 1.0),
            Curve::DecelerateMid => (0.0, 0.0, 0.0, 1.0),
            Curve::DecelerateMin => (0.33, 0.0, 0.1, 1.0),
            Curve::EasyEaseMax => (0.8, 0.0, 0.2, 1.0),
            Curve::EasyEase => (0.33, 0.0, 0.67, 1.0),
        }
    }

    /// Map elapsed-time fraction `t` to eased progress, both in `[0, 1]`.
    pub fn eval(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if self == Curve::Linear {
            return t;
        }
        let (x1, y1, x2, y2) = self.control_points();
        let s = solve_for_x(x1, x2, t);
        axis(y1, y2, s).clamp(0.0, 1.0)
    }

    /// The `t` whose eased value is `value` — the inverse of [`Curve::eval`].
    ///
    /// Used to resume an interrupted animation from the value already on
    /// screen. `eval` is monotone for every curve in this table, so a plain
    /// bisection is exact enough and cannot diverge.
    pub fn invert(self, value: f32) -> f32 {
        let value = value.clamp(0.0, 1.0);
        if self == Curve::Linear {
            return value;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if self.eval(mid) < value {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }
}

/// One axis of a cubic bezier with the endpoints pinned to 0 and 1.
fn axis(p1: f32, p2: f32, s: f32) -> f32 {
    let u = 1.0 - s;
    3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s
}

/// Derivative of [`axis`] with respect to `s`.
fn axis_derivative(p1: f32, p2: f32, s: f32) -> f32 {
    let u = 1.0 - s;
    3.0 * u * u * p1 + 6.0 * u * s * (p2 - p1) + 3.0 * s * s * (1.0 - p2)
}

/// Find the curve parameter `s` with `X(s) = t`.
///
/// Newton-Raphson seeded at `s = t` converges in a couple of iterations for
/// the well-conditioned curves. `AccelerateMid` and `DecelerateMid` have a
/// zero X-derivative at an endpoint, where Newton stalls or steps outside
/// `[0, 1]`; bisection then finishes the job. X is monotone in `s` for every
/// curve in the table (all control points lie in `[0, 1]`), so bisection
/// always converges.
fn solve_for_x(x1: f32, x2: f32, t: f32) -> f32 {
    const EPSILON: f32 = 1e-6;

    let mut s = t;
    for _ in 0..8 {
        let err = axis(x1, x2, s) - t;
        if err.abs() < EPSILON {
            return s;
        }
        let d = axis_derivative(x1, x2, s);
        if d.abs() < EPSILON {
            break;
        }
        s -= err / d;
        if !(0.0..=1.0).contains(&s) {
            break;
        }
    }

    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if axis(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every curve this project uses.
    const ALL: [Curve; 9] = [
        Curve::Linear,
        Curve::AccelerateMax,
        Curve::AccelerateMid,
        Curve::AccelerateMin,
        Curve::DecelerateMax,
        Curve::DecelerateMid,
        Curve::DecelerateMin,
        Curve::EasyEaseMax,
        Curve::EasyEase,
    ];

    #[test]
    fn every_curve_starts_at_0_and_ends_at_1() {
        for c in ALL {
            assert!(c.eval(0.0).abs() < 1e-3, "{c:?} at 0");
            assert!((c.eval(1.0) - 1.0).abs() < 1e-3, "{c:?} at 1");
        }
    }

    #[test]
    fn every_curve_is_monotonically_increasing() {
        for c in ALL {
            let mut prev = -1.0;
            for i in 0..=100 {
                let v = c.eval(i as f32 / 100.0);
                assert!(v >= prev - 1e-4, "{c:?} dipped at t={}", i as f32 / 100.0);
                prev = v;
            }
        }
    }

    #[test]
    fn linear_is_the_identity() {
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            assert!((Curve::Linear.eval(t) - t).abs() < 1e-6);
        }
    }

    #[test]
    fn out_of_range_inputs_clamp() {
        for c in ALL {
            assert!(c.eval(-1.0).abs() < 1e-3, "{c:?} below 0");
            assert!((c.eval(2.0) - 1.0).abs() < 1e-3, "{c:?} above 1");
        }
    }

    /// `EasyEaseMax` (0.8, 0, 0.2, 1) and `EasyEase` (0.33, 0, 0.67, 1) are
    /// both point-symmetric about (0.5, 0.5) — x2 = 1-x1 and y2 = 1-y1 — so
    /// their midpoint is exactly 0.5. This is the one closed-form value the
    /// solver can be checked against without a reference implementation.
    #[test]
    fn symmetric_curves_pass_through_their_midpoint() {
        assert!((Curve::EasyEaseMax.eval(0.5) - 0.5).abs() < 1e-3);
        assert!((Curve::EasyEase.eval(0.5) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn accelerate_lags_linear_and_decelerate_leads_it() {
        assert!(Curve::AccelerateMax.eval(0.5) < 0.5);
        assert!(Curve::DecelerateMax.eval(0.5) > 0.5);
    }

    /// `AccelerateMid` (1, 0, 1, 1) has a zero X-derivative at t=1 and
    /// `DecelerateMid` (0, 0, 0, 1) has one at t=0. Newton-Raphson stalls
    /// there; these two exist to exercise the bisection fallback directly.
    #[test]
    fn degenerate_curves_still_solve() {
        for c in [Curve::AccelerateMid, Curve::DecelerateMid] {
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                let v = c.eval(t);
                assert!(v.is_finite(), "{c:?} not finite at {t}");
                assert!((0.0..=1.0).contains(&v), "{c:?} out of range at {t}: {v}");
            }
        }
    }

    #[test]
    fn invert_round_trips_through_eval() {
        for c in ALL {
            for i in 0..=10 {
                let v = i as f32 / 10.0;
                let t = c.invert(v);
                assert!(
                    (c.eval(t) - v).abs() < 1e-2,
                    "{c:?}: invert({v}) = {t}, eval back = {}",
                    c.eval(t)
                );
            }
        }
    }

    #[test]
    fn durations_match_the_fluent_table() {
        assert_eq!(duration::ULTRA_FAST, 50);
        assert_eq!(duration::FASTER, 100);
        assert_eq!(duration::FAST, 150);
        assert_eq!(duration::NORMAL, 200);
        assert_eq!(duration::GENTLE, 250);
        assert_eq!(duration::SLOW, 300);
        assert_eq!(duration::SLOWER, 400);
        assert_eq!(duration::ULTRA_SLOW, 500);
    }
}
