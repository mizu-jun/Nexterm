//! Desktop notification wrapper (Sprint 4-1).
//!
//! Notifications requested via OSC 9 / 777 are forwarded — after user consent —
//! to the platform's native notification API (through `notify-rust`).
//!
//! Send failures (no D-Bus, missing notification permissions, etc.) are logged as
//! warnings only; the app never crashes on them.

use tracing::warn;

/// Send a desktop notification.
///
/// # Arguments
/// - `title`: notification title (prefixed with "Nexterm: ")
/// - `body`: notification body
///
/// Failures only produce a `warn` log entry.
pub fn send_notification(title: &str, body: &str) {
    let summary = format!("Nexterm: {title}");
    let result = notify_rust::Notification::new()
        .summary(&summary)
        .body(body)
        .appname("Nexterm")
        .show();
    if let Err(e) = result {
        warn!("failed to send desktop notification: {}", e);
    }
}
