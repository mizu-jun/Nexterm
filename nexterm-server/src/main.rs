#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Entry point for the `nexterm-server` standalone binary.
//! The server logic itself lives in `lib.rs`.

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging here when running standalone.
    // When embedded in the GPU client, the client already initialized logging.
    let _log_guard = init_tracing();
    nexterm_server::run_server().await
}

/// Initialize logging. Windows release builds write to a file
/// (`%LOCALAPPDATA%\nexterm\nexterm-server.log`); other environments write to stdout.
/// Dropping the returned guard stops log writes, so it must be kept alive for the lifetime of
/// `main()`.
#[cfg(all(windows, not(debug_assertions)))]
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("nexterm");
    std::fs::create_dir_all(&log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "nexterm-server.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("NEXTERM_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(non_blocking)
        .init();
    Some(guard)
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("NEXTERM_LOG"))
        .init();
    None
}
