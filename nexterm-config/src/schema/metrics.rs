//! Metric design tokens (UI/UX v3 phase P1a).
//!
//! Where [`super::tokens::DesignTokens`] answers *what color is this chrome
//! element*, `MetricTokens` answers *how big, how round, how deep, and how
//! fast*. Both are derived rather than configured: the renderer builds them
//! once per frame instead of reaching for ad-hoc literals.
//!
//! The values follow the Windows 11 Fluent Design reference ramps (geometry,
//! typography, elevation, motion). They are expressed in **effective pixels**
//! (epx, i.e. DPI-independent units); call [`MetricTokens::scaled`] with the
//! window's `scale_factor()` to obtain physical pixels.
//!
//! Radii are the one exception: they are bridged from the existing
//! [`UiConfig`] knobs so that `corner_radius_chrome` / `corner_radius_overlay`
//! keep working exactly as before. The Fluent reference radii are still
//! exposed as [`RadiusTokens::FLUENT_CONTROL`] / [`RadiusTokens::FLUENT_SURFACE`].

use super::UiConfig;

// ─────────────────────────────────────────────────────────────────────────────
// Spacing
// ─────────────────────────────────────────────────────────────────────────────

/// Fluent content-spacing ramp, in epx.
///
/// Only these five steps should appear in chrome layout code; anything in
/// between reads as accidental.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpacingRamp {
    /// 8 epx — gap between tightly related items (icon ↔ label).
    pub xs: f32,
    /// 12 epx — padding inside small controls.
    pub s: f32,
    /// 16 epx — default padding of a panel or a row.
    pub m: f32,
    /// 32 epx — gap between groups within a page.
    pub l: f32,
    /// 48 epx — gap between major page sections.
    pub xl: f32,
}

