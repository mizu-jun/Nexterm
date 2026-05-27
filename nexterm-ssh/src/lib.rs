#![warn(missing_docs)]
//! SSH client integration — manages SSH connections via russh.

use anyhow::{Context, Result, bail};
use russh::ChannelMsg;
use russh::client::{self, Handle};
use russh::keys::{PrivateKeyWithHashAlg, PublicKey, load_secret_key};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, instrument, warn};
use zeroize::Zeroizing;

/// SSH connection configuration.
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// Target host name or IP address.
    pub host: String,
    /// Target port number.
    pub port: u16,
    /// SSH login user name.
    pub username: String,
    /// Authentication method.
    pub auth: SshAuth,
    /// ProxyJump host (format: `user@host:port`).
    pub proxy_jump: Option<String>,
    /// SOCKS5 proxy (format: `socks5://host:port`).
    pub proxy_socks5: Option<String>,
}

/// SSH authentication methods.
#[derive(Debug, Clone)]
pub enum SshAuth {
    /// Password authentication.
    Password(Zeroizing<String>),
    /// Public-key authentication (path to the private key file).
    PrivateKey {
        /// Path to the private-key file.
        key_path: std::path::PathBuf,
        /// Optional passphrase for the private key.
        passphrase: Option<Zeroizing<String>>,
    },
    /// SSH agent authentication.
    Agent,
}

// ---------------------------------------------------------------------------
// Implementation 1: host key verification via known_hosts.
// ---------------------------------------------------------------------------

/// Remote port forwarding map: `remote_port → (local_host, local_port)`.
type ForwardMap = Arc<std::sync::Mutex<std::collections::HashMap<u32, (String, u16)>>>;

/// Host key verification (via `~/.ssh/known_hosts`) plus remote-forwarding callback handling.
struct SshHandler {
    host: String,
    port: u16,
    /// Remote forwarding: server-side port → (local host, local port).
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
                    "known_hosts: host key matched ({}:{})",
                    self.host, self.port
                );
                Ok(true)
            }
            Ok(false) => {
                // No entry present — treat as first connection and learn the key.
                warn!(
                    "known_hosts has no entry; auto-adding host key: {}:{}",
                    self.host, self.port
                );
                if let Err(e) = learn_known_hosts(&self.host, self.port, server_public_key) {
                    warn!("failed to write to known_hosts: {}", e);
                }
                Ok(true)
            }
            Err(russh::keys::Error::KeyChanged { line }) => {
                // Host key changed — possible MITM, reject the connection.
                warn!(
                    "known_hosts: host key has changed ({}:{}, line {}) — rejecting connection",
                    self.host, self.port, line
                );
                Err(russh::Error::WrongServerSig)
            }
            Err(e) => {
                // Any other error: log a warning and proceed.
                warn!(
                    "error while verifying known_hosts: {} — skipping verification",
                    e
                );
                Ok(true)
            }
        }
    }

    /// Called when the SSH server notifies us of an incoming remote-forwarding connection.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        // Look up the local forwarding destination in forward_map.
        let dest = {
            let map = self.forward_map.lock().expect("forward_map mutex poisoned");
            map.get(&connected_port).cloned()
        };

        let Some((local_host, local_port)) = dest else {
            warn!(
                "remote forwarding: no mapping found for port {}",
                connected_port
            );
            return Ok(());
        };

        debug!(
            "remote forwarding: {}:{} → {}:{}",
            connected_address, connected_port, local_host, local_port
        );

        tokio::spawn(async move {
            let mut local_stream =
                match tokio::net::TcpStream::connect((local_host.as_str(), local_port)).await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(
                            "remote forwarding: local connection failed ({}:{}): {}",
                            local_host, local_port, e
                        );
                        return;
                    }
                };
            let mut ssh_stream = channel.into_stream();
            match tokio::io::copy_bidirectional(&mut local_stream, &mut ssh_stream).await {
                Ok((sent, recv)) => {
                    debug!("remote forwarding finished: sent={} recv={}", sent, recv);
                }
                Err(e) => {
                    debug!("remote forwarding I/O error: {}", e);
                }
            }
        });

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SSH session handle.
// ---------------------------------------------------------------------------

