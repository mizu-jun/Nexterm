//! Vertex builders for overlay UI (Sprint 5-4 / A2).
//!
//! The original `overlay_verts.rs` (1,958 lines) was split into 3 files by responsibility:
//!
//! - [`picker`]: command palette / SFTP file transfer / macro picker /
//!   host manager (list-style UI, 4 methods)
//! - [`dialog`]: password input modal / context menu / consent dialog
//!   (modal-style UI, 3 methods)
//! - [`settings`]: settings panel (tabbed, more complex UI, single method)
//! - [`util`]: shared helpers used by the consent dialog (pane_id extraction,
//!   preview formatting, text wrapping)
//!
//! Each submodule grows methods on `impl WgpuState`, so callers
//! (`renderer/mod.rs`) keep using `self.build_*_verts(...)` as before.

mod dialog;
mod key_hint;
mod picker;
mod settings;
mod util;
// `pub(in crate::renderer)` so `event_handler` can hit-test against the same
// widget specs the renderer draws, instead of recomputing the geometry.
pub(in crate::renderer) mod widgets;

// Phase B1: `render_frame.rs` needs this to apply the settings panel's
// measured content height back onto `SettingsPanel::scroll` and to scissor
// the scrollable content when drawing.
pub(in crate::renderer) use settings::SettingsPanelScrollMetrics;
