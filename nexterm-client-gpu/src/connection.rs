//! IPC 接続 — `nexterm-client-core` の `Connection` を GPU 用にラップする
//!
//! Sprint 3-6: 共通実装は [`nexterm_client_core`] に集約された。
//! ここでは GPU 固有のハンドシェイク（`ClientKind::Gpu` + 自分のバージョン）を
//! 渡しながら接続するエントリポイントを提供する。

use anyhow::Result;
use nexterm_proto::ClientKind;

pub use nexterm_client_core::Connection;

impl ConnectionExt for Connection {}

/// `Connection::connect()` を `ClientKind::Gpu` 固定で呼ぶ拡張トレイト
pub trait ConnectionExt {
    fn connect_gpu() -> impl std::future::Future<Output = Result<Connection>> + Send {
        Connection::connect(ClientKind::Gpu, env!("CARGO_PKG_VERSION").to_string())
    }
}
