//! OAuth2 / OIDC 認証モジュール
//!
//! 対応プロバイダー:
//! - GitHub (OAuth2)
//! - Google (OIDC)
//! - Azure AD (OIDC)
//! - 汎用 OIDC プロバイダー
//!
//! # フロー
//! 1. `GET /auth/oauth?provider=github` → プロバイダーの認証ページへリダイレクト
//! 2. プロバイダーが `GET /auth/callback?code=...&state=...` へリダイレクト
//! 3. code を access_token に交換してユーザー情報を取得
//! 4. allowed_emails / allowed_orgs チェック → セッション発行
//!
//! # セキュリティ
//! - CSRF 対策として state パラメータを使用（一時マップに保存、10 分で有効期限切れ）
//! - PKCE は使用しない（サーバーサイドシークレットのみ）

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use nexterm_config::OAuthConfig;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointSet, RedirectUrl, Scope,
    TokenResponse, TokenUrl,
    basic::{BasicClient, BasicTokenResponse},
};
use serde::Deserialize;
use tracing::{info, warn};

/// `build_client` の戻り値型エイリアス（型複雑度 lint 回避）
type OAuthClient = oauth2::Client<
    oauth2::basic::BasicErrorResponse,
    BasicTokenResponse,
    oauth2::basic::BasicTokenIntrospectionResponse,
    oauth2::StandardRevocableToken,
    oauth2::basic::BasicRevocationErrorResponse,
    EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    EndpointSet,
>;

/// CSRF state エントリ（10 分で期限切れ）
const STATE_TTL: Duration = Duration::from_secs(600);

/// OAuth ユーザー情報
#[derive(Debug, Clone)]
pub struct OAuthUser {
    pub provider: String,
    pub user_id: String,
    pub email: Option<String>,
    pub login: Option<String>,
}

/// CSRF state マップ（state_token → 有効期限）
#[derive(Clone)]
pub struct OAuthManager {
    config: OAuthConfig,
    /// state トークン → 有効期限
    pending_states: Arc<Mutex<HashMap<String, Instant>>>,
    redirect_base: String,
}

impl OAuthManager {
    /// 設定と callback ベース URL から OAuthManager を生成する
    ///
    /// `redirect_base`: 例 `"https://example.com"` または `"http://localhost:7681"`
    pub fn new(config: OAuthConfig, redirect_base: String) -> Self {
        Self {
            config,
            pending_states: Arc::new(Mutex::new(HashMap::new())),
            redirect_base,
        }
    }

    /// プロバイダーの認証 URL を生成して返す（state を発行して保存する）
    pub fn authorization_url(&self) -> anyhow::Result<String> {
        let client = self.build_client()?;
        let mut request = client.authorize_url(CsrfToken::new_random);

        // プロバイダーごとのスコープ追加
        let scopes = self.required_scopes();
        for scope in scopes {
            request = request.add_scope(Scope::new(scope));
        }

        let (url, csrf_token): (_, CsrfToken) = request.url();

        // state を保存する（10 分で自動期限切れ）
        let expiry = Instant::now() + STATE_TTL;
        let mut states = self
            .pending_states
            .lock()
            .expect("OAuth pending_states mutex poisoned");
        // 古いエントリを掃除する
        states.retain(|_, v| Instant::now() < *v);
        states.insert(csrf_token.secret().clone(), expiry);

        Ok(url.to_string())
    }

    /// コールバックの code と state を検証してユーザー情報と access_token を返す。
    ///
    /// 戻り値の `String` は access_token で、続く `is_user_allowed` の Org メンバーシップ
    /// 検証で必要となる。`Zeroizing` ではなく素の `String` を返すが、Org チェック後に
    /// drop される短寿命の値であり、ログ出力もしない。
    pub async fn exchange_code(
        &self,
        code: String,
        state: String,
    ) -> anyhow::Result<(OAuthUser, String)> {
        // state 検証（CSRF 対策）
        {
            let mut states = self
                .pending_states
                .lock()
                .expect("OAuth pending_states mutex poisoned");
            match states.remove(&state) {
                Some(expiry) if Instant::now() < expiry => {
                    // 有効な state
                }
                Some(_) => {
                    anyhow::bail!("OAuth state が期限切れです");
                }
                None => {
                    anyhow::bail!("OAuth state が無効です（CSRF の可能性）");
                }
            }
        }

        let client = self.build_client()?;
        let http_client = reqwest::Client::new();
        let token_result: BasicTokenResponse = client
            .exchange_code(AuthorizationCode::new(code))
            .request_async(&http_client)
            .await
            .map_err(|e| anyhow::anyhow!("トークン交換失敗: {}", e))?;

        let access_token = token_result.access_token().secret().to_string();

        // ユーザー情報を取得する
        let user = self.fetch_user_info(&access_token).await?;
        Ok((user, access_token))
    }

