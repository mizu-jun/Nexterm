#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! nexterm-server entry point

mod hooks;
mod ipc;
mod pane;
mod persist;
mod serial;
mod session;
mod snapshot;
mod template;
mod web;
mod window;

use std::sync::Arc;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use session::SessionManager;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging (controlled by NEXTERM_LOG environment variable)
    let _log_guard = init_tracing();

    info!("nexterm-server starting...");

    let manager = Arc::new(SessionManager::new());

    // スナップショットが存在すれば前回のセッションを復元する
    if let Some(snap) = persist::load_snapshot() {
        let restored = manager.restore_from_snapshot(&snap).await;
        if !restored.is_empty() {
            info!("復元したセッション: {:?}", restored);
            // 復元したペイン/ウィンドウの最大 ID より大きい値をカウンターに設定して衝突を防ぐ
            let max_pane_id = max_pane_id_in_snapshot(&snap);
            pane::set_min_pane_id(max_pane_id + 1);
            let max_window_id = max_window_id_in_snapshot(&snap);
            session::set_min_window_id(max_window_id + 1);
        }
    }

    let manager_for_ipc = Arc::clone(&manager);

    // 設定を読み込んでフック設定・ログ設定・ホスト設定を抽出する
    let (hooks, lua_runner, log_config, hosts, web_config) = {
        let cfg = nexterm_config::ConfigLoader::load().unwrap_or_default();
        let lua_script = nexterm_config::lua_path();
        let runner = nexterm_config::LuaHookRunner::new(Some(lua_script));

        // WASM プラグインをロードする
        if !cfg.plugins_disabled {
            let plugin_dir = cfg.plugin_dir.as_deref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(nexterm_plugin::default_plugin_dir);
            let mgr = nexterm_plugin::PluginManager::new(
                std::sync::Arc::new(|_pane_id, _data| {
                    // TODO: セッションマネージャーへの書き込みはフェーズ 2 で実装
                }),
            );
            match mgr.load_dir(&plugin_dir) {
                Ok(n) if n > 0 => info!("{} 個の WASM プラグインをロードしました", n),
                Ok(_) => {}
                Err(e) => tracing::warn!("プラグインのロードに失敗しました: {}", e),
            }
        }

        (Arc::new(cfg.hooks), Arc::new(runner), Arc::new(cfg.log), Arc::new(cfg.hosts), cfg.web)
    };

    // Web ターミナルが有効な場合はバックグラウンドで起動する
    if web_config.enabled {
        let web_manager = Arc::clone(&manager);
        tokio::spawn(web::start_web_server(web_config, web_manager));
    }

    // Run IPC server and wait for shutdown signal
    tokio::select! {
        result = ipc::serve(manager_for_ipc, hooks, lua_runner, log_config, hosts) => {
            result?;
        }
        _ = shutdown_signal() => {
            info!("Shutdown signal received. Exiting...");
        }
    }

    // シャットダウン時にスナップショットを保存する
    let snap = manager.to_snapshot().await;
    if !snap.sessions.is_empty()
        && let Err(e) = persist::save_snapshot(&snap) {
            tracing::warn!("スナップショットの保存に失敗しました: {}", e);
        }

    info!("nexterm-server stopped");
    Ok(())
}

/// スナップショット内の最大ペイン ID を返す（カウンター更新に使用）
fn max_pane_id_in_snapshot(snap: &snapshot::ServerSnapshot) -> u32 {
    snap.sessions
        .iter()
        .flat_map(|s| s.windows.iter())
        .map(|w| max_pane_id_in_node(&w.layout))
        .max()
        .unwrap_or(0)
}

fn max_pane_id_in_node(node: &snapshot::SplitNodeSnapshot) -> u32 {
    match node {
        snapshot::SplitNodeSnapshot::Pane { pane_id, .. } => *pane_id,
        snapshot::SplitNodeSnapshot::Split { left, right, .. } => {
            max_pane_id_in_node(left).max(max_pane_id_in_node(right))
        }
    }
}

/// スナップショット内の最大ウィンドウ ID を返す（カウンター更新に使用）
fn max_window_id_in_snapshot(snap: &snapshot::ServerSnapshot) -> u32 {
    snap.sessions
        .iter()
        .flat_map(|s| s.windows.iter())
        .map(|w| w.id)
        .max()
        .unwrap_or(0)
}

/// Shutdown signal handler (Unix/Windows)
#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("Failed to set up SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("Failed to set up SIGINT handler");
    tokio::select! {
        _ = term.recv() => { info!("Received SIGTERM"); }
        _ = int.recv()  => { info!("Received SIGINT"); }
    }
}

#[cfg(windows)]
async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.expect("Failed to set up Ctrl+C handler");
    info!("Received Ctrl+C");
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
