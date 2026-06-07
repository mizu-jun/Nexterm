//! Session-related IPC handlers — Ping/Hello/Attach/Detach/ListSessions/KillSession/
//! StartRecording/StopRecording/StartAsciicast/StopAsciicast/SetBroadcast/DisplayPanes.

use nexterm_proto::{Grid, ServerToClient};
use tracing::{info, warn};

use super::dispatch::DispatchContext;
use super::dispatch_util::validate_recording_path;

/// Returns `true` when every cell in `grid` is blank (`ch == ' '`).
///
/// Used by `handle_attach` to detect the "restored session is still
/// rendering" state: the PTY reader has not yet processed the shell's
/// initial prompt, so `make_full_refresh` returns an empty grid even
/// though the shell process is alive. The caller then nudges the PTY
/// with `\r` so the shell re-emits its prompt and the next `GridDiff`
/// populates the screen.
fn is_blank_grid(grid: &Grid) -> bool {
    grid.rows
        .iter()
        .all(|row| row.iter().all(|cell| cell.ch == ' '))
}

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

            // v1.9.4 — subscribe to the session's broadcast BEFORE composing
            // the Full Refresh. `tokio::sync::broadcast::Receiver` only sees
            // messages sent after it subscribes, so any `GridDiff` emitted
            // between `make_full_refresh` and the previous subscribe location
            // (end of this function) used to be lost. That race was the
            // residual cause of the "blank screen on restored Windows
            // session" symptom: the PTY reader produces the shell prompt
            // diff in the gap and the client never receives it.
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
                info!(
                    "attach '{}': broadcast forwarder spawned before FullRefresh",
                    session_name
                );
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
            // v1.9.4 — if the focused pane's grid is still blank when we
            // attach (typical for a freshly restored session where the
            // shell has not had time to print its prompt before the client
            // arrived), nudge the PTY with a CR. The shell echoes it and
            // re-emits the prompt; the subsequent GridDiff is delivered
            // because the forwarder above is already subscribed.
            if let Some((pane_id, grid)) = &refresh {
                let non_blank_rows = grid
                    .rows
                    .iter()
                    .filter(|row| row.iter().any(|c| c.ch != ' '))
                    .count();
                info!(
                    "attach '{}': FullRefresh pane_id={} size={}x{} non_blank_rows={} cursor=({},{})",
                    session_name,
                    pane_id,
                    grid.width,
                    grid.height,
                    non_blank_rows,
                    grid.cursor_col,
                    grid.cursor_row
                );
                let should_nudge = grid.width > 0 && grid.height > 0 && is_blank_grid(grid);
                if should_nudge {
                    let arc = manager.sessions();
                    let sessions = arc.lock().await;
                    if let Some(s) = sessions.get(session_name)
                        && let Some(w) = s.focused_window()
                        && let Some(p) = w.pane(w.focused_pane_id())
                    {
                        match p.write_input(b"\r") {
                            Ok(()) => info!(
                                "attach '{}': pane_id={} grid was blank; nudged PTY with CR",
                                session_name, pane_id
                            ),
                            Err(e) => warn!(
                                "attach '{}': pane_id={} nudge write_input failed: {}",
                                session_name, pane_id, e
                            ),
                        }
                    }
                }
            }
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

#[cfg(test)]
mod tests {
    use super::is_blank_grid;
    use nexterm_proto::{Cell, Grid};

    #[test]
    fn is_blank_grid_returns_true_for_fresh_grid() {
        // A newly created grid is full of default cells (ch == ' ').
        let g = Grid::new(80, 24);
        assert!(is_blank_grid(&g));
    }

    #[test]
    fn is_blank_grid_returns_false_when_any_cell_has_a_visible_char() {
        let mut g = Grid::new(80, 24);
        g.rows[2][5] = Cell {
            ch: '$',
            ..Cell::default()
        };
        assert!(!is_blank_grid(&g));
    }

    #[test]
    fn is_blank_grid_treats_explicit_space_cells_as_blank() {
        // A row of explicit ' ' default cells is still blank.
        let mut g = Grid::new(80, 24);
        for cell in g.rows[0].iter_mut() {
            *cell = Cell::default();
        }
        assert!(is_blank_grid(&g));
    }

    #[test]
    fn is_blank_grid_handles_a_zero_size_grid() {
        // Degenerate but valid: a 0×0 grid is vacuously blank and must not
        // panic. Callers should not nudge in this case but the helper still
        // returns true.
        let g = Grid::new(0, 0);
        assert!(is_blank_grid(&g));
    }
}
