//! nexterm-server entry point

mod hooks;
mod ipc;
mod pane;
mod persist;
mod serial;
mod session;
mod snapshot;
mod template;
mod window;

use std::sync::Arc;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use session::SessionManager;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging (controlled by NEXTERM_LOG environment variable)
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("NEXTERM_LOG"))
        .init();

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
    let (hooks, lua_runner, log_config, hosts) = {
        let cfg = nexterm_config::ConfigLoader::load().unwrap_or_default();
        let lua_script = nexterm_config::lua_path();
        let runner = nexterm_config::LuaHookRunner::new(Some(lua_script));
        (Arc::new(cfg.hooks), Arc::new(runner), Arc::new(cfg.log), Arc::new(cfg.hosts))
    };

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
