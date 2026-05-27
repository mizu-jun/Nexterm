//! Sixel / Kitty image protocol decoders.

use std::collections::HashMap;

/// Maximum bytes per image (256 MiB).
///
/// Defends against a malicious PTY / SSH host that specifies an extreme
/// `width × height`, overflows the `u32` multiplication inside
/// `vec![0u8; width * height * 4]`, allocates a tiny buffer, and then writes
/// out of bounds to corrupt the heap.
const MAX_IMAGE_BYTES: usize = 256 * 1024 * 1024;

/// Computes `width × height × channels` safely as a `usize`.
///
/// Detects overflow by routing through `u64` and returns `None` when the result
/// would exceed [`MAX_IMAGE_BYTES`].
fn checked_image_bytes(width: u32, height: u32, channels: u32) -> Option<usize> {
    let bytes = (width as u64)
        .checked_mul(height as u64)?
        .checked_mul(channels as u64)?;
    if bytes > MAX_IMAGE_BYTES as u64 {
        return None;
    }
    Some(bytes as usize)
}

/// Decoded image data (RGBA).
#[derive(Debug, Clone)]
pub struct DecodedImage {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// RGBA pixel data (`width × height × 4` bytes).
    pub rgba: Vec<u8>,
}

// ---- Sixel decoder ----

