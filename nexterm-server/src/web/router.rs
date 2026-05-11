//! Web ターミナルのルーター構築 + HTTP/HTTPS サーバー起動。

use std::net::SocketAddr;

use axum::{
    Router,
    routing::{get, post},
};
use tokio::net::TcpListener;
use tracing::warn;

use super::AppState;
use super::handlers;

/// ルーターを構築する
pub(in crate::web) fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::page::serve_index))
        .route("/login", get(handlers::page::serve_login))
        .route("/setup", get(handlers::page::serve_setup))
        .route("/auth/login", post(handlers::login::handle_login))
        .route("/auth/oauth", get(handlers::oauth::handle_oauth_redirect))
        .route(
            "/auth/callback",
            get(handlers::oauth::handle_oauth_callback),
        )
        .route("/auth/logout", post(handlers::login::handle_logout))
        .route("/auth/setup-url", get(handlers::login::handle_setup_url))
        .route("/setup/verify", post(handlers::login::handle_setup_verify))
        .route("/ws", get(handlers::ws::ws_handler))
        .with_state(state)
}

/// 平文 HTTP サーバーを起動する
pub(in crate::web) async fn start_plain_http(addr: SocketAddr, app: Router) {
    match TcpListener::bind(&addr).await {
        Ok(listener) => {
            if let Err(e) = axum::serve(listener, app).await {
                warn!("Web サーバーエラー: {}", e);
            }
        }
        Err(e) => {
            warn!("Web サーバーのバインドに失敗: {}: {}", addr, e);
        }
    }
}

/// TLS (HTTPS) サーバーを起動する
pub(in crate::web) async fn start_tls_server(
    addr: SocketAddr,
    app: Router,
    cert_pem: Vec<u8>,
    key_pem: Vec<u8>,
) {
    use hyper_util::{
        rt::{TokioExecutor, TokioIo},
        server::conn::auto::Builder,
        service::TowerToHyperService,
    };
    use std::sync::Arc;

    // PEM 証明書を解析する
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = {
        let mut reader = std::io::BufReader::new(cert_pem.as_slice());
        rustls_pemfile::certs(&mut reader)
            .filter_map(|r| r.ok())
            .collect()
    };
    let private_key = {
        let mut reader = std::io::BufReader::new(key_pem.as_slice());
        match rustls_pemfile::private_key(&mut reader) {
            Ok(Some(k)) => k,
            _ => {
                warn!("TLS: 秘密鍵の解析に失敗しました。HTTP にフォールバックします。");
                start_plain_http(addr, app).await;
                return;
            }
        }
    };

    let tls_config = match rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, private_key)
    {
        Ok(c) => Arc::new(c),
        Err(e) => {
            warn!("TLS 設定エラー: {}。HTTP にフォールバックします。", e);
            start_plain_http(addr, app).await;
            return;
        }
    };

    let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            warn!("TLS バインドに失敗: {}: {}", addr, e);
            return;
        }
    };

    loop {
        let (tcp_stream, _remote_addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                warn!("TCP accept エラー: {}", e);
                continue;
            }
        };

        let acceptor = acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(tcp_stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TLS ハンドシェイクエラー: {}", e);
                    return;
                }
            };
            let io = TokioIo::new(tls_stream);
            let service = TowerToHyperService::new(app);
            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(io, service)
                .await
            {
                tracing::debug!("HTTP 接続エラー: {}", e);
            }
        });
    }
}
