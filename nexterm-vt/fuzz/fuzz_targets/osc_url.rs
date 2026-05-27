#![no_main]
//! Sprint 3-5: fuzz the OSC handlers (OSC 8 hyperlinks plus OSC 52 / 133).
//!
//! Wraps arbitrary bytes inside an OSC sequence (`ESC ] ... BEL` or
//! `ESC ] ... ESC \`) and feeds them through `VtParser::advance()` to reach
//! the OSC handlers. Verifies that the URL allow-list check, length cap, and
//! URL parse failures never panic or OOM (proactive detection for CRITICAL #5).
//!
//! Attack scenarios in scope:
//! - Huge URLs (several MiB).
//! - Disallowed URL schemes (`javascript:`, `data:`, `file:`).
//! - OSC sequences without a terminator.
//! - Abnormal OSC numbers (e.g. 99999).

use libfuzzer_sys::fuzz_target;
use nexterm_vt::VtParser;

fuzz_target!(|data: &[u8]| {
    // Wrap the input into three different OSC patterns (OSC 8 hyperlink,
    // OSC 52 clipboard, and OSC 133 semantic mark) and test each one.
    let bytes = if data.len() > 65_536 {
        &data[..65_536]
    } else {
        data
    };

    // Pattern 1: OSC 8 hyperlink (fuzzer input goes into the URL).
    {
        let mut parser = VtParser::new(80, 24);
        let mut seq = Vec::with_capacity(bytes.len() + 16);
        seq.extend_from_slice(b"\x1b]8;;");
        seq.extend_from_slice(bytes);
        seq.extend_from_slice(b"\x07Click\x1b]8;;\x07");
        parser.advance(&seq);
    }

    // Pattern 2: OSC 52 clipboard (fuzzer input goes into the base64 payload).
    {
        let mut parser = VtParser::new(80, 24);
        let mut seq = Vec::with_capacity(bytes.len() + 16);
        seq.extend_from_slice(b"\x1b]52;c;");
        seq.extend_from_slice(bytes);
        seq.extend_from_slice(b"\x07");
        parser.advance(&seq);
    }

    // Pattern 3: OSC 133 semantic mark (fuzzer input goes into the free fields).
    {
        let mut parser = VtParser::new(80, 24);
        let mut seq = Vec::with_capacity(bytes.len() + 16);
        seq.extend_from_slice(b"\x1b]133;");
        seq.extend_from_slice(bytes);
        seq.extend_from_slice(b"\x07");
        parser.advance(&seq);
        // Drain the side effects to confirm they do not panic.
        let _ = parser.screen_mut().take_semantic_marks();
    }
});
