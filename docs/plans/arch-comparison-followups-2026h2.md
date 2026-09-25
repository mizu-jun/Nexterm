# Architecture-Comparison Follow-ups (2026 H2)

- **Status:** Planning — triage only, no code changes made in this pass.
- **Source:** `docs/plans/audit-round4-2026h2.md`, §3 ("Architecture comparison, 5
  dimensions, 21 findings"), Round 4 audit (v1.17.1, 2026-09-11).
- **Purpose:** Break the 21 architecture-comparison insights out of the audit report and
  into a prioritized, actionable backlog. This document does not implement any fix; it
  decides *what order* to pick things up in and *how big* each item is, so a future
  session (or a stack of PRs) can execute directly from the table below instead of
  re-reading the full audit narrative each time.
- **Relationship to the 46 confirmed bugs:** those are tracked separately in
  `audit-round4-2026h2.md` §1 and are out of scope here — this document is exclusively
  the 21 comparison-derived insights (§3.1–§3.5 of that report).

## Priority legend

| Tier | Meaning |
|---|---|
| **P0** | Confirmed correctness/security gap, small-to-medium fix, do next |
| **P1** | Confirmed bug or meaningful robustness fix, moderate effort |
| **P2** | Quality-of-life, diagnostics, or low-effort doc addition |
| **P3** | Deferred — blocked on an unshipped feature, or speculative until a real need appears |
| **N/A** | No action needed (already fixed, or confirmed non-issue) |

Effort is a rough T-shirt size (S = under an hour of focused work, M = a session, L =
multi-session/needs its own plan doc).

## Backlog

| # | Priority | Effort | Insight | Where | Suggested action |
|---|---|---|---|---|---|
| 1 | P2 | M | Single-binary `CloseAction::Detach` is effectively a kill (server thread dies with the process); the code's own comment admits this | `nexterm-client-gpu/src/renderer/event_handler/window.rs`, `main.rs`, `nexterm-server/src/ipc/platform.rs` | Make this explicit in the close-confirmation UI copy; consider a single-instance guard in `serve_unix()` (probe the existing socket before `remove_file()`+`bind()`) as a first step toward real detach/reattach |
| 2 | P3 | L | Scrollback is out of scope for snapshot persistence today ("future work" per the crate's own doc comment) | `nexterm-server/src/snapshot.rs` | No action until scrollback persistence is actually planned; when it is, mirror Zellij's opt-in, capped `scrollback_lines_to_serialize` design rather than defaulting to unbounded capture |
| 3 | P0 | S | Snapshot write path (`write_atomic_secure`) checks ownership/mode; **read path (`load_snapshot`) performs no `stat()` at all** — an asymmetry | `nexterm-server/src/persist.rs` | Add an OpenSSH-style "permissions are too open" check before reading, plus a size cap, both falling into the existing fail-closed pattern |
| 4 | N/A | — | Doc-drift: `nexterm-server/CLAUDE.md` said Schema v3; actual `SNAPSHOT_VERSION` is 5 | `nexterm-server/CLAUDE.md` | Already corrected in the audit-round-4 pass itself — no further action |
| 5 | P1 | S | `bsp.rs::compute()`'s `(cols as f32 * ratio) as u16` truncates instead of rounds (sway fixed the identical bug in 2019); left/top child always loses the rounding error | `nexterm-server/src/window/bsp.rs` | Switch to `.round()` in both `bsp.rs::compute()` and `tiling.rs::compute_pane_sizes()`; add a regression test pinning `cols=81, ratio=0.5 → 41` |
| 6 | P0 | M | `adjust_ratio_for` always resizes the **outermost (root) split**, not the split closest to the focused pane — a real, always-reproducible UI bug (dragging an inner divider in a 3+ pane layout moves the outer split instead) | `nexterm-server/src/window/mod.rs` (or wherever `adjust_ratio_for` lives) | Attach a generation counter to `Window`, echo it in the layout snapshot the client already receives, and make `handle_resize_split` a no-op on a stale generation instead of applying blindly. Fixing the "wrong split" targeting itself is the bulk of the work here — file as its own bug, not just a race fix |
| 7 | P1 | S | Very small splits (`cols=1`) can produce a logically out-of-bounds child rect; the crate's own test comments already acknowledge `rows=0` rects are possible | `nexterm-server/src/window/bsp.rs` | Pick one explicit policy — either refuse the split (tmux-style) or clamp-and-disable (sway's 2025 fix) — rather than leaving the gap implicit |
| 8 | P1 | M | Ratio→cell conversion is independently duplicated in `bsp.rs::compute()` (live tree) and `tiling.rs::compute_pane_sizes()` (snapshot-restore path), with no shared implementation and no test that the two agree | `nexterm-server/src/window/bsp.rs`, `tiling.rs` | Extract a single `split_extent()` function both call, plus a proptest asserting parity between the two call sites |
| 9 | P2 | S | ADR-0005's rejection of a Zellij-style tiling array cites "resize logic gets complex" — Zellij's actual complexity is specifically N-ary percentage redistribution, a problem Nexterm's strictly-binary tree structurally avoids | `docs/adr/0005-*.md` | One-line addition to the ADR for accuracy; no code change |
| 10 | P2 | S | The open macOS-only `chrome_advance` `inf`-for-CJK bug matches a known **class** of bug (Ghostty `#8712`, Alacritty `PR #1029`, WezTerm `#614`) rather than being unique to Nexterm, but root cause is still unknown | `nexterm-client-gpu/src/font.rs` | When `chrome_advance` produces a non-finite value, log which face/family cosmic-text actually resolved to (macOS-gated or `NEXTERM_LOG=trace`) — turns "root cause unknown" into a one-shot diagnostic for the next macOS+CJK contributor |
| 11 | N/A | — | Terminal-grid width judgement (`unicode_width` static tables) is already fully independent of font shaping, matching WezTerm's/kitty's design — confirmed not to share the macOS `inf` bug's blast radius | `nexterm-client-gpu/src/font.rs` | No action — confirmed correct as-is |
| 12 | N/A | — | Glyph-atlas overflow clears `self.cache` but not `self.ligature_cache`, while the atlas cursor resets — found *during* the architecture fact-check | `nexterm-client-gpu/src/glyph_atlas.rs:376` | Already tracked and fixed as bug #16 in `audit-round4-2026h2.md` §1.2 — no duplicate work here, cross-reference only |
| 13 | P1 | S | `measure_char_width`'s '0'-only measurement is unproven-safe, not trap-proof — the project's own design doc confirms there is no finiteness guard, contradicting an earlier claim that the narrow scope was a deliberate CJK-trap dodge | `nexterm-client-gpu/src/font.rs:222` | Add an explicit finiteness guard (`is_finite()` check with a safe fallback), matching the pattern already used elsewhere in this round (see `finite_or` helper added for opacity clamping) |
| 14 | P3 | L | Future `nexterm.*` Lua namespace (planned per ADR-0004) risks a second, independently-drifting capability check unless it reuses the existing WASM guard order | `nexterm-plugin` (WASM side), Lua config/status-bar runtime | Blocked until the `nexterm.*` Lua namespace is actually built; when it is, route it through the same `ReadFn`/`ConsentPolicy`/guard-order already defined in ADR-0008 rather than inventing a parallel Lua-specific check |
| 15 | P3 | L | The single global `plugin_read: ConsentPolicy` field does not scale past one capability — granting plugin A read access silently grants it to every other loaded plugin too | `nexterm-plugin`, `SecurityConfig` | No second host-function family is shipping yet, so this is speculative; when one does, Zellij's `PermissionCache` (path → `Vec<PermissionType>`, persisted, per-plugin) is documented prior art for where `SecurityConfig` needs to grow |
| 16 | P2 | S | The tmux+TPM zero-sandbox baseline (unconditional full-account shell execution) is a legitimate comparison quantifying what Nexterm's WASM boundary buys, but currently only exists implicitly across several ADRs | `docs/PRODUCT.md`, `examples/plugins/README.md` | Add one explicit paragraph making this comparison to justify the WASM sandbox's engineering cost — doc-only |
| 17 | P2 | S | Zellij migrated wasmer → wasmtime → wasmi (2026-03-23); Nexterm is pinned two major versions behind on wasmi (0.38 vs. upstream 2.0.0) | `nexterm-plugin/Cargo.toml` | Add to the existing dependency-review rotation alongside russh/lru (see `dependency-followups` memory) rather than a one-off bump — a larger, WASI-linked consumer stress-testing the same interpreter is a leading indicator for where interpreter bugs surface first |
| 18 | P1 | M | `docs/THREAT_MODEL.md` claimed a Windows named-pipe DACL was set; no such code exists — `serve_named_pipe()` only sets `first_pipe_instance(false)` / `reject_remote_clients(true)` (doc-drift already corrected this round; the underlying gap is still open) | `nexterm-server/src/ipc/platform.rs` (Windows) | Three-part hardening: ① `first_pipe_instance(true)` on first bind to fail fast on pipe squatting; ② `SetNamedSecurityInfoW` with a creator-SID-only DACL; ③ `ImpersonateNamedPipeClient` + `GetTokenInformation` to check the connecting SID, mirroring Unix's `verify_peer_uid()`. Windows-only — needs a Windows dev/CI box to verify, hence M not S |
| 19 | P0 | S | `verify_peer_uid()`'s reject-on-mismatch branch has no test coverage (confirmed by grep) | `nexterm-server/src/ipc/platform.rs` (Unix) | Add a `UnixStream::pair()`-based unit test passing a deliberately wrong UID — no root or second real user needed, small and high-value |
| 20 | P2 | M | `nexterm-ctl` only prints locale-dependent fixed-width text via `fl!`; no scriptable structured output exists | `nexterm-ctl` | Add a `--json`/NDJSON output mode, giving tmux's "binary main line + scriptable text layer" split without touching the hot IPC path |
| 21 | P2 | S | Windows Terminal's Monarch/Peasant model is a different product concept (window/command routing, not a PTY-holding daemon) — not a security gap, but the reason Nexterm accepts a different attack surface is undocumented | `docs/THREAT_MODEL.md` | One-line note explaining *why* Nexterm accepts a persistent, detachable-session attack surface that Windows Terminal's model doesn't have — doc-only |

## Suggested execution order

1. **P0 first** (#3, #6, #19) — one confirmed UI-correctness bug and two small, well-scoped
   security-hardening gaps. All three are S/M effort and self-contained.
2. **P1 batch on the pane-layout crate** (#5, #7, #8) — same file family (`bsp.rs` /
   `tiling.rs`), worth doing together in one PR to avoid re-touching the same functions
   across separate changesets.
3. **P1 remainder** (#13, #18) — #13 is a quick finiteness-guard fix; #18 is Windows-only
   and should be scheduled when Windows CI/dev access is available.
4. **P2 doc-only batch** (#9, #16, #21) — trivial, can be done in a single small PR
   alongside whichever code PR touches the same area, or standalone.
5. **P2 remainder** (#1, #10, #17, #20) — independent, pick up opportunistically.
6. **P3** (#2, #14, #15) — do not schedule; revisit only when the blocking feature
   (scrollback persistence, the Lua `nexterm.*` namespace, or a second plugin capability)
   is actually proposed.
7. **N/A** (#4, #11, #12) — no further action; kept in this table only for the "21
   insights fully triaged" record.

## Non-goals for this document

- No code was changed in the session that produced this triage.
- This document does not re-litigate the fact-check corrections themselves (tmux ACL
  parity, the render-pass count, the Lua `pane_id` visibility, etc.) — those are recorded
  as-is in `audit-round4-2026h2.md` §3 and are treated here as settled inputs.
- Items should graduate out of this table into GitHub issues or their own
  `docs/plans/*.md` file as they are picked up; this table's job is prioritization, not
  long-term tracking.
