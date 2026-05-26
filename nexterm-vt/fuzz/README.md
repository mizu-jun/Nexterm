# nexterm-vt fuzzing

A [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html)-based fuzzing harness
introduced in Sprint 3-5. It continuously verifies that the VT parser, the Sixel/Kitty
decoders, and the OSC handlers do not panic, exhaust memory, or hang on arbitrary byte input.

## Targets

| Target | API under test | Threat model |
|--------|---------------|--------------|
| `vt_parser_input` | `VtParser::advance()` | Malformed CSI/OSC/DCS/APC, oversized parameters |
| `sixel_decode` | `image::decode_sixel()` | Oversized repeat counts, malformed colour maps |
| `kitty_image` | `image::decode_kitty()` | Oversized width/height, malformed base64 |
| `osc_url` | OSC 8 / 52 / 133 paths | Oversized URLs, unknown schemes, missing terminators |

## Local execution

```bash
# One-time setup
cargo install cargo-fuzz
rustup toolchain install nightly

cd nexterm-vt

# Run for 60 seconds (same configuration as CI)
cargo +nightly fuzz run vt_parser_input -- -max_total_time=60
cargo +nightly fuzz run sixel_decode    -- -max_total_time=60
cargo +nightly fuzz run kitty_image     -- -max_total_time=60
cargo +nightly fuzz run osc_url         -- -max_total_time=60

# Run indefinitely (crash-hunting mode)
cargo +nightly fuzz run vt_parser_input
```

## CI

`.github/workflows/fuzz.yml` runs each target for 60 seconds **daily at 03:00 UTC (12:00 JST)**.
It can also be triggered manually via `workflow_dispatch`. When a crash is found it is reported
in the GitHub Actions job summary.

## When a crash is found

```bash
# Minimise the crashing payload
cargo +nightly fuzz tmin <target> artifacts/<target>/crash-xxxxx

# Add a regression test
# Promote the byte sequence saved under fuzz/artifacts/ into a unit test.
```

## Workspace exclusion

`nexterm-vt/fuzz/` is excluded from the parent workspace (see the `exclude` list in the root
`Cargo.toml`). It is not part of regular `cargo build` / `cargo test` / `cargo clippy --workspace`
runs and is resolved only when `cargo +nightly fuzz` is invoked inside the fuzz directory.

## Related CRITICAL / HIGH items

- CRITICAL #5: OSC URL allowlist hardening (addressed in Sprint 3-1)
- CRITICAL #7: APC buffer cap (addressed within Sprint 1)
- HIGH #4: Sixel oversized repeat count
- HIGH #5: Kitty image size validation
