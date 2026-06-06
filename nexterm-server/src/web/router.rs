//! Web terminal router construction and HTTP/HTTPS server startup.

use std::net::SocketAddr;

use axum::{
    Router, middleware,
    routing::{get, post},
};
use tokio::net::TcpListener;
use tracing::warn;

use super::AppState;
use super::handlers;
use super::middleware as mw;

/// Build the router.
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
        .layer(middleware::from_fn_with_state(
            state.clone(),
            mw::security_headers,
        ))
        .with_state(state)
}

/// Start a plain HTTP server.
pub(in crate::web) async fn start_plain_http(addr: SocketAddr, app: Router) {
    match TcpListener::bind(&addr).await {
        Ok(listener) => {
            if let Err(e) = axum::serve(listener, app).await {
                warn!("web server error: {}", e);
            }
        }
        Err(e) => {
            warn!("failed to bind web server: {}: {}", addr, e);
        }
    }
}

/// Start a TLS (HTTPS) server.
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

    // Parse the PEM certificate.
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
                warn!("TLS: failed to parse private key; falling back to HTTP.");
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
            warn!("TLS config error: {}; falling back to HTTP.", e);
            start_plain_http(addr, app).await;
            return;
        }
    };

    let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            warn!("failed to bind TLS: {}: {}", addr, e);
            return;
        }
    };

    loop {
        let (tcp_stream, _remote_addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                warn!("TCP accept error: {}", e);
                continue;
            }
        };

        let acceptor = acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(tcp_stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TLS handshake error: {}", e);
                    return;
                }
            };
            let io = TokioIo::new(tls_stream);
            let service = TowerToHyperService::new(app);
            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(io, service)
                .await
            {
                tracing::debug!("HTTP connection error: {}", e);
            }
        });
    }
}
