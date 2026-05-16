//! Quake モード制御コマンド (Sprint 5-7 / Phase 2-2): `quake toggle / show / hide`。
//!
//! Wayland など `global-hotkey` クレートが動作しない環境向けのワークアラウンドとして
//! 用意されている。サーバーに `ClientToServer::QuakeToggle { action }` を送信すると、
//! サーバーが接続中の全 GPU クライアントに `ServerToClient::QuakeToggleRequest` を
//! ブロードキャストして実際のウィンドウ操作を依頼する。
//!
//! 想定運用（Wayland 上の Sway / Hyprland）:
//!
//! ```text
//! # ~/.config/sway/config
//! bindsym Ctrl+grave exec nexterm-ctl quake toggle
//! ```

use anyhow::Result;
use nexterm_proto::ClientToServer;

use crate::ipc::IpcConn;

async fn send_action(action: &str) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::QuakeToggle {
        action: action.to_string(),
    })
    .await?;
    // サーバーは応答メッセージを返さない（全クライアントへのブロードキャストのみ）。
    // 送信できれば成功とみなす。
    println!("Quake モードに '{}' を送信しました", action);
    Ok(())
}

/// Quake モード表示状態をトグルする
pub(crate) async fn cmd_quake_toggle() -> Result<()> {
    send_action("toggle").await
}

/// Quake モードを表示する（既に表示中なら no-op）
pub(crate) async fn cmd_quake_show() -> Result<()> {
    send_action("show").await
}

/// Quake モードを非表示にする（既に非表示なら no-op）
pub(crate) async fn cmd_quake_hide() -> Result<()> {
    send_action("hide").await
}
