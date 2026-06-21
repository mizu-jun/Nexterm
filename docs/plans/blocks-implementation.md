# Command Blocks Implementation Plan (revised)

Status: **Phase 1 in progress** (revised 2026-06-21)
Target release: v2.0.0
Owner: @mizu-jun

## Revision history

- 2026-06-21 v1 — initial plan based on the Warp comparison.
- 2026-06-21 **v2 — corrected after discovering OSC 133 was already implemented in v1.x (Sprint 5-2 / B1)**. The original "Phase 1: VT parser" is dropped; the IPC plumbing in the original "Phase 2" is also dropped. Phases renumbered accordingly.

## Motivation

Warp shipped an OSS Rust terminal in April 2026 whose primary differentiator
versus existing GPU terminals (Alacritty / WezTerm / kitty / Ghostty) is
*block-style UI* — each shell command and its output is treated as a discrete,
addressable unit. The block model enables per-command selection, copy, replay,
collapse, and naming.

Nexterm already matches Warp on GPU rendering, cross-platform support, and
shell access. The OSC 133 parsing layer is also in place. What is missing is
the **block abstraction** on top of the existing semantic marks, the UI to
operate on it, and the persistence of user-assigned names.

AI-agent integration and proprietary-model routing remain out of scope.

## What already exists (do not redo)

| Capability | Location | Notes |
|---|---|---|
| OSC 133 A/B/C/D parsing, exit-code parsing | `nexterm-vt/src/performer.rs:363` | All four kinds plus exit code on `D` |
| `SemanticMark { row, kind, exit_code }` | `nexterm-vt/src/screen.rs:202` | `SemanticMarkKind` enum already public via `lib.rs` |
| `Screen::take_semantic_marks()` drain API | `nexterm-vt/src/screen.rs:1146` | Server pane reader already drains per tick |
| IPC `ServerToClient::SemanticMark { pane_id, row, kind, exit_code }` | `nexterm-proto/src/message.rs:735` | postcard, current PROTOCOL is sufficient |
| Pane reader broadcasts marks to clients | `nexterm-server/src/pane.rs:739` | Already drains and emits per mark |
| Client receives marks, builds `prompt_anchors`, sets exit-code status text | `nexterm-client-gpu/src/state/server_message.rs:151` | Used by Sprint 5-2 / B1 "jump-to-prompt" |
| Unit tests for OSC 133 | `nexterm-vt/src/lib.rs` (`osc_133_*`) | 2 tests, green |

PROTOCOL and SNAPSHOT version bumps are **not required** for this feature.

## Scope (unchanged from v1)

In scope:

1. **Block-style UI** — derive `CommandBlock` records from the existing
   `SemanticMark` stream, render left-border / exit-status overlays, support
   keyboard and mouse selection, copy, replay, and collapse.
2. **Named command history** — let the user name any block, persist names to
   `~/.local/state/nexterm/named_blocks.json` (atomic write, mode 0600), surface
   them in the command palette via an `@name` prefix.

Out of scope:

- AI agent integration, open-model routing.
- Block timing / duration analytics.
- Web terminal block UI (xterm.js renderer is a separate effort).
- Auto-installing shell prompt integration scripts (docs only).
- PROTOCOL / SNAPSHOT bumps (not needed — see "What already exists").

## Architecture

```
existing ServerToClient::SemanticMark stream
         │
         ▼
ClientState.panes[pane_id]
   .scrollback ── existing
   .prompt_anchors ── existing (Sprint 5-2 / B1)
   .marks: Vec<SemanticMark> ── NEW (accumulator)
         │
         ▼  extract_command_blocks(&marks) — pure
   .blocks: Vec<CommandBlock> ── NEW
         │
         ▼
named_blocks.json  ◄────► block_names: HashMap<BlockId, String>
   (atomic, 0600)            ── NEW, palette_history pattern
         │
         ▼
renderer block-overlay pass (4th pass)
   border ✓ badge ✓ selection ✓ collapse chevron
```

Block ID: `BlockId = (pane_id as u64) << 32 | (start_row as u32)`.
`start_row` is the scrollback index of the `A` mark, so the pair is unique
within a session lifetime. (Scrollback rotation can collide IDs after many
millions of rows — acceptable for naming, since the user re-names if needed.)

`CommandBlock` shape:

```rust
pub struct CommandBlock {
    pub id: BlockId,
    pub pane_id: u32,
    pub prompt_row: usize,     // A
    pub command_row: usize,    // B (defaults to prompt_row if missing)
    pub output_row: usize,     // C (defaults to command_row if missing)
    pub end_row: Option<usize>,// D — None while running
    pub exit_code: Option<i32>,
    pub collapsed: bool,
}
```

A "complete" block has all four marks; an incomplete block (still running, or
missing one of B/C) is rendered with a grey badge and is not eligible for
replay.

