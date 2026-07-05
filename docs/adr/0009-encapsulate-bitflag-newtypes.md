# ADR-0009: Encapsulate the raw bits of bit-flag newtypes (Attrs, Modifiers)

## Status

Accepted (2026-07-06)

## Context

`nexterm_proto::Attrs` and `nexterm_proto::Modifiers` are bit-flag newtypes that
exposed their inner byte publicly:

```rust
pub struct Attrs(pub u8);
pub struct Modifiers(pub u8);
```

Audit round 3 (finding A5/R5) flagged the `pub u8` as leaking the internal
representation across the crate boundary: every consumer (`nexterm-vt`,
`nexterm-server`, `nexterm-client-gpu`, `nexterm-client-tui`) could read and
mutate `.0` directly, so the encoding could not evolve without touching all of
them, and construction bypassed any central place to validate or document the
flags.

The audit listed this as a "next `PROTOCOL_VERSION` bump" candidate, implying the
change might be wire-breaking. On inspection it is not: `serde`'s derive
serializes a newtype struct as its single inner value regardless of the field's
visibility, and both types are serialized with `postcard`. Making the field
private therefore produces byte-identical output.

## Decision

We will make the inner `u8` of `Attrs` and `Modifiers` private and expose a small
API instead:

- `Attrs::new(bits) -> Self`, `Attrs::bits(self) -> u8`, `Attrs::insert(&mut self, flags)`, `Attrs::remove(&mut self, flags)` (plus the existing `is_bold` / `is_italic` / … predicates).
- `Modifiers::new(bits) -> Self`, `Modifiers::bits(self) -> u8` (plus `is_shift` / `is_ctrl` / `is_alt` / `is_meta`).

All external construction (`Attrs(x)` / `Modifiers(x)`) becomes `::new(x)`, and
all external `.0` reads become `.bits()`. The in-place SGR flag toggling in the
VT parser (`current_attrs.0 |= …` / `&= !…`) uses `insert` / `remove`.

**We will NOT bump `PROTOCOL_VERSION`.** The wire format is unchanged, so a bump
would be misleading and would force an unnecessary handshake incompatibility.

## Consequences

### Positive
- The bit representation is now owned by `nexterm-proto`; consumers go through a
  named API, so the encoding can change later without a cross-crate sweep.
- Flag mutation reads intentionally (`insert(BOLD)` vs `.0 |= BOLD`).
- No wire change, no `PROTOCOL_VERSION` bump, no client/server incompatibility.

### Negative
- One-time mechanical churn across the consumer crates (construction and `.0`
  sites), already applied in this change.
- `new`/`bits` accept/return a raw `u8`, so an out-of-range mask is still
  representable. For bit flags every `u8` is a valid mask, so this is acceptable;
  a future change could switch to a real `bitflags!` type if stricter typing is
  wanted.

## Alternatives

- **`pub(crate)` field**: rejected — the types cross crate boundaries, so
  `pub(crate)` in `nexterm-proto` would not be visible to the other crates.
- **Bump `PROTOCOL_VERSION` anyway**: rejected — the wire format is identical, so
  a bump would create a false incompatibility between otherwise-compatible peers.
- **Adopt the `bitflags` crate**: deferred — a larger change and a new
  dependency; the lightweight newtype + `insert`/`remove` covers current needs.
