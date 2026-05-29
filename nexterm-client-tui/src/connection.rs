//! IPC connection — wraps [`nexterm_client_core::Connection`] for the TUI.
//!
//! Sprint 3-6: the shared implementation lives in [`nexterm_client_core`].
//! This module exposes a TUI-specific entry point that performs the handshake
//! with `ClientKind::Tui` and the crate's own version string.

use anyhow::Result;
use nexterm_proto::ClientKind;

pub use nexterm_client_core::Connection;

impl ConnectionExt for Connection {}

/// Extension trait that calls `Connection::connect()` with `ClientKind::Tui`.
pub trait ConnectionExt {
    fn connect_tui() -> impl std::future::Future<Output = Result<Connection>> + Send {
        Connection::connect(ClientKind::Tui, env!("CARGO_PKG_VERSION").to_string())
    }
}
