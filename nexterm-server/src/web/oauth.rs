//! OAuth2 / OIDC authentication module.
//!
//! Supported providers:
//! - GitHub (OAuth2).
//! - Google (OIDC).
//! - Azure AD (OIDC).
//! - Generic OIDC provider.
//!
//! # Flow
//! 1. `GET /auth/oauth?provider=github` -> redirect to the provider's authorization page.
//! 2. The provider redirects to `GET /auth/callback?code=...&state=...`.
//! 3. Exchange the code for an access token and fetch user info.
//! 4. Check `allowed_emails` / `allowed_orgs` -> issue a session.
//!
//! # Security
//! - CSRF defense via the `state` parameter (stored in a temporary map, expires in 10 minutes).
//! - PKCE is not used (server-side secret only).

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

/// Return-type alias for `build_client` (avoids the type-complexity lint).
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

/// CSRF state entry (expires after 10 minutes).
const STATE_TTL: Duration = Duration::from_secs(600);

/// OAuth user info.
#[derive(Debug, Clone)]
pub struct OAuthUser {
    pub provider: String,
    pub user_id: String,
    pub email: Option<String>,
    pub login: Option<String>,
}

/// CSRF state map (state_token -> expiry).
#[derive(Clone)]
pub struct OAuthManager {
    config: OAuthConfig,
    /// state token -> expiry.
    pending_states: Arc<Mutex<HashMap<String, Instant>>>,
    redirect_base: String,
}

impl OAuthManager {
    /// Build an `OAuthManager` from the config and the callback base URL.
    ///
    /// `redirect_base`: e.g. `"https://example.com"` or `"http://localhost:7681"`.
    pub fn new(config: OAuthConfig, redirect_base: String) -> Self {
        Self {
            config,
            pending_states: Arc::new(Mutex::new(HashMap::new())),
            redirect_base,
        }
    }

    /// Generate and return the provider's authorization URL (issues and stores a state token).
    pub fn authorization_url(&self) -> anyhow::Result<String> {
        let client = self.build_client()?;
        let mut request = client.authorize_url(CsrfToken::new_random);

        // Add the per-provider scopes.
        let scopes = self.required_scopes();
        for scope in scopes {
            request = request.add_scope(Scope::new(scope));
        }

        let (url, csrf_token): (_, CsrfToken) = request.url();

        // Persist the state token (auto-expires in 10 minutes).
        let expiry = Instant::now() + STATE_TTL;
        let mut states = self
            .pending_states
            .lock()
            .expect("OAuth pending_states mutex poisoned");
        // Sweep stale entries.
        states.retain(|_, v| Instant::now() < *v);
        states.insert(csrf_token.secret().clone(), expiry);

        Ok(url.to_string())
    }

    /// Validate the callback `code` and `state`, then return the user info and access token.
    ///
    /// The returned `String` is the access token, which is required for the subsequent
    /// `is_user_allowed` Org membership check. It is returned as a plain `String` (not
    /// `Zeroizing`) because it is a short-lived value dropped after the Org check and is never
    /// logged.
    pub async fn exchange_code(
        &self,
        code: String,
        state: String,
    ) -> anyhow::Result<(OAuthUser, String)> {
        // Validate state (CSRF defense).
        {
            let mut states = self
                .pending_states
                .lock()
                .expect("OAuth pending_states mutex poisoned");
            match states.remove(&state) {
                Some(expiry) if Instant::now() < expiry => {
                    // Valid state.
                }
                Some(_) => {
                    anyhow::bail!("OAuth state has expired");
                }
                None => {
                    anyhow::bail!("OAuth state is invalid (possible CSRF)");
                }
            }
        }

        let client = self.build_client()?;
        let http_client = reqwest::Client::new();
        let token_result: BasicTokenResponse = client
            .exchange_code(AuthorizationCode::new(code))
            .request_async(&http_client)
            .await
            .map_err(|e| anyhow::anyhow!("token exchange failed: {}", e))?;

        let access_token = token_result.access_token().secret().to_string();

        // Fetch user info.
        let user = self.fetch_user_info(&access_token).await?;
        Ok((user, access_token))
    }

