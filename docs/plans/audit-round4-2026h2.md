# Audit Round 4 (v1.17.1) — Bug Hunt + Architecture Comparison

- **Date:** 2026-09-11
- **Target:** `master` at v1.17.1
- **Method:** 184 agents across an 11-dimension parallel bug hunt (one dimension per
  crate/subsystem) plus a 5-dimension architecture comparison against tmux, GNU screen,
  Zellij, WezTerm, Alacritty, kitty and Windows Terminal. Every bug candidate was
  adversarially voted on by three independent lenses (correctness / severity / refutation)
  against the live source, requiring 2-of-3 agreement to confirm or refute. Every
  comparison finding was independently fact-checked against primary sources (GitHub API,
  official docs) by an agent with no visibility into the original claim's reasoning.
  Total run: ~94 minutes, 4,488 tool calls, 22.3M subagent tokens.
- **Predecessors:** Round 3 (`docs/plans/audit-round3-2026h2.md`, security / performance /
  correctness at v1.12.0). This round was an independent fresh sweep and was **not**
  explicitly diffed against Round 3's findings — some overlap with already-fixed Round 3
  items is possible and should be checked during triage.
- **Scope note:** This round produced findings only; per the requester's explicit
  instruction, only the documentation-drift corrections below were implemented in this
  same pass. The 46 real bugs and 21 architecture-comparison insights are reported but
  **not yet actioned** — they are a punch list for future rounds/PRs, not a completed
  changeset.

## Implementation status

- **Done (this pass):** three documentation-drift corrections, found as a byproduct of
  grounding the audit prompts in the repo's own docs:
  - Root `CLAUDE.md`: the IPC section referenced a `nexterm-proto/src/codec.rs` that
    does not exist. Corrected to point at the actual locations — `MAX_MSG_LEN` /
    `validate_msg_len` in `nexterm-proto/src/lib.rs`, and the framing I/O itself in
    `nexterm-client-core/src/lib.rs`.
  - `nexterm-server/CLAUDE.md`: claimed "Schema v3 (`SNAPSHOT_VERSION = 3`)"; the crate
    is actually at v5. Corrected to v5 with a short per-version history (v2 `session_title`,
    v3 `workspace_name`, v4 `client_os_windows`, v5 `known_workspaces`).
  - `docs/THREAT_MODEL.md` (Boundary 1, Spoofing row): claimed "Windows: set the
    named-pipe DACL to allow only the creating user" as an *existing* mitigation. No such
    code exists (`serve_named_pipe()` only sets `first_pipe_instance(false)` /
    `reject_remote_clients(true)`; no `windows-sys` security APIs are used). Moved this to
    the Residual risk column as an honest, tracked gap instead of a claimed mitigation.
- **Deferred (this document):** all 46 real bugs (§1), all 21 architecture-comparison
  insights (§3). None have been triaged into tickets yet.
- **No action needed:** 5 findings the audit confirmed are already handled safely (§2),
  and 3 candidate findings the adversarial verification refuted (§4).

---

## 1. Bug hunt: confirmed findings (46 real bugs + 5 confirmed-safe = 51 total)

Methodology: 11 independent finder agents (one per risk dimension below) proposed 55 raw
candidates over the whole workspace; a dedup pass merged near-duplicates to 54 distinct
candidates; each was then voted on by three adversarial lenses reading the live source.
0 candidates landed in "no majority" (uncertain); 51 reached a 2-of-3 (or 3-of-3) verdict
of *confirmed* — 46 of those are real defects, the other 5 are "confirmed safe" results
(the auditors went looking for a bug and instead verified the code already handles the
case correctly — kept here for coverage transparency, see §2). Severities below are the
**post-verification** severity: three items were corrected down from the finder's
original claim (High → Medium) and two were corrected up (Low → Medium), based on the
adversarial panel's reasoning, not the original claim. The 11 finder dimensions were:
CJK/wide-glyph measurement consistency, floating-point inf/NaN propagation, server
concurrency & shared-state races, BSP pane lifecycle invariants, the russh 0.62.7
migration, IPC protocol/codec correctness, the WASM plugin sandbox, config hot-reload
races, VT-parser input safety (untrusted PTY bytes), i18n coverage, and GPU
resource/texture lifecycle.

