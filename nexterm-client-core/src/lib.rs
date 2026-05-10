//! クライアント-サーバー IPC 共通実装（Sprint 3-6）
//!
//! `nexterm-client-gpu` / `nexterm-client-tui` の `connection.rs` に重複していた
//! Unix Domain Socket / Windows Named Pipe 上のフレーミング・ハンドシェイク・
//! 送受信タスク管理ロジックをここに集約する。
//!
//! # 使用例
//!
//! ```ignore
//! use nexterm_client_core::Connection;
//! use nexterm_proto::ClientKind;
//!
//! let mut conn = Connection::connect(
//!     ClientKind::Tui,
//!     env!("CARGO_PKG_VERSION").to_string(),
//! ).await?;
//!
//! conn.send(ClientToServer::Ping).await?;
//! if let Ok(msg) = conn.try_recv() { /* ... */ }
//! ```
//!
//! # フレーミング
//!
//! - 各メッセージは「4 バイトの長さプレフィックス（little-endian）+ bincode ペイロード」
//! - 受信側は [`nexterm_proto::validate_msg_len`] で OOM 攻撃を防ぐ
//! - 接続直後に `ClientToServer::Hello { proto_version, client_kind, client_version }` を送信
//!
//! # 切断検知
//!
//! - 送受信タスクは I/O エラー発生時に終了する
//! - メインタスクは `send` の `Err` または `try_recv` の `Disconnected` で検知する

use anyhow::Result;
use nexterm_proto::{
    ClientKind, ClientToServer, PROTOCOL_VERSION, ServerToClient, validate_msg_len,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// IPC 接続ハンドル
pub struct Connection {
    /// サーバーへの送信チャネル（内部タスクが実際の書き込みを行う）
    pub send_tx: mpsc::Sender<ClientToServer>,
    /// サーバーからの受信チャネル
    pub recv_rx: mpsc::Receiver<ServerToClient>,
}

impl Connection {
    /// IPC ソケットへ接続して送受信タスクを起動する
    pub async fn connect(client_kind: ClientKind, client_version: String) -> Result<Self> {
        #[cfg(unix)]
        {
            connect_unix(client_kind, client_version).await
        }
        #[cfg(windows)]
        {
            connect_named_pipe(client_kind, client_version).await
        }
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

/// 既定のソケットパスを返す
#[cfg(unix)]
pub fn unix_socket_path() -> String {
    let uid = unsafe { libc::getuid() };
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", uid));
    format!("{}/nexterm.sock", runtime_dir)
}

/// 既定の名前付きパイプ名を返す
#[cfg(windows)]
pub fn named_pipe_name() -> String {
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
    format!("\\\\.\\pipe\\nexterm-{}", username)
}

/// 共通の接続セットアップ — ストリームから送受信タスクを起動する
async fn setup<S>(stream: S, client_kind: ClientKind, client_version: String) -> Result<Connection>
where
    S: AsyncReadExt + AsyncWriteExt + Send + 'static,
{
    let (mut read_half, mut write_half) = tokio::io::split(stream);
    let (send_tx, mut send_rx) = mpsc::channel::<ClientToServer>(256);
    let (recv_tx, recv_rx) = mpsc::channel::<ServerToClient>(256);

    // ハンドシェイク: 接続直後にプロトコルバージョンを送信（CRITICAL/B1 対応）
    let hello = ClientToServer::Hello {
        proto_version: PROTOCOL_VERSION,
        client_kind,
        client_version,
    };
    if send_tx.send(hello).await.is_err() {
        return Err(anyhow::anyhow!(
            "Hello メッセージの送信キューが閉じています"
        ));
    }

    // 送信タスク: チャネルから取り出してソケットへ書き込む
    tokio::spawn(async move {
        while let Some(msg) = send_rx.recv().await {
            match postcard::to_stdvec(&msg) {
                Ok(payload) => {
                    let len = payload.len() as u32;
                    if write_half.write_all(&len.to_le_bytes()).await.is_err() {
                        break;
                    }
                    if write_half.write_all(&payload).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::error!("ClientToServer のシリアライズに失敗: {}", e),
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
            if validate_msg_len(msg_len).is_err() {
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

            match postcard::from_bytes::<ServerToClient>(&payload) {
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
async fn connect_unix(client_kind: ClientKind, client_version: String) -> Result<Connection> {
    use tokio::net::UnixStream;

    let socket_path = unix_socket_path();
    let stream = UnixStream::connect(&socket_path).await?;
    setup(stream, client_kind, client_version).await
}

#[cfg(windows)]
async fn connect_named_pipe(client_kind: ClientKind, client_version: String) -> Result<Connection> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let pipe_name = named_pipe_name();
    let stream = ClientOptions::new().open(&pipe_name)?;
    setup(stream, client_kind, client_version).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_socket_path_returns_xdg_runtime_dir_path() {
        // XDG_RUNTIME_DIR が設定されている環境ではそれを使う
        // 設定されていない場合は /run/user/{uid}/nexterm.sock
        #[cfg(unix)]
        {
            let path = unix_socket_path();
            assert!(path.ends_with("nexterm.sock"));
        }
        #[cfg(windows)]
        {
            let name = named_pipe_name();
            assert!(name.starts_with(r"\\.\pipe\nexterm-"));
        }
    }

    #[test]
    fn connection_struct_has_expected_fields() {
        // Connection 構造体のフィールドが pub で外部から作成可能であることを確認
        // （main からは Connection::connect() 経由で作るため、構造体直接生成はテストのみ）
        let (send_tx, _send_rx) = mpsc::channel::<ClientToServer>(1);
        let (_recv_tx, recv_rx) = mpsc::channel::<ServerToClient>(1);
        let _conn = Connection { send_tx, recv_rx };
    }
}
