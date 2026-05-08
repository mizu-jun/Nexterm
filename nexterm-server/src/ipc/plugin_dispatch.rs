//! プラグイン関連 IPC コマンドのハンドラ
//!
//! `ListPlugins` / `LoadPlugin` / `UnloadPlugin` / `ReloadPlugin` の処理を集約する。

use nexterm_proto::ServerToClient;
use tokio::sync::mpsc;

use crate::session::SessionManager;

/// `ListPlugins` — 現在ロード済みプラグインのパス一覧を返す
pub(super) async fn handle_list_plugins(
    manager: &SessionManager,
    tx: &mpsc::Sender<ServerToClient>,
) {
    // ロックスコープを await 前に終わらせる（MutexGuard は Send でないため）
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

/// `LoadPlugin` — 指定パスのプラグインをロードする
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
            None => Err(anyhow::anyhow!(
                "プラグインマネージャーが初期化されていません"
            )),
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

/// `UnloadPlugin` — 指定パスのプラグインをアンロードする
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
            None => Err(anyhow::anyhow!(
                "プラグインマネージャーが初期化されていません"
            )),
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
                    message: format!("プラグインが見つかりません: {}", path),
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

/// `ReloadPlugin` — 指定パスのプラグインをリロード（unload → load）する
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
            None => Err(anyhow::anyhow!(
                "プラグインマネージャーが初期化されていません"
            )),
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