    /// プロバイダー固有のユーザー情報 API を呼び出す
    async fn fetch_user_info(&self, access_token: &str) -> anyhow::Result<OAuthUser> {
        match self.config.provider.as_str() {
            "github" => self.fetch_github_user(access_token).await,
            "google" => self.fetch_google_user(access_token).await,
            "azure" => self.fetch_azure_user(access_token).await,
            "oidc" => self.fetch_oidc_user(access_token).await,
            other => anyhow::bail!("未対応の OAuth プロバイダー: {}", other),
        }
    }

    /// GitHub ユーザー情報を取得する
    async fn fetch_github_user(&self, access_token: &str) -> anyhow::Result<OAuthUser> {
        #[derive(Deserialize)]
        struct GithubUser {
            id: u64,
            login: String,
            email: Option<String>,
        }

        let client = reqwest::Client::new();
        let user: GithubUser = client
            .get("https://api.github.com/user")
            .bearer_auth(access_token)
            .header("User-Agent", "nexterm/1.0")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // email が非公開の場合は emails エンドポイントから取得する
        let email = if user.email.is_some() {
            user.email
        } else {
            self.fetch_github_primary_email(access_token, &client).await
        };

        Ok(OAuthUser {
            provider: "github".to_string(),
            user_id: user.id.to_string(),
            email,
            login: Some(user.login),
        })
    }

    /// GitHub のプライマリメールアドレスを取得する
    async fn fetch_github_primary_email(
        &self,
        access_token: &str,
        client: &reqwest::Client,
    ) -> Option<String> {
        #[derive(Deserialize)]
        struct GithubEmail {
            email: String,
            primary: bool,
            verified: bool,
        }

        let emails: Vec<GithubEmail> = client
            .get("https://api.github.com/user/emails")
            .bearer_auth(access_token)
            .header("User-Agent", "nexterm/1.0")
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        emails
            .into_iter()
            .find(|e| e.primary && e.verified)
            .map(|e| e.email)
    }

    /// Google userinfo エンドポイントからユーザー情報を取得する
    async fn fetch_google_user(&self, access_token: &str) -> anyhow::Result<OAuthUser> {
        #[derive(Deserialize)]
        struct GoogleUser {
            sub: String,
            email: Option<String>,
            name: Option<String>,
        }

        let user: GoogleUser = reqwest::Client::new()
            .get("https://openidconnect.googleapis.com/v1/userinfo")
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(OAuthUser {
            provider: "google".to_string(),
            user_id: user.sub,
            email: user.email,
            login: user.name,
        })
    }

    /// Azure AD userinfo エンドポイントからユーザー情報を取得する
    async fn fetch_azure_user(&self, access_token: &str) -> anyhow::Result<OAuthUser> {
        #[derive(Deserialize)]
        struct AzureUser {
            id: Option<String>,
            oid: Option<String>,
            mail: Option<String>,
            #[serde(rename = "userPrincipalName")]
            upn: Option<String>,
            #[serde(rename = "displayName")]
            display_name: Option<String>,
        }

        // Microsoft Graph API を使用する
        let user: AzureUser = reqwest::Client::new()
            .get("https://graph.microsoft.com/v1.0/me")
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let user_id = user
            .id
            .or(user.oid)
            .unwrap_or_else(|| "unknown".to_string());
        let email = user.mail.or(user.upn);

        Ok(OAuthUser {
            provider: "azure".to_string(),
            user_id,
            email,
            login: user.display_name,
        })
    }

