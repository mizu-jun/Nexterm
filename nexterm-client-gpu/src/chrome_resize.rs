//! Edge hit-testing for the custom title bar's window-resize affordance.
//!
//! With `window.decorations = "notitle"` the OS chrome is gone, so the
//! client has to detect "the cursor is on the window outline" itself and
//! hand the actual resize loop to the OS via `Window::drag_resize_window`.
//! The math is kept in pure functions so it can be unit-tested without a
//! window.

use winit::window::{CursorIcon, ResizeDirection};

/// Hit-test the window outline for a resize edge.
///
/// `border_px` is the grab-band thickness in physical pixels (scale it by
/// the DPI factor at the call site). `chrome_h` is the tab-bar height;
/// `excluded_x` carries the window-button spans inside that strip, which
/// win over the resize band so the close button stays clickable in the
/// top-right corner. Returns `None` outside the window or off the bands.
pub(crate) fn resize_edge_at(
    px: f32,
    py: f32,
    win_w: f32,
    win_h: f32,
    border_px: f32,
    chrome_h: f32,
    excluded_x: &[(f32, f32)],
) -> Option<ResizeDirection> {
    if px < 0.0 || py < 0.0 || px >= win_w || py >= win_h {
        return None;
    }
    if py < chrome_h && excluded_x.iter().any(|&(x0, x1)| px >= x0 && px < x1) {
        return None;
    }
    let north = py < border_px;
    let south = py >= win_h - border_px;
    let west = px < border_px;
    let east = px >= win_w - border_px;
    match (north, south, west, east) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (true, _, _, true) => Some(ResizeDirection::NorthEast),
        (_, true, true, _) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::North),
        (_, true, _, _) => Some(ResizeDirection::South),
        (_, _, true, _) => Some(ResizeDirection::West),
        (_, _, _, true) => Some(ResizeDirection::East),
        _ => None,
    }
}

/// Cursor icon matching a resize direction.
pub(crate) fn resize_cursor(dir: ResizeDirection) -> CursorIcon {
    match dir {
        ResizeDirection::North => CursorIcon::NResize,
        ResizeDirection::South => CursorIcon::SResize,
        ResizeDirection::East => CursorIcon::EResize,
        ResizeDirection::West => CursorIcon::WResize,
        ResizeDirection::NorthEast => CursorIcon::NeResize,
        ResizeDirection::NorthWest => CursorIcon::NwResize,
        ResizeDirection::SouthEast => CursorIcon::SeResize,
        ResizeDirection::SouthWest => CursorIcon::SwResize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 800.0;
    const H: f32 = 600.0;
    const B: f32 = 6.0;
    const CHROME: f32 = 32.0;

    fn edge(px: f32, py: f32) -> Option<ResizeDirection> {
        resize_edge_at(px, py, W, H, B, CHROME, &[])
    }

    #[test]
    fn interior_is_not_an_edge() {
        assert_eq!(edge(400.0, 300.0), None);
        assert_eq!(edge(B, B), None);
        assert_eq!(edge(W - B - 1.0, H - B - 1.0), None);
    }

    #[test]
    fn outside_the_window_is_not_an_edge() {
        assert_eq!(edge(-1.0, 300.0), None);
        assert_eq!(edge(400.0, -0.5), None);
        assert_eq!(edge(W, 300.0), None);
        assert_eq!(edge(400.0, H), None);
    }

    #[test]
    fn cardinal_edges() {
        assert_eq!(edge(400.0, 0.0), Some(ResizeDirection::North));
        assert_eq!(edge(400.0, H - 1.0), Some(ResizeDirection::South));
        assert_eq!(edge(0.0, 300.0), Some(ResizeDirection::West));
        assert_eq!(edge(W - 1.0, 300.0), Some(ResizeDirection::East));
    }

    #[test]
    fn corners_win_over_cardinal_edges() {
        assert_eq!(edge(0.0, 0.0), Some(ResizeDirection::NorthWest));
        assert_eq!(edge(W - 1.0, 0.0), Some(ResizeDirection::NorthEast));
        assert_eq!(edge(0.0, H - 1.0), Some(ResizeDirection::SouthWest));
        assert_eq!(edge(W - 1.0, H - 1.0), Some(ResizeDirection::SouthEast));
    }

    #[test]
    fn band_boundary_is_exclusive() {
        assert_eq!(edge(400.0, B), None);
        assert_eq!(edge(400.0, B - 0.01), Some(ResizeDirection::North));
        assert_eq!(edge(W - B, 300.0), Some(ResizeDirection::East));
        assert_eq!(edge(W - B - 0.01, 300.0), None);
    }

    #[test]
    fn window_buttons_beat_the_north_and_east_bands() {
        // A close button occupying the top-right corner of the chrome strip.
        let buttons = [(W - 40.0, W)];
        // Inside the button span: no resize edge anywhere in the strip.
        assert_eq!(
            resize_edge_at(W - 10.0, 0.0, W, H, B, CHROME, &buttons),
            None
        );
        assert_eq!(
            resize_edge_at(W - 1.0, CHROME - 1.0, W, H, B, CHROME, &buttons),
            None
        );
        // Below the chrome strip the east band works again.
        assert_eq!(
            resize_edge_at(W - 1.0, CHROME + 1.0, W, H, B, CHROME, &buttons),
            Some(ResizeDirection::East)
        );
        // Left of the button span the north band still works.
        assert_eq!(
            resize_edge_at(W - 60.0, 0.0, W, H, B, CHROME, &buttons),
            Some(ResizeDirection::North)
        );
    }

    #[test]
    fn every_direction_maps_to_a_distinct_cursor() {
        use std::collections::HashSet;
        let dirs = [
            ResizeDirection::North,
            ResizeDirection::South,
            ResizeDirection::East,
            ResizeDirection::West,
            ResizeDirection::NorthEast,
            ResizeDirection::NorthWest,
            ResizeDirection::SouthEast,
            ResizeDirection::SouthWest,
        ];
        let cursors: HashSet<_> = dirs.iter().map(|&d| resize_cursor(d)).collect();
        assert_eq!(cursors.len(), dirs.len());
    }
}
