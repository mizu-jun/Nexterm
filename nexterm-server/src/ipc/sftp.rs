//! SFTP アップロード・ダウンロードヘルパー

use nexterm_proto::ServerToClient;
use tokio::sync::mpsc;

/// HostConfig から SshConfig を構築してアップロードを実行する
pub(super) async fn run_sftp_upload(
    host: &nexterm_config::HostConfig,
    local_path: &str,
    remote_path: &str,
    tx: mpsc::Sender<ServerToClient>,
) -> anyhow::Result<()> {
    use nexterm_ssh::{SshAuth, SshConfig, SshSession};
    use std::path::PathBuf;
    use zeroize::Zeroizing;

    let auth = match host.auth_type.as_str() {
        "password" => SshAuth::Password(Zeroizing::new(String::new())),
        "key" => SshAuth::PrivateKey {
            key_path: PathBuf::from(host.key_path.clone().unwrap_or_else(|| {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_default();
                format!("{}/.ssh/id_rsa", home)
            })),
            passphrase: None,
        },
        _ => SshAuth::Agent,
    };

    let ssh_config = SshConfig {
        host: host.host.clone(),
        port: host.port,
        username: host.username.clone(),
        auth,
        proxy_jump: host.proxy_jump.clone(),
        proxy_socks5: None,
    };

    let mut session = SshSession::connect(&ssh_config).await?;
    session.authenticate(&ssh_config).await?;

    // 進捗チャネル
    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<(u64, u64)>(32);
    let tx2 = tx.clone();
    let path_display = local_path.to_string();
    tokio::spawn(async move {
        while let Some((transferred, total)) = prog_rx.recv().await {
            let _ = tx2
                .send(ServerToClient::SftpProgress {
                    path: path_display.clone(),
                    transferred,
                    total,
                })
                .await;
        }
    });

    session
        .upload_file(
            std::path::Path::new(local_path),
            remote_path,
            Some(prog_tx),
        )
        .await
}

/// HostConfig から SshConfig を構築してダウンロードを実行する
pub(super) async fn run_sftp_download(
    host: &nexterm_config::HostConfig,
    remote_path: &str,
    local_path: &str,
    tx: mpsc::Sender<ServerToClient>,
) -> anyhow::Result<()> {
    use nexterm_ssh::{SshAuth, SshConfig, SshSession};
    use std::path::PathBuf;
    use zeroize::Zeroizing;

    let auth = match host.auth_type.as_str() {
        "password" => SshAuth::Password(Zeroizing::new(String::new())),
        "key" => SshAuth::PrivateKey {
            key_path: PathBuf::from(host.key_path.clone().unwrap_or_else(|| {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_default();
                format!("{}/.ssh/id_rsa", home)
            })),
            passphrase: None,
        },
        _ => SshAuth::Agent,
    };

    let ssh_config = SshConfig {
        host: host.host.clone(),
        port: host.port,
        username: host.username.clone(),
        auth,
        proxy_jump: host.proxy_jump.clone(),
        proxy_socks5: None,
    };

    let mut session = SshSession::connect(&ssh_config).await?;
    session.authenticate(&ssh_config).await?;

    // 進捗チャネル
    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<(u64, u64)>(32);
    let tx2 = tx.clone();
    let path_display = remote_path.to_string();
    tokio::spawn(async move {
        while let Some((transferred, total)) = prog_rx.recv().await {
            let _ = tx2
                .send(ServerToClient::SftpProgress {
                    path: path_display.clone(),
                    transferred,
                    total,
                })
                .await;
        }
    });

    session
        .download_file(
            remote_path,
            std::path::Path::new(local_path),
            Some(prog_tx),
        )
        .await
}
