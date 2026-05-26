# ADR-0001: wgpu upgrade strategy

## Status

Investigated (2026-05-11, Sprint 5-3 / C2). Actual code changes are deferred to a future sprint.

## Context

At Nexterm v1.1.0 we use `wgpu = "22"` (as a workspace dependency).
Audit round 2 item C2 asked us to evaluate whether to move to a newer wgpu.

Main motivations:

1. **Resolve RUSTSEC-2024-0436 (paste)**: we initially hoped that going from wgpu 22 → 23 would clear it, but `cargo tree -p nexterm-client-gpu -i paste` shows the chain is `wasmi 0.38 → wasmi_core → paste 1.0.15`. Upgrading wgpu does not resolve it. Fixing this advisory requires bumping the wasmi major version or replacing paste in a separate task.
2. wgpu's own security fixes, performance improvements, and tracking newer GPU APIs.
3. Future alignment with cosmic-text / winit (cosmic-text 0.18 still tolerates the wgpu 22 line).

## Options considered

### A. Stay on wgpu 22

- Pros: zero effort, zero immediate risk, fully compatible with cosmic-text 0.18.
- Cons: we are several versions behind. Catch-up cost grows over the mid-to-long term.

### B. Minimal upgrade to wgpu 23

- Pros: `Instance::new`'s signature is roughly compatible with 22. Most of the APIs we use should require little or no change.
- Cons: 23 is a transitional release — 24/25/26 are already out. We would have to upgrade again soon.

### C. Jump straight to wgpu 26 (the current stable)

- Pros: get the latest improvements (validation, PollType, the MemoryHints choices).
- Cons: several breaking changes must be handled at once.

## Breaking changes from wgpu 22 → 26 that affect this codebase

Verified via `context7`. See the wgpu CHANGELOG for the exact version-by-version diff.

| Impact | 22 | 26 | Location in our code |
|--------|----|----|----------------------|
| `wgpu::ImageCopyTexture` | exists | **removed** → `TexelCopyTextureInfo` | `renderer/mod.rs:1227`, `glyph_atlas.rs:215, 287` |
| `wgpu::ImageDataLayout` | exists | **removed** → `TexelCopyBufferLayout` | `renderer/mod.rs:1234`, `glyph_atlas.rs:226, 298` |
| `wgpu::ImageCopyBuffer` | exists | **removed** → `TexelCopyBufferInfo` | unused |
| `Instance::new(desc)` | returns `Instance` directly | the compatible signature stays, but `new_with_display_handle` / `new_without_display_handle` become the standard | `renderer/mod.rs:146` |
| `request_device(&desc, trace_path)` | second arg `Option<&Path>` | second arg removed; merged into `DeviceDescriptor::trace: wgpu::Trace`. `experimental_features` and a new `memory_hints` shape are also added | `renderer/mod.rs:163-173` |
| `device.poll(Maintain)` | `Maintain` enum | renamed to **`PollType`** | not currently used in our code (needs another check) |
| `PresentMode::AutoVsync` | exists | unchanged (expected) | `renderer/mod.rs:186` |

### Summary of affected sites

- `nexterm-client-gpu/src/renderer/mod.rs`: 5–7 sites
- `nexterm-client-gpu/src/glyph_atlas.rs`: 4 sites
- Total of **~10 mechanical renames** plus field adjustments inside `request_device`.

Also: bump `wgpu = "22"` → `wgpu = "26"` in `Cargo.toml`, regenerate `Cargo.lock`, and re-check compatibility with the rest of the dependency graph (cosmic-text and winit in particular).

## Decision

**Postpone Option B; keep Option A (stay on 22) for now.**

Reasons:

1. We learned that the wgpu upgrade does not resolve RUSTSEC-2024-0436 (the wasmi/paste path needs a separate task). The standalone priority of the wgpu upgrade therefore drops.
2. It is highly likely we will need to upgrade cosmic-text 0.18 → 0.x (the wgpu-26-compatible release) at the same time, which broadens the API impact. That is outside Sprint 5-3's scope (performance and benchmarks).
3. This work is more efficient to do in **Sprint 5-4 or later**, paired with the cosmic-text upgrade.

## Notes for the future-sprint procedure

1. **Preparation**:
   - Check the latest cosmic-text release and look up its wgpu compatibility matrix.
   - Read the latest `winit 0.30.x` release notes (verify there are no breaking changes around `ApplicationHandler`).
2. **Bump dependencies**: update the workspace `Cargo.toml` to `wgpu = "26"` / `cosmic-text = "<compatible version>"`.
3. **Rename pass** (mechanical):
   ```bash
   # Renames within this codebase
   grep -rln "ImageCopyTexture" nexterm-client-gpu/src/ \
     | xargs sed -i 's/ImageCopyTexture/TexelCopyTextureInfo/g'
   grep -rln "ImageDataLayout" nexterm-client-gpu/src/ \
     | xargs sed -i 's/ImageDataLayout/TexelCopyBufferLayout/g'
   ```
4. **Update `request_device`**: add `experimental_features` / `trace` to `DeviceDescriptor`, and drop the second `None` argument.
5. **Smoke testing**:
   - `cargo build -p nexterm-client-gpu`
   - `cargo clippy -p nexterm-client-gpu -- -D warnings`
   - Launch the GUI and confirm no rendering breakage and no perf regression
6. **Bench regression**: the `vt_throughput` suite added in Sprint 5-3 covers the VT layer only and does not depend on wgpu, but compare before/after for reference. If GPU micro-benches exist later, compare those too (none today).

## Related

- Audit round 2: `memory/project_audit_round2.md`, item C2
- Sprint 5-3 progress notes (the origin of this ADR)
- Related tasks (tracked separately):
  - wasmi 0.38 → latest + remove paste dependency (RUSTSEC-2024-0436)
  - cosmic-text 0.18 → latest (C7)
