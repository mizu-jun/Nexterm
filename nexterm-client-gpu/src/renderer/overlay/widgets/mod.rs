//! Shared widget layer for the settings panel and other chrome surfaces
//! (UI/UX v3 phase P1b).
//!
//! Controls used to be defined three times over — draw code, hit-test, and
//! AccessKit tree — with comments asking future maintainers to keep the
//! copies in sync. This module replaces that with one immediate-mode
//! descriptor, [`spec::WidgetSpec`], rebuilt every frame and consumed by each
//! reader:
//!
//! | Reader | Entry point |
//! |---|---|
//! | Visuals | [`draw::draw_widget`] |
//! | Mouse routing | [`spec::hit_test`] |
//! | AccessKit | [`spec::WidgetDesc`] via `accessibility::widget_node` |
//!
//! [`tooltip`] is the first component built on top of the layer.
//!
//! Migration ran tab-by-tab and is complete: all nine settings categories
//! build specs. What deliberately stays outside the layer is the pair of
//! delete-confirmation modals (Ssh, Keybindings) — a modal over the whole
//! panel is not a settings row, so it keeps its hand-written nodes.

pub(crate) mod action;
pub(crate) mod draw;
pub(crate) mod geometry;
pub(crate) mod navigation;
pub(crate) mod settings_blocks;
pub(crate) mod settings_font;
pub(crate) mod settings_keybindings;
pub(crate) mod settings_profiles;
pub(crate) mod settings_security;
pub(crate) mod settings_ssh;
pub(crate) mod settings_startup;
pub(crate) mod settings_theme;
pub(crate) mod settings_window;
pub(crate) mod spec;
pub(crate) mod tooltip;
