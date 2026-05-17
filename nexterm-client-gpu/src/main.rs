#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! nexterm エントリーポイント — wgpu + winit デスクトップクライアント（シングルバイナリ）
//!
//! nexterm-server のロジックを Tokio タスクとして内部起動し、
//! 単一プロセスで全機能を提供する。

mod animations;
mod color_util;
mod connection;
mod font;
mod glyph_atlas;
mod host_manager;
mod key_map;
mod macro_picker;
mod notification;
mod palette;
mod platform;
mod quake;
mod renderer;
mod scrollback;
mod settings_panel;
mod shaders;
mod signature_verify;
mod state;
mod update_checker;
mod vertex_util;

use anyhow::Result;
use nexterm_config::{ConfigLoader, StatusBarEvaluator, watch_config};
use tokio::sync::mpsc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use winit::event_loop::EventLoop;

#[tokio::main]
async fn main() -> Result<()> {
    let _log_guard = init_tracing();

    // サーバーを Tokio タスクとして内部起動する（別プロセス不要）
    // IPC ソケットは同じプロトコルをそのまま使用する
    let server_handle = tokio::spawn(async {
        if let Err(e) = nexterm_server::run_server().await {
            tracing::error!("nexterm-server エラー: {}", e);
        }
    });

    // 設定ロード（TOML → Lua）
    let config = ConfigLoader::load()?;
    // config の language 設定を優先し、"auto" の場合は OS ロケールを検出する
    if config.language == "auto" {
        nexterm_i18n::init();
    } else {
        nexterm_i18n::set_locale(&config.language);
    }

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

    // バックグラウンド更新チェッカーを起動する（5 秒後に GitHub Releases API を確認）
    let update_rx = update_checker::start(env!("CARGO_PKG_VERSION"), config.auto_check_update);

    // winit イベントループを作成する
    let event_loop = EventLoop::new()?;

    // GPU アプリケーションを起動する
    let app = renderer::NextermApp::new(config).await?;
    event_loop.run_app(&mut app.into_event_handler(
        Some(config_rx),
        config_watcher,
        status_eval,
        server_handle,
        update_rx,
    ))?;

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