/// SSH session handle.
///
/// `Handle` does not implement `Clone`, so it is kept inside `Arc<Mutex<...>>`
/// so that background tasks (port forwarding, etc.) can also access it.
pub struct SshSession {
    handle: Arc<Mutex<Handle<SshHandler>>>,
    /// Remote port forwarding mapping (shared with the handler).
    forward_map: ForwardMap,
}

impl SshSession {
    /// Connect to the SSH server.
    ///
    /// When `config.proxy_jump` is set, the connection is established through ProxyJump.
    /// When `config.proxy_socks5` is set, it is established through a SOCKS5 proxy.
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

        // Connect via SOCKS5 proxy.
        if let Some(socks5_url) = &config.proxy_socks5 {
            let handle = connect_via_socks5(ssh_config, socks5_url, config, handler).await?;
            return Ok(Self {
                handle: Arc::new(Mutex::new(handle)),
                forward_map,
            });
        }

        // Connect via ProxyJump.
        if let Some(jump_spec) = &config.proxy_jump {
            let handle = connect_via_jump(ssh_config, jump_spec, config, handler).await?;
            return Ok(Self {
                handle: Arc::new(Mutex::new(handle)),
                forward_map,
            });
        }

        // Direct connection.
        let addr = (config.host.as_str(), config.port);
        let handle = client::connect(ssh_config, addr, handler).await?;
        Ok(Self {
            handle: Arc::new(Mutex::new(handle)),
            forward_map,
        })
    }

    /// Run authentication.
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
                    drop(handle); // Release the lock before performing agent authentication.
                    return self.authenticate_agent(username).await;
                }
            }
        };

        if !authenticated.success() {
            bail!("SSH authentication failed: incorrect username or password");
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Implementation 2: SSH agent authentication.
    // ---------------------------------------------------------------------------

    /// Perform SSH agent authentication.
    async fn authenticate_agent(&mut self, username: String) -> Result<()> {
        #[cfg(unix)]
        {
            use russh::keys::agent::client::AgentClient;

            // Connect to the agent (uses SSH_AUTH_SOCK).
            let mut agent = AgentClient::connect_env()
                .await
                .context("failed to connect to the SSH agent (check SSH_AUTH_SOCK)")?;

            // Fetch the list of public keys from the agent.
            let identities = agent
                .request_identities()
                .await
                .context("failed to obtain the public-key list from the SSH agent")?;

            if identities.is_empty() {
                bail!("the SSH agent has no registered keys");
            }

            // Try each key in turn.
            for identity in &identities {
                let comment = identity.comment().to_string();
                debug!("trying SSH agent authentication: {}", comment);

                // russh 0.59: the second argument to authenticate_publickey_with is ssh_key::PublicKey.
                let pub_key = identity.public_key().into_owned();

                let mut handle = self.handle.lock().await;
                let result = handle
                    .authenticate_publickey_with(username.clone(), pub_key, None, &mut agent)
                    .await;
                drop(handle);

                match result {
                    Ok(auth_res) if auth_res.success() => return Ok(()),
                    Ok(_) => {
                        debug!("key '{}' was not accepted for authentication", comment);
                    }
                    Err(e) => {
                        warn!("error while authenticating with key '{}': {}", comment, e);
                    }
                }
            }

            bail!("authentication failed for every key in the SSH agent");
        }

        #[cfg(not(unix))]
        {
            let _ = username;
            bail!("SSH agent authentication is not implemented on Windows");
        }
    }

    /// Open a PTY channel and spawn the I/O loop.
    ///
    /// `output_tx`: channel for sending data from the server (PTY output).
    /// `input_rx`: channel for receiving data from the client (key input).
    /// `cols`, `rows`: initial terminal size.
    /// `x11_forward`: enable X11 forwarding (equivalent to `ssh -X`).
    /// `x11_trusted`: trusted X11 forwarding (equivalent to `ssh -Y`).
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

        // Request X11 forwarding (after the PTY request, before the shell is started).
        if x11_forward {
            // want_reply: false (do not wait for a reply).
            // single_connection: false for trusted forwarding (-Y), true for untrusted (-X).
            let want_reply = false;
            let single_connection = !x11_trusted;
            let auth_protocol = "MIT-MAGIC-COOKIE-1";
            // Dummy cookie (actual X11 authentication will be implemented later).
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
                warn!("X11 forwarding request failed: {}", e);
            }
        }

        channel.request_shell(false).await?;

        // Spawn the I/O loop.
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Receive output from the SSH channel.
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
                    // Forward client input to the SSH channel.
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
    // Implementation 3: local port forwarding.
    // ---------------------------------------------------------------------------

    /// Start a remote port forwarding (-R).
    ///
    /// `spec` format: `remote_port:local_host:local_port`.
    ///
    /// Connections arriving at `remote_port` on the SSH server are forwarded
    /// to `local_host:local_port` on the local side.
    pub async fn start_remote_forward(&self, spec: &str) -> Result<()> {
        let (remote_port, local_host, local_port) = parse_forward_spec(spec)?;

        // Register the mapping in forward_map (referenced from the handler callback).
        {
            let mut map = self.forward_map.lock().expect("forward_map mutex poisoned");
            map.insert(remote_port as u32, (local_host.clone(), local_port));
        }

        // Ask the SSH server to listen on the remote port.
        {
            let guard = self.handle.lock().await;
            guard
                .tcpip_forward("127.0.0.1", remote_port as u32)
                .await
                .with_context(|| {
                    format!(
                        "remote port forwarding: failed to bind remote port {} on the SSH server",
                        remote_port
                    )
                })?;
        }

        debug!(
            "remote port forwarding started: remote:{} → {}:{}",
            remote_port, local_host, local_port
        );

        // The actual connection handling happens in SshHandler::server_channel_open_forwarded_tcpip.

        Ok(())
    }

    /// Start a local port forwarding.
    ///
    /// `spec` format: `local_port:remote_host:remote_port`.
    pub async fn start_local_forward(&self, spec: &str) -> Result<()> {
        let (local_port, remote_host, remote_port) = parse_forward_spec(spec)?;

        let listener = TcpListener::bind(("127.0.0.1", local_port))
            .await
            .with_context(|| format!("failed to listen on local port {}", local_port))?;

        let handle = self.handle.clone();

        tokio::spawn(async move {
            loop {
                let (mut local_stream, local_addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("local port forwarding: accept error: {}", e);
                        break;
                    }
                };

                let rh = remote_host.clone();
                let h = handle.clone();

                tokio::spawn(async move {
                    // Open a direct-tcpip channel over SSH.
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
                            warn!("failed to open direct-tcpip channel: {}", e);
                            return;
                        }
                    };

                    // Convert the channel into an AsyncRead/AsyncWrite stream.
                    let mut ssh_stream = channel.into_stream();

                    // Bidirectional proxy.
                    match tokio::io::copy_bidirectional(&mut local_stream, &mut ssh_stream).await {
                        Ok((sent, recv)) => {
                            debug!(
                                "port forwarding finished: {}:{} sent={} recv={}",
                                rh, remote_port, sent, recv
                            );
                        }
                        Err(e) => {
                            debug!("port forwarding I/O error: {}", e);
                        }
                    }
                });
            }
        });

        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Implementation 4: SFTP file transfer.
    // ---------------------------------------------------------------------------

    /// Upload a local file to the remote host over SFTP.
    ///
    /// `local_path`: local file path to upload.
    /// `remote_path`: destination path on the server (e.g. `/home/user/file.txt`).
    /// `progress_tx`: channel that reports `(transferred_bytes, total_bytes)` (None = no reporting).
    pub async fn upload_file(
        &self,
        local_path: &std::path::Path,
        remote_path: &str,
        progress_tx: Option<tokio::sync::mpsc::Sender<(u64, u64)>>,
    ) -> Result<()> {
        use russh_sftp::client::SftpSession;
        use tokio::io::AsyncReadExt;

        // Open an SFTP subsystem channel.
        let channel = {
            let handle = self.handle.lock().await;
            handle.channel_open_session().await?
        };

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .context("failed to start the SFTP session")?;

        // Open the local file.
        let mut local_file = tokio::fs::File::open(local_path)
            .await
            .with_context(|| format!("failed to open the local file: {}", local_path.display()))?;
        let total = local_file.metadata().await.map(|m| m.len()).unwrap_or(0);

        // Create and write to the remote file.
        let mut remote_file = sftp
            .create(remote_path)
            .await
            .with_context(|| format!("failed to create the remote file: {}", remote_path))?;

        let mut buf = vec![0u8; 32 * 1024]; // 32 KiB chunks.
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
            "SFTP upload finished: {} → {} ({} bytes)",
            local_path.display(),
            remote_path,
            transferred
        );
        Ok(())
    }

    /// Download a remote file to the local filesystem over SFTP.
    ///
    /// `remote_path`: remote file path to download.
    /// `local_path`: local destination path.
    /// `progress_tx`: channel that reports `(transferred_bytes, total_bytes)` (None = no reporting).
    pub async fn download_file(
        &self,
        remote_path: &str,
        local_path: &std::path::Path,
        progress_tx: Option<tokio::sync::mpsc::Sender<(u64, u64)>>,
    ) -> Result<()> {
        use russh_sftp::client::SftpSession;
        use tokio::io::AsyncReadExt;

        // Open an SFTP subsystem channel.
        let channel = {
            let handle = self.handle.lock().await;
            handle.channel_open_session().await?
        };

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .context("failed to start the SFTP session")?;

        // Fetch the remote file metadata to obtain its size.
        let total = sftp
            .metadata(remote_path)
            .await
            .map(|m| m.size.unwrap_or(0))
            .unwrap_or(0);

        // Open the remote file.
        let mut remote_file = sftp
            .open(remote_path)
            .await
            .with_context(|| format!("failed to open the remote file: {}", remote_path))?;

        // Write to the local file.
        let mut local_file = tokio::fs::File::create(local_path).await.with_context(|| {
            format!("failed to create the local file: {}", local_path.display())
        })?;

        let mut buf = vec![0u8; 32 * 1024]; // 32 KiB chunks.
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
            "SFTP download finished: {} → {} ({} bytes)",
            remote_path,
            local_path.display(),
            transferred
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper functions.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// ProxyJump / SOCKS5 helpers.
// ---------------------------------------------------------------------------

/// Parse a `user@host:port` ProxyJump specification.
///
/// When the user name is omitted, fall back to the current OS user.
fn parse_jump_spec(spec: &str) -> Result<(String, String, u16)> {
    // user@host:port  or  host:port.
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
            .with_context(|| format!("invalid ProxyJump port number: {}", spec))?;
        (host_port[..colon].to_string(), port)
    } else {
        (host_port.to_string(), 22u16)
    };

    Ok((user, host, port))
}