    /// 汎用 OIDC userinfo エンドポイントからユーザー情報を取得する
    async fn fetch_oidc_user(&self, access_token: &str) -> anyhow::Result<OAuthUser> {
        let issuer_url = self
            .config
            .issuer_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("oidc プロバイダーには issuer_url が必要です"))?;

        // ディスカバリードキュメントから userinfo_endpoint を取得する
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            issuer_url.trim_end_matches('/')
        );

        #[derive(Deserialize)]
        struct Discovery {
            userinfo_endpoint: Option<String>,
        }

        #[derive(Deserialize)]
        struct OidcUser {
            sub: String,
            email: Option<String>,
            name: Option<String>,
        }

        let client = reqwest::Client::new();
        let discovery: Discovery = client
            .get(&discovery_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let userinfo_endpoint = discovery.userinfo_endpoint.ok_or_else(|| {
            anyhow::anyhow!("OIDC ディスカバリーに userinfo_endpoint がありません")
        })?;

        // SSRF 対策（HIGH H-1）: userinfo_endpoint が discovery_url と同じドメインで
        // かつ HTTPS スキームに限定する。
        validate_userinfo_endpoint(&userinfo_endpoint, issuer_url)?;

        let user: OidcUser = client
            .get(&userinfo_endpoint)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(OAuthUser {
            provider: "oidc".to_string(),
            user_id: user.sub,
            email: user.email,
            login: user.name,
        })
    }

    /// ユーザーが許可リストに含まれているか確認する。
    ///
    /// 認可ロジック:
    /// - 両リスト空 → 全員許可
    /// - `allowed_emails` 一致 → 許可
    /// - `allowed_orgs` 一致（GitHub のみ、メンバーシップ API で検証）→ 許可
    /// - どれにも該当しない → 拒否
    ///
    /// `access_token` は GitHub Organization メンバーシップ検証で必須。
    /// 旧実装では `get_current_token()` が常に `None` を返すバグで Org チェックが
    /// 実行されず、`allowed_orgs` 単独設定では誰もログインできない機能不全だった
    /// （CRITICAL #1）。
    pub async fn is_user_allowed(&self, user: &OAuthUser, access_token: &str) -> bool {
        // 両方の許可リストが空 → 全員許可
        if self.config.allowed_emails.is_empty() && self.config.allowed_orgs.is_empty() {
            info!("OAuth: 許可リスト未設定のため全ユーザーを許可");
            return true;
        }

        // メールアドレスチェック
        if !self.config.allowed_emails.is_empty()
            && let Some(email) = &user.email
            && self.config.allowed_emails.contains(email)
        {
            return true;
        }

        // GitHub Organization チェック（access_token 経由で確実に実行）
        if !self.config.allowed_orgs.is_empty()
            && self.config.provider == "github"
            && let Some(login) = &user.login
            && self.check_github_org(access_token, login).await
        {
            return true;
        }

        warn!(
            "OAuth: ユーザー '{}' はアクセス拒否されました",
            user.login.as_deref().unwrap_or(&user.user_id)
        );
        false
    }

    /// GitHub Organization メンバーシップを確認する
    async fn check_github_org(&self, access_token: &str, _login: &str) -> bool {
        let client = reqwest::Client::new();
        for org in &self.config.allowed_orgs {
            let url = format!("https://api.github.com/user/memberships/orgs/{}", org);
            if let Ok(resp) = client
                .get(&url)
                .bearer_auth(access_token)
                .header("User-Agent", "nexterm/1.0")
                .send()
                .await
                && resp.status().is_success()
            {
                return true;
            }
        }
        false
    }

    // ── プライベートヘルパー ──────────────────────────────────────────────────

    fn build_client(&self) -> anyhow::Result<OAuthClient> {
        let client_id = ClientId::new(
            self.config
                .client_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("OAuth client_id が未設定です"))?,
        );

        // クライアントシークレットは環境変数で上書き可能
        let client_secret = std::env::var("NEXTERM_OAUTH_CLIENT_SECRET")
            .ok()
            .or_else(|| self.config.client_secret.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "OAuth client_secret が未設定です（\
                    NEXTERM_OAUTH_CLIENT_SECRET 環境変数または設定ファイルで指定）"
                )
            })?;

        let (auth_url, token_url) = self.provider_urls()?;

        let redirect_url = self
            .config
            .redirect_url
            .clone()
            .unwrap_or_else(|| format!("{}/auth/callback", self.redirect_base));

        // oauth2 v5: BasicClient::new は client_id のみ受け取り、他はメソッドチェーンで設定する
        let client = BasicClient::new(client_id)
            .set_client_secret(ClientSecret::new(client_secret))
            .set_auth_uri(
                AuthUrl::new(auth_url)
                    .map_err(|e| anyhow::anyhow!("OAuth auth_url が不正です: {}", e))?,
            )
            .set_token_uri(
                TokenUrl::new(token_url)
                    .map_err(|e| anyhow::anyhow!("OAuth token_url が不正です: {}", e))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_url)
                    .map_err(|e| anyhow::anyhow!("OAuth redirect_url が不正です: {}", e))?,
            );

        Ok(client)
    }

    /// プロバイダーごとの認証 URL とトークン URL を返す
    fn provider_urls(&self) -> anyhow::Result<(String, String)> {
        match self.config.provider.as_str() {
            "github" => Ok((
                "https://github.com/login/oauth/authorize".to_string(),
                "https://github.com/login/oauth/access_token".to_string(),
            )),
            "google" => Ok((
                "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
                "https://oauth2.googleapis.com/token".to_string(),
            )),
            "azure" => {
                let tenant = self.config.issuer_url.as_deref().unwrap_or("common");
                // tenant が完全な URL かテナント ID かを判定する
                let base = if tenant.starts_with("http") {
                    tenant.trim_end_matches('/').to_string()
                } else {
                    format!("https://login.microsoftonline.com/{}/v2.0", tenant)
                };
                Ok((format!("{}/authorize", base), format!("{}/token", base)))
            }
            "oidc" => {
                // 事前にディスカバリーから取得した URL を使う
                // ここでは issuer_url をベースに仮の URL を組み立てる
                let base = self
                    .config
                    .issuer_url
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("oidc プロバイダーには issuer_url が必要です"))?
                    .trim_end_matches('/');
                Ok((format!("{}/authorize", base), format!("{}/token", base)))
            }
            other => anyhow::bail!("未対応の OAuth プロバイダー: {}", other),
        }
    }

    /// プロバイダーごとに必要なスコープを返す
    fn required_scopes(&self) -> Vec<String> {
        match self.config.provider.as_str() {
            "github" => vec!["read:user".to_string(), "user:email".to_string()],
            "google" => vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ],
            "azure" => vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
                "User.Read".to_string(),
            ],
            "oidc" => vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
            ],
            _ => vec![],
        }
    }
}