### 1.1 Critical (5)

| # | Finding | Location | Votes |
|---|---|---|---|
| 1 | Copy-mode `search_forward` conflates a grid **column** index with a **byte** index into the row string. `col_start` (a column count) is used directly as `&row_str[col_start..]`, and `str::find`'s byte offset is added straight onto it. Any row with a multi-byte UTF-8 character (CJK output, a Powerline/Nerd Font prompt glyph, an emoji, even an accented Latin letter) before the search cursor panics with "byte index is not a char boundary" — or, past the panic boundary, silently returns a wrong column. No `catch_unwind` exists anywhere in the crate, so in the default single-binary build the panic kills the embedded server task too, dropping every pane's PTY session in that window. Reachable via the ordinary Copy Mode `/` search. | `nexterm-client-gpu/src/renderer/input_handler/copy_mode.rs:372` | 2/3 confirmed |
| 2 | The same column/byte conflation exists independently in `search_prev`. | `nexterm-client-gpu/src/renderer/input_handler/copy_mode.rs:402` | 2/3 confirmed |
| 3 | The SFTP client never sends the `subsystem sftp` request after opening its channel, so a real SSH server never enters SFTP mode — every SFTP transfer fails. Direct consequence of the still-unclosed gap that real SSH connections (password/pubkey/SFTP) have never been exercised end-to-end, before or after the russh 0.62.7 upgrade. | `nexterm-ssh/src/lib.rs:566` | 3/3 confirmed |
| 4 | The Sixel decoder allocates a pixel buffer sized from the sequence's declared width×height **before** validating that size against any sane maximum — a malformed or malicious Sixel sequence (arriving from any remote SSH host or untrusted program) can trigger an unbounded allocation. | `nexterm-vt/src/image.rs:178` | 3/3 confirmed |
| 5 | Decoded Sixel/Kitty image dimensions are never checked against the GPU's actual `max_texture_dimension_2d` before upload. Because rendering runs on one shared event loop across all panes, one oversized image can crash the whole client window, not just its own pane — a strictly wider blast radius than #4. | `nexterm-client-gpu/src/renderer/image.rs:70` | 3/3 confirmed |

### 1.2 High (8)

| # | Finding | Location | Votes |
|---|---|---|---|
| 6 | URL-detection hit-testing has the same column/byte conflation as #1/#2, independently, in the selection code. | `nexterm-client-gpu/src/state/selection.rs:43` | 3/3 |
| 7 | IME preedit (composition) text is positioned by character count, not measured width — misaligned during Japanese/CJK input composition. | `nexterm-client-gpu/src/renderer/render_frame.rs:1337` | 3/3 |
| 8 | A failed `insert_after` into the BSP tree has its error silently discarded by a caller; the pane is created but never placed in the tree, so it exists but is never drawn — a permanently invisible "ghost pane". | `nexterm-server/src/window/mod.rs:237` | 3/3 |
| 9 | `add_serial_pane` spawns the PTY at the wrong size and never issues the follow-up resize the documented "reserve → insert → recompute → spawn → resize" pane-creation sequence calls for. | `nexterm-server/src/window/mod.rs:955` | 3/3 |
| 10 | `new_with_pane` (used when a tab is torn out into a new OS window) never resizes the pane's PTY to match the new window, leaving it stuck at the old size. | `nexterm-server/src/window/mod.rs:181` | 3/3 |
| 12 | The plugin WASM memory-page cap is checked only once at load time, not on every subsequent `memory.grow`, so a plugin can grow past the intended limit after loading. | `nexterm-plugin/src/lib.rs:512` | 3/3 |
| 14 | `SettingsPanel` is never resynced after a config hot-reload; if the panel is open when an external edit reloads `config.toml`, saving from the (now-stale) panel silently clobbers the external change. | `nexterm-client-gpu/src/settings/save.rs:38` | 3/3 |
| 16 | On glyph-atlas overflow, the plain-glyph path (`get_or_insert`) clears `self.cache` but **not** `self.ligature_cache`, while the atlas cursor still resets — a ligature glyph cached earlier keeps handing out a UV rect into what will become overwritten texture data. (Independently rediscovered during the architecture-comparison fact-check, see §3.3.) | `nexterm-client-gpu/src/glyph_atlas.rs:376` | 3/3 |

