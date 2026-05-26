# ADR-0003: Plugin API migration from v1 to v2 and removal timeline

## Status

Accepted (2026-05-12; records the Sprint 4-2 decision retroactively and adds an explicit removal timeline from Sprint 5-4)

## Context

The Nexterm WASM plugin API started at v1. We then introduced v2 to tighten host-side security.

### Problems with v1

- The `pane_id` argument to `nexterm_on_output` was not sanitized, so invalid values could be passed in.
- `write_pane` could write to any pane, so a malicious plugin could interfere with other panes.
- Clipboard writes (OSC 52) and notifications were silently allowed.

### Improvements in v2

- The `pane_id` argument is sanitized.
- `write_pane` is restricted to panes listed in `allowed_panes`.
- Clipboard writes and notifications are validated against a host-side allow list.

### Audit round 2, item A8

The audit flagged "the removal timeline for plugin v1 must be made explicit" (`nexterm-plugin/src/lib.rs:28` says "removal timing TBD"). This ADR finalises the policy, combined with Sprint 5-4 / F1.

## Decision

1. **Plugin API v2 is the standard.** New plugins are written against v2.
2. **v1 support will be removed at the v2.0.0 release.**
3. **Loading a v1 plugin logs a deprecation warning.** (Implemented in Sprint 4-2.)
4. **`PLUGIN_API_VERSION = 2`** (exported from `nexterm-plugin/src/lib.rs`).
5. **All four samples under `examples/plugins/` are already migrated to v2** (Sprint 5-4 / F1).

### Rationale for the removal timeline

- v2.0.0 is the major-version bump where we conventionally collect breaking changes.
- Until then, we keep both code paths so that users who wrote v1 plugins have time to migrate (we maintain dual-support throughout the v1.x line).
- The exact release date of v2.0.0 is decided separately.

## Consequences

### Positive

- Stronger security (no arbitrary-pane writes; no automatic clipboard access).
- The removal timing is clear (v2.0.0), so users can plan their migration.
- `examples/plugins/README.md` ships a v1 → v2 migration guide.

### Negative

- We must maintain dual-support code until v2.0.0 ships.
- Users attached to old plugins will lose them at v2.0.0.
- Migration effort, although minimal: in the simplest case, just add a single `nexterm_api_version()` export.

## Alternatives

- **Alternative A: support v1 indefinitely** — code complexity keeps growing.
- **Alternative B: remove v1 immediately** — too disruptive for existing plugin users.
- **Alternative C: expose the ABI version via configuration** — making v2's hardening opt-in undermines its purpose.

## References

- The `PLUGIN_API_VERSION` definition in `nexterm-plugin/src/lib.rs`
- The v1 → v2 migration guide section in `examples/plugins/README.md`
- Audit round 2, item A8 (explicit removal timing)
- Sprint 5-4 / F1 commit (migrating the example plugins to v2)
