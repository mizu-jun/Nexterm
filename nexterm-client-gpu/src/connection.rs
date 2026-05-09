//! IPC 接続 — client-tui と同じプロトコルを GPU クライアントで使う

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use nexterm_proto::{ClientToServer, ServerToClient};

pub struct Connection {
    pub send_tx: mpsc::Sender<ClientToServer>,
    pub recv_rx: mpsc::Receiver<ServerToClient>,
}

impl Connection {
    pub async fn connect() -> Result<Self> {
        #[cfg(unix)]
        return connect_unix().await;
        #[cfg(windows)]
        return connect_named_pipe().await;
    }

    #[allow(dead_code)]
    pub async fn send(&mut self, msg: ClientToServer) -> Result<()> {
        self.send_tx
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("サーバーとの接続が切断されました"))
    }

    #[allow(dead_code)]
    pub fn try_recv(&mut self) -> Result<ServerToClient, mpsc::error::TryRecvError> {
        self.recv_rx.try_recv()
    }
}

async fn setup<S>(stream: S) -> Result<Connection>
where
    S: AsyncReadExt + AsyncWriteExt + Send + 'static,
{
    let (mut read_half, mut write_half) = tokio::io::split(stream);
    let (send_tx, mut send_rx) = mpsc::channel::<ClientToServer>(256);
    let (recv_tx, recv_rx) = mpsc::channel::<ServerToClient>(256);

    // ハンドシェイク: 接続直後にプロトコルバージョンを送信（CRITICAL/B1 対応）
    let hello = ClientToServer::Hello {
        proto_version: nexterm_proto::PROTOCOL_VERSION,
        client_kind: nexterm_proto::ClientKind::Gpu,
        client_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    if send_tx.send(hello).await.is_err() {
        return Err(anyhow::anyhow!(
            "Hello メッセージの送信キューが閉じています"
        ));
    }

    tokio::spawn(async move {
        while let Some(msg) = send_rx.recv().await {
            if let Ok(payload) = bincode::serialize(&msg) {
                let len = payload.len() as u32;
                if write_half.write_all(&len.to_le_bytes()).await.is_err() {
                    break;
                }
                if write_half.write_all(&payload).await.is_err() {
                    break;
                }
            }
        }
    });

    tokio::spawn(async move {
        loop {
            let mut len_buf = [0u8; 4];
            if read_half.read_exact(&mut len_buf).await.is_err() {
                break;
            }
            let msg_len = u32::from_le_bytes(len_buf) as usize;
            // 巨大な長さプレフィックスによる OOM 攻撃を防ぐ
            if nexterm_proto::validate_msg_len(msg_len).is_err() {
                tracing::error!(
                    "サーバーからの IPC メッセージサイズが上限超過: {} バイト — 接続を切断します",
                    msg_len
                );
                break;
            }
            let mut payload = vec![0u8; msg_len];
            if read_half.read_exact(&mut payload).await.is_err() {
                break;
            }
            match bincode::deserialize::<ServerToClient>(&payload) {
                Ok(msg) => {
                    if recv_tx.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    // サイレントドロップせず警告ログ（プロトコル不整合の検出に必要）
                    tracing::warn!("ServerToClient のデシリアライズに失敗: {}", e);
                }
            }
        }
    });

    Ok(Connection { send_tx, recv_rx })
}

#[cfg(unix)]
async fn connect_unix() -> Result<Connection> {
    let uid = unsafe { libc::getuid() };
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", uid));
    let path = format!("{}/nexterm.sock", dir);
    let stream = tokio::net::UnixStream::connect(path).await?;
    setup(stream).await
}

#[cfg(windows)]
async fn connect_named_pipe() -> Result<Connection> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
    let name = format!("\\\\.\\pipe\\nexterm-{}", username);
    let stream = ClientOptions::new().open(&name)?;
    setup(stream).await
}
