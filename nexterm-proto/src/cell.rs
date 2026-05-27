//! Terminal cell type definitions.

use serde::{Deserialize, Serialize};

/// Character attributes for a cell (bit flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Attrs(pub u8);

impl Attrs {
    /// SGR 1: bold.
    pub const BOLD: u8 = 0b0000_0001;
    /// SGR 3: italic.
    pub const ITALIC: u8 = 0b0000_0010;
    /// SGR 4: underline.
    pub const UNDERLINE: u8 = 0b0000_0100;
    /// SGR 5: blink.
    pub const BLINK: u8 = 0b0000_1000;
    /// SGR 7: reverse video (swap foreground and background).
    pub const REVERSE: u8 = 0b0001_0000;
    /// SGR 9: strikethrough.
    pub const STRIKETHROUGH: u8 = 0b0010_0000;

    /// Returns whether the bold flag is set.
    pub fn is_bold(self) -> bool {
        self.0 & Self::BOLD != 0
    }
    /// Returns whether the italic flag is set.
    pub fn is_italic(self) -> bool {
        self.0 & Self::ITALIC != 0
    }
    /// Returns whether the underline flag is set.
    pub fn is_underline(self) -> bool {
        self.0 & Self::UNDERLINE != 0
    }
    /// Returns whether the reverse-video flag is set.
    pub fn is_reverse(self) -> bool {
        self.0 & Self::REVERSE != 0
    }
}

/// Terminal color (supports 256-color palette and TrueColor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Color {
    /// Default color (defers to the terminal's settings).
    #[default]
    Default,
    /// xterm 256-color index (0..=15: ANSI 16 colors, 16..=231: 6×6×6 color cube,
    /// 232..=255: grayscale ramp). Corresponds to SGR `38;5;N` / `48;5;N`.
    Indexed(u8),
    /// 24-bit TrueColor (8 bits per channel).
    /// Corresponds to SGR `38;2;R;G;B` / `48;2;R;G;B`.
    Rgb(u8, u8, u8),
}

/// A single cell in the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// Displayed character (a blank cell holds `' '`).
    pub ch: char,
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Character attributes.
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
    fn default_cell_is_a_blank() {
        let cell = Cell::default();
        assert_eq!(cell.ch, ' ');
        assert_eq!(cell.fg, Color::Default);
        assert_eq!(cell.bg, Color::Default);
        assert_eq!(cell.attrs.0, 0);
    }

    #[test]
    fn attrs_bit_flags_work_correctly() {
        let attrs = Attrs(Attrs::BOLD | Attrs::ITALIC);
        assert!(attrs.is_bold());
        assert!(attrs.is_italic());
        assert!(!attrs.is_underline());
    }

    #[test]
    fn cell_postcard_roundtrip() {
        let cell = Cell {
            ch: 'A',
            fg: Color::Rgb(255, 0, 0),
            bg: Color::Indexed(0),
            attrs: Attrs(Attrs::BOLD),
        };
        let encoded = postcard::to_stdvec(&cell).unwrap();
        let decoded: Cell = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(cell, decoded);
    }

    #[test]
    fn attrs_every_flag_individually() {
        assert!(Attrs(Attrs::BOLD).is_bold());
        assert!(!Attrs(Attrs::BOLD).is_italic());
        assert!(Attrs(Attrs::ITALIC).is_italic());
        assert!(Attrs(Attrs::UNDERLINE).is_underline());
        assert!(Attrs(Attrs::REVERSE).is_reverse());
        // BLINK and STRIKETHROUGH have no dedicated getter, so verify with raw bits.
        assert_ne!(Attrs(Attrs::BLINK).0 & Attrs::BLINK, 0);
        assert_ne!(Attrs(Attrs::STRIKETHROUGH).0 & Attrs::STRIKETHROUGH, 0);
    }

    #[test]
    fn attrs_combined_flags_via_bitwise_ops() {
        let all = Attrs(Attrs::BOLD | Attrs::ITALIC | Attrs::UNDERLINE | Attrs::REVERSE);
        assert!(all.is_bold());
        assert!(all.is_italic());
        assert!(all.is_underline());
        assert!(all.is_reverse());
    }

    #[test]
    fn color_variants_equality() {
        assert_eq!(Color::Default, Color::Default);
        assert_eq!(Color::Indexed(42), Color::Indexed(42));
        assert_ne!(Color::Indexed(42), Color::Indexed(43));
        assert_eq!(Color::Rgb(255, 128, 0), Color::Rgb(255, 128, 0));
        assert_ne!(Color::Rgb(255, 128, 0), Color::Rgb(0, 128, 255));
    }

    #[test]
    fn cell_roundtrip_with_a_cjk_character() {
        let cell = Cell {
            ch: '日',
            fg: Color::Default,
            bg: Color::Rgb(0, 0, 255),
            attrs: Attrs(Attrs::ITALIC),
        };
        let encoded = postcard::to_stdvec(&cell).unwrap();
        let decoded: Cell = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(cell, decoded);
        assert_eq!(decoded.ch, '日');
    }
}