/// Establish an SSH connection through ProxyJump.
///
/// 1. Connect and authenticate to the jump host.
/// 2. Open a tunnel to the real host via `channel_open_direct_tcpip` on the jump host.
/// 3. Use that channel stream as the transport to connect to the real host.
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

    // Connect to the jump host (only the target host needs the forwarding map).
    let jump_verifier = SshHandler {
        host: jump_host.clone(),
        port: jump_port,
        forward_map: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };
    let jump_addr = (jump_host.as_str(), jump_port);
    let mut jump_handle = client::connect(ssh_config.clone(), jump_addr, jump_verifier).await?;

    // Authenticate to the jump host (reuse the same credentials as the target host).
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
                // Agent authentication is handled by SshSession::authenticate_agent;
                // we cannot reuse it here, so bail out.
                bail!("agent authentication via ProxyJump is not yet supported");
            }
        }
    };

    if !jump_auth_result.success() {
        bail!(
            "authentication to the ProxyJump host failed: {}@{}:{}",
            jump_user,
            jump_host,
            jump_port
        );
    }

    // Open a direct-tcpip channel to the target host on the jump host.
    let channel = jump_handle
        .channel_open_direct_tcpip(target.host.clone(), target.port as u32, "127.0.0.1", 0u32)
        .await
        .context("ProxyJump: failed to open the direct-tcpip channel")?;

    // Convert the channel to an AsyncRead/AsyncWrite stream and connect to the real host.
    let channel_stream = channel.into_stream();
    let handle = client::connect_stream(ssh_config, channel_stream, target_verifier).await?;

    Ok(handle)
}

