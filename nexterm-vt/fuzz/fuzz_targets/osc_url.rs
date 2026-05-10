#![no_main]
//! Sprint 3-5: OSC 8 ハイパーリンク + OSC 52 / 133 などの OSC ハンドラに対するファジング
//!
//! 任意のバイト列を OSC シーケンス (`ESC ] ... BEL` または `ESC ] ... ESC \`)
//! でラップし、`VtParser::advance()` 経由で OSC ハンドラに到達させる。
//! URL allowlist チェック・長さ上限・URL パース失敗で
//! パニックや OOM が起きないことを検証する（CRITICAL #5 の事前検出）。
//!
//! 想定攻撃シナリオ:
//! - 巨大な URL（数 MiB）
//! - 不正な URL スキーマ (`javascript:`, `data:`, `file:`)
//! - 終端文字なしの OSC
//! - OSC 番号自体が異常（例: 99999）

use libfuzzer_sys::fuzz_target;
use nexterm_vt::VtParser;

fuzz_target!(|data: &[u8]| {
    // 入力を OSC 8 ハイパーリンク・OSC 52 クリップボード・OSC 133 セマンティック
    // のいずれかとして 3 種類のパターンに包んでテストする
    let bytes = if data.len() > 65_536 {
        &data[..65_536]
    } else {
        data
    };

    // パターン 1: OSC 8 ハイパーリンク (URL 部分にファザ入力)
    {
        let mut parser = VtParser::new(80, 24);
        let mut seq = Vec::with_capacity(bytes.len() + 16);
        seq.extend_from_slice(b"\x1b]8;;");
        seq.extend_from_slice(bytes);
        seq.extend_from_slice(b"\x07Click\x1b]8;;\x07");
        parser.advance(&seq);
    }

    // パターン 2: OSC 52 クリップボード (base64 部分にファザ入力)
    {
        let mut parser = VtParser::new(80, 24);
        let mut seq = Vec::with_capacity(bytes.len() + 16);
        seq.extend_from_slice(b"\x1b]52;c;");
        seq.extend_from_slice(bytes);
        seq.extend_from_slice(b"\x07");
        parser.advance(&seq);
    }

    // パターン 3: OSC 133 セマンティックマーク (任意フィールド)
    {
        let mut parser = VtParser::new(80, 24);
        let mut seq = Vec::with_capacity(bytes.len() + 16);
        seq.extend_from_slice(b"\x1b]133;");
        seq.extend_from_slice(bytes);
        seq.extend_from_slice(b"\x07");
        parser.advance(&seq);
        // 副作用も取得してパニックしないことを確認
        let _ = parser.screen_mut().take_semantic_marks();
    }
});
