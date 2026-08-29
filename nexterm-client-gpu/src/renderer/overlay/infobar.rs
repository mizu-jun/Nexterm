//! The InfoBar stack — model and pure layout (UI/UX v3 P6a).
//!
//! Nexterm shipped three top-of-screen banners (update / offline / error),
//! each with its own state field, its own builder and its own hand-written
//! stacking arithmetic, so a fourth message type meant editing three
//! functions and the error banner re-derived its own `y` by testing the
//! other two. This module is the single surface that replaces them: one
//! kind enum, one stack, and one function that computes where a bar sits.
//!
//! Everything here is pure — no GPU handles, no font state, no clock of its
//! own — so the ordering, the cap and the count suffix are unit-testable
//! without a device. P6b removed the three `Option` fields and the three
//! builders and made this the only stack; P6c gave every kind an AccessKit
//! node (`accessibility::build_info_bar_nodes`) and the cap its `+{count}
//! more` suffix; P6d gave every bar an entrance, an exit and — for the info
//! severity only — a deadline that dismisses it on its own.
//!
//! The structural gate for the phase (G-single) is that [`bar_rects`] stays
//! the only function that computes a bar's `y`.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::animations::Timed;
use crate::renderer::overlay::widgets::spec::WidgetRect;

/// Height of one bar, as a multiple of the cell height.
///
/// Matches the `cell_h * 1.4` the three legacy banners each open-coded, so
/// the migration in P6b is not also a size change.
const BAR_HEIGHT_FACTOR: f32 = 1.4;

/// How many bars are drawn at once.
///
/// The stack overlays terminal content that is neither reflowed nor
/// scrolled (D2), so it must not grow without bound. Bars past the cap are
/// counted rather than drawn, and the bottom one reports them.
pub const MAX_VISIBLE_BARS: usize = 2;

/// How long an informational bar stays up before it dismisses itself.
///
/// Only the `info` severity auto-dismisses (D3); the timer is applied in
/// P6d, but the policy belongs to the kind rather than to the renderer.
pub const INFO_BAR_TTL: Duration = Duration::from_secs(20);

/// How loud a bar is — the primary ordering key of the stack.
///
/// The variants are declared quietest-first so the derived `Ord` sorts an
/// error above a warning above a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Purely informational; the user asked for nothing and lost nothing.
    Info,
    /// Something is degraded but is expected to resolve on its own.
    Warning,
    /// Something the user asked for did not happen.
    Error,
}

/// Which single-slot surface a kind occupies.
///
/// Each of the three migrated banners was a single `Option` field: a second
/// update notice replaced the first rather than stacking, and the error slot
/// was explicitly documented as "overwritten by the latest error (never
/// stacks)". The stack keeps that, so the slot is the identity a call site
/// queues, replaces and clears by — and, being fieldless, it is also what the
/// accessibility tree hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InfoBarSlot {
    /// A newer release is available.
    Update,
    /// The client has not yet reached the server.
    Offline,
    /// The last error the server reported.
    ServerError,
}

/// What a bar is about.
///
/// Adding a message type costs one arm here rather than a fourth builder,
/// which is the whole point of the consolidation (D1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfoBarKind {
    /// A newer release is available; `Enter` opens its page.
    UpdateAvailable {
        /// Version string as reported by the GitHub Releases API.
        version: String,
    },
    /// The client has not yet reached the server.
    Offline {
        /// When the connection first started failing, for the elapsed count.
        since: Instant,
    },
    /// A `ServerToClient::Error` — PTY launch, config load, split failure.
    ServerError {
        /// The server's message, shown verbatim after the localised prefix.
        message: String,
    },
}

impl InfoBarKind {
    /// The single slot this kind occupies.
    pub fn slot(&self) -> InfoBarSlot {
        match self {
            InfoBarKind::UpdateAvailable { .. } => InfoBarSlot::Update,
            InfoBarKind::Offline { .. } => InfoBarSlot::Offline,
            InfoBarKind::ServerError { .. } => InfoBarSlot::ServerError,
        }
    }

