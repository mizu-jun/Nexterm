#![no_main]
//! Fuzz the postcard deserializer for `ClientToServer` and `ServerToClient`.
//!
//! Motivation: the IPC socket boundary is one of the highest-trust attack
//! surfaces in the project — any local process that can connect to the UDS /
//! named pipe can speak postcard. The deserializer must therefore tolerate
//! arbitrary byte input without panicking, regardless of the wire validity.
//!
//! Scenarios in scope:
//! - Truncated / oversized variant tags.
//! - Lengths that point past the end of the buffer.
//! - Malformed UTF-8 inside string fields.
//! - Recursive `PaneLayout` trees that exhaust the stack (postcard does not
//!   recurse, but the resulting `PaneLayout` may be reconstructed downstream).
//!
//! This complements `validate_msg_len` (which guards `MAX_MSG_LEN`) by
//! covering the bytes-to-typed-message step.

use libfuzzer_sys::fuzz_target;
use nexterm_proto::{ClientToServer, ServerToClient};

fuzz_target!(|data: &[u8]| {
    // Both directions must be robust against arbitrary input. The fuzzer
    // explores the union of both decode paths.
    let _ = postcard::from_bytes::<ClientToServer>(data);
    let _ = postcard::from_bytes::<ServerToClient>(data);
});
