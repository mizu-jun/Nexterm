//! Quake mode control commands (Sprint 5-7 / Phase 2-2): `quake toggle / show / hide`.
//!
//! Acts as a workaround for environments where the `global-hotkey` crate does not work
//! (most notably Wayland). Sending `ClientToServer::QuakeToggle { action }` causes the
//! server to broadcast `ServerToClient::QuakeToggleRequest` to every connected GPU client,
//! which then performs the actual window operation.
//!
//! Intended usage (Sway / Hyprland on Wayland):
//!
//! ```text
//! # ~/.config/sway/config
//! bindsym Ctrl+grave exec nexterm-ctl quake toggle
//! ```

use anyhow::Result;
use nexterm_proto::ClientToServer;

use crate::ipc::IpcConn;

async fn send_action(action: &str) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::QuakeToggle {
        action: action.to_string(),
    })
    .await?;
    // The server does not send a response message (it only broadcasts to clients).
    // A successful send is treated as success.
    println!("sent '{}' to quake mode", action);
    Ok(())
}

/// Toggle the visibility of the quake window.
pub(crate) async fn cmd_quake_toggle() -> Result<()> {
    send_action("toggle").await
}

/// Show the quake window (no-op if already visible).
pub(crate) async fn cmd_quake_show() -> Result<()> {
    send_action("show").await
}

/// Hide the quake window (no-op if already hidden).
pub(crate) async fn cmd_quake_hide() -> Result<()> {
    send_action("hide").await
}
