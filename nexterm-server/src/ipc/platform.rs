//! Platform-specific IPC listener implementations.
//!
//! - Unix: Unix Domain Socket (`$XDG_RUNTIME_DIR/nexterm.sock`, mode 0600).
//! - Windows: Named Pipe (`\\.\pipe\nexterm-<USERNAME>`).

use anyhow::Result;
#[cfg(unix)]
use tracing::warn;
use tracing::{error, info};

use crate::runtime_config::SharedRuntimeConfig;
use crate::session::SessionManager;

// ---- Unix Domain Socket implementation ----

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

    // Take the server's own UID (the baseline for peer UID validation).
    // SAFETY: getuid() always succeeds and is safe.
    let server_uid = unsafe { libc::getuid() };

    info!("listening on Unix socket: {}", socket_path);

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                // Reject connections whose peer UID does not match the server's UID.
                if !verify_peer_uid(&stream, server_uid) {
                    warn!(
                        "rejected connection with mismatched UID (server UID={})",
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
                        error!("client handling error: {}", e);
                    }
                });
            }
            Err(e) => error!("accept error: {}", e),
        }
    }
}

/// Validate the peer UID on a Unix domain socket connection.
///
/// On success: returns `peer_uid == expected_uid`.
/// On failure (unsupported OS, etc.): returns `true` and relies on the 0600 permission.
#[cfg(unix)]
fn verify_peer_uid(stream: &tokio::net::UnixStream, expected_uid: libc::uid_t) -> bool {
    match peer_uid_impl(stream) {
        Some(uid) => uid == expected_uid,
        None => true, // On environments without peer UID support, rely on permission 0600.
    }
}

/// Linux: obtain the peer UID via SO_PEERCRED.
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
    // SAFETY: fd is a valid Unix domain socket; the size of cred matches SO_PEERCRED.
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
        // SAFETY: __errno_location() returns a thread-local errno pointer; dereferencing is always safe.
        warn!("failed to get SO_PEERCRED (errno={})", unsafe {
            *libc::__errno_location()
        });
        None
    }
}

/// macOS / FreeBSD / NetBSD / OpenBSD: obtain the peer UID via getpeereid().
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
    // SAFETY: fd is a valid Unix domain socket.
    let ret = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    if ret == 0 {
        Some(uid)
    } else {
        warn!("getpeereid failed");
        None
    }
}

/// Other Unix environments: peer UID is unsupported (we rely on permission 0600).
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
    // SAFETY: getuid() always succeeds and is side-effect free.
    let uid = unsafe { libc::getuid() };
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", uid));
    format!("{}/nexterm.sock", runtime_dir)
}

// ---- Windows Named Pipe implementation ----

#[cfg(windows)]
pub(super) async fn serve_named_pipe(
    manager: std::sync::Arc<SessionManager>,
    runtime_cfg: SharedRuntimeConfig,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name = named_pipe_name();
    info!("listening on named pipe: {}", pipe_name);

    let mut iteration: u64 = 0;
    loop {
        // P1-A diagnostic: surface the underlying error when `create` fails.
        // `first_pipe_instance(false)` lets multiple nexterm processes share the
        // same pipe name, but if the OS still rejects the create (e.g. ACL
        // mismatch, name collision with a non-nexterm process) we previously
        // bubbled the error up with `?` and the calling task died silently.
        // Now we log it explicitly with the pipe name and current iteration so
        // the 2026-06-03 "no listening log" symptom can be distinguished from
        // "listening logged once, then create failed on the next loop".
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            // Explicitly reject remote clients (allow same-machine only).
            .reject_remote_clients(true)
            .create(&pipe_name)
            .map_err(|e| {
                error!(
                    "ServerOptions::create failed on iteration {} for pipe {}: {} (raw os error: {:?})",
                    iteration,
                    pipe_name,
                    e,
                    e.raw_os_error()
                );
                e
            })?;
        iteration = iteration.wrapping_add(1);

        server.connect().await?;

        let manager = std::sync::Arc::clone(&manager);
        let runtime_cfg = std::sync::Arc::clone(&runtime_cfg);
        let lua = std::sync::Arc::clone(&lua);
        tokio::spawn(async move {
            if let Err(e) = super::handler::handle_client(server, manager, runtime_cfg, lua).await {
                error!("client handling error: {}", e);
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
    // Platform-specific tests.

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
            // Unix paths start with '/'.
            assert!(path.starts_with('/'));
        }

        #[test]
        fn unix_socket_path_includes_run_or_tmp() {
            let path = unix_socket_path();
            // Either XDG_RUNTIME_DIR or /run/user/<uid> or /tmp.
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
            // After the prefix there should be a username.
            let prefix = "\\\\.\\pipe\\nexterm-";
            assert!(name.len() > prefix.len());
        }

        #[test]
        fn named_pipe_does_not_end_with_hyphen() {
            let name = named_pipe_name();
            // Even when no username is available, "nexterm" is used as fallback.
            assert!(!name.ends_with('-'));
        }
    }

    // Cross-platform test.
    #[test]
    fn platform_detection_works() {
        // The appropriate platform must be detected at compile time.
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
