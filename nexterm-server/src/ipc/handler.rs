//! 共通クライアントハンドラ — 接続済みストリームの読み書きを処理する

use anyhow::Result;
use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::runtime_config::SharedRuntimeConfig;
use crate::session::SessionManager;

/// 接続済みクライアントの読み書きを処理する
pub(super) async fn handle_client<S>(
    stream: S,
    manager: std::sync::Arc<SessionManager>,
    runtime_cfg: SharedRuntimeConfig,
    lua: std::sync::Arc<nexterm_config::LuaHookRunner>,
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

    // ハンドシェイク状態
    let mut hello_received = false;

    // クライアント → サーバー 受信ループ
    loop {
        let mut len_buf = [0u8; 4];
        if read_half.read_exact(&mut len_buf).await.is_err() {
            info!("クライアントが切断しました");
            break;
        }
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        // 巨大な長さプレフィックスによる OOM 攻撃を防ぐ
        if let Err(e) = nexterm_proto::validate_msg_len(msg_len) {
            error!("{} — 接続を切断します", e);
            break;
        }
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

        // ハンドシェイク: 最初のメッセージは Hello でなければならない
        if !hello_received {
            match &msg {
                ClientToServer::Hello {
                    proto_version,
                    client_kind,
                    client_version,
                } => {
                    if *proto_version != nexterm_proto::PROTOCOL_VERSION {
                        error!(
                            "プロトコルバージョン不一致: クライアント={}, サーバー={}。接続を切断します。",
                            proto_version,
                            nexterm_proto::PROTOCOL_VERSION
                        );
                        let _ = tx
                            .send(ServerToClient::Error {
                                message: format!(
                                    "プロトコルバージョン不一致 (client={}, server={})。\
                                     クライアントを更新してください。",
                                    proto_version,
                                    nexterm_proto::PROTOCOL_VERSION
                                ),
                            })
                            .await;
                        break;
                    }
                    info!(
                        "クライアント Hello 受信: kind={:?}, version={}, proto={}",
                        client_kind, client_version, proto_version
                    );
                    // HelloAck で応答
                    let _ = tx
                        .send(ServerToClient::HelloAck {
                            proto_version: nexterm_proto::PROTOCOL_VERSION,
                            server_version: env!("CARGO_PKG_VERSION").to_string(),
                        })
                        .await;
                    hello_received = true;
                    continue;
                }
                _ => {
                    error!("ハンドシェイク前に非 Hello メッセージを受信。接続を切断します。");
                    let _ = tx
                        .send(ServerToClient::Error {
                            message: "ハンドシェイクが必要です。Hello メッセージを最初に送信してください。"
                                .to_string(),
                        })
                        .await;
                    break;
                }
            }
        }

        // 各メッセージ処理ごとに最新のランタイム設定スナップショットを取得する
        // （config watcher が ArcSwap を差し替えても次のメッセージから即座に反映される）
        let snapshot = runtime_cfg.load_full();
        super::dispatch::dispatch(
            &msg,
            &manager,
            tx.clone(),
            &mut current_session,
            &snapshot.hooks,
            std::sync::Arc::clone(&lua),
            &snapshot.log_config,
            &snapshot.hosts,
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
        // on_detach フック（切断時、最新スナップショットの hooks を使用）
        let snapshot = runtime_cfg.load_full();
        crate::hooks::on_detach(&snapshot.hooks, &lua, name);
    }

    Ok(())
}
