#![warn(missing_docs)]
//! nexterm-server ライブラリクレート
//! GPU クライアントに組み込んでシングルバイナリとして実行するために公開する。

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

/// nexterm-server のメインロジックを実行する。
/// ログ初期化は呼び出し元で行うこと（GPU クライアントに組み込む場合は不要）。
/// シャットダウンシグナルまたは IPC サーバー終了まで待機する。
pub async fn run_server() -> Result<()> {
    info!("nexterm-server 起動中...");

    let shell_config = nexterm_config::ConfigLoader::load()
        .unwrap_or_default()
        .shell;
    let manager = Arc::new(SessionManager::new(shell_config));

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
    let (runtime_cfg, lua_runner, web_config) = {
        let cfg = nexterm_config::ConfigLoader::load().unwrap_or_default();
        let lua_script = nexterm_config::lua_path();
        let runner = nexterm_config::LuaHookRunner::new(Some(lua_script));

        // WASM プラグインマネージャーを初期化して SessionManager に登録する
        if !cfg.plugins_disabled {
            let plugin_dir = cfg
                .plugin_dir
                .as_deref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(nexterm_plugin::default_plugin_dir);
            let mgr = nexterm_plugin::PluginManager::new(std::sync::Arc::new(|_pane_id, _data| {}));
            if plugin_dir.exists() {
                match mgr.load_dir(&plugin_dir) {
                    Ok(n) if n > 0 => info!("{} 個の WASM プラグインをロードしました", n),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("プラグインのロードに失敗しました: {}", e),
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

    // config.toml の変更を監視してランタイム設定をホットリロードする
    // _watcher は drop されると監視を停止するため run_server スコープで保持する
    let _watcher = match runtime_config::spawn_watcher(Arc::clone(&runtime_cfg)) {
        Ok(w) => Some(w),
        Err(e) => {
            tracing::warn!(
                "config watcher の起動に失敗しました（ホットリロードは無効）: {}",
                e
            );
            None
        }
    };

    // Web ターミナルが有効な場合はバックグラウンドで起動する
    if web_config.enabled {
        let web_manager = Arc::clone(&manager);
        tokio::spawn(web::start_web_server(web_config, web_manager));
    }

    // 30秒ごとにスナップショットを自動保存するバックグラウンドタスク
    let auto_save_manager = Arc::clone(&manager);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await; // 最初の tick は即時なのでスキップ
        loop {
            interval.tick().await;
            let snap = auto_save_manager.to_snapshot().await;
            if !snap.sessions.is_empty()
                && let Err(e) = persist::save_snapshot(&snap)
            {
                tracing::warn!("自動保存に失敗しました: {}", e);
            }
        }
    });

    // IPC サーバーを実行してシャットダウンシグナルを待機する
    tokio::select! {
        result = ipc::serve(manager_for_ipc, runtime_cfg, lua_runner) => {
            result?;
        }
        _ = shutdown_signal() => {
            info!("シャットダウンシグナルを受信しました。終了します...");
        }
    }

    // シャットダウン時にスナップショットを保存する
    let snap = manager.to_snapshot().await;
    if !snap.sessions.is_empty()
        && let Err(e) = persist::save_snapshot(&snap)
    {
        tracing::warn!("スナップショットの保存に失敗しました: {}", e);
    }

    info!("nexterm-server 停止");
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

/// シャットダウンシグナルハンドラー（Unix/Windows）
#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("SIGTERM ハンドラーの設定に失敗");
    let mut int = signal(SignalKind::interrupt()).expect("SIGINT ハンドラーの設定に失敗");
    tokio::select! {
        _ = term.recv() => { info!("SIGTERM を受信しました"); }
        _ = int.recv()  => { info!("SIGINT を受信しました"); }
    }
}

#[cfg(windows)]
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Ctrl+C ハンドラーの設定に失敗");
    info!("Ctrl+C を受信しました");
}
