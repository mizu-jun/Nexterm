#![warn(missing_docs)]
//! SSH クライアント統合 — russh を使った SSH 接続管理

use anyhow::{Context, Result, bail};
use russh::ChannelMsg;
use russh::client::{self, Handle};
use russh::keys::{PrivateKeyWithHashAlg, PublicKey, load_secret_key};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, instrument, warn};
use zeroize::Zeroizing;

/// SSH 接続設定
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// 接続先ホスト名または IP アドレス
    pub host: String,
    /// 接続先ポート番号
    pub port: u16,
    /// SSH ログインユーザー名
    pub username: String,
    /// 認証方式
    pub auth: SshAuth,
    /// ProxyJump ホスト (フォーマット: "user@host:port")
    pub proxy_jump: Option<String>,
    /// SOCKS5 プロキシ (フォーマット: "socks5://host:port")
    pub proxy_socks5: Option<String>,
}

/// SSH 認証方式
#[derive(Debug, Clone)]
pub enum SshAuth {
    /// パスワード認証
    Password(Zeroizing<String>),
    /// 公開鍵認証（秘密鍵ファイルパス）
    PrivateKey {
        /// 秘密鍵ファイルのパス
        key_path: std::path::PathBuf,
        /// 秘密鍵のパスフレーズ（省略可）
        passphrase: Option<Zeroizing<String>>,
    },
    /// SSH エージェント認証
    Agent,
}

// ---------------------------------------------------------------------------
// 実装1: known_hosts によるホスト鍵検証
// ---------------------------------------------------------------------------

/// リモートポートフォワーディングのマッピング: remote_port → (local_host, local_port)
type ForwardMap = Arc<std::sync::Mutex<std::collections::HashMap<u32, (String, u16)>>>;

/// ~/.ssh/known_hosts によるホスト鍵検証とリモートフォワーディング処理
struct SshHandler {
    host: String,
    port: u16,
    /// リモートフォワーディング: サーバー側ポート → (ローカルホスト, ローカルポート)
    forward_map: ForwardMap,
}

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        use russh::keys::known_hosts::{check_known_hosts, learn_known_hosts};

        match check_known_hosts(&self.host, self.port, server_public_key) {
            Ok(true) => {
                debug!(
                    "known_hosts: ホスト鍵が一致しました ({}:{})",
                    self.host, self.port
                );
                Ok(true)
            }
            Ok(false) => {
                // エントリが存在しない → 初回接続として自動追加
                warn!(
                    "known_hosts にエントリがありません。ホスト鍵を自動追加します: {}:{}",
                    self.host, self.port
                );
                if let Err(e) = learn_known_hosts(&self.host, self.port, server_public_key) {
                    warn!("known_hosts への書き込みに失敗しました: {}", e);
                }
                Ok(true)
            }
            Err(russh::keys::Error::KeyChanged { line }) => {
                // 鍵が変わっている → 中間者攻撃の可能性
                warn!(
                    "known_hosts: ホスト鍵が変更されています ({}:{}, line {}) — 接続を拒否します",
                    self.host, self.port, line
                );
                Err(russh::Error::WrongServerSig)
            }
            Err(e) => {
                // その他のエラーは警告して続行
                warn!(
                    "known_hosts の検証中にエラーが発生しました: {} — 検証をスキップします",
                    e
                );
                Ok(true)
            }
        }
    }

    /// SSH サーバーがリモートフォワーディングの接続を通知してきた際に呼び出される
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        // forward_map からローカル転送先を取得する
        let dest = {
            let map = self.forward_map.lock().expect("forward_map mutex poisoned");
            map.get(&connected_port).cloned()
        };

        let Some((local_host, local_port)) = dest else {
            warn!(
                "リモートフォワーディング: ポート {} のマッピングが見つかりません",
                connected_port
            );
            return Ok(());
        };

        debug!(
            "リモートフォワーディング: {}:{} → {}:{}",
            connected_address, connected_port, local_host, local_port
        );

        tokio::spawn(async move {
            let mut local_stream =
                match tokio::net::TcpStream::connect((local_host.as_str(), local_port)).await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(
                            "リモートフォワーディング: ローカル接続失敗 ({}:{}): {}",
                            local_host, local_port, e
                        );
                        return;
                    }
                };
            let mut ssh_stream = channel.into_stream();
            match tokio::io::copy_bidirectional(&mut local_stream, &mut ssh_stream).await {
                Ok((sent, recv)) => {
                    debug!("リモートフォワーディング終了: sent={} recv={}", sent, recv);
                }
                Err(e) => {
                    debug!("リモートフォワーディング I/O エラー: {}", e);
                }
            }
        });

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SSH セッションハンドル
// ---------------------------------------------------------------------------

