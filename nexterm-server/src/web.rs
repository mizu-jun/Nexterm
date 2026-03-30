//! Web ターミナル — axum WebSocket + xterm.js 埋め込み
//!
//! デフォルトは無効。`[web]` セクションで有効化する:
//! ```toml
//! [web]
//! enabled = true
//! port = 7681
//! token = "your-secret-token"  # 省略時は認証なし（LAN 限定推奨）
//! ```

use std::sync::Arc;

use axum::{
    Router,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::{Html, IntoResponse, Response},
    routing::get,
};
use nexterm_config::WebConfig;
use rust_embed::Embed;
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing::{info, warn};

use crate::session::SessionManager;

/// 埋め込み静的ファイル（nexterm-server/static/ ディレクトリを埋め込む）
#[derive(Embed)]
#[folder = "static/"]
struct Assets;

/// アプリケーション共有状態
#[derive(Clone)]
struct AppState {
    manager: Arc<SessionManager>,
    token: Option<String>,
}

/// WebSocket クエリパラメータ
#[derive(Deserialize)]
struct WsQuery {
    #[serde(default = "default_session_name")]
    session: String,
    #[serde(default)]
    token: String,
}

fn default_session_name() -> String {
    "main".to_string()
}

/// Web サーバーを起動する
pub async fn start_web_server(config: WebConfig, manager: Arc<SessionManager>) {
    let state = AppState {
        manager,
        token: config.token,
    };

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    info!("Web ターミナルを起動します: http://{}", addr);

    match TcpListener::bind(&addr).await {
        Ok(listener) => {
            if let Err(e) = axum::serve(listener, app).await {
                warn!("Web サーバーエラー: {}", e);
            }
        }
        Err(e) => {
            warn!("Web サーバーのバインドに失敗しました {}: {}", addr, e);
        }
    }
}

/// index.html を返す
async fn serve_index() -> impl IntoResponse {
    match Assets::get("index.html") {
        Some(file) => {
            let html = String::from_utf8_lossy(file.data.as_ref()).to_string();
            Html(html).into_response()
        }
        None => {
            axum::http::Response::builder()
                .status(404)
                .body(axum::body::Body::from("index.html not found"))
                .unwrap()
        }
    }
}

/// WebSocket ハンドラ — PTY セッションへのブリッジ
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> Response {
    // トークン検証
    if let Some(ref expected) = state.token {
        if query.token != *expected {
            warn!("WebSocket 認証失敗: 無効なトークン");
            return axum::http::Response::builder()
                .status(401)
                .body(axum::body::Body::from("Unauthorized"))
                .unwrap();
        }
    }

    let session_name = query.session.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, state.manager, session_name))
}

/// WebSocket 接続ごとの処理 — PTY 出力をブラウザに転送し、キー入力を PTY に転送する
async fn handle_socket(mut socket: WebSocket, manager: Arc<SessionManager>, session_name: String) {
    // セッションが存在しない場合は 80×24 で作成する
    let _ = manager.get_or_create_and_attach(&session_name, 80, 24).await;

    // セッションの broadcast channel を購読する
    let sessions_arc = manager.sessions();
    let mut rx = {
        let sessions = sessions_arc.lock().await;
        if let Some(session) = sessions.get(&session_name) {
            session.attach()
        } else {
            warn!("WebSocket: セッション '{}' が見つかりません", session_name);
            return;
        }
    };

    loop {
        tokio::select! {
            // PTY → ブラウザ
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Some(text) = pty_message_to_text(&msg) {
                            if socket.send(Message::Text(text)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // ラグが発生した場合は継続する
                    }
                }
            }
            // ブラウザ → PTY
            result = socket.recv() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        let sessions = sessions_arc.lock().await;
                        if let Some(session) = sessions.get(&session_name) {
                            let _ = session.write_to_focused(text.as_bytes());
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        let sessions = sessions_arc.lock().await;
                        if let Some(session) = sessions.get(&session_name) {
                            let _ = session.write_to_focused(&data);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    info!("WebSocket 切断: セッション '{}'", session_name);
}

/// ServerToClient メッセージからテキスト出力を抽出する
fn pty_message_to_text(msg: &nexterm_proto::ServerToClient) -> Option<String> {
    use nexterm_proto::ServerToClient;
    match msg {
        ServerToClient::GridDiff { dirty_rows, .. } => {
            let text: String = dirty_rows
                .iter()
                .map(|row| {
                    let line: String = row.cells.iter().map(|c| c.ch).collect();
                    format!("\r{}\r\n", line)
                })
                .collect();
            if text.is_empty() { None } else { Some(text) }
        }
        ServerToClient::FullRefresh { grid, .. } => {
            let text: String = grid.rows
                .iter()
                .map(|row| {
                    let line: String = row.iter().map(|c| c.ch).collect();
                    format!("{}\r\n", line)
                })
                .collect();
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}
