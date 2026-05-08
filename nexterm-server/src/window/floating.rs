//! フローティングペインの矩形情報

/// フローティングペインの矩形情報
#[derive(Clone, Debug)]
pub struct FloatRect {
    pub col_off: u16,
    pub row_off: u16,
    pub cols: u16,
    pub rows: u16,
}
