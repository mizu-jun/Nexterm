//! Session-related IPC handlers — Ping/Hello/Attach/Detach/ListSessions/KillSession/
//! StartRecording/StopRecording/StartAsciicast/StopAsciicast/SetBroadcast/DisplayPanes.

use nexterm_proto::ServerToClient;
use tracing::info;

use super::dispatch::DispatchContext;
use super::dispatch_util::validate_recording_path;

pub(super) async fn handle_ping(ctx: &mut DispatchContext<'_>) {
    let _ = ctx.tx.send(ServerToClient::Pong).await;
}

pub(super) fn handle_hello() {
    // Hello is handled during the handshake phase by handler.rs.
    // Reaching this point means the client violated the protocol (re-sent Hello); ignore it.
    tracing::warn!("Hello re-sent after the handshake; ignoring");
}

pub(super) async fn handle_attach(ctx: &mut DispatchContext<'_>, session_name: &str) {
    let manager = ctx.manager;
    let tx = ctx.tx.clone();

    // If the session does not exist, create and attach to a new one.
    let is_new_session = {
        let arc = manager.sessions();
        let sessions = arc.lock().await;
        !sessions.contains_key(session_name)
    };
    match manager.get_or_create_and_attach(session_name, 80, 24).await {
        Ok(()) => {
            *ctx.current_session = Some(session_name.to_string());
            if is_new_session {
                crate::hooks::on_session_start(ctx.hooks, &ctx.lua, session_name);
            }

            // Send a Full Refresh.
            let refresh = {
                let arc = manager.sessions();
                let sessions = arc.lock().await;
                sessions.get(session_name).and_then(|s| {
                    s.focused_window().and_then(|w| {
                        let pid = w.focused_pane_id();
                        w.pane(pid).map(|p| (p.id, p.make_full_refresh()))
                    })
                })
            };
            if let Some((pane_id, grid)) = refresh {
                let _ = tx.send(ServerToClient::FullRefresh { pane_id, grid }).await;
            }

            // Send a layout-changed notification (positions and sizes of every pane).
            let layout_msg = {
                let arc = manager.sessions();
                let sessions = arc.lock().await;
                sessions.get(session_name).and_then(|s| {
                    s.focused_window()
                        .map(|w| w.layout_changed_msg(s.cols, s.rows))
                })
            };
            if let Some(msg) = layout_msg {
                let _ = tx.send(msg).await;
            }

            // Also send the session list.
            let list = manager.list_sessions().await;
            let _ = tx
                .send(ServerToClient::SessionList { sessions: list })
                .await;

            // Sprint 5-12 Phase 4: drain and forward startup warnings (e.g. config load failure).
            // The client visualizes these as `error_banner`.
            // Multiple warnings are joined with "; " (the banner has a single slot, so the latest overwrites).
            let warnings = manager.take_startup_warnings();
            if !warnings.is_empty() {
                let _ = tx
                    .send(ServerToClient::Error {
                        message: warnings.join("; "),
                    })
                    .await;
            }

            // on_attach hook.
            crate::hooks::on_attach(ctx.hooks, &ctx.lua, session_name);

            // Spawn the broadcast forwarder task.
            let bcast_rx = {
                let arc = manager.sessions();
                let sessions = arc.lock().await;
                sessions.get(session_name).map(|s| s.attach())
            };
            if let Some(mut bcast_rx) = bcast_rx {
                let fwd_tx = tx.clone();
                if let Some(h) = ctx.bcast_forwarder.take() {
                    let _: () = h.abort();
                }
                let handle = tokio::spawn(async move {
                    loop {
                        match bcast_rx.recv().await {
                            Ok(msg) => {
                                if fwd_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!(
                                    "broadcast: skipped {} messages (buffer overflow)",
                                    n
                                );
                                continue;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
                *ctx.bcast_forwarder = Some(handle.abort_handle());
            }
        }
        Err(e) => {
            let _ = tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn handle_detach(ctx: &mut DispatchContext<'_>) {
    if let Some(name) = ctx.current_session.take() {
        let arc = ctx.manager.sessions();
        let mut sessions = arc.lock().await;
        if let Some(s) = sessions.get_mut(&name) {
            s.detach_all();
            info!("detached session '{}'", name);
        }
    }
}

pub(super) async fn handle_list_sessions(ctx: &mut DispatchContext<'_>) {
    let list = ctx.manager.list_sessions().await;
    let _ = ctx
        .tx
        .send(ServerToClient::SessionList { sessions: list })
        .await;
}

pub(super) async fn handle_kill_session(ctx: &mut DispatchContext<'_>, name: &str) {
    match ctx.manager.kill_session(name).await {
        Ok(()) => {
            let list = ctx.manager.list_sessions().await;
            let _ = ctx
                .tx
                .send(ServerToClient::SessionList { sessions: list })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn handle_start_recording(
    ctx: &mut DispatchContext<'_>,
    session_name: &str,
    output_path: &str,
) {
    if let Err(e) = validate_recording_path(output_path) {
        let _ = ctx
            .tx
            .send(ServerToClient::Error {
                message: e.to_string(),
            })
            .await;
        return;
    }
    match ctx.manager.start_recording(session_name, output_path).await {
        Ok(pane_id) => {
            let _ = ctx
                .tx
                .send(ServerToClient::RecordingStarted {
                    pane_id,
                    path: output_path.to_string(),
                })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn handle_stop_recording(ctx: &mut DispatchContext<'_>, session_name: &str) {
    match ctx.manager.stop_recording(session_name).await {
        Ok(pane_id) => {
            let _ = ctx
                .tx
                .send(ServerToClient::RecordingStopped { pane_id })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn handle_start_asciicast(
    ctx: &mut DispatchContext<'_>,
    session_name: &str,
    output_path: &str,
) {
    if let Err(e) = validate_recording_path(output_path) {
        let _ = ctx
            .tx
            .send(ServerToClient::Error {
                message: e.to_string(),
            })
            .await;
        return;
    }
    match ctx.manager.start_asciicast(session_name, output_path).await {
        Ok(pane_id) => {
            let _ = ctx
                .tx
                .send(ServerToClient::AsciicastStarted {
                    pane_id,
                    path: output_path.to_string(),
                })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn handle_stop_asciicast(ctx: &mut DispatchContext<'_>, session_name: &str) {
    match ctx.manager.stop_asciicast(session_name).await {
        Ok(pane_id) => {
            let _ = ctx
                .tx
                .send(ServerToClient::AsciicastStopped { pane_id })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn handle_set_broadcast(ctx: &mut DispatchContext<'_>, enabled: bool) {
    if let Some(ref name) = *ctx.current_session {
        let arc = ctx.manager.sessions();
        let mut sessions = arc.lock().await;
        if let Some(s) = sessions.get_mut(name) {
            s.set_broadcast(enabled);
            let _ = ctx
                .tx
                .send(ServerToClient::BroadcastModeChanged { enabled })
                .await;
        }
    }
}

pub(super) fn handle_display_panes() {
    // No server-side processing required (the client just shows an overlay).
}

// ---- Workspace management (Sprint 5-7 / Phase 2-1) ----

/// Send the current workspace list to the client.
pub(super) async fn handle_list_workspaces(ctx: &mut DispatchContext<'_>) {
    let (current, workspaces) = ctx.manager.list_workspaces().await;
    let _ = ctx
        .tx
        .send(ServerToClient::WorkspaceList {
            current,
            workspaces,
        })
        .await;
}

/// Create a new workspace and send the latest list.
pub(super) async fn handle_create_workspace(ctx: &mut DispatchContext<'_>, name: &str) {
    match ctx.manager.create_workspace(name).await {
        Ok(()) => {
            let (current, workspaces) = ctx.manager.list_workspaces().await;
            let _ = ctx
                .tx
                .send(ServerToClient::WorkspaceList {
                    current,
                    workspaces,
                })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

/// Switch the current workspace and send a switch notification together with the latest list.
pub(super) async fn handle_switch_workspace(ctx: &mut DispatchContext<'_>, name: &str) {
    match ctx.manager.switch_workspace(name).await {
        Ok(switched) => {
            let _ = ctx
                .tx
                .send(ServerToClient::WorkspaceSwitched {
                    name: switched.clone(),
                })
                .await;
            let (current, workspaces) = ctx.manager.list_workspaces().await;
            let _ = ctx
                .tx
                .send(ServerToClient::WorkspaceList {
                    current,
                    workspaces,
                })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

/// Rename a workspace and send the latest list.
pub(super) async fn handle_rename_workspace(ctx: &mut DispatchContext<'_>, from: &str, to: &str) {
    match ctx.manager.rename_workspace(from, to).await {
        Ok(()) => {
            let (current, workspaces) = ctx.manager.list_workspaces().await;
            let _ = ctx
                .tx
                .send(ServerToClient::WorkspaceList {
                    current,
                    workspaces,
                })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

// ---- Quake mode (Sprint 5-7 / Phase 2-2) ----

/// Broadcast a Quake toggle request to every GPU client.
///
/// `action` must be one of "toggle" / "show" / "hide". Anything else is logged as a warning
/// and treated as "toggle" (the client parses it strictly again).
pub(super) async fn handle_quake_toggle(ctx: &mut DispatchContext<'_>, action: &str) {
    let normalized = match action {
        "toggle" | "show" | "hide" => action.to_string(),
        other => {
            tracing::warn!(
                "received unknown QuakeToggle action='{}'; falling back to 'toggle'",
                other
            );
            "toggle".to_string()
        }
    };
    let delivered = ctx.manager.broadcast_quake_request(&normalized).await;
    tracing::info!(
        "broadcast Quake toggle request '{}' to {} session(s)",
        normalized,
        delivered
    );
}

/// Delete a workspace and send the latest list.
pub(super) async fn handle_delete_workspace(
    ctx: &mut DispatchContext<'_>,
    name: &str,
    force: bool,
) {
    match ctx.manager.delete_workspace(name, force).await {
        Ok(()) => {
            let (current, workspaces) = ctx.manager.list_workspaces().await;
            let _ = ctx
                .tx
                .send(ServerToClient::WorkspaceList {
                    current,
                    workspaces,
                })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}
