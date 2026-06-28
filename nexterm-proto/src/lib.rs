//! nexterm-proto — IPC protocol definitions.
//!
//! Defines the message types exchanged between `nexterm-server` and `nexterm-client`.
//! Messages are serialized with `postcard` and transferred over a Unix Domain Socket
//! or a Windows Named Pipe.
#![warn(missing_docs)]

/// Terminal cell types (`Attrs`, `Cell`, `Color`).
pub mod cell;
/// Virtual grid types (`Grid`, `DirtyRow`, `HyperlinkSpan`).
pub mod grid;
/// IPC message types (`ClientToServer`, `ServerToClient`).
pub mod message;

pub use cell::{Attrs, Cell, Color};
pub use grid::{DirtyRow, Grid, HyperlinkSpan};
pub use message::{
    ClientKind, ClientToServer, KeyCode, Modifiers, PaneLayout, ServerToClient, SessionInfo,
    WindowInfo, WorkspaceInfo,
};

/// IPC protocol version.
///
/// Exchanged via the Hello handshake at connection time. The server drops the
/// connection on a version mismatch.
///
/// # History
///
/// - v1: Initial release (introduced in PR-C Task 10).
/// - v2: Sprint 5-1 / G1 — Removed `password: Option<String>` from `ConnectSsh` and
///   replaced it with `password_keyring_account: Option<String>` + `ephemeral_password: bool`.
///   Plain-text SSH passwords no longer flow over the IPC channel; the server fetches
///   credentials from its keyring instead.
/// - v3: Sprint 5-1 / G3 — Switched the IPC wire format from `bincode` 1.x to
///   `postcard` 1.x. Mitigates RUSTSEC-2025-0141 (bincode 1.x supply-chain status).
///   Varint encoding also reduces message size.
/// - v4: Sprint 5-2 / B2 — Added `ServerToClient::CwdChanged`. The shell's current
///   working directory, received via OSC 7 (`file://host/path`), is propagated to the
///   client so new panes can inherit the parent CWD and tabs can display it. Because
///   this adds an enum variant, v3 clients cannot decode `CwdChanged` from a new server.
/// - v5: Sprint 5-7 / Phase 2-1 — Added workspace support. New variants:
///   `ClientToServer::{CreateWorkspace, SwitchWorkspace, ListWorkspaces, RenameWorkspace, DeleteWorkspace}`
///   and `ServerToClient::{WorkspaceList, WorkspaceSwitched}`. `SessionInfo` gained a
///   `workspace_name: String` field (`#[serde(default)]` for compatibility with older
///   clients), bundling sessions into logical groups.
/// - v6: Sprint 5-7 / Phase 2-2 — Added Quake mode (global-hotkey show/hide toggle).
///   `ClientToServer::QuakeToggle { action }` was introduced so that `nexterm-ctl` can
///   trigger the toggle via the compositor's `bindsym` on Wayland and similar
///   environments. The server broadcasts `ServerToClient::QuakeToggleRequest { action }`
///   to every connected GPU client to perform the actual window operation.
/// - v7: Sprint 5-7 / Phase 2-3 — Tab reordering by drag.
///   `ClientToServer::ReorderPanes { pane_ids }` was added: the client sends the new
///   order decided by drag-and-drop on the tab bar, and the server updates
///   `Window.pane_order` so the ordering is reflected in the next `LayoutChanged.panes`.
///   `ReorderPanes` is the only new variant in this bump; nothing else changed.
/// - v8: Sprint 5-8 / Phase 4-3 — Tab tearing (drop a tab outside its tab bar).
///   `ClientToServer::MovePaneToWindow { pane_id, target_window_id, insert_at }` was
///   added. When the client drops a tab onto another OS Window (or a brand-new one),
///   the server calls `Window::detach_pane` + `Window::attach_pane` to change the
///   session's Window layout. v7 clients cannot decode the new message, so the server
///   drops the connection during Hello on a version mismatch.
///   - Phase 4-5 appended `ClientToServer::QueryForegroundProcess` and
///     `ServerToClient::ForegroundProcessStatus` at the end of the enums (additive,
///     still v8 compatible). Old v8 clients/servers neither send nor receive the new
///     variants, so existing v8 connections are unaffected.
// Phase 2c (UI/UX v2): bumped 8 → 9 to make room for
// `ServerToClient::ProcessChanged`. Adding a new postcard enum variant
// is a wire-format break (postcard tags variants positionally), so the
// version pin must move in lockstep. Single-binary `nexterm` ships
// both halves so the synchronised upgrade is automatic for users.
pub const PROTOCOL_VERSION: u32 = 9;

/// Maximum size (in bytes) of a single IPC message.
///
/// The receiver drops the connection as soon as a length prefix exceeds this value.
/// 64 MiB accommodates the worst-case `ImagePlaced` payload (4096 × 4096 × 4 = 64 MiB)
/// plus its metadata, and guards against OOM attacks that would otherwise allocate
/// `vec![0u8; msg_len]` for a multi-gigabyte length prefix.
pub const MAX_MSG_LEN: usize = 64 * 1024 * 1024;

/// Shared helper that validates a received length prefix.
///
/// Returns an error when `msg_len` exceeds [`MAX_MSG_LEN`].
///
/// # Arguments
/// - `msg_len`: The received length prefix in bytes.
///
/// # Errors
/// When `msg_len` is greater than [`MAX_MSG_LEN`].
pub fn validate_msg_len(msg_len: usize) -> std::result::Result<(), String> {
    if msg_len > MAX_MSG_LEN {
        Err(format!(
            "IPC message size exceeds the limit: {} > {} bytes",
            msg_len, MAX_MSG_LEN
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_within_the_limit_are_accepted() {
        assert!(validate_msg_len(0).is_ok());
        assert!(validate_msg_len(1024).is_ok());
        assert!(validate_msg_len(MAX_MSG_LEN).is_ok());
    }

    #[test]
    fn sizes_over_the_limit_are_rejected() {
        assert!(validate_msg_len(MAX_MSG_LEN + 1).is_err());
        assert!(validate_msg_len(usize::MAX).is_err());
        assert!(validate_msg_len(u32::MAX as usize).is_err());
    }

    #[test]
    fn error_message_includes_the_size() {
        let err = validate_msg_len(MAX_MSG_LEN + 1).unwrap_err();
        assert!(err.contains(&format!("{}", MAX_MSG_LEN + 1)));
        assert!(err.contains(&format!("{}", MAX_MSG_LEN)));
    }
}
