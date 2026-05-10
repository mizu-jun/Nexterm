#![no_main]
//! Sprint 3-5: VT パーサ全体に対する任意バイト列ファジング
//!
//! `nexterm_vt::VtParser::advance()` に任意のバイト列を投入し、
//! パニック・OOM・無限ループを起こさないことを検証する。
//!
//! 想定攻撃シナリオ:
//! - 不正な CSI / OSC / DCS / APC シーケンス
//! - 巨大な CSI パラメータ列
//! - 改行・タブ・カーソル移動の異常な組み合わせ
//! - 同期出力モード (DEC ?2026) の悪用

use libfuzzer_sys::fuzz_target;
use nexterm_vt::VtParser;

fuzz_target!(|data: &[u8]| {
    // 80x24 の標準サイズで初期化（リソース消費を抑える）
    let mut parser = VtParser::new(80, 24);

    // 入力サイズに上限を設けてファザのメモリ消費を抑制する
    // (cargo-fuzz の -max_len はデフォルト 4096 だが念のため明示)
    let bytes = if data.len() > 65_536 {
        &data[..65_536]
    } else {
        data
    };

    parser.advance(bytes);

    // 副次的な状態取得もパニックしないことを検証する
    let _ = parser.screen().grid();
    let _ = parser.screen().cursor();
    let _ = parser.bracketed_paste_mode();
    let _ = parser.synchronized_output_mode();
});