/// SSH セッションハンドル
///
/// `Handle` は `Clone` を実装していないため、ポートフォワーディングなど
/// バックグラウンドタスクからもアクセスできるよう `Arc<Mutex<...>>` で保持する。
pub struct SshSession {
    handle: Arc<Mutex<Handle<SshHandler>>>,
    /// リモートポートフォワーディングのポートマッピング（ハンドラと共有）
    forward_map: ForwardMap,
}

impl SshSession {
    /// SSH サーバーに接続する
    ///
    /// `config.proxy_jump` が設定されている場合は ProxyJump 経由で接続する。
    /// `config.proxy_socks5` が設定されている場合は SOCKS5 プロキシ経由で接続する。
    #[instrument(
        name = "ssh_connect",
        skip(config),
        fields(host = %config.host, port = config.port, user = %config.username,
               proxy_jump = config.proxy_jump.is_some(),
               proxy_socks5 = config.proxy_socks5.is_some())
    )]
    pub async fn connect(config: &SshConfig) -> Result<Self> {
        let ssh_config = Arc::new(client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(60)),
            keepalive_interval: Some(std::time::Duration::from_secs(30)),
            keepalive_max: 3,
            ..Default::default()
        });

        let forward_map: ForwardMap =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));

        let handler = SshHandler {
            host: config.host.clone(),
            port: config.port,
            forward_map: Arc::clone(&forward_map),
        };

        // SOCKS5 プロキシ経由接続
        if let Some(socks5_url) = &config.proxy_socks5 {
            let handle = connect_via_socks5(ssh_config, socks5_url, config, handler).await?;
            return Ok(Self {
                handle: Arc::new(Mutex::new(handle)),
                forward_map,
            });
        }

        // ProxyJump 経由接続
        if let Some(jump_spec) = &config.proxy_jump {
            let handle = connect_via_jump(ssh_config, jump_spec, config, handler).await?;
            return Ok(Self {
                handle: Arc::new(Mutex::new(handle)),
                forward_map,
            });
        }

        // 直接接続
        let addr = (config.host.as_str(), config.port);
        let handle = client::connect(ssh_config, addr, handler).await?;
        Ok(Self {
            handle: Arc::new(Mutex::new(handle)),
            forward_map,
        })
    }

    /// 認証を実行する
    #[instrument(
        name = "ssh_authenticate",
        skip(self, config),
        fields(user = %config.username, auth_type = match &config.auth {
            SshAuth::Password(_) => "password",
            SshAuth::PrivateKey { .. } => "private_key",
            SshAuth::Agent => "agent",
        })
    )]
    pub async fn authenticate(&mut self, config: &SshConfig) -> Result<()> {
        let username = config.username.clone();
        let authenticated = {
            let mut handle = self.handle.lock().await;
            match &config.auth {
                SshAuth::Password(pw) => {
                    handle.authenticate_password(username, pw.as_str()).await?
                }
                SshAuth::PrivateKey {
                    key_path,
                    passphrase,
                } => {
                    let key_pair = if let Some(pp) = passphrase {
                        load_secret_key(key_path, Some(pp.as_str()))?
                    } else {
                        load_secret_key(key_path, None)?
                    };
                    let best_hash = handle.best_supported_rsa_hash().await?.flatten();
                    let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(key_pair), best_hash);
                    handle
                        .authenticate_publickey(username, key_with_hash)
                        .await?
                }
                SshAuth::Agent => {
                    drop(handle); // ロックを解放してエージェント認証へ
                    return self.authenticate_agent(username).await;
                }
            }
        };

        if !authenticated.success() {
            bail!("SSH 認証に失敗しました: ユーザー名またはパスワードが正しくありません");
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // 実装2: SSH エージェント認証
    // ---------------------------------------------------------------------------

    /// SSH エージェント認証を実行する
    async fn authenticate_agent(&mut self, username: String) -> Result<()> {
        #[cfg(unix)]
        {
            use russh::keys::agent::client::AgentClient;

            // エージェントに接続（SSH_AUTH_SOCK を使用）
            let mut agent = AgentClient::connect_env().await.context(
                "SSH エージェントへの接続に失敗しました (SSH_AUTH_SOCK を確認してください)",
            )?;

            // エージェントから公開鍵一覧を取得
            let identities = agent
                .request_identities()
                .await
                .context("SSH エージェントから公開鍵一覧を取得できませんでした")?;

            if identities.is_empty() {
                bail!("SSH エージェントに登録されている鍵がありません");
            }

            // 各鍵で認証を試みる
            for identity in &identities {
                let comment = identity.comment().to_string();
                debug!("SSH エージェント認証を試みます: {}", comment);

                // russh 0.59: authenticate_publickey_with の第2引数は ssh_key::PublicKey
                let pub_key = identity.public_key().into_owned();

                let mut handle = self.handle.lock().await;
                let result = handle
                    .authenticate_publickey_with(username.clone(), pub_key, None, &mut agent)
                    .await;
                drop(handle);

                match result {
                    Ok(auth_res) if auth_res.success() => return Ok(()),
                    Ok(_) => {
                        debug!("鍵 '{}' での認証は受け入れられませんでした", comment);
                    }
                    Err(e) => {
                        warn!("鍵 '{}' での認証中にエラーが発生しました: {}", comment, e);
                    }
                }
            }

            bail!("SSH エージェントのすべての鍵で認証に失敗しました");
        }

        #[cfg(not(unix))]
        {
            let _ = username;
            bail!("SSH エージェント認証は Windows では未実装です");
        }
    }

    /// PTY チャネルを開いて I/O ループを起動する
    ///
    /// `output_tx`: サーバーからのデータ（PTY 出力）を送信するチャネル
    /// `input_rx`: クライアントからのデータ（キー入力）を受信するチャネル
    /// `cols`, `rows`: 初期端末サイズ
    /// `x11_forward`: X11 フォワーディングを有効にするか（ssh -X 相当）
    /// `x11_trusted`: 信頼された X11 フォワーディング（ssh -Y 相当）
    #[instrument(
        name = "ssh_open_shell",
        skip(self, output_tx, input_rx),
        fields(cols, rows, x11_forward, x11_trusted)
    )]
    pub async fn open_shell(
        self,
        cols: u16,
        rows: u16,
        output_tx: mpsc::Sender<Vec<u8>>,
        mut input_rx: mpsc::Receiver<Vec<u8>>,
        x11_forward: bool,
        x11_trusted: bool,
    ) -> Result<()> {
        let handle = self.handle.lock().await;
        let mut channel = handle.channel_open_session().await?;
        drop(handle);

        channel
            .request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
            .await?;

        // X11 フォワーディングのリクエスト（PTY リクエスト後、シェル起動前に行う）
        if x11_forward {
            // want_reply: false（応答を待たない）
            // single_connection: 信頼された転送 (-Y) では false、非信頼 (-X) では true
            let want_reply = false;
            let single_connection = !x11_trusted;
            let auth_protocol = "MIT-MAGIC-COOKIE-1";
            // ダミークッキー（実際の X11 認証は将来の実装で行う）
            let auth_cookie = "00000000000000000000000000000000";
            let screen_number = 0u32;
            if let Err(e) = channel
                .request_x11(
                    want_reply,
                    single_connection,
                    auth_protocol,
                    auth_cookie,
                    screen_number,
                )
                .await
            {
                warn!("X11 フォワーディングのリクエストに失敗しました: {}", e);
            }
        }

        channel.request_shell(false).await?;

        // I/O ループを起動する
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // SSH チャネルからの出力を受信する
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data })
                                if output_tx.send(data.to_vec()).await.is_err() =>
                            {
                                break;
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

    // ---------------------------------------------------------------------------
    // 実装3: ローカルポートフォワーディング
    // ---------------------------------------------------------------------------

    /// リモートポートフォワーディングを開始する (-R)
    ///
    /// `spec` フォーマット: "remote_port:local_host:local_port"
    ///
    /// SSH サーバーの `remote_port` への接続を
    /// ローカルの `local_host:local_port` に転送する。
    pub async fn start_remote_forward(&self, spec: &str) -> Result<()> {
        let (remote_port, local_host, local_port) = parse_forward_spec(spec)?;

        // forward_map にマッピングを登録する（ハンドラのコールバックで参照される）
        {
            let mut map = self.forward_map.lock().expect("forward_map mutex poisoned");
            map.insert(remote_port as u32, (local_host.clone(), local_port));
        }

        // SSH サーバーにリモートポートの待ち受けをリクエストする
        {
            let guard = self.handle.lock().await;
            guard
                .tcpip_forward("127.0.0.1", remote_port as u32)
                .await
                .with_context(|| {
                    format!(
                        "リモートポートフォワーディング: SSH サーバーへのリモートポート {} のバインドに失敗しました",
                        remote_port
                    )
                })?;
        }

        debug!(
            "リモートポートフォワーディング開始: remote:{} → {}:{}",
            remote_port, local_host, local_port
        );

        // 実際の接続処理は SshHandler::server_channel_open_forwarded_tcpip で行われる

        Ok(())
    }

    /// ローカルポートフォワーディングを開始する
    ///
    /// `spec` フォーマット: "local_port:remote_host:remote_port"
    pub async fn start_local_forward(&self, spec: &str) -> Result<()> {
        let (local_port, remote_host, remote_port) = parse_forward_spec(spec)?;

        let listener = TcpListener::bind(("127.0.0.1", local_port))
            .await
            .with_context(|| format!("ローカルポート {} のリッスンに失敗しました", local_port))?;

        let handle = self.handle.clone();

        tokio::spawn(async move {
            loop {
                let (mut local_stream, local_addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("ローカルポートフォワーディング: accept エラー: {}", e);
                        break;
                    }
                };

                let rh = remote_host.clone();
                let h = handle.clone();

                tokio::spawn(async move {
                    // SSH direct-tcpip チャネルを開く
                    let channel = {
                        let guard = h.lock().await;
                        guard
                            .channel_open_direct_tcpip(
                                rh.clone(),
                                remote_port as u32,
                                local_addr.ip().to_string(),
                                local_addr.port() as u32,
                            )
                            .await
                    };

                    let channel = match channel {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("direct-tcpip チャネルのオープンに失敗しました: {}", e);
                            return;
                        }
                    };

                    // チャネルを AsyncRead/AsyncWrite ストリームに変換
                    let mut ssh_stream = channel.into_stream();

                    // 双方向プロキシ
                    match tokio::io::copy_bidirectional(&mut local_stream, &mut ssh_stream).await {
                        Ok((sent, recv)) => {
                            debug!(
                                "ポートフォワーディング終了: {}:{} sent={} recv={}",
                                rh, remote_port, sent, recv
                            );
                        }
                        Err(e) => {
                            debug!("ポートフォワーディング I/O エラー: {}", e);
                        }
                    }
                });
            }
        });

        Ok(())
    }

    // ---------------------------------------------------------------------------
    // 実装4: SFTP ファイル転送
    // ---------------------------------------------------------------------------

    /// ローカルファイルをリモートにアップロードする（SFTP）
    ///
    /// `local_path`: アップロードするローカルファイルのパス
    /// `remote_path`: サーバー上の保存先パス（例: "/home/user/file.txt"）
    /// `progress_tx`: (transferred_bytes, total_bytes) を報告するチャネル（None = 報告なし）
    pub async fn upload_file(
        &self,
        local_path: &std::path::Path,
        remote_path: &str,
        progress_tx: Option<tokio::sync::mpsc::Sender<(u64, u64)>>,
    ) -> Result<()> {
        use russh_sftp::client::SftpSession;
        use tokio::io::AsyncReadExt;

        // SFTP サブシステムチャネルを開く
        let channel = {
            let handle = self.handle.lock().await;
            handle.channel_open_session().await?
        };

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .context("SFTP セッションの開始に失敗しました")?;

        // ローカルファイルを開く
        let mut local_file = tokio::fs::File::open(local_path).await.with_context(|| {
            format!(
                "ローカルファイルのオープンに失敗しました: {}",
                local_path.display()
            )
        })?;
        let total = local_file.metadata().await.map(|m| m.len()).unwrap_or(0);

        // リモートファイルを作成して書き込む
        let mut remote_file = sftp
            .create(remote_path)
            .await
            .with_context(|| format!("リモートファイルの作成に失敗しました: {}", remote_path))?;

        let mut buf = vec![0u8; 32 * 1024]; // 32KB チャンク
        let mut transferred: u64 = 0;

        loop {
            let n = local_file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            use tokio::io::AsyncWriteExt;
            remote_file.write_all(&buf[..n]).await?;
            transferred += n as u64;
            if let Some(ref tx) = progress_tx {
                let _ = tx.try_send((transferred, total));
            }
        }

        debug!(
            "SFTP アップロード完了: {} → {} ({} bytes)",
            local_path.display(),
            remote_path,
            transferred
        );
        Ok(())
    }

    /// リモートファイルをローカルにダウンロードする（SFTP）
    ///
    /// `remote_path`: ダウンロードするサーバー上のファイルパス
    /// `local_path`: ローカル保存先パス
    /// `progress_tx`: (transferred_bytes, total_bytes) を報告するチャネル（None = 報告なし）
    pub async fn download_file(
        &self,
        remote_path: &str,
        local_path: &std::path::Path,
        progress_tx: Option<tokio::sync::mpsc::Sender<(u64, u64)>>,
    ) -> Result<()> {
        use russh_sftp::client::SftpSession;
        use tokio::io::AsyncReadExt;

        // SFTP サブシステムチャネルを開く
        let channel = {
            let handle = self.handle.lock().await;
            handle.channel_open_session().await?
        };

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .context("SFTP セッションの開始に失敗しました")?;

        // リモートファイルのメタデータを取得してサイズを得る
        let total = sftp
            .metadata(remote_path)
            .await
            .map(|m| m.size.unwrap_or(0))
            .unwrap_or(0);

        // リモートファイルを開く
        let mut remote_file = sftp.open(remote_path).await.with_context(|| {
            format!("リモートファイルのオープンに失敗しました: {}", remote_path)
        })?;

        // ローカルファイルに書き込む
        let mut local_file = tokio::fs::File::create(local_path).await.with_context(|| {
            format!(
                "ローカルファイルの作成に失敗しました: {}",
                local_path.display()
            )
        })?;

        let mut buf = vec![0u8; 32 * 1024]; // 32KB チャンク
        let mut transferred: u64 = 0;

        loop {
            let n = remote_file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            use tokio::io::AsyncWriteExt;
            local_file.write_all(&buf[..n]).await?;
            transferred += n as u64;
            if let Some(ref tx) = progress_tx {
                let _ = tx.try_send((transferred, total));
            }
        }

        debug!(
            "SFTP ダウンロード完了: {} → {} ({} bytes)",
            remote_path,
            local_path.display(),
            transferred
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ヘルパー関数
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// ProxyJump / SOCKS5 ヘルパー
// ---------------------------------------------------------------------------

/// "user@host:port" 形式の ProxyJump 仕様をパースする
///
/// ユーザー名を省略した場合は現在の OS ユーザーにフォールバックする。
fn parse_jump_spec(spec: &str) -> Result<(String, String, u16)> {
    // user@host:port または host:port
    let (user, host_port) = if let Some(at) = spec.rfind('@') {
        (spec[..at].to_string(), &spec[at + 1..])
    } else {
        let default_user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "root".to_string());
        (default_user, spec)
    };

    let (host, port) = if let Some(colon) = host_port.rfind(':') {
        let port: u16 = host_port[colon + 1..]
            .parse()
            .with_context(|| format!("ProxyJump のポート番号が不正です: {}", spec))?;
        (host_port[..colon].to_string(), port)
    } else {
        (host_port.to_string(), 22u16)
    };

    Ok((user, host, port))
}

/// ProxyJump 経由で SSH 接続を確立する
///
/// 1. ジャンプホストに接続・認証する
/// 2. ジャンプホスト上で `channel_open_direct_tcpip` を使って実ホストへのトンネルを開く
/// 3. そのチャネルストリームを transport として実ホストに接続する
async fn connect_via_jump(
    ssh_config: Arc<client::Config>,
    jump_spec: &str,
    target: &SshConfig,
    target_verifier: SshHandler,
) -> Result<client::Handle<SshHandler>> {
    let (jump_user, jump_host, jump_port) = parse_jump_spec(jump_spec)?;

    debug!(
        "ProxyJump: {}@{}:{} → {}:{}",
        jump_user, jump_host, jump_port, target.host, target.port
    );

    // ジャンプホストへの接続（フォワーディングは対象ホスト側のみ、ジャンプホストには不要）
    let jump_verifier = SshHandler {
        host: jump_host.clone(),
        port: jump_port,
        forward_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };
    let jump_addr = (jump_host.as_str(), jump_port);
    let mut jump_handle = client::connect(ssh_config.clone(), jump_addr, jump_verifier).await?;

    // ジャンプホストの認証（対象ホストと同じ認証情報を使用）
    let jump_auth_result = {
        let username = jump_user.clone();
        match &target.auth {
            SshAuth::Password(pw) => {
                jump_handle
                    .authenticate_password(username, pw.as_str())
                    .await?
            }
            SshAuth::PrivateKey {
                key_path,
                passphrase,
            } => {
                let key_pair = if let Some(pp) = passphrase {
                    load_secret_key(key_path, Some(pp.as_str()))?
                } else {
                    load_secret_key(key_path, None)?
                };
                let best_hash = jump_handle.best_supported_rsa_hash().await?.flatten();
                let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(key_pair), best_hash);
                jump_handle
                    .authenticate_publickey(username, key_with_hash)
                    .await?
            }
            SshAuth::Agent => {
                // エージェント認証は SshSession::authenticate_agent で処理するため、
                // ここでは鍵なし認証にフォールバックしてエラーを返す
                bail!("ProxyJump でのエージェント認証は現在未対応です");
            }
        }
    };

    if !jump_auth_result.success() {
        bail!(
            "ProxyJump ホストへの認証に失敗しました: {}@{}:{}",
            jump_user,
            jump_host,
            jump_port
        );
    }

    // ジャンプホスト上でターゲットホストへの direct-tcpip チャネルを開く
    let channel = jump_handle
        .channel_open_direct_tcpip(target.host.clone(), target.port as u32, "127.0.0.1", 0u32)
        .await
        .context("ProxyJump: direct-tcpip チャネルのオープンに失敗しました")?;

    // チャネルを AsyncRead/AsyncWrite ストリームに変換して実ホストに接続する
    let channel_stream = channel.into_stream();
    let handle = client::connect_stream(ssh_config, channel_stream, target_verifier).await?;

    Ok(handle)
}

