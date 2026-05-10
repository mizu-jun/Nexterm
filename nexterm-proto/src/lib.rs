//! nexterm-proto — IPC 通信プロトコル定義
//!
//! nexterm-server と nexterm-client 間で交わすメッセージ型を定義する。
//! postcard でシリアライズしてUnix Domain Socket / Named Pipe で転送する。
#![warn(missing_docs)]

/// ターミナルセル型定義（`Attrs`・`Cell`・`Color`）
pub mod cell;
/// 仮想グリッド型定義（`Grid`・`DirtyRow`・`HyperlinkSpan`）
pub mod grid;
/// IPC メッセージ型定義（`ClientToServer`・`ServerToClient`）
pub mod message;

pub use cell::{Attrs, Cell, Color};
pub use grid::{DirtyRow, Grid, HyperlinkSpan};
pub use message::{
    ClientKind, ClientToServer, KeyCode, Modifiers, PaneLayout, ServerToClient, SessionInfo,
    WindowInfo,
};

/// IPC プロトコルバージョン。
///
/// クライアントとサーバーが接続時に Hello メッセージで交換する。
/// バージョン不一致時はサーバーが接続を切断する。
///
/// # 履歴
///
/// - v1: 初版（PR-C Task 10 で導入）
/// - v2: Sprint 5-1 / G1 — `ConnectSsh` から `password: Option<String>` を削除し、
///   `password_keyring_account: Option<String>` + `ephemeral_password: bool` に置換。
///   IPC 経路で SSH パスワード平文が流れないようサーバー側 keyring 取得に統一。
/// - v3: Sprint 5-1 / G3 — IPC ワイヤフォーマットを `bincode` 1.x から
///   `postcard` 1.x に変更。RUSTSEC-2025-0141 (bincode 1.x の supply chain 状態) 対応。
///   varint エンコードでメッセージサイズが縮小する。
pub const PROTOCOL_VERSION: u32 = 3;

/// IPC メッセージ 1 件の最大サイズ（バイト数）。
///
/// 受信側はこの値を超える長さプレフィックスを受け取った時点で接続を切断する。
/// 64 MiB は `ImagePlaced` の最大ペイロード（4096×4096×4 = 64 MiB）+ メタデータを許容する設定。
/// 4 GiB の `vec![0u8; msg_len]` 確保による OOM 攻撃を防ぐ。
pub const MAX_MSG_LEN: usize = 64 * 1024 * 1024;

/// 受信した長さプレフィックスを検証する共通ヘルパー。
///
/// `MAX_MSG_LEN` を超える場合はエラーを返す。
///
/// # 引数
/// - `msg_len`: 受信した長さプレフィックス（バイト数）
///
/// # エラー
/// `msg_len` が `MAX_MSG_LEN` を超えた場合
pub fn validate_msg_len(msg_len: usize) -> std::result::Result<(), String> {
    if msg_len > MAX_MSG_LEN {
        Err(format!(
            "IPC メッセージサイズが上限を超えています: {} > {} バイト",
            msg_len, MAX_MSG_LEN
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 上限以内のサイズは許容される() {
        assert!(validate_msg_len(0).is_ok());
        assert!(validate_msg_len(1024).is_ok());
        assert!(validate_msg_len(MAX_MSG_LEN).is_ok());
    }

    #[test]
    fn 上限超過は拒否される() {
        assert!(validate_msg_len(MAX_MSG_LEN + 1).is_err());
        assert!(validate_msg_len(usize::MAX).is_err());
        assert!(validate_msg_len(u32::MAX as usize).is_err());
    }

    #[test]
    fn エラーメッセージにサイズが含まれる() {
        let err = validate_msg_len(MAX_MSG_LEN + 1).unwrap_err();
        assert!(err.contains(&format!("{}", MAX_MSG_LEN + 1)));
        assert!(err.contains(&format!("{}", MAX_MSG_LEN)));
    }
}
