//! プラットフォーム別 IPC リスナー実装
//!
//! - Unix: Unix Domain Socket (`$XDG_RUNTIME_DIR/nexterm.sock`, 0600)
//! - Windows: Named Pipe (`\\.\pipe\nexterm-<USERNAME>`)

use anyhow::Result;
#[cfg(unix)]
use tracing::warn;
use tracing::{error, info};

use crate::runtime_config::SharedRuntimeConfig;
use crate::session::SessionManager;

// ---- Unix Domain Socket 実装 ----

#[cfg(unix)]
pub(super) async fn serve_unix(
    manager: std::sync::Arc<SessionManager>,
    runtime_cfg: SharedRuntimeConfig,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
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
                    warn!(
                        "UID 不一致の接続を拒否しました（サーバー UID={}）",
                        server_uid
                    );
                    continue;
                }
                let manager = std::sync::Arc::clone(&manager);
                let runtime_cfg = std::sync::Arc::clone(&runtime_cfg);
                let lua = std::sync::Arc::clone(&lua);
                tokio::spawn(async move {
                    if let Err(e) =
                        super::handler::handle_client(stream, manager, runtime_cfg, lua).await
                    {
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
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
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
        // SAFETY: __errno_location() はスレッドローカルな errno ポインタを返す。逆参照は常に安全。
        warn!("SO_PEERCRED の取得に失敗しました (errno={})", unsafe {
            *libc::__errno_location()
        });
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
pub(super) fn unix_socket_path() -> String {
    // SAFETY: getuid() は常に成功し、副作用なし。
    let uid = unsafe { libc::getuid() };
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", uid));
    format!("{}/nexterm.sock", runtime_dir)
}

// ---- Windows Named Pipe 実装 ----

#[cfg(windows)]
pub(super) async fn serve_named_pipe(
    manager: std::sync::Arc<SessionManager>,
    runtime_cfg: SharedRuntimeConfig,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
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
        let runtime_cfg = std::sync::Arc::clone(&runtime_cfg);
        let lua = std::sync::Arc::clone(&lua);
        tokio::spawn(async move {
            if let Err(e) = super::handler::handle_client(server, manager, runtime_cfg, lua).await {
                error!("クライアント処理エラー: {}", e);
            }
        });
    }
}

#[cfg(windows)]
pub(super) fn named_pipe_name() -> String {
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
    format!("\\\\.\\pipe\\nexterm-{}", username)
}

#[cfg(test)]
mod tests {
    // platform-specific tests

    #[cfg(unix)]
    mod unix_tests {
        use super::super::*;

        #[test]
        fn unix_socket_path_contains_nexterm() {
            let path = unix_socket_path();
            assert!(path.contains("nexterm"));
        }

        #[test]
        fn unix_socket_path_ends_with_sock() {
            let path = unix_socket_path();
            assert!(path.ends_with(".sock"));
        }

        #[test]
        fn unix_socket_path_is_absolute() {
            let path = unix_socket_path();
            // Unixパスは / で始まる
            assert!(path.starts_with('/'));
        }

        #[test]
        fn unix_socket_path_includes_run_or_tmp() {
            let path = unix_socket_path();
            // XDG_RUNTIME_DIRまたは /run/user/<uid> または /tmp
            assert!(path.contains("run") || path.contains("tmp"));
        }
    }

    #[cfg(windows)]
    mod windows_tests {
        use super::super::*;

        #[test]
        fn named_pipe_has_correct_prefix() {
            let name = named_pipe_name();
            assert!(name.starts_with("\\\\.\\pipe\\nexterm-"));
        }

        #[test]
        fn named_pipe_includes_username() {
            let name = named_pipe_name();
            // プレフィックスの後にユーザー名がある
            let prefix = "\\\\.\\pipe\\nexterm-";
            assert!(name.len() > prefix.len());
        }

        #[test]
        fn named_pipe_does_not_end_with_hyphen() {
            let name = named_pipe_name();
            // ユーザー名が取得できない場合でも "nexterm" がフォールバック
            assert!(!name.ends_with('-'));
        }
    }

    // クロスプラットフォームテスト
    #[test]
    fn platform_detection_works() {
        // コンパイル時に適切なプラットフォームが検出されている
        let platform = if cfg!(unix) {
            "unix"
        } else if cfg!(windows) {
            "windows"
        } else {
            "unknown"
        };
        assert!(matches!(platform, "unix" | "windows"));
    }
}
