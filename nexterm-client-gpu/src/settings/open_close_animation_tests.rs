//! Tests for `SettingsPanel`'s open/close animation state machine: entrance
//! progress, the logical-vs-visible distinction during the exit fade,
//! reopening mid-fade, and the reduced-motion (disabled animations) path.

use super::*;
use nexterm_config::{AnimationsConfig, AnimationsEnabled};
use std::time::{Duration, Instant};

fn on() -> AnimationsConfig {
    AnimationsConfig::default()
}

fn off() -> AnimationsConfig {
    let mut cfg = AnimationsConfig::default();
    cfg.enabled = AnimationsEnabled::No;
    cfg
}

#[test]
fn a_fresh_panel_is_closed_and_invisible() {
    let sp = SettingsPanel::default();
    assert!(!sp.is_open);
    assert!(!sp.is_visible());
    assert!(sp.eased_progress(Instant::now()).abs() < 1e-4);
}

#[test]
fn open_runs_from_0_to_1_over_the_entrance_duration() {
    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.open(t0, &on());
    assert!(sp.is_open);
    assert!(sp.is_visible());
    assert!(sp.eased_progress(t0).abs() < 1e-3);
    assert!((sp.eased_progress(t0 + Duration::from_millis(200)) - 1.0).abs() < 1e-3);
}

/// `is_open` is the truth for input routing and the AccessKit tree, so
/// dismissing the panel must close it at once. Only the renderer knows
/// about the fade-out.
#[test]
fn close_closes_logically_but_keeps_the_panel_visible() {
    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.open(t0, &on());
    let t1 = t0 + Duration::from_millis(200);
    sp.close(t1, &on());
    assert!(!sp.is_open);
    assert!(sp.is_visible());
    assert!(sp.eased_progress(t1) > 0.9);
}

#[test]
fn the_close_animation_fades_to_0_and_then_stops_being_visible() {
    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.open(t0, &on());
    // Let the entrance finish first: closing a panel that is still at 0
    // produces an exit animation that is born finished, which would make
    // this test pass without exercising the fade at all.
    let opened = t0 + Duration::from_millis(200);
    sp.close(opened, &on());
    assert!(sp.eased_progress(opened) > 0.9);
    let done = opened + Duration::from_millis(150);
    assert!(sp.eased_progress(done).abs() < 1e-3);
    assert!(
        sp.is_visible(),
        "still drawn until the frame loop retires it"
    );
    sp.motion.retire(done);
    assert!(!sp.is_visible());
}

/// Reopening mid-fade must pick up the value already on screen, not
/// snap to 0 and replay the entrance.
#[test]
fn reopening_during_the_fade_out_is_continuous() {
    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.open(t0, &on());
    let opened = t0 + Duration::from_millis(200);
    sp.close(opened, &on());
    let mid = opened + Duration::from_millis(75);
    let before = sp.eased_progress(mid);
    sp.open(mid, &on());
    let after = sp.eased_progress(mid);
    assert!(
        (after - before).abs() < 5e-2,
        "value jumped on reopen: {before} -> {after}"
    );
    // The two samples above are both taken at `mid`, so they cannot tell an
    // entrance from an un-cancelled exit that merely happened to match the
    // value at that instant. Sample later instead: only a resumed entrance
    // rises toward 1.0, while a still-running exit would keep falling
    // toward 0.
    let later = mid + Duration::from_millis(50);
    assert!(
        sp.motion.is_active(mid),
        "the resumed entrance needs frames"
    );
    assert!(
        sp.eased_progress(later) > after,
        "reopening must resume the entrance, not continue the fade-out: {after} -> {}",
        sp.eased_progress(later)
    );
}

/// The reduced-motion path. `scaled_duration_ms` returns 0 when
/// animations are disabled, so both transitions are finished the moment
/// they start.
#[test]
fn disabled_animations_open_and_close_instantly() {
    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.open(t0, &off());
    assert!((sp.eased_progress(t0) - 1.0).abs() < 1e-4);
    sp.close(t0, &off());
    assert!(sp.eased_progress(t0).abs() < 1e-4);
    sp.motion.retire(t0);
    assert!(!sp.is_visible());
}

/// The tooltip has no open flag: `tick_tooltip` turns the dwell predicate
/// into an entrance and an exit.
#[test]
fn the_tooltip_opens_once_the_dwell_is_ready_and_closes_when_it_clears() {
    use super::hover::{HoverDwell, TOOLTIP_DELAY_MS};

    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.hover_widget = Some(HoverDwell::enter(None, 2, 0, t0));

    sp.tick_tooltip(t0, &on());
    assert!(!sp.tooltip_motion.is_visible(), "not yet — still dwelling");

    let ready = t0 + Duration::from_millis(TOOLTIP_DELAY_MS as u64);
    sp.tick_tooltip(ready, &on());
    assert!(sp.tooltip_motion.is_visible());
    assert!(sp.tooltip_motion.progress(ready).abs() < 1e-3);
    let shown = ready + Duration::from_millis(150);
    assert!((sp.tooltip_motion.progress(shown) - 1.0).abs() < 1e-3);

    // The pointer leaves the panel.
    sp.hover_widget = None;
    sp.tick_tooltip(shown, &on());
    assert!(sp.tooltip_motion.is_visible(), "it must fade, not vanish");
    let gone = shown + Duration::from_millis(100);
    assert!(sp.tooltip_motion.progress(gone).abs() < 1e-3);
    sp.tooltip_motion.retire(gone);
    assert!(!sp.tooltip_motion.is_visible());
}

/// Moving to another control restarts the dwell, so the tooltip that was
/// showing must close.
#[test]
fn moving_to_another_control_closes_the_tooltip() {
    use super::hover::{HoverDwell, TOOLTIP_DELAY_MS};

    let mut sp = SettingsPanel::default();
    let t0 = Instant::now();
    sp.hover_widget = Some(HoverDwell::enter(None, 2, 0, t0));
    let ready = t0 + Duration::from_millis(TOOLTIP_DELAY_MS as u64);
    sp.tick_tooltip(ready, &on());
    assert!(sp.tooltip_motion.is_visible());

    sp.hover_widget = Some(HoverDwell::enter(sp.hover_widget, 2, 1, ready));
    sp.tick_tooltip(ready, &on());
    assert!(
        sp.tooltip_motion
            .progress(ready + Duration::from_millis(100))
            .abs()
            < 1e-3
    );
}