### 1.3 Medium (22)

Three items here (#11, #13, #15) were downgraded from the finder's original High claim
by unanimous or majority vote; two (#41, #47) were upgraded from Low, in #47's case after
one lens ran an empirical benchmark.

| # | Finding | Location | Votes |
|---|---|---|---|
| 11 | postcard's enum wire tag is implicit declaration order (not self-describing). Inserting a new variant anywhere but the end is a silent wire-format break between differently-versioned client/server builds — currently avoided only by convention (append-only + version-bump comments), not enforced. *(High → Medium)* | `nexterm-proto/src/message.rs:101` | 3/3 |
| 13 | The settings panel's `toml_edit` write-back to `config.toml` is not atomic and can race the config crate's `notify`-based hot-reload watcher. *(High → Medium)* | `nexterm-client-gpu/src/settings/save.rs:29` | 3/3 |
| 15 | Resizing while an alternate screen (vim/less) is active can desync the primary/alt grid dimension bookkeeping. *(High → Medium: usually self-heals on returning to the primary screen)* | `nexterm-vt/src/screen.rs:507` | 3/3 |
| 17 | `window.background_opacity` has no NaN/inf validation in the config schema. | `nexterm-config/src/schema/window.rs:273` | 3/3 |
| 18 | `clamped_opacity()` does not actually remove NaN. | `nexterm-config/src/schema/window.rs:167` | 3/3 |
| 19 | BSP split-ratio clamping arithmetic can overflow for very small parent rects. | `nexterm-server/src/window/bsp.rs:100` | 3/3 |
| 20 | Floating-pane offset/size clamping is inconsistent between the compute path and the draw path. | `nexterm-server/src/window/mod.rs:328` | 2/3 |
| 21 | A BSP tree restored from a snapshot can end up with a duplicated `pane_id`. | `nexterm-server/src/window/mod.rs:1044` | 3/3 |
| 22 | `focused_window_id` can be left dangling after a snapshot restore. | `nexterm-server/src/session.rs:570` | 3/3 |
| 23 | The X11-forwarding channel is opened but never bridged to anything — the feature silently does nothing. | `nexterm-ssh/src/lib.rs:386` | 3/3 |
| 24 | IPC has no per-connection timeout and no cap on concurrent connections. | `nexterm-server/src/ipc/platform.rs:37` | 3/3 |
| 25 | `#[serde(default)]` does not provide forward-compatibility over postcard the way the codebase assumes elsewhere (postcard is positional, not self-describing). | `nexterm-proto/src/message.rs:110` | 3/3 |
| 26 | A WASM module's `start` section runs before the plugin API's own permission gates are wired up, so a plugin can call `write_pane` from its start section bypassing the intended consent gate. | `nexterm-plugin/src/lib.rs:434` | 3/3 |
| 27 | The plugin `on_output` / `on_command` hooks are fully implemented but never wired into any live IPC dispatch path. | `nexterm-server/src/ipc/dispatch.rs:249` | 3/3 |
| 28 | The config file watcher has no debounce/coalescing; a single external save that emits multiple filesystem events (temp file + rename) can trigger multiple reloads, one of them against a transiently incomplete file. | `nexterm-config/src/watcher.rs:50` | 3/3 |
| 29 | Lua hooks/config are not re-evaluated on hot-reload; changes require a full process restart. | `nexterm-config/src/lua_worker.rs:52` | 3/3 |
| 30 | The SSH delete-confirmation dialog's label is off-center due to padding baked into the translated string. | `nexterm-i18n/locales/en.json:177` | 3/3 |
| 31 | The keybindings delete-confirmation dialog is double-decorated (locale-string decoration plus the drawn frame). | `nexterm-i18n/locales/en.json:160` | 3/3 |
| 32 | GPU-init / window-create failure i18n keys exist but are unused; a hardcoded English string is shown instead. | `nexterm-client-gpu/src/renderer/event_handler/lifecycle.rs:78` | 3/3 |
| 33 | The glyph atlas's LRU capacity accounting does not account for the actual footprint of wide (double-cell) glyphs. | `nexterm-client-gpu/src/glyph_atlas.rs:320` | 3/3 |
| 41 | SSH host-key verification fails **open** on an I/O error, which contradicts the project's own documented "verification is mandatory" stance. *(Low → Medium)* | `nexterm-ssh/src/lib.rs:98` | 2/3 |
| 47 | wasmi 0.38 has no epoch-interruption mechanism at all (that's a wasmtime API — the original Low rating assumed it existed). Fuel bounds instruction count, not wall-clock time; a benchmark during verification measured a single plugin-load call at 536 ms vs. 49 ns for a no-op, reachable today via the already-wired `LoadPlugin` IPC handler. *(Low → Medium, unanimous, with empirical measurement)* | `nexterm-plugin/src/lib.rs:354` | 3/3 |

### 1.4 Low (11)

| # | Finding | Location | Votes |
|---|---|---|---|
| 34 | Settings-panel button width is decoupled from its label text. | `nexterm-client-gpu/src/renderer/ui_verts.rs:263` | 3/3 |
| 35 | Search-bar hint-text alignment still depends on character count. | `nexterm-client-gpu/src/renderer/ui_verts.rs:1344` | 3/3 |
| 36 | `measure_char_width`'s fallback check (`advance > 1.0`) does not exclude `+inf` (`inf > 1.0` is `true` in IEEE 754) — the same class of bug as the open macOS/CJK `chrome_advance` issue, just unguarded in the sibling function. | `nexterm-client-gpu/src/font.rs:263` | 3/3 |
| 37 | `compute_background_quad`'s NaN guard has an uncovered branch. | `nexterm-client-gpu/src/renderer/background_pass.rs:74` | 3/3 |
| 39 | A test's env-var save/restore is not panic-safe (won't restore the original value if the test body panics first). | `nexterm-server/tests/snapshot_roundtrip.rs:406` | 3/3 |
| 40 | PTY/shell-open SSH requests use `want_reply=false` (plausibly intentional per the SSH spec; flagged for confirmation rather than as a confirmed defect). | `nexterm-ssh/src/lib.rs:372` | 3/3 |
| 42 | Documentation drift: root `CLAUDE.md` referenced a `nexterm-proto/src/codec.rs` that does not exist. **Fixed in this round** — see Implementation status above. | `nexterm-proto/src/lib.rs:24` | 3/3 |
| 46 | A coarse plugin-wide mutex serializes all plugin operations across the whole server for the duration of each WASM call — an availability tradeoff, not a safety bug (it's what makes #12's memory-cap race and any use-after-free structurally impossible). | `nexterm-plugin/src/lib.rs:332` | 3/3 |
| 48 | No regression test exists for either the plugin API-version-rejection path or the memory-cap bypass (#12). | `nexterm-plugin/tests/plugin_host.rs:338` | 3/3 |
| 50 | The Kitty graphics protocol's final (non-chunked) transfer chunk is appended without re-checking `MAX_KITTY_CHUNK_LEN`, letting a single transfer overshoot the 64 MiB cap by up to ~4 MiB (bounded, non-cumulative). | `nexterm-vt/src/screen.rs:928` | 3/3 |
| 51 | 6 i18n locale keys are fully translated into all 8 languages but referenced by no code anywhere in the workspace. | `nexterm-i18n/locales/en.json:106` | 3/3 |

---

## 2. Confirmed safe (5)

These were investigated as candidate bugs and found, on inspection, to already be
handled correctly. Kept for audit-coverage transparency.

- **Float math outside the known `chrome_advance` bug is NaN/inf-safe**: spring physics,
  easing curves, and glyph-atlas sizing math were all checked independently of the known
  macOS/CJK issue and found safe. `nexterm-client-gpu/src/animations/mod.rs:93`
- **Oversized IPC length prefixes are rejected before allocation.** `nexterm-proto/src/lib.rs:94`
- **A corrupted/malformed IPC payload decodes to a clean `Err` and is logged, not a panic** —
  traced through postcard's own internals to confirm. `nexterm-server/src/ipc/handler.rs:69`
- **Protocol-version mismatch at Hello produces a clean, explicit disconnect**, not silent
  byte misinterpretation. `nexterm-server/src/ipc/handler.rs:85`
- **Plugins have zero filesystem/network host imports** — exactly 6 host functions are
  linked (`log`, `write_pane`, `read_pane`, `read_grid`, `read_scrollback`,
  `api_version`), confirmed by a workspace-wide grep for WASI-style imports (0 hits).
  `nexterm-plugin/src/lib.rs:395`

## 3. Architecture comparison (5 dimensions, 21 findings)

Each dimension below was independently fact-checked by a separate agent against primary
sources (GitHub API for issue/PR/commit content, official docs) with no visibility into
the original analysis's reasoning. **4 of the 5 dimensions came back with at least one
factual correction needed** — the summaries below already incorporate those corrections;
where a claim was wrong, that is called out explicitly rather than silently smoothed
over.

### 3.1 Session persistence & daemon model — fact-check: verified (4 minor wording notes)

Compared against tmux/GNU screen (always-separate client/daemon processes) and Zellij
(native ~1s KDL session serialization, opt-in scrollback).

- Nexterm's default single-binary build runs the "server" as a thread inside the same
  OS process as the GUI (a dedicated Tokio runtime since v1.7.7), not a separate process.
  Even `CloseAction::Detach` calls `signal_server_shutdown()` — the code's own comment
  admits this is "effectively a kill". **Suggestion:** make this explicit in the
  close-confirmation UI, and consider a single-instance guard in `serve_unix()` (probe
  the existing socket before `remove_file()`+`bind()`) to make detach/reattach a
  first-class, race-free feature rather than an accident of thread lifetime.
  *(nexterm-client-gpu/src/renderer/event_handler/window.rs, nexterm-client-gpu/src/main.rs, nexterm-server/src/ipc/platform.rs)*
- Nexterm's JSON snapshot persistence (schema v1–v5, auto-migrated, 30s interval) is a
  real advantage over tmux/screen's plugin-dependent persistence, but PTY output and
  scrollback are explicitly out of scope today ("future work" per the crate's own doc
  comment). **Suggestion:** if scrollback persistence is ever added, mirror Zellij's
  opt-in, capped `scrollback_lines_to_serialize` design rather than defaulting to
  unbounded capture. *(nexterm-server/src/snapshot.rs)*
- Write path (`write_atomic_secure`: temp file → `sync_all` → rename, mode 0600) is
  solid; **read path performs no owner/permission `stat()` at all**, unlike the write
  path's discipline — an asymmetry. **Suggestion:** add an OpenSSH-style
  "permissions are too open" check before `load_snapshot()` reads the file, plus a size
  cap, both falling into the existing fail-closed path. *(nexterm-server/src/persist.rs)*
- *(Doc-drift note, fixed in this round):* `nexterm-server/CLAUDE.md` said Schema v3;
  actual `SNAPSHOT_VERSION` is 5.

### 3.2 Pane layout algorithm — fact-check: **not verified**, 2 factual errors found

Compared against sway (a 2019 floor→round fix, and a 2025 "too many containers" crash
fix), i3 (`i3#5447`, an unresolved interactive-resize crash), tmux (absolute-integer
`layout_cell`, no ratios), and Zellij (KDL `Constraint::Fixed`/`Percent`).

> **Correction 1:** the original finding claimed `adjust_ratio_for` resizes "the split
> closest to the focused pane." This is false — reproduced independently, it always
> resizes the **outermost (root) split** regardless of nesting depth, which is itself an
> always-reproducible UI bug (dragging an inner divider in a 3+ pane layout moves the
> outer split instead), independent of any multi-client race.
> **Correction 2:** the claim that "`nexterm-ctl`, the standalone server, and the TUI
> client all directly execute the same `compute()`" is false — only the server process
> executes it; `nexterm-ctl` and the TUI client don't even depend on the
> `nexterm-server` crate (`SplitNode` is `pub(super)`-scoped to it). The weaker claim
> ("compute always runs through the one server-side path regardless of which client
> triggered it") is correct.

- `bsp.rs::compute()`'s `(cols as f32 * ratio) as u16` truncates rather than rounds
  (sway fixed the identical bug in 2019); the left/top child always loses the rounding
  error. **Suggestion:** switch to `.round()` in both `bsp.rs::compute()` and
  `tiling.rs::compute_pane_sizes()`; add a regression test pinning `cols=81, ratio=0.5 → 41`
  (today's truncating code already passes the existing sum-only test).
- With the corrected understanding of `adjust_ratio_for` above: attach a generation
  counter to `Window`, echo it in the layout snapshot the client already receives, and
  make `handle_resize_split` a no-op on a stale generation instead of applying blindly.
- Very small splits (`cols=1`) can produce a logically out-of-bounds child rect; the
  crate's own test comments already acknowledge `rows=0` rects are possible. Nexterm has
  no equivalent of tmux's outright refusal or sway's 2025 clamp-and-disable fix — pick
  one explicitly rather than leaving the gap implicit.
- The ratio→cell conversion is independently duplicated in `bsp.rs::compute()` (live
  tree) and `tiling.rs::compute_pane_sizes()` (snapshot-restore path) with no shared
  implementation and no test that the two agree. **Suggestion:** extract a single
  `split_extent()` function both call, plus a proptest asserting parity.
- ADR-0005's rejection of a Zellij-style tiling array cites "resize logic gets complex" —
  Zellij's actual complexity is specifically N-ary percentage redistribution, a problem
  Nexterm's strictly-binary tree structurally avoids. Worth a one-line addition to the ADR.

### 3.3 GPU rendering pipeline & font metrics — fact-check: **not verified**, 1 factual error + 3 inaccuracies found

Compared against Ghostty (`#8712`), Alacritty (`PR #1029`, `#4038`), WezTerm (`#614`,
`#3625`), and kitty.

> **Correction:** the claim that Nexterm's text pipeline "runs in the same render pass as
> the image pass" is false. The actual frame records at least 6 separately-labeled wgpu
> render passes in sequence on one command encoder (clear → background-image →
> optional acrylic-capture → main [bg+text] → one image pass per visible image → one
> text-size pass per OSC 66 entry), all targeting the same swapchain view with
> `LoadOp::Load` — visually composited, not merged. Also: "the glyph atlas cosmic-text
> supplies" is backwards — `glyph_atlas.rs` has zero dependency on cosmic-text; the atlas
> is entirely Nexterm's own construction, and cosmic-text only rasterizes individual
> glyph bitmaps that get packed into it.

- The open, unexplained macOS-only bug where `chrome_advance` (font.rs) returns `inf`
  for CJK characters appears to be a known **class** of bug, not unique to Nexterm:
  Ghostty `#8712` (root-caused to fallback logic estimating rather than measuring the
  actual fallback face), Alacritty `PR #1029` (moved off "max advance in the font" to a
  reference-glyph measurement for the same reason), and WezTerm `#614` (CJK glyph
  splitting, suspected float precision) all hit variations of the same macOS+CJK-fallback
  combination. **Suggestion:** when `chrome_advance` produces a non-finite value, log
  which face/family cosmic-text actually resolved to (macOS-gated or `NEXTERM_LOG=trace`)
  — turns "root cause unknown" into a one-shot diagnostic the next macOS+CJK contributor
  can capture immediately.
- Terminal-grid width judgement (`unicode_width` static tables) is already fully
  independent of font shaping, matching WezTerm's and kitty's design — the macOS `inf`
  bug cannot reach grid rendering, only the chrome/UI layer.
- **New bug found during fact-checking** (folded into §1.2 #16): glyph-atlas overflow
  clears `self.cache` but not `self.ligature_cache` in the plain-glyph path, while the
  atlas cursor still resets to origin.
- The claim that `measure_char_width`'s '0'-only measurement was a *deliberate* dodge of
  the CJK trap contradicts the project's own design doc, which states plainly there is no
  finiteness guard and the narrow scope was intentional discipline, not a designed
  mitigation — the function is unproven-safe, not trap-proof.

### 3.4 Plugin/extension architecture — fact-check: **not verified**, 1 factual error found (central claim)

Compared against WezTerm (single shared Lua/mlua runtime for both config and plugins,
no sandbox), Zellij (17-member `PermissionType` enum, WASI-based, per-plugin persisted
grants), and tmux+TPM (unsandboxed shell scripts as the zero-sandbox baseline).

> **Correction:** the claim that "Lua code is never given a pane_id to misuse" is false.
> `LuaHookRunner::call_macro` explicitly calls the Lua macro function with `pane_id` as
> its second argument (`function(session, pane_id)`, per the module's own doc comment) —
> Lua code does see it. The narrower, actually-true claim: Lua's return value is a plain
> string with no pane selector, so even though Lua can see which pane it was invoked for,
> it cannot redirect the eventual `write_input()` to a *different* pane (the host resolves
> and reuses its own `pane_id` for both the call and the write).

- Nexterm runs two structurally unrelated sandboxes (wasmi for plugins, a separately
  restricted mlua for Lua config/status-bar) versus WezTerm's one shared, unsandboxed
  runtime. ADR-0004 documents this as a deliberate hybrid choice. **Suggestion:** when
  ADR-0004's planned `nexterm.*` Lua namespace is built, route it through the *same*
  `ReadFn`/`ConsentPolicy`/guard-order ADR-0008 already defined and audited for WASM,
  rather than a second Lua-specific capability check that can drift independently.
- Nexterm's plugin ABI links exactly 6 host functions with no file/network/process
  imports at all — there is structurally no verb to grant, versus Zellij's 17-permission,
  per-plugin-persisted model. This is safer against an unknown `.wasm` but puts a hard
  ceiling on any future "let a plugin run one command" feature. **Suggestion:** the
  current single global `plugin_read: ConsentPolicy` field does not scale past one
  capability — granting plugin A read access silently grants it to every other loaded
  plugin too. Zellij's `PermissionCache` (path → `Vec<PermissionType>`, persisted) is
  proven prior art for where `SecurityConfig` needs to grow before a second host-function
  family ships.
- The tmux+TPM zero-sandbox baseline is not a like-for-like comparison, but it is a
  legitimate one: it quantifies what "prefix+I" costs (unconditional full-account shell
  execution) and is the reason Nexterm's WASM boundary is worth its engineering cost.
  This specific, stronger comparison currently only exists implicitly across several
  ADRs — worth a one-paragraph, explicit callout in `docs/PRODUCT.md` and
  `examples/plugins/README.md`.
- Zellij migrated wasmer → wasmtime → wasmi (`PR #4449`, 2026-03-23); the task's original
  premise that "Zellij is the wasmtime one" is ~18 months stale. Both projects now share
  the wasmi interpreter family, but Nexterm is pinned two major versions behind
  (0.38 vs. upstream 2.0.0) — worth adding to the existing dependency-review rotation
  alongside russh/lru, since a larger, WASI-linked consumer stress-testing the same
  interpreter is a leading indicator for where interpreter bugs surface first.

### 3.5 IPC transport & security model — fact-check: **not verified**, 1 factual error found that reverses a stated conclusion

Compared against Windows named-pipe security guidance, tmux (`server-acl.c`), Zellij,
and Windows Terminal's Monarch/Peasant model.

> **Correction (significant):** the original finding claimed tmux calls `getpeereid()`
> but "only stores the result, never uses it for verification," concluding "Nexterm's
> Unix-side design is the most robust of the three." **This is false.** tmux has a real
> ACL mechanism (`server-acl.c`): `server_accept()` calls `server_acl_join()` immediately
> after `accept()`, checks the peer's UID/GID, and disconnects with "access not allowed"
> on any mismatch — functionally equivalent to (and, with its UID/GID allow/deny +
> read-only settings, more flexible than) Nexterm's own `verify_peer_uid()`. Zellij is
> confirmed to have no such check (socket-path permissions only). The corrected
> conclusion: **tmux and Nexterm are peers on this point; Zellij is the outlier**, not
> "Nexterm is uniquely best."

- **This round also independently confirmed the pre-existing doc/implementation gap
  fixed above**: `docs/THREAT_MODEL.md` claimed a Windows named-pipe DACL was set; no
  such code exists. `serve_named_pipe()` sets only `first_pipe_instance(false)` /
  `reject_remote_clients(true)`. **Suggestion (not yet implemented):** ①
  `first_pipe_instance(true)` on the first bind to fail fast on pipe squatting; ②
  `SetNamedSecurityInfoW` with a creator-SID-only DACL; ③
  `ImpersonateNamedPipeClient` + `GetTokenInformation` to check the connecting SID,
  mirroring Unix's `verify_peer_uid`.
- `verify_peer_uid()`'s reject-on-mismatch branch has no test coverage today (confirmed
  by grep). **Suggestion:** a `UnixStream::pair()`-based unit test passing a deliberately
  wrong UID would cover this without needing root or a second real user — small,
  in-scope, high-value.
- The claim that postcard "can carry raw PTY byte streams" is not accurate — the only
  `Vec<u8>` raw-byte payload in the whole protocol is `ImagePlaced.rgba`; PTY output is
  always sent as structured `GridDiff`/`FullRefresh` messages. **Suggestion (unaffected
  by the correction):** add a `--json`/NDJSON output mode to `nexterm-ctl` (which today
  prints locale-dependent fixed-width text via `fl!`) to get tmux's
  "binary main line + scriptable text layer" split without touching the hot IPC path.
- Windows Terminal's Monarch/Peasant model is a different product concept (window/command
  routing, not a PTY-holding daemon) — not a security gap, but worth a one-line note in
  `THREAT_MODEL.md` explaining *why* Nexterm accepts the attack surface Windows Terminal
  doesn't (a persistent, detachable session is the point).

---

## 4. Refuted candidates (3)

Adversarial verification actively disproved these; kept for transparency.

- **"Background-image downscale can truncate a dimension to 0, crashing wgpu"** —
  `nexterm-client-gpu/src/renderer/background_pass.rs:348`. False: `DynamicImage::resize()`
  (not `resize_exact()`) clamps each output dimension to a floor of 1 internally, so the
  image degrades to a 1×1 pixel — a cosmetic defect, not a crash.
- **"`measure_char_width` is clean/safe because it never receives non-ASCII input"** —
  `nexterm-client-gpu/src/font.rs:222`. The narrow claim is technically true, but 2 of 3
  votes refuted the "clean" framing because it ignores an adjacent, unguarded `+inf`
  propagation risk in the same function (see §1.4 #36) — this is the same underlying gap
  as #36, just approached from the "is it safe" side rather than the "here's the gap" side.
- **"All 8 i18n locale files have identical key sets and every call site resolves"** —
  `nexterm-i18n/locales/*.json`. All three votes agree no real bug exists, but 2 of 3
  labeled this "refuted" rather than "confirmed" on the grounds that a verified non-issue
  should be refuted-as-a-concern rather than confirmed-as-a-finding; one vote also found
  an undercounted call site (`dialog.rs:416`) that, once included, still turned up nothing.

---

## Appendix: source data

Full per-agent reasoning (all three adversarial votes per bug, full fact-check notes per
comparison dimension) is preserved in the session's workflow journal
(`journal.jsonl`, run id `wf_5e18ef0f-6bc`) and is not reproduced here for length. An
interactive HTML version of this report with the same findings was also published as a
Claude Artifact during the same session.
