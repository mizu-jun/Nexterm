# Architecture Decision Records (ADR)

This directory records the important architectural decisions taken in Nexterm.

## What is an ADR?

An Architecture Decision Record (ADR) is a lightweight document that captures **why** a significant technical choice was made in a project. Reading the code tells you *what* it does, but the *why* fades with time, so we capture it in ADRs.

Further reading: [Michael Nygard, "Documenting Architecture Decisions"](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions).

## When to write an ADR

Record decisions of the kind someone is likely to ask **"why did we end up like this?"** about later:

- Choices of framework / library / protocol (e.g. bincode → postcard)
- Backwards-compatibility and versioning policy (e.g. PROTOCOL_VERSION, Plugin v1 → v2)
- Core data-structure design (e.g. the BSP-tree pane split)
- Security trade-offs (e.g. whether TLS fallback is allowed)
- Trade-offs between performance and simplicity (e.g. the `present_mode` default)

When in doubt, write one — even a short note tends to be more useful than no note.

## ADR index

| ID | Title | Status | Date |
|----|-------|--------|------|
| [0001](0001-wgpu-upgrade.md) | wgpu 22 → 26 upgrade strategy | Accepted | 2026-05-12 |
| [0002](0002-protocol-versioning.md) | `PROTOCOL_VERSION` stays a `u32` with monotonic bumps | Accepted | 2026-05-12 (retroactive) |
| [0003](0003-plugin-api-v2.md) | Plugin API v1 → v2 migration and removal timing | Accepted | 2026-05-12 (retroactive) |
| [0004](0004-toml-lua-config.md) | Hybrid configuration with TOML + Lua | Accepted | 2026-05-12 (retroactive) |
| [0005](0005-bsp-pane-layout.md) | BSP-tree pane layout | Accepted | 2026-05-12 (retroactive) |
| [0006](0006-postcard-vs-bincode.md) | IPC serializer: bincode 1.x → postcard | Accepted | 2026-05-12 (retroactive) |
| [0007](0007-snapshot-v1-deprecation.md) | Snapshot v1 removal timing (raise `SNAPSHOT_VERSION_MIN` to 2 at v2.0.0) | Accepted | 2026-05-12 |

## How to add a new ADR

1. Copy `template.md` to a new file with the next sequence number (e.g. `0008-xxx.md`).
2. Start with status "Proposed", review with stakeholders, then move to "Accepted".
3. Add a row to the ADR index table in this `README.md`.
4. After acceptance, **do not rewrite the body**. If you need to change the decision, raise a new ADR and link it back as "superseded by".

## Status values

- **Proposed**: under discussion; not yet reflected in code.
- **Accepted**: decided and reflected in the codebase.
- **Deprecated**: no longer applies (with no successor).
- **Superseded (ADR-NNNN)**: replaced by a later ADR.

## Related documents

- [docs/benchmarks.md](../benchmarks.md) — performance-measurement reference
- [CLAUDE.md](../../CLAUDE.md) — repository overview and coding conventions
- [memory/](../../memory/) — sprint progress and audit results (Claude memory)
