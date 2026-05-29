//! WASM plugin management commands: `plugin list / load / unload / reload`.

use anyhow::{Result, bail};
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// Display the list of loaded plugins.
pub(crate) async fn cmd_plugin_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListPlugins).await?;
    match conn.recv().await? {
        ServerToClient::PluginList { paths } => {
            if paths.is_empty() {
                println!("no plugins are loaded");
            } else {
                println!("{:<6} Path", "No.");
                println!("{}", "-".repeat(60));
                for (i, path) in paths.iter().enumerate() {
                    println!("{:<6} {}", i + 1, path);
                }
            }
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// Load a WASM plugin.
pub(crate) async fn cmd_plugin_load(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::LoadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("plugin {}: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// Unload a plugin.
pub(crate) async fn cmd_plugin_unload(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::UnloadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("plugin {}: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// Reload a plugin.
pub(crate) async fn cmd_plugin_reload(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ReloadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("plugin {}: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}
