# ADR-0006: IPC serializer — migrate from bincode 1.x to postcard

## Status

Accepted (2026-05-12; records the Sprint 5-1 / G3 decision retroactively)

## Context

Nexterm's IPC (Unix socket on Linux/macOS, named pipe on Windows, between client and server) sends `ClientToServer` / `ServerToClient` messages through serde.

### Initial implementation

We started with `bincode 1.x`. It is widely used in the Rust ecosystem, serde-compatible, and concise.

### What went wrong (2025)

- **RUSTSEC-2025-0141**: an advisory was published noting that bincode 1.x is at risk of becoming unmaintained.
- We worked around it by adding ignores to `.cargo/audit.toml` / `deny.toml` for `cargo audit`, but that is not a fundamental fix — a dependency-side advisory could still become a CVE in the future.

### Candidates

- **bincode 2.x**: the API is redesigned, so there is a migration cost, but it is in the same family.
- **postcard**: designed for no_std / embedded but a good fit for Nexterm. Smaller payloads, actively maintained, no RUSTSEC advisories.
- **rmp-serde (MessagePack)**: highly compatible, but loses to postcard on both size and speed.

### Audit round 2, item G3

The audit flagged "migrate from bincode 1.x to postcard" at CRITICAL priority — one of the top items for Sprint 5+.

## Decision

**Migrate to postcard 1.x.**

### Changes made in Sprint 5-1

1. **`nexterm-proto/Cargo.toml`** — drop the bincode dependency and add postcard.
2. **Rewrite serialization at every IPC endpoint to postcard**:
   - `nexterm-server/src/ipc/`
   - `nexterm-client-gpu/src/connection.rs`
   - `nexterm-client-tui/`
   - `nexterm-ctl/src/ipc.rs`
   - `nexterm-launcher/` (removed in v1.4.0)
3. **Bump `PROTOCOL_VERSION` 1 → 2 → 3** to mark the migration stages.
   - v1: bincode
   - v2: postcard with the old field layout
   - v3: postcard with the message layout cleaned up in Sprint 5-1
4. **Add postcard round-trip tests** (`#[cfg(test)]` module in `nexterm-ctl/src/main.rs`).
5. **Remove the bincode ignore from `cargo audit`.**

### Migration notes

- The message-length prefix (4-byte LE) is unchanged.
- Byte counts are slightly smaller (postcard's varint encoding is more efficient).
- Benches (measured in Sprint 5-3): parse speed is on par or slightly faster.

## Consequences

### Positive

- Resolves the RUSTSEC advisory.
- One entry removed from the `cargo audit` ignore list.
- Binaries are slightly smaller (postcard varints).
- Depends on an actively maintained library.

### Negative

- Migration was an L-sized effort (spread across many crates).
- postcard's API is subtly different from bincode (some learning cost).
- Old v1 clients are no longer compatible — clients and servers must update together.

## Alternatives

- **Alternative A: stay on bincode 1.x with ignores** — carries the future-CVE risk forever.
- **Alternative B: upgrade to bincode 2.x** — comparable migration cost (the API was redesigned), and similar problems could recur later.
- **Alternative C: rmp-serde (MessagePack)** — loses on size and speed compared to postcard.
- **Alternative D: a custom format** — needlessly complex.

## References

- Sprint 5-1 progress: `memory/project_sprint5_1_progress.md`
- Audit round 2, item G3
- RUSTSEC-2025-0141: bincode 1.x advisory
- postcard upstream: https://github.com/jamesmunns/postcard
- Sprint 5-3 benchmarks: `docs/benchmarks.md` (postcard's impact on VT-layer parse speed is negligible)
