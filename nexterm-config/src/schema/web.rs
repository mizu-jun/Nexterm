//! Web terminal (HTTPS / WebSocket / OAuth / TOTP) configuration.

use serde::{Deserialize, Serialize};

/// TOTP authentication configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebAuthConfig {
    /// Whether to enable TOTP authentication (default: `false`).
    #[serde(default)]
    pub totp_enabled: bool,
    /// TOTP secret (Base32-encoded). When unset, it is generated on first
    /// launch and configured via the browser.
    pub totp_secret: Option<String>,
    /// Issuer name shown by the authenticator app (default: `"Nexterm"`).
    #[serde(default = "default_totp_issuer")]
    pub issuer: String,
    /// OAuth2 / OIDC configuration (takes precedence over TOTP when present).
    #[serde(default)]
    pub oauth: OAuthConfig,
    /// Session expiration (seconds). Default: 86_400 (24 hours).
    #[serde(default = "default_session_timeout_secs")]
    pub session_timeout_secs: u64,
}

fn default_totp_issuer() -> String {
    "Nexterm".to_string()
}

fn default_session_timeout_secs() -> u64 {
    86_400
}

impl Default for WebAuthConfig {
    fn default() -> Self {
        Self {
            totp_enabled: false,
            totp_secret: None,
            issuer: default_totp_issuer(),
            oauth: OAuthConfig::default(),
            session_timeout_secs: default_session_timeout_secs(),
        }
    }
}

/// OAuth2 / OIDC authentication configuration.
///
/// Supported providers: GitHub / Google / Azure AD / any OIDC provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OAuthConfig {
    /// Whether to enable OAuth2 (default: `false`).
    #[serde(default)]
    pub enabled: bool,
    /// Provider identifier: `"github"` | `"google"` | `"azure"` | `"oidc"`.
    #[serde(default)]
    pub provider: String,
    /// Client ID.
    pub client_id: Option<String>,
    /// Client secret. Overriding via the `NEXTERM_OAUTH_CLIENT_SECRET`
    /// environment variable is recommended.
    pub client_secret: Option<String>,
    /// OIDC discovery URL (used when `provider = "oidc"`).
    /// Example: `"https://login.microsoftonline.com/{tenant}/v2.0"`.
    pub issuer_url: Option<String>,
    /// Allow-list of e-mail addresses (empty means "everyone is allowed").
    #[serde(default)]
    pub allowed_emails: Vec<String>,
    /// Allow-list of GitHub organization names (only with `provider = "github"`).
    #[serde(default)]
    pub allowed_orgs: Vec<String>,
    /// OAuth2 callback URL (default: `"http://localhost:{port}/auth/callback"`).
    pub redirect_url: Option<String>,
}

/// TLS / HTTPS configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TlsConfig {
    /// Whether to enable HTTPS (default: `false`).
    #[serde(default)]
    pub enabled: bool,
    /// Path to the certificate file (PEM). When omitted, a self-signed
    /// certificate is generated automatically.
    pub cert_file: Option<String>,
    /// Path to the private-key file (PEM).
    pub key_file: Option<String>,
}

/// Access-log configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessLogConfig {
    /// Whether to enable the access log (default: `false`).
    #[serde(default)]
    pub enabled: bool,
    /// Log file path. When omitted, the access log is written through the
    /// regular tracing pipeline.
    pub file: Option<String>,
    /// Maximum size per file (MiB). 0 disables size-based rotation.
    #[serde(default = "default_access_log_max_size_mib")]
    pub max_size_mib: u64,
    /// Number of generations to keep (0 disables rotation; 1+ keeps
    /// `.1`..=`.N`).
    #[serde(default = "default_access_log_max_generations")]
    pub max_generations: u32,
    /// Enable gzip compression for rotated files (saved as `.{N}.gz`).
    #[serde(default)]
    pub compress: bool,
}

fn default_access_log_max_size_mib() -> u64 {
    10
}

fn default_access_log_max_generations() -> u32 {
    7
}

impl Default for AccessLogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            file: None,
            max_size_mib: default_access_log_max_size_mib(),
            max_generations: default_access_log_max_generations(),
            compress: false,
        }
    }
}

/// Web terminal configuration (WebSocket + xterm.js).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    /// Whether to enable the web terminal (default: `false`).
    #[serde(default)]
    pub enabled: bool,
    /// Listen port (default: 7681).
    #[serde(default = "default_web_port")]
    pub port: u16,
    /// Authentication token — kept for backward compatibility (cannot be used
    /// together with TOTP).
    pub token: Option<String>,
    /// TOTP authentication configuration.
    #[serde(default)]
    pub auth: WebAuthConfig,
    /// TLS / HTTPS configuration.
    #[serde(default)]
    pub tls: TlsConfig,
    /// Whether to redirect HTTP access to HTTPS (default: `false`).
    /// Effective only when `tls.enabled = true`.
    #[serde(default)]
    pub force_https: bool,
    /// Maximum concurrent sessions (0 = unlimited; default: 0).
    #[serde(default)]
    pub max_sessions: usize,
    /// Access-log configuration.
    #[serde(default)]
    pub access_log: AccessLogConfig,
    /// **Dangerous**: allow falling back to plain-text HTTP when TLS
    /// configuration fails (default: `false`).
    ///
    /// Controls the behavior when `tls.enabled = true` and the certificate
    /// file is missing, fails to load, has permission errors, etc.:
    /// - `false` (default, recommended): abort startup so that session
    ///   tokens and TOTP codes are never leaked in plain text.
    /// - `true`: log a warning and fall back to HTTP (for testing /
    ///   development only).
    #[serde(default)]
    pub allow_http_fallback: bool,
}

fn default_web_port() -> u16 {
    7681
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_web_port(),
            token: None,
            auth: WebAuthConfig::default(),
            tls: TlsConfig::default(),
            force_https: false,
            max_sessions: 0,
            access_log: AccessLogConfig::default(),
            allow_http_fallback: false,
        }
    }
}
