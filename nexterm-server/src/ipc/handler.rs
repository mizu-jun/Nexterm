//! Common client handler — drives read/write on a connected stream.

use anyhow::Result;
use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::runtime_config::SharedRuntimeConfig;
use crate::session::SessionManager;

/// Handle reads and writes on a connected client.
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

    // Server -> client send task.
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match postcard::to_stdvec(&msg) {
                Ok(payload) => {
                    let len = payload.len() as u32;
                    if write_half.write_all(&len.to_le_bytes()).await.is_err() {
                        break;
                    }
                    if write_half.write_all(&payload).await.is_err() {
                        break;
                    }
                }
                Err(e) => error!("serialization error: {}", e),
            }
        }
    });

    // Currently attached session name (set by Attach).
    let mut current_session: Option<String> = None;

    // Handle for the broadcast forwarder task (set on Attach, aborted on disconnect).
    let mut bcast_forwarder: Option<tokio::task::AbortHandle> = None;

    // Handshake state.
    let mut hello_received = false;

    // Client -> server receive loop.
    loop {
        let mut len_buf = [0u8; 4];
        if read_half.read_exact(&mut len_buf).await.is_err() {
            info!("client disconnected");
            break;
        }
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        // Defend against OOM attacks using a huge length prefix.
        if let Err(e) = nexterm_proto::validate_msg_len(msg_len) {
            error!("{} — closing connection", e);
            break;
        }
        let mut payload = vec![0u8; msg_len];
        if read_half.read_exact(&mut payload).await.is_err() {
            break;
        }
        let msg: ClientToServer = match postcard::from_bytes(&payload) {
            Ok(m) => m,
            Err(e) => {
                error!("deserialization error: {}", e);
                continue;
            }
        };

        // Handshake: the first message must be Hello.
        if !hello_received {
            match &msg {
                ClientToServer::Hello {
                    proto_version,
                    client_kind,
                    client_version,
                } => {
                    if *proto_version != nexterm_proto::PROTOCOL_VERSION {
                        error!(
                            "protocol version mismatch: client={}, server={}; closing connection",
                            proto_version,
                            nexterm_proto::PROTOCOL_VERSION
                        );
                        let _ = tx
                            .send(ServerToClient::Error {
                                message: format!(
                                    "protocol version mismatch (client={}, server={}). \
                                     please update the client.",
                                    proto_version,
                                    nexterm_proto::PROTOCOL_VERSION
                                ),
                            })
                            .await;
                        break;
                    }
                    info!(
                        "received client Hello: kind={:?}, version={}, proto={}",
                        client_kind, client_version, proto_version
                    );
                    // Reply with HelloAck.
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
                    error!("received non-Hello message before handshake; closing connection");
                    let _ = tx
                        .send(ServerToClient::Error {
                            message: "handshake required: send a Hello message first.".to_string(),
                        })
                        .await;
                    break;
                }
            }
        }

        // Fetch the latest runtime config snapshot per message.
        // (When the config watcher swaps the `ArcSwap`, the next message picks it up immediately.)
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

    // Cleanup: stop the broadcast forwarder.
    if let Some(h) = bcast_forwarder.take() {
        h.abort();
    }
    if let Some(ref name) = current_session {
        let arc = manager.sessions();
        let mut sessions = arc.lock().await;
        if let Some(session) = sessions.get_mut(name) {
            session.detach_one(&tx);
            info!("detached from session '{}' on disconnect", name);
        }
        // on_detach hook (on disconnect, using the latest snapshot's hooks).
        let snapshot = runtime_cfg.load_full();
        crate::hooks::on_detach(&snapshot.hooks, &lua, name);
    }

    Ok(())
}
