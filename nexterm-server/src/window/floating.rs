//! Rectangle information for floating panes.

/// Rectangle information for a floating pane.
#[derive(Clone, Debug)]
pub struct FloatRect {
    pub col_off: u16,
    pub row_off: u16,
    pub cols: u16,
    pub rows: u16,
}
