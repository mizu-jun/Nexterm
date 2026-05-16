//! ワークスペース管理コマンド (Sprint 5-7 / Phase 2-1):
//! `workspace list / create / switch / rename / delete`。
//!
//! IPC `ListWorkspaces` / `CreateWorkspace` / `SwitchWorkspace` /
//! `RenameWorkspace` / `DeleteWorkspace` を送信し、サーバーから
//! `WorkspaceList` / `WorkspaceSwitched` / `Error` のいずれかを受け取って表示する。

use anyhow::{Result, bail};
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// ワークスペース一覧を表示する
pub(crate) async fn cmd_workspace_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListWorkspaces).await?;
    match conn.recv().await? {
        ServerToClient::WorkspaceList {
            current,
            workspaces,
        } => {
            println!(
                "{:<6} {:<20} {:>10} {:<10}",
                "No.", "Name", "Sessions", "Active"
            );
            println!("{}", "-".repeat(60));
            for (i, w) in workspaces.iter().enumerate() {
                let active = if w.is_active { "*" } else { "" };
                println!(
                    "{:<6} {:<20} {:>10} {:<10}",
                    i + 1,
                    w.name,
                    w.session_count,
                    active,
                );
            }
            println!();
            println!("現在のワークスペース: {}", current);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// 新規ワークスペースを作成する
pub(crate) async fn cmd_workspace_create(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::CreateWorkspace { name: name.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::WorkspaceList { workspaces, .. } => {
            println!("ワークスペース '{}' を作成しました", name);
            println!("既知のワークスペース数: {}", workspaces.len());
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// アクティブなワークスペースを切り替える
pub(crate) async fn cmd_workspace_switch(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::SwitchWorkspace { name: name.clone() })
        .await?;
    // 最初の応答が WorkspaceSwitched、続いて WorkspaceList が送られる想定だが、
    // 順序保証を厳密化したくないので最初の有効な応答だけを見て成否を判定する。
    match conn.recv().await? {
        ServerToClient::WorkspaceSwitched { name: switched } => {
            println!("ワークスペースを '{}' に切り替えました", switched);
        }
        ServerToClient::WorkspaceList { current, .. } => {
            println!("ワークスペースを '{}' に切り替えました", current);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// ワークスペースをリネームする
pub(crate) async fn cmd_workspace_rename(from: String, to: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::RenameWorkspace {
        from: from.clone(),
        to: to.clone(),
    })
    .await?;
    match conn.recv().await? {
        ServerToClient::WorkspaceList { .. } => {
            println!("ワークスペース '{}' を '{}' にリネームしました", from, to);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// ワークスペースを削除する
///
/// `force=true` の場合、配下のセッションを default ワークスペースに退避させる。
pub(crate) async fn cmd_workspace_delete(name: String, force: bool) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::DeleteWorkspace {
        name: name.clone(),
        force,
    })
    .await?;
    match conn.recv().await? {
        ServerToClient::WorkspaceList { .. } => {
            println!("ワークスペース '{}' を削除しました (force={})", name, force);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}
