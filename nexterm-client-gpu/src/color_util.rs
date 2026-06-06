//! Color-conversion utilities — ANSI 256 colors and hex color strings.

/// Convert a `nexterm_proto::Color` into RGBA in the [0, 1] range.
///
/// When `is_fg` is true the `Default` color resolves to the foreground; otherwise to
/// the background. If `palette` is provided, the color scheme palette takes precedence.
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
        Color::Rgb(r, g, b) => [u8_to_f32(*r), u8_to_f32(*g), u8_to_f32(*b), 1.0],
        Color::Indexed(n) => {
            // ANSI 0-15: prefer the scheme palette when available.
            if *n < 16
                && let Some(p) = palette
            {
                let c = p.ansi[*n as usize];
                return [u8_to_f32(c[0]), u8_to_f32(c[1]), u8_to_f32(c[2]), 1.0];
            }
            ansi_256_to_rgb(*n)
        }
    }
}

/// ANSI 256-color palette → RGBA in [0, 1].
pub(crate) fn ansi_256_to_rgb(n: u8) -> [f32; 4] {
    // Basic 16 colors (simple implementation).
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

    // 216-color cube (16–231).
    if (16..=231).contains(&n) {
        let idx = n - 16;
        let b = (idx % 6) as f32 / 5.0;
        let g = ((idx / 6) % 6) as f32 / 5.0;
        let r = ((idx / 36) % 6) as f32 / 5.0;
        return [r, g, b, 1.0];
    }

    // Grayscale (232–255).
    let grey = (n - 232) as f32 / 23.0;
    [grey, grey, grey, 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi256_basic_16_colors_convert() {
        let black = ansi_256_to_rgb(0);
        assert_eq!(black, [0.0, 0.0, 0.0, 1.0]);
        let white = ansi_256_to_rgb(15);
        assert_eq!(white, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn ansi256_grayscale_converts() {
        let grey = ansi_256_to_rgb(232);
        assert_eq!(grey, [0.0, 0.0, 0.0, 1.0]);
        let bright = ansi_256_to_rgb(255);
        assert_eq!(bright, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn default_color_resolves() {
        let fg = resolve_color(&nexterm_proto::Color::Default, true, None);
        assert!(fg[0] > 0.5); // foreground is bright
        let bg = resolve_color(&nexterm_proto::Color::Default, false, None);
        assert!(bg[0] < 0.5); // background is dark
    }
}
