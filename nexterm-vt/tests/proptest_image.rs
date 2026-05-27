//! Sprint 4-4: property-based tests for the Sixel / Kitty parsers.
//!
//! Complementary to `cargo-fuzz` (Sprint 3-5): proptest verifies **semantic
//! invariants** instead of raw crashes:
//!
//! - Decoding must not panic on arbitrary byte input.
//! - When decoding succeeds, `rgba.len() == width * height * 4` always holds.
//! - Payloads that claim huge dimensions must reject decoding (subject to the
//!   16-MB `MAX_IMAGE_BYTES` cap).
//! - The same invariants must hold when the same bytes flow through `VtParser`.
//!
//! `cargo-fuzz` searches for **raw crashes** within its time budget, whereas
//! proptest verifies **specific postconditions** (dimensions matching the buffer
//! size, APC termination behavior, etc.) with shrinking. The two approaches are
//! complementary.

use nexterm_vt::VtParser;
use nexterm_vt::image::{decode_kitty, decode_sixel};
use proptest::prelude::*;

// ---- Helpers ----

/// proptest's default of 256 cases is enough to exercise the Sixel parser.
/// Each case handles up to a few KB, but each test runs in a sampling fashion.
fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 256,
        // Panics should fail immediately, so keep max_local_rejects modest.
        max_local_rejects: 1024,
        ..ProptestConfig::default()
    }
}

// ---- decode_sixel: arbitrary bytes ----

proptest! {
    #![proptest_config(config())]

    /// `decode_sixel` must not panic on arbitrary byte input.
    #[test]
    fn decode_sixel_never_panics(input in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = decode_sixel(&input);
    }

    /// On success, `rgba.len()` must equal `width * height * 4`.
    #[test]
    fn decode_sixel_buffer_size_matches_dimensions(
        input in proptest::collection::vec(any::<u8>(), 0..4096)
    ) {
        if let Some(img) = decode_sixel(&input) {
            let expected = (img.width as u64)
                .checked_mul(img.height as u64)
                .and_then(|v| v.checked_mul(4))
                .map(|v| v as usize);
            prop_assert_eq!(Some(img.rgba.len()), expected);
            // Must be a whole number of channels (multiple of 4).
            prop_assert_eq!(img.rgba.len() % 4, 0);
        }
    }
}

// ---- decode_sixel: structured generation ----

/// Generate only bytes meaningful to the Sixel parser (for performance).
/// Mixes `?`..=`~` pixel data with `#` color commands, `-` newlines,
/// `$` returns, and `!` repeats.
fn arb_sixel_bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(
        prop_oneof![
            // Pixel characters (0x3F..=0x7E).
            (0x3Fu8..=0x7E).prop_map(|b| vec![b]),
            // Newline / carriage return / repeat / color.
            Just(b"-".to_vec()),
            Just(b"$".to_vec()),
            // !<n><pixel>
            (1u32..=99, 0x3Fu8..=0x7E)
                .prop_map(|(n, ch)| format!("!{}{}", n, ch as char).into_bytes()),
            // #<n>;2;<r>;<g>;<b>
            (0u16..=255, 0u16..=100, 0u16..=100, 0u16..=100).prop_map(|(idx, r, g, b)| format!(
                "#{};2;{};{};{}",
                idx, r, g, b
            )
            .into_bytes()),
        ],
        0..256,
    )
    .prop_map(|chunks| chunks.into_iter().flatten().collect())
}

proptest! {
    #![proptest_config(config())]

    /// Structured Sixel data must not panic, and the dimensions must stay consistent.
    #[test]
    fn decode_sixel_structured_invariants(input in arb_sixel_bytes()) {
        if let Some(img) = decode_sixel(&input) {
            let total = (img.width as u64)
                .checked_mul(img.height as u64)
                .and_then(|v| v.checked_mul(4))
                .map(|v| v as usize);
            prop_assert_eq!(Some(img.rgba.len()), total);
            // Sixel rows come in groups of 6, so height is always a multiple of 6.
            prop_assert_eq!(img.height % 6, 0);
            // Width and height never reach zero (those cases are filtered by an early None).
            prop_assert!(img.width > 0);
            prop_assert!(img.height > 0);
        }
    }
}

// ---- decode_kitty: arbitrary bytes ----