## Phases

### Phase 1 — CommandBlock abstraction + named-block persistence (≈4 days)

Pure logic + state plumbing. No renderer changes yet. Verifiable entirely by
unit tests.

| Step | File(s) | Output |
|------|---------|--------|
| 1.1 | `nexterm-client-gpu/src/command_blocks.rs` (new) | `CommandBlock`, `BlockId`, `extract_command_blocks(&[SemanticMark]) -> Vec<CommandBlock>` (pure). Unit tests for: full A-B-C-D, only A (running), out-of-order tolerance, multi-block, exit-code propagation |
| 1.2 | `nexterm-client-gpu/src/state.rs` (or `state/*.rs` module split) | `PaneState.marks: Vec<SemanticMark>` accumulator, `PaneState.blocks: Vec<CommandBlock>` derived view, recompute on each new `SemanticMark` IPC |
| 1.3 | `nexterm-client-gpu/src/state/server_message.rs:151` | Extend the existing `ServerToClient::SemanticMark` handler to also append to `marks` and refresh `blocks` (keep the existing `prompt_anchors` and status-bar logic intact) |
| 1.4 | `nexterm-client-gpu/src/named_blocks.rs` (new) | `NamedBlockStore { names: HashMap<BlockId, String>, last_used: HashMap<BlockId, SystemTime> }`. `load()` / `save()` via atomic write + mode 0600, mirroring `palette_history.rs`. Path: `~/.local/state/nexterm/named_blocks.json`. Schema version field for forward-compat |
| 1.5 | `nexterm-client-gpu/tests/named_blocks_test.rs` (new) or `#[cfg(test)] mod tests` | Round-trip save/load, corrupted-file recovery, atomic-write crash safety (tempfile + rename) |

Acceptance:

- `cargo test -p nexterm-client-gpu command_blocks` green
- `cargo test -p nexterm-client-gpu named_blocks` green
- `cargo clippy -- -D warnings` green
- `cargo fmt --check` green
- Manual: send OSC 133 from bash, verify via debug overlay that
  `ClientState.blocks` reflects the expected number of blocks (debug overlay
  will be temporary; full UI lands in Phase 2)

### Phase 2 — Rendering + keyboard / mouse operations (≈1 week)

| Step | File(s) | Output |
|------|---------|--------|
| 2.1 | `nexterm-client-gpu/src/renderer/block_overlay_pass.rs` (new) | 4th render pass. Left border (2 px) coloured by exit code: green `0`, red non-zero, grey unknown. Status badge in the right margin: `✓` / `✗` / `●`. Selection highlight: alpha 0.15 bg over the block's row range. Collapse chevron at `prompt_row` |
| 2.2 | `nexterm-client-gpu/src/renderer.rs` | Hook `block_overlay_pass` after the existing image pass; gate behind `BlocksConfig.enabled` |
| 2.3 | `nexterm-client-gpu/src/key_map.rs` + `nexterm-config` keybindings | `Ctrl+Shift+Up/Down` navigate; `Ctrl+Shift+C` copy block; `Ctrl+Shift+R` replay block (re-send the command line via existing pane-input path); `Ctrl+Shift+L` open name modal; `Ctrl+Shift+X` remove name; `Ctrl+Shift+/` collapse / expand |
| 2.4 | `nexterm-client-gpu/src/state.rs` | `selected_block: Option<BlockId>`; navigation logic; copy assembles `command \n output`; replay extracts the command-line text between `command_row` and `output_row` and writes to the pane stdin |
| 2.5 | `nexterm-client-gpu/src/state.rs` mouse path | Click on left border → select; click on chevron → toggle collapse; right-click → context menu (copy / replay / name / remove) |
| 2.6 | `nexterm-client-gpu/src/state/modals.rs` (or new `block_name_modal.rs`) | Text-input modal for naming (mirrors `PasswordModal`) |
| 2.7 | `nexterm-client-gpu/src/scrollback.rs` | Honour `CommandBlock.collapsed` — collapsed blocks render only the prompt + first output line; the underlying grid is untouched |
| 2.8 | Tests | Pure-function tests for navigation (`next_block`, `prev_block`), copy-range assembly, command extraction. Vi-mode regression test: `Ctrl+Shift+Up/Down` not bound by vi-mode or copy-mode |

Acceptance:

- `cargo test -p nexterm-client-gpu` green
- Manual with bash `PS1`: visible coloured border, navigation works, copy/replay/name work end to end, Vi mode and copy mode unaffected
- No measurable FPS regression on a 200-block scrollback (criterion bench or eyeball)

### Phase 3 — Palette integration + i18n + config polish (≈3 days)