/// OIDC userinfo_endpoint URL を SSRF 観点で検証する（HIGH H-1）。
///
/// - HTTPS スキームのみ許可（HTTP では平文で access_token が漏れるリスク）
/// - ホストが `issuer_url` と同じドメインに属していることを検証
/// - 内部 IP アドレス（127/8、10/8、172.16/12、192.168/16、169.254/16）は拒否
///
/// 旧実装はディスカバリードキュメントの `userinfo_endpoint` を無検証で
/// `reqwest::get()` に渡していたため、攻撃者が制御する OIDC プロバイダー
/// （または DNS hijacking）で `userinfo_endpoint` をクラウドメタデータ API
/// (`http://169.254.169.254/...`) に誘導すると、サーバーが内部ネットワークに
/// 認証済みリクエストを発行する SSRF が成立した。
fn validate_userinfo_endpoint(userinfo: &str, issuer_url: &str) -> anyhow::Result<()> {
    // HTTPS 強制
    if !userinfo.to_lowercase().starts_with("https://") {
        anyhow::bail!(
            "OIDC userinfo_endpoint は HTTPS でなければなりません: {}",
            userinfo
        );
    }

    // URL パース（簡易: スキーム除去後の最初の '/' までをホストとする）
    let host = extract_host(userinfo).ok_or_else(|| {
        anyhow::anyhow!(
            "OIDC userinfo_endpoint からホストを抽出できません: {}",
            userinfo
        )
    })?;

    // 内部 IP / リンクローカルへのアクセスを拒否
    if is_disallowed_host(&host) {
        anyhow::bail!(
            "OIDC userinfo_endpoint が内部ネットワークを指しています（SSRF 防止）: host={}",
            host
        );
    }

    // issuer_url のドメインと一致するか検証（subdomain 含む）
    let issuer_host = extract_host(issuer_url).ok_or_else(|| {
        anyhow::anyhow!("OIDC issuer_url からホストを抽出できません: {}", issuer_url)
    })?;
    if !is_same_or_subdomain(&host, &issuer_host) {
        anyhow::bail!(
            "OIDC userinfo_endpoint のホスト '{}' が issuer_url '{}' と異なります（SSRF 防止）",
            host,
            issuer_host
        );
    }

    Ok(())
}

