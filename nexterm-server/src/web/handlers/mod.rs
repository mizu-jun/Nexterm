//! Web ターミナルの HTTP / WebSocket ハンドラ群（Sprint 5-4 / A3）。
//!
//! 旧 `web/mod.rs` (1,088 行) を以下に再分割した:
//!
//! - [`page`][] HTML 配信 (index / login / setup)
//! - [`login`][] TOTP ログイン / ログアウト / TOTP セットアップ
//! - [`oauth`][] OAuth2 リダイレクト / コールバック
//! - [`ws`][] WebSocket ハンドラ (PTY ブリッジ)
//! - [`assets`][] 静的ファイル配信 + リダイレクトレスポンス

pub(in crate::web) mod assets;
pub(in crate::web) mod login;
pub(in crate::web) mod oauth;
pub(in crate::web) mod page;
pub(in crate::web) mod ws;
