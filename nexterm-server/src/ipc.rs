//! IPC 層 — Unix Domain Socket (Linux/macOS) / Named Pipe (Windows) の切り替え

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{error, info};
#[cfg(unix)]
use tracing::warn;

use nexterm_proto::{ClientToServer, KeyCode, Modifiers, ServerToClient};
use crate::window::SplitDir;
use tokio::sync::mpsc;

use crate::session::SessionManager;

/// IPC サーバーを起動してクライアント接続を受け付ける
pub async fn serve(
    manager: std::sync::Arc<SessionManager>,
    hooks: std::sync::Arc<nexterm_config::HooksConfig>,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    log_config: std::sync::Arc<nexterm_config::LogConfig>,
    hosts: std::sync::Arc<Vec<nexterm_config::HostConfig>>,
) -> Result<()> {
    #[cfg(unix)]
    return serve_unix(manager, hooks, lua, log_config, hosts).await;

    #[cfg(windows)]
    return serve_named_pipe(manager, hooks, lua, log_config, hosts).await;
}

// ---- Unix Domain Socket 実装 ----

#[cfg(unix)]
async fn serve_unix(
    manager: std::sync::Arc<SessionManager>,
    hooks: std::sync::Arc<nexterm_config::HooksConfig>,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    log_config: std::sync::Arc<nexterm_config::LogConfig>,
    hosts: std::sync::Arc<Vec<nexterm_config::HostConfig>>,
) -> Result<()> {
    use tokio::net::UnixListener;

    let socket_path = unix_socket_path();
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;

    // サーバー自身の UID を取得する（接続元 UID の検証基準）
    // SAFETY: getuid() は常に成功し、安全である
    let server_uid = unsafe { libc::getuid() };

    info!("Unix ソケットでリッスン中: {}", socket_path);

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                // 接続元の UID がサーバー UID と一致しない場合は拒否する
                if !verify_peer_uid(&stream, server_uid) {
                    warn!("UID 不一致の接続を拒否しました（サーバー UID={}）", server_uid);
                    continue;
                }
                let manager = std::sync::Arc::clone(&manager);
                let hooks = std::sync::Arc::clone(&hooks);
                let lua = std::sync::Arc::clone(&lua);
                let log_config = std::sync::Arc::clone(&log_config);
                let hosts = std::sync::Arc::clone(&hosts);
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, manager, hooks, lua, log_config, hosts).await {
                        error!("クライアント処理エラー: {}", e);
                    }
                });
            }
            Err(e) => error!("接続受け付けエラー: {}", e),
        }
    }
}

/// Unix ドメインソケットの接続元 UID を検証する
///
/// 取得に成功した場合: `peer_uid == expected_uid` を返す。
/// 取得に失敗した場合（非対応 OS 等）: `true` を返し、0600 パーミッションに依存する。
#[cfg(unix)]
fn verify_peer_uid(stream: &tokio::net::UnixStream, expected_uid: libc::uid_t) -> bool {
    match peer_uid_impl(stream) {
        Some(uid) => uid == expected_uid,
        None => true, // 取得不可の環境ではパーミッション 0600 に依存する
    }
}

/// Linux: SO_PEERCRED で接続元の UID を取得する
#[cfg(target_os = "linux")]
fn peer_uid_impl(stream: &tokio::net::UnixStream) -> Option<libc::uid_t> {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: fd は有効な Unix ドメインソケット。cred のサイズは SO_PEERCRED に適合。
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if ret == 0 {
        Some(cred.uid)
    } else {
        warn!("SO_PEERCRED の取得に失敗しました (errno={})", unsafe { *libc::__errno_location() });
        None
    }
}

/// macOS / FreeBSD / NetBSD / OpenBSD: getpeereid() で接続元の UID を取得する
#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
fn peer_uid_impl(stream: &tokio::net::UnixStream) -> Option<libc::uid_t> {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: fd は有効な Unix ドメインソケット。
    let ret = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    if ret == 0 {
        Some(uid)
    } else {
        warn!("getpeereid の取得に失敗しました");
        None
    }
}

