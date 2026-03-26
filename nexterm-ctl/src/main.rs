//! nexterm-ctl — nexterm セッション制御 CLI
//!
//! # 使用例
//!
//! ```text
//! nexterm-ctl list              # セッション一覧を表示する
//! nexterm-ctl new <name>        # 新規セッションを作成する
//! nexterm-ctl attach <name>     # セッションへのアタッチ方法を表示する
//! nexterm-ctl kill <name>       # セッションを強制終了する
//! ```

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing_subscriber::EnvFilter;

// ---- CLI 定義 ----

#[derive(Parser)]
#[command(name = "nexterm-ctl", about = "nexterm セッション制御 CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// セッション一覧を表示する
    List,
    /// 新規セッションを作成する
    New {
        /// セッション名
        name: String,
    },
    /// セッションへのアタッチ方法を案内する
    Attach {
        /// セッション名
        name: String,
    },
    /// セッションを強制終了する
    Kill {
        /// セッション名
        name: String,
    },
}

// ---- エントリーポイント ----

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("NEXTERM_LOG"))
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::List => cmd_list().await,
        Commands::New { name } => cmd_new(name).await,
        Commands::Attach { name } => cmd_attach(&name),
        Commands::Kill { name } => cmd_kill(name).await,
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
                println!("セッションはありません。");
            } else {
                println!("{:<20} {:<8} {}", "名前", "ウィンドウ数", "アタッチ状態");
                println!("{}", "-".repeat(44));
                for s in &sessions {
                    let state = if s.attached { "アタッチ中" } else { "デタッチ" };
                    println!("{:<20} {:<8} {}", s.name, s.window_count, state);
                }
            }
        }
        ServerToClient::Error { message } => bail!("サーバーエラー: {}", message),
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

    // SessionList を受け取るまで最大8メッセージ読み飛ばす
    let mut created = false;
    for _ in 0..8 {
        match conn.recv().await? {
            ServerToClient::SessionList { sessions } => {
                created = sessions.iter().any(|s| s.name == name);
                break;
            }
            ServerToClient::Error { message } => bail!("エラー: {}", message),
            _ => {} // FullRefresh / LayoutChanged などは読み飛ばす
        }
    }
    conn.send(ClientToServer::Detach).await?;

    if created {
        println!("セッション '{}' を作成しました。", name);
        println!("アタッチするには nexterm-client-tui または nexterm-client-gpu を起動してください。");
    } else {
        bail!("セッション '{}' の作成を確認できませんでした。", name);
    }

    Ok(())
}

/// セッションへのアタッチ方法を案内する（ctl 自体はインタラクティブ端末ではない）
fn cmd_attach(name: &str) -> Result<()> {
    println!("セッション '{}' にアタッチするには:", name);
    println!("  TUI クライアント: NEXTERM_SESSION={name} nexterm-client-tui");
    println!("  GPU クライアント: NEXTERM_SESSION={name} nexterm-client-gpu");
    Ok(())
}

/// セッションを強制終了する
async fn cmd_kill(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::KillSession { name: name.clone() }).await?;
    match conn.recv().await? {
        ServerToClient::SessionList { .. } => {
            println!("セッション '{}' を終了しました。", name);
        }
        ServerToClient::Error { message } => bail!("エラー: {}", message),
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
                anyhow::anyhow!(
                    "nexterm サーバーへの接続に失敗しました: {}\n\
                     nexterm-server が起動しているか確認してください。",
                    e
                )
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
                anyhow::anyhow!(
                    "nexterm サーバーへの接続に失敗しました: {}\n\
                     nexterm-server が起動しているか確認してください。",
                    e
                )
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
    fn メッセージのbincode往復() {
        let msg = ClientToServer::ListSessions;
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ClientToServer = bincode::deserialize(&encoded).unwrap();
        assert!(matches!(decoded, ClientToServer::ListSessions));
    }

    #[test]
    fn kill_sessionのシリアライズ() {
        let msg = ClientToServer::KillSession { name: "main".to_string() };
        let encoded = bincode::serialize(&msg).unwrap();
        let decoded: ClientToServer = bincode::deserialize(&encoded).unwrap();
        assert!(matches!(decoded, ClientToServer::KillSession { .. }));
    }
}
