//! Integration tests for `nexterm-client-core` (Phase 3).
//!
//! Covers the IPC wire-format contract end-to-end on Unix Domain Sockets:
//! when a client calls [`Connection::connect`], the very first bytes on the
//! socket must be `[4-byte little-endian length] [postcard-serialized Hello]`
//! and the `Hello` must carry the current [`PROTOCOL_VERSION`].
//!
//! Why a dedicated test exists: the framing is duplicated in spirit between
//! `nexterm-client-core::setup` (write side) and `nexterm-server::ipc::handler`
//! (read side). If anyone changes the prefix encoding or accidentally swaps
//! endianness the existing unit tests would still pass; this test guards the
//! contract from the outside.
//!
//! Skipped on Windows because the named-pipe path is exercised by the
//! `nexterm-server` integration tests; pulling in `tokio::net::windows::named_pipe`
//! here would significantly enlarge the test surface for limited extra signal.

#![cfg(unix)]

use nexterm_client_core::Connection;
use nexterm_proto::{ClientKind, ClientToServer, PROTOCOL_VERSION};
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tokio::sync::Mutex;

/// Serialises the tests that rewrite `XDG_RUNTIME_DIR`.
///
/// `set_var` is process-global and the harness runs the tests of one binary on
/// parallel threads, so without this both tests below could be between their
/// own `set_var` and their own read when the other one overwrites the variable
/// — whoever lost the race then resolved the socket path into the other's
/// tempdir. It surfaced as ubuntu-only CI failures that alternated between the
/// two tests and passed on rerun (PR #57 hit one, PR #60 the other).
///
/// Same remedy as `nexterm-server/tests/snapshot_roundtrip.rs`, which hit this
/// on Windows with `APPDATA`. A `tokio::sync::Mutex` rather than a `std` one
/// because these tests hold the guard across an `.await`.
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

/// Point `XDG_RUNTIME_DIR` at `dir`, restoring the previous value on drop.
///
/// Only sound while `ENV_LOCK` is held — construct it after taking the guard.
struct RuntimeDirVar(Option<String>);

impl RuntimeDirVar {
    fn set(dir: &std::path::Path) -> Self {
        let previous = std::env::var("XDG_RUNTIME_DIR").ok();
        // SAFETY: `ENV_LOCK` serialises every test in this file that touches
        // the environment, so no other thread here reads or writes it
        // concurrently.
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", dir);
        }
        Self(previous)
    }
}

impl Drop for RuntimeDirVar {
    fn drop(&mut self) {
        // SAFETY: as above — still inside the `ENV_LOCK` critical section,
        // because the guard outlives this value in both tests.
        unsafe {
            match self.0.take() {
                Some(previous) => std::env::set_var("XDG_RUNTIME_DIR", previous),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
        }
    }
}

/// Read one `[len LE u32][payload]` frame from the socket and deserialize
/// it as `ClientToServer`.
async fn read_one_frame(stream: &mut tokio::net::UnixStream) -> ClientToServer {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.expect("len prefix");
    let len = u32::from_le_bytes(len_buf) as usize;
    assert!(
        len < 4096,
        "Hello payload should never be this large: {len}"
    );
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await.expect("payload");
    postcard::from_bytes(&payload).expect("postcard decode")
}

#[tokio::test]
async fn connect_sends_hello_with_current_protocol_version_first() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let sock_path = dir.path().join("nexterm.sock");

    // Stand up a one-shot listener that accepts a single client and reads
    // the first frame it sees.
    let listener = UnixListener::bind(&sock_path).expect("bind");
    let server_task = tokio::spawn(async move {
        let (mut stream, _addr) = listener.accept().await.expect("accept");
        read_one_frame(&mut stream).await
    });

    // Point the client at our tmpdir socket. `unix_socket_path()` reads
    // `XDG_RUNTIME_DIR`, so we override it for this process — under `ENV_LOCK`,
    // and for as long as `connect` needs to resolve the path.
    let _env_guard = ENV_LOCK.lock().await;
    let _runtime_dir = RuntimeDirVar::set(dir.path());

    let client_kind = ClientKind::Tui;
    let _conn = Connection::connect(client_kind, "0.0.0-test".to_string())
        .await
        .expect("connect");

    let first = tokio::time::timeout(std::time::Duration::from_secs(5), server_task)
        .await
        .expect("server task did not complete in time")
        .expect("server task panicked");

    match first {
        ClientToServer::Hello {
            proto_version,
            client_kind: kind,
            client_version,
        } => {
            assert_eq!(
                proto_version, PROTOCOL_VERSION,
                "the very first message must declare the current PROTOCOL_VERSION"
            );
            assert_eq!(kind, client_kind, "client_kind must round-trip");
            assert_eq!(client_version, "0.0.0-test");
        }
        other => panic!("expected Hello as the first frame, got {other:?}"),
    }
}

#[tokio::test]
async fn unix_socket_path_uses_xdg_runtime_dir_when_set() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let _env_guard = ENV_LOCK.lock().await;
    let _runtime_dir = RuntimeDirVar::set(dir.path());
    let path = nexterm_client_core::unix_socket_path();
    assert!(
        path.starts_with(dir.path().to_str().unwrap()),
        "unix_socket_path() must honour XDG_RUNTIME_DIR; got {path}"
    );
    assert!(path.ends_with("nexterm.sock"));
}
