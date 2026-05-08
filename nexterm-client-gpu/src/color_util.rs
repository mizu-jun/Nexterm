//! 色変換ユーティリティ — ANSI 256 色・16 進数カラー文字列の変換

/// `nexterm_proto::Color` を RGBA [0, 1] に変換する
///
/// `is_fg` が true の場合は前景色、false の場合は背景色として Default を解決する。
/// `palette` が指定されている場合はスキームパレットを優先して参照する。
pub(crate) fn resolve_color(
    color: &nexterm_proto::Color,
    is_fg: bool,
    palette: Option<&nexterm_config::SchemePalette>,
) -> [f32; 4] {
    use nexterm_proto::Color;
    let u8_to_f32 = |v: u8| v as f32 / 255.0;
    match color {
        Color::Default => {
            if let Some(p) = palette {
                let c = if is_fg { p.fg } else { p.bg };
                [u8_to_f32(c[0]), u8_to_f32(c[1]), u8_to_f32(c[2]), 1.0]
            } else if is_fg {
                [0.85, 0.85, 0.85, 1.0]
            } else {
                [0.05, 0.05, 0.05, 1.0]
            }
        }
        Color::Rgb(r, g, b) => {
            [u8_to_f32(*r), u8_to_f32(*g), u8_to_f32(*b), 1.0]
        }
        Color::Indexed(n) => {
            // ANSI 0-15: スキームパレットを優先する
            if *n < 16
                && let Some(p) = palette {
                    let c = p.ansi[*n as usize];
                    return [u8_to_f32(c[0]), u8_to_f32(c[1]), u8_to_f32(c[2]), 1.0];
                }
            ansi_256_to_rgb(*n)
        }
    }
}

/// ANSI 256 色パレット → RGBA [0, 1]
pub(crate) fn ansi_256_to_rgb(n: u8) -> [f32; 4] {
    // 基本 16 色（簡易実装）
    const BASIC: [[f32; 3]; 16] = [
        [0.0, 0.0, 0.0],       // 0: black
        [0.502, 0.0, 0.0],     // 1: red
        [0.0, 0.502, 0.0],     // 2: green
        [0.502, 0.502, 0.0],   // 3: yellow
        [0.0, 0.0, 0.502],     // 4: blue
        [0.502, 0.0, 0.502],   // 5: magenta
        [0.0, 0.502, 0.502],   // 6: cyan
        [0.753, 0.753, 0.753], // 7: white
        [0.502, 0.502, 0.502], // 8: bright black
        [1.0, 0.0, 0.0],       // 9: bright red
        [0.0, 1.0, 0.0],       // 10: bright green
        [1.0, 1.0, 0.0],       // 11: bright yellow
        [0.0, 0.0, 1.0],       // 12: bright blue
        [1.0, 0.0, 1.0],       // 13: bright magenta
        [0.0, 1.0, 1.0],       // 14: bright cyan
        [1.0, 1.0, 1.0],       // 15: bright white
    ];

    if (n as usize) < BASIC.len() {
        let c = BASIC[n as usize];
        return [c[0], c[1], c[2], 1.0];
    }

    // 216 色キューブ（16〜231）
    if (16..=231).contains(&n) {
        let idx = n - 16;
        let b = (idx % 6) as f32 / 5.0;
        let g = ((idx / 6) % 6) as f32 / 5.0;
        let r = ((idx / 36) % 6) as f32 / 5.0;
        return [r, g, b, 1.0];
    }

    // グレースケール（232〜255）
    let grey = (n - 232) as f32 / 23.0;
    [grey, grey, grey, 1.0]
}

/// `#rrggbb` 形式の16進カラー文字列を `[f32; 4]` RGBA に変換する
pub(crate) fn hex_to_rgba(hex: &str, alpha: f32) -> [f32; 4] {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(hex.get(0..2).unwrap_or("80"), 16).unwrap_or(128) as f32 / 255.0;
    let g = u8::from_str_radix(hex.get(2..4).unwrap_or("80"), 16).unwrap_or(128) as f32 / 255.0;
    let b = u8::from_str_radix(hex.get(4..6).unwrap_or("80"), 16).unwrap_or(128) as f32 / 255.0;
    [r, g, b, alpha]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi256_基本16色が変換できる() {
        let black = ansi_256_to_rgb(0);
        assert_eq!(black, [0.0, 0.0, 0.0, 1.0]);
        let white = ansi_256_to_rgb(15);
        assert_eq!(white, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn ansi256_グレースケールが変換できる() {
        let grey = ansi_256_to_rgb(232);
        assert_eq!(grey, [0.0, 0.0, 0.0, 1.0]);
        let bright = ansi_256_to_rgb(255);
        assert_eq!(bright, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn デフォルト色の解決() {
        let fg = resolve_color(&nexterm_proto::Color::Default, true, None);
        assert!(fg[0] > 0.5); // 前景は明るい
        let bg = resolve_color(&nexterm_proto::Color::Default, false, None);
        assert!(bg[0] < 0.5); // 背景は暗い
    }

    #[test]
    fn hex_to_rgba_変換() {
        let c = hex_to_rgba("#ae8b2d", 1.0);
        assert!((c[0] - 0xae as f32 / 255.0).abs() < 1e-3);
        assert!((c[1] - 0x8b as f32 / 255.0).abs() < 1e-3);
        assert!((c[2] - 0x2d as f32 / 255.0).abs() < 1e-3);
        assert_eq!(c[3], 1.0);
    }

    #[test]
    fn hex_to_rgba_ハッシュなし() {
        let c = hex_to_rgba("ffffff", 0.5);
        assert_eq!(c, [1.0, 1.0, 1.0, 0.5]);
    }
}
