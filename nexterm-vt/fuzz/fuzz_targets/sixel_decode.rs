#![no_main]
//! Sprint 3-5: Sixel デコーダに対するファジング
//!
//! `nexterm_vt::image::decode_sixel()` に任意のバイト列を投入し、
//! 不正なカラーマップ・繰り返し回数・座標で OOM やパニックを
//! 起こさないことを検証する。
//!
//! 想定攻撃シナリオ:
//! - 巨大な repeat count (`!9999999`) によるメモリ枯渇
//! - 不正なカラー定義 (`#0;2;...`) でのパース誤り
//! - DCS 終端なし・途中切れ
//! - 負の座標・オーバーフロー値

use libfuzzer_sys::fuzz_target;
use nexterm_vt::image::decode_sixel;

fuzz_target!(|data: &[u8]| {
    // Sixel ペイロードは数 KiB が現実的な上限。
    // 100 KiB を超える入力はファザリソース節約のため切り詰める
    let bytes = if data.len() > 100 * 1024 {
        &data[..100 * 1024]
    } else {
        data
    };

    // 戻り値は使わない — パニック・OOM が起きないことのみを確認する
    let _ = decode_sixel(bytes);
});
