//! 録画コマンド: record start / record stop。

use anyhow::{Result, bail};
use nexterm_i18n::fl;
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// セッションのフォーカスペインで録音を開始する
pub(crate) async fn cmd_record_start(session: String, file: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::StartRecording {
        session_name: session.clone(),
        output_path: file.clone(),
    })
    .await?;
    match conn.recv().await? {
        ServerToClient::RecordingStarted { pane_id, path } => {
            println!(
                "{}",
                fl!(
                    "ctl-record-started",
                    session = session,
                    pane_id = pane_id,
                    path = path
                )
            );
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}

/// セッションのフォーカスペインの録音を停止する
pub(crate) async fn cmd_record_stop(session: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::StopRecording {
        session_name: session.clone(),
    })
    .await?;
    match conn.recv().await? {
        ServerToClient::RecordingStopped { pane_id } => {
            println!(
                "{}",
                fl!("ctl-record-stopped", session = session, pane_id = pane_id)
            );
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}
