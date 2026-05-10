//! デスクトップ通知ラッパー（Sprint 4-1）
//!
//! OSC 9 / 777 経由で要求された通知を、ユーザー同意後にプラットフォーム
//! ネイティブの通知 API（notify-rust 経由）で送信する。
//!
//! 送信失敗（D-Bus 不在・通知権限なし等）は警告ログのみで、アプリ全体は
//! クラッシュさせない。

use tracing::warn;

/// デスクトップ通知を送信する
///
/// # Arguments
/// - `title`: 通知タイトル（先頭に "Nexterm: " を付ける）
/// - `body`: 通知本文
///
/// 失敗した場合は warn ログのみ出力する。
pub fn send_notification(title: &str, body: &str) {
    let summary = format!("Nexterm: {title}");
    let result = notify_rust::Notification::new()
        .summary(&summary)
        .body(body)
        .appname("Nexterm")
        .show();
    if let Err(e) = result {
        warn!("デスクトップ通知の送信に失敗しました: {}", e);
    }
}
