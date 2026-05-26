# ADR-0002: `PROTOCOL_VERSION` stays a `u32` with monotonic bumps

## Status

Accepted (2026-05-12; recorded retroactively from the decisions made during Sprints 5-1 and 5-2)

## Context

Nexterm's IPC (Unix socket / named pipe between client and server) exchanges a protocol version during the `Hello` handshake. The granularity, type, and bump policy of that version were vague early on.

Audit round 2 (2026-05-10) raised item A7 — "convert `PROTOCOL_VERSION` from `u32` to a `(major, minor)` tuple and write an ADR" — at MEDIUM priority. Given what was actually implemented in Sprints 5-1 and 5-2, this ADR restates the policy.

### What happened during Sprints 5-1 and 5-2

- **Sprint 5-1 (commit 35b9c5b)**: bumped `PROTOCOL_VERSION` from 1 → 2 → 3 as part of the bincode → postcard migration
- **Sprint 5-2 (commit 829d55b)**: bumped `PROTOCOL_VERSION` from 3 → 4 to add OSC 7 (CwdChanged)
- When an old client connects to a new server (or vice versa), the peer is rejected at `HelloAck` time

### Constraints

- During design we hesitated between semantic versioning (major.minor) and a simple `u32`, and chose `u32` for implementation simplicity.
- We are already at v4; switching to a `(major, minor)` tuple would itself be a breaking change.

## Decision

Keep `PROTOCOL_VERSION: u32` and **bump it by +1 on any change that breaks compatibility**.

We do not introduce a minor / major split. Instead, the server keeps a "minimum acceptable version" and the client also reads `HelloAck.server_proto_version` during the handshake to confirm compatibility.

## Consequences

### Positive

- The implementation is simple (one field, very little branching logic).
- Existing code continues to work as-is (v1 through v4 are already in place).
- The version number can grow as large as needed (`u32` = ~4.2 billion).

### Negative

- A fine-grained migration strategy such as "minor bumps preserve backwards compatibility" is not available.
- Every breaking change becomes a uniform "+1", so the magnitude of a change is not visible from the number.
- The compatibility check is exact-match, so even a backwards-compatible field addition still requires a bump.

## Alternatives

- **Alternative A: `(major, minor)` tuple**: more flexible, but we are already at v4 and the switch itself would be breaking. The ratio of added maintenance cost to gained flexibility is low.
- **Alternative B: SemVer string (e.g. `"1.4.0"`)**: over-abstracted. Requires parsing during the handshake.
- **Alternative C: per-message `version` field**: over-complicated. We do not expect the version to change within a single session.

## References

- Sprint 5-1 progress: `memory/project_sprint5_1_progress.md` (history of the bump to PROTOCOL_VERSION 3)
- Sprint 5-2 progress: `memory/project_sprint5_2_progress.md` (bump to PROTOCOL_VERSION 4)
- commit 35b9c5b: bincode → postcard + PROTOCOL_VERSION 3
- commit 829d55b: OSC 7 CwdChanged + PROTOCOL_VERSION 4
- Audit round 2, item A7 (re-evaluated; we keep `u32`)