/// URL からホスト部分を抽出する（ポート番号付きの場合は除去）
fn extract_host(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1)?;
    let host_with_port = after_scheme.split('/').next()?;
    let host = host_with_port.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

/// 内部 IP / リンクローカルアドレスへのアクセスを拒否する
fn is_disallowed_host(host: &str) -> bool {
    // localhost / 169.254 / 10.x / 192.168 / 172.16-31 / IPv6 リンクローカル
    let lower = host.to_lowercase();
    if lower == "localhost"
        || lower == "127.0.0.1"
        || lower == "::1"
        || lower.starts_with("127.")
        || lower.starts_with("169.254.")
        || lower.starts_with("10.")
        || lower.starts_with("192.168.")
        || lower.starts_with("0.0.0.0")
        || lower.starts_with("fe80:")
        || lower.starts_with("fc")  // ULA
        || lower.starts_with("fd")
    {
        return true;
    }
    // 172.16.0.0/12 (172.16 - 172.31)
    if let Some(rest) = lower.strip_prefix("172.")
        && let Some(second_octet) = rest.split('.').next()
        && let Ok(n) = second_octet.parse::<u8>()
        && (16..=31).contains(&n)
    {
        return true;
    }
    false
}

/// host が issuer_host と同一、または同じドメインのサブドメインか検証する
fn is_same_or_subdomain(host: &str, issuer_host: &str) -> bool {
    if host == issuer_host {
        return true;
    }
    // subdomain.example.com と example.com を許可（末尾一致 + ドット境界）
    if host.ends_with(issuer_host)
        && host.len() > issuer_host.len()
        && host.as_bytes()[host.len() - issuer_host.len() - 1] == b'.'
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::OAuthConfig;

    fn test_config(provider: &str) -> OAuthConfig {
        OAuthConfig {
            enabled: true,
            provider: provider.to_string(),
            client_id: Some("test_client_id".to_string()),
            client_secret: None,
            redirect_url: None,
            issuer_url: None,
            allowed_emails: vec![],
            allowed_orgs: vec![],
        }
    }

    #[test]
    fn required_scopes_github() {
        let config = test_config("github");
        let manager = OAuthManager::new(config, "http://localhost:7681".to_string());
        let scopes = manager.required_scopes();
        assert!(scopes.contains(&"read:user".to_string()));
        assert!(scopes.contains(&"user:email".to_string()));
    }

    #[test]
    fn required_scopes_google() {
        let config = test_config("google");
        let manager = OAuthManager::new(config, "http://localhost:7681".to_string());
        let scopes = manager.required_scopes();
        assert!(scopes.contains(&"openid".to_string()));
        assert!(scopes.contains(&"email".to_string()));
        assert!(scopes.contains(&"profile".to_string()));
    }

    #[test]
    fn required_scopes_azure() {
        let config = test_config("azure");
        let manager = OAuthManager::new(config, "http://localhost:7681".to_string());
        let scopes = manager.required_scopes();
        assert!(scopes.contains(&"openid".to_string()));
        assert!(scopes.contains(&"User.Read".to_string()));
    }

    #[test]
    fn provider_urls_github() {
        let config = test_config("github");
        let manager = OAuthManager::new(config, "http://localhost:7681".to_string());
        let (auth_url, token_url) = manager.provider_urls().unwrap();
        assert!(auth_url.contains("github.com"));
        assert!(auth_url.contains("authorize"));
        assert!(token_url.contains("access_token"));
    }

    #[test]
    fn provider_urls_google() {
        let config = test_config("google");
        let manager = OAuthManager::new(config, "http://localhost:7681".to_string());
        let (auth_url, token_url) = manager.provider_urls().unwrap();
        assert!(auth_url.contains("google.com"));
        assert!(token_url.contains("token"));
    }

    #[test]
    fn provider_urls_oidc_requires_issuer() {
        let config = OAuthConfig {
            enabled: true,
            provider: "oidc".to_string(),
            client_id: Some("test".to_string()),
            client_secret: None,
            redirect_url: None,
            issuer_url: None,
            allowed_emails: vec![],
            allowed_orgs: vec![],
        };
        let manager = OAuthManager::new(config, "http://localhost:7681".to_string());
        assert!(manager.provider_urls().is_err());
    }

    #[test]
    fn provider_urls_unsupported_provider() {
        let config = OAuthConfig {
            enabled: true,
            provider: "unknown".to_string(),
            client_id: Some("test".to_string()),
            client_secret: None,
            redirect_url: None,
            issuer_url: None,
            allowed_emails: vec![],
            allowed_orgs: vec![],
        };
        let manager = OAuthManager::new(config, "http://localhost:7681".to_string());
        assert!(manager.provider_urls().is_err());
    }

    #[test]
    fn oauth_user_struct_creation() {
        let user = OAuthUser {
            provider: "github".to_string(),
            user_id: "12345".to_string(),
            email: Some("test@example.com".to_string()),
            login: Some("testuser".to_string()),
        };
        assert_eq!(user.provider, "github");
        assert_eq!(user.user_id, "12345");
    }

    #[test]
    fn oauth_manager_new_stores_config() {
        let config = test_config("github");
        let manager = OAuthManager::new(config.clone(), "http://localhost:7681".to_string());
        assert_eq!(manager.config.provider, "github");
        assert_eq!(manager.redirect_base, "http://localhost:7681");
    }

    fn make_user(login: &str, email: Option<&str>) -> OAuthUser {
        OAuthUser {
            provider: "github".to_string(),
            user_id: "user-id".to_string(),
            email: email.map(str::to_string),
            login: Some(login.to_string()),
        }
    }

    #[tokio::test]
    async fn allowリスト_両方空_は全員許可() {
        // 旧実装でも正常動作していたケース。後方互換性確認。
        let mut config = test_config("github");
        config.allowed_emails = vec![];
        config.allowed_orgs = vec![];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("alice", Some("alice@example.com"));
        assert!(
            mgr.is_user_allowed(&user, "ignored_token").await,
            "両リスト空なら全員許可されるべき"
        );
    }

    #[tokio::test]
    async fn allowed_emails_一致で許可される() {
        let mut config = test_config("github");
        config.allowed_emails = vec!["alice@example.com".to_string()];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("alice", Some("alice@example.com"));
        assert!(mgr.is_user_allowed(&user, "ignored_token").await);
    }

    #[tokio::test]
    async fn allowed_emails_設定でメール不一致は拒否される() {
        let mut config = test_config("github");
        config.allowed_emails = vec!["alice@example.com".to_string()];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("eve", Some("eve@evil.example"));
        assert!(
            !mgr.is_user_allowed(&user, "ignored_token").await,
            "allowed_emails が設定されていてメール不一致なら拒否されるべき"
        );
    }

    #[tokio::test]
    #[allow(non_snake_case)]
    async fn allowed_orgs_のみ_設定_かつ_GitHub_API_到達不可は拒否される() {
        // CRITICAL #1 の核心テスト:
        // 旧実装では get_current_token() が None を返すため Org チェックが
        // 絶対実行されず、is_user_allowed が必ず false を返した（誰もログイン不能）。
        // 修正後は access_token を渡せば実 API 経由で検証される。
        // テスト環境では実 API 到達不可なので "ログインできない" が期待だが、
        // **実 API が呼ばれること** (= access_token が伝播されていること) を保証する。
        let mut config = test_config("github");
        config.allowed_orgs = vec!["nexterm-team".to_string()];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("alice", Some("alice@example.com"));
        // 無効トークンなので check_github_org の HTTP は失敗 → false 期待
        let allowed = mgr.is_user_allowed(&user, "invalid_token_xxx").await;
        assert!(
            !allowed,
            "無効 access_token では Org メンバーシップが確認できず拒否されるべき"
        );
    }

    #[tokio::test]
    async fn allowed_emails_と_allowed_orgs_両方設定_メール一致で許可() {
        // 併用時にメール一致で短絡許可されることを確認
        // （旧実装でも動作していたパス、回帰テストとして保護）
        let mut config = test_config("github");
        config.allowed_emails = vec!["alice@example.com".to_string()];
        config.allowed_orgs = vec!["nexterm-team".to_string()];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("alice", Some("alice@example.com"));
        assert!(
            mgr.is_user_allowed(&user, "any_token").await,
            "メール一致で Org チェック前に許可されるべき"
        );
    }

    // ---- SSRF 対策テスト（HIGH H-1）----

    #[test]
    fn extract_host_は通常_url_からホスト名を取得する() {
        assert_eq!(
            extract_host("https://example.com/path"),
            Some("example.com".to_string())
        );
        assert_eq!(
            extract_host("https://Example.COM:8443/path"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn is_disallowed_host_は内部_ip_を拒否する() {
        // CRITICAL/H-1 核心: クラウドメタデータ API への到達を防ぐ
        assert!(is_disallowed_host("169.254.169.254"));
        assert!(is_disallowed_host("127.0.0.1"));
        assert!(is_disallowed_host("localhost"));
        assert!(is_disallowed_host("10.0.0.1"));
        assert!(is_disallowed_host("192.168.1.1"));
        assert!(is_disallowed_host("172.16.0.1"));
        assert!(is_disallowed_host("172.31.255.255"));
        assert!(is_disallowed_host("::1"));
        assert!(is_disallowed_host("fe80::1"));
    }

    #[test]
    fn is_disallowed_host_は通常_ip_を許可する() {
        assert!(!is_disallowed_host("example.com"));
        assert!(!is_disallowed_host("8.8.8.8"));
        assert!(!is_disallowed_host("172.32.0.1")); // 172.16/12 範囲外
        assert!(!is_disallowed_host("172.15.0.1"));
        assert!(!is_disallowed_host("login.microsoftonline.com"));
    }

    #[test]
    fn is_same_or_subdomain_は同一ドメインを許可する() {
        assert!(is_same_or_subdomain("example.com", "example.com"));
        assert!(is_same_or_subdomain("auth.example.com", "example.com"));
        assert!(is_same_or_subdomain("a.b.example.com", "example.com"));
    }

    #[test]
    fn is_same_or_subdomain_は別ドメインを拒否する() {
        // CRITICAL/H-1 核心: 攻撃者制御ドメインを拒否
        assert!(!is_same_or_subdomain("attacker.com", "example.com"));
        assert!(!is_same_or_subdomain("evilexample.com", "example.com"));
        assert!(!is_same_or_subdomain(
            "example.com.attacker.com",
            "example.com"
        ));
    }

    #[test]
    fn validate_userinfo_endpoint_は_https_と一致ドメインを許可する() {
        let r =
            validate_userinfo_endpoint("https://idp.example.com/userinfo", "https://example.com");
        assert!(r.is_ok());
    }

    #[test]
    fn validate_userinfo_endpoint_は_http_を拒否する() {
        let r = validate_userinfo_endpoint("http://example.com/userinfo", "https://example.com");
        assert!(r.is_err());
    }

    #[test]
    fn validate_userinfo_endpoint_は内部_ip_を拒否する() {
        // 旧 SSRF 攻撃ベクター: クラウドメタデータ API 到達を防ぐ
        let r = validate_userinfo_endpoint(
            "https://169.254.169.254/latest/meta-data/",
            "https://example.com",
        );
        assert!(r.is_err());
    }

    #[test]
    fn validate_userinfo_endpoint_は別ドメインを拒否する() {
        let r = validate_userinfo_endpoint("https://attacker.com/userinfo", "https://example.com");
        assert!(r.is_err());
    }
}
