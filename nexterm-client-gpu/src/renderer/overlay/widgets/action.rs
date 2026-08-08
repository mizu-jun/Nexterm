//! The action vocabulary shared by every tab's `apply_*_action` router.
//!
//! Cross-cutting rather than tab-specific: the AccessKit dispatch and the
//! mouse handler both translate into it, and each tab interprets it for its
//! own controls.

/// What an accessibility client asked to do to a control.
///
/// Not `Copy`: [`Self::SetText`] carries an owned string.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WidgetAction {
    /// Default action (AccessKit `Click`): flip a toggle, pick a swatch,
    /// advance a cycler.
    Activate,
    /// Step forward (AccessKit `Increment`).
    Next,
    /// Step backward (AccessKit `Decrement`).
    Prev,
    /// Set a numeric control directly (AccessKit `SetValue`). Ignored by
    /// kinds that carry no number.
    SetValue(f64),
    /// Set a text control directly (AccessKit `SetValue` with a string).
    /// Ignored by kinds that carry no text.
    SetText(String),
}