    /// How loud this kind is.
    pub fn severity(&self) -> Severity {
        match self {
            InfoBarKind::UpdateAvailable { .. } => Severity::Info,
            InfoBarKind::Offline { .. } => Severity::Warning,
            InfoBarKind::ServerError { .. } => Severity::Error,
        }
    }

    /// The accent colour of the bar, from the severity's semantic token.
    ///
    /// Exhaustive over the enum, so the colour choice stops being
    /// open-coded once per builder. The label colour is *not* derived from
    /// this: a banner's ground is a tint between `surface_2` and the accent,
    /// so callers still correct against the ground `draw_banner_bg` returns
    /// (UI/UX v3 P5b).
    pub fn accent(&self, tokens: &nexterm_config::DesignTokens) -> [f32; 4] {
        match self.severity() {
            Severity::Info => tokens.semantic_success,
            Severity::Warning => tokens.semantic_warning,
            Severity::Error => tokens.semantic_error,
        }
    }

    /// Whether `Enter` does anything while this kind is the top bar (D4).
    pub fn has_activation(&self) -> bool {
        matches!(self, InfoBarKind::UpdateAvailable { .. })
    }

    /// Whether `Esc` can dismiss this kind.
    ///
    /// The offline bar is the exception and always has been: its whole content
    /// is "still not connected", so dismissing it would only hide a condition
    /// that is still true. It clears when the connection succeeds.
    pub fn is_dismissible(&self) -> bool {
        !matches!(self, InfoBarKind::Offline { .. })
    }

    /// The localised one-line message this kind shows.
    ///
    /// `now` is passed rather than read so the offline bar's elapsed count
    /// stays a function of its arguments. The label lives with the kind rather
    /// than with the builder because P6c's AccessKit node needs the same text.
    pub fn label(&self, now: Instant) -> String {
        match self {
            InfoBarKind::UpdateAvailable { version } => {
                nexterm_i18n::fl!("update-available").replace("{version}", version)
            }
            InfoBarKind::Offline { since } => {
                let elapsed = now.saturating_duration_since(*since).as_secs();
                nexterm_i18n::fl!("offline-banner-connecting")
                    .replace("{seconds}", &elapsed.to_string())
            }
            InfoBarKind::ServerError { message } => {
                format!("{} {}", nexterm_i18n::fl!("error-banner-prefix"), message)
            }
        }
    }

    /// How long this kind lives before dismissing itself, if at all (D3).
    ///
    /// An error reports that something the user asked for did not happen and
    /// must never disappear on a timer; the offline bar's whole content is
    /// "still not connected", so it ends when that ends.
    pub fn auto_dismiss_after(&self) -> Option<Duration> {
        match self.severity() {
            Severity::Info => Some(INFO_BAR_TTL),
            Severity::Warning | Severity::Error => None,
        }
    }
}

/// A non-blocking status message. Fluent's InfoBar, not its Dialog or Flyout.
#[derive(Debug, Clone)]
pub struct InfoBar {
    /// What the bar is about; carries its severity and its colour.
    pub kind: InfoBarKind,
    /// When the bar was queued. The secondary ordering key, and the reason
    /// this field exists rather than reusing `entrance`: a `Timed` does not
    /// expose its start, and ordering must not depend on the animation
    /// config — with animations off every entrance is born finished, which
    /// would leave same-severity bars in an arbitrary order.
    pub created_at: Instant,
    /// Entrance animation (P6d wires it; born finished when motion is off).
    pub entrance: Timed,
    /// `Some` once the bar has been dismissed and is only being drawn out.
    pub exit: Option<Timed>,
    /// Wall-clock deadline for an auto-dismissing kind.
    pub expires_at: Option<Instant>,
}

impl InfoBar {
    /// Queue a bar at `now`, taking its dismissal policy from its kind.
    pub fn new(kind: InfoBarKind, now: Instant, entrance: Timed) -> Self {
        let expires_at = kind.auto_dismiss_after().and_then(|d| now.checked_add(d));
        Self {
            kind,
            created_at: now,
            entrance,
            exit: None,
            expires_at,
        }
    }

