#![warn(missing_docs)]
//! nexterm-server library crate.
//! Exposed for embedding into the GPU client as a single binary.

mod hooks;
mod ipc;
mod pane;
pub mod persist;
pub mod runtime_config;
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
use tracing::{info, warn};

pub use runtime_config::{
    RuntimeConfig, SharedRuntimeConfig, build_shared as build_shared_runtime_config,
};

use session::SessionManager;
use snapshot::ServerSnapshot;

/// Run the main logic of `nexterm-server`, loading the config file from disk.
///
/// Use this entry point when running as a standalone `nexterm-server` binary.
/// The single-binary GPU client should call [`run_server_with_config`] instead
/// to avoid reading and parsing the same TOML twice (once on the client, once
/// here) — see `nexterm-client.log.2026-06-05` where the duplicate
/// `Loaded the TOML configuration` log lines from the client and the embedded
/// server task fire within microseconds of each other.
///
/// The caller is responsible for log initialization (no need when embedded in
/// the GPU client). Waits until shutdown signal or until the IPC server exits.
pub async fn run_server() -> Result<()> {
    info!("starting nexterm-server...");

    // Sprint 5-12 Phase 4: when config loading fails, instead of silently falling back to
    // defaults we queue warning messages on the SessionManager. They are delivered to the client
    // as `ServerToClient::Error` on the first attach and shown as an error banner.
    let mut startup_warnings: Vec<String> = Vec::new();
    let cfg = match nexterm_config::ConfigLoader::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!("failed to load config file: {e}");
            startup_warnings.push(format!(
                "failed to load config file (starting with defaults): {e}"
            ));
            nexterm_config::Config::default()
        }
    };
    run_server_inner(cfg, startup_warnings, None, None).await
}

/// Run the main logic of `nexterm-server` using a pre-loaded config.
///
/// Intended for the single-binary GPU client, which has already parsed the
/// TOML for its own use (font, language, status bar, etc.). Reusing that
/// `Config` here eliminates the duplicate TOML read previously visible as
/// twin `nexterm_config::loader: Loaded the TOML configuration` log lines on
/// startup.
pub async fn run_server_with_config(cfg: nexterm_config::Config) -> Result<()> {
    info!("starting nexterm-server...");
    run_server_inner(cfg, Vec::new(), None, None).await
}

/// Run the main logic of `nexterm-server` with externally owned hot-reload state.
///
/// Intended for the single-binary GPU client, which already owns the
/// [`SharedRuntimeConfig`] and a `notify` watcher (Sprint 5-13 / v1.7.7). With
/// the shared handle in hand, the embedded server skips `spawn_watcher` and
/// reuses the client's existing watcher — `nexterm-client.log.2026-06-05`
/// showed `nexterm_config::watcher: Watching the configuration directory`
/// firing twice on every startup because both the client and the embedded
/// server installed their own watcher on the same directory.
///
/// `shutdown_rx` lets the caller stop the server cleanly without the
/// `tokio::task::JoinHandle::abort()` hack. v1.7.7 moves the embedded server
/// onto its own OS thread / Tokio runtime so winit's main-thread occupation
/// cannot starve the server task (see
/// `memory/project_windows_powershell_startup_investigation.md` problem 3);
/// in that layout `abort()` is no longer available because the server is no
/// longer a Tokio task.
pub async fn run_server_with_config_and_runtime(
    cfg: nexterm_config::Config,
    runtime_cfg: SharedRuntimeConfig,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    info!("starting nexterm-server...");
    run_server_inner(cfg, Vec::new(), Some(runtime_cfg), Some(shutdown_rx)).await
}

