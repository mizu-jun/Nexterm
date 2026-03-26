//! nexterm-server エントリーポイント

mod ipc;
mod pane;
mod persist;
mod session;
mod window;

use std::sync::Arc;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use session::SessionManager;

#[tokio::main]
async fn main() -> Result<()> {
    // ログ初期化（NEXTERM_LOG 環境変数で制御）
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("NEXTERM_LOG"))
        .init();

    info!("nexterm-server を起動中...");

    // 保存済みセッション名を読み込む（将来の復元用）
    let _saved_names = persist::load_session_names();

    let manager = Arc::new(SessionManager::new());
    let manager_for_ipc = Arc::clone(&manager);

    // シャットダウンシグナルを待ちながら IPC を実行する
    tokio::select! {
        result = ipc::serve(manager_for_ipc) => {
            result?;
        }
        _ = shutdown_signal() => {
            info!("シャットダウンシグナルを受信しました。終了します...");
        }
    }

    // シャットダウン時にアクティブなセッション名を保存する
    let arc = manager.sessions();
    let sessions = arc.lock().await;
    let names: Vec<String> = sessions.keys().cloned().collect();
    drop(sessions);
    if !names.is_empty() {
        if let Err(e) = persist::save_session_names(&names) {
            tracing::warn!("セッション名の保存に失敗しました: {}", e);
        }
    }

    info!("nexterm-server を終了しました");
    Ok(())
}

/// Unix/Windows 共通のシャットダウンシグナルハンドラ
#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("SIGTERM ハンドラ設定失敗");
    let mut int = signal(SignalKind::interrupt()).expect("SIGINT ハンドラ設定失敗");
    tokio::select! {
        _ = term.recv() => { info!("SIGTERM を受信しました"); }
        _ = int.recv()  => { info!("SIGINT を受信しました"); }
    }
}

#[cfg(windows)]
async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.expect("Ctrl+C ハンドラ設定失敗");
    info!("Ctrl+C を受信しました");
}
