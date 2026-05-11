//! テンプレート管理コマンド: template save / load / list。

use anyhow::{Result, bail};
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// 現在のセッションレイアウトをテンプレートとして保存する
pub(crate) async fn cmd_template_save(name: String, session: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    // セッションにアタッチしてから SaveTemplate を送信する
    conn.send(ClientToServer::Attach {
        session_name: session.clone(),
    })
    .await?;
    // Attach の応答（FullRefresh, LayoutChanged, SessionList）を読み飛ばす
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
            println!("テンプレート '{}' を保存しました: {}", saved_name, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    conn.send(ClientToServer::Detach).await?;
    Ok(())
}

/// 保存済みテンプレートを読み込む
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
            println!("テンプレート '{}' を読み込みました", loaded_name);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    conn.send(ClientToServer::Detach).await?;
    Ok(())
}

/// 保存済みテンプレート一覧を表示する
pub(crate) async fn cmd_template_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListTemplates).await?;
    match conn.recv().await? {
        ServerToClient::TemplateList { names } => {
            if names.is_empty() {
                println!("保存済みテンプレートはありません");
            } else {
                println!("保存済みテンプレート:");
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
