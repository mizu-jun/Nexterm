//! IPC connection wrapper (Sprint 5-4 / A1: extracted from `main.rs`).
//!
//! A platform-agnostic read/write split that handles both Unix sockets and Windows
//! named pipes. Provides the handshake plus message send/receive helpers.

use anyhow::Result;
use nexterm_i18n::fl;
use nexterm_proto::{ClientToServer, ServerToClient};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// IPC connection wrapper (platform-agnostic read/write halves).
pub(crate) struct IpcConn {
    reader: Box<dyn AsyncRead + Unpin + Send>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
}

impl IpcConn {
    /// Connect to the IPC socket appropriate for the current platform.
    pub(crate) async fn connect() -> Result<Self> {
        let mut conn: Self = {
            #[cfg(windows)]
            {
                use tokio::net::windows::named_pipe::ClientOptions;
                let username = std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
                let pipe = format!("\\\\.\\pipe\\nexterm-{}", username);
                let stream = ClientOptions::new()
                    .open(&pipe)
                    .map_err(|e| anyhow::anyhow!("{}", fl!("ctl-connect-failed", error = e)))?;
                let (r, w) = tokio::io::split(stream);
                Self {
                    reader: Box::new(r),
                    writer: Box::new(w),
                }
            }

            #[cfg(unix)]
            {
                let uid = unsafe { libc::getuid() };
                let dir = std::env::var("XDG_RUNTIME_DIR")
                    .unwrap_or_else(|_| format!("/run/user/{}", uid));
                let path = format!("{}/nexterm.sock", dir);
                let stream = tokio::net::UnixStream::connect(&path)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", fl!("ctl-connect-failed", error = e)))?;
                let (r, w) = tokio::io::split(stream);
                Self {
                    reader: Box::new(r),
                    writer: Box::new(w),
                }
            }
        };

        // Handshake: send the protocol version immediately after connecting and wait for HelloAck.
        conn.send(ClientToServer::Hello {
            proto_version: nexterm_proto::PROTOCOL_VERSION,
            client_kind: nexterm_proto::ClientKind::Ctl,
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
        .await?;
        match conn.recv().await? {
            ServerToClient::HelloAck { .. } => {}
            ServerToClient::Error { message } => {
                anyhow::bail!("handshake error from server: {}", message);
            }
            other => {
                anyhow::bail!(
                    "unexpected handshake response: {:?} (expected HelloAck)",
                    other
                );
            }
        }

        Ok(conn)
    }

    /// Send a message (4-byte LE length prefix + postcard payload).
    pub(crate) async fn send(&mut self, msg: ClientToServer) -> Result<()> {
        let payload = postcard::to_stdvec(&msg)?;
        let len = payload.len() as u32;
        self.writer.write_all(&len.to_le_bytes()).await?;
        self.writer.write_all(&payload).await?;
        Ok(())
    }

    /// Receive a message.
    pub(crate) async fn recv(&mut self) -> Result<ServerToClient> {
        let mut len_buf = [0u8; 4];
        self.reader.read_exact(&mut len_buf).await?;
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        // Guard against OOM attacks with a huge length prefix.
        nexterm_proto::validate_msg_len(msg_len).map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut payload = vec![0u8; msg_len];
        self.reader.read_exact(&mut payload).await?;
        Ok(postcard::from_bytes(&payload)?)
    }
}
