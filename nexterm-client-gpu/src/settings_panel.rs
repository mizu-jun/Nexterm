//! Compatibility shim.
//!
//! The settings panel implementation was split out of this single file into
//! `settings/` (Phase B6 mechanical refactor: many small files instead of one
//! ~5,400-line file). This module just re-exports everything so existing
//! `crate::settings_panel::X` call sites throughout the crate keep working
//! unchanged. New code should reference `crate::settings::X` directly.

pub use crate::settings::*;
