#![warn(missing_docs)]
//! nexterm-server library crate.
//! Exposed for embedding into the GPU client as a single binary.

mod hooks;
mod ipc;
mod pane;
pub mod persist;
mod runtime_config;
mod serial;
mod session;
pub mod snapshot;
mod template;
#[cfg(test)]
pub mod test_utils;
mod web;
mod window;

use std::sync::Arc;

use anyhow::Result;
use tracing::info;

use session::SessionManager;

/// Run the main logic of `nexterm-server`.
/// The caller is responsible for log initialization (no need when embedded in the GPU client).
/// Waits until shutdown signal or until the IPC server exits.
pub async fn run_server() -> Result<()> {
    info!("starting nexterm-server...");

    // Sprint 5-12 Phase 4: when config loading fails, instead of silently falling back to
    // defaults we queue warning messages on the SessionManager. They are delivered to the client
    // as `ServerToClient::Error` on the first attach and shown as an error banner.
    let mut startup_warnings: Vec<String> = Vec::new();
    let shell_config = match nexterm_config::ConfigLoader::load() {
        Ok(cfg) => cfg.shell,
        Err(e) => {
            tracing::error!("failed to load config file: {e}");
            startup_warnings.push(format!(
                "failed to load config file (starting with defaults): {e}"
            ));
            nexterm_config::Config::default().shell
        }
    };
    let manager = Arc::new(SessionManager::new(shell_config));

    // Restore the previous session(s) when a snapshot exists.
    if let Some(snap) = persist::load_snapshot() {
        let restored = manager.restore_from_snapshot(&snap).await;
        if !restored.is_empty() {
            info!("restored sessions: {:?}", restored);
            // Bump the counters above the largest restored pane/window ID to avoid collisions.
            let max_pane_id = max_pane_id_in_snapshot(&snap);
            pane::set_min_pane_id(max_pane_id + 1);
            let max_window_id = max_window_id_in_snapshot(&snap);
            session::set_min_window_id(max_window_id + 1);
        }
    }

    let manager_for_ipc = Arc::clone(&manager);

    // Load config and extract hook / log / hosts configuration.
    let (runtime_cfg, lua_runner, web_config) = {
        // Sprint 5-12 Phase 4: queue warnings on the second failed load as well.
        let cfg = match nexterm_config::ConfigLoader::load() {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::error!("failed to reload config file: {e}");
                startup_warnings.push(format!(
                    "failed to load hook/web config (starting with defaults): {e}"
                ));
                nexterm_config::Config::default()
            }
        };
        let lua_script = nexterm_config::lua_path();
        let runner = nexterm_config::LuaHookRunner::new(Some(lua_script));

        // Initialize the WASM plugin manager and register it on the SessionManager.
        if !cfg.plugins_disabled {
            let plugin_dir = cfg
                .plugin_dir
                .as_deref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(nexterm_plugin::default_plugin_dir);
            let mgr = nexterm_plugin::PluginManager::new(std::sync::Arc::new(|_pane_id, _data| {}));
            if plugin_dir.exists() {
                match mgr.load_dir(&plugin_dir) {
                    Ok(n) if n > 0 => info!("loaded {} WASM plugin(s)", n),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("failed to load plugins: {}", e),
                }
            }
            manager.set_plugin_manager(mgr);
        }

        (
            runtime_config::build_shared(&cfg),
            Arc::new(runner),
            cfg.web,
        )
    };

    // Sprint 5-12 Phase 4: register startup warnings (e.g. config load failures) on the
    // SessionManager so they can be forwarded to the client; drained on first attach.
    if !startup_warnings.is_empty() {
        manager.set_startup_warnings(startup_warnings);
    }

    // Watch config.toml and hot-reload runtime config on changes.
    // `_watcher` stops watching when dropped, so retain it within run_server's scope.
    let _watcher = match runtime_config::spawn_watcher(Arc::clone(&runtime_cfg)) {
        Ok(w) => Some(w),
        Err(e) => {
            tracing::warn!(
                "failed to start config watcher (hot-reload disabled): {}",
                e
            );
            None
        }
    };

    // Launch the web terminal in the background if enabled.
    if web_config.enabled {
        let web_manager = Arc::clone(&manager);
        tokio::spawn(web::start_web_server(web_config, web_manager));
    }

    // Background task: auto-save the snapshot every 30 seconds.
    let auto_save_manager = Arc::clone(&manager);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await; // The first tick fires immediately, so skip it.
        loop {
            interval.tick().await;
            let snap = auto_save_manager.to_snapshot().await;
            if !snap.sessions.is_empty()
                && let Err(e) = persist::save_snapshot(&snap)
            {
                tracing::warn!("auto-save failed: {}", e);
            }
        }
    });

    // Run the IPC server and wait for a shutdown signal.
    tokio::select! {
        result = ipc::serve(manager_for_ipc, runtime_cfg, lua_runner) => {
            result?;
        }
        _ = shutdown_signal() => {
            info!("received shutdown signal; exiting...");
        }
    }

    // Save a snapshot on shutdown.
    let snap = manager.to_snapshot().await;
    if !snap.sessions.is_empty()
        && let Err(e) = persist::save_snapshot(&snap)
    {
        tracing::warn!("failed to save snapshot: {}", e);
    }

    info!("nexterm-server stopped");
    Ok(())
}

/// Return the maximum pane ID inside a snapshot (used to bump the counter).
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

/// Return the maximum window ID inside a snapshot (used to bump the counter).
fn max_window_id_in_snapshot(snap: &snapshot::ServerSnapshot) -> u32 {
    snap.sessions
        .iter()
        .flat_map(|s| s.windows.iter())
        .map(|w| w.id)
        .max()
        .unwrap_or(0)
}

/// Shutdown signal handler (Unix/Windows).
#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");
    tokio::select! {
        _ = term.recv() => { info!("received SIGTERM"); }
        _ = int.recv()  => { info!("received SIGINT"); }
    }
}

#[cfg(windows)]
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    info!("received Ctrl+C");
}
