//! Workspace management commands (Sprint 5-7 / Phase 2-1):
//! `workspace list / create / switch / rename / delete`.
//!
//! Sends one of `ListWorkspaces` / `CreateWorkspace` / `SwitchWorkspace` /
//! `RenameWorkspace` / `DeleteWorkspace` over IPC and renders the matching
//! `WorkspaceList` / `WorkspaceSwitched` / `Error` response.

use anyhow::{Result, bail};
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// Display the workspace list.
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
            println!("current workspace: {}", current);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// Create a new workspace.
pub(crate) async fn cmd_workspace_create(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::CreateWorkspace { name: name.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::WorkspaceList { workspaces, .. } => {
            println!("created workspace '{}'", name);
            println!("total workspaces: {}", workspaces.len());
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// Switch the active workspace.
pub(crate) async fn cmd_workspace_switch(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::SwitchWorkspace { name: name.clone() })
        .await?;
    // The first response is expected to be `WorkspaceSwitched` followed by
    // `WorkspaceList`, but rather than asserting on the order we just inspect the
    // first useful response to decide success/failure.
    match conn.recv().await? {
        ServerToClient::WorkspaceSwitched { name: switched } => {
            println!("switched to workspace '{}'", switched);
        }
        ServerToClient::WorkspaceList { current, .. } => {
            println!("switched to workspace '{}'", current);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// Rename a workspace.
pub(crate) async fn cmd_workspace_rename(from: String, to: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::RenameWorkspace {
        from: from.clone(),
        to: to.clone(),
    })
    .await?;
    match conn.recv().await? {
        ServerToClient::WorkspaceList { .. } => {
            println!("renamed workspace '{}' to '{}'", from, to);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// Delete a workspace.
///
/// When `force=true`, the sessions still under that workspace are migrated to
/// the `default` workspace before deletion.
pub(crate) async fn cmd_workspace_delete(name: String, force: bool) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::DeleteWorkspace {
        name: name.clone(),
        force,
    })
    .await?;
    match conn.recv().await? {
        ServerToClient::WorkspaceList { .. } => {
            println!("deleted workspace '{}' (force={})", name, force);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}
