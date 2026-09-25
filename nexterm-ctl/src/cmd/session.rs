//! Basic session commands: list / new / attach / kill.

use anyhow::{Result, bail};
use nexterm_i18n::fl;
use nexterm_proto::{ClientToServer, ServerToClient};

use crate::ipc::IpcConn;

/// Fetch and display the session list.
///
/// `json`: architecture-comparison audit follow-up (2026-09, item #20).
/// `nexterm-ctl` otherwise only prints locale-dependent, fixed-width text via
/// `fl!` — fine for a human at a terminal, useless to a script that wants to
/// know how many sessions exist or whether one is attached. `--json` emits
/// NDJSON (one compact JSON object per line, via `SessionInfo`'s existing
/// `Serialize` impl — no new wire format, no change to the postcard IPC path)
/// so a caller can `jq` it without locale-parsing English/Japanese/etc. text.
/// Errors still go to the human-readable path either way: a script piping
/// `--json` output should treat a non-zero exit / stderr line as failure, not
/// try to parse an error out of stdout.
pub(crate) async fn cmd_list(json: bool) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListSessions).await?;

    match conn.recv().await? {
        ServerToClient::SessionList { sessions } => {
            if json {
                for s in &sessions {
                    println!("{}", serde_json::to_string(s)?);
                }
            } else if sessions.is_empty() {
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

/// Create a new session and detach immediately.
pub(crate) async fn cmd_new(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::Attach {
        session_name: name.clone(),
    })
    .await?;

    // Read up to 8 messages while waiting for the SessionList.
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

/// Print attach instructions (`ctl` itself is not an interactive terminal).
pub(crate) fn cmd_attach(name: &str) -> Result<()> {
    println!("{}", fl!("ctl-attach-guide", name = name));
    println!("{}", fl!("ctl-attach-tui", name = name));
    println!("{}", fl!("ctl-attach-gpu", name = name));
    Ok(())
}

/// Forcibly kill a session.
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
