//! IPC 層 — Unix Domain Socket (Linux/macOS) / Named Pipe (Windows) の切り替え

mod dispatch;
mod handler;
mod key;
mod platform;
mod plugin_dispatch;
mod sftp;

use crate::session::SessionManager;
use anyhow::Result;

/// IPC サーバーを起動してクライアント接続を受け付ける
pub async fn serve(
    manager: std::sync::Arc<SessionManager>,
    hooks: std::sync::Arc<nexterm_config::HooksConfig>,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    log_config: std::sync::Arc<nexterm_config::LogConfig>,
    hosts: std::sync::Arc<Vec<nexterm_config::HostConfig>>,
) -> Result<()> {
    #[cfg(unix)]
    return platform::serve_unix(manager, hooks, lua, log_config, hosts).await;

    #[cfg(windows)]
    return platform::serve_named_pipe(manager, hooks, lua, log_config, hosts).await;
}