| Step | File(s) | Output |
|------|---------|--------|
| 3.1 | `nexterm-client-gpu/src/palette.rs` | `@<query>` prefix switches the palette into named-block search; fuzzy-match against `NamedBlockStore.names`; selection: focus the block and offer "Replay" / "Copy" |
| 3.2 | `nexterm-i18n/locales/{en,ja,zh-CN,ko,de,fr,es,it}.json` | New keys: `block.copy`, `block.replay`, `block.name`, `block.remove`, `block.collapse`, `block.expand`, `block.exit_code`, `block.success`, `block.failed`, `block.running`, `palette.named_blocks`, `block.name_prompt`, `block.context_menu_title` |
| 3.3 | `nexterm-config/src/lib.rs` + `examples/config.toml` | `BlocksConfig { enabled: bool = true, border_width_px: u32 = 2, show_exit_code_badge: bool = true, collapsed_lines_shown: u32 = 1 }` |
| 3.4 | Settings panel (`settings_panel.rs`) | New "Blocks" category with the four toggles above. `toml_edit` writeback preserves comments |
| 3.5 | `docs/KEYBINDINGS.md`, `docs/CONFIGURATION.md`, `CHANGELOG.md` | Document new bindings, new config keys, new palette prefix |
| 3.6 | `docs/shell-integration.md` (new) | Manual prompt-integration snippets for bash / zsh / fish (we don't auto-install; we document) |

Acceptance:

- `cargo test --workspace` green
- 8 locale JSONs parse and contain the new keys (CI check via `serde_json` round-trip in a unit test)
- Manual: palette `@deploy` finds a named block and replays it
- Settings panel writes `[blocks]` section to `config.toml` without dropping unrelated comments
- `CHANGELOG.md` has an `[Unreleased]` entry citing the three phases

## Risks and mitigations

| Risk | Sev. | Mitigation |
|------|------|------------|
| `BlockId` collision after extreme scrollback rotation | LOW | Use `(pane_id, start_row)` and accept that very long-lived sessions may eventually wrap. The cost is at worst that a name unexpectedly applies to a later block; the user re-names. Document in `shell-integration.md` |
| `Ctrl+Shift+Up/Down` collides with Vi mode (D2) | MED | Vi mode binds `j/k/Ctrl+U/D` without Shift. Add a regression test that asserts neither vi-mode nor copy-mode binds the new chords |
| Replay re-sends control sequences from the captured row range | HIGH | Extract only the *user input* between `command_row` and `output_row`. Strip any embedded OSC / CSI sequences before writing back to the pane. Reject replay if the slice contains anything outside printable + LF (defensive) |
| `named_blocks.json` grows unbounded | LOW | Cap at 10 000 entries; evict oldest by `last_used`. Mirrors `palette_history.json` capacity logic |
| Renderer 4th pass adds GPU cost | LOW | Vertex count is O(blocks visible on screen) ≪ 100. Reuses the existing vertex pipeline; no new shader |
| `BlocksConfig.enabled = false` must fully disable the pass | MED | Early-return in `block_overlay_pass` and skip the per-frame block-derivation work too. Add a test that confirms zero block-related allocations when disabled |
| Settings-panel writeback drops existing comments | MED | Use `toml_edit` exclusively (per project rule). Add a snapshot test of a round-tripped `config.toml` |

## Test plan summary

| Test type | Where | Count target |
|---|---|---|
| Pure unit (extract_command_blocks, navigation, command-extraction sanitiser) | `command_blocks.rs` `state.rs` | ≥ 12 |
| Persistence round-trip (named_blocks.json) | `named_blocks.rs` | ≥ 4 (save/load, corrupted, missing dir, atomic write) |
| Integration (vi-mode keybind non-collision, palette `@` mode) | `nexterm-client-gpu/tests/blocks_integration.rs` (new) | ≥ 3 |
| i18n locale completeness | new test in `nexterm-i18n` | 1 (asserts every new key exists in all 8 locales) |

## References

- FinalTerm OSC 133 spec: <https://iterm2.com/documentation-shell-integration.html>
- Existing patterns to imitate:
  - `nexterm-client-gpu/src/palette.rs` + `~/.local/state/nexterm/palette_history.json` — atomic write + mode 0600
  - `nexterm-client-gpu/src/host_manager.rs` `PasswordModal` — text-input modal pattern
  - `nexterm-client-gpu/src/renderer/background_pass.rs` — pure compute helpers + small render pass
  - `nexterm-client-gpu/src/state/server_message.rs:151` — existing `SemanticMark` handler (extend, do not replace)

## Out-of-band coordination

- No PROTOCOL or SNAPSHOT bump required.
- `docs/KEYBINDINGS.md` and `docs/CONFIGURATION.md` updated in Phase 3.
- `CHANGELOG.md` accumulates entries under `[Unreleased]` per phase.
