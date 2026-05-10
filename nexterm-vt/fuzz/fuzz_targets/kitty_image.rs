#![no_main]
//! Sprint 3-5: Kitty グラフィックスプロトコルデコーダに対するファジング
//!
//! `nexterm_vt::image::decode_kitty()` に任意の APC ペイロードを投入し、
//! 不正な base64・幅高指定・チャンク連結で OOM やパニックを
//! 起こさないことを検証する。
//!
//! 想定攻撃シナリオ:
//! - 巨大な width/height 指定 (`s=99999,v=99999`) によるメモリ枯渇
//! - 不正な base64 文字列
//! - 不一致な width × height × bytes_per_pixel
//! - 未知のフォーマット指定子 (`f=`)

use libfuzzer_sys::fuzz_target;
use nexterm_vt::image::decode_kitty;

fuzz_target!(|data: &[u8]| {
    // Kitty APC ペイロードは画像サイズによる。
    // 4 MiB（VtParser の APC バッファ上限と同じ）に切り詰める
    let bytes = if data.len() > 4 * 1024 * 1024 {
        &data[..4 * 1024 * 1024]
    } else {
        data
    };

    let _ = decode_kitty(bytes);
});
