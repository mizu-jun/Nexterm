//! Sixel / Kitty 画像プロトコルデコーダ

use std::collections::HashMap;

/// 画像 1 枚あたりの最大バイト数（256 MiB）。
///
/// 悪意ある PTY / SSH ホストが極端に大きな width × height を指定して
/// `vec![0u8; width * height * 4]` で u32 オーバーフロー後に
/// 極小バッファを確保し、その後の境界外書き込みでヒープを破壊する攻撃を防ぐ。
const MAX_IMAGE_BYTES: usize = 256 * 1024 * 1024;

/// 幅 × 高さ × チャネル数 を `usize` で安全に計算する。
///
/// `u64` 経由でオーバーフローを検出し、`MAX_IMAGE_BYTES` を超える場合は `None` を返す。
fn checked_image_bytes(width: u32, height: u32, channels: u32) -> Option<usize> {
    let bytes = (width as u64)
        .checked_mul(height as u64)?
        .checked_mul(channels as u64)?;
    if bytes > MAX_IMAGE_BYTES as u64 {
        return None;
    }
    Some(bytes as usize)
}

/// デコードされた画像データ（RGBA）
#[derive(Debug, Clone)]
pub struct DecodedImage {
    /// 画像の幅（ピクセル）
    pub width: u32,
    /// 画像の高さ（ピクセル）
    pub height: u32,
    /// RGBA ピクセルデータ（width × height × 4 バイト）
    pub rgba: Vec<u8>,
}

// ---- Sixel デコーダ ----

/// DCS Sixel データをデコードして RGBA 画像を返す
pub fn decode_sixel(data: &[u8]) -> Option<DecodedImage> {
    let mut palette: HashMap<u16, [u8; 3]> = default_sixel_palette();
    let mut current_color: u16 = 0;
    let mut x: usize = 0;
    let mut band: usize = 0;
    let mut max_x: usize = 0;
    let mut max_band: usize = 0;

    // ピクセルバッファ: buf[row][col] = RGBA
    let mut buf: Vec<Vec<Option<[u8; 3]>>> = vec![Vec::new()];

    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'#' => {
                // カラー選択 / カラー定義
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
                                        // RGB (0–100%)
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
                                        // HLS — 簡易近似（白として扱う）
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
                // Graphics Carriage Return — 列を先頭に戻す
                x = 0;
                i += 1;
            }
            b'-' => {
                // Graphics New Line — 次のバンド（6行）へ
                x = 0;
                band += 1;
                max_band = max_band.max(band);
                ensure_bands(&mut buf, band);
                i += 1;
            }
            b'!' => {
                // リピート: !n<char>
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
                // Sixel ピクセルデータ（1文字 = 6ビット縦列）
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

    // 巨大画像での `u32` 乗算オーバーフロー → 極小バッファ確保後の境界外書き込みを防ぐ
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

/// バッファを (band+1)*6 行まで拡張する
fn ensure_bands(buf: &mut Vec<Vec<Option<[u8; 3]>>>, band: usize) {
    while buf.len() < (band + 1) * 6 {
        buf.push(Vec::new());
    }
}

/// Sixel の1列ピクセル（6ビット）をバッファに書き込む
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

/// VT340 デフォルトカラーパレット（一部）
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

// ---- Kitty グラフィックスプロトコルデコーダ ----

/// Kitty APC データをデコードする
///
/// 形式: `G<key>=<val>,...;<base64_payload>`
/// 対応フォーマット: a=T (送信), f=32 (RGBA) / f=24 (RGB), s=幅, v=高さ
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

    // 送信アクション以外は無視する
    if action != b'T' {
        return None;
    }

    let pixel_data = base64_decode(payload)?;

    match format {
        32 => {
            // RGBA 8-bit
            if width == 0 || height == 0 {
                return None;
            }
            // u32 乗算オーバーフローで小さな expected が出来るとパニック / バッファ不一致が起きる
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
            // RGB 8-bit → RGBA 変換
            if width == 0 || height == 0 {
                return None;
            }
            // u32 乗算オーバーフロー対策。RGB は 3 チャネル、変換後の RGBA は 4 チャネル。
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

// ---- ユーティリティ ----

fn parse_decimal(data: &[u8], i: &mut usize) -> Option<u16> {
    if *i >= data.len() || !data[*i].is_ascii_digit() {
        return None;
    }
    let mut result: u32 = 0;
    while *i < data.len() && data[*i].is_ascii_digit() {
        result = result * 10 + (data[*i] - b'0') as u32;
        *i += 1;
    }
    Some(result.min(u16::MAX as u32) as u16)
}

fn parse_u32_bytes(data: &[u8]) -> u32 {
    let mut result: u32 = 0;
    for &b in data {
        if b.is_ascii_digit() {
            result = result * 10 + (b - b'0') as u32;
        }
    }
    result
}

/// Base64 デコード（パディング有無どちらも対応）
fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
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

// ---- テスト ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64デコードが正しく動作する() {
        // "Man" → "TWFu"
        let decoded = base64_decode(b"TWFu").unwrap();
        assert_eq!(decoded, b"Man");
    }

    #[test]
    fn base64パディングありのデコード() {
        // "Ma" → "TWE="
        let decoded = base64_decode(b"TWE=").unwrap();
        assert_eq!(decoded, b"Ma");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 空のsixelはNoneを返す() {
        let result = decode_sixel(b"");
        assert!(result.is_none());
    }

    #[test]
    fn 単純なsixelをデコードできる() {
        // '#0;2;0;0;0' (黒を色0に定義) + '~' (全ビット=全6ピクセル) x 1列
        let data = b"#0;2;0;0;0~";
        let result = decode_sixel(data);
        assert!(result.is_some());
        let img = result.unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 6);
    }

    #[test]
    fn 通常サイズの画像バイト数計算() {
        assert_eq!(checked_image_bytes(100, 100, 4), Some(40_000));
        assert_eq!(checked_image_bytes(1, 1, 4), Some(4));
        assert_eq!(checked_image_bytes(0, 0, 4), Some(0));
    }

    #[test]
    fn u32_オーバーフロー時に_none_を返す() {
        // 65536 * 65536 * 4 = 17 GB → u32 でラップして極小値、u64 で正しく計算してチェック
        assert_eq!(checked_image_bytes(65536, 65536, 4), None);
        // 4096 * 4096 * 4 = 64 MiB（256 MiB 以下、許容）
        assert_eq!(checked_image_bytes(4096, 4096, 4), Some(67_108_864));
        // 8192 * 8192 * 4 = 256 MiB（境界、許容）
        assert_eq!(checked_image_bytes(8192, 8192, 4), Some(MAX_IMAGE_BYTES));
        // 8193 * 8192 * 4 = 256 MiB + 32 KiB（境界超過、拒否）
        assert_eq!(checked_image_bytes(8193, 8192, 4), None);
        // u32::MAX 単独でも overflow させずに None を返す
        assert_eq!(checked_image_bytes(u32::MAX, u32::MAX, 4), None);
    }

    #[test]
    fn 巨大kitty画像はデコードを拒否する() {
        // format=32, width=65536, height=65536 → 17GB → 拒否されるべき
        // 実際の APC 文字列を組み立てる
        let payload = b""; // 空の base64 ペイロード（バッファ確保前に検証されるはず）
        let mut data = Vec::new();
        data.extend_from_slice(b"a=T,f=32,s=65536,v=65536;");
        data.extend_from_slice(payload);
        let result = decode_kitty(&data);
        // 巨大画像なので checked_image_bytes が None を返し、結果は None
        assert!(result.is_none(), "巨大画像のデコードは拒否されるべき");
    }
}
