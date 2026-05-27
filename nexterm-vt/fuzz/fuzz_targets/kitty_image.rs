#![no_main]
//! Sprint 3-5: fuzz the Kitty graphics protocol decoder.
//!
//! Feeds arbitrary APC payloads to `nexterm_vt::image::decode_kitty()` and
//! verifies that no OOM or panic occurs from malformed base64, width/height
//! specifiers, or chunk concatenation.
//!
//! Attack scenarios in scope:
//! - Memory exhaustion through huge width/height values (`s=99999,v=99999`).
//! - Malformed base64 strings.
//! - Inconsistent `width × height × bytes_per_pixel`.
//! - Unknown format specifiers (`f=`).

use libfuzzer_sys::fuzz_target;
use nexterm_vt::image::decode_kitty;

fuzz_target!(|data: &[u8]| {
    // A Kitty APC payload depends on the image size.
    // Truncate at 4 MiB (matching the VtParser APC buffer limit).
    let bytes = if data.len() > 4 * 1024 * 1024 {
        &data[..4 * 1024 * 1024]
    } else {
        data
    };

    let _ = decode_kitty(bytes);
});
