//! HTML ページ配信ハンドラ: index / login / setup。

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

/// GET / — メイン画面（未認証はログインページへリダイレクト）
pub(in crate::web) async fn serve_index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // force_https: TLS 無効または既に HTTPS の場合は無視する
    // ここでは簡易チェック（X-Forwarded-Proto ヘッダーを確認）
    if state.force_https && !state.tls_enabled {
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");
        if proto != "https" {
            return https_redirect(&headers, 7681); // デフォルトポート
        }
    }

    if !has_valid_session(&state, &headers) {
        return redirect("/login");
    }
    serve_asset("index.html")
}

/// GET /login — ログインページ
pub(in crate::web) async fn serve_login(State(state): State<AppState>) -> Response {
    serve_login_html(&state)
}

/// ログインページ HTML を動的に生成する（TOTP / OAuth ボタンの表示制御）
fn serve_login_html(state: &AppState) -> Response {
    let oauth_button = if let Some(ref oauth_mgr) = state.oauth_mgr {
        // OAuth ボタンを表示するために認証 URL を生成する
        match oauth_mgr.authorization_url() {
            Ok(url) => {
                let provider_label = "OAuth でログイン";
                format!(
                    r#"<div class="oauth-section">
  <div class="or-divider"><span>または</span></div>
  <a href="{}" class="oauth-btn">{}</a>
</div>"#,
                    url, provider_label
                )
            }
            Err(e) => {
                warn!("OAuth URL 生成エラー: {}", e);
                String::new()
            }
        }
    } else {
        String::new()
    };

    // login.html テンプレートに OAuth セクションを埋め込む
    let base_html = match Assets::get("login.html") {
        Some(file) => String::from_utf8_lossy(file.data.as_ref()).into_owned(),
        None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let html = base_html.replace("<!-- OAUTH_SECTION -->", &oauth_button);
    Html(html).into_response()
}

/// GET /setup — 初回 TOTP セットアップページ
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
