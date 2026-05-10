//! セッション関連の IPC ハンドラ — Ping/Hello/Attach/Detach/ListSessions/KillSession/
//! StartRecording/StopRecording/StartAsciicast/StopAsciicast/SetBroadcast/DisplayPanes

use nexterm_proto::ServerToClient;
use tracing::info;

use super::dispatch::DispatchContext;
use super::dispatch_util::validate_recording_path;

pub(super) async fn handle_ping(ctx: &mut DispatchContext<'_>) {
    let _ = ctx.tx.send(ServerToClient::Pong).await;
}

pub(super) fn handle_hello() {
    // Hello はハンドシェイク段階で handler.rs が処理する。
    // ここに到達した場合はプロトコル違反（Hello を再送）として無視する。
    tracing::warn!("ハンドシェイク後に Hello を再送信されました。無視します。");
}

pub(super) async fn handle_attach(ctx: &mut DispatchContext<'_>, session_name: &str) {
    let manager = ctx.manager;
    let tx = ctx.tx.clone();

    // セッションが存在しない場合は新規作成してアタッチ
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

            // Full Refresh を送信する
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

            // レイアウト変更通知を送信する（全ペインの位置・サイズを通知）
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

            // セッション一覧も送信する
            let list = manager.list_sessions().await;
            let _ = tx
                .send(ServerToClient::SessionList { sessions: list })
                .await;

            // on_attach フック
            crate::hooks::on_attach(ctx.hooks, &ctx.lua, session_name);

            // broadcast forwarder タスクを起動する
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
                                    "broadcast: {} メッセージをスキップしました（バッファ溢れ）",
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
            info!("セッション '{}' をデタッチしました", name);
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
    // サーバー側での処理は不要（クライアント側のオーバーレイ表示のみ）
}
