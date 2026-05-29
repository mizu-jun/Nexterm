//! SSH connection helpers
//!
//! Extracted from `input_handler.rs`:
//! - `connect_ssh_host` — connect in the current pane (unused)
//! - `connect_ssh_host_new_tab` — connect in a new tab (for public-key / agent auth hosts)
//! - `connect_ssh_host_with_password` — Sprint 5-1 / G1: pass the password via the OS keyring

use nexterm_proto::ClientToServer;

use super::EventHandler;

impl EventHandler {
    /// Send a ConnectSsh message from a HostConfig (connects in the current pane)
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

    /// Open a new tab from a HostConfig and send a ConnectSsh message
    ///
    /// Phase 5-11-6 #2: widened to `pub(in crate::renderer)` so the HostItem
    /// Click path in `event_handler::accessibility` can also call it.
    pub(in crate::renderer) fn connect_ssh_host_new_tab(&self, host: &nexterm_config::HostConfig) {
        let Some(conn) = &self.connection else { return };
        // First create a new window (tab), then request the SSH connection.
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

    /// Open a new tab with a password and send a ConnectSsh message (for password-auth hosts)
    ///
    /// **Sprint 5-1 / G1**: never send the plaintext password over IPC.
    /// The client stores it in advance in the OS keyring
    /// (Service="nexterm-ssh", Account="<user>@<host>") and only the account
    /// name is sent over IPC; the server fetches it from the keyring.
    ///
    /// Even with `remember=false` (the user chose "do not save" in the
    /// PasswordModal), if `take_password()` did not persist it, we store it
    /// here temporarily. `ephemeral_password=true` tells the server to delete
    /// the keyring entry once authentication completes.
    ///
    /// The password is received as `Zeroizing<String>`, so it is zeroed when
    /// the function returns.
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
            // With remember=true the PasswordModal::take_password() already
            // saved the password. Only when remember=false do we temporarily
            // write to the keyring here (the server removes it after auth).
            if !remember
                && let Err(e) =
                    nexterm_config::keyring::store_password(&host.name, &host.username, &password)
            {
                tracing::error!(
                    "aborting SSH connect: failed to store password temporarily in OS keyring: host={} user={} err={}",
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
