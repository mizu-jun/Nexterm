//! nexterm-ctl サブコマンド群（Sprint 5-4 / A1: main.rs から抽出）。
//!
//! 旧 `main.rs` (1,757 行) を以下に再分割した:
//!
//! - [`session`][] list / new / attach / kill
//! - [`record`][] record start / stop
//! - [`template`][] template save / load / list
//! - [`service`][] systemd / launchd / Windows SCM
//! - [`ghostty`][] import-ghostty (Ghostty 設定の変換)
//! - [`theme`][] theme import (iTerm2 / Alacritty / Base16)
//! - [`plugin`][] WASM プラグイン管理 (list / load / unload / reload)
//! - [`workspace`][] ワークスペース管理 (list / create / switch / rename / delete)
//! - [`quake`][] Quake モード制御 (toggle / show / hide) — Wayland 等の global-hotkey 非対応環境向け

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
