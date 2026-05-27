#![no_main]
//! Sprint 3-5: fuzz the whole VT parser with arbitrary byte streams.
//!
//! Feeds arbitrary bytes to `nexterm_vt::VtParser::advance()` and verifies
//! that no panic, OOM, or infinite loop occurs.
//!
//! Attack scenarios in scope:
//! - Malformed CSI / OSC / DCS / APC sequences.
//! - Huge CSI parameter lists.
//! - Unusual combinations of newlines, tabs, and cursor motion.
//! - Abuse of synchronized output mode (DEC ?2026).

use libfuzzer_sys::fuzz_target;
use nexterm_vt::VtParser;

fuzz_target!(|data: &[u8]| {
    // Initialize at the standard 80x24 size (to keep resource use low).
    let mut parser = VtParser::new(80, 24);

    // Cap the input size to limit the fuzzer's memory footprint
    // (cargo-fuzz's `-max_len` defaults to 4096, but enforce it here as well).
    let bytes = if data.len() > 65_536 {
        &data[..65_536]
    } else {
        data
    };

    parser.advance(bytes);

    // Confirm that the side-effect getters also do not panic.
    let _ = parser.screen().grid();
    let _ = parser.screen().cursor();
    let _ = parser.bracketed_paste_mode();
    let _ = parser.synchronized_output_mode();
});
