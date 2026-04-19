//! IPC メッセージディスパッチ — ClientToServer メッセージを処理する

use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::session::SessionManager;
use crate::window::SplitDir;

/// クライアントからのメッセージをディスパッチする
#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch(
    msg: &ClientToServer,
    manager: &SessionManager,
    tx: mpsc::Sender<ServerToClient>,
    current_session: &mut Option<String>,
    hooks: &nexterm_config::HooksConfig,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    log_config: &nexterm_config::LogConfig,
    hosts: &[nexterm_config::HostConfig],
    bcast_forwarder: &mut Option<tokio::task::AbortHandle>,
) {
    use ClientToServer::*;

    match msg {
        Ping => {
            let _ = tx.send(ServerToClient::Pong).await;
        }

        Attach { session_name } => {
            // セッションが存在しない場合は新規作成してアタッチ
            let is_new_session = {
                let arc = manager.sessions();
                let sessions = arc.lock().await;
                !sessions.contains_key(session_name.as_str())
            };
            match manager
                .get_or_create_and_attach(session_name, 80, 24)
                .await
            {
                Ok(()) => {
                    *current_session = Some(session_name.clone());
                    if is_new_session {
                        crate::hooks::on_session_start(hooks, &lua, session_name);
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
                        let _ = tx
                            .send(ServerToClient::FullRefresh { pane_id, grid })
                            .await;
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
                    crate::hooks::on_attach(hooks, &lua, session_name);

                    // broadcast forwarder タスクを起動する
                    let bcast_rx = {
                        let arc = manager.sessions();
                        let sessions = arc.lock().await;
                        sessions.get(session_name).map(|s| s.attach())
                    };
                    if let Some(mut bcast_rx) = bcast_rx {
                        let fwd_tx = tx.clone();
                        if let Some(h) = bcast_forwarder.take() {
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
                        *bcast_forwarder = Some(handle.abort_handle());
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

        Detach => {
            if let Some(name) = current_session.take() {
                let arc = manager.sessions();
                let mut sessions = arc.lock().await;
                if let Some(s) = sessions.get_mut(&name) {
                    s.detach_all();
                    info!("セッション '{}' をデタッチしました", name);
                }
            }
        }

        KeyEvent { code, modifiers } => {
            if let Some(ref name) = *current_session {
                let bytes = super::key::key_to_bytes(code, *modifiers);
                if !bytes.is_empty() {
                    let arc = manager.sessions();
                    let sessions = arc.lock().await;
                    if let Some(s) = sessions.get(name)
                        && let Err(e) = s.write_to_focused(&bytes)
                    {
                        error!("PTY 書き込みエラー: {}", e);
                    }
                }
            }
        }

        Resize { cols, rows } => {
            if let Some(ref name) = *current_session {
                let layout_msg = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        if let Err(e) = s.resize_focused(*cols, *rows) {
                            error!("リサイズエラー: {}", e);
                        }
                        s.focused_window().map(|w| w.layout_changed_msg(*cols, *rows))
                    } else {
                        None
                    }
                };
                if let Some(msg) = layout_msg {
                    let _ = tx.send(msg).await;
                }
            }
        }

        SplitVertical | SplitHorizontal => {
            if let Some(ref name) = *current_session {
                let dir = if matches!(msg, SplitVertical) {
                    SplitDir::Vertical
                } else {
                    SplitDir::Horizontal
                };
                let split_result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        let shell = s.shell().to_string();
                        let pane_tx = s.broadcast_sender();
                        s.focused_window_mut()
                            .map(|w| w.add_pane(cols, rows, pane_tx, &shell, dir))
                    } else {
                        None
                    }
                };
                match split_result {
                    Some(Ok(pane_id)) => {
                        crate::hooks::on_pane_open(hooks, &lua, name, pane_id);
                        if log_config.auto_log
                            && let Some(log_dir) = &log_config.log_dir
                                && let Err(e) = manager
                                    .start_recording_with_log_config(name, log_dir, log_config)
                                    .await
                                {
                                    tracing::warn!("auto_log 録音開始失敗 (pane={}): {}", pane_id, e);
                                }
                        let msgs = {
                            let arc = manager.sessions();
                            let sessions = arc.lock().await;
                            sessions.get(name).and_then(|s| {
                                s.focused_window().map(|w| {
                                    let layout_msg = w.layout_changed_msg(s.cols, s.rows);
                                    let (pc, pr) = if let ServerToClient::LayoutChanged {
                                        ref panes, ..
                                    } = layout_msg
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
                            let _ = tx.send(refresh).await;
                            let _ = tx.send(layout).await;
                        }
                    }
                    Some(Err(e)) => {
                        let _ = tx
                            .send(ServerToClient::Error {
                                message: e.to_string(),
                            })
                            .await;
                    }
                    None => {}
                }
            }
        }

        FocusNextPane => {
            if let Some(ref name) = *current_session {
                let layout_msg = {
                    let arc = manager.sessions();
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
                    let _ = tx.send(msg).await;
                }
            }
        }

        FocusPrevPane => {
            if let Some(ref name) = *current_session {
                let layout_msg = {
                    let arc = manager.sessions();
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
                    let _ = tx.send(msg).await;
                }
            }
        }

        FocusPane { pane_id } => {
            if let Some(ref name) = *current_session {
                let layout_msg = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        if let Some(w) = s.focused_window_mut() {
                            w.set_focused_pane(*pane_id);
                            Some(w.layout_changed_msg(cols, rows))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(msg) = layout_msg {
                    let _ = tx.send(msg).await;
                }
            }
        }

        PasteText { text } => {
            if let Some(ref name) = *current_session {
                let arc = manager.sessions();
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

        MouseReport { button, col, row, pressed, motion } => {
            if let Some(ref name) = *current_session {
                let arc = manager.sessions();
                let sessions = arc.lock().await;
                if let Some(s) = sessions.get(name) {
                    let mode = s.focused_mouse_mode();
                    if mode > 0 {
                        let suffix = if *pressed || *motion { b'M' } else { b'm' };
                        let cb = *button as u32 + if *motion { 32 } else { 0 };
                        let seq =
                            format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, suffix as char);
                        if let Err(e) = s.write_to_focused(seq.as_bytes()) {
                            error!("マウスレポート送信エラー: {}", e);
                        }
                    }
                }
            }
        }

        ListSessions => {
            let list = manager.list_sessions().await;
            let _ = tx.send(ServerToClient::SessionList { sessions: list }).await;
        }

        KillSession { name } => {
            match manager.kill_session(name).await {
                Ok(()) => {
                    let list = manager.list_sessions().await;
                    let _ = tx.send(ServerToClient::SessionList { sessions: list }).await;
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

        StartRecording { session_name, output_path } => {
            if let Err(e) = validate_recording_path(output_path) {
                let _ = tx
                    .send(ServerToClient::Error { message: e.to_string() })
                    .await;
                return;
            }
            match manager.start_recording(session_name, output_path).await {
                Ok(pane_id) => {
                    let _ = tx
                        .send(ServerToClient::RecordingStarted {
                            pane_id,
                            path: output_path.to_string(),
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerToClient::Error { message: e.to_string() })
                        .await;
                }
            }
        }

        StopRecording { session_name } => {
            match manager.stop_recording(session_name).await {
                Ok(pane_id) => {
                    let _ = tx
                        .send(ServerToClient::RecordingStopped { pane_id })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerToClient::Error { message: e.to_string() })
                        .await;
                }
            }
        }

        ClosePane => {
            if let Some(ref name) = *current_session {
                let result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        s.focused_window_mut().map(|w| w.remove_focused_pane(cols, rows))
                    } else {
                        None
                    }
                };
                match result {
                    Some(Ok(removed_id)) => {
                        let layout_msg = {
                            let arc = manager.sessions();
                            let sessions = arc.lock().await;
                            sessions.get(name).and_then(|s| {
                                s.focused_window()
                                    .map(|w| w.layout_changed_msg(s.cols, s.rows))
                            })
                        };
                        let _ = tx
                            .send(ServerToClient::PaneClosed { pane_id: removed_id })
                            .await;
                        if let Some(msg) = layout_msg {
                            let _ = tx.send(msg).await;
                        }
                    }
                    Some(Err(e)) => {
                        let _ = tx
                            .send(ServerToClient::Error { message: e.to_string() })
                            .await;
                    }
                    None => {}
                }
            }
        }

        ResizeSplit { delta } => {
            if let Some(ref name) = *current_session {
                let layout_msg = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        if let Some(w) = s.focused_window_mut() {
                            w.adjust_split_ratio(*delta, cols, rows);
                            Some(w.layout_changed_msg(cols, rows))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(msg) = layout_msg {
                    let _ = tx.send(msg).await;
                }
            }
        }

        NewWindow => {
            if let Some(ref name) = *current_session {
                let result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    sessions
                        .get_mut(name)
                        .map(|s| s.add_window().map(|wid| (wid, s.window_list())))
                };
                match result {
                    Some(Ok((_wid, windows))) => {
                        let _ = tx
                            .send(ServerToClient::WindowListChanged { windows })
                            .await;
                        let refresh_msg = {
                            let arc = manager.sessions();
                            let sessions = arc.lock().await;
                            sessions.get(name).and_then(|s| {
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
                            let _ = tx.send(refresh).await;
                            let _ = tx.send(layout).await;
                        }
                    }
                    Some(Err(e)) => {
                        let _ = tx
                            .send(ServerToClient::Error { message: e.to_string() })
                            .await;
                    }
                    None => {}
                }
            }
        }

        CloseWindow { window_id } => {
            if let Some(ref name) = *current_session {
                let result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let r = s.remove_window(*window_id);
                        r.map(|_| s.window_list())
                    } else {
                        Ok(vec![])
                    }
                };
                match result {
                    Ok(windows) => {
                        let _ = tx
                            .send(ServerToClient::WindowListChanged { windows })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(ServerToClient::Error { message: e.to_string() })
                            .await;
                    }
                }
            }
        }

        FocusWindow { window_id } => {
            if let Some(ref name) = *current_session {
                let result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let r = s.focus_window(*window_id);
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
                        let _ = tx
                            .send(ServerToClient::WindowListChanged { windows })
                            .await;
                        let _ = tx.send(layout).await;
                        if let Some(r) = refresh {
                            let _ = tx.send(r).await;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = tx
                            .send(ServerToClient::Error { message: e.to_string() })
                            .await;
                    }
                }
            }
        }

        RenameWindow { window_id, name: new_name } => {
            if let Some(ref session_name) = *current_session {
                let result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(session_name) {
                        let r = s.rename_window(*window_id, new_name.clone());
                        r.map(|_| s.window_list())
                    } else {
                        Ok(vec![])
                    }
                };
                match result {
                    Ok(windows) => {
                        let _ = tx
                            .send(ServerToClient::WindowListChanged { windows })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(ServerToClient::Error { message: e.to_string() })
                            .await;
                    }
                }
            }
        }

        SetBroadcast { enabled } => {
            if let Some(ref name) = *current_session {
                let arc = manager.sessions();
                let mut sessions = arc.lock().await;
                if let Some(s) = sessions.get_mut(name) {
                    s.set_broadcast(*enabled);
                    let _ = tx
                        .send(ServerToClient::BroadcastModeChanged { enabled: *enabled })
                        .await;
                }
            }
        }

        DisplayPanes { .. } => {
            // サーバー側での処理は不要（クライアント側のオーバーレイ表示のみ）
        }

        StartAsciicast { session_name, output_path } => {
            if let Err(e) = validate_recording_path(output_path) {
                let _ = tx
                    .send(ServerToClient::Error { message: e.to_string() })
                    .await;
                return;
            }
            match manager.start_asciicast(session_name, output_path).await {
                Ok(pane_id) => {
                    let _ = tx
                        .send(ServerToClient::AsciicastStarted {
                            pane_id,
                            path: output_path.to_string(),
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerToClient::Error { message: e.to_string() })
                        .await;
                }
            }
        }

        StopAsciicast { session_name } => {
            match manager.stop_asciicast(session_name).await {
                Ok(pane_id) => {
                    let _ = tx
                        .send(ServerToClient::AsciicastStopped { pane_id })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerToClient::Error { message: e.to_string() })
                        .await;
                }
            }
        }

        SaveTemplate { name } => {
            let result: anyhow::Result<String> = async {
                let session_name = current_session
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("セッションにアタッチしていません"))?;
                let (window_titles, pane_counts) = {
                    let arc = manager.sessions();
                    let sessions = arc.lock().await;
                    let session = sessions
                        .get(session_name)
                        .ok_or_else(|| anyhow::anyhow!("セッションが見つかりません: {}", session_name))?;
                    let info = session.window_list();
                    let titles: Vec<String> = info.iter().map(|w| w.name.clone()).collect();
                    let counts: Vec<usize> = info.iter().map(|w| w.pane_count as usize).collect();
                    (titles, counts)
                };
                let template =
                    crate::template::template_from_session_info(name, window_titles, pane_counts);
                let path = template.save()?;
                Ok(path)
            }
            .await;
            match result {
                Ok(path) => {
                    let _ = tx
                        .send(ServerToClient::TemplateSaved { name: name.clone(), path })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerToClient::Error { message: e.to_string() })
                        .await;
                }
            }
        }

        LoadTemplate { name } => {
            match crate::template::LayoutTemplate::load(name) {
                Ok(_template) => {
                    let _ = tx
                        .send(ServerToClient::TemplateLoaded { name: name.clone() })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerToClient::Error { message: e.to_string() })
                        .await;
                }
            }
        }

        ListTemplates => {
            match crate::template::LayoutTemplate::list() {
                Ok(names) => {
                    let _ = tx.send(ServerToClient::TemplateList { names }).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerToClient::Error { message: e.to_string() })
                        .await;
                }
            }
        }

        ConnectSsh {
            host,
            port,
            username,
            auth_type,
            password,
            key_path,
            remote_forwards,
            x11_forward: _,
            x11_trusted: _,
        } => {
            use nexterm_ssh::{SshAuth, SshConfig, SshSession};
            use zeroize::Zeroizing;

            let auth = match auth_type.as_str() {
                "password" => {
                    let pw = password.clone().unwrap_or_default();
                    SshAuth::Password(Zeroizing::new(pw))
                }
                "key" => {
                    let kp = key_path.clone().unwrap_or_else(|| {
                        std::env::var_os("HOME")
                            .or_else(|| std::env::var_os("USERPROFILE"))
                            .map(|h| {
                                std::path::PathBuf::from(h)
                                    .join(".ssh")
                                    .join("id_rsa")
                            })
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default()
                    });
                    SshAuth::PrivateKey {
                        key_path: std::path::PathBuf::from(kp),
                        passphrase: None,
                    }
                }
                _ => SshAuth::Agent,
            };

            let ssh_config = SshConfig {
                host: host.clone(),
                port: *port,
                username: username.clone(),
                auth,
                proxy_jump: None,
                proxy_socks5: None,
            };

            match SshSession::connect(&ssh_config).await {
                Ok(mut session) => {
                    match session.authenticate(&ssh_config).await {
                        Ok(()) => {
                            for spec in remote_forwards {
                                if let Err(e) = session.start_remote_forward(spec).await {
                                    tracing::warn!("リモートフォワーディング失敗 '{}': {}", spec, e);
                                }
                            }
                            let _ = tx
                                .send(ServerToClient::Error {
                                    message: "SSH 認証成功。シェル統合は開発中です".to_string(),
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(ServerToClient::Error {
                                    message: format!("SSH 認証失敗: {}", e),
                                })
                                .await;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx
                        .send(ServerToClient::Error {
                            message: format!("SSH 接続失敗: {}", e),
                        })
                        .await;
                }
            }
        }

        ToggleZoom => {
            if let Some(ref name) = *current_session {
                let result = {
                    let arc = manager.sessions();
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
                    let _ = tx.send(ServerToClient::ZoomChanged { is_zoomed }).await;
                    let _ = tx.send(layout_msg).await;
                }
            }
        }

        SwapPane { target_pane_id } => {
            if let Some(ref name) = *current_session {
                let layout_msg = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        if let Some(w) = s.focused_window_mut() {
                            w.swap_focused_with(*target_pane_id);
                            Some(w.layout_changed_msg(cols, rows))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if let Some(msg) = layout_msg {
                    let _ = tx.send(msg).await;
                }
            }
        }

        BreakPane => {
            if let Some(ref name) = *current_session {
                let result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        let old_layout =
                            s.focused_window().map(|w| w.layout_changed_msg(cols, rows));
                        s.break_pane().ok().map(|new_win_id| {
                            let pane_id = s
                                .focused_window()
                                .map(|w| w.focused_pane_id())
                                .unwrap_or(0);
                            let new_layout =
                                s.focused_window().map(|w| w.layout_changed_msg(cols, rows));
                            let windows = s.window_list();
                            (new_win_id, pane_id, old_layout, new_layout, windows)
                        })
                    } else {
                        None
                    }
                };
                if let Some((new_win_id, pane_id, old_layout, new_layout, windows)) = result {
                    let _ = tx
                        .send(ServerToClient::PaneBroken { new_window_id: new_win_id, pane_id })
                        .await;
                    let _ = tx
                        .send(ServerToClient::WindowListChanged { windows })
                        .await;
                    if let Some(msg) = old_layout {
                        let _ = tx.send(msg).await;
                    }
                    if let Some(msg) = new_layout {
                        let _ = tx.send(msg).await;
                    }
                }
            }
        }

        JoinPane { target_window_id } => {
            if let Some(ref name) = *current_session {
                let result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        s.join_pane(*target_window_id).ok().map(|pane_id| {
                            let new_layout =
                                s.focused_window().map(|w| w.layout_changed_msg(cols, rows));
                            let windows = s.window_list();
                            (pane_id, new_layout, windows)
                        })
                    } else {
                        None
                    }
                };
                if let Some((pane_id, new_layout, windows)) = result {
                    let _ = tx
                        .send(ServerToClient::WindowListChanged { windows })
                        .await;
                    if let Some(msg) = new_layout {
                        let _ = tx.send(msg).await;
                    }
                    let refresh = {
                        let arc = manager.sessions();
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
                        let _ = tx.send(r).await;
                    }
                }
            }
        }

        SftpUpload { host_name, local_path, remote_path } => {
            if let Some(host_cfg) = hosts.iter().find(|h| &h.name == host_name) {
                let host_cfg = host_cfg.clone();
                let local = local_path.clone();
                let remote = remote_path.clone();
                let tx2 = tx.clone();
                let display = local_path.clone();

                tokio::spawn(async move {
                    let result =
                        super::sftp::run_sftp_upload(&host_cfg, &local, &remote, tx2.clone())
                            .await;
                    let _ = tx2
                        .send(ServerToClient::SftpDone {
                            path: display,
                            error: result.err().map(|e| e.to_string()),
                        })
                        .await;
                });
            } else {
                let _ = tx
                    .send(ServerToClient::Error {
                        message: format!("SFTP: ホスト '{}' が設定に見つかりません", host_name),
                    })
                    .await;
            }
        }

        SftpDownload { host_name, remote_path, local_path } => {
            if let Some(host_cfg) = hosts.iter().find(|h| &h.name == host_name) {
                let host_cfg = host_cfg.clone();
                let remote = remote_path.clone();
                let local = local_path.clone();
                let tx2 = tx.clone();
                let display = remote_path.clone();

                tokio::spawn(async move {
                    let result =
                        super::sftp::run_sftp_download(&host_cfg, &remote, &local, tx2.clone())
                            .await;
                    let _ = tx2
                        .send(ServerToClient::SftpDone {
                            path: display,
                            error: result.err().map(|e| e.to_string()),
                        })
                        .await;
                });
            } else {
                let _ = tx
                    .send(ServerToClient::Error {
                        message: format!("SFTP: ホスト '{}' が設定に見つかりません", host_name),
                    })
                    .await;
            }
        }

        RunMacro { macro_fn, display_name } => {
            if let Some(ref name) = *current_session {
                let focused_pane_id = {
                    let arc = manager.sessions();
                    let sessions = arc.lock().await;
                    sessions
                        .get(name)
                        .and_then(|s| s.focused_window())
                        .map(|w| w.focused_pane_id())
                };
                if let Some(pane_id) = focused_pane_id {
                    tracing::info!("RunMacro: {} (fn={})", display_name, macro_fn);
                    let lua_ref = lua.clone();
                    let fn_name = macro_fn.clone();
                    let session_name = name.clone();
                    let output = tokio::task::spawn_blocking(move || {
                        lua_ref.call_macro(&fn_name, &session_name, pane_id)
                    })
                    .await
                    .unwrap_or(None);

                    if let Some(text) = output {
                        let arc = manager.sessions();
                        let sessions = arc.lock().await;
                        if let Some(session) = sessions.get(name)
                            && let Some(window) = session.focused_window()
                                && let Some(pane) = window.pane(pane_id)
                        {
                            let _ = pane.write_input(text.as_bytes());
                        }
                    }
                }
            }
        }

        SetLayoutMode { mode } => {
            if let Some(ref name) = *current_session {
                let layout_msg = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        s.focused_window_mut().map(|w| {
                            w.set_layout_mode(
                                crate::window::LayoutMode::from_str(mode),
                                cols,
                                rows,
                            );
                            w.layout_changed_msg(cols, rows)
                        })
                    } else {
                        None
                    }
                };
                if let Some(msg) = layout_msg {
                    let _ = tx.send(msg).await;
                }
            }
        }

        OpenFloatingPane => {
            if let Some(ref name) = *current_session {
                let result = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    if let Some(s) = sessions.get_mut(name) {
                        let cols = s.cols;
                        let rows = s.rows;
                        let shell = s.shell().to_string();
                        let sender = s.broadcast_sender();
                        s.focused_window_mut()
                            .map(|w| w.open_floating_pane(cols, rows, sender, &shell))
                    } else {
                        None
                    }
                };
                match result {
                    Some(Ok((pane_id, rect))) => {
                        let _ = tx
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
                        let _ = tx
                            .send(ServerToClient::Error { message: e.to_string() })
                            .await;
                    }
                    None => {}
                }
            }
        }

        CloseFloatingPane { pane_id } => {
            if let Some(ref name) = *current_session {
                let closed = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    sessions.get_mut(name).and_then(|s| {
                        s.focused_window_mut()
                            .map(|w| w.close_floating_pane(*pane_id))
                    })
                };
                if closed == Some(true) {
                    let _ = tx
                        .send(ServerToClient::FloatingPaneClosed { pane_id: *pane_id })
                        .await;
                }
            }
        }

        MoveFloatingPane { pane_id, col_off, row_off } => {
            if let Some(ref name) = *current_session {
                let rect_opt = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    sessions.get_mut(name).and_then(|s| {
                        s.focused_window_mut()
                            .and_then(|w| w.move_floating_pane(*pane_id, *col_off, *row_off))
                    })
                };
                if let Some(rect) = rect_opt {
                    let _ = tx
                        .send(ServerToClient::FloatingPaneMoved {
                            pane_id: *pane_id,
                            col_off: rect.col_off,
                            row_off: rect.row_off,
                            cols: rect.cols,
                            rows: rect.rows,
                        })
                        .await;
                }
            }
        }

        ResizeFloatingPane { pane_id, cols, rows } => {
            if let Some(ref name) = *current_session {
                let rect_opt = {
                    let arc = manager.sessions();
                    let mut sessions = arc.lock().await;
                    sessions.get_mut(name).and_then(|s| {
                        s.focused_window_mut()
                            .and_then(|w| w.resize_floating_pane(*pane_id, *cols, *rows))
                    })
                };
                if let Some(rect) = rect_opt {
                    let _ = tx
                        .send(ServerToClient::FloatingPaneMoved {
                            pane_id: *pane_id,
                            col_off: rect.col_off,
                            row_off: rect.row_off,
                            cols: rect.cols,
                            rows: rect.rows,
                        })
                        .await;
                }
            }
        }

        ConnectSerial { port, baud_rate, data_bits, stop_bits, parity } => {
            if let Some(ref name) = *current_session {
                let result = manager
                    .connect_serial(name, port, *baud_rate, *data_bits, *stop_bits, parity)
                    .await;
                match result {
                    Ok(pane_id) => {
                        let _ = tx
                            .send(ServerToClient::SerialConnected {
                                pane_id,
                                port: port.clone(),
                            })
                            .await;
                        let layout_msg = {
                            let arc = manager.sessions();
                            let sessions = arc.lock().await;
                            sessions.get(name).and_then(|s| {
                                s.focused_window()
                                    .map(|w| w.layout_changed_msg(s.cols, s.rows))
                            })
                        };
                        if let Some(msg) = layout_msg {
                            let _ = tx.send(msg).await;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(ServerToClient::Error { message: e.to_string() })
                            .await;
                    }
                }
            }
        }
    }
}

