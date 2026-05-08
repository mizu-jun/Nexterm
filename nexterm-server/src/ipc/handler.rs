//! 共通クライアントハンドラ — 接続済みストリームの読み書きを処理する

use anyhow::Result;
use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::session::SessionManager;

/// 接続済みクライアントの読み書きを処理する
pub(super) async fn handle_client<S>(
    stream: S,
    manager: std::sync::Arc<SessionManager>,
    hooks: std::sync::Arc<nexterm_config::HooksConfig>,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
    log_config: std::sync::Arc<nexterm_config::LogConfig>,
    hosts: std::sync::Arc<Vec<nexterm_config::HostConfig>>,
) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel::<ServerToClient>(256);
    let (mut read_half, mut write_half) = tokio::io::split(stream);

    // サーバー → クライアント 送信タスク
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match bincode::serialize(&msg) {
                Ok(payload) => {
                    let len = payload.len() as u32;
                    if write_half.write_all(&len.to_le_bytes()).await.is_err() {
                        break;
                    }
                    if write_half.write_all(&payload).await.is_err() {
                        break;
                    }
                }
                Err(e) => error!("シリアライズエラー: {}", e),
            }
        }
    });

    // 接続中のセッション名（Attach で設定される）
    let mut current_session: Option<String> = None;

    // broadcast forwarder タスクのハンドル（Attach 時に設定、切断時に abort）
    let mut bcast_forwarder: Option<tokio::task::AbortHandle> = None;

    // クライアント → サーバー 受信ループ
    loop {
        let mut len_buf = [0u8; 4];
        if read_half.read_exact(&mut len_buf).await.is_err() {
            info!("クライアントが切断しました");
            break;
        }
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        let mut payload = vec![0u8; msg_len];
        if read_half.read_exact(&mut payload).await.is_err() {
            break;
        }
        let msg: ClientToServer = match bincode::deserialize(&payload) {
            Ok(m) => m,
            Err(e) => {
                error!("デシリアライズエラー: {}", e);
                continue;
            }
        };

        super::dispatch::dispatch(
            &msg,
            &manager,
            tx.clone(),
            &mut current_session,
            &hooks,
            std::sync::Arc::clone(&lua),
            &log_config,
            &hosts,
            &mut bcast_forwarder,
        )
        .await;
    }

    // クリーンアップ: broadcast forwarder を停止する
    if let Some(h) = bcast_forwarder.take() {
        h.abort();
    }
    if let Some(ref name) = current_session {
        let arc = manager.sessions();
        let mut sessions = arc.lock().await;
        if let Some(session) = sessions.get_mut(name) {
            session.detach_one(&tx);
            info!("切断によりセッション '{}' からデタッチしました", name);
        }
        // on_detach フック（切断時）
        crate::hooks::on_detach(&hooks, &lua, name);
    }

    Ok(())
}
