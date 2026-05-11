//! WASM プラグイン管理コマンド: plugin list / load / unload / reload。

use anyhow::{Result, bail};
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// ロード済みプラグイン一覧を表示する
pub(crate) async fn cmd_plugin_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListPlugins).await?;
    match conn.recv().await? {
        ServerToClient::PluginList { paths } => {
            if paths.is_empty() {
                println!("ロード済みプラグインはありません");
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

/// WASM プラグインをロードする
pub(crate) async fn cmd_plugin_load(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::LoadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("プラグインを{}しました: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// プラグインをアンロードする
pub(crate) async fn cmd_plugin_unload(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::UnloadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("プラグインを{}しました: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// プラグインを再ロードする
pub(crate) async fn cmd_plugin_reload(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ReloadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("プラグインを{}しました: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}