/// 上記以外の Unix 環境: UID 取得は非対応（パーミッション 0600 に依存）
#[cfg(all(
    unix,
    not(target_os = "linux"),
    not(target_os = "macos"),
    not(target_os = "freebsd"),
    not(target_os = "netbsd"),
    not(target_os = "openbsd"),
))]
fn peer_uid_impl(_stream: &tokio::net::UnixStream) -> Option<libc::uid_t> {
    None
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
async fn serve_named_pipe(
    manager: std::sync::Arc<SessionManager>,
    hooks: std::sync::Arc<nexterm_config::HooksConfig>,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    log_config: std::sync::Arc<nexterm_config::LogConfig>,
    hosts: std::sync::Arc<Vec<nexterm_config::HostConfig>>,
) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name = named_pipe_name();
    info!("Named Pipe でリッスン中: {}", pipe_name);

    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            // リモートクライアントを明示的に拒否する（同一マシンのみ許可）
            .reject_remote_clients(true)
            .create(&pipe_name)?;

        server.connect().await?;

        let manager = std::sync::Arc::clone(&manager);
        let hooks = std::sync::Arc::clone(&hooks);
        let lua = std::sync::Arc::clone(&lua);
        let log_config = std::sync::Arc::clone(&log_config);
        let hosts = std::sync::Arc::clone(&hosts);
        tokio::spawn(async move {
            if let Err(e) = handle_client(server, manager, hooks, lua, log_config, hosts).await {
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
async fn handle_client<S>(
    stream: S,
    manager: std::sync::Arc<SessionManager>,
    hooks: std::sync::Arc<nexterm_config::HooksConfig>,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    log_config: std::sync::Arc<nexterm_config::LogConfig>,
    hosts: std::sync::Arc<Vec<nexterm_config::HostConfig>>,
) -> Result<()>
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

    // broadcast forwarder タスクのハンドル（Attach 時に設定、切断時に abort）
    let mut bcast_forwarder: Option<tokio::task::AbortHandle> = None;

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

        dispatch(&msg, &manager, tx.clone(), &mut current_session, &hooks, std::sync::Arc::clone(&lua), &log_config, &hosts, &mut bcast_forwarder).await;
    }

    // クリーンアップ: broadcast forwarder を停止する（broadcast::Receiver は自動 drop）
    if let Some(h) = bcast_forwarder.take() {
        h.abort();
    }
    if let Some(ref name) = current_session {
        let arc = manager.sessions();
        let mut sessions = arc.lock().await;
        if let Some(session) = sessions.get_mut(name) {
            session.detach_one(&tx); // no-op: broadcast では Receiver が drop されるだけ
            info!("切断によりセッション '{}' からデタッチしました", name);
        }
        // on_detach フック（切断時）
        crate::hooks::on_detach(&hooks, &lua, name);
    }

    Ok(())
}

/// 録音出力パスのバリデーション（ディレクトリトラバーサル攻撃を防ぐ）
fn validate_recording_path(output_path: &str) -> Result<()> {
    use std::path::{Component, Path};
    if output_path.is_empty() {
        return Err(anyhow::anyhow!("出力パスが空です"));
    }
    // ".." コンポーネントを含むパスを禁止する
    if Path::new(output_path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(anyhow::anyhow!(
            "セキュリティエラー: パスに '..' を含めることはできません: {}",
            output_path
        ));
    }

    // 許可ディレクトリ: ~/nexterm/recordings/ または $TMPDIR/nexterm/
    let allowed = allowed_recording_dirs();
    let input_path = Path::new(output_path);

    // 絶対パスの場合のみ許可ディレクトリチェックを行う
    if input_path.is_absolute() {
        let parent = input_path.parent().unwrap_or(input_path);
        let is_allowed = allowed.iter().any(|dir| {
            // ディレクトリが許可プレフィックスで始まるか確認する
            parent.starts_with(dir)
        });
        if !is_allowed {
            // 許可ディレクトリを自動作成してから再チェック
            let first_allowed = &allowed[0];
            std::fs::create_dir_all(first_allowed).ok();
            return Err(anyhow::anyhow!(
                "セキュリティエラー: 録音ファイルは {} または {} 内に保存してください (指定パス: {})",
                allowed[0].display(),
                allowed.get(1).map(|p| p.display().to_string()).unwrap_or_default(),
                output_path
            ));
        }
        // 親ディレクトリを作成する
        std::fs::create_dir_all(parent)?;
    }

    Ok(())
}

/// 録音ファイルを保存できる許可ディレクトリ一覧を返す
fn allowed_recording_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();

    // ~/nexterm/recordings/
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let rec_dir = std::path::PathBuf::from(home).join("nexterm").join("recordings");
        std::fs::create_dir_all(&rec_dir).ok();
        dirs.push(rec_dir);
    }

    // $TMPDIR/nexterm/ または /tmp/nexterm/
    let tmp_base = std::env::var_os("TMPDIR")
        .or_else(|| std::env::var_os("TEMP"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let tmp_dir = tmp_base.join("nexterm");
    std::fs::create_dir_all(&tmp_dir).ok();
    dirs.push(tmp_dir);

    // /tmp/nexterm/ を常に許可する（macOS では $TMPDIR が /var/folders/... のため明示追加）
    #[cfg(unix)]
    {
        let unix_tmp = std::path::PathBuf::from("/tmp/nexterm");
        std::fs::create_dir_all(&unix_tmp).ok();
        dirs.push(unix_tmp);
    }

    dirs
}

/// クライアントからのメッセージをディスパッチする
async fn dispatch(
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
                    // PTY 出力（GridDiff, Bell 等）を broadcast → 本クライアントの mpsc に転送する
                    let bcast_rx = {
                        let arc = manager.sessions();
                        let sessions = arc.lock().await;
                        sessions.get(session_name).map(|s| s.attach())
                    };
                    if let Some(mut bcast_rx) = bcast_rx {
                        let fwd_tx = tx.clone();
                        // 既存の forwarder を中断してから新しいものを起動する
                        if let Some(h) = bcast_forwarder.take() {
                            let _: () = h.abort();
                        }
                        let handle = tokio::spawn(async move {
                            loop {
                                match bcast_rx.recv().await {
                                    Ok(msg) => {
                                        if fwd_tx.send(msg).await.is_err() {
                                            break; // クライアント切断
                                        }
                                    }
                                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                        tracing::warn!("broadcast: {} メッセージをスキップしました（バッファ溢れ）", n);
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
                let bytes = key_to_bytes(code, *modifiers);
                if !bytes.is_empty() {
                    let arc = manager.sessions();
                    let sessions = arc.lock().await;
                    if let Some(s) = sessions.get(name)
                        && let Err(e) = s.write_to_focused(&bytes) {
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
                        // on_pane_open フック
                        crate::hooks::on_pane_open(hooks, &lua, name, pane_id);
                        // auto_log が有効なら新ペインの録音を自動開始する
                        if log_config.auto_log {
                            if let Some(log_dir) = &log_config.log_dir {
                                if let Err(e) = manager
                                    .start_recording_with_log_config(name, log_dir, log_config)
                                    .await
                                {
                                    tracing::warn!("auto_log 録音開始失敗 (pane={}): {}", pane_id, e);
                                }
                            }
                        }
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
                    // ブラケットペーストモード有効時は ESC[200~ / ESC[201~ で囲む
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

        MouseReport {
            button,
            col,
            row,
            pressed,
            motion,
        } => {
            if let Some(ref name) = *current_session {
                let arc = manager.sessions();
                let sessions = arc.lock().await;
                if let Some(s) = sessions.get(name) {
                    let mode = s.focused_mouse_mode();
                    if mode > 0 {
                        // SGR モード（2）: CSI < Cb ; Cx ; Cy M/m
                        // X11 モード（1）: 同様だが座標が 8bit に制限される（ここでは SGR で代用）
                        let suffix = if *pressed || *motion { b'M' } else { b'm' };
                        // ボタンコードを計算する（SGR 拡張）
                        // motion 中は button に 32 を加算する
                        let cb = *button as u32 + if *motion { 32 } else { 0 };
                        let seq = format!("\x1b[<{};{};{}{}", cb, col + 1, row + 1, suffix as char);
                        if let Err(e) = s.write_to_focused(seq.as_bytes()) {
                            error!("マウスレポート送信エラー: {}", e);
                        }
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

        // 録音コマンド（パストラバーサルを検証してからセッション側に委譲する）
        StartRecording { session_name, output_path } => {
            // セキュリティ: ".." を含むパスを拒否する
            if let Err(e) = validate_recording_path(output_path) {
                let _ = tx
                    .send(ServerToClient::Error { message: e.to_string() })
                    .await;
                return;
            }
            match manager.start_recording(session_name, output_path).await {
                Ok(pane_id) => {
                    let _ = tx
                        .send(ServerToClient::RecordingStarted { pane_id, path: output_path.to_string() })
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
                                s.focused_window().map(|w| w.layout_changed_msg(s.cols, s.rows))
                            })
                        };
                        let _ = tx.send(ServerToClient::PaneClosed { pane_id: removed_id }).await;
                        if let Some(msg) = layout_msg {
                            let _ = tx.send(msg).await;
                        }
                    }
                    Some(Err(e)) => {
                        let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
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
                    sessions.get_mut(name).map(|s| s.add_window().map(|wid| (wid, s.window_list())))
                };
                match result {
                    Some(Ok((_wid, windows))) => {
                        let _ = tx.send(ServerToClient::WindowListChanged { windows }).await;
                        // 新ウィンドウの最初のペインに FullRefresh を送る
                        let refresh_msg = {
                            let arc = manager.sessions();
                            let sessions = arc.lock().await;
                            sessions.get(name).and_then(|s| {
                                s.focused_window().and_then(|w| {
                                    let pid = w.focused_pane_id();
                                    w.pane(pid).map(|p| {
                                        let layout = w.layout_changed_msg(s.cols, s.rows);
                                        let refresh = ServerToClient::FullRefresh { pane_id: p.id, grid: p.make_full_refresh() };
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
                        let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
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
                        let _ = tx.send(ServerToClient::WindowListChanged { windows }).await;
                    }
                    Err(e) => {
                        let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
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
                                let refresh = w.pane(pid).map(|p| ServerToClient::FullRefresh { pane_id: p.id, grid: p.make_full_refresh() });
                                (windows, layout, refresh)
                            })
                        })
                    } else {
                        Ok(None)
                    }
                };
                match result {
                    Ok(Some((windows, layout, refresh))) => {
                        let _ = tx.send(ServerToClient::WindowListChanged { windows }).await;
                        let _ = tx.send(layout).await;
                        if let Some(r) = refresh {
                            let _ = tx.send(r).await;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
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
                        let _ = tx.send(ServerToClient::WindowListChanged { windows }).await;
                    }
                    Err(e) => {
                        let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
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
                    let _ = tx.send(ServerToClient::BroadcastModeChanged { enabled: *enabled }).await;
                }
            }
        }

        DisplayPanes { .. } => {
            // サーバー側での処理は不要（クライアント側のオーバーレイ表示のみ）
        }

        StartAsciicast { session_name, output_path } => {
            // セキュリティ: ".." を含むパスを拒否する
            if let Err(e) = validate_recording_path(output_path) {
                let _ = tx
                    .send(ServerToClient::Error { message: e.to_string() })
                    .await;
                return;
            }
            match manager.start_asciicast(session_name, output_path).await {
                Ok(pane_id) => {
                    let _ = tx
                        .send(ServerToClient::AsciicastStarted { pane_id, path: output_path.to_string() })
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

        // レイアウトテンプレートを現在のセッションから保存する
        SaveTemplate { name } => {
            let result: anyhow::Result<String> = async {
                let session_name = current_session.as_deref()
                    .ok_or_else(|| anyhow::anyhow!("セッションにアタッチしていません"))?;
                let (window_titles, pane_counts) = {
                    let arc = manager.sessions();
                    let sessions = arc.lock().await;
                    let session = sessions.get(session_name)
                        .ok_or_else(|| anyhow::anyhow!("セッションが見つかりません: {}", session_name))?;
                    let info = session.window_list();
                    let titles: Vec<String> = info.iter().map(|w| w.name.clone()).collect();
                    let counts: Vec<usize> = info.iter().map(|w| w.pane_count as usize).collect();
                    (titles, counts)
                };
                let template = crate::template::template_from_session_info(name, window_titles, pane_counts);
                let path = template.save()?;
                Ok(path)
            }.await;
            match result {
                Ok(path) => {
                    let _ = tx.send(ServerToClient::TemplateSaved { name: name.clone(), path }).await;
                }
                Err(e) => {
                    let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
                }
            }
        }

        // 保存済みテンプレートを読み込む（現バージョンでは通知のみ、ペイン生成は将来実装）
        LoadTemplate { name } => {
            match crate::template::LayoutTemplate::load(name) {
                Ok(_template) => {
                    let _ = tx.send(ServerToClient::TemplateLoaded { name: name.clone() }).await;
                }
                Err(e) => {
                    let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
                }
            }
        }

        // 保存済みテンプレート一覧を返す
        ListTemplates => {
            match crate::template::LayoutTemplate::list() {
                Ok(names) => {
                    let _ = tx.send(ServerToClient::TemplateList { names }).await;
                }
                Err(e) => {
                    let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
                }
            }
        }

        ConnectSsh { host, port, username, auth_type, password, key_path, remote_forwards, x11_forward, x11_trusted } => {
            use nexterm_ssh::{SshAuth, SshConfig, SshSession};
            use zeroize::Zeroizing;

            // 認証方式をパースする
            let auth = match auth_type.as_str() {
                "password" => {
                    let pw = password.clone().unwrap_or_default();
                    SshAuth::Password(Zeroizing::new(pw))
                }
                "key" => {
                    let kp = key_path.clone().unwrap_or_else(|| {
                        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
                            .map(|h| std::path::PathBuf::from(h).join(".ssh").join("id_rsa"))
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
                            // リモートポートフォワーディングを起動する
                            for spec in remote_forwards {
                                if let Err(e) = session.start_remote_forward(spec).await {
                                    tracing::warn!("リモートフォワーディング失敗 '{}': {}", spec, e);
                                }
                            }

                            // SSH シェルを開く（ペイン生成はシェル接続後に実装予定）
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
                        // 元ウィンドウのレイアウトを先に取得（borrow checker 対策）
                        let old_layout = s.focused_window().map(|w| w.layout_changed_msg(cols, rows));
                        s.break_pane().ok().map(|new_win_id| {
                            let pane_id = s.focused_window()
                                .and_then(|w| Some(w.focused_pane_id()))
                                .unwrap_or(0);
                            let new_layout = s.focused_window()
                                .map(|w| w.layout_changed_msg(cols, rows));
                            let windows = s.window_list();
                            (new_win_id, pane_id, old_layout, new_layout, windows)
                        })
                    } else {
                        None
                    }
                };
                if let Some((new_win_id, pane_id, old_layout, new_layout, windows)) = result {
                    let _ = tx.send(ServerToClient::PaneBroken { new_window_id: new_win_id, pane_id }).await;
                    let _ = tx.send(ServerToClient::WindowListChanged { windows }).await;
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
                            let new_layout = s.focused_window()
                                .map(|w| w.layout_changed_msg(cols, rows));
                            let windows = s.window_list();
                            (pane_id, new_layout, windows)
                        })
                    } else {
                        None
                    }
                };
                if let Some((pane_id, new_layout, windows)) = result {
                    let _ = tx.send(ServerToClient::WindowListChanged { windows }).await;
                    if let Some(msg) = new_layout {
                        let _ = tx.send(msg).await;
                    }
                    // 移動したペインのグリッドを再送する
                    let refresh = {
                        let arc = manager.sessions();
                        let sessions = arc.lock().await;
                        sessions.get(name).and_then(|s| {
                            s.focused_window().and_then(|w| {
                                w.pane(pane_id).map(|p| ServerToClient::FullRefresh { pane_id, grid: p.make_full_refresh() })
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
            // 設定ファイルからホスト設定を探す
            if let Some(host_cfg) = hosts.iter().find(|h| &h.name == host_name) {
                let host_cfg = host_cfg.clone();
                let local = local_path.clone();
                let remote = remote_path.clone();
                let tx2 = tx.clone();
                let display = local_path.clone();

                tokio::spawn(async move {
                    let result = run_sftp_upload(&host_cfg, &local, &remote, tx2.clone()).await;
                    let _ = tx2.send(ServerToClient::SftpDone {
                        path: display,
                        error: result.err().map(|e| e.to_string()),
                    }).await;
                });
            } else {
                let _ = tx.send(ServerToClient::Error {
                    message: format!("SFTP: ホスト '{}' が設定に見つかりません", host_name),
                }).await;
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
                    let result = run_sftp_download(&host_cfg, &remote, &local, tx2.clone()).await;
                    let _ = tx2.send(ServerToClient::SftpDone {
                        path: display,
                        error: result.err().map(|e| e.to_string()),
                    }).await;
                });
            } else {
                let _ = tx.send(ServerToClient::Error {
                    message: format!("SFTP: ホスト '{}' が設定に見つかりません", host_name),
                }).await;
            }
        }
        RunMacro { macro_fn, display_name } => {
            // Lua マクロを実行してフォーカスペインに PTY 入力として送信する
            if let Some(ref name) = *current_session {
                let focused_pane_id = {
                    let arc = manager.sessions();
                    let sessions = arc.lock().await;
                    sessions.get(name)
                        .and_then(|s| s.focused_window())
                        .map(|w| w.focused_pane_id())
                };
                if let Some(pane_id) = focused_pane_id {
                    tracing::info!("RunMacro: {} (fn={})", display_name, macro_fn);
                    // Lua マクロ呼び出し（spawn_blocking で同期 API を呼ぶ）
                    let lua_ref = lua.clone();
                    let fn_name = macro_fn.clone();
                    let session_name = name.clone();
                    let output = tokio::task::spawn_blocking(move || {
                        lua_ref.call_macro(&fn_name, &session_name, pane_id)
                    })
                    .await
                    .unwrap_or(None);

                    if let Some(text) = output {
                        // マクロの出力をフォーカスペインの PTY に書き込む
                        let arc = manager.sessions();
                        let sessions = arc.lock().await;
                        if let Some(session) = sessions.get(name) {
                            if let Some(window) = session.focused_window() {
                                if let Some(pane) = window.pane(pane_id) {
                                    let _ = pane.write_input(text.as_bytes());
                                }
                            }
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

        ConnectSerial { port, baud_rate, data_bits, stop_bits, parity } => {
            if let Some(ref name) = *current_session {
                let result = manager.connect_serial(
                    name,
                    port,
                    *baud_rate,
                    *data_bits,
                    *stop_bits,
                    parity,
                ).await;
                match result {
                    Ok(pane_id) => {
                        let _ = tx.send(ServerToClient::SerialConnected {
                            pane_id,
                            port: port.clone(),
                        }).await;
                        // レイアウト更新を送信する
                        let layout_msg = {
                            let arc = manager.sessions();
                            let sessions = arc.lock().await;
                            sessions.get(name).and_then(|s| {
                                s.focused_window().map(|w| w.layout_changed_msg(s.cols, s.rows))
                            })
                        };
                        if let Some(msg) = layout_msg {
                            let _ = tx.send(msg).await;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(ServerToClient::Error { message: e.to_string() }).await;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SFTP ヘルパー関数
// ---------------------------------------------------------------------------

/// HostConfig から SshConfig を構築してアップロードを実行する
async fn run_sftp_upload(
    host: &nexterm_config::HostConfig,
    local_path: &str,
    remote_path: &str,
    tx: tokio::sync::mpsc::Sender<ServerToClient>,
) -> anyhow::Result<()> {
    use nexterm_ssh::{SshAuth, SshConfig, SshSession};
    use std::path::PathBuf;
    use zeroize::Zeroizing;

    let auth = match host.auth_type.as_str() {
        "password" => SshAuth::Password(Zeroizing::new(String::new())),
        "key" => SshAuth::PrivateKey {
            key_path: PathBuf::from(host.key_path.clone().unwrap_or_else(|| {
                let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
                format!("{}/.ssh/id_rsa", home)
            })),
            passphrase: None,
        },
        _ => SshAuth::Agent,
    };

    let ssh_config = SshConfig {
        host: host.host.clone(),
        port: host.port,
        username: host.username.clone(),
        auth,
        proxy_jump: host.proxy_jump.clone(),
        proxy_socks5: None,
    };

    let mut session = SshSession::connect(&ssh_config).await?;
    session.authenticate(&ssh_config).await?;

    // 進捗チャネル
    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<(u64, u64)>(32);
    let tx2 = tx.clone();
    let path_display = local_path.to_string();
    tokio::spawn(async move {
        while let Some((transferred, total)) = prog_rx.recv().await {
            let _ = tx2.send(ServerToClient::SftpProgress {
                path: path_display.clone(),
                transferred,
                total,
            }).await;
        }
    });

    session.upload_file(
        std::path::Path::new(local_path),
        remote_path,
        Some(prog_tx),
    ).await
}

/// HostConfig から SshConfig を構築してダウンロードを実行する
async fn run_sftp_download(
    host: &nexterm_config::HostConfig,
    remote_path: &str,
    local_path: &str,
    tx: tokio::sync::mpsc::Sender<ServerToClient>,
) -> anyhow::Result<()> {
    use nexterm_ssh::{SshAuth, SshConfig, SshSession};
    use std::path::PathBuf;
    use zeroize::Zeroizing;

    let auth = match host.auth_type.as_str() {
        "password" => SshAuth::Password(Zeroizing::new(String::new())),
        "key" => SshAuth::PrivateKey {
            key_path: PathBuf::from(host.key_path.clone().unwrap_or_else(|| {
                let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
                format!("{}/.ssh/id_rsa", home)
            })),
            passphrase: None,
        },
        _ => SshAuth::Agent,
    };

    let ssh_config = SshConfig {
        host: host.host.clone(),
        port: host.port,
        username: host.username.clone(),
        auth,
        proxy_jump: host.proxy_jump.clone(),
        proxy_socks5: None,
    };

    let mut session = SshSession::connect(&ssh_config).await?;
    session.authenticate(&ssh_config).await?;

    // 進捗チャネル
    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<(u64, u64)>(32);
    let tx2 = tx.clone();
    let path_display = remote_path.to_string();
    tokio::spawn(async move {
        while let Some((transferred, total)) = prog_rx.recv().await {
            let _ = tx2.send(ServerToClient::SftpProgress {
                path: path_display.clone(),
                transferred,
                total,
            }).await;
        }
    });

    session.download_file(
        remote_path,
        std::path::Path::new(local_path),
        Some(prog_tx),
    ).await
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
        // 相対パスは許可ディレクトリチェックをスキップ（実行時のCWD依存）
        assert!(validate_recording_path("recording.txt").is_ok());
        // 許可ディレクトリ内の絶対パスは通過する
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
