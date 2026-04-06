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

### TOTP / One-Time Password

Enable time-based one-time password (RFC 6238) authentication:

```toml
[web.auth]
totp_enabled = true
issuer       = "Nexterm"   # shown in your authenticator app
# totp_secret is set automatically during first-run setup
```

**First-run setup flow:**

1. Start `nexterm-server` — it logs:
   ```
   TOTP auth is enabled but no secret is configured.
   Open http://localhost:7681/setup in a browser to complete setup.
   ```
2. Open `/setup`, scan the QR code, enter the 6-digit code to verify.
3. `totp_secret` is automatically written to `nexterm.toml`.

### OAuth2 / SSO (Enterprise)

Connect with GitHub, Google, Azure AD, or any OIDC provider:

```toml
[web.auth.oauth]
enabled  = true
provider = "github"      # "github" | "google" | "azure" | "oidc"
client_id = "YOUR_CLIENT_ID"
# Recommended: set client_secret via environment variable
# NEXTERM_OAUTH_CLIENT_SECRET=xxx nexterm-server

# Optional: restrict to specific email addresses
allowed_emails = ["alice@example.com", "bob@example.com"]

# GitHub only: restrict to organization members
# allowed_orgs = ["my-org"]
```

**Provider-specific notes:**

| Provider | `provider` value | Required scope |
|----------|-----------------|----------------|
| GitHub | `github` | `read:user`, `user:email` |
| Google | `google` | `openid`, `email`, `profile` |
| Azure AD | `azure` | `openid`, `email`, `profile`, `User.Read` |
| Generic OIDC | `oidc` | `openid`, `email`, `profile` |

For **Azure AD**, set `issuer_url` to your tenant ID or discovery URL:

```toml
[web.auth.oauth]
enabled     = true
provider    = "azure"
client_id   = "YOUR_APP_ID"
issuer_url  = "your-tenant-id"   # or full https://login.microsoftonline.com/{tenant}/v2.0
```

For **generic OIDC**, set `issuer_url` to the issuer base URL:

```toml
[web.auth.oauth]
enabled    = true
provider   = "oidc"
client_id  = "YOUR_CLIENT_ID"
issuer_url = "https://sso.example.com/realms/nexterm"
```

The OAuth callback URL defaults to `http(s)://localhost:{port}/auth/callback`. Override with:

```toml
redirect_url = "https://nexterm.example.com/auth/callback"
```

#### Security: Client Secret

Never store `client_secret` in `nexterm.toml` if the file is world-readable. Use the environment variable instead:

```sh
NEXTERM_OAUTH_CLIENT_SECRET=xxx nexterm-server
```

### Legacy Token (Backward Compatible)

```toml
[web]
token = "your-secret-token"
```

Connect with: `http://localhost:7681?token=your-secret-token`

---

## Session Settings

### Timeout and Concurrent Limit

```toml
[web.auth]
session_timeout_secs = 86400  # 24 hours (default)

[web]
max_sessions = 10   # 0 = unlimited (default)
```

When `max_sessions` is reached, the oldest session is automatically evicted.

### Logout

Authenticated users can log out by POSTing to `/auth/logout`, which invalidates the session cookie.

---

## HTTPS / TLS

### Self-Signed Certificate (Auto-Generated)

```toml
[web.tls]
enabled = true
```

The certificate is stored at `~/.config/nexterm/tls/cert.pem` and reused across restarts.

### Custom Certificate

```toml
[web.tls]
enabled   = true
cert_file = "/etc/nexterm/tls/fullchain.pem"
key_file  = "/etc/nexterm/tls/privkey.pem"
```

### Force HTTPS Redirect

When running behind a load balancer that terminates TLS, or to ensure all HTTP traffic is redirected to HTTPS:

```toml
[web]
force_https = true
```

This checks the `X-Forwarded-Proto` header and redirects HTTP requests to HTTPS.

---

## Access Log

Record every HTTP request (including WebSocket upgrades and failed authentication attempts):

```toml
[web.access_log]
enabled = true
file    = "/var/log/nexterm/access.csv"   # omit to log to the server log
```

**CSV format:**

```
timestamp,remote_addr,method,path,status,auth_method,user_id
2024-01-01T12:00:00Z,192.168.1.1,GET,/ws,101,totp,
2024-01-01T12:00:01Z,10.0.0.2,GET,/auth/callback,302,oauth:github,octocat
2024-01-01T12:00:02Z,203.0.113.1,POST,/auth/login,401,totp,
```

`auth_method` is one of: `totp`, `oauth:github`, `oauth:google`, `oauth:azure`, `oauth:oidc`, or empty for unauthenticated/legacy-token requests.

---

## Full Configuration Reference

```toml
[web]
enabled      = true
port         = 7681
force_https  = false   # redirect HTTP → HTTPS (requires reverse proxy with X-Forwarded-Proto)
max_sessions = 0       # 0 = unlimited

[web.auth]
totp_enabled         = false
issuer               = "Nexterm"
session_timeout_secs = 86400  # 24 hours

[web.auth.oauth]
enabled        = false
provider       = "github"
client_id      = ""
# client_secret via NEXTERM_OAUTH_CLIENT_SECRET env var (recommended)
# allowed_emails = []
# allowed_orgs   = []   # GitHub only
# redirect_url   = ""   # defaults to http(s)://localhost:{port}/auth/callback
# issuer_url     = ""   # required for "azure" and "oidc" providers

[web.tls]
enabled   = false
# cert_file = "/path/to/cert.pem"
# key_file  = "/path/to/key.pem"

[web.access_log]
enabled = false
# file = "/var/log/nexterm/access.csv"
```

---

## Example: Enterprise Setup with GitHub SSO

```toml
[web]
enabled      = true
port         = 7681
force_https  = true
max_sessions = 20

[web.auth]
session_timeout_secs = 28800   # 8 hours

[web.auth.oauth]
enabled       = true
provider      = "github"
client_id     = "Iv1.abc123def456"
allowed_orgs  = ["my-company"]

[web.tls]
enabled   = true
cert_file = "/etc/nexterm/tls/fullchain.pem"
key_file  = "/etc/nexterm/tls/privkey.pem"

[web.access_log]
enabled = true
file    = "/var/log/nexterm/access.csv"
```

Start with:

```sh
NEXTERM_OAUTH_CLIENT_SECRET=ghp_xxx nexterm-server
```

---

## Session Selection

Append `?session=<name>` to connect to a specific named session:

```
https://localhost:7681?session=work
```

The default session name is `main`.