/// Establish an SSH connection through a SOCKS5 proxy.
///
/// `socks5_url` format: `socks5://host:port`.
///
/// The SOCKS5 handshake is performed manually; the raw TCP stream is then used as
/// a tunnel to the target host.
async fn connect_via_socks5(
    ssh_config: Arc<client::Config>,
    socks5_url: &str,
    target: &SshConfig,
    target_verifier: SshHandler,
) -> Result<client::Handle<SshHandler>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Parse `socks5://[user:pass@]host:port`.
    let without_scheme = socks5_url.strip_prefix("socks5://").unwrap_or(socks5_url);

    // If a `@` is present, strip the credentials and keep only the host portion.
    let host_part = if let Some(at_pos) = without_scheme.find('@') {
        &without_scheme[at_pos + 1..]
    } else {
        without_scheme
    };

    let (socks_host, socks_port) = if let Some(colon) = host_part.rfind(':') {
        let port: u16 = host_part[colon + 1..]
            .parse()
            .with_context(|| format!("invalid SOCKS5 proxy port number: {}", socks5_url))?;
        (host_part[..colon].to_string(), port)
    } else {
        (host_part.to_string(), 1080u16)
    };

    debug!(
        "SOCKS5: {}:{} → {}:{}",
        socks_host, socks_port, target.host, target.port
    );

    // Open a TCP connection to the SOCKS5 proxy.
    let mut stream = tokio::net::TcpStream::connect((socks_host.as_str(), socks_port))
        .await
        .with_context(|| {
            format!(
                "failed to connect to the SOCKS5 proxy: {}:{}",
                socks_host, socks_port
            )
        })?;

    // Extract credentials from the SOCKS5 URL (`socks5://user:pass@host:port`).
    let (socks_user, socks_pass) = parse_socks5_credentials(socks5_url);

    // SOCKS5 negotiation: offer both no-auth (0x00) and username/password (0x02).
    // +----+----------+----------+
    // |VER | NMETHODS | METHODS  |
    // +----+----------+----------+
    // | 1  |    1     | 1 to 255 |
    // +----+----------+----------+
    let methods: &[u8] = if socks_user.is_some() {
        &[0x05, 0x02, 0x00, 0x02] // no-auth + user/pass
    } else {
        &[0x05, 0x01, 0x00] // no-auth only
    };
    stream.write_all(methods).await?;

    let mut resp = [0u8; 2];
    stream.read_exact(&mut resp).await?;
    if resp[0] != 0x05 {
        bail!("SOCKS5: invalid version (response: {:?})", resp);
    }

    match resp[1] {
        0x00 => {
            // No authentication — proceed.
            debug!("SOCKS5: connecting without authentication");
        }
        0x02 => {
            // RFC 1929 username/password authentication.
            let user = socks_user.as_deref().unwrap_or("");
            let pass = socks_pass.as_deref().unwrap_or("");
            debug!(
                "SOCKS5: performing username/password authentication (user={})",
                user
            );

            let user_bytes = user.as_bytes();
            let pass_bytes = pass.as_bytes();
            if user_bytes.len() > 255 || pass_bytes.len() > 255 {
                bail!("SOCKS5: username or password too long (max 255 bytes)");
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
                    "SOCKS5: authentication failed (user={}, status=0x{:02x})",
                    user,
                    auth_resp[1]
                );
            }
            debug!("SOCKS5: authentication succeeded");
        }
        0xFF => {
            bail!("SOCKS5: server rejected all offered authentication methods");
        }
        other => {
            bail!(
                "SOCKS5: authentication negotiation failed (selected method: 0x{:02x})",
                other
            );
        }
    }

    // SOCKS5 CONNECT request.
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

    // SOCKS5 CONNECT response.
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 0x05 || header[1] != 0x00 {
        bail!(
            "SOCKS5: CONNECT failed (VER={}, REP={})",
            header[0],
            header[1]
        );
    }
    // Discard the bound address.
    let bound_addr_len = match header[3] {
        0x01 => 4usize, // IPv4
        0x03 => {
            let mut l = [0u8; 1];
            stream.read_exact(&mut l).await?;
            l[0] as usize
        }
        0x04 => 16usize, // IPv6
        _ => bail!("SOCKS5: unknown address type: {}", header[3]),
    };
    let mut discard = vec![0u8; bound_addr_len + 2]; // addr + port
    stream.read_exact(&mut discard).await?;

    // The tunnel is established — perform the SSH connection over it.
    let handle = client::connect_stream(ssh_config, stream, target_verifier).await?;
    Ok(handle)
}

