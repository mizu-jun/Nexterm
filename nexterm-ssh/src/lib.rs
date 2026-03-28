//! SSH クライアント統合 — russh を使った SSH 接続管理

use anyhow::{bail, Result};
use russh::client::{self, Handle};
use russh::keys::{load_secret_key, PublicKey, PrivateKeyWithHashAlg};
use russh::ChannelMsg;
use std::sync::Arc;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

/// SSH 接続設定
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
}

/// SSH 認証方式
#[derive(Debug, Clone)]
pub enum SshAuth {
    /// パスワード認証
    Password(Zeroizing<String>),
    /// 公開鍵認証（秘密鍵ファイルパス）
    PrivateKey {
        key_path: std::path::PathBuf,
        passphrase: Option<Zeroizing<String>>,
    },
    /// SSH エージェント認証
    Agent,
}

/// ホスト鍵の検証ポリシー（MVP: 常に信頼）
struct AcceptAllVerifier;

impl client::Handler for AcceptAllVerifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        // TODO: known_hosts による検証を実装する
        Ok(true)
    }
}

/// SSH セッションハンドル
pub struct SshSession {
    handle: Handle<AcceptAllVerifier>,
}

impl SshSession {
    /// SSH サーバーに接続する
    pub async fn connect(config: &SshConfig) -> Result<Self> {
        let ssh_config = Arc::new(client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(60)),
            keepalive_interval: Some(std::time::Duration::from_secs(30)),
            keepalive_max: 3,
            ..Default::default()
        });

        let addr = (config.host.as_str(), config.port);
        let handle = client::connect(ssh_config, addr, AcceptAllVerifier).await?;
        Ok(Self { handle })
    }

    /// 認証を実行する
    pub async fn authenticate(&mut self, config: &SshConfig) -> Result<()> {
        let username = config.username.clone();
        let authenticated = match &config.auth {
            SshAuth::Password(pw) => {
                self.handle
                    .authenticate_password(username, pw.as_str())
                    .await?
            }
            SshAuth::PrivateKey { key_path, passphrase } => {
                let key_pair = if let Some(pp) = passphrase {
                    load_secret_key(key_path, Some(pp.as_str()))?
                } else {
                    load_secret_key(key_path, None)?
                };
                let best_hash = self.handle.best_supported_rsa_hash().await?.flatten();
                let key_with_hash =
                    PrivateKeyWithHashAlg::new(Arc::new(key_pair), best_hash);
                self.handle
                    .authenticate_publickey(username, key_with_hash)
                    .await?
            }
            SshAuth::Agent => {
                bail!("SSH エージェント認証は未実装です");
            }
        };

        if !authenticated.success() {
            bail!("SSH 認証に失敗しました: ユーザー名またはパスワードが正しくありません");
        }
        Ok(())
    }

    /// PTY チャネルを開いて I/O ループを起動する
    ///
    /// `output_tx`: サーバーからのデータ（PTY 出力）を送信するチャネル
    /// `input_rx`: クライアントからのデータ（キー入力）を受信するチャネル
    /// `cols`, `rows`: 初期端末サイズ
    pub async fn open_shell(
        self,
        cols: u16,
        rows: u16,
        output_tx: mpsc::Sender<Vec<u8>>,
        mut input_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<()> {
        let mut channel = self.handle.channel_open_session().await?;

        channel
            .request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
            .await?;

        channel.request_shell(false).await?;

        // I/O ループを起動する
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // SSH チャネルからの出力を受信する
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data }) => {
                                if output_tx.send(data.to_vec()).await.is_err() {
                                    break;
                                }
                            }
                            Some(ChannelMsg::ExitStatus { .. }) | None => break,
                            _ => {}
                        }
                    }
                    // クライアントからの入力を SSH チャネルに送信する
                    Some(data) = input_rx.recv() => {
                        if channel.data(data.as_ref()).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(())
    }
}
