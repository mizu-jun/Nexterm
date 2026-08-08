//! Panel geometry shared by every migrated settings tab.
//!
//! Lives outside any one tab module because all six of them — plus the mouse
//! hit-test — build the same value. It used to sit in `settings_theme`, which
//! made every other tab appear to depend on Theme for no reason.

/// The panel geometry a tab needs to lay its widgets out, in physical pixels.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TabGeometry {
    /// Top of the category content area.
    pub content_top: f32,
    /// Left edge of the content area's inner padding.
    pub content_inner_x: f32,
    /// Width of the content area.
    pub content_w: f32,
    /// Character cell width.
    pub cell_w: f32,
    /// Character cell height.
    pub cell_h: f32,
}
