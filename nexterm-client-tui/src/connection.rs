//! IPC 接続 — Unix Domain Socket / Named Pipe の抽象化

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use nexterm_proto::{ClientToServer, ServerToClient};

/// IPC 接続ハンドル
pub struct Connection {
    /// サーバーへの送信チャネル（内部タスクが実際の書き込みを行う）
    send_tx: mpsc::Sender<ClientToServer>,
    /// サーバーからの受信チャネル
    recv_rx: mpsc::Receiver<ServerToClient>,
}

impl Connection {
    /// IPC ソケットへ接続する
    pub async fn connect() -> Result<Self> {
        #[cfg(unix)]
        return connect_unix().await;

        #[cfg(windows)]
        return connect_named_pipe().await;
    }

    /// サーバーへメッセージを送信する
    pub async fn send(&mut self, msg: ClientToServer) -> Result<()> {
        self.send_tx
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("サーバーとの接続が切断されました"))
    }

    /// サーバーからのメッセージを non-blocking で受信する
    pub fn try_recv(&mut self) -> Result<ServerToClient, mpsc::error::TryRecvError> {
        self.recv_rx.try_recv()
    }
}

/// 共通の接続セットアップ — ストリームから送受信タスクを起動する
async fn setup_connection<S>(stream: S) -> Result<Connection>
where
    S: AsyncReadExt + AsyncWriteExt + Send + 'static,
{
    let (mut read_half, mut write_half) = tokio::io::split(stream);
    let (send_tx, mut send_rx) = mpsc::channel::<ClientToServer>(256);
    let (recv_tx, recv_rx) = mpsc::channel::<ServerToClient>(256);

    // ハンドシェイク: 接続直後にプロトコルバージョンを送信
    let hello = ClientToServer::Hello {
        proto_version: nexterm_proto::PROTOCOL_VERSION,
        client_kind: nexterm_proto::ClientKind::Tui,
        client_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    if send_tx.send(hello).await.is_err() {
        return Err(anyhow::anyhow!(
            "Hello メッセージの送信キューが閉じています"
        ));
    }

    // 送信タスク: チャネルから取り出してソケットへ書き込む
    tokio::spawn(async move {
        while let Some(msg) = send_rx.recv().await {
            match bincode::serialize(&msg) {
                Ok(payload) => {
                    let len = payload.len() as u32;
                    if write_half.write_all(&len.to_le_bytes()).await.is_err() {
                        break;
                    }
                    if write_half.write_all(&payload).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::error!("シリアライズエラー: {}", e),
            }
        }
    });

    // 受信タスク: ソケットから読み取ってチャネルへ送る
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
                Err(e) => tracing::error!("デシリアライズエラー: {}", e),
            }
        }
    });

    Ok(Connection { send_tx, recv_rx })
}

// ---- Unix Domain Socket ----

#[cfg(unix)]
async fn connect_unix() -> Result<Connection> {
    use tokio::net::UnixStream;

    let socket_path = unix_socket_path();
    let stream = UnixStream::connect(&socket_path).await?;
    setup_connection(stream).await
}

#[cfg(unix)]
fn unix_socket_path() -> String {
    let uid = unsafe { libc::getuid() };
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", uid));
    format!("{}/nexterm.sock", runtime_dir)
}

// ---- Windows Named Pipe ----

#[cfg(windows)]
async fn connect_named_pipe() -> Result<Connection> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let pipe_name = named_pipe_name();
    let stream = ClientOptions::new().open(&pipe_name)?;
    setup_connection(stream).await
}

#[cfg(windows)]
fn named_pipe_name() -> String {
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
    format!("\\\\.\\pipe\\nexterm-{}", username)
}