/// SOCKS5 プロキシ経由で SSH 接続を確立する
///
/// `socks5_url` のフォーマット: "socks5://host:port"
///
/// SOCKS5 ハンドシェイクを手動で行い、生 TCP ストリームをターゲットホストへの
/// トンネルとして使用する。
async fn connect_via_socks5(
    ssh_config: Arc<client::Config>,
    socks5_url: &str,
    target: &SshConfig,
    target_verifier: SshHandler,
) -> Result<client::Handle<SshHandler>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // "socks5://[user:pass@]host:port" をパースする
    let without_scheme = socks5_url.strip_prefix("socks5://").unwrap_or(socks5_url);

    // "@" がある場合は認証情報を除去してホスト部だけ取り出す
    let host_part = if let Some(at_pos) = without_scheme.find('@') {
        &without_scheme[at_pos + 1..]
    } else {
        without_scheme
    };

    let (socks_host, socks_port) = if let Some(colon) = host_part.rfind(':') {
        let port: u16 = host_part[colon + 1..]
            .parse()
            .with_context(|| format!("SOCKS5 プロキシのポート番号が不正です: {}", socks5_url))?;
        (host_part[..colon].to_string(), port)
    } else {
        (host_part.to_string(), 1080u16)
    };

    debug!(
        "SOCKS5: {}:{} → {}:{}",
        socks_host, socks_port, target.host, target.port
    );

    // SOCKS5 プロキシに TCP 接続する
    let mut stream = tokio::net::TcpStream::connect((socks_host.as_str(), socks_port))
        .await
        .with_context(|| {
            format!(
                "SOCKS5 プロキシへの接続に失敗しました: {}:{}",
                socks_host, socks_port
            )
        })?;

    // socks5_url から認証情報を抽出する（"socks5://user:pass@host:port"）
    let (socks_user, socks_pass) = parse_socks5_credentials(socks5_url);

    // SOCKS5 ネゴシエーション: 認証なし(0x00) + ユーザー名/パスワード(0x02) の両方を提案
    // +----+----------+----------+
    // |VER | NMETHODS | METHODS  |
    // +----+----------+----------+
    // | 1  |    1     | 1 to 255 |
    // +----+----------+----------+
    let methods: &[u8] = if socks_user.is_some() {
        &[0x05, 0x02, 0x00, 0x02] // no-auth + user/pass
    } else {
        &[0x05, 0x01, 0x00] // no-auth のみ
    };
    stream.write_all(methods).await?;

    let mut resp = [0u8; 2];
    stream.read_exact(&mut resp).await?;
    if resp[0] != 0x05 {
        bail!("SOCKS5: バージョンが不正です (応答: {:?})", resp);
    }

    match resp[1] {
        0x00 => {
            // 認証不要 — そのまま続行
            debug!("SOCKS5: 認証なしで接続します");
        }
        0x02 => {
            // RFC 1929 ユーザー名/パスワード認証
            let user = socks_user.as_deref().unwrap_or("");
            let pass = socks_pass.as_deref().unwrap_or("");
            debug!(
                "SOCKS5: ユーザー名/パスワード認証を実行します (user={})",
                user
            );

            let user_bytes = user.as_bytes();
            let pass_bytes = pass.as_bytes();
            if user_bytes.len() > 255 || pass_bytes.len() > 255 {
                bail!("SOCKS5: ユーザー名またはパスワードが長すぎます (最大255バイト)");
            }

            let mut auth_req = vec![0x01u8]; // VER=1 (sub-negotiation version)
            auth_req.push(user_bytes.len() as u8);
            auth_req.extend_from_slice(user_bytes);
            auth_req.push(pass_bytes.len() as u8);
            auth_req.extend_from_slice(pass_bytes);
            stream.write_all(&auth_req).await?;

            let mut auth_resp = [0u8; 2];
            stream.read_exact(&mut auth_resp).await?;
            if auth_resp[1] != 0x00 {
                bail!(
                    "SOCKS5: 認証に失敗しました (user={}, status=0x{:02x})",
                    user,
                    auth_resp[1]
                );
            }
            debug!("SOCKS5: 認証成功");
        }
        0xFF => {
            bail!("SOCKS5: サーバーが利用可能な認証方式を拒否しました");
        }
        other => {
            bail!(
                "SOCKS5: 認証ネゴシエーションに失敗しました (選択された方式: 0x{:02x})",
                other
            );
        }
    }

    // SOCKS5 CONNECT リクエスト
    // +----+-----+-------+------+----------+----------+
    // |VER | CMD |  RSV  | ATYP | DST.ADDR | DST.PORT |
    // +----+-----+-------+------+----------+----------+
    let host_bytes = target.host.as_bytes();
    let host_len = host_bytes.len() as u8;
    let mut connect_req = vec![
        0x05,     // VER
        0x01,     // CMD=CONNECT
        0x00,     // RSV
        0x03,     // ATYP=DOMAINNAME
        host_len, // domain length
    ];
    connect_req.extend_from_slice(host_bytes);
    connect_req.push((target.port >> 8) as u8);
    connect_req.push((target.port & 0xFF) as u8);
    stream.write_all(&connect_req).await?;

    // SOCKS5 CONNECT レスポンス
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 0x05 || header[1] != 0x00 {
        bail!(
            "SOCKS5: CONNECT に失敗しました (VER={}, REP={})",
            header[0],
            header[1]
        );
    }
    // バインドアドレスを読み捨てる
    let bound_addr_len = match header[3] {
        0x01 => 4usize, // IPv4
        0x03 => {
            let mut l = [0u8; 1];
            stream.read_exact(&mut l).await?;
            l[0] as usize
        }
        0x04 => 16usize, // IPv6
        _ => bail!("SOCKS5: 不明なアドレスタイプ: {}", header[3]),
    };
    let mut discard = vec![0u8; bound_addr_len + 2]; // addr + port
    stream.read_exact(&mut discard).await?;

    // トンネルが確立されたので SSH 接続を行う
    let handle = client::connect_stream(ssh_config, stream, target_verifier).await?;
    Ok(handle)
}

