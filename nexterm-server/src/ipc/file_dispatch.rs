//! ファイル / 外部接続関連の IPC ハンドラ — Templates/SSH/SFTP/Macro/Serial

use nexterm_proto::ServerToClient;

use super::dispatch::DispatchContext;

pub(super) async fn handle_save_template(ctx: &mut DispatchContext<'_>, name: &str) {
    let manager = ctx.manager;
    let session_name_opt = ctx.current_session.clone();
    let result: anyhow::Result<String> = async {
        let session_name = session_name_opt
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("セッションにアタッチしていません"))?;
        let (window_titles, pane_counts) = {
            let arc = manager.sessions();
            let sessions = arc.lock().await;
            let session = sessions
                .get(session_name)
                .ok_or_else(|| anyhow::anyhow!("セッションが見つかりません: {}", session_name))?;
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

    // Sprint 5-1 / G1: パスワード認証時は IPC 経由ではなく OS キーリング経由で取得する。
    // クライアントが事前に Service="nexterm-ssh", Account="<username>@<host_name>" で
    // 保存したエントリをサーバーが取り出して russh に渡す。
    // ephemeral_password=true（PasswordModal.remember=false 由来）のときは
    // 認証完了後にエントリを削除する。
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
                            // Zeroizing<String> を SshAuth::Password に渡す
                            SshAuth::Password(pw)
                        }
                        Err(e) => {
                            let _ = ctx
                                .tx
                                .send(ServerToClient::Error {
                                    message: format!(
                                        "OS キーリングからのパスワード取得に失敗しました: {}",
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
                                "不正な keyring account 形式 (期待: <username>@<host_name>): '{}'",
                                account
                            ),
                        })
                        .await;
                    return;
                }
            },
            None => {
                // パスワード認証なのに account がない場合は空パスワードで進む（既存挙動を保持）
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
                        tracing::warn!("リモートフォワーディング失敗 '{}': {}", spec, e);
                    }
                }
                let _ = ctx
                    .tx
                    .send(ServerToClient::Error {
                        message: "SSH 認証成功。シェル統合は開発中です".to_string(),
                    })
                    .await;
            }
            Err(e) => {
                let _ = ctx
                    .tx
                    .send(ServerToClient::Error {
                        message: format!("SSH 認証失敗: {}", e),
                    })
                    .await;
            }
        },
        Err(e) => {
            let _ = ctx
                .tx
                .send(ServerToClient::Error {
                    message: format!("SSH 接続失敗: {}", e),
                })
                .await;
        }
    }

    // ephemeral_password=true の場合は keyring エントリを削除（成功・失敗を問わず）
    if let Some((host_name, user)) = keyring_cleanup
        && let Err(e) = nexterm_config::keyring::delete_password(&host_name, &user)
    {
        tracing::warn!(
            "ephemeral keyring エントリ削除に失敗しました (host={}, user={}): {}",
            host_name,
            user,
            e
        );
    }
}

/// `<username>@<host_name>` 形式の keyring account を `(username, host_name)` に分解する。
///
/// `host_name` 側に `@` が含まれる場合は最初の `@` で分割し、残りはすべて host 部とする。
/// account に `@` が無い、もしくは username 部が空の場合は `None` を返す。
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
                message: format!("SFTP: ホスト '{}' が設定に見つかりません", host_name),
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
                message: format!("SFTP: ホスト '{}' が設定に見つかりません", host_name),
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
    fn parse_keyring_account_標準形式() {
        let parsed = parse_keyring_account("alice@dev-server").unwrap();
        assert_eq!(parsed.0, "alice");
        assert_eq!(parsed.1, "dev-server");
    }

    #[test]
    fn parse_keyring_account_host側に_at_を含む() {
        // host_name にラベルとして @ が含まれていても最初の @ で分割される
        let parsed = parse_keyring_account("alice@host@label").unwrap();
        assert_eq!(parsed.0, "alice");
        assert_eq!(parsed.1, "host@label");
    }

    #[test]
    fn parse_keyring_account_at_無し() {
        assert!(parse_keyring_account("alice").is_none());
    }

    #[test]
    fn parse_keyring_account_空ユーザー() {
        assert!(parse_keyring_account("@dev-server").is_none());
    }

    #[test]
    fn parse_keyring_account_空ホスト() {
        assert!(parse_keyring_account("alice@").is_none());
    }

    #[test]
    fn parse_keyring_account_空文字列() {
        assert!(parse_keyring_account("").is_none());
    }
}
