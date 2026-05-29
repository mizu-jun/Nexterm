//! WebSocket handler — bridge to a PTY session.

use std::sync::Arc;

use axum::{
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{info, warn};

use crate::session::SessionManager;
use crate::web::AppState;
use crate::web::access_log;
use crate::web::auth;
use crate::web::middleware::{client_ip, has_valid_session};

/// WebSocket query parameters.
#[derive(Deserialize)]
pub(in crate::web) struct WsQuery {
    #[serde(default = "default_session_name")]
    session: String,
    #[serde(default)]
    token: String,
}

fn default_session_name() -> String {
    "main".to_string()
}

/// GET /ws — WebSocket handler (bridges to a PTY session).
pub(in crate::web) async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let addr = client_ip(&headers);

    // Session check.
    if !has_valid_session(&state, &headers) {
        state.access_logger.log(&access_log::AccessLogEntry {
            remote_addr: addr.clone(),
            method: "GET".to_string(),
            path: "/ws".to_string(),
            status: 401,
            auth_method: String::new(),
            user_id: String::new(),
        });
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Backward compatibility: validate the legacy query-parameter token (HIGH H-2: constant-time compare).
    if let Some(ref expected) = state.legacy_token {
        use subtle::ConstantTimeEq;
        // Compare in constant time including the length (prevents side-channel via short-circuit).
        let provided_bytes = query.token.as_bytes();
        let expected_bytes = expected.as_bytes();
        // `ct_eq` is only meaningful when the lengths match, but we still perform a byte compare
        // even on length mismatch to minimize length leakage.
        let len_match = provided_bytes.len() == expected_bytes.len();
        let bytes_match = if len_match {
            provided_bytes.ct_eq(expected_bytes).unwrap_u8() == 1
        } else {
            // Even on length mismatch, run the same amount of work using a buffer of the expected length.
            let dummy = vec![0u8; expected_bytes.len()];
            let _ = dummy.ct_eq(expected_bytes).unwrap_u8();
            false
        };
        if !(len_match && bytes_match) {
            warn!("WebSocket auth failure: invalid token ({})", addr);
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    // Record the WebSocket connection in the access log.
    let (auth_method, user_id) = auth::extract_session_cookie(&headers)
        .and_then(|t| state.auth_mgr.session_info(&t))
        .unwrap_or_default();

    state.access_logger.log(&access_log::AccessLogEntry {
        remote_addr: addr,
        method: "GET".to_string(),
        path: "/ws".to_string(),
        status: 101,
        auth_method,
        user_id,
    });

    let session_name = query.session.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, state.manager, session_name))
}

/// Per-connection handler — forward PTY output to the browser and key input to the PTY.
async fn handle_socket(mut socket: WebSocket, manager: Arc<SessionManager>, session_name: String) {
    let _ = manager
        .get_or_create_and_attach(&session_name, 80, 24)
        .await;

    let sessions_arc = manager.sessions();
    let mut rx = {
        let sessions = sessions_arc.lock().await;
        if let Some(session) = sessions.get(&session_name) {
            session.attach()
        } else {
            warn!("WebSocket: session '{}' not found", session_name);
            return;
        }
    };

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Some(text) = pty_message_to_text(&msg)
                            && socket.send(Message::Text(text)).await.is_err() {
                                break;
                            }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
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

    info!("WebSocket disconnected: session '{}'", session_name);
}

/// Extract text output from a `ServerToClient` message.
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
            let text: String = grid
                .rows
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_session_name_returns_main() {
        assert_eq!(default_session_name(), "main");
    }
}