    /// Whether the auto-dismiss deadline has passed.
    pub fn is_expired(&self, now: Instant) -> bool {
        self.expires_at.is_some_and(|deadline| now >= deadline)
    }

    /// Whether the bar has been dismissed and is only being drawn out.
    ///
    /// A dismissed bar is gone as far as everything except the renderer is
    /// concerned: it does not take an `Esc`, it does not hold its slot
    /// against a fresh message, and it leaves the AccessKit tree at once
    /// rather than announcing a bar the user just closed.
    pub fn is_dismissed(&self) -> bool {
        self.exit.is_some()
    }

    /// Whether the bar is dismissed and its exit has finished drawing.
    ///
    /// `ClientState::retire_info_bars` drops bars that answer `true`; a bar
    /// that animates but never retires keeps the event loop requesting frames
    /// forever (G-idle).
    pub fn is_retired(&self, now: Instant) -> bool {
        self.exit.is_some_and(|exit| exit.is_done(now))
    }

    /// How opaque the bar is right now, in `[0, 1]`.
    ///
    /// The exit counts *down* from whatever the entrance had reached, which
    /// is why it is one expression rather than two: a bar dismissed while it
    /// is still fading in must not jump to fully opaque first. With motion
    /// off both `Timed`s are born finished, so this is 1 while the bar is up
    /// and 0 the instant it is dismissed — the reduced-motion path.
    pub fn visibility(&self, now: Instant) -> f32 {
        match self.exit {
            Some(exit) => 1.0 - exit.progress(now),
            None => self.entrance.progress(now),
        }
    }

    /// Whether the bar still needs another frame.
    pub fn is_animating(&self, now: Instant) -> bool {
        !self.entrance.is_done(now) || self.exit.is_some_and(|exit| !exit.is_done(now))
    }
}

/// Where the stack put each bar for one frame.
pub struct StackLayout {
    /// The drawn bars, top-down: an index into the input slice and its rect.
    ///
    /// At most [`MAX_VISIBLE_BARS`] entries. The index is carried because
    /// the caller has to draw *that* bar's message and, for `Enter`, know
    /// which bar is on top — recomputing the order at the call site is the
    /// second stacking expression G-single forbids.
    pub visible: Vec<(usize, WidgetRect)>,
    /// Bars queued past the cap; the bottom bar reports them as
    /// `infobar-more-count`.
    pub hidden: usize,
}

impl StackLayout {
    /// The localised `+{count} more` suffix, or `None` while the stack fits.
    ///
    /// Lives with the layout that produced the count rather than with the
    /// builder that draws it, so the cap and the way the cap is reported
    /// cannot disagree.
    pub fn more_label(&self) -> Option<String> {
        (self.hidden > 0).then(|| {
            nexterm_i18n::fl!("infobar-more-count").replace("{count}", &self.hidden.to_string())
        })
    }
}

/// Borrow the stack as one contiguous slice, for the functions below.
///
/// `ClientState` holds the stack in a `VecDeque`, which is two slices in the
/// general case; every consumer of [`stack_order`] and [`bar_rects`] needs one.
/// It is contiguous in practice — the stack holds at most one bar per slot and
/// never wraps in a run that short — so the copy is a cold path kept for
/// correctness rather than a per-frame allocation.
pub fn contiguous(bars: &VecDeque<InfoBar>) -> Cow<'_, [InfoBar]> {
    let (front, back) = bars.as_slices();
    if back.is_empty() {
        Cow::Borrowed(front)
    } else {
        Cow::Owned(front.iter().chain(back).cloned().collect())
    }
}

/// The stack's order, loudest first: indices into `bars`, top-down.
///
/// By severity, then by age, so an error never sits below an update notice and
/// two bars of the same severity keep the order they were queued in. Split out
/// of [`bar_rects`] because the keyboard path has to know which bar is on top
/// (D4) and has no surface size to lay anything out with — recomputing the
/// order there is exactly the second expression this module exists to prevent.
pub fn stack_order(bars: &[InfoBar]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..bars.len()).collect();
    // `sort_by` is stable, so bars queued in the same instant keep their
    // insertion order.
    order.sort_by(|&a, &b| {
        bars[b]
            .kind
            .severity()
            .cmp(&bars[a].kind.severity())
            .then(bars[a].created_at.cmp(&bars[b].created_at))
    });
    order
}

