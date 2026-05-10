//! ペイン関連の IPC ハンドラ — KeyEvent/PasteText/MouseReport/Focus*/ClosePane/
//! ToggleZoom/SwapPane/BreakPane/JoinPane/フローティング系

use nexterm_proto::{KeyCode, Modifiers, ServerToClient};
use tracing::error;

use super::dispatch::DispatchContext;

pub(super) async fn handle_key_event(
    ctx: &mut DispatchContext<'_>,
    code: &KeyCode,
    modifiers: Modifiers,
) {
    if let Some(ref name) = *ctx.current_session {
        let bytes = super::key::key_to_bytes(code, modifiers);
        if !bytes.is_empty() {
            let arc = ctx.manager.sessions();
            let sessions = arc.lock().await;
            if let Some(s) = sessions.get(name)
                && let Err(e) = s.write_to_focused(&bytes)
            {
                error!("PTY 書き込みエラー: {}", e);
            }
        }
    }
}

pub(super) async fn handle_paste_text(ctx: &mut DispatchContext<'_>, text: &str) {
    if let Some(ref name) = *ctx.current_session {
        let arc = ctx.manager.sessions();
        let sessions = arc.lock().await;
        if let Some(s) = sessions.get(name) {
            let data: Vec<u8> = if s.focused_bracketed_paste_mode() {
                let mut v = b"\x1b[200~".to_vec();
                v.extend_from_slice(text.as_bytes());
                v.extend_from_slice(b"\x1b[201~");
                v
            } else {
                text.as_bytes().to_vec()
            };
            if let Err(e) = s.write_to_focused(&data) {
                error!("ペーストエラー: {}", e);
            }
        }
    }
}

pub(super) async fn handle_mouse_report(
    ctx: &mut DispatchContext<'_>,
    button: u8,
    col: u16,
    row: u16,
    pressed: bool,
    motion: bool,
) {
    if let Some(ref name) = *ctx.current_session {
        let arc = ctx.manager.sessions();
        let sessions = arc.lock().await;
        if let Some(s) = sessions.get(name) {
            let mode = s.focused_mouse_mode();
            if mode > 0 {
                let suffix = if pressed || motion { b'M' } else { b'm' };
                let cb = button as u32 + if motion { 32 } else { 0 };
                let seq = format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, suffix as char);
                if let Err(e) = s.write_to_focused(seq.as_bytes()) {
                    error!("マウスレポート送信エラー: {}", e);
                }
            }
        }
    }
}

