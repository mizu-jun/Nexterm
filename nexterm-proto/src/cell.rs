//! ターミナルセルの型定義

use serde::{Deserialize, Serialize};

/// セルの文字属性（ビットフラグ）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Attrs(pub u8);

impl Attrs {
    pub const BOLD: u8 = 0b0000_0001;
    pub const ITALIC: u8 = 0b0000_0010;
    pub const UNDERLINE: u8 = 0b0000_0100;
    pub const BLINK: u8 = 0b0000_1000;
    pub const REVERSE: u8 = 0b0001_0000;
    pub const STRIKETHROUGH: u8 = 0b0010_0000;

    pub fn is_bold(self) -> bool {
        self.0 & Self::BOLD != 0
    }
    pub fn is_italic(self) -> bool {
        self.0 & Self::ITALIC != 0
    }
    pub fn is_underline(self) -> bool {
        self.0 & Self::UNDERLINE != 0
    }
    pub fn is_reverse(self) -> bool {
        self.0 & Self::REVERSE != 0
    }
}

/// 端末カラー（256色 + TrueColor 対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum Color {
    /// デフォルト色（端末設定に従う）
    #[default]
    Default,
    /// ANSI 16色（0〜15）
    Indexed(u8),
    /// 24bit TrueColor
    Rgb(u8, u8, u8),
}


/// ターミナルの1セル
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// 表示文字（空白セルは ' '）
    pub ch: char,
    /// 前景色
    pub fg: Color,
    /// 背景色
    pub bg: Color,
    /// 文字属性
    pub attrs: Attrs,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Default,
            bg: Color::Default,
            attrs: Attrs::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn セルのデフォルト値は空白() {
        let cell = Cell::default();
        assert_eq!(cell.ch, ' ');
        assert_eq!(cell.fg, Color::Default);
        assert_eq!(cell.bg, Color::Default);
        assert_eq!(cell.attrs.0, 0);
    }

    #[test]
    fn attrs_ビットフラグが正しく動作する() {
        let attrs = Attrs(Attrs::BOLD | Attrs::ITALIC);
        assert!(attrs.is_bold());
        assert!(attrs.is_italic());
        assert!(!attrs.is_underline());
    }

    #[test]
    fn セルのbincodeシリアライズ往復() {
        let cell = Cell {
            ch: 'A',
            fg: Color::Rgb(255, 0, 0),
            bg: Color::Indexed(0),
            attrs: Attrs(Attrs::BOLD),
        };
        let encoded = bincode::serialize(&cell).unwrap();
        let decoded: Cell = bincode::deserialize(&encoded).unwrap();
        assert_eq!(cell, decoded);
    }
}
