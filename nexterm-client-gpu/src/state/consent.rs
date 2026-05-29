//! Consent dialogs for sensitive operations (Sprint 4-1)
//!
//! Extracted from `state/mod.rs`:
//! - `ConsentKind` — kind of consent request (OpenUrl / ClipboardWrite / Notification)
//! - `ConsentDialog` — dialog state (kind and selected button index)
//! - `SessionConsentOverrides` — session-wide "always allow / deny" overrides

/// Holds "always allow/deny within the session" per consent dialog kind.
///
/// While `None`, fall back to the policy (i.e. the dialog will be shown again).
/// Only written when the user picks [Always Allow] / [Always Deny].
#[derive(Default)]
pub struct SessionConsentOverrides {
    pub external_url: Option<bool>,
    pub osc52_clipboard: Option<bool>,
    pub osc_notification: Option<bool>,
}

/// Consent dialog kind
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsentKind {
    /// Attempting to open an external URL
    OpenUrl(String),
    /// OSC 52 clipboard write request
    ClipboardWrite {
        /// Requesting pane ID (None = locally issued)
        source_pane: Option<u32>,
        /// Text to write
        text: String,
    },
    /// OSC 9 / 777 desktop notification request
    Notification {
        /// Requesting pane ID
        source_pane: u32,
        /// Notification title
        title: String,
        /// Notification body
        body: String,
    },
}

/// Consent dialog state
#[derive(Clone, Debug)]
pub struct ConsentDialog {
    /// Kind of consent request
    pub kind: ConsentKind,
    /// Selected button index (0=Allow, 1=Deny, 2=Always Allow, 3=Always Deny)
    pub selected: usize,
}

impl ConsentDialog {
    pub fn new(kind: ConsentKind) -> Self {
        Self { kind, selected: 0 }
    }
}
