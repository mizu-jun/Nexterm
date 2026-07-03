//! Dynamic-color helpers for OSC 4 / 10 / 11 (parse specs, builtin palette,
//! xterm-style query replies).

/// Fallback default foreground (#d9d9d9). Mirrors the client-side
/// `Color::Default` fallback in `nexterm-client-gpu/src/color_util.rs`
/// (0.85 in linear f32 per channel). The server overrides this with the
/// active theme via `Screen::set_default_colors`.
pub(crate) const FALLBACK_FG: [u8; 3] = [0xd9, 0xd9, 0xd9];

/// Fallback default background (#0d0d0d). See [`FALLBACK_FG`].
pub(crate) const FALLBACK_BG: [u8; 3] = [0x0d, 0x0d, 0x0d];

/// Parses an xparsecolor-style color spec.
///
/// Supported forms (the ones emitted by real-world tools):
/// - `#RRGGBB` — 8 bits per channel
/// - `rgb:R/G/B` with 1–4 hex digits per channel (each channel is scaled
///   down to 8 bits by taking the most significant byte)
///
/// Returns `None` for anything else; callers ignore invalid specs.
pub(crate) fn parse_color_spec(spec: &str) -> Option<[u8; 3]> {
    let spec = spec.trim();
    if let Some(hex) = spec.strip_prefix('#') {
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some([r, g, b]);
    }
    if let Some(body) = spec.strip_prefix("rgb:") {
        let mut channels = body.split('/');
        let r = parse_rgb_channel(channels.next()?)?;
        let g = parse_rgb_channel(channels.next()?)?;
        let b = parse_rgb_channel(channels.next()?)?;
        if channels.next().is_some() {
            return None;
        }
        return Some([r, g, b]);
    }
    None
}

/// Parses one `rgb:` channel of 1–4 hex digits and scales it to 8 bits.
fn parse_rgb_channel(s: &str) -> Option<u8> {
    if s.is_empty() || s.len() > 4 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u16::from_str_radix(s, 16).ok()?;
    let max = (1u32 << (4 * s.len() as u32)) - 1;
    // Scale to 0..=255 rounding to nearest.
    Some(((value as u32 * 255 + max / 2) / max) as u8)
}

/// Formats a color as the xterm 16-bit-per-channel reply payload
/// (`rgb:rrrr/gggg/bbbb`). 8-bit channels are widened by repetition,
/// matching xterm's behavior.
pub(crate) fn format_rgb_reply(c: [u8; 3]) -> String {
    format!(
        "rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}",
        r = c[0],
        g = c[1],
        b = c[2]
    )
}

/// Returns the builtin xterm 256-color palette entry.
///
/// - 0–15: standard xterm ANSI colors
/// - 16–231: 6×6×6 color cube with levels 0/95/135/175/215/255
/// - 232–255: 24-step grayscale ramp (8 + 10·n)
pub(crate) fn builtin_palette_color(index: u8) -> [u8; 3] {
    const ANSI16: [[u8; 3]; 16] = [
        [0x00, 0x00, 0x00],
        [0xcd, 0x00, 0x00],
        [0x00, 0xcd, 0x00],
        [0xcd, 0xcd, 0x00],
        [0x00, 0x00, 0xee],
        [0xcd, 0x00, 0xcd],
        [0x00, 0xcd, 0xcd],
        [0xe5, 0xe5, 0xe5],
        [0x7f, 0x7f, 0x7f],
        [0xff, 0x00, 0x00],
        [0x00, 0xff, 0x00],
        [0xff, 0xff, 0x00],
        [0x5c, 0x5c, 0xff],
        [0xff, 0x00, 0xff],
        [0x00, 0xff, 0xff],
        [0xff, 0xff, 0xff],
    ];
    match index {
        0..=15 => ANSI16[index as usize],
        16..=231 => {
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let i = index as usize - 16;
            [LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6]]
        }
        232..=255 => {
            let v = 8 + 10 * (index - 232);
            [v, v, v]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hash_rrggbb() {
        assert_eq!(parse_color_spec("#ff8800"), Some([0xff, 0x88, 0x00]));
        assert_eq!(parse_color_spec(" #012345 "), Some([0x01, 0x23, 0x45]));
    }

    #[test]
    fn rejects_malformed_hash_specs() {
        assert_eq!(parse_color_spec("#zzz"), None);
        assert_eq!(parse_color_spec("#12345"), None);
        assert_eq!(parse_color_spec("#1234567"), None);
    }

    #[test]
    fn parses_rgb_slash_forms_of_all_widths() {
        // 1-digit: f/15 → 255; 2-digit passthrough; 4-digit takes the MSB.
        assert_eq!(parse_color_spec("rgb:f/0/8"), Some([255, 0, 136]));
        assert_eq!(parse_color_spec("rgb:12/34/56"), Some([0x12, 0x34, 0x56]));
        assert_eq!(
            parse_color_spec("rgb:1212/3434/5656"),
            Some([0x12, 0x34, 0x56])
        );
    }

    #[test]
    fn rejects_malformed_rgb_specs() {
        assert_eq!(parse_color_spec("rgb:12/34"), None);
        assert_eq!(parse_color_spec("rgb:12/34/56/78"), None);
        assert_eq!(parse_color_spec("rgb:12345/0/0"), None);
        assert_eq!(parse_color_spec("notacolor"), None);
        assert_eq!(parse_color_spec(""), None);
    }

    #[test]
    fn reply_format_widens_channels_by_repetition() {
        assert_eq!(format_rgb_reply([0xff, 0x88, 0x00]), "rgb:ffff/8888/0000");
    }

    #[test]
    fn builtin_palette_matches_xterm() {
        assert_eq!(builtin_palette_color(1), [0xcd, 0x00, 0x00]);
        assert_eq!(builtin_palette_color(3), [0xcd, 0xcd, 0x00]);
        assert_eq!(builtin_palette_color(196), [0xff, 0x00, 0x00]); // cube corner
        assert_eq!(builtin_palette_color(16), [0x00, 0x00, 0x00]); // cube origin
        assert_eq!(builtin_palette_color(244), [0x80, 0x80, 0x80]); // grayscale
        assert_eq!(builtin_palette_color(255), [0xee, 0xee, 0xee]);
    }
}
