//! nexterm-vt — VT シーケンスパーサ + 仮想グリッド実装
//!
//! vte クレートを使って端末エスケープシーケンスをパースし、
//! Cell の二次元配列（仮想グリッド）に反映する。

pub mod image;
mod performer;
mod screen;

pub use screen::{PendingImage, Screen};

/// VT シーケンスを処理してグリッドを更新するパーサ
pub struct VtParser {
    parser: vte::Parser,
    screen: Screen,
}

impl VtParser {
    /// 指定サイズの仮想スクリーンを持つパーサを生成する
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            parser: vte::Parser::new(),
            screen: Screen::new(cols, rows),
        }
    }

    /// バイト列を処理してグリッドを更新する
    pub fn advance(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.parser.advance(&mut self.screen, byte);
        }
    }

    /// 現在のスクリーン状態への参照を返す
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// 現在のスクリーン状態への可変参照を返す
    pub fn screen_mut(&mut self) -> &mut Screen {
        &mut self.screen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 通常文字を書き込める() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Hello");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'H');
        assert_eq!(grid.get(1, 0).unwrap().ch, 'e');
        assert_eq!(grid.get(4, 0).unwrap().ch, 'o');
    }

    #[test]
    fn キャリッジリターン改行でカーソルが移動する() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Line1\r\nLine2");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'L');
        assert_eq!(grid.get(0, 1).unwrap().ch, 'L');
    }

    #[test]
    fn カーソル位置指定エスケープが動作する() {
        let mut parser = VtParser::new(80, 24);
        // CSI 5;10H → 行5列10へ移動（1始まり）
        parser.advance(b"\x1b[5;10HA");
        let grid = parser.screen().grid();
        // 行4列9（0始まり）に 'A' が書かれる
        assert_eq!(grid.get(9, 4).unwrap().ch, 'A');
    }

    #[test]
    fn ダーティフラグが書き込みで立つ() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"X");
        let screen = parser.screen();
        assert!(screen.is_dirty(0), "行0はダーティであるべき");
        assert!(!screen.is_dirty(1), "行1はクリーンであるべき");
    }

    #[test]
    fn ダーティフラグをクリアできる() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"X");
        parser.screen_mut().clear_dirty();
        assert!(!parser.screen().is_dirty(0));
    }

    #[test]
    fn リサイズで新しいサイズに変わる() {
        let mut parser = VtParser::new(80, 24);
        parser.screen_mut().resize(120, 40);
        let grid = parser.screen().grid();
        assert_eq!(grid.width, 120);
        assert_eq!(grid.height, 40);
    }
}
