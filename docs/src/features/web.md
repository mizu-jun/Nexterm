# Web Terminal

Nexterm includes an optional web terminal that exposes PTY sessions through any browser via WebSocket and an embedded [xterm.js](https://xtermjs.org/) front-end. This allows access to your sessions from any device without installing the native client.

## Enabling the Web Terminal

Add the following to `nexterm.toml`:

```toml
[web]
enabled = true
port    = 7681   # default
```

Then navigate to `http://localhost:7681` in a browser.

---

## Authentication

### No Authentication (LAN only)

By default the web terminal is unauthenticated. Keep `port` bound on a trusted network or use a reverse proxy.

### TOTP / One-Time Password (Recommended)

Enable time-based one-time password (RFC 6238) authentication:

```toml
[web.auth]
totp_enabled = true
issuer       = "Nexterm"   # shown in your authenticator app
# totp_secret is set automatically during first-run setup
```

**First-run setup flow:**

1. Start `nexterm-server` — it will log a message like:
   ```
   TOTP 認証が有効ですが、シークレットが未設定です。
   ブラウザで http://localhost:7681/setup を開いてセットアップしてください。
   ```
2. Open `/setup` in a browser.
3. Scan the QR code with Google Authenticator, Authy, or any TOTP app.
4. Enter the displayed 6-digit code to verify.
5. `totp_secret` is automatically written to `nexterm.toml`.

From now on, every browser visit to `/` requires a fresh 6-digit code.

> **Note**: Sessions remain valid for 24 hours after successful login. Session state is held in memory; restarting the server requires re-authentication.

### Legacy Token (Backward Compatible)

You can still use the original static token via URL query parameter:

```toml
[web]
token = "your-secret-token"
```

Connect with: `http://localhost:7681?token=your-secret-token`

---

## HTTPS / TLS

### Self-Signed Certificate (Auto-Generated)

Enable HTTPS without any external tools — nexterm generates a self-signed certificate automatically:

```toml
[web.tls]
enabled = true
```

The certificate is stored at `~/.config/nexterm/tls/cert.pem` and reused across restarts. Add it to your browser or system trust store to eliminate the "Not Secure" warning.

The WebSocket client in `index.html` automatically switches from `ws://` to `wss://` when served over HTTPS.

### Custom Certificate

Provide your own PEM certificate (e.g., from Let's Encrypt or a corporate CA):

```toml
[web.tls]
enabled   = true
cert_file = "/etc/nexterm/tls/fullchain.pem"
key_file  = "/etc/nexterm/tls/privkey.pem"
```

---

## Full Secure Configuration Example

```toml
[web]
enabled = true
port    = 7681

[web.auth]
totp_enabled = true
issuer       = "Nexterm"

[web.tls]
enabled = true
```

With this configuration:
- All traffic is encrypted via HTTPS / WSS
- Every login requires a fresh TOTP code
- Sessions expire after 24 hours

---

## Session Selection

Append `?session=<name>` to connect to a specific named session:

```
https://localhost:7681?session=work
```

The default session name is `main`.
