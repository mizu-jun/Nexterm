//! SSH 接続関連のヘルパー
//!
//! `input_handler.rs` から抽出した:
//! - `connect_ssh_host` — 現在のペインで接続（未使用）
//! - `connect_ssh_host_new_tab` — 新しいタブで接続（公開鍵 / agent 認証ホスト用）
//! - `connect_ssh_host_with_password` — Sprint 5-1 / G1: パスワードを OS キーリング経由で渡す

use nexterm_proto::ClientToServer;

use super::EventHandler;

impl EventHandler {
    /// HostConfig から ConnectSsh メッセージを送信する（現在のペインに接続）
    #[allow(dead_code)]
    pub(super) fn connect_ssh_host(&self, host: &nexterm_config::HostConfig) {
        let Some(conn) = &self.connection else { return };
        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password_keyring_account: None,
            ephemeral_password: false,
            key_path: host.key_path.clone(),
            remote_forwards: host.forward_remote.clone(),
            x11_forward: host.x11_forward,
            x11_trusted: host.x11_trusted,
        });
    }

    /// HostConfig から新しいタブを開いて ConnectSsh メッセージを送信する
    pub(super) fn connect_ssh_host_new_tab(&self, host: &nexterm_config::HostConfig) {
        let Some(conn) = &self.connection else { return };
        // 先に新しいウィンドウ（タブ）を作成してから SSH 接続を要求する
        let _ = conn.send_tx.try_send(ClientToServer::NewWindow);
        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password_keyring_account: None,
            ephemeral_password: false,
            key_path: host.key_path.clone(),
            remote_forwards: host.forward_remote.clone(),
            x11_forward: host.x11_forward,
            x11_trusted: host.x11_trusted,
        });
    }

    /// パスワード付きで新しいタブを開いて ConnectSsh メッセージを送信する（パスワード認証ホスト用）
    ///
    /// **Sprint 5-1 / G1**: IPC 経路にパスワード平文を流さない。
    /// クライアントが事前に OS キーリング（Service="nexterm-ssh", Account="<user>@<host>"）
    /// に保存し、IPC では account 名のみ送る。サーバーは keyring から取得する。
    ///
    /// `remember=false`（PasswordModal でユーザーが「保存しない」を選んだ場合）でも、
    /// `take_password()` 側で永続保存していない場合はここで一時保存する。
    /// `ephemeral_password=true` をサーバーに通知し、認証完了後に keyring エントリを削除させる。
    ///
    /// パスワード文字列は `Zeroizing<String>` で受け取り、関数終了時にゼロクリアされる。
    pub(super) fn connect_ssh_host_with_password(
        &self,
        host: &nexterm_config::HostConfig,
        password: zeroize::Zeroizing<String>,
        remember: bool,
    ) {
        let Some(conn) = &self.connection else { return };
        let _ = conn.send_tx.try_send(ClientToServer::NewWindow);

        let password_keyring_account = if password.is_empty() {
            None
        } else {
            // remember=true の場合は PasswordModal::take_password() 内で既に保存済み。
            // remember=false の場合のみ一時的に keyring に書き込む（サーバーが認証後に削除）。
            if !remember
                && let Err(e) =
                    nexterm_config::keyring::store_password(&host.name, &host.username, &password)
            {
                tracing::error!(
                    "OS キーリングへの一時保存に失敗したため SSH 接続を中止します: host={} user={} err={}",
                    host.name,
                    host.username,
                    e
                );
                return;
            }
            Some(format!("{}@{}", host.username, host.name))
        };

        let _ = conn.send_tx.try_send(ClientToServer::ConnectSsh {
            host: host.host.clone(),
            port: host.port,
            username: host.username.clone(),
            auth_type: host.auth_type.clone(),
            password_keyring_account,
            ephemeral_password: !remember,
            key_path: host.key_path.clone(),
            remote_forwards: host.forward_remote.clone(),
            x11_forward: host.x11_forward,
            x11_trusted: host.x11_trusted,
        });
    }
}