proptest! {
    #![proptest_config(config())]

    /// `decode_kitty` must not panic on arbitrary byte input.
    #[test]
    fn decode_kitty_never_panics(input in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = decode_kitty(&input);
    }

    /// On success, `rgba.len()` must equal `width * height * 4`.
    #[test]
    fn decode_kitty_buffer_size_matches_dimensions(
        input in proptest::collection::vec(any::<u8>(), 0..4096)
    ) {
        if let Some(img) = decode_kitty(&input) {
            let expected = (img.width as u64)
                .checked_mul(img.height as u64)
                .and_then(|v| v.checked_mul(4))
                .map(|v| v as usize);
            prop_assert_eq!(Some(img.rgba.len()), expected);
            prop_assert_eq!(img.rgba.len() % 4, 0);
            prop_assert!(img.width > 0);
            prop_assert!(img.height > 0);
        }
    }
}

// ---- decode_kitty: structured generation ----

/// Generate plausible Kitty APC sequences (covers both panic resistance and
/// dimensional checks).
fn arb_kitty_apc() -> impl Strategy<Value = Vec<u8>> {
    (
        prop_oneof![Just(32u32), Just(24u32), 0..255u32], // f= format
        0..2048u32,                                       // s= width
        0..2048u32,                                       // v= height
        proptest::collection::vec(any::<u8>(), 0..4096),  // payload
    )
        .prop_map(|(f, s, v, payload)| {
            // Skip base64 encoding and feed raw bytes
            // → many cases will make `decode_kitty` fail at base64_decode and return None.
            let mut out = format!("Ga=T,f={},s={},v={};", f, s, v).into_bytes();
            out.extend_from_slice(&payload);
            out
        })
}

proptest! {
    #![proptest_config(config())]

    /// Structured APC payloads must still satisfy the postconditions
    /// (dimensions matching the buffer size).
    #[test]
    fn decode_kitty_structured_invariants(input in arb_kitty_apc()) {
        if let Some(img) = decode_kitty(&input) {
            let total = (img.width as u64)
                .checked_mul(img.height as u64)
                .and_then(|v| v.checked_mul(4))
                .map(|v| v as usize);
            prop_assert_eq!(Some(img.rgba.len()), total);
            prop_assert_eq!(img.rgba.len() % 4, 0);
        }
    }

    /// APCs that claim huge dimensions (over 16 MB worth of pixels) must always
    /// return None. `MAX_IMAGE_BYTES = 256 MiB` per the implementation, so
    /// `height=8193, width=8192, RGBA=4 → 256 MiB+`.
    #[test]
    fn decode_kitty_oversized_dimensions_rejected(
        s in 8193u32..=u16::MAX as u32,
        v in 8192u32..=u16::MAX as u32,
    ) {
        // The payload can be empty (dimensional checks happen first).
        let apc = format!("Ga=T,f=32,s={},v={};", s, v).into_bytes();
        let result = decode_kitty(&apc);
        prop_assert!(
            result.is_none(),
            "huge dimensions ({}x{}) should be rejected",
            s, v
        );
    }
}

// ---- Panic resistance through VtParser ----

proptest! {
    #![proptest_config(ProptestConfig {
        // VtParser transitions state byte by byte, so it is somewhat heavier;
        // reduce the case count.
        cases: 64,
        ..config()
    })]

    /// `VtParser` must not panic on arbitrary byte input.
    /// (Complementary to cargo-fuzz: proptest stays in CI for regression detection.)
    #[test]
    fn vt_parser_never_panics(input in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut parser = VtParser::new(80, 24);
        parser.advance(&input);
        // Inspecting the internal state must also be panic-free.
        let _ = parser.bracketed_paste_mode();
        let _ = parser.synchronized_output_mode();
    }

    /// Inputs that include an APC (Kitty graphics) sequence must not panic.
    #[test]
    fn vt_parser_apc_robust(
        prefix in proptest::collection::vec(any::<u8>(), 0..256),
        apc_body in proptest::collection::vec(any::<u8>(), 0..2048),
        suffix in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let mut input = prefix;
        input.extend_from_slice(b"\x1b_");
        input.extend_from_slice(&apc_body);
        input.extend_from_slice(b"\x1b\\");
        input.extend_from_slice(&suffix);
        let mut parser = VtParser::new(80, 24);
        parser.advance(&input);
    }
}
