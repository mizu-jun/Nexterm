//! Template management commands: `template save / load / list`.

use anyhow::{Result, bail};
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// Save the current session layout as a template.
pub(crate) async fn cmd_template_save(name: String, session: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    // Attach to the session first, then send SaveTemplate.
    conn.send(ClientToServer::Attach {
        session_name: session.clone(),
    })
    .await?;
    // Drop the Attach responses (FullRefresh, LayoutChanged, SessionList).
    for _ in 0..8 {
        match conn.recv().await? {
            ServerToClient::SessionList { .. } => break,
            ServerToClient::Error { message } => bail!("{}", message),
            _ => {}
        }
    }
    conn.send(ClientToServer::SaveTemplate { name: name.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::TemplateSaved {
            name: saved_name,
            path,
        } => {
            println!("saved template '{}' to {}", saved_name, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    conn.send(ClientToServer::Detach).await?;
    Ok(())
}

/// Load a saved template.
pub(crate) async fn cmd_template_load(name: String, session: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::Attach {
        session_name: session.clone(),
    })
    .await?;
    for _ in 0..8 {
        match conn.recv().await? {
            ServerToClient::SessionList { .. } => break,
            ServerToClient::Error { message } => bail!("{}", message),
            _ => {}
        }
    }
    conn.send(ClientToServer::LoadTemplate { name: name.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::TemplateLoaded { name: loaded_name } => {
            println!("loaded template '{}'", loaded_name);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    conn.send(ClientToServer::Detach).await?;
    Ok(())
}

/// Display the list of saved templates.
pub(crate) async fn cmd_template_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListTemplates).await?;
    match conn.recv().await? {
        ServerToClient::TemplateList { names } => {
            if names.is_empty() {
                println!("no saved templates");
            } else {
                println!("saved templates:");
                for name in &names {
                    println!("  {}", name);
                }
            }
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}
