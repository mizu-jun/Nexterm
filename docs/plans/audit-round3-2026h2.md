# Audit Round 3 (v1.12.0) — Security / Performance / Correctness

- **Date:** 2026-07-05
- **Target:** `master` at v1.12.0
- **Method:** Four parallel review passes (security, performance, concurrency/bugs, Rust-specific),
  every finding re-verified against the current source before inclusion.
- **Predecessors:** Round 1 (completed at v1.1.0), Round 2 (70 items, most HIGH resolved).
  This round only reports findings not already tracked or resolved there.

## Implementation status

- **Done:** P1, B1, P2, R1 (PR #26), P4 (PR #27), S1 and C2 (this change).
- **Deferred (still tracked below):** P3 and P6 (need profiling), P5 and R5/A5
  (require a `PROTOCOL_VERSION` bump), P7 (large lock-architecture refactor).

### C2 note

C2 was reclassified from LOW/dormant to a reachable bug: `SerialPane::spawn` is
wired into `Window::add_serial_pane`, so serial panes are live. The fix mirrors
the PTY `Pane` grid snapshot (a reader-maintained `latest_grid` applied via
`Grid::apply_dirty_row`) and threads serial panes through the attach and lag
-resync refresh paths (`Window::focused_pane_full_refresh` /
`all_full_refreshes`). DA/DSR write-back is not added — serial devices do not run
the PSReadLine handshake that made it necessary for ConPTY.

## Baseline statistics

| Metric | Value | Note |
|--------|-------|------|
| `unwrap()` outside tests | 438 | Mostly config/cache load paths |
| `unsafe` blocks | 35 | All carry a `SAFETY:` comment (spot-verified 100%) |
| `Mutex`/`RwLock::new` | 35 | — |
| TODO/FIXME/HACK | 1 | — |
| clippy `-D warnings` / fmt / cargo-deny | green | CI, 3-OS matrix |

## Important corrections from verification

- **S1 (session token timing attack) is LOW, not CRITICAL.** The reviewer flagged the `HashMap`
  lookup in `web/auth.rs::is_valid` as a CRITICAL timing side channel. Verified against source:
  Rust's `HashMap` uses SipHash with a per-process random key, so lookup time is dominated by the
  keyed hash, and byte comparison only runs on a hash collision. Tokens are 48-char alphanumeric
  (~285 bits of entropy, random per session), and HTTP round-trip jitter buries nanosecond-scale
  comparison differences. Practical exploitability is negligible. `auth.rs` already has adversarial
  tests for forged / tampered / expired tokens. Constant-time comparison remains reasonable
  defense-in-depth but is **not urgent**.
- **B1 (poisoned-lock panic cascade) is confirmed and widespread.** An initial grep pattern missed
  it, but the chained form `.plugin_manager` / `.lock()` / `.expect("plugin_manager poisoned")` does
  exist at `plugin_dispatch.rs:20,43,78,120` and `session.rs:626`, and the same
  `std::sync::Mutex` + `.expect("...poisoned")` pattern appears across `web/auth.rs`, `oauth.rs`,
  `handlers/login.rs`, `handlers/page.rs`.

## Findings

Severity is the post-verification reassessment, which may differ from the raw reviewer output.

### HIGH

#### P1 — PTY reader clones the whole grid on every dirty batch  *(confirmed)*

- **Location:** `nexterm-server/src/pane.rs:874-875`
- **Problem:** In the reader loop, every time dirty rows appear the code runs
  `*g = parser.screen().full_refresh_grid()`, cloning every cell of the screen into the heap. The
  `GridDiff` broadcast itself only carries the dirty rows (efficient); the `latest_grid` update
  beside it is the full copy. Under heavy output (`yes`, `cat largefile`) this burns memory
  bandwidth proportional to (cell count × dirty frequency), multiplied across panes. Two independent
  performance passes converged on this exact line.
- **Fix direction:** Apply only the changed rows to `latest_grid`; the full clone is only needed at
  attach time.

```rust
// Apply only the changed rows to latest_grid instead of cloning the whole grid.
let dirty = parser.screen_mut().take_dirty_rows();
if !dirty.is_empty() {
    if let Ok(mut g) = latest_grid_clone.lock() {
        for row in &dirty {
            g.apply_row(row); // requires a small apply_row(&DirtyRow) on Grid
        }
        let (cc, cr) = parser.screen().cursor();
        g.set_cursor(cc, cr);
    }
    let (cursor_col, cursor_row) = parser.screen().cursor();
    let _ = tx_reader.send(ServerToClient::GridDiff {
        pane_id,
        dirty_rows: dirty,
        cursor_col,
        cursor_row,
    });
}
```

- **Measurement before committing:** `criterion` comparing `full_refresh_grid()` vs
  `apply_row × dirty_count`; on real hardware, watch the reader thread's busy time in
  `tokio-console` while running `yes | head -c 100M`.

### MEDIUM

#### B1 — Poisoned lock causes a panic cascade  *(confirmed)*

- **Location:** `nexterm-server/src/ipc/plugin_dispatch.rs:20,43,78,120`, `session.rs:626`,
  `web/auth.rs` (57/87/96/110/117), `web/oauth.rs` (99/123), `web/handlers/login.rs`, `page.rs`
- **Problem:** These `std::sync::Mutex` locks use `.expect("...poisoned")`. If any code panics while
  holding one of these locks, the mutex becomes poisoned and every subsequent access panics too,
  taking down the whole feature (plugins, web sessions) in a cascade.
- **Fix direction:** Recover the inner value instead of propagating the panic.

```rust
// Recover from a poisoned lock instead of cascading panics.
let mut lock = manager.plugin_manager.lock().unwrap_or_else(|poisoned| {
    tracing::error!("plugin_manager mutex poisoned; recovering inner state");
    poisoned.into_inner()
});
```

#### P2 — `take_dirty_rows` allocates the result Vec without capacity  *(confirmed)*

- **Location:** `nexterm-vt/src/screen.rs` (`take_dirty_rows`)
- **Problem:** Builds the result with `Vec::new()` and `push`, reallocating as dirty rows accumulate.
- **Fix direction:** Count dirty rows first, then `Vec::with_capacity(count)`. Low cost, clear win.

#### P3 — Cursor blink may rebuild all pane vertices  *(needs measurement)*

- **Location:** `nexterm-client-gpu/src/renderer.rs`
- **Problem:** The per-pane vertex cache (C4) invalidation condition includes cursor movement, so a
  blink alone may rebuild the full cell vertex buffer of a pane, even on an otherwise idle screen.
- **Fix direction:** Split the cursor into a separate lightweight vertex buffer so the text cache
  stays valid across blinks.
- **Measurement:** Count `build_pane_vertices` calls/sec via `tracing` (ideal: 0 while idle).

#### P4 — Broadcast saturation drops GridDiff with no recovery path  *(confirmed)*

- **Location:** `nexterm-server/src/session.rs` (broadcast channel, capacity 2048)
- **Problem:** For a slow/remote client that can't keep up, `RecvError::Lagged` drops older messages
  and the screen corrupts until the next FullRefresh.
- **Fix direction:** On `Lagged`, trigger a FullRefresh to resync the client.

```rust
match rx.recv().await {
    Ok(msg) => forward(msg).await,
    Err(broadcast::error::RecvError::Lagged(n)) => {
        warn!("client lagged by {n} messages, requesting full refresh");
        request_full_refresh(pane_id).await;
    }
    Err(broadcast::error::RecvError::Closed) => break,
}
```

#### R1 — Integer overflow in glyph atlas capacity math  *(confirmed, local config only)*

- **Location:** `nexterm-client-gpu/src/glyph_atlas.rs:160,207`
- **Problem:** `(size * size)` and `(atlas_size * atlas_size)` are unchecked `u32` multiplications.
  `atlas_size` is user-configurable (`gpu.atlas_size: u32` in config). At `atlas_size >= 65536` this
  overflows (debug: panic, release: wrap → wrong LRU capacity). Not remote-attacker input, so
  LOW–MEDIUM, but worth hardening.

```rust
fn lru_cap_from_cell(atlas_size: u32, cell_w: u32, cell_h: u32) -> NonZeroUsize {
    // Guard against u32 overflow in atlas_size * atlas_size and cell_w * cell_h.
    let atlas_sq = (atlas_size as u64).saturating_mul(atlas_size as u64);
    let cell_area = (cell_w as u64).saturating_mul(cell_h as u64).max(1);
    let cap = (atlas_sq / cell_area).max(256).min(usize::MAX as u64) as usize;
    NonZeroUsize::new(cap).unwrap_or(NonZeroUsize::MIN)
}
```

### LOW / known tech debt

- **C2 — `serial.rs` same-shape bug as the old Pane grid bug.** `make_full_refresh` returns an empty
  grid, GridDiff is dropped before attach, and there is no DA/DSR response write-back. The Pane side
  was fixed in v1.9.3–v1.9.5; the serial side is not. Currently `#[allow(dead_code)]` / unused, so it
  is dormant, but the same fix is required before serial support is used for real.
  See `pty-grid-sharing-architecture` memory.
- **P5 — No in-row RLE compression for GridDiff over the web terminal.** Negligible for local IPC,
  matters for bandwidth over WebSocket. Requires a PROTOCOL bump; measure cost/benefit first.
- **P6 — glyph atlas `LruCache::get` takes `&mut self`.** Use `peek` where LRU promotion is not
  needed. Profile first.
- **P7 — SessionManager guarded by a single Mutex.** Contends with many panes + multiple clients;
  small impact at current pane counts. Same family as the known ClientState responsibility split.
- **S1 — Constant-time session token comparison.** Defense-in-depth, not urgent (see corrections).
- **R5 / A5 — Over-exposed `pub` fields such as `Modifiers(pub u8)`.** A newtype could forbid
  invalid values; candidate for the next PROTOCOL bump.

## Areas verified clean

IPC UID validation (SO_PEERCRED / getpeereid + 0600 perms; named pipe `reject_remote_clients`),
IPC message-length cap (`validate_msg_len`), SSH password `Zeroizing` and host-key TOFU/known_hosts,
WASM sandbox (fuel 10M / memory 16 MiB / input sanitization), Lua sandbox
(`os.execute`/`io`/`require` removed), image decode `checked_image_bytes` (256 MiB cap),
atomic writes (tmp + rename + 0600), snapshot v1→v4 migration, and `SAFETY:` coverage on every
`unsafe` block.

## Recommended order

1. **P1** — the only HIGH; the clear heavy-output bottleneck (confirm with a benchmark).
2. **B1** — poison-cascade recovery; mechanical, low-risk change.
3. **P2 / R1** — cheap, preventive.
4. **P4 / P3** — decide after measurement.

## Notes

- Findings are from static analysis plus targeted source verification. All performance items marked
  "needs measurement" should be confirmed with `criterion` / `tokio-console` / a GPU profiler before
  optimization.
- Implementation is intentionally out of scope for this document; it lists findings and fix
  directions only.
