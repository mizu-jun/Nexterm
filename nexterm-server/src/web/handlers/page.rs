//! HTML page handlers: index / login / setup.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
};
use tracing::warn;

use super::assets::{redirect, serve_asset};
use crate::web::AppState;
use crate::web::Assets;
use crate::web::middleware::{has_valid_session, https_redirect};

/// GET / — main page (unauthenticated requests are redirected to the login page).
pub(in crate::web) async fn serve_index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // force_https: ignored when TLS is disabled or the request is already HTTPS.
    // Cheap check here via the X-Forwarded-Proto header.
    if state.force_https && !state.tls_enabled {
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");
        if proto != "https" {
            return https_redirect(&headers, 7681); // default port
        }
    }

    if !has_valid_session(&state, &headers) {
        return redirect("/login");
    }
    serve_asset("index.html")
}

/// GET /login — login page.
pub(in crate::web) async fn serve_login(State(state): State<AppState>) -> Response {
    serve_login_html(&state)
}

/// Dynamically render the login HTML (control TOTP / OAuth button visibility).
fn serve_login_html(state: &AppState) -> Response {
    let oauth_button = if let Some(ref oauth_mgr) = state.oauth_mgr {
        // Generate the authorization URL so the OAuth button can be shown.
        match oauth_mgr.authorization_url() {
            Ok(url) => {
                let provider_label = "Sign in with OAuth";
                format!(
                    r#"<div class="oauth-section">
  <div class="or-divider"><span>or</span></div>
  <a href="{}" class="oauth-btn">{}</a>
</div>"#,
                    url, provider_label
                )
            }
            Err(e) => {
                warn!("OAuth URL generation error: {}", e);
                String::new()
            }
        }
    } else {
        String::new()
    };

    // Embed the OAuth section into the login.html template.
    let base_html = match Assets::get("login.html") {
        Some(file) => String::from_utf8_lossy(file.data.as_ref()).into_owned(),
        None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let html = base_html.replace("<!-- OAUTH_SECTION -->", &oauth_button);
    Html(html).into_response()
}

/// GET /setup — initial TOTP setup page.
pub(in crate::web) async fn serve_setup(State(state): State<AppState>) -> Response {
    if state
        .pending_setup
        .lock()
        .expect("pending_setup mutex poisoned")
        .is_none()
    {
        return redirect("/");
    }
    serve_asset("setup.html")
}
