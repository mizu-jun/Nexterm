//! Web ターミナル（HTTPS / WebSocket / OAuth / TOTP）設定

use serde::{Deserialize, Serialize};

/// TOTP 認証設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebAuthConfig {
    /// TOTP 認証を有効にするか（デフォルト: false）
    #[serde(default)]
    pub totp_enabled: bool,
    /// TOTP シークレット（Base32 エンコード）。未設定の場合は初回起動時に生成してブラウザで設定
    pub totp_secret: Option<String>,
    /// 認証アプリに表示する発行者名（デフォルト: "Nexterm"）
    #[serde(default = "default_totp_issuer")]
    pub issuer: String,
    /// OAuth2 / OIDC 設定（設定された場合は TOTP より優先）
    #[serde(default)]
    pub oauth: OAuthConfig,
    /// セッション有効期限（秒）。デフォルト: 86400（24 時間）
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

/// OAuth2 / OIDC 認証設定
///
/// 対応プロバイダー: GitHub / Google / Azure AD / 任意の OIDC プロバイダー
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OAuthConfig {
    /// OAuth2 を有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// プロバイダー識別子: "github" | "google" | "azure" | "oidc"
    #[serde(default)]
    pub provider: String,
    /// クライアント ID
    pub client_id: Option<String>,
    /// クライアントシークレット（環境変数 NEXTERM_OAUTH_CLIENT_SECRET での上書き推奨）
    pub client_secret: Option<String>,
    /// OIDC ディスカバリー URL（provider = "oidc" の場合に使用）
    /// 例: "https://login.microsoftonline.com/{tenant}/v2.0"
    pub issuer_url: Option<String>,
    /// 許可するメールアドレスのリスト（空 = 全員許可）
    #[serde(default)]
    pub allowed_emails: Vec<String>,
    /// 許可する GitHub Organization 名のリスト（provider = "github" のみ）
    #[serde(default)]
    pub allowed_orgs: Vec<String>,
    /// OAuth2 コールバック URL（デフォルト: "http://localhost:{port}/auth/callback"）
    pub redirect_url: Option<String>,
}

/// TLS / HTTPS 設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TlsConfig {
    /// HTTPS を有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// 証明書ファイルパス（PEM）。省略時は自己署名証明書を自動生成
    pub cert_file: Option<String>,
    /// 秘密鍵ファイルパス（PEM）
    pub key_file: Option<String>,
}

/// アクセスログ設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AccessLogConfig {
    /// アクセスログを有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// ログファイルパス。省略時はサーバーログ（tracing）に出力
    pub file: Option<String>,
}

/// Web ターミナル設定（WebSocket + xterm.js）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    /// Web ターミナルを有効にするか（デフォルト: false）
    #[serde(default)]
    pub enabled: bool,
    /// 待ち受けポート（デフォルト: 7681）
    #[serde(default = "default_web_port")]
    pub port: u16,
    /// 認証トークン — 後方互換性のために残す（TOTP と併用不可）
    pub token: Option<String>,
    /// TOTP 認証設定
    #[serde(default)]
    pub auth: WebAuthConfig,
    /// TLS / HTTPS 設定
    #[serde(default)]
    pub tls: TlsConfig,
    /// HTTP アクセス時に HTTPS へ強制リダイレクトするか（デフォルト: false）
    /// tls.enabled = true の場合のみ有効
    #[serde(default)]
    pub force_https: bool,
    /// 同時セッション数の上限（0 = 無制限。デフォルト: 0）
    #[serde(default)]
    pub max_sessions: usize,
    /// アクセスログ設定
    #[serde(default)]
    pub access_log: AccessLogConfig,
    /// **危険**: TLS 設定失敗時に平文 HTTP でフォールバック起動を許可するか（デフォルト: false）
    ///
    /// `tls.enabled = true` で証明書ファイル不在・読み込み失敗・パーミッションエラー
    /// 等が起きた場合の挙動を制御する:
    /// - `false`（デフォルト・推奨）: サーバー起動を中止する。セッショントークンや
    ///   TOTP コードが平文で漏れることを防ぐ。
    /// - `true`: 警告ログを出して HTTP にフォールバックする（テスト・開発のみ）。
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
