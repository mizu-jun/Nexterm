//! IPC handlers for file and external-connection commands — Templates/SSH/SFTP/Macro/Serial.

use nexterm_proto::ServerToClient;

use super::dispatch::DispatchContext;

pub(super) async fn handle_save_template(ctx: &mut DispatchContext<'_>, name: &str) {
    let manager = ctx.manager;
    let session_name_opt = ctx.current_session.clone();
    let result: anyhow::Result<String> = async {
        let session_name = session_name_opt
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("not attached to any session"))?;
        let (window_titles, pane_counts) = {
            let arc = manager.sessions();
            let sessions = arc.lock().await;
            let session = sessions
                .get(session_name)
                .ok_or_else(|| anyhow::anyhow!("session not found: {}", session_name))?;
            let info = session.window_list();
            let titles: Vec<String> = info.iter().map(|w| w.name.clone()).collect();
            let counts: Vec<usize> = info.iter().map(|w| w.pane_count as usize).collect();
            (titles, counts)
        };
        let template =
            crate::template::template_from_session_info(name, window_titles, pane_counts);
        let path = template.save()?;
        Ok(path)
    }
    .await;
    match result {
        Ok(path) => {
            let _ = ctx
                .tx
                .send(ServerToClient::TemplateSaved {
                    name: name.to_string(),
                    path,
                })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn handle_load_template(ctx: &mut DispatchContext<'_>, name: &str) {
    match crate::template::LayoutTemplate::load(name) {
        Ok(_template) => {
            let _ = ctx
                .tx
                .send(ServerToClient::TemplateLoaded {
                    name: name.to_string(),
                })
                .await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

pub(super) async fn handle_list_templates(ctx: &mut DispatchContext<'_>) {
    match crate::template::LayoutTemplate::list() {
        Ok(names) => {
            let _ = ctx.tx.send(ServerToClient::TemplateList { names }).await;
        }
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: e.to_string(),
                })
                .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_connect_ssh(
    ctx: &mut DispatchContext<'_>,
    host: &str,
    port: u16,
    username: &str,
    auth_type: &str,
    password_keyring_account: &Option<String>,
    ephemeral_password: bool,
    key_path: &Option<String>,
    remote_forwards: &[String],
) {
    use nexterm_ssh::{SshAuth, SshConfig, SshSession};
    use zeroize::Zeroizing;

    // Sprint 5-1 / G1: for password auth, fetch credentials via the OS keyring instead of IPC.
    // The client stores the entry as Service="nexterm-ssh", Account="<username>@<host_name>" up
    // front, and the server retrieves it here to pass into russh. When ephemeral_password=true
    // (`PasswordModal.remember=false`), the entry is deleted after the auth attempt.
    let mut keyring_cleanup: Option<(String, String)> = None;

    let auth = match auth_type {
        "password" => match password_keyring_account {
            Some(account) => match parse_keyring_account(account) {
                Some((kr_user, kr_host)) => {
                    match nexterm_config::keyring::get_password(&kr_host, &kr_user) {
                        Ok(pw) => {
                            if ephemeral_password {
                                keyring_cleanup = Some((kr_host.clone(), kr_user.clone()));
                            }
                            // Pass a `Zeroizing<String>` into `SshAuth::Password`.
                            SshAuth::Password(pw)
                        }
                        Err(e) => {
                            let _ = ctx
                                .tx
                                .send(ServerToClient::Error {
                                    message: format!(
                                        "failed to fetch password from OS keyring: {}",
                                        e
                                    ),
                                })
                                .await;
                            return;
                        }
                    }
                }
                None => {
                    let _ = ctx
                        .tx
                        .send(ServerToClient::Error {
                            message: format!(
                                "invalid keyring account format (expected <username>@<host_name>): '{}'",
                                account
                            ),
                        })
                        .await;
                    return;
                }
            },
            None => {
                // Password auth without an account: proceed with an empty password (legacy behavior).
                SshAuth::Password(Zeroizing::new(String::new()))
            }
        },
        "key" => {
            let kp = key_path.clone().unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| std::path::PathBuf::from(h).join(".ssh").join("id_rsa"))
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
            SshAuth::PrivateKey {
                key_path: std::path::PathBuf::from(kp),
                passphrase: None,
            }
        }
        _ => SshAuth::Agent,
    };

    let ssh_config = SshConfig {
        host: host.to_string(),
        port,
        username: username.to_string(),
        auth,
        proxy_jump: None,
        proxy_socks5: None,
    };

    match SshSession::connect(&ssh_config).await {
        Ok(mut session) => match session.authenticate(&ssh_config).await {
            Ok(()) => {
                for spec in remote_forwards {
                    if let Err(e) = session.start_remote_forward(spec).await {
                        tracing::warn!("remote forwarding failed '{}': {}", spec, e);
                    }
                }
                let _ = ctx
                    .tx
                    .send(ServerToClient::Error {
                        message:
                            "SSH authentication succeeded; shell integration is in development"
                                .to_string(),
                    })
                    .await;
            }
            Err(e) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::Error {
                        message: format!("SSH authentication failed: {}", e),
                    })
                    .await;
            }
        },
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: format!("SSH connection failed: {}", e),
                })
                .await;
        }
    }

    // If ephemeral_password=true, delete the keyring entry (regardless of success/failure).
    if let Some((host_name, user)) = keyring_cleanup
        && let Err(e) = nexterm_config::keyring::delete_password(&host_name, &user)
    {
        tracing::warn!(
            "failed to delete ephemeral keyring entry (host={}, user={}): {}",
            host_name,
            user,
            e
        );
    }
}

