# ADR-0007: Snapshot v1 removal timeline

## Status

Accepted (2026-05-12, addressing Sprint 5-5 / A9)

> **Addendum 2026-05-17**: Sprint 5-7 / Phase 2-1 added `workspace_name`, bumping `SNAPSHOT_VERSION` to **v3**. v3 auto-migrates both v1 and v2. Read "v2 is the standard schema" below as "v3 is the standard schema". The plan to raise `SNAPSHOT_VERSION_MIN` to `2` at the v2.0.0 release is unchanged (v2 stays absorbed; only v1 is dropped).

## Context

The session-persistence snapshot in `nexterm-server` currently supports two schema versions in parallel: v1 and v2.

```rust
// nexterm-server/src/snapshot.rs (as of 2026-05-17)
pub const SNAPSHOT_VERSION: u32 = 3;
pub const SNAPSHOT_VERSION_MIN: u32 = 1;
```

### History

- **v1**: the initial schema (`shell_args` was added later and kept compatible via `#[serde(default)]`).
- **v2**: added the `session_title` field. From Sprint 5-1 onwards, `persist::load_snapshot` auto-migrates v1 to v2.
- **v3**: Sprint 5-7 / Phase 2-1 added `workspace_name`. Both v1 and v2 are auto-upgraded to v3 at load time.

### Audit round 2, item A9

The audit flagged that "the removal timing of Snapshot v1 must be made explicit" (the planned raise of `snapshot.rs:34`'s `SNAPSHOT_VERSION_MIN = 1` to `2` at v2.0 was undocumented).

This ADR finalises the policy, mirroring ADR-0003 (Plugin API v2 removal).

## Decision

1. **v2 is the standard schema.** All newly saved snapshots are written as v2 (already implemented).
2. **Loading v1 snapshots will be removed at the v2.0.0 release.**
3. **Loading a v1 snapshot logs a migration warning.** (Existing behaviour is preserved.)
4. **`SNAPSHOT_VERSION_MIN` will be raised to 2 at the v2.0.0 release.**
5. **Migration steps for users holding v1 snapshots**:
   - Start the server at least once while still on a v1.x release; the snapshot is automatically rewritten as v2.
   - Upgrading directly to v2.0.0 without that intermediate step makes v1 snapshots unloadable. Call this out in the CHANGELOG.

### Rationale for the removal timing

- v2.0.0 is the major-version bump where we conventionally collect breaking changes (matches ADR-0003).
- The design auto-migrates during the v1.x line, so users do not need to do anything.
- Removing the v1 fallback branch in `persist.rs` improves maintainability.

## Consequences

### Positive

- The removal date is explicit (v2.0.0), so users can plan their upgrade.
- The v1 migration code in `persist::load_snapshot` can be removed at v2.0.0.
- Bumping `SNAPSHOT_VERSION_MIN` makes the security-relevant compatibility boundary explicit.

### Negative

- We must maintain the migration code until v2.0.0 (small incremental cost, since the code already exists).
- A user who skips the last v1.x release entirely and jumps straight to v2.0.0 loses their session — communicate this in the CHANGELOG / README.

## Alternatives

- **Alternative A: support v1 indefinitely** — migration-branch code lingers forever, and `SNAPSHOT_VERSION_MIN` loses meaning.
- **Alternative B: drop v1 immediately** — disruptive for users that already hold v1 snapshots, especially those keeping long-lived sessions.
- **Alternative C: auto-backup + drop v1** — implementation cost is high. The existing v1→v2 auto-migration already provides equivalent protection.

## References

- `SNAPSHOT_VERSION` / `SNAPSHOT_VERSION_MIN` definitions in `nexterm-server/src/snapshot.rs`
- Migration logic in `nexterm-server/src/persist.rs` (`load_snapshot`)
- ADR-0003: Plugin API v1 → v2 removal timing (same policy)
- Audit round 2, item A9