async fn run_server_inner(
    cfg: nexterm_config::Config,
    startup_warnings: Vec<String>,
    runtime_cfg_ext: Option<SharedRuntimeConfig>,
    shutdown_rx_ext: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<()> {
    let manager = Arc::new(SessionManager::new(cfg.shell.clone()));

    // Restore the previous session(s) when a snapshot exists.
    // P1-A diagnostic: log each major startup step so we can locate where the
    // server stalls between `restored sessions` and `ipc::serve`. Previously the
    // 38 s gap between Session2 and Session3 (2026-06-03 log) left no breadcrumbs
    // in this region — see `memory/project_windows_powershell_startup_investigation.md`.
    info!("startup: loading snapshot...");
    if let Some(snap) = persist::load_snapshot() {
        let original_window_count = total_window_count(&snap);
        let original_session_count = snap.sessions.len();

        let restored = manager.restore_from_snapshot(&snap).await;
        if !restored.is_empty() {
            info!("restored sessions: {:?}", restored);
            // Bump the counters above the largest restored pane/window ID to avoid collisions.
            let max_pane_id = max_pane_id_in_snapshot(&snap);
            pane::set_min_pane_id(max_pane_id + 1);
            let max_window_id = max_window_id_in_snapshot(&snap);
            session::set_min_window_id(max_window_id + 1);
        }

        // Self-heal: if any window or session failed to restore (e.g. ConPTY
        // returned E_INVALIDARG because the saved cwd no longer exists), rewrite
        // the snapshot immediately so the broken entries do not keep failing on
        // every subsequent launch. Without this, short-lived sessions can leave
        // bad entries in the snapshot file forever because the 30 s auto-save
        // never fires.
        info!("startup: running snapshot self-heal check...");
        let current_snap = manager.to_snapshot().await;
        let current_window_count = total_window_count(&current_snap);
        let current_session_count = current_snap.sessions.len();
        if current_window_count < original_window_count
            || current_session_count < original_session_count
        {
            let dropped_windows = original_window_count.saturating_sub(current_window_count);
            let dropped_sessions = original_session_count.saturating_sub(current_session_count);
            warn!(
                "snapshot self-heal: {} window(s) / {} session(s) failed to restore; rewriting snapshot",
                dropped_windows, dropped_sessions
            );
            if let Err(e) = persist::save_snapshot(&current_snap) {
                warn!("snapshot self-heal save failed: {}", e);
            }
        }
    }

    let manager_for_ipc = Arc::clone(&manager);

    info!("startup: building runtime config and Lua hook runner...");
    // Extract hook / log / hosts configuration from the already-loaded config.
    let (runtime_cfg, lua_runner, web_config) = {
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
                info!(
                    "startup: loading WASM plugins from {}...",
                    plugin_dir.display()
                );
                match mgr.load_dir(&plugin_dir) {
                    Ok(n) if n > 0 => info!("loaded {} WASM plugin(s)", n),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("failed to load plugins: {}", e),
                }
            }
            manager.set_plugin_manager(mgr);
        }

        // Use the externally provided SharedRuntimeConfig if available
        // (single-binary client owns one already + drives its watcher), else
        // build one and spawn our own watcher below.
        let runtime_cfg = runtime_cfg_ext.unwrap_or_else(|| runtime_config::build_shared(&cfg));
        (runtime_cfg, Arc::new(runner), cfg.web)
    };

    // Sprint 5-12 Phase 4: register startup warnings (e.g. config load failures) on the
    // SessionManager so they can be forwarded to the client; drained on first attach.
    if !startup_warnings.is_empty() {
        manager.set_startup_warnings(startup_warnings);
    }

    // Spawn the config watcher only when no external SharedRuntimeConfig was
    // supplied. When the single-binary GPU client (v1.7.7+) hands us a runtime
    // config, it also owns the `notify::Watcher` and pushes updates into the
    // same `SharedRuntimeConfig` we just received, so a second watcher here
    // would only duplicate file-system events.
    let _watcher = if shutdown_rx_ext.is_none() {
        info!("startup: spawning config watcher...");
        // Watch config.toml and hot-reload runtime config on changes.
        // `_watcher` stops watching when dropped, so retain it within run_server's scope.
        match runtime_config::spawn_watcher(Arc::clone(&runtime_cfg)) {
            Ok(w) => Some(w),
            Err(e) => {
                tracing::warn!(
                    "failed to start config watcher (hot-reload disabled): {}",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    // Launch the web terminal in the background if enabled.
    if web_config.enabled {
        info!("startup: launching web terminal server...");
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

    // Phase 2c (UI/UX v2): foreground-process polling ticker (1 Hz).
    //
    // Inspects every pane's foreground process once per second, broadcasts
    // `ServerToClient::ProcessChanged` only when the name changes. Gated on
    // `runtime_cfg.tab_bar.show_process_icon` so users who keep the
    // default off pay zero OS-inspection cost. The `last_seen` map keeps
    // the diff state across ticks; `SessionManager::poll_foreground_processes`
    // prunes vanished panes each tick so memory stays bounded.
    let process_poll_manager = Arc::clone(&manager);
    let process_poll_runtime = Arc::clone(&runtime_cfg);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.tick().await; // First tick is immediate; skip it so we
        // don't broadcast `None` for every fresh shell before it has had
        // a chance to spawn anything.
        let mut last_seen: std::collections::HashMap<u32, Option<String>> =
            std::collections::HashMap::new();
        loop {
            interval.tick().await;
            let show_icons = process_poll_runtime.load().tab_bar.show_process_icon;
            if !show_icons {
                // Clear the cache so a re-enable starts fresh and the
                // client sees a fresh `ProcessChanged` even when the
                // last-seen name happens to match the current one.
                last_seen.clear();
                continue;
            }
            process_poll_manager
                .poll_foreground_processes(&mut last_seen)
                .await;
        }
    });

    info!("startup: entering ipc::serve (this is the last step before accept loop)");
    // Run the IPC server and wait for a shutdown signal.
    //
    // When the caller (single-binary client) supplies an explicit `shutdown_rx`,
    // wait on it. Otherwise (standalone `nexterm-server` binary) fall back to
    // the OS-level Ctrl+C / SIGTERM handler.
    tokio::select! {
        result = ipc::serve(manager_for_ipc, runtime_cfg, lua_runner) => {
            result?;
        }
        _ = async {
            if let Some(rx) = shutdown_rx_ext {
                let _ = rx.await;
                info!("received explicit shutdown request; exiting...");
            } else {
                shutdown_signal().await;
                info!("received shutdown signal; exiting...");
            }
        } => {}
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

/// Total number of windows across every session in the snapshot.
///
/// Used by the snapshot self-heal path to detect when entries failed to restore.
fn total_window_count(snap: &ServerSnapshot) -> usize {
    snap.sessions.iter().map(|s| s.windows.len()).sum()
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
