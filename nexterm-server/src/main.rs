#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! nexterm-server スタンドアロンバイナリのエントリーポイント。
//! サーバーロジックは lib.rs に集約されている。

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // スタンドアロン起動時はここでログを初期化する
    // GPU クライアントに組み込まれた場合はクライアント側で初期化済み
    let _log_guard = init_tracing();
    nexterm_server::run_server().await
}

/// ログ初期化。Windows リリースビルドではファイル出力（%LOCALAPPDATA%\nexterm\nexterm-server.log）。
/// 他の環境では標準出力に出力する。
/// 戻り値の guard はドロップするとログ書き込みが停止するため、main() の lifetime まで保持する。
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
