//! ウィンドウ関連の IPC ハンドラ — Resize/SplitVertical/SplitHorizontal/ResizeSplit/
//! NewWindow/CloseWindow/FocusWindow/RenameWindow/SetLayoutMode

use nexterm_proto::ServerToClient;
use tracing::error;

use super::dispatch::DispatchContext;
use crate::window::SplitDir;

pub(super) async fn handle_resize(ctx: &mut DispatchContext<'_>, cols: u16, rows: u16) {
    if let Some(ref name) = *ctx.current_session {
        let layout_msg = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                if let Err(e) = s.resize_focused(cols, rows) {
                    error!("リサイズエラー: {}", e);
                }
                s.focused_window().map(|w| w.layout_changed_msg(cols, rows))
            } else {
                None
            }
        };
        if let Some(msg) = layout_msg {
            let _ = ctx.tx.send(msg).await;
        }
    }
}

pub(super) async fn handle_split(ctx: &mut DispatchContext<'_>, dir: SplitDir) {
    if let Some(ref name) = *ctx.current_session {
        let session_name = name.clone();
        let split_result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(&session_name) {
                let cols = s.cols;
                let rows = s.rows;
                let shell = s.shell().to_string();
                let args = s.shell_args().to_vec();
                let pane_tx = s.broadcast_sender();
                s.focused_window_mut()
                    .map(|w| w.add_pane(cols, rows, pane_tx, &shell, &args, dir))
            } else {
                None
            }
        };
        match split_result {
            Some(Ok(pane_id)) => {
                crate::hooks::on_pane_open(ctx.hooks, &ctx.lua, &session_name, pane_id);
                if ctx.log_config.auto_log
                    && let Some(log_dir) = &ctx.log_config.log_dir
                    && let Err(e) = ctx
                        .manager
                        .start_recording_with_log_config(&session_name, log_dir, ctx.log_config)
                        .await
                {
                    tracing::warn!("auto_log 録音開始失敗 (pane={}): {}", pane_id, e);
                }
                let msgs = {
                    let arc = ctx.manager.sessions();
                    let sessions = arc.lock().await;
                    sessions.get(&session_name).and_then(|s| {
                        s.focused_window().map(|w| {
                            let layout_msg = w.layout_changed_msg(s.cols, s.rows);
                            let (pc, pr) =
                                if let ServerToClient::LayoutChanged { ref panes, .. } = layout_msg
                                {
                                    panes
                                        .iter()
                                        .find(|p| p.pane_id == pane_id)
                                        .map(|r| (r.cols, r.rows))
                                        .unwrap_or((s.cols, s.rows))
                                } else {
                                    (s.cols, s.rows)
                                };
                            let refresh = ServerToClient::FullRefresh {
                                pane_id,
                                grid: nexterm_proto::Grid::new(pc, pr),
                            };
                            (refresh, layout_msg)
                        })
                    })
                };
                if let Some((refresh, layout)) = msgs {
                    let _ = ctx.tx.send(refresh).await;
                    let _ = ctx.tx.send(layout).await;
                }
            }
            Some(Err(e)) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::Error {
                        message: e.to_string(),
                    })
                    .await;
            }
            None => {}
        }
    }
}

pub(super) async fn handle_resize_split(ctx: &mut DispatchContext<'_>, delta: f32) {
    if let Some(ref name) = *ctx.current_session {
        let layout_msg = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                if let Some(w) = s.focused_window_mut() {
                    w.adjust_split_ratio(delta, cols, rows);
                    Some(w.layout_changed_msg(cols, rows))
                } else {
                    None
                }
            } else {
                None
            }
        };
        if let Some(msg) = layout_msg {
            let _ = ctx.tx.send(msg).await;
        }
    }
}

pub(super) async fn handle_new_window(ctx: &mut DispatchContext<'_>) {
    if let Some(ref name) = *ctx.current_session {
        let session_name = name.clone();
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            sessions
                .get_mut(&session_name)
                .map(|s| s.add_window().map(|wid| (wid, s.window_list())))
        };
        match result {
            Some(Ok((_wid, windows))) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::WindowListChanged { windows })
                    .await;
                let refresh_msg = {
                    let arc = ctx.manager.sessions();
                    let sessions = arc.lock().await;
                    sessions.get(&session_name).and_then(|s| {
                        s.focused_window().and_then(|w| {
                            let pid = w.focused_pane_id();
                            w.pane(pid).map(|p| {
                                let layout = w.layout_changed_msg(s.cols, s.rows);
                                let refresh = ServerToClient::FullRefresh {
                                    pane_id: p.id,
                                    grid: p.make_full_refresh(),
                                };
                                (refresh, layout)
                            })
                        })
                    })
                };
                if let Some((refresh, layout)) = refresh_msg {
                    let _ = ctx.tx.send(refresh).await;
                    let _ = ctx.tx.send(layout).await;
                }
            }
            Some(Err(e)) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::Error {
                        message: e.to_string(),
                    })
                    .await;
            }
            None => {}
        }
    }
}

pub(super) async fn handle_close_window(ctx: &mut DispatchContext<'_>, window_id: u32) {
    if let Some(ref name) = *ctx.current_session {
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let r = s.remove_window(window_id);
                r.map(|_| s.window_list())
            } else {
                Ok(vec![])
            }
        };
        match result {
            Ok(windows) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::WindowListChanged { windows })
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
}

pub(super) async fn handle_focus_window(ctx: &mut DispatchContext<'_>, window_id: u32) {
    if let Some(ref name) = *ctx.current_session {
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let r = s.focus_window(window_id);
                r.map(|_| {
                    let windows = s.window_list();
                    s.focused_window().map(|w| {
                        let layout = w.layout_changed_msg(s.cols, s.rows);
                        let pid = w.focused_pane_id();
                        let refresh = w.pane(pid).map(|p| ServerToClient::FullRefresh {
                            pane_id: p.id,
                            grid: p.make_full_refresh(),
                        });
                        (windows, layout, refresh)
                    })
                })
            } else {
                Ok(None)
            }
        };
        match result {
            Ok(Some((windows, layout, refresh))) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::WindowListChanged { windows })
                    .await;
                let _ = ctx.tx.send(layout).await;
                if let Some(r) = refresh {
                    let _ = ctx.tx.send(r).await;
                }
            }
            Ok(None) => {}
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
}

pub(super) async fn handle_rename_window(
    ctx: &mut DispatchContext<'_>,
    window_id: u32,
    new_name: &str,
) {
    if let Some(ref session_name) = *ctx.current_session {
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(session_name) {
                let r = s.rename_window(window_id, new_name.to_string());
                r.map(|_| s.window_list())
            } else {
                Ok(vec![])
            }
        };
        match result {
            Ok(windows) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::WindowListChanged { windows })
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
}

pub(super) async fn handle_set_layout_mode(ctx: &mut DispatchContext<'_>, mode: &str) {
    if let Some(ref name) = *ctx.current_session {
        let layout_msg = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                s.focused_window_mut().map(|w| {
                    w.set_layout_mode(crate::window::LayoutMode::from_str(mode), cols, rows);
                    w.layout_changed_msg(cols, rows)
                })
            } else {
                None
            }
        };
        if let Some(msg) = layout_msg {
            let _ = ctx.tx.send(msg).await;
        }
    }
}
