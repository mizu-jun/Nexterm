//! Web terminal HTTP / WebSocket handlers (Sprint 5-4 / A3).
//!
//! Splits the old `web/mod.rs` (1,088 lines) into:
//!
//! - [`page`][] HTML serving (index / login / setup).
//! - [`login`][] TOTP login / logout / TOTP setup.
//! - [`oauth`][] OAuth2 redirect / callback.
//! - [`ws`][] WebSocket handler (PTY bridge).
//! - [`assets`][] static-file serving and redirect responses.

pub(in crate::web) mod assets;
pub(in crate::web) mod login;
pub(in crate::web) mod oauth;
pub(in crate::web) mod page;
pub(in crate::web) mod ws;
