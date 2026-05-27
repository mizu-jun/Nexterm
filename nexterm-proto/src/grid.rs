//! Virtual grid (a W×H array of cells) type definitions.

use serde::{Deserialize, Serialize};

use crate::Cell;

/// A dirty row used for differential updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyRow {
    /// Row index (0-based).
    pub row: u16,
    /// Cells in this row (from column 0 to column W-1).
    pub cells: Vec<Cell>,
}

/// OSC 8 hyperlink span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HyperlinkSpan {
    /// Row index (0-based).
    pub row: u16,
    /// Start column of the link (0-based, inclusive).
    pub col_start: u16,
    /// End column of the link (0-based, exclusive).
    pub col_end: u16,
    /// Link target URL.
    pub url: String,
}

/// Full-screen snapshot (used for a full refresh).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    /// Number of columns in the grid (in characters).
    pub width: u16,
    /// Number of rows in the grid (in characters).
    pub height: u16,
    /// Row-major 2D array (`rows[y][x]`).
    pub rows: Vec<Vec<Cell>>,
    /// Cursor column (0-based).
    pub cursor_col: u16,
    /// Cursor row (0-based).
    pub cursor_row: u16,
    /// OSC 8 hyperlinks declared explicitly via VT sequences.
    #[serde(default)]
    pub hyperlinks: Vec<HyperlinkSpan>,
}

impl Grid {
    /// Creates an empty grid.
    pub fn new(width: u16, height: u16) -> Self {
        let rows = vec![vec![Cell::default(); width as usize]; height as usize];
        Self {
            width,
            height,
            rows,
            cursor_col: 0,
            cursor_row: 0,
            hyperlinks: Vec::new(),
        }
    }

    /// Returns the cell at the given position (or `None` if out of bounds).
    pub fn get(&self, col: u16, row: u16) -> Option<&Cell> {
        self.rows
            .get(row as usize)
            .and_then(|r| r.get(col as usize))
    }

    /// Writes to the given cell (out-of-bounds writes are ignored).
    pub fn set(&mut self, col: u16, row: u16, cell: Cell) {
        if let Some(r) = self.rows.get_mut(row as usize)
            && let Some(c) = r.get_mut(col as usize)
        {
            *c = cell;
        }
    }

    /// Fills the entire row with default cells (out-of-bounds rows are ignored
    /// instead of panicking).
    pub fn clear_row(&mut self, row: u16) {
        if let Some(r) = self.rows.get_mut(row as usize) {
            r.iter_mut().for_each(|c| *c = Cell::default());
        }
    }

    /// Copies the contents of row `src` into row `dst` (out-of-bounds indices are
    /// ignored instead of panicking).
    pub fn copy_row(&mut self, dst: u16, src: u16) {
        if dst == src {
            return;
        }
        // Clone the source row first, then write into the destination, to sidestep
        // the borrow-checker conflict.
        let src_cells = match self.rows.get(src as usize) {
            Some(r) => r.clone(),
            None => return,
        };
        if let Some(dst_row) = self.rows.get_mut(dst as usize) {
            *dst_row = src_cells;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attrs, Color};

    #[test]
    fn grid_creation_and_cell_access() {
        let grid = Grid::new(80, 24);
        assert_eq!(grid.width, 80);
        assert_eq!(grid.height, 24);
        // Every cell should be the default (blank).
        assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
        assert_eq!(grid.get(79, 23).unwrap().ch, ' ');
        assert!(grid.get(80, 0).is_none()); // out of bounds
    }

    #[test]
    fn grid_set() {
        let mut grid = Grid::new(80, 24);
        let cell = Cell {
            ch: 'X',
            fg: Color::Rgb(0, 255, 0),
            bg: Color::Default,
            attrs: Attrs::default(),
        };
        grid.set(10, 5, cell.clone());
        assert_eq!(grid.get(10, 5).unwrap(), &cell);
    }

    #[test]
    fn grid_postcard_roundtrip() {
        let mut grid = Grid::new(10, 5);
        grid.set(
            3,
            2,
            Cell {
                ch: 'Z',
                fg: Color::Indexed(1),
                bg: Color::Default,
                attrs: Attrs(Attrs::BOLD),
            },
        );
        let encoded = postcard::to_stdvec(&grid).unwrap();
        let decoded: Grid = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(grid, decoded);
    }

    #[test]
    fn out_of_bounds_set_is_ignored() {
        let mut grid = Grid::new(5, 5);
        // Out-of-bounds writes must not panic; they are silently dropped.
        grid.set(100, 100, Cell::default());
        // Existing cells must remain untouched.
        assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn cursor_position_defaults_to_origin() {
        let grid = Grid::new(80, 24);
        assert_eq!(grid.cursor_col, 0);
        assert_eq!(grid.cursor_row, 0);
    }

    #[test]
    fn dirty_row_postcard_roundtrip() {
        let row = DirtyRow {
            row: 3,
            cells: vec![
                Cell {
                    ch: 'A',
                    fg: Color::Default,
                    bg: Color::Default,
                    attrs: Attrs::default(),
                },
                Cell {
                    ch: 'B',
                    fg: Color::Rgb(255, 0, 0),
                    bg: Color::Default,
                    attrs: Attrs(Attrs::BOLD),
                },
            ],
        };
        let encoded = postcard::to_stdvec(&row).unwrap();
        let decoded: DirtyRow = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(row, decoded);
    }

    #[test]
    fn hyperlink_span_field_check() {
        let span = HyperlinkSpan {
            row: 1,
            col_start: 5,
            col_end: 15,
            url: "https://example.com".to_string(),
        };
        assert_eq!(span.url, "https://example.com");
        assert_eq!(span.col_end - span.col_start, 10);
    }
}
