//! `nexterm-ctl` subcommands (Sprint 5-4 / A1: extracted from `main.rs`).
//!
//! The original 1,757-line `main.rs` was split into the following modules:
//!
//! - [`session`][] list / new / attach / kill
//! - [`record`][] record start / stop
//! - [`template`][] template save / load / list
//! - [`service`][] systemd / launchd / Windows SCM
//! - [`ghostty`][] import-ghostty (Ghostty config conversion)
//! - [`theme`][] theme import (iTerm2 / Alacritty / Base16)
//! - [`plugin`][] WASM plugin management (list / load / unload / reload)
//! - [`workspace`][] workspace management (list / create / switch / rename / delete)
//! - [`quake`][] Quake mode control (toggle / show / hide) — for environments such as Wayland
//!   where the global hotkey cannot be registered

pub(crate) mod ghostty;
pub(crate) mod plugin;
pub(crate) mod quake;
pub(crate) mod record;
pub(crate) mod service;
pub(crate) mod session;
pub(crate) mod template;
pub(crate) mod theme;
pub(crate) mod util;
pub(crate) mod workspace;
pub(crate) mod wsl;
