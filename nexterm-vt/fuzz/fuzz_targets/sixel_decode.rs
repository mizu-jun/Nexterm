#![no_main]
//! Sprint 3-5: fuzz the Sixel decoder.
//!
//! Feeds arbitrary bytes to `nexterm_vt::image::decode_sixel()` and verifies
//! that no OOM or panic occurs on malformed color maps, repeat counts, or
//! coordinates.
//!
//! Attack scenarios in scope:
//! - Memory exhaustion through huge repeat counts (`!9999999`).
//! - Misparses on invalid color definitions (`#0;2;...`).
//! - Truncated DCS or missing terminator.
//! - Negative or overflowing coordinates.

use libfuzzer_sys::fuzz_target;
use nexterm_vt::image::decode_sixel;

fuzz_target!(|data: &[u8]| {
    // A Sixel payload is realistically only a few KiB.
    // Truncate inputs above 100 KiB to save fuzzer resources.
    let bytes = if data.len() > 100 * 1024 {
        &data[..100 * 1024]
    } else {
        data
    };

    // The return value is unused — we only check that no panic / OOM happens.
    let _ = decode_sixel(bytes);
});
