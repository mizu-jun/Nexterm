# Architecture-Comparison Follow-ups (2026 H2)

- **Status:** P0 and part of P1 implemented and merged (2026-09-25/26, PRs #116 and
  follow-up). See the "Progress" section below for what shipped, what turned out to
  already be fixed, and what was deferred with a documented reason.
- **Source:** `docs/plans/audit-round4-2026h2.md`, §3 ("Architecture comparison, 5
  dimensions, 21 findings"), Round 4 audit (v1.17.1, 2026-09-11).
- **Purpose:** Break the 21 architecture-comparison insights out of the audit report and
  into a prioritized, actionable backlog, then track execution against it as items are
  picked up in priority-tier PRs.
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

| # | Priority | Effort | Status | Insight | Where | Suggested action |
|---|---|---|---|---|---|---|
| 1 | P2 | M | Not started | Single-binary `CloseAction::Detach` is effectively a kill (server thread dies with the process); the code's own comment admits this | `nexterm-client-gpu/src/renderer/event_handler/window.rs`, `main.rs`, `nexterm-server/src/ipc/platform.rs` | Make this explicit in the close-confirmation UI copy; consider a single-instance guard in `serve_unix()` (probe the existing socket before `remove_file()`+`bind()`) as a first step toward real detach/reattach |
| 2 | P3 | L | Deferred | Scrollback is out of scope for snapshot persistence today ("future work" per the crate's own doc comment) | `nexterm-server/src/snapshot.rs` | No action until scrollback persistence is actually planned; when it is, mirror Zellij's opt-in, capped `scrollback_lines_to_serialize` design rather than defaulting to unbounded capture |
| 3 | P0 | S | ✅ Done (PR #116) | Snapshot write path (`write_atomic_secure`) checks ownership/mode; **read path (`load_snapshot`) performs no `stat()` at all** — an asymmetry | `nexterm-server/src/persist.rs` | Added an OpenSSH-style "permissions are too open" check plus a 16 MiB size cap before reading, falling into the existing fail-closed pattern |
| 4 | N/A | — | ✅ Done (audit pass itself) | Doc-drift: `nexterm-server/CLAUDE.md` said Schema v3; actual `SNAPSHOT_VERSION` is 5 | `nexterm-server/CLAUDE.md` | Already corrected in the audit-round-4 pass itself — no further action |
| 5 | P1 | S | ✅ Done (P1 PR) | `bsp.rs::compute()`'s `(cols as f32 * ratio) as u16` truncates instead of rounds (sway fixed the identical bug in 2019); left/top child always loses the rounding error | `nexterm-server/src/window/bsp.rs` | Switched to `.round()` inside a shared `split_extent()` helper (see #8); regression test pins `cols=81, ratio=0.5 → 41` |
| 6 | P0 | M | ✅ Done (PR #116) | `adjust_ratio_for` always resizes the **outermost (root) split**, not the split closest to the focused pane — a real, always-reproducible UI bug (dragging an inner divider in a 3+ pane layout moves the outer split instead) | `nexterm-server/src/window/bsp.rs` | Fixed the targeting bug itself: recurse into the containing child first, only adjust the current node's ratio when that call bottoms out. **Scope note:** the generation-counter race hardening originally bundled into this item was *not* implemented — it needs a wire-protocol change (new field on `LayoutChanged` / `ResizeSplit`) across client and server, which is a separate, larger change. Split out as new item **#22** below rather than silently dropped |
| 7 | P1 | S | ✅ Done (P1 PR) | Very small splits (`cols=1`) can produce a logically out-of-bounds child rect; the crate's own test comments already acknowledge `rows=0` rects are possible | `nexterm-server/src/window/bsp.rs` | Picked the explicit policy: below the minimum, `split_extent()` hands the whole available extent to the first child and leaves the second at 0 (still exact, never overflowing) rather than an implicit, unstated gap |
| 8 | P1 | M | ✅ Done (P1 PR) | Ratio→cell conversion is independently duplicated in `bsp.rs::compute()` (live tree) and `tiling.rs::compute_pane_sizes()` (snapshot-restore path), with no shared implementation and no test that the two agree | `nexterm-server/src/window/bsp.rs`, `tiling.rs` | Extracted `bsp::split_extent()`, used by both call sites; added a regression test asserting their per-pane outputs agree across several sizes for the same layout |
| 9 | P2 | S | Not started | ADR-0005's rejection of a Zellij-style tiling array cites "resize logic gets complex" — Zellij's actual complexity is specifically N-ary percentage redistribution, a problem Nexterm's strictly-binary tree structurally avoids | `docs/adr/0005-*.md` | One-line addition to the ADR for accuracy; no code change |
| 10 | P2 | S | Not started | The open macOS-only `chrome_advance` `inf`-for-CJK bug matches a known **class** of bug (Ghostty `#8712`, Alacritty `PR #1029`, WezTerm `#614`) rather than being unique to Nexterm, but root cause is still unknown | `nexterm-client-gpu/src/font.rs` | When `chrome_advance` produces a non-finite value, log which face/family cosmic-text actually resolved to (macOS-gated or `NEXTERM_LOG=trace`) — turns "root cause unknown" into a one-shot diagnostic for the next macOS+CJK contributor |
| 11 | N/A | — | ✅ Confirmed non-issue | Terminal-grid width judgement (`unicode_width` static tables) is already fully independent of font shaping, matching WezTerm's/kitty's design — confirmed not to share the macOS `inf` bug's blast radius | `nexterm-client-gpu/src/font.rs` | No action — confirmed correct as-is |
| 12 | N/A | — | ✅ Done (tracked as bug #16) | Glyph-atlas overflow clears `self.cache` but not `self.ligature_cache`, while the atlas cursor resets — found *during* the architecture fact-check | `nexterm-client-gpu/src/glyph_atlas.rs:376` | Already tracked and fixed as bug #16 in `audit-round4-2026h2.md` §1.2 — no duplicate work here, cross-reference only |
| 13 | P1 | S | ✅ Already fixed independently | `measure_char_width`'s '0'-only measurement is unproven-safe, not trap-proof — the project's own design doc confirms there is no finiteness guard, contradicting an earlier claim that the narrow scope was a deliberate CJK-trap dodge | `nexterm-client-gpu/src/font.rs:222` | Discovered during the P1 pass to already be fixed: `resolve_measured_advance()` was extracted with an `is_finite()` guard and a regression test in the same-day medium/low severity round (item #36's fix), before this item was picked up here. No further action |
| 14 | P3 | L | Deferred | Future `nexterm.*` Lua namespace (planned per ADR-0004) risks a second, independently-drifting capability check unless it reuses the existing WASM guard order | `nexterm-plugin` (WASM side), Lua config/status-bar runtime | Blocked until the `nexterm.*` Lua namespace is actually built; when it is, route it through the same `ReadFn`/`ConsentPolicy`/guard-order already defined in ADR-0008 rather than inventing a parallel Lua-specific check |
| 15 | P3 | L | Deferred | The single global `plugin_read: ConsentPolicy` field does not scale past one capability — granting plugin A read access silently grants it to every other loaded plugin too | `nexterm-plugin`, `SecurityConfig` | No second host-function family is shipping yet, so this is speculative; when one does, Zellij's `PermissionCache` (path → `Vec<PermissionType>`, persisted, per-plugin) is documented prior art for where `SecurityConfig` needs to grow |
| 16 | P2 | S | Not started | The tmux+TPM zero-sandbox baseline (unconditional full-account shell execution) is a legitimate comparison quantifying what Nexterm's WASM boundary buys, but currently only exists implicitly across several ADRs | `docs/PRODUCT.md`, `examples/plugins/README.md` | Add one explicit paragraph making this comparison to justify the WASM sandbox's engineering cost — doc-only |
| 17 | P2 | S | Not started | Zellij migrated wasmer → wasmtime → wasmi (2026-03-23); Nexterm is pinned two major versions behind on wasmi (0.38 vs. upstream 2.0.0) | `nexterm-plugin/Cargo.toml` | Add to the existing dependency-review rotation alongside russh/lru (see `dependency-followups` memory) rather than a one-off bump — a larger, WASI-linked consumer stress-testing the same interpreter is a leading indicator for where interpreter bugs surface first |
| 18 | P1 | M | ⚠️ Blocked — design conflict found | `docs/THREAT_MODEL.md` claimed a Windows named-pipe DACL was set; no such code exists — `serve_named_pipe()` only sets `first_pipe_instance(false)` / `reject_remote_clients(true)` (doc-drift already corrected this round; the underlying gap is still open) | `nexterm-server/src/ipc/platform.rs` (Windows) | **Sub-item ① (`first_pipe_instance(true)` on first bind) was investigated and rejected as originally scoped**: `serve_unix`/`serve_named_pipe` deliberately let multiple Nexterm processes for the same user share one pipe name (`first_pipe_instance(false)`, documented in the file's own comment) — the OS cannot distinguish "an attacker pre-created this pipe" from "a second legitimate Nexterm instance already owns it", so forcing `true` on the first bind would make every second-or-later legitimate launch crash at startup instead of only rejecting squatters. Sub-items ② (`SetNamedSecurityInfoW` creator-SID DACL) and ③ (`ImpersonateNamedPipeClient` + `GetTokenInformation` peer check) are unaffected by this and remain valid, but still need a real Windows box to develop and verify iteratively (compile errors can't be round-tripped through CI alone) — re-scope as Windows-only follow-up work, not bundled with a Linux-developed PR |
| 19 | P0 | S | ✅ Done (PR #116) | `verify_peer_uid()`'s reject-on-mismatch branch has no test coverage (confirmed by grep) | `nexterm-server/src/ipc/platform.rs` (Unix) | Added `UnixStream::pair()`-based tests for both the accept path and the reject path (reject-path test gated to platforms where `peer_uid_impl` queries the OS) |
| 20 | P2 | M | Not started | `nexterm-ctl` only prints locale-dependent fixed-width text via `fl!`; no scriptable structured output exists | `nexterm-ctl` | Add a `--json`/NDJSON output mode, giving tmux's "binary main line + scriptable text layer" split without touching the hot IPC path |
| 21 | P2 | S | Not started | Windows Terminal's Monarch/Peasant model is a different product concept (window/command routing, not a PTY-holding daemon) — not a security gap, but the reason Nexterm accepts a different attack surface is undocumented | `docs/THREAT_MODEL.md` | One-line note explaining *why* Nexterm accepts a persistent, detachable-session attack surface that Windows Terminal's model doesn't have — doc-only |
| 22 | P3 | L | New (split from #6) | `handle_resize_split` applies a resize delta blindly even if the client's view of the layout is stale (e.g. a concurrent split/close changed the tree first) — could resize the wrong split after #6's targeting fix, in a narrow race window | `nexterm-server/src/window/mod.rs`, `nexterm-proto/src/message.rs` (`LayoutChanged`, `ResizeSplit`) | Attach a generation counter to `Window`, echo it in `LayoutChanged`, have the client echo it back on `ResizeSplit`, and make the handler a no-op on a stale generation. Needs a wire-protocol bump (postcard positional encoding — see root `CLAUDE.md`) across both client and server; not attempted here since #6's targeting fix already closes the reproducible bug and this is a separate, narrower race |

## Progress

- **P0 (#3, #6, #19) — done, PR #116, merged 2026-09-25.** All three were self-contained
  and landed together with regression tests for each. CI passed on all 3 OSes plus
  ConPTY; `cargo-deny` and `Security audit` failed but were confirmed pre-existing on
  `master` itself (yanked/vulnerable crates in the russh/wasmi dependency tree,
  unrelated to this change) before merging.
- **P1 (#5, #7, #8) — done**, landed as a single PR on top of #116 (same branch family,
  `fix/arch-followups-p1`). Unified the previously-duplicated ratio→cell arithmetic into
  one `bsp::split_extent()` used by both the live-tree and snapshot-restore paths.
- **P1 (#13) — turned out to already be fixed** by an unrelated same-day fix (the
  medium/low severity round's bug #36 fix extracted `resolve_measured_advance()` with
  the exact finiteness guard this item asked for). No duplicate work done.
- **P1 (#18) — blocked, not implemented.** Investigating sub-item ① surfaced a design
  conflict with Nexterm's deliberate multi-instance pipe sharing (see the table row for
  detail); shipping it as originally scoped would have been a regression, not a fix.
  Left as an open item pending a redesign that can tell "attacker squatting" apart from
  "second legitimate instance," and pending real Windows-box access for ②/③ regardless.
- **#6's generation-counter race hardening — split out as new item #22**, P3. The
  reproducible targeting bug itself (what #6 was actually about) is fixed; the race
  hardening is a separate, larger, wire-protocol-touching change and was not bundled in.
- **Remaining:** P2 (#1, #9, #10, #16, #17, #20, #21) not started. P3 (#2, #14, #15, #22)
  deferred by design — revisit only when the blocking feature or need materializes.

## Suggested execution order (remaining work)

1. **P2 doc-only batch** (#9, #16, #21) — trivial, can be done in a single small PR.
2. **P2 remainder** (#1, #10, #17, #20) — independent, pick up opportunistically.
3. **P1/P2, Windows-only, needs real hardware** (#18 ②/③) — schedule once Windows
   dev/CI access is available for iterative compile-error round-tripping; needs a
   redesign of ① first (see table row #18).
4. **P3** (#2, #14, #15, #22) — do not schedule; revisit only when the blocking feature
   (scrollback persistence, the Lua `nexterm.*` namespace, a second plugin capability, or
   a reported stale-resize race) is actually proposed or observed.
5. **N/A** (#4, #11, #12) — no further action; kept in this table only for the "21
   insights fully triaged" record.

## Non-goals for this document

- This document does not re-litigate the fact-check corrections themselves (tmux ACL
  parity, the render-pass count, the Lua `pane_id` visibility, etc.) — those are recorded
  as-is in `audit-round4-2026h2.md` §3 and are treated here as settled inputs.
- Items should graduate out of this table into GitHub issues or their own
  `docs/plans/*.md` file as they are picked up; this table's job is prioritization, not
  long-term tracking.