/// Split a `<username>@<host_name>` keyring account into `(username, host_name)`.
///
/// When `host_name` itself contains `@`, the split is performed on the first `@` and the rest
/// becomes the host part. Returns `None` when `account` has no `@` or the username part is empty.
fn parse_keyring_account(account: &str) -> Option<(String, String)> {
    let (user, host) = account.split_once('@')?;
    if user.is_empty() || host.is_empty() {
        return None;
    }
    Some((user.to_string(), host.to_string()))
}

pub(super) async fn handle_sftp_upload(
    ctx: &mut DispatchContext<'_>,
    host_name: &str,
    local_path: &str,
    remote_path: &str,
) {
    if let Some(host_cfg) = ctx.hosts.iter().find(|h| h.name == host_name) {
        let host_cfg = host_cfg.clone();
        let local = local_path.to_string();
        let remote = remote_path.to_string();
        let tx2 = ctx.tx.clone();
        let display = local_path.to_string();

        tokio::spawn(async move {
            let result =
                super::sftp::run_sftp_upload(&host_cfg, &local, &remote, tx2.clone()).await;
            let _ = tx2
                .send(ServerToClient::SftpDone {
                    path: display,
                    error: result.err().map(|e| e.to_string()),
                })
                .await;
        });
    } else {
        let _ = ctx
            .tx
            .send(ServerToClient::Error {
                message: format!("SFTP: host '{}' not found in configuration", host_name),
            })
            .await;
    }
}

pub(super) async fn handle_sftp_download(
    ctx: &mut DispatchContext<'_>,
    host_name: &str,
    remote_path: &str,
    local_path: &str,
) {
    if let Some(host_cfg) = ctx.hosts.iter().find(|h| h.name == host_name) {
        let host_cfg = host_cfg.clone();
        let remote = remote_path.to_string();
        let local = local_path.to_string();
        let tx2 = ctx.tx.clone();
        let display = remote_path.to_string();

        tokio::spawn(async move {
            let result =
                super::sftp::run_sftp_download(&host_cfg, &remote, &local, tx2.clone()).await;
            let _ = tx2
                .send(ServerToClient::SftpDone {
                    path: display,
                    error: result.err().map(|e| e.to_string()),
                })
                .await;
        });
    } else {
        let _ = ctx
            .tx
            .send(ServerToClient::Error {
                message: format!("SFTP: host '{}' not found in configuration", host_name),
            })
            .await;
    }
}

pub(super) async fn handle_run_macro(
    ctx: &mut DispatchContext<'_>,
    macro_fn: &str,
    display_name: &str,
) {
    if let Some(ref name) = *ctx.current_session {
        let manager = ctx.manager;
        let focused_pane_id = {
            let arc = manager.sessions();
            let sessions = arc.lock().await;
            sessions
                .get(name)
                .and_then(|s| s.focused_window())
                .map(|w| w.focused_pane_id())
        };
        if let Some(pane_id) = focused_pane_id {
            tracing::info!("RunMacro: {} (fn={})", display_name, macro_fn);
            let lua_ref = ctx.lua.clone();
            let fn_name = macro_fn.to_string();
            let session_name = name.clone();
            let output = tokio::task::spawn_blocking(move || {
                lua_ref.call_macro(&fn_name, &session_name, pane_id)
            })
            .await
            .unwrap_or(None);

            if let Some(text) = output {
                let arc = manager.sessions();
                let sessions = arc.lock().await;
                if let Some(session) = sessions.get(name)
                    && let Some(window) = session.focused_window()
                    && let Some(pane) = window.pane(pane_id)
                {
                    let _ = pane.write_input(text.as_bytes());
                }
            }
        }
    }
}

pub(super) async fn handle_connect_serial(
    ctx: &mut DispatchContext<'_>,
    port: &str,
    baud_rate: u32,
    data_bits: u8,
    stop_bits: u8,
    parity: &str,
) {
    if let Some(ref name) = *ctx.current_session {
        let result = ctx
            .manager
            .connect_serial(name, port, baud_rate, data_bits, stop_bits, parity)
            .await;
        match result {
            Ok(pane_id) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::SerialConnected {
                        pane_id,
                        port: port.to_string(),
                    })
                    .await;
                let layout_msg = {
                    let arc = ctx.manager.sessions();
                    let sessions = arc.lock().await;
                    sessions.get(name).and_then(|s| {
                        s.focused_window()
                            .map(|w| w.layout_changed_msg(s.cols, s.rows))
                    })
                };
                if let Some(msg) = layout_msg {
                    let _ = ctx.tx.send(msg).await;
                }
            }
            Err(e) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::Error {
                        message: e.to_string(),
                    })
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod connect_ssh_tests {
    use super::parse_keyring_account;

    #[test]
    fn parse_keyring_account_standard_form() {
        let parsed = parse_keyring_account("alice@dev-server").unwrap();
        assert_eq!(parsed.0, "alice");
        assert_eq!(parsed.1, "dev-server");
    }

    #[test]
    fn parse_keyring_account_host_contains_at_sign() {
        // Even if the host_name contains an extra '@' label, the split happens on the first '@'.
        let parsed = parse_keyring_account("alice@host@label").unwrap();
        assert_eq!(parsed.0, "alice");
        assert_eq!(parsed.1, "host@label");
    }

    #[test]
    fn parse_keyring_account_without_at_sign() {
        assert!(parse_keyring_account("alice").is_none());
    }

    #[test]
    fn parse_keyring_account_empty_user() {
        assert!(parse_keyring_account("@dev-server").is_none());
    }

    #[test]
    fn parse_keyring_account_empty_host() {
        assert!(parse_keyring_account("alice@").is_none());
    }

    #[test]
    fn parse_keyring_account_empty_string() {
        assert!(parse_keyring_account("").is_none());
    }
}
