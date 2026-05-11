//! TOTP ログイン / ログアウト / 初回 TOTP セットアップハンドラ。

use axum::{
    Form, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::assets::redirect;
use crate::web::AppState;
use crate::web::access_log;
use crate::web::auth;
use crate::web::middleware::client_ip;
use crate::web::otp;

/// ログインフォームのフィールド
#[derive(Deserialize)]
pub(in crate::web) struct LoginForm {
    code: String,
}

/// POST /auth/login — TOTP コードを検証してセッションを発行する
pub(in crate::web) async fn handle_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let addr = client_ip(&headers);

    // レート制限: 同一 IP から 60 秒に 5 回までしか試行を許さない
    // （TOTP ブルートフォース対策、CRITICAL #2）
    if !state.totp_rate_limiter.check_and_record(&addr) {
        warn!("TOTP ログインレート制限超過（{}）", addr);
        state.access_logger.log(&access_log::AccessLogEntry {
            remote_addr: addr.clone(),
            method: "POST".to_string(),
            path: "/auth/login".to_string(),
            status: 429,
            auth_method: "totp".to_string(),
            user_id: String::new(),
        });
        return Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("Retry-After", "60")
            .body(axum::body::Body::from(
                "Too many failed attempts. Try again in 60 seconds.",
            ))
            .unwrap_or_else(|_| redirect("/login?error=rate_limited"));
    }

    let totp_guard = state.totp.read().await;
    let totp = match totp_guard.as_ref() {
        Some(t) => t,
        None => {
            state.access_logger.log(&access_log::AccessLogEntry {
                remote_addr: addr.clone(),
                method: "POST".to_string(),
                path: "/auth/login".to_string(),
                status: 302,
                auth_method: "totp".to_string(),
                user_id: String::new(),
            });
            return redirect("/login?error=not_configured");
        }
    };

    if !totp.verify(&form.code) {
        warn!("TOTP ログイン失敗: 無効なコード（{}）", addr);
        state.access_logger.log(&access_log::AccessLogEntry {
            remote_addr: addr.clone(),
            method: "POST".to_string(),
            path: "/auth/login".to_string(),
            status: 401,
            auth_method: "totp".to_string(),
            user_id: String::new(),
        });
        return redirect("/login?error=invalid_code");
    }

    // 認証成功: レート制限カウンタをリセット（正規ユーザーをペナルティから解放）
    state.totp_rate_limiter.reset(&addr);

    let token = match state.auth_mgr.create_session("totp", "") {
        Some(t) => t,
        None => return redirect("/login?error=session_limit"),
    };
    let cookie = auth::make_session_cookie(&token, state.tls_enabled);

    info!("TOTP ログイン成功（{}）", addr);
    state.access_logger.log(&access_log::AccessLogEntry {
        remote_addr: addr,
        method: "POST".to_string(),
        path: "/auth/login".to_string(),
        status: 302,
        auth_method: "totp".to_string(),
        user_id: String::new(),
    });

    Response::builder()
        .status(302)
        .header("Location", "/")
        .header("Set-Cookie", cookie)
        .body(axum::body::Body::empty())
        .expect("Response::builder への無効なヘッダー値")
}

/// POST /auth/logout — セッションを破棄してログインページへリダイレクト
pub(in crate::web) async fn handle_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Some(token) = auth::extract_session_cookie(&headers) {
        state.auth_mgr.revoke_session(&token);
    }
    Response::builder()
        .status(302)
        .header("Location", "/login")
        .header("Set-Cookie", auth::make_logout_cookie())
        .body(axum::body::Body::empty())
        .expect("logout redirect レスポンスの構築に失敗")
}

/// セットアップ URL レスポンス
#[derive(Serialize)]
struct SetupUrlResponse {
    url: String,
    secret: String,
}

/// GET /auth/setup-url — セットアップ用の otpauth:// URL とシークレットを返す
pub(in crate::web) async fn handle_setup_url(State(state): State<AppState>) -> Response {
    let guard = state
        .pending_setup
        .lock()
        .expect("pending_setup mutex poisoned");
    match guard.as_ref() {
        Some(ps) => Json(SetupUrlResponse {
            url: ps.totp.get_url(),
            secret: ps.totp.secret_b32().to_string(),
        })
        .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// POST /setup/verify — 初回 TOTP コードを検証してシークレットを保存する
pub(in crate::web) async fn handle_setup_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let addr = client_ip(&headers);
    let (secret_clone, is_valid) = {
        let guard = state
            .pending_setup
            .lock()
            .expect("pending_setup mutex poisoned");
        match guard.as_ref() {
            Some(ps) => (ps.secret.clone(), ps.totp.verify(&form.code)),
            None => return redirect("/?setup=done"),
        }
    };

    if !is_valid {
        return redirect("/setup?error=invalid_code");
    }

    if let Err(e) = otp::save_secret_to_config(&secret_clone) {
        warn!(
            "TOTP シークレットの保存に失敗: {}。インメモリのみで動作します。",
            e
        );
    }

    match otp::TotpManager::from_secret(&secret_clone, &state.issuer) {
        Ok(mgr) => {
            *state.totp.write().await = Some(mgr);
            *state
                .pending_setup
                .lock()
                .expect("pending_setup mutex poisoned") = None;
            info!("TOTP セットアップが完了しました（{}）", addr);
        }
        Err(e) => {
            warn!("TOTP マネージャーの作成に失敗: {}", e);
            return redirect("/setup?error=internal");
        }
    }

    let token = match state.auth_mgr.create_session("totp", "") {
        Some(t) => t,
        None => return redirect("/login?error=session_limit"),
    };
    let cookie = auth::make_session_cookie(&token, state.tls_enabled);
    Response::builder()
        .status(302)
        .header("Location", "/")
        .header("Set-Cookie", cookie)
        .body(axum::body::Body::empty())
        .expect("Response::builder への無効なヘッダー値")
}