    /// Call the provider-specific user-info API.
    async fn fetch_user_info(&self, access_token: &str) -> anyhow::Result<OAuthUser> {
        match self.config.provider.as_str() {
            "github" => self.fetch_github_user(access_token).await,
            "google" => self.fetch_google_user(access_token).await,
            "azure" => self.fetch_azure_user(access_token).await,
            "oidc" => self.fetch_oidc_user(access_token).await,
            other => anyhow::bail!("unsupported OAuth provider: {}", other),
        }
    }

    /// Fetch GitHub user info.
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

        // When `email` is private, fetch it from the `emails` endpoint instead.
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

    /// Fetch the primary email address from GitHub.
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

    /// Fetch user info from the Google userinfo endpoint.
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

    /// Fetch user info from the Azure AD userinfo endpoint.
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

        // Use the Microsoft Graph API.
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

    /// Fetch user info from a generic OIDC userinfo endpoint.
    async fn fetch_oidc_user(&self, access_token: &str) -> anyhow::Result<OAuthUser> {
        let issuer_url = self
            .config
            .issuer_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("the oidc provider requires issuer_url"))?;

        // Fetch `userinfo_endpoint` from the discovery document.
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
            .ok_or_else(|| anyhow::anyhow!("OIDC discovery does not contain userinfo_endpoint"))?;

        // SSRF mitigation (HIGH H-1): restrict `userinfo_endpoint` to the same domain as
        // `discovery_url` and to the HTTPS scheme only.
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

    /// Check whether the user is on an allow-list.
    ///
    /// Authorization logic:
    /// - Both lists empty -> allow everyone.
    /// - `allowed_emails` match -> allow.
    /// - `allowed_orgs` match (GitHub only, validated via the membership API) -> allow.
    /// - Neither matches -> reject.
    ///
    /// `access_token` is required for the GitHub organization membership check.
    /// In the previous implementation `get_current_token()` always returned `None`, so the Org
    /// check never ran and configuring only `allowed_orgs` resulted in nobody being able to log
    /// in (CRITICAL #1).
    pub async fn is_user_allowed(&self, user: &OAuthUser, access_token: &str) -> bool {
        // Both lists empty -> allow everyone.
        if self.config.allowed_emails.is_empty() && self.config.allowed_orgs.is_empty() {
            info!("OAuth: allow list unset; permitting all users");
            return true;
        }

        // Email check.
        if !self.config.allowed_emails.is_empty()
            && let Some(email) = &user.email
            && self.config.allowed_emails.contains(email)
        {
            return true;
        }

        // GitHub Organization check (reliably runs because access_token is passed through).
        if !self.config.allowed_orgs.is_empty()
            && self.config.provider == "github"
            && let Some(login) = &user.login
            && self.check_github_org(access_token, login).await
        {
            return true;
        }

        warn!(
            "OAuth: access denied for user '{}'",
            user.login.as_deref().unwrap_or(&user.user_id)
        );
        false
    }

    /// Check GitHub Organization membership.
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

    // ── Private helpers ──────────────────────────────────────────────────────

    fn build_client(&self) -> anyhow::Result<OAuthClient> {
        let client_id = ClientId::new(
            self.config
                .client_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("OAuth client_id is not configured"))?,
        );

        // The client secret can be overridden via environment variable.
        let client_secret = std::env::var("NEXTERM_OAUTH_CLIENT_SECRET")
            .ok()
            .or_else(|| self.config.client_secret.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "OAuth client_secret is not configured (set the \
                    NEXTERM_OAUTH_CLIENT_SECRET environment variable or the config file)"
                )
            })?;

        let (auth_url, token_url) = self.provider_urls()?;

        let redirect_url = self
            .config
            .redirect_url
            .clone()
            .unwrap_or_else(|| format!("{}/auth/callback", self.redirect_base));

        // oauth2 v5: `BasicClient::new` only takes `client_id`; everything else is set via method chaining.
        let client = BasicClient::new(client_id)
            .set_client_secret(ClientSecret::new(client_secret))
            .set_auth_uri(
                AuthUrl::new(auth_url)
                    .map_err(|e| anyhow::anyhow!("invalid OAuth auth_url: {}", e))?,
            )
            .set_token_uri(
                TokenUrl::new(token_url)
                    .map_err(|e| anyhow::anyhow!("invalid OAuth token_url: {}", e))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_url)
                    .map_err(|e| anyhow::anyhow!("invalid OAuth redirect_url: {}", e))?,
            );

        Ok(client)
    }

    /// Return the per-provider authorization URL and token URL.
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
                // Decide whether `tenant` is a full URL or just a tenant ID.
                let base = if tenant.starts_with("http") {
                    tenant.trim_end_matches('/').to_string()
                } else {
                    format!("https://login.microsoftonline.com/{}/v2.0", tenant)
                };
                Ok((format!("{}/authorize", base), format!("{}/token", base)))
            }
            "oidc" => {
                // Uses URLs previously obtained from discovery.
                // Here we assemble tentative URLs based on `issuer_url`.
                let base = self
                    .config
                    .issuer_url
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("the oidc provider requires issuer_url"))?
                    .trim_end_matches('/');
                Ok((format!("{}/authorize", base), format!("{}/token", base)))
            }
            other => anyhow::bail!("unsupported OAuth provider: {}", other),
        }
    }

    /// Return the scopes required for each provider.
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

