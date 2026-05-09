//! IPC 層 — Unix Domain Socket (Linux/macOS) / Named Pipe (Windows) の切り替え

mod dispatch;
mod dispatch_util;
mod handler;
mod key;
mod platform;
mod plugin_dispatch;
mod sftp;

use crate::runtime_config::SharedRuntimeConfig;
use crate::session::SessionManager;
use anyhow::Result;

/// IPC サーバーを起動してクライアント接続を受け付ける
pub async fn serve(
    manager: std::sync::Arc<SessionManager>,
    runtime_cfg: SharedRuntimeConfig,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
) -> Result<()> {
    #[cfg(unix)]
    return platform::serve_unix(manager, runtime_cfg, lua).await;

    #[cfg(windows)]
    return platform::serve_named_pipe(manager, runtime_cfg, lua).await;
}
