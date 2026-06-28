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
    // `XDG_RUNTIME_DIR`, so we override it for this process.
    // SAFETY: this test does not run in parallel with anything that reads
    // XDG_RUNTIME_DIR concurrently — cargo runs integration test files in
    // separate processes by default.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    }

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
    // SAFETY: see note above; integration test files run in their own process.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    }
    let path = nexterm_client_core::unix_socket_path();
    assert!(
        path.starts_with(dir.path().to_str().unwrap()),
        "unix_socket_path() must honour XDG_RUNTIME_DIR; got {path}"
    );
    assert!(path.ends_with("nexterm.sock"));
}