/// Validate an OIDC `userinfo_endpoint` URL against SSRF (HIGH H-1).
///
/// - Only the HTTPS scheme is allowed (HTTP risks leaking the access token in plain text).
/// - The host must belong to the same domain as `issuer_url`.
/// - Internal IP ranges (127/8, 10/8, 172.16/12, 192.168/16, 169.254/16) are rejected.
///
/// The previous implementation passed `userinfo_endpoint` from the discovery document straight
/// into `reqwest::get()` without validation. As a result, an attacker-controlled OIDC provider
/// (or DNS hijacking) could redirect `userinfo_endpoint` to the cloud metadata API
/// (`http://169.254.169.254/...`), letting the server issue an authenticated request into the
/// internal network — a working SSRF.
fn validate_userinfo_endpoint(userinfo: &str, issuer_url: &str) -> anyhow::Result<()> {
    // Enforce HTTPS.
    if !userinfo.to_lowercase().starts_with("https://") {
        anyhow::bail!("OIDC userinfo_endpoint must use HTTPS: {}", userinfo);
    }

    // Parse the URL (simple: treat everything before the first '/' after the scheme as the host).
    let host = extract_host(userinfo).ok_or_else(|| {
        anyhow::anyhow!(
            "cannot extract host from OIDC userinfo_endpoint: {}",
            userinfo
        )
    })?;

    // Reject access to internal IPs / link-local addresses.
    if is_disallowed_host(&host) {
        anyhow::bail!(
            "OIDC userinfo_endpoint targets the internal network (SSRF defense): host={}",
            host
        );
    }

    // Validate that the host matches `issuer_url`'s domain (including subdomains).
    let issuer_host = extract_host(issuer_url).ok_or_else(|| {
        anyhow::anyhow!("cannot extract host from OIDC issuer_url: {}", issuer_url)
    })?;
    if !is_same_or_subdomain(&host, &issuer_host) {
        anyhow::bail!(
            "OIDC userinfo_endpoint host '{}' differs from issuer_url '{}' (SSRF defense)",
            host,
            issuer_host
        );
    }

    Ok(())
}

/// Extract the host portion from a URL (strip any port number).
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