/// The bar `Enter` acts on: the loudest one still on screen (D4).
///
/// A bar that is fading out is skipped — the user has already dismissed it,
/// and a key aimed at the bar underneath must not land on a ghost (P6d).
pub fn top_live(bars: &[InfoBar]) -> Option<usize> {
    stack_order(bars)
        .into_iter()
        .find(|&index| !bars[index].is_dismissed())
}

/// The bar `Esc` acts on: the loudest one that can be dismissed at all.
///
/// This is what keeps the pre-P6 ordering, where the error banner cleared
/// before the update one, without either handler knowing the other exists.
/// The offline bar is skipped rather than dismissed: it reports a condition
/// that is still true.
pub fn top_dismissible(bars: &[InfoBar]) -> Option<usize> {
    stack_order(bars)
        .into_iter()
        .find(|&index| !bars[index].is_dismissed() && bars[index].kind.is_dismissible())
}

/// Lay the stack out: the rect of each visible bar, top-down, below the tab bar.
///
/// The only function in the crate that computes a bar's `y` (G-single).
/// Ordering is by severity, then by age, so an error never sits below an
/// update notice and two bars of the same severity keep the order they were
/// queued in. Bars past [`MAX_VISIBLE_BARS`] are reported in
/// [`StackLayout::hidden`] rather than drawn (G-cap).
///
/// `width` is the surface width; the design spec's sketch omitted it, but a
/// full-width bar still needs a rect rather than a bare `y`, and returning
/// the rect keeps the arithmetic here instead of half here and half at the
/// call site. A non-positive `width` or `cell_h` yields an empty stack — the
/// same "a collapsed rectangle draws and hits nothing" rule `WidgetRect`
/// already follows.
pub fn bar_rects(bars: &[InfoBar], tab_bar_h: f32, cell_h: f32, width: f32) -> StackLayout {
    if bars.is_empty() || width <= 0.0 || cell_h <= 0.0 {
        return StackLayout {
            visible: Vec::new(),
            hidden: 0,
        };
    }

    let order = stack_order(bars);
    let bar_h = cell_h * BAR_HEIGHT_FACTOR;
    let hidden = bars.len().saturating_sub(MAX_VISIBLE_BARS);
    let visible = order
        .into_iter()
        .take(MAX_VISIBLE_BARS)
        .enumerate()
        .map(|(slot, index)| {
            let y = tab_bar_h + bar_h * slot as f32;
            (index, WidgetRect::new(0.0, y, width, bar_h))
        })
        .collect();

    StackLayout { visible, hidden }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animations::Curve;

    const TAB_BAR_H: f32 = 32.0;
    const CELL_H: f32 = 20.0;
    const WIDTH: f32 = 800.0;
    const BAR_H: f32 = CELL_H * BAR_HEIGHT_FACTOR;

    fn bar_at(kind: InfoBarKind, now: Instant) -> InfoBar {
        InfoBar::new(kind, now, Timed::new(now, 0, Curve::Linear))
    }

    fn update(now: Instant) -> InfoBar {
        bar_at(
            InfoBarKind::UpdateAvailable {
                version: "1.9.0".to_string(),
            },
            now,
        )
    }

    fn offline(now: Instant) -> InfoBar {
        bar_at(InfoBarKind::Offline { since: now }, now)
    }

    fn error(now: Instant) -> InfoBar {
        bar_at(
            InfoBarKind::ServerError {
                message: "pty launch failed".to_string(),
            },
            now,
        )
    }

    #[test]
    fn an_empty_stack_lays_nothing_out() {
        let layout = bar_rects(&[], TAB_BAR_H, CELL_H, WIDTH);
        assert!(layout.visible.is_empty());
        assert_eq!(layout.hidden, 0);
        assert_eq!(top_live(&[]), None);
    }

    /// D2: the stack overlays terminal content, but it starts *below* the tab
    /// bar — chrome hiding chrome is the worse of the two costs.
    #[test]
    fn the_first_bar_sits_directly_below_the_tab_bar() {
        let now = Instant::now();
        let layout = bar_rects(&[update(now)], TAB_BAR_H, CELL_H, WIDTH);
        let (index, rect) = layout.visible[0];
        assert_eq!(index, 0);
        assert_eq!(rect, WidgetRect::new(0.0, TAB_BAR_H, WIDTH, BAR_H));
    }

    #[test]
    fn bars_stack_downward_without_a_gap() {
        let now = Instant::now();
        let bars = [error(now), update(now)];
        let layout = bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH);
        assert_eq!(layout.visible[0].1.y, TAB_BAR_H);
        assert_eq!(layout.visible[1].1.y, TAB_BAR_H + BAR_H);
        assert!(layout.visible.iter().all(|(_, r)| r.h == BAR_H));
    }

    /// G-order.
    #[test]
    fn an_error_queued_last_is_laid_out_above_an_update() {
        let now = Instant::now();
        let bars = [update(now), error(now + Duration::from_secs(1))];
        let layout = bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH);
        assert_eq!(top_live(&bars), Some(1));
        assert_eq!(layout.visible[1].0, 0);
    }

    #[test]
    fn severity_orders_error_above_warning_above_info() {
        let now = Instant::now();
        // Queued quietest-first, so only severity can produce the order.
        let bars = [update(now), offline(now), error(now)];
        let layout = bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH);
        assert_eq!(layout.visible[0].0, 2);
        assert_eq!(layout.visible[1].0, 1);
    }

    #[test]
    fn bars_of_equal_severity_keep_the_oldest_on_top() {
        let now = Instant::now();
        let older = error(now);
        let newer = error(now + Duration::from_millis(1));
        // Insertion order reversed, so age alone decides.
        assert_eq!(top_live(&[newer, older]), Some(1));
    }

    /// G-cap: §1.5's unbounded stack is the defect this closes.
    #[test]
    fn at_most_two_bars_are_drawn_and_the_rest_are_counted() {
        let now = Instant::now();
        let bars = [error(now), offline(now), update(now)];
        let layout = bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH);
        assert_eq!(layout.visible.len(), MAX_VISIBLE_BARS);
        assert_eq!(layout.hidden, 1);
        // The dropped bar is the quietest one, not an arbitrary one.
        assert!(!layout.visible.iter().any(|&(index, _)| index == 2));
    }

    #[test]
    fn the_hidden_count_is_zero_while_the_stack_fits() {
        let now = Instant::now();
        assert_eq!(bar_rects(&[error(now)], TAB_BAR_H, CELL_H, WIDTH).hidden, 0);
        let bars = [error(now), update(now)];
        assert_eq!(bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH).hidden, 0);
    }

    #[test]
    fn a_collapsed_surface_lays_nothing_out() {
        let now = Instant::now();
        let bars = [error(now)];
        assert!(bar_rects(&bars, TAB_BAR_H, CELL_H, 0.0).visible.is_empty());
        assert!(bar_rects(&bars, TAB_BAR_H, 0.0, WIDTH).visible.is_empty());
    }

    /// D4: `Enter` acts on the top bar, and only if its kind has an
    /// activation — so it does nothing while an error sits above the update
    /// notice.
    #[test]
    fn only_the_top_bar_carries_an_activation() {
        let now = Instant::now();
        let bars = [update(now), error(now)];
        let top = &bars[top_live(&bars).expect("a non-empty stack has a top bar")];
        assert!(!top.kind.has_activation());

        let bars = [update(now)];
        let top = &bars[top_live(&bars).expect("a non-empty stack has a top bar")];
        assert!(top.kind.has_activation());
    }

    /// D3: only the info severity auto-dismisses.
    #[test]
    fn only_the_informational_bar_expires() {
        let now = Instant::now();
        let long_after = now + INFO_BAR_TTL + Duration::from_secs(1);

        assert!(update(now).is_expired(long_after));
        assert!(!update(now).is_expired(now));
        assert!(!offline(now).is_expired(long_after));
        assert!(!error(now).is_expired(long_after));
    }

    #[test]
    fn a_bar_is_retired_only_once_its_exit_has_finished() {
        let now = Instant::now();
        let mut bar = error(now);
        assert!(!bar.is_retired(now));

        bar.exit = Some(Timed::new(now, 200, Curve::AccelerateMax));
        assert!(!bar.is_retired(now));
        assert!(bar.is_retired(now + Duration::from_millis(200)));
    }

    /// P6d: the bar fades in from nothing and out to nothing, and a bar with
    /// motion off is simply opaque — the same reduced-motion path every other
    /// overlay takes.
    #[test]
    fn visibility_rises_with_the_entrance_and_falls_with_the_exit() {
        let now = Instant::now();
        let mut bar = InfoBar::new(
            InfoBarKind::ServerError {
                message: "boom".to_string(),
            },
            now,
            Timed::new(now, 200, Curve::Linear),
        );
        assert!(bar.visibility(now) < 1e-3);
        assert!((bar.visibility(now + Duration::from_millis(200)) - 1.0).abs() < 1e-3);

        let dismissed_at = now + Duration::from_millis(200);
        bar.exit = Some(Timed::new(dismissed_at, 200, Curve::Linear));
        assert!((bar.visibility(dismissed_at) - 1.0).abs() < 1e-3);
        assert!(bar.visibility(dismissed_at + Duration::from_millis(200)) < 1e-3);

        assert!((error(now).visibility(now) - 1.0).abs() < 1e-3);
    }

    /// A dismissed bar is out of everything but the renderer, from the frame
    /// it is dismissed rather than from the frame its exit finishes.
    #[test]
    fn a_bar_counts_as_dismissed_the_moment_its_exit_starts() {
        let now = Instant::now();
        let mut bar = error(now);
        assert!(!bar.is_dismissed());
        bar.exit = Some(Timed::new(now, 200, Curve::AccelerateMax));
        assert!(bar.is_dismissed());
        assert!(!bar.is_retired(now));
    }

    /// G-idle in miniature: with motion off every `Timed` is born finished,
    /// so a freshly queued bar must not ask for another frame.
    #[test]
    fn a_bar_with_no_motion_is_never_animating() {
        let now = Instant::now();
        assert!(!error(now).is_animating(now));

        let animated = InfoBar::new(
            InfoBarKind::Offline { since: now },
            now,
            Timed::new(now, 200, Curve::DecelerateMax),
        );
        assert!(animated.is_animating(now));
        assert!(!animated.is_animating(now + Duration::from_millis(200)));
    }

    /// `Esc` walks the same order the stack is drawn in, so the error bar
    /// clears before the update bar exactly as the two open-coded handlers did.
    #[test]
    fn the_stack_order_matches_the_drawn_order() {
        let now = Instant::now();
        let bars = [update(now), offline(now), error(now)];
        let order = stack_order(&bars);
        assert_eq!(order, vec![2, 1, 0]);

        let layout = bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH);
        let drawn: Vec<usize> = layout.visible.iter().map(|&(index, _)| index).collect();
        assert_eq!(drawn, order[..MAX_VISIBLE_BARS]);
    }

    /// The offline bar reports a condition rather than an event, so `Esc`
    /// skips it and lands on the dismissible bar underneath.
    #[test]
    fn only_the_offline_bar_resists_dismissal() {
        let now = Instant::now();
        assert!(update(now).kind.is_dismissible());
        assert!(error(now).kind.is_dismissible());
        assert!(!offline(now).kind.is_dismissible());

        let bars = [update(now), offline(now)];
        assert_eq!(top_dismissible(&bars), Some(0));
    }

    #[test]
    fn each_kind_occupies_its_own_slot() {
        let now = Instant::now();
        assert_eq!(update(now).kind.slot(), InfoBarSlot::Update);
        assert_eq!(offline(now).kind.slot(), InfoBarSlot::Offline);
        assert_eq!(error(now).kind.slot(), InfoBarSlot::ServerError);
    }

    /// The label is what the migrated builders used to format inline; the
    /// substitution is the part worth pinning, not the translated text.
    #[test]
    fn each_kind_substitutes_its_own_runtime_value() {
        let now = Instant::now();
        let version = update(now).kind.label(now);
        assert!(version.contains("1.9.0"), "got {version}");
        assert!(!version.contains("{version}"));

        let elapsed = offline(now).kind.label(now + Duration::from_secs(7));
        assert!(elapsed.contains('7'), "got {elapsed}");
        assert!(!elapsed.contains("{seconds}"));

        let failure = error(now).kind.label(now);
        assert!(failure.ends_with("pty launch failed"), "got {failure}");
    }

    /// A bar laid out before its `since` — a clock that went backwards, or a
    /// bar drawn in the same frame it was queued — must not panic.
    #[test]
    fn the_offline_label_saturates_rather_than_underflowing() {
        let now = Instant::now() + Duration::from_secs(60);
        let label = offline(now).kind.label(now - Duration::from_secs(30));
        assert!(label.contains('0'), "got {label}");
    }

    /// G-single, as the spec words it: a gate over `ui_verts.rs` that finds no
    /// second stacking expression. The three builders each carried a `bar_y`
    /// and re-derived it from the other two; if one comes back, the stack has
    /// two disagreeing sources of truth again and this fails.
    #[test]
    fn no_second_stacking_expression_survives_in_the_vertex_builders() {
        // The tab bar legitimately has a `bar_y` of its own, so the gate is on
        // the shape the banners used — an offset accumulated across surfaces —
        // rather than on the name.
        let src = include_str!("../ui_verts.rs");
        assert!(
            !src.contains("+= bar_h"),
            "ui_verts.rs accumulates a bar offset again; bar_rects owns that"
        );
        assert!(
            !src.contains("cell_h * 1.4"),
            "ui_verts.rs re-derives the bar height; BAR_HEIGHT_FACTOR owns that"
        );
        assert_eq!(
            src.matches("bar_rects(").count(),
            1,
            "the stack is laid out in exactly one place in ui_verts.rs"
        );
    }

    /// G-cap's other half: the bars past the cap have to be *reported*, and
    /// the count is what the bottom bar draws (G-i18n's one new string).
    #[test]
    fn the_count_suffix_reports_the_bars_the_cap_dropped() {
        let now = Instant::now();
        let bars = [error(now), offline(now), update(now)];
        let label = bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH)
            .more_label()
            .expect("a stack over the cap reports a count");
        assert!(label.contains('1'), "got {label}");
        assert!(!label.contains("{count}"), "got {label}");

        let fits = bar_rects(&bars[..2], TAB_BAR_H, CELL_H, WIDTH);
        assert_eq!(fits.more_label(), None);
    }

    /// The stack is a `VecDeque` on `ClientState`, and both consumers — the
    /// vertex builder and the AccessKit tree — need it as one slice.
    #[test]
    fn a_wrapped_stack_is_flattened_without_reordering() {
        let now = Instant::now();
        let mut deque: VecDeque<InfoBar> = VecDeque::new();
        // Force a wrap: pushing to the front moves the head behind the tail,
        // so the ring is stored as two slices.
        deque.push_back(error(now));
        deque.push_back(update(now));
        deque.push_front(offline(now));
        assert!(!deque.as_slices().1.is_empty(), "the deque did not wrap");

        let flat = contiguous(&deque);
        let slots: Vec<InfoBarSlot> = flat.iter().map(|bar| bar.kind.slot()).collect();
        assert_eq!(
            slots,
            vec![
                InfoBarSlot::Offline,
                InfoBarSlot::ServerError,
                InfoBarSlot::Update
            ]
        );
    }

    #[test]
    fn each_kind_maps_to_its_own_semantic_token() {
        let tokens = nexterm_config::DesignTokens::default();
        let now = Instant::now();
        assert_eq!(update(now).kind.accent(&tokens), tokens.semantic_success);
        assert_eq!(offline(now).kind.accent(&tokens), tokens.semantic_warning);
        assert_eq!(error(now).kind.accent(&tokens), tokens.semantic_error);
    }
}