pub(super) async fn handle_focus_next_pane(ctx: &mut DispatchContext<'_>) {
    if let Some(ref name) = *ctx.current_session {
        let layout_msg = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                if let Some(w) = s.focused_window_mut() {
                    w.focus_next();
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

pub(super) async fn handle_focus_prev_pane(ctx: &mut DispatchContext<'_>) {
    if let Some(ref name) = *ctx.current_session {
        let layout_msg = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                if let Some(w) = s.focused_window_mut() {
                    w.focus_prev();
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

pub(super) async fn handle_focus_pane(ctx: &mut DispatchContext<'_>, pane_id: u32) {
    if let Some(ref name) = *ctx.current_session {
        let layout_msg = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                if let Some(w) = s.focused_window_mut() {
                    w.set_focused_pane(pane_id);
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

pub(super) async fn handle_close_pane(ctx: &mut DispatchContext<'_>) {
    if let Some(ref name) = *ctx.current_session {
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                s.focused_window_mut()
                    .map(|w| w.remove_focused_pane(cols, rows))
            } else {
                None
            }
        };
        match result {
            Some(Ok(removed_id)) => {
                let layout_msg = {
                    let arc = ctx.manager.sessions();
                    let sessions = arc.lock().await;
                    sessions.get(name).and_then(|s| {
                        s.focused_window()
                            .map(|w| w.layout_changed_msg(s.cols, s.rows))
                    })
                };
                let _ = ctx
                    .tx
                    .send(ServerToClient::PaneClosed {
                        pane_id: removed_id,
                    })
                    .await;
                if let Some(msg) = layout_msg {
                    let _ = ctx.tx.send(msg).await;
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

pub(super) async fn handle_toggle_zoom(ctx: &mut DispatchContext<'_>) {
    if let Some(ref name) = *ctx.current_session {
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                s.focused_window_mut().map(|w| {
                    let is_zoomed = w.toggle_zoom(cols, rows);
                    let layout_msg = w.layout_changed_msg(cols, rows);
                    (is_zoomed, layout_msg)
                })
            } else {
                None
            }
        };
        if let Some((is_zoomed, layout_msg)) = result {
            let _ = ctx.tx.send(ServerToClient::ZoomChanged { is_zoomed }).await;
            let _ = ctx.tx.send(layout_msg).await;
        }
    }
}

pub(super) async fn handle_swap_pane(ctx: &mut DispatchContext<'_>, target_pane_id: u32) {
    if let Some(ref name) = *ctx.current_session {
        let layout_msg = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                if let Some(w) = s.focused_window_mut() {
                    w.swap_focused_with(target_pane_id);
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

pub(super) async fn handle_break_pane(ctx: &mut DispatchContext<'_>) {
    if let Some(ref name) = *ctx.current_session {
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                let old_layout = s.focused_window().map(|w| w.layout_changed_msg(cols, rows));
                s.break_pane().ok().map(|new_win_id| {
                    let pane_id = s.focused_window().map(|w| w.focused_pane_id()).unwrap_or(0);
                    let new_layout = s.focused_window().map(|w| w.layout_changed_msg(cols, rows));
                    let windows = s.window_list();
                    (new_win_id, pane_id, old_layout, new_layout, windows)
                })
            } else {
                None
            }
        };
        if let Some((new_win_id, pane_id, old_layout, new_layout, windows)) = result {
            let _ = ctx
                .tx
                .send(ServerToClient::PaneBroken {
                    new_window_id: new_win_id,
                    pane_id,
                })
                .await;
            let _ = ctx
                .tx
                .send(ServerToClient::WindowListChanged { windows })
                .await;
            if let Some(msg) = old_layout {
                let _ = ctx.tx.send(msg).await;
            }
            if let Some(msg) = new_layout {
                let _ = ctx.tx.send(msg).await;
            }
        }
    }
}

pub(super) async fn handle_join_pane(ctx: &mut DispatchContext<'_>, target_window_id: u32) {
    if let Some(ref name) = *ctx.current_session {
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                s.join_pane(target_window_id).ok().map(|pane_id| {
                    let new_layout = s.focused_window().map(|w| w.layout_changed_msg(cols, rows));
                    let windows = s.window_list();
                    (pane_id, new_layout, windows)
                })
            } else {
                None
            }
        };
        if let Some((pane_id, new_layout, windows)) = result {
            let _ = ctx
                .tx
                .send(ServerToClient::WindowListChanged { windows })
                .await;
            if let Some(msg) = new_layout {
                let _ = ctx.tx.send(msg).await;
            }
            let refresh = {
                let arc = ctx.manager.sessions();
                let sessions = arc.lock().await;
                sessions.get(name).and_then(|s| {
                    s.focused_window().and_then(|w| {
                        w.pane(pane_id).map(|p| ServerToClient::FullRefresh {
                            pane_id,
                            grid: p.make_full_refresh(),
                        })
                    })
                })
            };
            if let Some(r) = refresh {
                let _ = ctx.tx.send(r).await;
            }
        }
    }
}

pub(super) async fn handle_open_floating_pane(ctx: &mut DispatchContext<'_>) {
    if let Some(ref name) = *ctx.current_session {
        let result = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            if let Some(s) = sessions.get_mut(name) {
                let cols = s.cols;
                let rows = s.rows;
                let shell = s.shell().to_string();
                let args = s.shell_args().to_vec();
                let sender = s.broadcast_sender();
                s.focused_window_mut()
                    .map(|w| w.open_floating_pane(cols, rows, sender, &shell, &args))
            } else {
                None
            }
        };
        match result {
            Some(Ok((pane_id, rect))) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::FloatingPaneOpened {
                        pane_id,
                        col_off: rect.col_off,
                        row_off: rect.row_off,
                        cols: rect.cols,
                        rows: rect.rows,
                    })
                    .await;
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

pub(super) async fn handle_close_floating_pane(ctx: &mut DispatchContext<'_>, pane_id: u32) {
    if let Some(ref name) = *ctx.current_session {
        let closed = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            sessions.get_mut(name).and_then(|s| {
                s.focused_window_mut()
                    .map(|w| w.close_floating_pane(pane_id))
            })
        };
        if closed == Some(true) {
            let _ = ctx
                .tx
                .send(ServerToClient::FloatingPaneClosed { pane_id })
                .await;
        }
    }
}

pub(super) async fn handle_move_floating_pane(
    ctx: &mut DispatchContext<'_>,
    pane_id: u32,
    col_off: u16,
    row_off: u16,
) {
    if let Some(ref name) = *ctx.current_session {
        let rect_opt = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            sessions.get_mut(name).and_then(|s| {
                s.focused_window_mut()
                    .and_then(|w| w.move_floating_pane(pane_id, col_off, row_off))
            })
        };
        if let Some(rect) = rect_opt {
            let _ = ctx
                .tx
                .send(ServerToClient::FloatingPaneMoved {
                    pane_id,
                    col_off: rect.col_off,
                    row_off: rect.row_off,
                    cols: rect.cols,
                    rows: rect.rows,
                })
                .await;
        }
    }
}

pub(super) async fn handle_resize_floating_pane(
    ctx: &mut DispatchContext<'_>,
    pane_id: u32,
    cols: u16,
    rows: u16,
) {
    if let Some(ref name) = *ctx.current_session {
        let rect_opt = {
            let arc = ctx.manager.sessions();
            let mut sessions = arc.lock().await;
            sessions.get_mut(name).and_then(|s| {
                s.focused_window_mut()
                    .and_then(|w| w.resize_floating_pane(pane_id, cols, rows))
            })
        };
        if let Some(rect) = rect_opt {
            let _ = ctx
                .tx
                .send(ServerToClient::FloatingPaneMoved {
                    pane_id,
                    col_off: rect.col_off,
                    row_off: rect.row_off,
                    cols: rect.cols,
                    rows: rect.rows,
                })
                .await;
        }
    }
}