impl Default for SpacingRamp {
    fn default() -> Self {
        Self {
            xs: 8.0,
            s: 12.0,
            m: 16.0,
            l: 32.0,
            xl: 48.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Radius
// ─────────────────────────────────────────────────────────────────────────────

/// Corner radii, in epx.
///
/// The effective values come from [`UiConfig`] (default 10 epx for both) so
/// that existing user configs are honoured; the Fluent reference values are
/// available as associated constants for new surfaces that want them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadiusTokens {
    /// In-page controls: buttons, toggles, input fields, tooltips.
    pub control: f32,
    /// Top-level surfaces: overlay panels, dialogs, flyouts.
    pub surface: f32,
}

impl RadiusTokens {
    /// Fluent reference radius for in-page controls (4 epx).
    pub const FLUENT_CONTROL: f32 = 4.0;
    /// Fluent reference radius for top-level surfaces (8 epx).
    pub const FLUENT_SURFACE: f32 = 8.0;

    /// Bridge the legacy [`UiConfig`] radii into token form.
    ///
    /// `corner_radius_chrome` drives controls, `corner_radius_overlay` drives
    /// surfaces. Both are clamped to non-negative by `UiConfig`.
    pub fn from_ui(ui: &UiConfig) -> Self {
        Self {
            control: ui.chrome_radius(),
            surface: ui.overlay_radius(),
        }
    }
}

impl Default for RadiusTokens {
    fn default() -> Self {
        Self::from_ui(&UiConfig::default())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Typography
// ─────────────────────────────────────────────────────────────────────────────

/// One step of the chrome type ramp.
///
/// This ramp applies to chrome only (tabs, panels, dialogs, status bar). The
/// terminal grid keeps its own font configuration and must never be sized
/// from here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeStyle {
    /// Font size in epx.
    pub size: f32,
    /// Line height in epx.
    pub line_height: f32,
    /// OpenType weight class (400 = Regular, 600 = SemiBold).
    pub weight: u16,
}

/// Fluent chrome type ramp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeRamp {
    /// Caption 12/16 Regular — key hints, secondary metadata.
    pub caption: TypeStyle,
    /// Body 14/20 Regular — the chrome default.
    pub body: TypeStyle,
    /// Body Strong 14/20 SemiBold — labels, selected rows.
    pub body_strong: TypeStyle,
    /// Subtitle 20/28 SemiBold — section headers inside a panel.
    pub subtitle: TypeStyle,
    /// Title 28/36 SemiBold — dialog titles.
    pub title: TypeStyle,
}

/// OpenType weight class for Regular.
const WEIGHT_REGULAR: u16 = 400;
/// OpenType weight class for SemiBold.
const WEIGHT_SEMIBOLD: u16 = 600;

impl Default for TypeRamp {
    fn default() -> Self {
        Self {
            caption: TypeStyle {
                size: 12.0,
                line_height: 16.0,
                weight: WEIGHT_REGULAR,
            },
            body: TypeStyle {
                size: 14.0,
                line_height: 20.0,
                weight: WEIGHT_REGULAR,
            },
            body_strong: TypeStyle {
                size: 14.0,
                line_height: 20.0,
                weight: WEIGHT_SEMIBOLD,
            },
            subtitle: TypeStyle {
                size: 20.0,
                line_height: 28.0,
                weight: WEIGHT_SEMIBOLD,
            },
            title: TypeStyle {
                size: 28.0,
                line_height: 36.0,
                weight: WEIGHT_SEMIBOLD,
            },
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Elevation
// ─────────────────────────────────────────────────────────────────────────────

/// Fluent elevation scale, in epx of shadow depth.
///
/// P1a only publishes the numbers; the soft-shadow renderer that consumes them
/// lands in P2. Higher means "floats further above the surface below".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationScale {
    /// 128 — modal dialogs.
    pub dialog: f32,
    /// 32 — flyouts, context menus, the command palette.
    pub flyout: f32,
    /// 16 — tooltips.
    pub tooltip: f32,
    /// 8 — cards and grouped panels.
    pub card: f32,
    /// 2 — resting controls.
    pub control: f32,
    /// 1 — a pressed control sinks toward its surface.
    pub control_pressed: f32,
}

impl Default for ElevationScale {
    fn default() -> Self {
        Self {
            dialog: 128.0,
            flyout: 32.0,
            tooltip: 16.0,
            card: 8.0,
            control: 2.0,
            control_pressed: 1.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Motion
// ─────────────────────────────────────────────────────────────────────────────

/// A cubic Bézier easing curve with fixed endpoints (0,0) and (1,1),
/// parameterised by its two control points — the same form CSS uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    /// x of the first control point.
    pub x1: f32,
    /// y of the first control point.
    pub y1: f32,
    /// x of the second control point.
    pub x2: f32,
    /// y of the second control point.
    pub y2: f32,
}

/// Bisection steps used to invert x(t). 24 steps give ~6e-8 precision, which
/// is far below one frame's worth of visible difference.
const BEZIER_SOLVE_STEPS: u32 = 24;

impl CubicBezier {
    /// Linear (no easing).
    pub const LINEAR: Self = Self {
        x1: 0.0,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };
    /// Fluent **Direct Entrance** `cubic-bezier(0, 0, 0, 1)` — elements
    /// entering the scene with no prior position.
    pub const DIRECT_ENTRANCE: Self = Self {
        x1: 0.0,
        y1: 0.0,
        x2: 0.0,
        y2: 1.0,
    };
    /// Fluent **Existing Elements** `cubic-bezier(0.55, 0.55, 0, 1)` —
    /// something already on screen moving or resizing.
    pub const EXISTING_ELEMENTS: Self = Self {
        x1: 0.55,
        y1: 0.55,
        x2: 0.0,
        y2: 1.0,
    };
    /// Fluent **Gentle Exit** `cubic-bezier(1, 0, 1, 1)` — elements leaving
    /// the scene.
    pub const GENTLE_EXIT: Self = Self {
        x1: 1.0,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };

    /// Evaluate the curve's progress value for a normalised time `t` in
    /// `[0, 1]`. Inputs outside that range are clamped.
    ///
    /// This inverts x(t) by bisection instead of Newton–Raphson: two of the
    /// Fluent curves have a zero derivative at an endpoint, where Newton
    /// stalls.
    pub fn eval(&self, t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        let mut param = x;
        for _ in 0..BEZIER_SOLVE_STEPS {
            if Self::bezier_axis(param, self.x1, self.x2) < x {
                lo = param;
            } else {
                hi = param;
            }
            param = 0.5 * (lo + hi);
        }
        Self::bezier_axis(param, self.y1, self.y2)
    }

    /// One axis of a cubic Bézier with endpoints 0 and 1.
    #[inline]
    fn bezier_axis(t: f32, p1: f32, p2: f32) -> f32 {
        let inv = 1.0 - t;
        3.0 * inv * inv * t * p1 + 3.0 * inv * t * t * p2 + t * t * t
    }
}

/// Fluent motion durations (in milliseconds) and the curve for each role.
///
/// The durations here are *base* values. Scale them through
/// [`super::AnimationsConfig::scaled_duration_ms`] so that the user's
/// intensity setting — including the reduced-motion `off` level — applies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionTokens {
    /// 83 ms — the "bare minimum" fade; also the floor for anything faster.
    pub instant_ms: u32,
    /// 167 ms — small, contained transitions (hover, press, toggle).
    pub fast_ms: u32,
    /// 250 ms — panel and flyout entrances.
    pub normal_ms: u32,
    /// 333 ms — full-surface transitions (dialogs, page changes).
    pub slow_ms: u32,
    /// Curve for elements entering the scene.
    pub entrance: CubicBezier,
    /// Curve for elements already on screen.
    pub existing: CubicBezier,
    /// Curve for elements leaving the scene.
    pub exit: CubicBezier,
}

impl Default for MotionTokens {
    fn default() -> Self {
        Self {
            instant_ms: 83,
            fast_ms: 167,
            normal_ms: 250,
            slow_ms: 333,
            entrance: CubicBezier::DIRECT_ENTRANCE,
            existing: CubicBezier::EXISTING_ELEMENTS,
            exit: CubicBezier::GENTLE_EXIT,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The token set
// ─────────────────────────────────────────────────────────────────────────────

/// The full metric token set consumed by the chrome renderer.
///
/// Obtain it with [`MetricTokens::from_ui`] (bridging the user's radius
/// config) and, if you need physical pixels, [`MetricTokens::scaled`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MetricTokens {
    /// Content spacing ramp.
    pub spacing: SpacingRamp,
    /// Corner radii.
    pub radius: RadiusTokens,
    /// Chrome type ramp (never the terminal grid).
    pub type_ramp: TypeRamp,
    /// Shadow depth scale.
    pub elevation: ElevationScale,
    /// Durations and easing curves.
    pub motion: MotionTokens,
}

impl MetricTokens {
    /// Build the token set, honouring the user's corner-radius config.
    pub fn from_ui(ui: &UiConfig) -> Self {
        Self {
            radius: RadiusTokens::from_ui(ui),
            ..Self::default()
        }
    }

    /// Return a copy with every length converted from epx to physical pixels.
    ///
    /// Durations, easing curves and font weights are DPI-independent and pass
    /// through untouched. A non-finite or non-positive `scale_factor` is
    /// treated as `1.0`.
    pub fn scaled(&self, scale_factor: f32) -> Self {
        let s = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        Self {
            spacing: SpacingRamp {
                xs: self.spacing.xs * s,
                s: self.spacing.s * s,
                m: self.spacing.m * s,
                l: self.spacing.l * s,
                xl: self.spacing.xl * s,
            },
            radius: RadiusTokens {
                control: self.radius.control * s,
                surface: self.radius.surface * s,
            },
            type_ramp: TypeRamp {
                caption: scale_type(self.type_ramp.caption, s),
                body: scale_type(self.type_ramp.body, s),
                body_strong: scale_type(self.type_ramp.body_strong, s),
                subtitle: scale_type(self.type_ramp.subtitle, s),
                title: scale_type(self.type_ramp.title, s),
            },
            elevation: ElevationScale {
                dialog: self.elevation.dialog * s,
                flyout: self.elevation.flyout * s,
                tooltip: self.elevation.tooltip * s,
                card: self.elevation.card * s,
                control: self.elevation.control * s,
                control_pressed: self.elevation.control_pressed * s,
            },
            motion: self.motion,
        }
    }
}

/// Scale a type style's lengths, keeping its weight.
#[inline]
fn scale_type(style: TypeStyle, s: f32) -> TypeStyle {
    TypeStyle {
        size: style.size * s,
        line_height: style.line_height * s,
        weight: style.weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{AnimationIntensity, AnimationsConfig};

    /// Tolerance for comparing eased progress values.
    const EPS: f32 = 1e-4;

    #[test]
    fn spacing_matches_the_fluent_ramp() {
        let s = SpacingRamp::default();
        assert_eq!([s.xs, s.s, s.m, s.l, s.xl], [8.0, 12.0, 16.0, 32.0, 48.0]);
    }

    #[test]
    fn radius_bridges_the_existing_ui_config() {
        let ui = UiConfig {
            corner_radius_chrome: 6.0,
            corner_radius_overlay: 14.0,
        };
        let r = RadiusTokens::from_ui(&ui);
        assert_eq!(r.control, 6.0);
        assert_eq!(r.surface, 14.0);
    }

    #[test]
    fn radius_inherits_the_ui_config_clamping() {
        let ui = UiConfig {
            corner_radius_chrome: -3.0,
            corner_radius_overlay: -1.0,
        };
        let r = RadiusTokens::from_ui(&ui);
        assert_eq!(r.control, 0.0);
        assert_eq!(r.surface, 0.0);
    }

    #[test]
    fn default_radius_keeps_the_shipped_10_px_look() {
        // Backward compatibility: the token default must not silently retheme
        // existing installs to the Fluent 4/8 values.
        let r = RadiusTokens::default();
        assert_eq!(r.control, 10.0);
        assert_eq!(r.surface, 10.0);
        assert_eq!(RadiusTokens::FLUENT_CONTROL, 4.0);
        assert_eq!(RadiusTokens::FLUENT_SURFACE, 8.0);
    }

    #[test]
    fn type_ramp_matches_the_fluent_steps() {
        let t = TypeRamp::default();
        assert_eq!((t.caption.size, t.caption.line_height), (12.0, 16.0));
        assert_eq!((t.body.size, t.body.line_height), (14.0, 20.0));
        assert_eq!((t.subtitle.size, t.subtitle.line_height), (20.0, 28.0));
        assert_eq!((t.title.size, t.title.line_height), (28.0, 36.0));
        // Body and Body Strong differ only in weight.
        assert_eq!(t.body.size, t.body_strong.size);
        assert_eq!(t.body.weight, WEIGHT_REGULAR);
        assert_eq!(t.body_strong.weight, WEIGHT_SEMIBOLD);
    }

    #[test]
    fn elevation_is_monotonically_ordered() {
        let e = ElevationScale::default();
        assert!(e.dialog > e.flyout);
        assert!(e.flyout > e.tooltip);
        assert!(e.tooltip > e.card);
        assert!(e.card > e.control);
        assert!(e.control > e.control_pressed);
    }

    #[test]
    fn motion_durations_match_the_fluent_ramp() {
        let m = MotionTokens::default();
        assert_eq!(
            [m.instant_ms, m.fast_ms, m.normal_ms, m.slow_ms],
            [83, 167, 250, 333]
        );
    }

    #[test]
    fn bezier_endpoints_are_exact() {
        for curve in [
            CubicBezier::LINEAR,
            CubicBezier::DIRECT_ENTRANCE,
            CubicBezier::EXISTING_ELEMENTS,
            CubicBezier::GENTLE_EXIT,
        ] {
            assert_eq!(curve.eval(0.0), 0.0);
            assert_eq!(curve.eval(1.0), 1.0);
        }
    }

    #[test]
    fn bezier_clamps_out_of_range_input() {
        let c = CubicBezier::EXISTING_ELEMENTS;
        assert_eq!(c.eval(-1.0), 0.0);
        assert_eq!(c.eval(2.0), 1.0);
    }

    #[test]
    fn linear_bezier_is_the_identity() {
        for &t in &[0.1f32, 0.25, 0.5, 0.75, 0.9] {
            assert!((CubicBezier::LINEAR.eval(t) - t).abs() < EPS);
        }
    }

    #[test]
    fn bezier_is_monotonically_increasing() {
        for curve in [
            CubicBezier::DIRECT_ENTRANCE,
            CubicBezier::EXISTING_ELEMENTS,
            CubicBezier::GENTLE_EXIT,
        ] {
            let mut prev = 0.0;
            for step in 0..=50 {
                let v = curve.eval(step as f32 / 50.0);
                assert!(v >= prev - EPS, "curve {curve:?} went backwards at {step}");
                prev = v;
            }
        }
    }

    #[test]
    fn entrance_curve_front_loads_and_exit_curve_back_loads() {
        // Direct Entrance decelerates: it is already past halfway at t = 0.5.
        assert!(CubicBezier::DIRECT_ENTRANCE.eval(0.5) > 0.5);
        // Gentle Exit accelerates: it is still below halfway at t = 0.5.
        assert!(CubicBezier::GENTLE_EXIT.eval(0.5) < 0.5);
    }

    #[test]
    fn scaled_multiplies_lengths_but_not_durations() {
        let t = MetricTokens::default().scaled(2.0);
        assert_eq!(t.spacing.m, 32.0);
        assert_eq!(t.radius.surface, 20.0);
        assert_eq!(t.type_ramp.body.size, 28.0);
        assert_eq!(t.type_ramp.body.weight, WEIGHT_REGULAR);
        assert_eq!(t.elevation.flyout, 64.0);
        assert_eq!(t.motion.normal_ms, 250);
    }

    #[test]
    fn scaled_rejects_a_degenerate_factor() {
        let base = MetricTokens::default();
        for bad in [0.0f32, -1.5, f32::NAN, f32::INFINITY] {
            assert_eq!(base.scaled(bad).spacing.m, base.spacing.m);
        }
    }

    #[test]
    fn from_ui_only_overrides_the_radii() {
        let ui = UiConfig {
            corner_radius_chrome: 4.0,
            corner_radius_overlay: 8.0,
        };
        let t = MetricTokens::from_ui(&ui);
        assert_eq!(t.radius.control, 4.0);
        assert_eq!(t.radius.surface, 8.0);
        assert_eq!(t.spacing, SpacingRamp::default());
        assert_eq!(t.motion, MotionTokens::default());
    }

    #[test]
    fn durations_compose_with_the_animation_intensity() {
        // Reduced motion must still be able to zero out the token durations.
        let m = MotionTokens::default();
        let off = AnimationsConfig {
            enabled: false,
            intensity: AnimationIntensity::Normal,
        };
        assert_eq!(off.scaled_duration_ms(m.normal_ms), 0);
        let subtle = AnimationsConfig {
            enabled: true,
            intensity: AnimationIntensity::Subtle,
        };
        assert_eq!(subtle.scaled_duration_ms(m.normal_ms), 125);
    }
}
