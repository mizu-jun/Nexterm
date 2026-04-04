#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! nexterm-client-gpu エントリーポイント — wgpu + winit デスクトップクライアント

mod connection;
mod font;
mod host_manager;
mod macro_picker;
mod palette;
mod renderer;
mod scrollback;
mod settings_panel;
mod state;

use anyhow::Result;
use nexterm_config::{watch_config, ConfigLoader, StatusBarEvaluator};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use winit::event_loop::EventLoop;

#[tokio::main]
async fn main() -> Result<()> {
    let _log_guard = init_tracing();

    // 設定ロード（TOML → Lua）
    let config = ConfigLoader::load()?;
    nexterm_i18n::init();

    info!(
        "Config loaded: font={} {}pt",
        config.font.family, config.font.size
    );

    // 設定ホットリロードウォッチャーを起動する
    let (config_tx, config_rx) = mpsc::channel(8);
    let config_watcher = watch_config(config_tx).ok();

    // Lua ステータスバー評価器（status_bar.enabled のときだけ生成する）
    let status_eval = if config.status_bar.enabled {
        Some(StatusBarEvaluator::new())
    } else {
        None
    };

    // winit イベントループを作成する
    let event_loop = EventLoop::new()?;

    // GPU アプリケーションを起動する
    let app = renderer::NextermApp::new(config).await?;
    event_loop.run_app(
        &mut app.into_event_handler(Some(config_rx), config_watcher, status_eval),
    )?;

    Ok(())
}

/// ログ初期化。Windows リリースビルドではファイル出力（%LOCALAPPDATA%\nexterm\nexterm-client.log）。
/// 他の環境では標準出力に出力する。
#[cfg(all(windows, not(debug_assertions)))]
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("nexterm");
    std::fs::create_dir_all(&log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "nexterm-client.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("NEXTERM_LOG")
                .unwrap_or_else(|_| EnvFilter::new("info")),
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
