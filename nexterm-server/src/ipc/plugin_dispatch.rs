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
        let lock = crate::lock_recover(&manager.plugin_manager, "plugin_manager");
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
        let mut lock = crate::lock_recover(&manager.plugin_manager, "plugin_manager");
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
        let mut lock = crate::lock_recover(&manager.plugin_manager, "plugin_manager");
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
        let mut lock = crate::lock_recover(&manager.plugin_manager, "plugin_manager");
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

// ── Integration tests (roadmap F2, Phase C) ──────────────────────────────────
//
// These exercise the plugin IPC handlers end-to-end against a real
// `SessionManager` with a real `PluginManager` and a real WASM fixture,
// asserting on the `ServerToClient` responses that flow back over the channel.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionManager;
    use nexterm_plugin::PluginManager;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    /// Minimal valid v2 plugin (declares its API version; no hooks needed for
    /// the dispatch-layer tests).
    const PLUGIN_WAT: &str = r#"
    (module
      (memory (export "memory") 2)
      (func (export "nexterm_api_version") (result i32) (i32.const 2)))
    "#;

    /// Build a `SessionManager` with an initialized (no-op) plugin manager.
    fn manager_with_plugins() -> SessionManager {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let pm = PluginManager::new(Arc::new(|_pane_id: u32, _data: &[u8]| {}));
        manager.set_plugin_manager(pm);
        manager
    }

    /// Compile the WAT fixture to a real `.wasm` file under `dir`.
    fn write_fixture(dir: &Path, name: &str) -> PathBuf {
        let bytes = wat::parse_str(PLUGIN_WAT).expect("fixture WAT must compile");
        let path = dir.join(name);
        std::fs::write(&path, &bytes).expect("write fixture");
        path
    }

    #[tokio::test]
    async fn load_list_unload_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_fixture(dir.path(), "p.wasm");
        let path_str = path.to_str().unwrap();
        let manager = manager_with_plugins();
        let (tx, mut rx) = mpsc::channel(8);

        // Load → PluginOk { action: "loaded" }.
        handle_load_plugin(&manager, &tx, path_str).await;
        match rx.recv().await.expect("load response") {
            ServerToClient::PluginOk { action, .. } => assert_eq!(action, "loaded"),
            _ => panic!("expected PluginOk(loaded)"),
        }

        // List → one path.
        handle_list_plugins(&manager, &tx).await;
        match rx.recv().await.expect("list response") {
            ServerToClient::PluginList { paths } => {
                assert_eq!(paths.len(), 1);
                assert!(paths[0].contains("p.wasm"));
            }
            _ => panic!("expected PluginList"),
        }

        // Unload → PluginOk { action: "unloaded" }.
        handle_unload_plugin(&manager, &tx, path_str).await;
        match rx.recv().await.expect("unload response") {
            ServerToClient::PluginOk { action, .. } => assert_eq!(action, "unloaded"),
            _ => panic!("expected PluginOk(unloaded)"),
        }

        // List is now empty.
        handle_list_plugins(&manager, &tx).await;
        match rx.recv().await.expect("list response") {
            ServerToClient::PluginList { paths } => assert!(paths.is_empty()),
            _ => panic!("expected empty PluginList"),
        }
    }

    #[tokio::test]
    async fn reload_after_load_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_fixture(dir.path(), "p.wasm");
        let path_str = path.to_str().unwrap();
        let manager = manager_with_plugins();
        let (tx, mut rx) = mpsc::channel(8);

        handle_load_plugin(&manager, &tx, path_str).await;
        let _ = rx.recv().await;

        handle_reload_plugin(&manager, &tx, path_str).await;
        match rx.recv().await.expect("reload response") {
            ServerToClient::PluginOk { action, .. } => assert_eq!(action, "reloaded"),
            _ => panic!("expected PluginOk(reloaded)"),
        }
    }

    #[tokio::test]
    async fn load_invalid_wasm_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.wasm");
        std::fs::write(&path, b"this is not wasm").unwrap();
        let manager = manager_with_plugins();
        let (tx, mut rx) = mpsc::channel(8);

        handle_load_plugin(&manager, &tx, path.to_str().unwrap()).await;
        match rx.recv().await.expect("error response") {
            ServerToClient::Error { message } => assert!(!message.is_empty()),
            _ => panic!("expected Error for invalid WASM"),
        }
    }

    #[tokio::test]
    async fn unload_nonexistent_returns_error() {
        let manager = manager_with_plugins();
        let (tx, mut rx) = mpsc::channel(8);

        handle_unload_plugin(&manager, &tx, "/no/such/plugin.wasm").await;
        match rx.recv().await.expect("error response") {
            ServerToClient::Error { message } => assert!(message.contains("not found")),
            _ => panic!("expected Error(not found)"),
        }
    }

    #[tokio::test]
    async fn load_without_initialized_manager_errors() {
        // A SessionManager whose plugin manager was never set must reject loads.
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let (tx, mut rx) = mpsc::channel(8);

        handle_load_plugin(&manager, &tx, "/whatever.wasm").await;
        match rx.recv().await.expect("error response") {
            ServerToClient::Error { message } => assert!(message.contains("not initialized")),
            _ => panic!("expected Error(not initialized)"),
        }
    }
}