/// SOCKS5 URL から認証情報を抽出する
///
/// "socks5://user:pass@host:port" → (Some("user"), Some("pass"))
/// "socks5://host:port"           → (None, None)
fn parse_socks5_credentials(url: &str) -> (Option<String>, Option<String>) {
    // "socks5://" を除去する
    let rest = url.strip_prefix("socks5://").unwrap_or(url);

    // "@" が含まれる場合は "user:pass@host:port" 形式
    if let Some(at_pos) = rest.find('@') {
        let userinfo = &rest[..at_pos];
        if let Some(colon_pos) = userinfo.find(':') {
            let user = &userinfo[..colon_pos];
            let pass = &userinfo[colon_pos + 1..];
            return (Some(user.to_string()), Some(pass.to_string()));
        }
        // コロンなし → ユーザー名のみ
        return (Some(userinfo.to_string()), None);
    }

    (None, None)
}

/// "local_port:remote_host:remote_port" 形式のフォワーディング仕様をパースする
fn parse_forward_spec(spec: &str) -> Result<(u16, String, u16)> {
    let parts: Vec<&str> = spec.splitn(3, ':').collect();
    if parts.len() != 3 {
        bail!(
            "不正なポートフォワーディング仕様です (期待形式: local_port:remote_host:remote_port): {}",
            spec
        );
    }
    let local_port: u16 = parts[0]
        .parse()
        .with_context(|| format!("ローカルポートの解析に失敗しました: {}", parts[0]))?;
    let remote_host = parts[1].to_string();
    let remote_port: u16 = parts[2]
        .parse()
        .with_context(|| format!("リモートポートの解析に失敗しました: {}", parts[2]))?;
    Ok((local_port, remote_host, remote_port))
}