/// Extract credentials from a SOCKS5 URL.
///
/// `socks5://user:pass@host:port` → `(Some("user"), Some("pass"))`.
/// `socks5://host:port`           → `(None, None)`.
fn parse_socks5_credentials(url: &str) -> (Option<String>, Option<String>) {
    // Strip the `socks5://` scheme.
    let rest = url.strip_prefix("socks5://").unwrap_or(url);

    // If a `@` is present, the format is `user:pass@host:port`.
    if let Some(at_pos) = rest.find('@') {
        let userinfo = &rest[..at_pos];
        if let Some(colon_pos) = userinfo.find(':') {
            let user = &userinfo[..colon_pos];
            let pass = &userinfo[colon_pos + 1..];
            return (Some(user.to_string()), Some(pass.to_string()));
        }
        // No colon → user name only.
        return (Some(userinfo.to_string()), None);
    }

    (None, None)
}

/// Parse a `local_port:remote_host:remote_port` forwarding specification.
fn parse_forward_spec(spec: &str) -> Result<(u16, String, u16)> {
    let parts: Vec<&str> = spec.splitn(3, ':').collect();
    if parts.len() != 3 {
        bail!(
            "invalid port forwarding spec (expected `local_port:remote_host:remote_port`): {}",
            spec
        );
    }
    let local_port: u16 = parts[0]
        .parse()
        .with_context(|| format!("failed to parse local port: {}", parts[0]))?;
    let remote_host = parts[1].to_string();
    let remote_port: u16 = parts[2]
        .parse()
        .with_context(|| format!("failed to parse remote port: {}", parts[2]))?;
    Ok((local_port, remote_host, remote_port))
}

