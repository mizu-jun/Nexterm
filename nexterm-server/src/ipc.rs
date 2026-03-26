//! IPC 層 — Unix Domain Socket (Linux/macOS) / Named Pipe (Windows) の切り替え

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{error, info};

use nexterm_proto::{ClientToServer, KeyCode, Modifiers, ServerToClient};
use crate::window::SplitDir;
use tokio::sync::mpsc;

use crate::session::SessionManager;

/// IPC サーバーを起動してクライアント接続を受け付ける
pub async fn serve(manager: std::sync::Arc<SessionManager>) -> Result<()> {

    #[cfg(unix)]
    return serve_unix(manager).await;

    #[cfg(windows)]
    return serve_named_pipe(manager).await;
}

// ---- Unix Domain Socket 実装 ----

#[cfg(unix)]
async fn serve_unix(manager: std::sync::Arc<SessionManager>) -> Result<()> {
    use tokio::net::UnixListener;

    let socket_path = unix_socket_path();
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;

    info!("Unix ソケットでリッスン中: {}", socket_path);

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let manager = std::sync::Arc::clone(&manager);
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, manager).await {
                        error!("クライアント処理エラー: {}", e);
                    }
                });
            }
            Err(e) => error!("接続受け付けエラー: {}", e),
        }
    }
}

#[cfg(unix)]
fn unix_socket_path() -> String {
    let uid = unsafe { libc::getuid() };
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", uid));
    format!("{}/nexterm.sock", runtime_dir)
}

// ---- Windows Named Pipe 実装 ----

#[cfg(windows)]
async fn serve_named_pipe(manager: std::sync::Arc<SessionManager>) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name = named_pipe_name();
    info!("Named Pipe でリッスン中: {}", pipe_name);

    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&pipe_name)?;

        server.connect().await?;

        let manager = std::sync::Arc::clone(&manager);
        tokio::spawn(async move {
            if let Err(e) = handle_client(server, manager).await {
                error!("クライアント処理エラー: {}", e);
            }
        });
    }
}

#[cfg(windows)]
fn named_pipe_name() -> String {
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
    format!("\\\\.\\pipe\\nexterm-{}", username)
}

// ---- 共通クライアントハンドラ ----

/// 接続済みクライアントの読み書きを処理する
async fn handle_client<S>(stream: S, manager: std::sync::Arc<SessionManager>) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel::<ServerToClient>(256);
    let (mut read_half, mut write_half) = tokio::io::split(stream);

    // サーバー → クライアント 送信タスク
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match bincode::serialize(&msg) {
                Ok(payload) => {
                    let len = payload.len() as u32;
                    if write_half.write_all(&len.to_le_bytes()).await.is_err() {
                        break;
                    }
                    if write_half.write_all(&payload).await.is_err() {
                        break;
                    }
                }
                Err(e) => error!("シリアライズエラー: {}", e),
            }
        }
    });

    // 接続中のセッション名（Attach で設定される）
    let mut current_session: Option<String> = None;

    // クライアント → サーバー 受信ループ
    loop {
        let mut len_buf = [0u8; 4];
        if read_half.read_exact(&mut len_buf).await.is_err() {
            info!("クライアントが切断しました");
            break;
        }
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        let mut payload = vec![0u8; msg_len];
        if read_half.read_exact(&mut payload).await.is_err() {
            break;
        }
        let msg: ClientToServer = match bincode::deserialize(&payload) {
            Ok(m) => m,
            Err(e) => {
                error!("デシリアライズエラー: {}", e);
                continue;
            }
        };

        dispatch(&msg, &manager, tx.clone(), &mut current_session).await;
    }

    // クリーンアップ: セッションをデタッチ
    if let Some(ref name) = current_session {
        let arc = manager.sessions();
        let mut sessions = arc.lock().await;
        if let Some(session) = sessions.get_mut(name) {
            session.detach();
            info!("切断によりセッション '{}' をデタッチしました", name);
        }
    }

    Ok(())
}

/// クライアントからのメッセージをディスパッチする
async fn dispatch(
    msg: &ClientToServer,
    manager: &SessionManager,
    tx: mpsc::Sender<ServerToClient>,
    current_session: &mut Option<String>,
) {
    use ClientToServer::*;

    match msg {
        Ping => {
            let _ = tx.send(ServerToClient::Pong).await;
        }

        Attach { session_name } => {
            // セッションが存在しない場合は新規作成してアタッチ
            match manager
                .get_or_create_and_attach(session_name, 80, 24, tx.clone())
                .await
            {
                Ok(()) => {
                    *current_session = Some(session_name.clone());

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
                    s.detach();
                    info!("セッション '{}' をデタッチしました", name);
                }
            }
        }

        KeyEvent { code, modifiers } => {
            if let Some(ref name) = *current_session {
                let bytes = key_to_bytes(code, *modifiers);
                if !bytes.is_empty() {
                    let arc = manager.sessions();
                    let sessions = arc.lock().await;
                    if let Some(s) = sessions.get(name) {
                        if let Err(e) = s.write_to_focused(&bytes) {
                            error!("PTY 書き込みエラー: {}", e);
                        }
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
                        let pane_tx = s.client_tx.clone();
                        if let Some(pane_tx) = pane_tx {
                            s.focused_window_mut()
                                .map(|w| w.add_pane(cols, rows, pane_tx, &shell, dir))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                match split_result {
                    Some(Ok(pane_id)) => {
                        // FullRefresh と LayoutChanged を送信する
                        let msgs = {
                            let arc = manager.sessions();
                            let sessions = arc.lock().await;
                            sessions.get(name).and_then(|s| {
                                s.focused_window().map(|w| {
                                    let layout_msg = w.layout_changed_msg(s.cols, s.rows);
                                    // 新ペインのサイズをレイアウトから取得する
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
                    if let Err(e) = s.write_to_focused(text.as_bytes()) {
                        error!("ペーストエラー: {}", e);
                    }
                }
            }
        }

        ListSessions => {
            // アタッチせずにセッション一覧だけ返す
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
    }
}

/// キーコードと修飾キーを VT100/xterm エスケープシーケンスに変換する
fn key_to_bytes(code: &KeyCode, mods: Modifiers) -> Vec<u8> {
    match code {
        KeyCode::Char(ch) => {
            if mods.is_ctrl() {
                // Ctrl+文字 → ASCII コントロールコード (1–26)
                let c = (*ch as u8) & 0x1f;
                if c > 0 {
                    return vec![c];
                }
            }
            ch.to_string().into_bytes()
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Escape => vec![0x1b],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => vec![],
        },
    }
}
