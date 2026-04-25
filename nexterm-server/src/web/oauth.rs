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
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointSet, RedirectUrl,
    Scope, TokenResponse, TokenUrl,
    basic::{BasicClient, BasicTokenResponse},
};
use serde::Deserialize;
use tracing::{info, warn};

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
        let mut states = self.pending_states.lock().expect("OAuth pending_states mutex poisoned");
        // 古いエントリを掃除する
        states.retain(|_, v| Instant::now() < *v);
        states.insert(csrf_token.secret().clone(), expiry);

        Ok(url.to_string())
    }

    /// コールバックの code と state を検証してユーザー情報を取得する
    pub async fn exchange_code(
        &self,
        code: String,
        state: String,
    ) -> anyhow::Result<OAuthUser> {
        // state 検証（CSRF 対策）
        {
            let mut states = self.pending_states.lock().expect("OAuth pending_states mutex poisoned");
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
        self.fetch_user_info(&access_token).await
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

        let userinfo_endpoint = discovery
            .userinfo_endpoint
            .ok_or_else(|| anyhow::anyhow!("OIDC ディスカバリーに userinfo_endpoint がありません"))?;

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

    /// ユーザーが許可リストに含まれているか確認する
    ///
    /// `allowed_emails` と `allowed_orgs` が両方空の場合は全員許可。
    pub async fn is_user_allowed(&self, user: &OAuthUser) -> bool {
        // メールアドレスチェック
        if !self.config.allowed_emails.is_empty()
            && let Some(email) = &user.email
                && self.config.allowed_emails.contains(email) {
                    return true;
                }
            // allowed_emails が設定されていてメールが一致しない場合は
            // allowed_orgs も確認する

        // GitHub Organization チェック
        if !self.config.allowed_orgs.is_empty() && self.config.provider == "github"
            && let Some(login) = &user.login
                && let Some(token) = self.get_current_token().await
                    && self.check_github_org(&token, login).await {
                        return true;
                    }

        // 両方の許可リストが空 → 全員許可
        if self.config.allowed_emails.is_empty() && self.config.allowed_orgs.is_empty() {
            info!("OAuth: 許可リスト未設定のため全ユーザーを許可");
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
                && resp.status().is_success() {
                    return true;
                }
        }
        false
    }

    /// 現在の access_token を一時保存から取得する（org チェック用）
    /// 注: 簡易実装として None を返す（org チェックは token exchange 後に行うこと）
    async fn get_current_token(&self) -> Option<String> {
        None
    }

    // ── プライベートヘルパー ──────────────────────────────────────────────────

    fn build_client(
        &self,
    ) -> anyhow::Result<
        oauth2::Client<
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
        >,
    > {
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

        let redirect_url = self.config.redirect_url.clone().unwrap_or_else(|| {
            format!("{}/auth/callback", self.redirect_base)
        });

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
                let tenant = self
                    .config
                    .issuer_url
                    .as_deref()
                    .unwrap_or("common");
                // tenant が完全な URL かテナント ID かを判定する
                let base = if tenant.starts_with("http") {
                    tenant.trim_end_matches('/').to_string()
                } else {
                    format!("https://login.microsoftonline.com/{}/v2.0", tenant)
                };
                Ok((
                    format!("{}/authorize", base),
                    format!("{}/token", base),
                ))
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
                Ok((
                    format!("{}/authorize", base),
                    format!("{}/token", base),
                ))
            }
            other => anyhow::bail!("未対応の OAuth プロバイダー: {}", other),
        }
    }

    /// プロバイダーごとに必要なスコープを返す
    fn required_scopes(&self) -> Vec<String> {
        match self.config.provider.as_str() {
            "github" => vec!["read:user".to_string(), "user:email".to_string()],
            "google" => vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
            "azure" => vec![
                "openid".to_string(),
                "email".to_string(),
                "profile".to_string(),
                "User.Read".to_string(),
            ],
            "oidc" => vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
            _ => vec![],
        }
    }
}
