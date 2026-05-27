//! IPC command handlers for plugins.
//!
//! Centralizes processing for `ListPlugins` / `LoadPlugin` / `UnloadPlugin` / `ReloadPlugin`.

use nexterm_proto::ServerToClient;
use tokio::sync::mpsc;

use crate::session::SessionManager;

/// `ListPlugins` — return the list of currently loaded plugin paths.
pub(super) async fn handle_list_plugins(
    manager: &SessionManager,
    tx: &mpsc::Sender<ServerToClient>,
) {
    // Drop the lock before any await (MutexGuard is not Send).
    let paths = {
        let lock = manager
            .plugin_manager
            .lock()
            .expect("plugin_manager poisoned");
        lock.as_ref()
            .map(|m| {
                m.plugin_paths()
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let _ = tx.send(ServerToClient::PluginList { paths }).await;
}

/// `LoadPlugin` — load the plugin at the given path.
pub(super) async fn handle_load_plugin(
    manager: &SessionManager,
    tx: &mpsc::Sender<ServerToClient>,
    path: &str,
) {
    let result = {
        let mut lock = manager
            .plugin_manager
            .lock()
            .expect("plugin_manager poisoned");
        match lock.as_mut() {
            Some(m) => m.load(std::path::Path::new(path)),
            None => Err(anyhow::anyhow!("plugin manager is not initialized")),
        }
    };
    match result {
        Ok(()) => {
            let _ = tx
                .send(ServerToClient::PluginOk {
                    path: path.to_string(),
                    action: "loaded".to_string(),
                })
                .await;
        }
        Err(e) => {
            let _ = tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

/// `UnloadPlugin` — unload the plugin at the given path.
pub(super) async fn handle_unload_plugin(
    manager: &SessionManager,
    tx: &mpsc::Sender<ServerToClient>,
    path: &str,
) {
    let result = {
        let mut lock = manager
            .plugin_manager
            .lock()
            .expect("plugin_manager poisoned");
        match lock.as_mut() {
            Some(m) => m.unload(std::path::Path::new(path)),
            None => Err(anyhow::anyhow!("plugin manager is not initialized")),
        }
    };
    match result {
        Ok(removed) if removed => {
            let _ = tx
                .send(ServerToClient::PluginOk {
                    path: path.to_string(),
                    action: "unloaded".to_string(),
                })
                .await;
        }
        Ok(_) => {
            let _ = tx
                .send(ServerToClient::Error {
                    message: format!("plugin not found: {}", path),
                })
                .await;
        }
        Err(e) => {
            let _ = tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

/// `ReloadPlugin` — reload the plugin at the given path (unload then load).
pub(super) async fn handle_reload_plugin(
    manager: &SessionManager,
    tx: &mpsc::Sender<ServerToClient>,
    path: &str,
) {
    let result = {
        let mut lock = manager
            .plugin_manager
            .lock()
            .expect("plugin_manager poisoned");
        match lock.as_mut() {
            Some(m) => m.reload(std::path::Path::new(path)),
            None => Err(anyhow::anyhow!("plugin manager is not initialized")),
        }
    };
    match result {
        Ok(()) => {
            let _ = tx
                .send(ServerToClient::PluginOk {
                    path: path.to_string(),
                    action: "reloaded".to_string(),
                })
                .await;
        }
        Err(e) => {
            let _ = tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}
