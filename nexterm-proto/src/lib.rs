//! nexterm-proto — IPC 通信プロトコル定義
//!
//! nexterm-server と nexterm-client 間で交わすメッセージ型を定義する。
//! bincode でシリアライズしてUnix Domain Socket / Named Pipe で転送する。

pub mod cell;
pub mod grid;
pub mod message;

pub use cell::{Attrs, Cell, Color};
pub use grid::{DirtyRow, Grid};
pub use message::{ClientToServer, KeyCode, Modifiers, PaneLayout, ServerToClient, SessionInfo};