// ---- セキュリティバリデーション ----

/// 録音出力パスのバリデーション（ディレクトリトラバーサル攻撃を防ぐ）
fn validate_recording_path(output_path: &str) -> anyhow::Result<()> {
    use std::path::{Component, Path};
    if output_path.is_empty() {
        return Err(anyhow::anyhow!("出力パスが空です"));
    }
    if Path::new(output_path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(anyhow::anyhow!(
            "セキュリティエラー: パスに '..' を含めることはできません: {}",
            output_path
        ));
    }

    let allowed = allowed_recording_dirs();
    let input_path = Path::new(output_path);

    if input_path.is_absolute() {
        let parent = input_path.parent().unwrap_or(input_path);
        let is_allowed = allowed.iter().any(|dir| parent.starts_with(dir));
        if !is_allowed {
            let first_allowed = &allowed[0];
            std::fs::create_dir_all(first_allowed).ok();
            return Err(anyhow::anyhow!(
                "セキュリティエラー: 録音ファイルは {} または {} 内に保存してください (指定パス: {})",
                allowed[0].display(),
                allowed.get(1).map(|p| p.display().to_string()).unwrap_or_default(),
                output_path
            ));
        }
        std::fs::create_dir_all(parent)?;
    }

    Ok(())
}

/// 録音ファイルを保存できる許可ディレクトリ一覧を返す
fn allowed_recording_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let rec_dir = std::path::PathBuf::from(home).join("nexterm").join("recordings");
        std::fs::create_dir_all(&rec_dir).ok();
        dirs.push(rec_dir);
    }

    let tmp_base = std::env::var_os("TMPDIR")
        .or_else(|| std::env::var_os("TEMP"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let tmp_dir = tmp_base.join("nexterm");
    std::fs::create_dir_all(&tmp_dir).ok();
    dirs.push(tmp_dir);

    #[cfg(unix)]
    {
        let unix_tmp = std::path::PathBuf::from("/tmp/nexterm");
        std::fs::create_dir_all(&unix_tmp).ok();
        dirs.push(unix_tmp);
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn パストラバーサルを含むパスは拒否される() {
        assert!(validate_recording_path("../../etc/passwd").is_err());
        assert!(validate_recording_path("../secret.txt").is_err());
        assert!(validate_recording_path("foo/../bar.txt").is_err());
    }

    #[test]
    fn 正常なパスは通過する() {
        assert!(validate_recording_path("recording.txt").is_ok());
        #[cfg(unix)]
        assert!(validate_recording_path("/tmp/nexterm/session.rec").is_ok());
        #[cfg(windows)]
        {
            let tmp = std::env::var("TEMP")
                .or_else(|_| std::env::var("TMP"))
                .unwrap_or_else(|_| "C:\\Temp".to_string());
            let allowed = format!("{}\\nexterm\\session.rec", tmp);
            assert!(validate_recording_path(&allowed).is_ok());
        }
    }

    #[test]
    fn 許可外の絶対パスは拒否される() {
        #[cfg(unix)]
        {
            assert!(validate_recording_path("/home/user/recording.txt").is_err());
            assert!(validate_recording_path("/etc/passwd").is_err());
        }
        #[cfg(windows)]
        {
            assert!(validate_recording_path("D:\\secret\\recording.txt").is_err());
            assert!(validate_recording_path("C:\\Windows\\System32\\recording.txt").is_err());
        }
    }

    #[test]
    fn 空パスは拒否される() {
        assert!(validate_recording_path("").is_err());
    }
}