/// Decodes DCS Sixel data into an RGBA image.
pub fn decode_sixel(data: &[u8]) -> Option<DecodedImage> {
    let mut palette: HashMap<u16, [u8; 3]> = default_sixel_palette();
    let mut current_color: u16 = 0;
    let mut x: usize = 0;
    let mut band: usize = 0;
    let mut max_x: usize = 0;
    let mut max_band: usize = 0;

    // Pixel buffer: `buf[row][col] = RGBA`.
    let mut buf: Vec<Vec<Option<[u8; 3]>>> = vec![Vec::new()];

    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'#' => {
                // Color selection / color definition.
                i += 1;
                let n = match parse_decimal(data, &mut i) {
                    Some(v) => v,
                    None => continue,
                };
                if i < data.len() && data[i] == b';' {
                    i += 1;
                    let kind = parse_decimal(data, &mut i).unwrap_or(0);
                    if i < data.len() && data[i] == b';' {
                        i += 1;
                        let p1 = parse_decimal(data, &mut i).unwrap_or(0) as u32;
                        if i < data.len() && data[i] == b';' {
                            i += 1;
                            let p2 = parse_decimal(data, &mut i).unwrap_or(0) as u32;
                            if i < data.len() && data[i] == b';' {
                                i += 1;
                                let p3 = parse_decimal(data, &mut i).unwrap_or(0) as u32;
                                match kind {
                                    2 => {
                                        // RGB (0–100%).
                                        palette.insert(
                                            n,
                                            [
                                                (p1 * 255 / 100).min(255) as u8,
                                                (p2 * 255 / 100).min(255) as u8,
                                                (p3 * 255 / 100).min(255) as u8,
                                            ],
                                        );
                                    }
                                    1 => {
                                        // HLS — rough approximation (treat as white).
                                        palette.insert(n, [200, 200, 200]);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                current_color = n;
            }
            b'$' => {
                // Graphics Carriage Return — reset the column to the start.
                x = 0;
                i += 1;
            }
            b'-' => {
                // Graphics New Line — move to the next band (6 rows).
                x = 0;
                band += 1;
                max_band = max_band.max(band);
                ensure_bands(&mut buf, band);
                i += 1;
            }
            b'!' => {
                // Repeat: `!n<char>`.
                i += 1;
                let count = parse_decimal(data, &mut i).unwrap_or(1) as usize;
                if i < data.len() {
                    let ch = data[i];
                    i += 1;
                    if matches!(ch, b'?'..=b'~') {
                        let color = *palette.get(&current_color).unwrap_or(&[200, 200, 200]);
                        let bits = ch - b'?';
                        ensure_bands(&mut buf, band);
                        for _ in 0..count {
                            paint_col(&mut buf, x, band, bits, color);
                            x += 1;
                        }
                        max_x = max_x.max(x);
                    }
                }
            }
            b'?'..=b'~' => {
                // Sixel pixel data (one character = 6 vertical bits).
                let color = *palette.get(&current_color).unwrap_or(&[200, 200, 200]);
                let bits = data[i] - b'?';
                ensure_bands(&mut buf, band);
                paint_col(&mut buf, x, band, bits, color);
                x += 1;
                max_x = max_x.max(x);
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    let height = (max_band + 1) * 6;
    let width = max_x;
    if width == 0 || height == 0 {
        return None;
    }

    // Prevent `u32` overflow on massive images (which would produce a tiny
    // `expected`, followed by an out-of-bounds write).
    let width_u32 = u32::try_from(width).ok()?;
    let height_u32 = u32::try_from(height).ok()?;
    let total = checked_image_bytes(width_u32, height_u32, 4)?;
    let mut rgba = vec![0u8; total];
    for (row_idx, row) in buf.iter().enumerate().take(height) {
        for (col_idx, pixel) in row.iter().enumerate().take(width) {
            if let Some(c) = pixel {
                let idx = (row_idx * width + col_idx) * 4;
                rgba[idx] = c[0];
                rgba[idx + 1] = c[1];
                rgba[idx + 2] = c[2];
                rgba[idx + 3] = 255;
            }
        }
    }

    Some(DecodedImage {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

/// Grows the buffer to `(band + 1) * 6` rows.
fn ensure_bands(buf: &mut Vec<Vec<Option<[u8; 3]>>>, band: usize) {
    while buf.len() < (band + 1) * 6 {
        buf.push(Vec::new());
    }
}

/// Writes one Sixel column (6 bits) into the buffer.
fn paint_col(buf: &mut [Vec<Option<[u8; 3]>>], x: usize, band: usize, bits: u8, color: [u8; 3]) {
    for bit in 0..6usize {
        if bits & (1 << bit) != 0 {
            let row = band * 6 + bit;
            if row < buf.len() {
                while buf[row].len() <= x {
                    buf[row].push(None);
                }
                buf[row][x] = Some(color);
            }
        }
    }
}

/// VT340 default color palette (a subset).
fn default_sixel_palette() -> HashMap<u16, [u8; 3]> {
    let mut m = HashMap::new();
    let colors: &[(u16, [u8; 3])] = &[
        (0, [0, 0, 0]),
        (1, [51, 204, 51]),
        (2, [204, 51, 51]),
        (3, [204, 204, 51]),
        (4, [51, 51, 204]),
        (5, [204, 51, 204]),
        (6, [51, 204, 204]),
        (7, [204, 204, 204]),
        (8, [102, 102, 102]),
        (9, [102, 255, 102]),
        (10, [255, 102, 102]),
        (11, [255, 255, 102]),
        (12, [102, 102, 255]),
        (13, [255, 102, 255]),
        (14, [102, 255, 255]),
        (15, [255, 255, 255]),
    ];
    for &(idx, color) in colors {
        m.insert(idx, color);
    }
    m
}

// ---- Kitty graphics protocol decoder ----

/// Decodes Kitty APC data.
///
/// Format: `G<key>=<val>,...;<base64_payload>`.
/// Supported parameters: `a=T` (transmit), `f=32` (RGBA) / `f=24` (RGB),
/// `s=width`, `v=height`.
pub fn decode_kitty(apc_data: &[u8]) -> Option<DecodedImage> {
    if apc_data.first() != Some(&b'G') {
        return None;
    }
    let data = &apc_data[1..];

    let sep = data.iter().position(|&b| b == b';')?;
    let params_bytes = &data[..sep];
    let payload = &data[sep + 1..];

    let mut format: u8 = 32;
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut action: u8 = b'T';

    for param in params_bytes.split(|&b| b == b',') {
        if param.len() < 2 || param[1] != b'=' {
            continue;
        }
        let val = &param[2..];
        match param[0] {
            b'a' => action = val.first().copied().unwrap_or(b'T'),
            b'f' => format = parse_u32_bytes(val) as u8,
            b's' => width = parse_u32_bytes(val),
            b'v' => height = parse_u32_bytes(val),
            _ => {}
        }
    }

    // Ignore anything other than the transmit action.
    if action != b'T' {
        return None;
    }

    let pixel_data = base64_decode(payload)?;

    match format {
        32 => {
            // 8-bit RGBA.
            if width == 0 || height == 0 {
                return None;
            }
            // A `u32` multiplication overflow could produce a tiny `expected`,
            // which would later cause a panic or a buffer-size mismatch.
            let expected = checked_image_bytes(width, height, 4)?;
            if pixel_data.len() < expected {
                return None;
            }
            Some(DecodedImage {
                width,
                height,
                rgba: pixel_data[..expected].to_vec(),
            })
        }
        24 => {
            // 8-bit RGB → convert to RGBA.
            if width == 0 || height == 0 {
                return None;
            }
            // Guard against `u32` overflow. RGB has 3 channels; the converted
            // RGBA has 4.
            let expected = checked_image_bytes(width, height, 3)?;
            let rgba_capacity = checked_image_bytes(width, height, 4)?;
            if pixel_data.len() < expected {
                return None;
            }
            let mut rgba = Vec::with_capacity(rgba_capacity);
            for chunk in pixel_data[..expected].chunks_exact(3) {
                rgba.extend_from_slice(chunk);
                rgba.push(255);
            }
            Some(DecodedImage {
                width,
                height,
                rgba,
            })
        }
        _ => None,
    }
}

// ---- Utilities ----

fn parse_decimal(data: &[u8], i: &mut usize) -> Option<u16> {
    if *i >= data.len() || !data[*i].is_ascii_digit() {
        return None;
    }
    let mut result: u32 = 0;
    while *i < data.len() && data[*i].is_ascii_digit() {
        // Use saturating arithmetic to avoid a `u32` overflow panic when a
        // malicious escape sequence supplies an enormous digit string. The
        // caller clamps to `u16`, so any value above the cap can be folded
        // down to `u16::MAX`.
        // Fix for the DoS bug found by the late Sprint 5-7 nightly fuzz.
        result = result
            .saturating_mul(10)
            .saturating_add((data[*i] - b'0') as u32);
        *i += 1;
    }
    Some(result.min(u16::MAX as u32) as u16)
}

fn parse_u32_bytes(data: &[u8]) -> u32 {
    let mut result: u32 = 0;
    for &b in data {
        if b.is_ascii_digit() {
            // Same idea as above. Fixes the panic that occurred when a Kitty
            // image protocol parameter carried an enormous number
            // (reproducible via the `kitty_image` fuzz target).
            result = result.saturating_mul(10).saturating_add((b - b'0') as u32);
        }
    }
    result
}

/// Base64 decode (handles both padded and unpadded input).
pub(crate) fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    fn decode_char(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let clean: Vec<u8> = input
        .iter()
        .filter(|&&b| b != b'=' && !b.is_ascii_whitespace())
        .copied()
        .collect();

    let mut result = Vec::with_capacity(clean.len() * 3 / 4 + 3);
    let mut i = 0;
    while i + 3 < clean.len() {
        let a = decode_char(clean[i])?;
        let b = decode_char(clean[i + 1])?;
        let c = decode_char(clean[i + 2])?;
        let d = decode_char(clean[i + 3])?;
        result.push((a << 2) | (b >> 4));
        result.push((b << 4) | (c >> 2));
        result.push((c << 6) | d);
        i += 4;
    }
    let rem = clean.len() - i;
    if rem >= 2 {
        let a = decode_char(clean[i])?;
        let b = decode_char(clean[i + 1])?;
        result.push((a << 2) | (b >> 4));
        if rem >= 3 {
            let c = decode_char(clean[i + 2])?;
            result.push((b << 4) | (c >> 2));
        }
    }

    Some(result)
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decode_works() {
        // "Man" → "TWFu".
        let decoded = base64_decode(b"TWFu").unwrap();
        assert_eq!(decoded, b"Man");
    }

    #[test]
    fn base64_decode_with_padding() {
        // "Ma" → "TWE=".
        let decoded = base64_decode(b"TWE=").unwrap();
        assert_eq!(decoded, b"Ma");
    }

    #[test]
    #[allow(non_snake_case)]
    fn empty_sixel_returns_None() {
        let result = decode_sixel(b"");
        assert!(result.is_none());
    }

    #[test]
    fn a_simple_sixel_decodes() {
        // '#0;2;0;0;0' (define color 0 as black) + '~' (all bits = full 6 pixels) × 1 column.
        let data = b"#0;2;0;0;0~";
        let result = decode_sixel(data);
        assert!(result.is_some());
        let img = result.unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 6);
    }

    #[test]
    fn ordinary_image_byte_count() {
        assert_eq!(checked_image_bytes(100, 100, 4), Some(40_000));
        assert_eq!(checked_image_bytes(1, 1, 4), Some(4));
        assert_eq!(checked_image_bytes(0, 0, 4), Some(0));
    }

    #[test]
    fn returns_none_on_u32_overflow() {
        // 65536 × 65536 × 4 = 17 GB → wraps under u32 to a tiny value; computed
        // correctly via u64 and rejected here.
        assert_eq!(checked_image_bytes(65536, 65536, 4), None);
        // 4096 × 4096 × 4 = 64 MiB (≤ 256 MiB, allowed).
        assert_eq!(checked_image_bytes(4096, 4096, 4), Some(67_108_864));
        // 8192 × 8192 × 4 = 256 MiB (boundary, allowed).
        assert_eq!(checked_image_bytes(8192, 8192, 4), Some(MAX_IMAGE_BYTES));
        // 8193 × 8192 × 4 = 256 MiB + 32 KiB (over the boundary, rejected).
        assert_eq!(checked_image_bytes(8193, 8192, 4), None);
        // `u32::MAX` alone must also return None without overflowing.
        assert_eq!(checked_image_bytes(u32::MAX, u32::MAX, 4), None);
    }

    #[test]
    fn rejects_decoding_a_huge_kitty_image() {
        // format=32, width=65536, height=65536 → 17 GB → must be rejected.
        // Build the actual APC string.
        let payload = b""; // empty base64 payload (validation should happen before allocation)
        let mut data = Vec::new();
        data.extend_from_slice(b"a=T,f=32,s=65536,v=65536;");
        data.extend_from_slice(payload);
        let result = decode_kitty(&data);
        // The image is huge, so `checked_image_bytes` returns None and the result is None.
        assert!(result.is_none(), "decoding a huge image should be rejected");
    }

    // ---- Regression tests for numeric-parser panics (Sprint 5-7 late-fuzz bugs) ----

    #[test]
    fn parse_decimal_does_not_panic_on_a_huge_digit_string() {
        // Pass a digit string longer than u32 can hold (10+ chars) and confirm
        // it is absorbed by saturating_mul/add rather than panicking.
        let data = b"99999999999999999999"; // 20 digits, well above u32::MAX (10 digits)
        let mut i = 0;
        let result = parse_decimal(data, &mut i).unwrap();
        // Result is capped at u16::MAX (65535).
        assert_eq!(result, u16::MAX);
        assert_eq!(i, data.len(), "every digit should be consumed");
    }

    #[test]
    fn parse_decimal_returns_normal_values_correctly() {
        let data = b"12345abc";
        let mut i = 0;
        let result = parse_decimal(data, &mut i).unwrap();
        assert_eq!(result, 12345);
        assert_eq!(i, 5, "stops just before the non-digit character");
    }

    #[test]
    fn parse_decimal_returns_none_for_non_digits() {
        let data = b"abc";
        let mut i = 0;
        assert!(parse_decimal(data, &mut i).is_none());
    }

    #[test]
    fn parse_u32_bytes_does_not_panic_on_a_huge_digit_string() {
        // A digit string long enough to overflow u32 is still absorbed by saturating math.
        let data = b"99999999999999999999"; // 20 digits
        let result = parse_u32_bytes(data);
        assert_eq!(result, u32::MAX);
    }

    #[test]
    fn parse_u32_bytes_returns_normal_values_correctly() {
        assert_eq!(parse_u32_bytes(b"42"), 42);
        // Non-digit bytes are skipped.
        assert_eq!(parse_u32_bytes(b"1a2b3"), 123);
        assert_eq!(parse_u32_bytes(b""), 0);
    }
}
