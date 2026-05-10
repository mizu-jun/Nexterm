//! IPC 接続 — `nexterm-client-core` の `Connection` を TUI 用にラップする
//!
//! Sprint 3-6: 共通実装は [`nexterm_client_core`] に集約された。
//! ここでは TUI 固有のハンドシェイク（`ClientKind::Tui` + 自分のバージョン）を
//! 渡しながら接続するエントリポイントを提供する。

use anyhow::Result;
use nexterm_proto::ClientKind;

pub use nexterm_client_core::Connection;

impl ConnectionExt for Connection {}

/// `Connection::connect()` を `ClientKind::Tui` 固定で呼ぶ拡張トレイト
pub trait ConnectionExt {
    fn connect_tui() -> impl std::future::Future<Output = Result<Connection>> + Send {
        Connection::connect(ClientKind::Tui, env!("CARGO_PKG_VERSION").to_string())
    }
}
