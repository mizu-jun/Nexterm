//! セッション基本操作コマンド: list / new / attach / kill。

use anyhow::{Result, bail};
use nexterm_i18n::fl;
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// セッション一覧を取得して表示する
pub(crate) async fn cmd_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListSessions).await?;

    match conn.recv().await? {
        ServerToClient::SessionList { sessions } => {
            if sessions.is_empty() {
                println!("{}", fl!("ctl-no-sessions"));
            } else {
                println!(
                    "{:<20} {:<12} {}",
                    fl!("ctl-list-col-name"),
                    fl!("ctl-list-col-windows"),
                    fl!("ctl-list-col-status")
                );
                println!("{}", "-".repeat(48));
                for s in &sessions {
                    let status = if s.attached {
                        fl!("ctl-status-attached")
                    } else {
                        fl!("ctl-status-detached")
                    };
                    println!("{:<20} {:<12} {}", s.name, s.window_count, status);
                }
            }
        }
        ServerToClient::Error { message } => {
            bail!("{}", fl!("ctl-server-error", message = message))
        }
        _ => {}
    }

    Ok(())
}

/// 新規セッションを作成してすぐデタッチする
pub(crate) async fn cmd_new(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::Attach {
        session_name: name.clone(),
    })
    .await?;

    // SessionList を受け取るまで最大 8 メッセージ読み飛ばす
    let mut created = false;
    for _ in 0..8 {
        match conn.recv().await? {
            ServerToClient::SessionList { sessions } => {
                created = sessions.iter().any(|s| s.name == name);
                break;
            }
            ServerToClient::Error { message } => {
                bail!("{}", fl!("ctl-error", message = message))
            }
            _ => {}
        }
    }
    conn.send(ClientToServer::Detach).await?;

    if created {
        println!("{}", fl!("ctl-session-created", name = name));
        println!("{}", fl!("ctl-session-created-hint"));
    } else {
        bail!("{}", fl!("ctl-session-create-failed", name = name));
    }

    Ok(())
}

/// セッションへのアタッチ方法を案内する（ctl 自体はインタラクティブ端末ではない）
pub(crate) fn cmd_attach(name: &str) -> Result<()> {
    println!("{}", fl!("ctl-attach-guide", name = name));
    println!("{}", fl!("ctl-attach-tui", name = name));
    println!("{}", fl!("ctl-attach-gpu", name = name));
    Ok(())
}

/// セッションを強制終了する
pub(crate) async fn cmd_kill(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::KillSession { name: name.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::SessionList { .. } => {
            println!("{}", fl!("ctl-session-killed", name = name));
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}
