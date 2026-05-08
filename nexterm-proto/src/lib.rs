//! nexterm-proto — IPC 通信プロトコル定義
//!
//! nexterm-server と nexterm-client 間で交わすメッセージ型を定義する。
//! bincode でシリアライズしてUnix Domain Socket / Named Pipe で転送する。
#![warn(missing_docs)]

/// ターミナルセル型定義（`Attrs`・`Cell`・`Color`）
pub mod cell;
/// 仮想グリッド型定義（`Grid`・`DirtyRow`・`HyperlinkSpan`）
pub mod grid;
/// IPC メッセージ型定義（`ClientToServer`・`ServerToClient`）
pub mod message;

pub use cell::{Attrs, Cell, Color};
pub use grid::{DirtyRow, Grid, HyperlinkSpan};
pub use message::{ClientToServer, KeyCode, Modifiers, PaneLayout, ServerToClient, SessionInfo, WindowInfo};
