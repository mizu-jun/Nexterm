//! nexterm-ctl — nexterm session management CLI
//!
//! # Usage
//!
//! ```text
//! nexterm-ctl list                          # List all sessions
//! nexterm-ctl new <name>                    # Create a new session
//! nexterm-ctl attach <name>                 # Show how to attach to a session
//! nexterm-ctl kill <name>                   # Kill a session
//! nexterm-ctl record start <session> <file> # Start recording PTY output
//! nexterm-ctl record stop <session>         # Stop recording
//! ```

use anyhow::{bail, Result};
use clap::{Arg, Command};
use nexterm_i18n::fl;
use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing_subscriber::EnvFilter;

// ---- CLI 定義（ビルダー形式でロケール対応） ----

fn build_cli() -> Command {
    Command::new("nexterm-ctl")
        .about(fl!("ctl-about"))
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about(fl!("ctl-list-about")))
        .subcommand(
            Command::new("new")
                .about(fl!("ctl-new-about"))
                .arg(Arg::new("name").help(fl!("ctl-arg-name")).required(true)),
        )
        .subcommand(
            Command::new("attach")
                .about(fl!("ctl-attach-about"))
                .arg(Arg::new("name").help(fl!("ctl-arg-name")).required(true)),
        )
        .subcommand(
            Command::new("kill")
                .about(fl!("ctl-kill-about"))
                .arg(Arg::new("name").help(fl!("ctl-arg-name")).required(true)),
        )
        .subcommand(
            Command::new("record")
                .about(fl!("ctl-record-about"))
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("start")
                        .about(fl!("ctl-record-start-about"))
                        .arg(Arg::new("session").help(fl!("ctl-arg-name")).required(true))
                        .arg(
                            Arg::new("file")
                                .help(fl!("ctl-record-arg-file"))
                                .required(true),
                        ),
                )
                .subcommand(
                    Command::new("stop")
                        .about(fl!("ctl-record-stop-about"))
                        .arg(Arg::new("session").help(fl!("ctl-arg-name")).required(true)),
                ),
        )
}

// ---- エントリーポイント ----

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("NEXTERM_LOG"))
        .init();

    // ロケールを検出してから CLI を構築する
    nexterm_i18n::init();

    let matches = build_cli().get_matches();

    match matches.subcommand() {
        Some(("list", _)) => cmd_list().await,
        Some(("new", sub)) => {
            let name = sub.get_one::<String>("name").unwrap().clone();
            cmd_new(name).await
        }
        Some(("attach", sub)) => {
            let name = sub.get_one::<String>("name").unwrap();
            cmd_attach(name)
        }
        Some(("kill", sub)) => {
            let name = sub.get_one::<String>("name").unwrap().clone();
            cmd_kill(name).await
        }
        Some(("record", sub)) => match sub.subcommand() {
            Some(("start", s)) => {
                let session = s.get_one::<String>("session").unwrap().clone();
                let file = s.get_one::<String>("file").unwrap().clone();
                cmd_record_start(session, file).await
            }
            Some(("stop", s)) => {
                let session = s.get_one::<String>("session").unwrap().clone();
                cmd_record_stop(session).await
            }
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

// ---- サブコマンド実装 ----

/// セッション一覧を取得して表示する
async fn cmd_list() -> Result<()> {
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
async fn cmd_new(name: String) -> Result<()> {
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
fn cmd_attach(name: &str) -> Result<()> {
    println!("{}", fl!("ctl-attach-guide", name = name));
    println!("{}", fl!("ctl-attach-tui", name = name));
    println!("{}", fl!("ctl-attach-gpu", name = name));
    Ok(())
}

/// セッションを強制終了する
async fn cmd_kill(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::KillSession { name: name.clone() }).await?;
    match conn.recv().await? {
        ServerToClient::SessionList { .. } => {
            println!("{}", fl!("ctl-session-killed", name = name));
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}

/// セッションのフォーカスペインで録音を開始する
async fn cmd_record_start(session: String, file: String) -> Result<()> {
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
                fl!("ctl-record-started", session = session, pane_id = pane_id, path = path)
            );
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}

/// セッションのフォーカスペインの録音を停止する
async fn cmd_record_stop(session: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::StopRecording { session_name: session.clone() }).await?;
    match conn.recv().await? {
        ServerToClient::RecordingStopped { pane_id } => {
            println!("{}", fl!("ctl-record-stopped", session = session, pane_id = pane_id));
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}

// ---- IPC 接続 ----

/// IPC 接続ラッパー（プラットフォーム非依存の read/write 半）
struct IpcConn {
    reader: Box<dyn AsyncRead + Unpin + Send>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
}

impl IpcConn {
    /// プラットフォームに応じて IPC に接続する
    async fn connect() -> Result<Self> {
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ClientOptions;
            let username =
                std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
            let pipe = format!("\\\\.\\pipe\\nexterm-{}", username);
            let stream = ClientOptions::new().open(&pipe).map_err(|e| {
                anyhow::anyhow!("{}", fl!("ctl-connect-failed", error = e))
            })?;
            let (r, w) = tokio::io::split(stream);
            Ok(Self { reader: Box::new(r), writer: Box::new(w) })
        }

        #[cfg(unix)]
        {
            let uid = unsafe { libc::getuid() };
            let dir = std::env::var("XDG_RUNTIME_DIR")
                .unwrap_or_else(|_| format!("/run/user/{}", uid));
            let path = format!("{}/nexterm.sock", dir);
            let stream = tokio::net::UnixStream::connect(&path).await.map_err(|e| {
                anyhow::anyhow!("{}", fl!("ctl-connect-failed", error = e))
            })?;
            let (r, w) = tokio::io::split(stream);
            Ok(Self { reader: Box::new(r), writer: Box::new(w) })
        }
    }

    /// メッセージを送信する（4B LE 長さプレフィックス + bincode）
    async fn send(&mut self, msg: ClientToServer) -> Result<()> {
        let payload = bincode::serialize(&msg)?;
        let len = payload.len() as u32;
        self.writer.write_all(&len.to_le_bytes()).await?;
        self.writer.write_all(&payload).await?;
        Ok(())
    }

    /// メッセージを受信する
    async fn recv(&mut self) -> Result<ServerToClient> {
        let mut len_buf = [0u8; 4];
        self.reader.read_exact(&mut len_buf).await?;
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        let mut payload = vec![0u8; msg_len];
        self.reader.read_exact(&mut payload).await?;
        Ok(bincode::deserialize(&payload)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bincode_roundtrip_list_sessions() {
        let msg = ClientToServer::ListSessions;
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ClientToServer = bincode::deserialize(&encoded).unwrap();
        assert!(matches!(decoded, ClientToServer::ListSessions));
    }

    #[test]
    fn bincode_roundtrip_kill_session() {
        let msg = ClientToServer::KillSession { name: "main".to_string() };
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ClientToServer = bincode::deserialize(&encoded).unwrap();
        assert!(matches!(decoded, ClientToServer::KillSession { .. }));
    }
}
