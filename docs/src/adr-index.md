# Architecture Decision Records

The Nexterm project tracks important architectural decisions as ADRs.
The canonical index lives in the repository root under `docs/adr/`.

For the up-to-date list, see [docs/adr/README.md on GitHub](https://github.com/mizu-jun/Nexterm/blob/master/docs/adr/README.md).

## Current ADRs (snapshot)

| ID | Title |
|----|-------|
| 0001 | wgpu 22 → 26 upgrade strategy |
| 0002 | `PROTOCOL_VERSION` as u32 + minor-bump policy |
| 0003 | Plugin API v1 → v2 migration and removal timing |
| 0004 | TOML + Lua hybrid configuration |
| 0005 | BSP tree for pane layout |
| 0006 | IPC serializer: bincode 1.x → postcard migration |
| 0007 | Snapshot v1 deprecation (removed at v2.0.0) |

## When to write a new ADR

Write an ADR for any decision a future maintainer would ask “why?” about:

- Choosing a framework / library / protocol
- Backwards-compatibility policy
- Core data structure design
- Security or performance trade-offs

See `docs/adr/template.md` in the repository for the format.