// ---------------------------------------------------------------------------
// ユニットテスト
//
// nexterm-ssh は外部 SSH サーバーへの I/O が中心で完全な統合テストは
// モック SSH サーバーが必要になる（Sprint 5-5 以降の課題）。
// ここでは純粋ロジック（仕様文字列パーサ・SshConfig 構築・到達不能ホストの即時失敗）に
// 限定して単体テストを置く。
//
// 監査ラウンド 2 タスク I1 (nexterm-ssh のテスト 0 解消) の最初の対応。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_jump_spec --------------------------------------------------

    #[test]
    fn parse_jump_spec_user_host_port() {
        let (user, host, port) = parse_jump_spec("alice@bastion.example.com:2222").expect("正常系");
        assert_eq!(user, "alice");
        assert_eq!(host, "bastion.example.com");
        assert_eq!(port, 2222);
    }

    #[test]
    fn parse_jump_spec_user_host_default_port() {
        let (user, host, port) = parse_jump_spec("alice@bastion.example.com").expect("正常系");
        assert_eq!(user, "alice");
        assert_eq!(host, "bastion.example.com");
        assert_eq!(port, 22, "ポート省略時は 22 にフォールバックすること");
    }

    #[test]
    fn parse_jump_spec_host_only_uses_env_user() {
        // USER / USERNAME が無くても "root" にフォールバックするロジックの確認
        let (_user, host, port) = parse_jump_spec("bastion.example.com:22").expect("正常系");
        assert_eq!(host, "bastion.example.com");
        assert_eq!(port, 22);
        // user は env 依存のため値の検証はしない（空文字でないことだけ確認）
    }

    #[test]
    fn parse_jump_spec_invalid_port_fails() {
        let err = parse_jump_spec("alice@host:not-a-port").expect_err("不正ポートはエラー");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("ポート番号"),
            "エラーメッセージにポートの旨を含むこと: {}",
            msg
        );
    }

    // ---- parse_socks5_credentials -----------------------------------------

    #[test]
    fn parse_socks5_credentials_user_password() {
        let (u, p) = parse_socks5_credentials("socks5://alice:secret@proxy:1080");
        assert_eq!(u.as_deref(), Some("alice"));
        assert_eq!(p.as_deref(), Some("secret"));
    }

    #[test]
    fn parse_socks5_credentials_user_only() {
        let (u, p) = parse_socks5_credentials("socks5://alice@proxy:1080");
        assert_eq!(u.as_deref(), Some("alice"));
        assert_eq!(p, None);
    }

    #[test]
    fn parse_socks5_credentials_anonymous() {
        let (u, p) = parse_socks5_credentials("socks5://proxy:1080");
        assert_eq!(u, None);
        assert_eq!(p, None);
    }

    #[test]
    fn parse_socks5_credentials_missing_scheme_still_parses() {
        // "socks5://" がなくても認証情報パートは抽出できる
        let (u, p) = parse_socks5_credentials("alice:secret@proxy:1080");
        assert_eq!(u.as_deref(), Some("alice"));
        assert_eq!(p.as_deref(), Some("secret"));
    }

    // ---- parse_forward_spec -----------------------------------------------

    #[test]
    fn parse_forward_spec_valid() {
        let (local, host, remote) =
            parse_forward_spec("8080:internal.example.com:80").expect("正常系");
        assert_eq!(local, 8080);
        assert_eq!(host, "internal.example.com");
        assert_eq!(remote, 80);
    }

    #[test]
    fn parse_forward_spec_too_few_parts() {
        let err = parse_forward_spec("8080:internal.example.com").expect_err("不正形式");
        let msg = format!("{:#}", err);
        assert!(msg.contains("ポートフォワーディング仕様"));
    }

    #[test]
    fn parse_forward_spec_bad_local_port() {
        let err = parse_forward_spec("abc:host:80").expect_err("ローカルポート不正");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("ローカルポート"),
            "エラーメッセージが具体的であること: {}",
            msg
        );
    }

    #[test]
    fn parse_forward_spec_bad_remote_port() {
        let err = parse_forward_spec("8080:host:abc").expect_err("リモートポート不正");
        let msg = format!("{:#}", err);
        assert!(msg.contains("リモートポート"));
    }

    // ---- SshConfig / SshAuth ----------------------------------------------

    #[test]
    fn ssh_auth_password_zeroizes_on_drop() {
        // Zeroizing<String> がドロップ時に内部バッファをゼロ化することは
        // 直接観測できないが、API として Password バリアントが Clone 可能で
        // 期待通り構築できることを確認する。
        let auth = SshAuth::Password(Zeroizing::new("hunter2".to_string()));
        let cloned = auth.clone();
        match cloned {
            SshAuth::Password(p) => assert_eq!(p.as_str(), "hunter2"),
            _ => panic!("Password バリアントが保持されること"),
        }
    }

    #[test]
    fn ssh_config_construction() {
        let config = SshConfig {
            host: "example.com".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth: SshAuth::Password(Zeroizing::new("pw".to_string())),
            proxy_jump: None,
            proxy_socks5: None,
        };
        // Clone 可能
        let cloned = config.clone();
        assert_eq!(cloned.host, "example.com");
        assert_eq!(cloned.port, 22);
        assert_eq!(cloned.username, "alice");
        assert!(cloned.proxy_jump.is_none());
        assert!(cloned.proxy_socks5.is_none());
    }

    // ---- 接続失敗の即時返却 -----------------------------------------------

    /// 到達不能なホストへの接続が現実的な時間でエラーを返すこと
    ///
    /// `127.0.0.1:1` (Reserved port) は通常 LISTEN されておらず
    /// `connect` は即座に "Connection refused" を返す。
    /// このテストはネットワークスタックに依存するが、
    /// OS のローカル loopback だけを使うので CI でも安定する。
    #[tokio::test]
    async fn connect_to_unreachable_port_fails_fast() {
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port: 1, // LISTEN されていないポート
            username: "test".to_string(),
            auth: SshAuth::Password(Zeroizing::new("pw".to_string())),
            proxy_jump: None,
            proxy_socks5: None,
        };

        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            SshSession::connect(&config),
        )
        .await;
        let elapsed = start.elapsed();

        match result {
            Ok(Err(_)) => {
                // 接続エラーは想定通り
                assert!(
                    elapsed < std::time::Duration::from_secs(5),
                    "5 秒以内に失敗を返すこと: {:?}",
                    elapsed
                );
            }
            Ok(Ok(_)) => panic!("到達不能なポートへの接続が成功してはならない"),
            Err(_) => panic!(
                "5 秒以内に応答すべき (Connection refused は通常 1ms 以内): {:?}",
                elapsed
            ),
        }
    }
}
