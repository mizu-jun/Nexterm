//! Sprint 4-1: consent flow for sensitive operations.
//!
//! Extracted from `event_handler.rs`:
//! - `handle_notification_request` / `handle_clipboard_write_request` / `request_open_url`
//! - `resolve_pending_consent` (invoked from input_handler)
//! - `truncate_utf8_at` helper

use super::EventHandler;

impl EventHandler {
    /// Handle an OSC 9 / 777 desktop-notification request.
    ///
    /// According to the `SecurityConfig.osc_notification` policy and the per-session
    /// override, either send immediately, deny, or show the consent dialog.
    pub(super) fn handle_notification_request(
        &mut self,
        pane_id: u32,
        title: String,
        body: String,
    ) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.osc_notification;
        let session_override = self.app.state.session_consent_overrides.osc_notification;
        // Truncate the notification body at the configured limit (DoS mitigation).
        let max = self.app.config.security.notification_max_bytes;
        let body = truncate_utf8_at(&body, max);

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                // Show the dialog. The user's choice is processed in input_handler.
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::Notification {
                        source_pane: pane_id,
                        title,
                        body,
                    },
                ));
                return;
            }
        };

        if allow {
            crate::notification::send_notification(&title, &body);
        } else {
            tracing::info!(
                "Desktop notification denied by policy: pane={} title={:?}",
                pane_id,
                title
            );
        }
    }

    /// Handle an OSC 52 clipboard-write request.
    ///
    /// According to the `SecurityConfig.osc52_clipboard` policy and the per-session
    /// override, either write immediately, deny, or show the consent dialog.
    pub(super) fn handle_clipboard_write_request(&mut self, pane_id: u32, text: String) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.osc52_clipboard;
        let session_override = self.app.state.session_consent_overrides.osc52_clipboard;
        // Requests exceeding the configured maximum size are denied unconditionally.
        let max = self.app.config.security.osc52_max_bytes;
        if text.len() > max {
            tracing::warn!(
                "OSC 52 request denied due to size limit: pane={} bytes={} max={}",
                pane_id,
                text.len(),
                max
            );
            return;
        }

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::ClipboardWrite {
                        source_pane: Some(pane_id),
                        text,
                    },
                ));
                return;
            }
        };

        if allow {
            match arboard::Clipboard::new() {
                Ok(mut clipboard) => {
                    if let Err(e) = clipboard.set_text(text) {
                        tracing::warn!("OSC 52 clipboard write failed: {}", e);
                    }
                }
                Err(_) => {
                    tracing::warn!("OSC 52: failed to initialize the clipboard API");
                }
            }
        } else {
            tracing::info!(
                "OSC 52 clipboard request denied by policy: pane={}",
                pane_id
            );
        }
    }

    /// Handle a request to open an external URL (Ctrl+click / via OSC 8).
    ///
    /// Honors the `SecurityConfig.external_url` policy and the per-session override.
    pub(super) fn request_open_url(&mut self, url: String) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.external_url;
        let session_override = self.app.state.session_consent_overrides.external_url;

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::OpenUrl(url),
                ));
                return;
            }
        };

        if allow {
            crate::vertex_util::open_url(&url);
        } else {
            tracing::info!("Open-URL request denied by policy: {}", url);
        }
    }

    /// Apply the user's decision from the consent dialog.
    ///
    /// The caller (input_handler) interprets the key input and invokes this method.
    /// Argument `decision`:
    /// - `Some(true)`: allow once
    /// - `Some(false)`: deny once
    /// - `None`: close the dialog only (treated as deny)
    ///
    /// Argument `always`: when true, treat any future request of the same kind with
    /// the same decision for the rest of the session.
    pub(in crate::renderer) fn resolve_pending_consent(
        &mut self,
        decision: Option<bool>,
        always: bool,
    ) {
        let Some(dialog) = self.app.state.pending_consent.take() else {
            return;
        };
        let allow = decision.unwrap_or(false);

        if always {
            match &dialog.kind {
                crate::state::ConsentKind::OpenUrl(_) => {
                    self.app.state.session_consent_overrides.external_url = Some(allow);
                }
                crate::state::ConsentKind::ClipboardWrite { .. } => {
                    self.app.state.session_consent_overrides.osc52_clipboard = Some(allow);
                }
                crate::state::ConsentKind::Notification { .. } => {
                    self.app.state.session_consent_overrides.osc_notification = Some(allow);
                }
            }
        }

        if !allow {
            return;
        }

        match dialog.kind {
            crate::state::ConsentKind::OpenUrl(url) => {
                crate::vertex_util::open_url(&url);
            }
            crate::state::ConsentKind::ClipboardWrite { text, .. } => {
                if let Ok(mut clipboard) = arboard::Clipboard::new()
                    && let Err(e) = clipboard.set_text(text)
                {
                    tracing::warn!("Clipboard write failed: {}", e);
                }
            }
            crate::state::ConsentKind::Notification { title, body, .. } => {
                crate::notification::send_notification(&title, &body);
            }
        }
    }
}

/// Truncate a string at `max_bytes`, respecting UTF-8 character boundaries.
fn truncate_utf8_at(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}