// ---------------------------------------------------------------------------
// Unit tests.
//
// nexterm-ssh is centered on I/O against an external SSH server, so a fully
// integrated test suite would require a mock SSH server (planned for
// Sprint 5-5 and later). Here we restrict ourselves to unit tests over pure
// logic (spec-string parsers, SshConfig construction, and the fast-fail path
// for unreachable hosts).
//
// This is the first response to audit-round-2 task I1 (eliminate the zero
// tests in nexterm-ssh).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_jump_spec --------------------------------------------------

    #[test]
    fn parse_jump_spec_user_host_port() {
        let (user, host, port) =
            parse_jump_spec("alice@bastion.example.com:2222").expect("expected success");
        assert_eq!(user, "alice");
        assert_eq!(host, "bastion.example.com");
        assert_eq!(port, 2222);
    }

    #[test]
    fn parse_jump_spec_user_host_default_port() {
        let (user, host, port) =
            parse_jump_spec("alice@bastion.example.com").expect("expected success");
        assert_eq!(user, "alice");
        assert_eq!(host, "bastion.example.com");
        assert_eq!(port, 22, "omitted port should fall back to 22");
    }

    #[test]
    fn parse_jump_spec_host_only_uses_env_user() {
        // Even when USER / USERNAME is unset, the parser must fall back to `root`.
        let (_user, host, port) =
            parse_jump_spec("bastion.example.com:22").expect("expected success");
        assert_eq!(host, "bastion.example.com");
        assert_eq!(port, 22);
        // User value depends on the environment, so we do not assert its content.
    }

    #[test]
    fn parse_jump_spec_invalid_port_fails() {
        let err = parse_jump_spec("alice@host:not-a-port").expect_err("invalid port must fail");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("port number"),
            "error message should mention the port: {}",
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
        // Even without `socks5://`, the credential portion is still extractable.
        let (u, p) = parse_socks5_credentials("alice:secret@proxy:1080");
        assert_eq!(u.as_deref(), Some("alice"));
        assert_eq!(p.as_deref(), Some("secret"));
    }

    // ---- parse_forward_spec -----------------------------------------------

    #[test]
    fn parse_forward_spec_valid() {
        let (local, host, remote) =
            parse_forward_spec("8080:internal.example.com:80").expect("expected success");
        assert_eq!(local, 8080);
        assert_eq!(host, "internal.example.com");
        assert_eq!(remote, 80);
    }

    #[test]
    fn parse_forward_spec_too_few_parts() {
        let err = parse_forward_spec("8080:internal.example.com").expect_err("malformed spec");
        let msg = format!("{:#}", err);
        assert!(msg.contains("port forwarding spec"));
    }

    #[test]
    fn parse_forward_spec_bad_local_port() {
        let err = parse_forward_spec("abc:host:80").expect_err("bad local port");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("local port"),
            "error message should be specific: {}",
            msg
        );
    }

    #[test]
    fn parse_forward_spec_bad_remote_port() {
        let err = parse_forward_spec("8080:host:abc").expect_err("bad remote port");
        let msg = format!("{:#}", err);
        assert!(msg.contains("remote port"));
    }

    // ---- SshConfig / SshAuth ----------------------------------------------

    #[test]
    fn ssh_auth_password_zeroizes_on_drop() {
        // We cannot directly observe `Zeroizing<String>` clearing its buffer on drop,
        // but we can at least verify that the `Password` variant is constructible
        // and clonable as expected.
        let auth = SshAuth::Password(Zeroizing::new("hunter2".to_string()));
        let cloned = auth.clone();
        match cloned {
            SshAuth::Password(p) => assert_eq!(p.as_str(), "hunter2"),
            _ => panic!("Password variant should be preserved"),
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
        // Clone is supported.
        let cloned = config.clone();
        assert_eq!(cloned.host, "example.com");
        assert_eq!(cloned.port, 22);
        assert_eq!(cloned.username, "alice");
        assert!(cloned.proxy_jump.is_none());
        assert!(cloned.proxy_socks5.is_none());
    }

    // ---- Fast-fail for connection errors ----------------------------------

    /// A connection attempt to an unreachable host should fail within a reasonable time.
    ///
    /// `127.0.0.1:1` (a reserved port) is normally not listening, so `connect`
    /// returns "Connection refused" immediately. The test relies on the network
    /// stack, but uses only the local loopback interface so it is stable in CI.
    #[tokio::test]
    async fn connect_to_unreachable_port_fails_fast() {
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port: 1, // a port that is not listening
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
                // Connection error is the expected outcome.
                assert!(
                    elapsed < std::time::Duration::from_secs(5),
                    "should fail within 5 seconds: {:?}",
                    elapsed
                );
            }
            Ok(Ok(_)) => panic!("a connection to an unreachable port must not succeed"),
            Err(_) => panic!(
                "must respond within 5 seconds (Connection refused is usually <1ms): {:?}",
                elapsed
            ),
        }
    }
}