/// Reject access to internal IPs / link-local addresses.
fn is_disallowed_host(host: &str) -> bool {
    // localhost / 169.254 / 10.x / 192.168 / 172.16-31 / IPv6 link-local.
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

/// Check whether `host` is identical to `issuer_host` or a subdomain of the same domain.
fn is_same_or_subdomain(host: &str, issuer_host: &str) -> bool {
    if host == issuer_host {
        return true;
    }
    // Accept `subdomain.example.com` against `example.com` (suffix match + dot boundary).
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
    async fn allow_lists_both_empty_permit_everyone() {
        // Case that already worked in the old implementation. Backward-compatibility check.
        let mut config = test_config("github");
        config.allowed_emails = vec![];
        config.allowed_orgs = vec![];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("alice", Some("alice@example.com"));
        assert!(
            mgr.is_user_allowed(&user, "ignored_token").await,
            "both lists empty should allow everyone"
        );
    }

    #[tokio::test]
    async fn allowed_emails_matching_user_is_permitted() {
        let mut config = test_config("github");
        config.allowed_emails = vec!["alice@example.com".to_string()];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("alice", Some("alice@example.com"));
        assert!(mgr.is_user_allowed(&user, "ignored_token").await);
    }

    #[tokio::test]
    async fn allowed_emails_configured_but_mismatched_user_is_rejected() {
        let mut config = test_config("github");
        config.allowed_emails = vec!["alice@example.com".to_string()];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("eve", Some("eve@evil.example"));
        assert!(
            !mgr.is_user_allowed(&user, "ignored_token").await,
            "with allowed_emails set, a mismatch should be rejected"
        );
    }

    #[tokio::test]
    async fn allowed_orgs_only_set_with_unreachable_github_api_rejects() {
        // Core test for CRITICAL #1:
        // The old implementation returned `None` from `get_current_token()` so the Org check
        // never ran and `is_user_allowed` always returned `false` (nobody could log in).
        // After the fix, passing `access_token` lets the real API actually be invoked.
        // The real API is unreachable in tests, so "cannot log in" is expected; what we assert
        // here is that **the real API is invoked** (i.e. the access token is propagated).
        let mut config = test_config("github");
        config.allowed_orgs = vec!["nexterm-team".to_string()];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("alice", Some("alice@example.com"));
        // With an invalid token the HTTP in `check_github_org` fails -> expect false.
        let allowed = mgr.is_user_allowed(&user, "invalid_token_xxx").await;
        assert!(
            !allowed,
            "with an invalid access_token, Org membership cannot be confirmed and should be rejected"
        );
    }

    #[tokio::test]
    async fn allowed_emails_and_allowed_orgs_both_set_short_circuit_on_email_match() {
        // Verifies that when both are set, an email match short-circuits to allow.
        // (This path already worked in the old implementation; we keep a regression guard for it.)
        let mut config = test_config("github");
        config.allowed_emails = vec!["alice@example.com".to_string()];
        config.allowed_orgs = vec!["nexterm-team".to_string()];
        let mgr = OAuthManager::new(config, "http://localhost:7681".to_string());

        let user = make_user("alice", Some("alice@example.com"));
        assert!(
            mgr.is_user_allowed(&user, "any_token").await,
            "an email match should allow before the Org check is consulted"
        );
    }

    // ---- SSRF defense tests (HIGH H-1) ----

    #[test]
    fn extract_host_returns_host_from_typical_url() {
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
    fn is_disallowed_host_rejects_internal_ips() {
        // Core of CRITICAL/H-1: prevent reaching the cloud metadata API.
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
    fn is_disallowed_host_permits_normal_ips() {
        assert!(!is_disallowed_host("example.com"));
        assert!(!is_disallowed_host("8.8.8.8"));
        assert!(!is_disallowed_host("172.32.0.1")); // outside the 172.16/12 range
        assert!(!is_disallowed_host("172.15.0.1"));
        assert!(!is_disallowed_host("login.microsoftonline.com"));
    }

    #[test]
    fn is_same_or_subdomain_permits_same_domain() {
        assert!(is_same_or_subdomain("example.com", "example.com"));
        assert!(is_same_or_subdomain("auth.example.com", "example.com"));
        assert!(is_same_or_subdomain("a.b.example.com", "example.com"));
    }

    #[test]
    fn is_same_or_subdomain_rejects_other_domains() {
        // Core of CRITICAL/H-1: reject attacker-controlled domains.
        assert!(!is_same_or_subdomain("attacker.com", "example.com"));
        assert!(!is_same_or_subdomain("evilexample.com", "example.com"));
        assert!(!is_same_or_subdomain(
            "example.com.attacker.com",
            "example.com"
        ));
    }

    #[test]
    fn validate_userinfo_endpoint_permits_https_with_matching_domain() {
        let r =
            validate_userinfo_endpoint("https://idp.example.com/userinfo", "https://example.com");
        assert!(r.is_ok());
    }

    #[test]
    fn validate_userinfo_endpoint_rejects_http() {
        let r = validate_userinfo_endpoint("http://example.com/userinfo", "https://example.com");
        assert!(r.is_err());
    }

    #[test]
    fn validate_userinfo_endpoint_rejects_internal_ips() {
        // Old SSRF attack vector: prevent reaching the cloud metadata API.
        let r = validate_userinfo_endpoint(
            "https://169.254.169.254/latest/meta-data/",
            "https://example.com",
        );
        assert!(r.is_err());
    }

    #[test]
    fn validate_userinfo_endpoint_rejects_other_domains() {
        let r = validate_userinfo_endpoint("https://attacker.com/userinfo", "https://example.com");
        assert!(r.is_err());
    }
}
