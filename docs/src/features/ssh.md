# SSH & Connectivity

See the [TOML Reference](../config/toml.md#hosts) for `[[hosts]]` configuration and the [project README](https://github.com/mizu-jun/Nexterm) for a feature overview.

Nexterm includes a built-in SSH client (powered by the `russh` crate) that opens remote shells as native panes without requiring an external `ssh` binary.

## Authentication Methods

| Method | `auth_type` value | Notes |
|--------|-------------------|-------|
| Public key | `"key"` | Specify `key_path` to the private key file |
| Password | `"password"` | Stored securely in the OS keychain |
| SSH agent | `"agent"` | Uses `SSH_AUTH_SOCK` on Linux/macOS |

## Advanced Connectivity

- **ProxyJump** — multi-hop connections via a bastion host (`proxy_jump` key)
- **SOCKS5 proxy** — route the connection through a SOCKS5 proxy (`socks5_proxy` key)
- **Local port forwarding** (`-L`) — defined via `[[hosts.local_forwards]]`
- **Remote port forwarding** (`-R`) — defined via `[[hosts.forward_remote]]`
- **X11 forwarding** — pass `x11_forward = true` in the host entry

## Host Manager

Open the SSH host manager from the command palette (`ShowHostManager`) to browse registered hosts and connect in a new pane.
