//! 仮想グリッド（W×H のセル配列）型定義

use serde::{Deserialize, Serialize};

use crate::Cell;

/// 差分転送用のダーティ行データ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyRow {
    /// 行インデックス（0始まり）
    pub row: u16,
    /// その行のセル配列（列0から列W-1まで）
    pub cells: Vec<Cell>,
}

/// OSC 8 ハイパーリンクのスパン情報
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HyperlinkSpan {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub url: String,
}

/// 画面全体のスナップショット（Full Refresh 用）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub width: u16,
    pub height: u16,
    /// 行優先の二次元配列（rows[y][x]）
    pub rows: Vec<Vec<Cell>>,
    /// カーソル位置
    pub cursor_col: u16,
    pub cursor_row: u16,
    /// OSC 8 ハイパーリンク（VT シーケンスで明示的に指定されたもの）
    #[serde(default)]
    pub hyperlinks: Vec<HyperlinkSpan>,
}

impl Grid {
    /// 空のグリッドを生成する
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

    /// 指定セルへアクセス（範囲外は None）
    pub fn get(&self, col: u16, row: u16) -> Option<&Cell> {
        self.rows
            .get(row as usize)
            .and_then(|r| r.get(col as usize))
    }

    /// 指定セルへ書き込み（範囲外は無視）
    pub fn set(&mut self, col: u16, row: u16, cell: Cell) {
        if let Some(r) = self.rows.get_mut(row as usize)
            && let Some(c) = r.get_mut(col as usize) {
                *c = cell;
            }
    }

    /// 行全体をデフォルトセルで埋める（範囲外は無視してパニックしない）
    pub fn clear_row(&mut self, row: u16) {
        if let Some(r) = self.rows.get_mut(row as usize) {
            r.iter_mut().for_each(|c| *c = Cell::default());
        }
    }

    /// src 行の内容を dst 行へコピーする（範囲外は無視してパニックしない）
    pub fn copy_row(&mut self, dst: u16, src: u16) {
        if dst == src {
            return;
        }
        // src 行を先にクローンしてから dst に書き込む（借用の競合を回避）
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
    fn グリッド生成とセルアクセス() {
        let grid = Grid::new(80, 24);
        assert_eq!(grid.width, 80);
        assert_eq!(grid.height, 24);
        // 全セルがデフォルト（空白）であること
        assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
        assert_eq!(grid.get(79, 23).unwrap().ch, ' ');
        assert!(grid.get(80, 0).is_none()); // 範囲外
    }

    #[test]
    fn グリッドのセット() {
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
    fn グリッドのbincode往復() {
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
        let encoded = bincode::serialize(&grid).unwrap();
        let decoded: Grid = bincode::deserialize(&encoded).unwrap();
        assert_eq!(grid, decoded);
    }

    #[test]
    fn 範囲外セットは無視される() {
        let mut grid = Grid::new(5, 5);
        // 範囲外への書き込みはパニックせず無視される
        grid.set(100, 100, Cell::default());
        // 既存セルが変わっていないこと
        assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn カーソル位置の初期値は0_0() {
        let grid = Grid::new(80, 24);
        assert_eq!(grid.cursor_col, 0);
        assert_eq!(grid.cursor_row, 0);
    }

    #[test]
    fn dirty_row_シリアライズ往復() {
        let row = DirtyRow {
            row: 3,
            cells: vec![
                Cell { ch: 'A', fg: Color::Default, bg: Color::Default, attrs: Attrs::default() },
                Cell { ch: 'B', fg: Color::Rgb(255, 0, 0), bg: Color::Default, attrs: Attrs(Attrs::BOLD) },
            ],
        };
        let encoded = bincode::serialize(&row).unwrap();
        let decoded: DirtyRow = bincode::deserialize(&encoded).unwrap();
        assert_eq!(row, decoded);
    }

    #[test]
    fn hyperlink_span_フィールド確認() {
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
