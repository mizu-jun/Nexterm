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
//! without a device. The drawing and the call sites arrive in P6b; the
//! AccessKit nodes in P6c; the motion and the auto-dismissal in P6d.
//!
//! The structural gate for the phase (G-single) is that [`bar_rects`] stays
//! the only function that computes a bar's `y`.
#![allow(dead_code)] // P6a lands the model; P6b wires the first call site.

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

    /// Whether the bar is dismissed and its exit has finished drawing.
    ///
    /// The retire path in P6d drops bars that answer `true`; a bar that
    /// animates but never retires keeps the event loop requesting frames
    /// forever (G-idle).
    pub fn is_retired(&self, now: Instant) -> bool {
        self.exit.is_some_and(|exit| exit.is_done(now))
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
    /// Index of the top bar, which is the only one `Enter` can act on (D4).
    pub fn top(&self) -> Option<usize> {
        self.visible.first().map(|&(index, _)| index)
    }
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

    let mut order: Vec<usize> = (0..bars.len()).collect();
    // Severity descending, then oldest first. `sort_by` is stable, so bars
    // queued in the same instant keep their insertion order.
    order.sort_by(|&a, &b| {
        bars[b]
            .kind
            .severity()
            .cmp(&bars[a].kind.severity())
            .then(bars[a].created_at.cmp(&bars[b].created_at))
    });

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
        assert_eq!(layout.top(), None);
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
        assert_eq!(layout.top(), Some(1));
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
        let layout = bar_rects(&[newer, older], TAB_BAR_H, CELL_H, WIDTH);
        assert_eq!(layout.top(), Some(1));
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
        let layout = bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH);
        let top = &bars[layout.top().expect("a non-empty stack has a top bar")];
        assert!(!top.kind.has_activation());

        let bars = [update(now)];
        let layout = bar_rects(&bars, TAB_BAR_H, CELL_H, WIDTH);
        let top = &bars[layout.top().expect("a non-empty stack has a top bar")];
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

    #[test]
    fn each_kind_maps_to_its_own_semantic_token() {
        let tokens = nexterm_config::DesignTokens::default();
        let now = Instant::now();
        assert_eq!(update(now).kind.accent(&tokens), tokens.semantic_success);
        assert_eq!(offline(now).kind.accent(&tokens), tokens.semantic_warning);
        assert_eq!(error(now).kind.accent(&tokens), tokens.semantic_error);
    }
}
