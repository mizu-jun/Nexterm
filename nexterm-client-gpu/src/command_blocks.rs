//! Command-block abstraction layer.
//!
//! Folds the stream of OSC 133 `SemanticMark`s emitted by `nexterm-vt` into
//! `CommandBlock` records keyed by the prompt row. Each block bundles the four
//! sequence positions (`A` prompt-start, `B` command-start, `C` output-start,
//! `D` command-end) plus the optional exit code reported on `D`.
//!
//! The extraction function is pure so that the renderer and the named-block
//! palette can call it without touching `ClientState`. The marker stream is
//! tolerant of malformed input: orphan `B`/`C`/`D` marks before the first `A`
//! are dropped, and a second `A` before `D` flushes the in-progress block as
//! "still running".
//!
//! NOTE: navigation, look-up, and sanitiser helpers below are unit-tested but
//! only consumed by the renderer / palette work in Phase 2b. `dead_code` is
//! silenced module-wide for that reason and the attribute should be removed
//! once Phase 2b wires the helpers up.

#![allow(dead_code)]

/// OSC 133 mark kind, mirroring `nexterm_vt::SemanticMarkKind`.
///
/// Kept as a client-local copy so that this module does not pull `nexterm-vt`
/// in directly — the IPC carries the kind as a single-character string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticMarkKind {
    /// `A` — prompt start.
    PromptStart,
    /// `B` — command (user-input) start.
    CommandStart,
    /// `C` — command output start.
    OutputStart,
    /// `D` — command end (carries the exit code).
    CommandEnd,
}

impl SemanticMarkKind {
    /// Decode the single-character kind string received over IPC.
    pub fn from_ipc(kind: &str) -> Option<Self> {
        match kind {
            "A" => Some(Self::PromptStart),
            "B" => Some(Self::CommandStart),
            "C" => Some(Self::OutputStart),
            "D" => Some(Self::CommandEnd),
            _ => None,
        }
    }
}

/// A single OSC 133 mark accumulated on a pane.
///
/// `row` is a **scrollback-absolute index** (i.e. `pane.scrollback.len()` at
/// the moment the mark was received), not the IPC-level grid row. Using the
/// absolute index keeps block IDs stable as the grid scrolls and avoids the
/// row-reuse problem that a `u16` grid index would have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticMark {
    pub row: usize,
    pub kind: SemanticMarkKind,
    pub exit_code: Option<i32>,
}

/// Stable identifier for a block within a session.
///
/// Layout: `(pane_id as u64) << 32 | (prompt_row as u32 as u64)`. `prompt_row`
/// is the scrollback-absolute index of the `A` mark. The pair is unique within
/// a session until the absolute index wraps past `u32::MAX` (≈4 billion rows),
/// which we treat as acceptable for the naming use case.
pub type BlockId = u64;

/// Construct a block ID from its parts.
pub fn make_block_id(pane_id: u32, prompt_row: usize) -> BlockId {
    ((pane_id as u64) << 32) | u64::from(prompt_row as u32)
}

/// A logical command block derived from the OSC 133 mark stream.
///
/// `command_row` / `output_row` default to `prompt_row` when the corresponding
/// `B` / `C` mark is missing, so callers can treat the rows as a valid range
/// even on shells that emit only `A` and `D`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandBlock {
    pub id: BlockId,
    pub pane_id: u32,
    pub prompt_row: usize,
    pub command_row: usize,
    pub output_row: usize,
    /// `None` while the command is still running or the shell omitted `D`.
    pub end_row: Option<usize>,
    /// `None` until `D` arrives; on `D` reflects the shell-reported exit code.
    pub exit_code: Option<i32>,
    pub collapsed: bool,
}

impl CommandBlock {
    /// True once the closing `D` mark has been observed.
    #[allow(dead_code)] // wired up in Phase 2 (renderer/state UI)
    pub fn is_complete(&self) -> bool {
        self.end_row.is_some()
    }

    /// True when the block ended with a non-zero exit code.
    #[allow(dead_code)] // wired up in Phase 2 (renderer/state UI)
    pub fn is_failure(&self) -> bool {
        matches!(self.exit_code, Some(code) if code != 0)
    }
}

/// Fold a marker stream into command blocks.
///
/// The input is consumed in order. Behaviour:
/// - `A` opens a new block; if a previous block was still open it is flushed
///   first (left with `end_row = None`).
/// - `B` and `C` update the current block's `command_row` / `output_row`.
/// - `D` closes the current block, copying its exit code in.
/// - Marks that arrive before the first `A` are ignored.
pub fn extract_command_blocks(pane_id: u32, marks: &[SemanticMark]) -> Vec<CommandBlock> {
    let mut blocks = Vec::with_capacity(marks.len() / 4);
    let mut current: Option<CommandBlock> = None;

    for m in marks {
        match m.kind {
            SemanticMarkKind::PromptStart => {
                if let Some(prev) = current.take() {
                    blocks.push(prev);
                }
                current = Some(CommandBlock {
                    id: make_block_id(pane_id, m.row),
                    pane_id,
                    prompt_row: m.row,
                    command_row: m.row,
                    output_row: m.row,
                    end_row: None,
                    exit_code: None,
                    collapsed: false,
                });
            }
            SemanticMarkKind::CommandStart => {
                if let Some(b) = current.as_mut() {
                    b.command_row = m.row;
                    // `B` arriving without a later `C` should still leave the
                    // output range starting at `B`.
                    b.output_row = m.row;
                }
            }
            SemanticMarkKind::OutputStart => {
                if let Some(b) = current.as_mut() {
                    b.output_row = m.row;
                }
            }
            SemanticMarkKind::CommandEnd => {
                if let Some(mut b) = current.take() {
                    b.end_row = Some(m.row);
                    b.exit_code = m.exit_code;
                    blocks.push(b);
                }
            }
        }
    }

    if let Some(b) = current.take() {
        blocks.push(b);
    }

    blocks
}

/// Find a block by ID. Pure helper used by navigation, copy, and replay.
pub fn find_block_by_id(blocks: &[CommandBlock], id: BlockId) -> Option<&CommandBlock> {
    blocks.iter().find(|b| b.id == id)
}

/// Pure predicate: does `abs_row` sit inside a collapsed block, and is it a
/// row that should be elided from the visible scrollback view?
///
/// A collapsed block is rendered as "prompt row + first output row only". So a
/// row is elided iff:
/// - it belongs to a block whose `collapsed == true`,
/// - the block has a known `end_row` (running blocks are not eligible — their
///   tail rows are still being written),
/// - the row is strictly between the prompt row + first-output row and the end
///   row. The prompt row and the output row themselves remain visible.
///
/// Pure — no allocations, no `ClientState` access.
pub fn is_row_collapsed(blocks: &[CommandBlock], abs_row: usize) -> bool {
    for b in blocks {
        if !b.collapsed {
            continue;
        }
        let Some(end) = b.end_row else {
            continue;
        };
        if abs_row < b.prompt_row || abs_row > end {
            continue;
        }
        if abs_row == b.prompt_row || abs_row == b.output_row {
            return false;
        }
        return true;
    }
    false
}

/// Return the block ID immediately after `current` (`None` → first), or `None`
/// when already at the end. Pure.
pub fn next_block_id(blocks: &[CommandBlock], current: Option<BlockId>) -> Option<BlockId> {
    match current {
        None => blocks.first().map(|b| b.id),
        Some(id) => {
            let idx = blocks.iter().position(|b| b.id == id)?;
            blocks.get(idx + 1).map(|b| b.id)
        }
    }
}

/// Return the block ID immediately before `current` (`None` → last), or `None`
/// when already at the start. Pure.
pub fn prev_block_id(blocks: &[CommandBlock], current: Option<BlockId>) -> Option<BlockId> {
    match current {
        None => blocks.last().map(|b| b.id),
        Some(id) => {
            let idx = blocks.iter().position(|b| b.id == id)?;
            if idx == 0 {
                None
            } else {
                blocks.get(idx - 1).map(|b| b.id)
            }
        }
    }
}

/// Visual status of a block, derived from its exit code and completeness.
///
/// Drives the colour of the left border and the choice of status badge in the
/// renderer overlay pass. The renderer itself decides the exact RGB values;
/// this enum only carries the categorical meaning so the mapping is unit-
/// testable independently of GPU code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockStatus {
    /// Block has not yet observed a `D` mark (still executing or shell
    /// omitted the trailing sequence).
    Running,
    /// Block ended with exit code `0`.
    Success,
    /// Block ended with a non-zero exit code.
    Failure,
}

impl BlockStatus {
    /// Derive the status of a block from its end_row / exit_code.
    pub fn of(block: &CommandBlock) -> Self {
        match (block.end_row, block.exit_code) {
            (None, _) => Self::Running,
            (Some(_), None) => Self::Running, // D arrived without an explicit code
            (Some(_), Some(0)) => Self::Success,
            (Some(_), Some(_)) => Self::Failure,
        }
    }
}

/// One block's contribution to the renderer overlay pass.
///
/// Coordinates are expressed in **grid rows relative to the top of the visible
/// area**. A value of `0` means the very first visible row; a negative value
/// means the block extends above the viewport and only its tail is on screen.
///
/// `visual_row_end` is **inclusive** (so an entry covering rows 3 through 5
/// has `start = 3, end = 5`). Callers that want a half-open range should add
/// one to `visual_row_end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockOverlayLine {
    pub block_id: BlockId,
    pub visual_row_start: i32,
    pub visual_row_end: i32,
    pub status: BlockStatus,
    pub selected: bool,
    /// Grid row hosting the prompt (the `A` mark). Used by the renderer to
    /// place the exit-status badge. `None` if the prompt sits off-screen above.
    pub prompt_visual_row: Option<i32>,
}

/// Map block records onto the visible grid window.
///
/// `visible_top` is the scrollback-absolute index that corresponds to the top
/// of the viewport (i.e. `pane.scrollback.len() - grid_rows - scroll_offset`
/// in the typical case). `visible_rows` is the grid height in rows.
///
/// The output is filtered to blocks that overlap the visible window. For a
/// `Running` block (no `D` mark) the trailing edge is taken to be the bottom
/// of the viewport, so the border keeps growing as new output arrives.
///
/// Pure — no `ClientState` or GPU dependencies, so it's safe to unit-test in
/// isolation.
pub fn compute_block_overlay_lines(
    blocks: &[CommandBlock],
    selected_block: Option<BlockId>,
    visible_top: usize,
    visible_rows: u16,
) -> Vec<BlockOverlayLine> {
    if visible_rows == 0 {
        return Vec::new();
    }
    let viewport_end_exclusive = visible_top.saturating_add(visible_rows as usize);
    let viewport_end_inclusive = viewport_end_exclusive.saturating_sub(1);

    blocks
        .iter()
        .filter_map(|b| {
            // Treat an unfinished block as extending to the bottom of the
            // viewport. A finished block ends on its `D` row.
            let block_start = b.prompt_row;
            let block_end = b.end_row.unwrap_or(viewport_end_inclusive);

            // Reject blocks that do not overlap the viewport at all.
            if block_end < visible_top {
                return None;
            }
            if block_start >= viewport_end_exclusive {
                return None;
            }

            // Translate to grid-row coordinates (signed because the start may
            // be above the viewport).
            let visual_row_start = block_start as i32 - visible_top as i32;
            let visual_row_end = block_end as i32 - visible_top as i32;
            let prompt_visual_row = if block_start >= visible_top {
                Some(block_start as i32 - visible_top as i32)
            } else {
                None
            };

            Some(BlockOverlayLine {
                block_id: b.id,
                visual_row_start,
                visual_row_end,
                status: BlockStatus::of(b),
                selected: selected_block == Some(b.id),
                prompt_visual_row,
            })
        })
        .collect()
}

/// Strip a candidate replay-command string of anything that is not safe to
/// re-inject into a PTY.
///
/// Rules:
/// - Trim leading / trailing whitespace.
/// - Reject (return `None`) if the result is empty.
/// - Reject if the string contains any ESC (`0x1B`), BEL (`0x07`), CSI start
///   bytes (`0x9B`), or any other C0 control byte except `\t` (`0x09`).
/// - A single trailing `\n` is tolerated and stripped (shells append it).
/// - Embedded `\n` / `\r` are rejected, since replaying a multi-line block by
///   re-injecting the captured output would execute everything immediately.
///
/// The intent is defensive: a remote SSH peer should not be able to turn a
/// captured "command" into a fresh exploit vector when the user hits Replay.
pub fn sanitize_replay_command(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    // Tolerate a single trailing newline that some shells emit on B.
    if let Some(stripped) = s.strip_suffix('\n') {
        s = stripped.trim_end_matches('\r');
    }
    if s.is_empty() {
        return None;
    }
    for byte in s.bytes() {
        // Allow printable ASCII + UTF-8 continuation + tab.
        if byte == b'\t' {
            continue;
        }
        if byte < 0x20 {
            return None;
        }
        if byte == 0x7F {
            return None;
        }
        // CSI single-byte start (0x9B). Other 0x80–0xFF are UTF-8 continuation.
        if byte == 0x9B {
            return None;
        }
    }
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(row: usize, kind: SemanticMarkKind, exit_code: Option<i32>) -> SemanticMark {
        SemanticMark {
            row,
            kind,
            exit_code,
        }
    }

    #[test]
    fn empty_marks_yield_no_blocks() {
        assert!(extract_command_blocks(1, &[]).is_empty());
    }

    #[test]
    fn full_abcd_sequence_emits_one_complete_block() {
        let marks = vec![
            m(10, SemanticMarkKind::PromptStart, None),
            m(10, SemanticMarkKind::CommandStart, None),
            m(11, SemanticMarkKind::OutputStart, None),
            m(15, SemanticMarkKind::CommandEnd, Some(0)),
        ];
        let blocks = extract_command_blocks(7, &marks);
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b.pane_id, 7);
        assert_eq!(b.prompt_row, 10);
        assert_eq!(b.command_row, 10);
        assert_eq!(b.output_row, 11);
        assert_eq!(b.end_row, Some(15));
        assert_eq!(b.exit_code, Some(0));
        assert!(b.is_complete());
        assert!(!b.is_failure());
        assert_eq!(b.id, make_block_id(7, 10));
    }

    #[test]
    fn nonzero_exit_marks_block_as_failure() {
        let marks = vec![
            m(0, SemanticMarkKind::PromptStart, None),
            m(0, SemanticMarkKind::CommandEnd, Some(127)),
        ];
        let blocks = extract_command_blocks(1, &marks);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].is_failure());
        assert_eq!(blocks[0].exit_code, Some(127));
    }

    #[test]
    fn multiple_blocks_are_collected_in_order() {
        let marks = vec![
            m(0, SemanticMarkKind::PromptStart, None),
            m(0, SemanticMarkKind::CommandEnd, Some(0)),
            m(5, SemanticMarkKind::PromptStart, None),
            m(5, SemanticMarkKind::CommandEnd, Some(1)),
            m(20, SemanticMarkKind::PromptStart, None),
            m(20, SemanticMarkKind::CommandEnd, Some(0)),
        ];
        let blocks = extract_command_blocks(2, &marks);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].prompt_row, 0);
        assert_eq!(blocks[1].prompt_row, 5);
        assert_eq!(blocks[2].prompt_row, 20);
        assert!(blocks[1].is_failure());
    }

    #[test]
    fn block_without_d_is_left_running() {
        let marks = vec![
            m(0, SemanticMarkKind::PromptStart, None),
            m(0, SemanticMarkKind::CommandStart, None),
            m(1, SemanticMarkKind::OutputStart, None),
        ];
        let blocks = extract_command_blocks(1, &marks);
        assert_eq!(blocks.len(), 1);
        assert!(!blocks[0].is_complete());
        assert_eq!(blocks[0].end_row, None);
        assert!(!blocks[0].is_failure(), "no exit code yet => not a failure");
    }

    #[test]
    fn second_a_before_d_flushes_previous_block_as_running() {
        let marks = vec![
            m(0, SemanticMarkKind::PromptStart, None),
            m(0, SemanticMarkKind::CommandStart, None),
            m(5, SemanticMarkKind::PromptStart, None),
            m(5, SemanticMarkKind::CommandEnd, Some(0)),
        ];
        let blocks = extract_command_blocks(1, &marks);
        assert_eq!(blocks.len(), 2);
        assert!(!blocks[0].is_complete());
        assert!(blocks[1].is_complete());
    }

    #[test]
    fn marks_before_first_a_are_ignored() {
        let marks = vec![
            m(0, SemanticMarkKind::CommandStart, None),
            m(0, SemanticMarkKind::OutputStart, None),
            m(0, SemanticMarkKind::CommandEnd, Some(0)),
            m(5, SemanticMarkKind::PromptStart, None),
            m(5, SemanticMarkKind::CommandEnd, Some(0)),
        ];
        let blocks = extract_command_blocks(1, &marks);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].prompt_row, 5);
    }

    #[test]
    fn missing_b_keeps_command_row_at_prompt() {
        // Some shells emit only A, C, D (no separate B).
        let marks = vec![
            m(3, SemanticMarkKind::PromptStart, None),
            m(3, SemanticMarkKind::OutputStart, None),
            m(8, SemanticMarkKind::CommandEnd, Some(0)),
        ];
        let blocks = extract_command_blocks(1, &marks);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].command_row, 3);
        assert_eq!(blocks[0].output_row, 3);
        assert_eq!(blocks[0].end_row, Some(8));
    }

    #[test]
    fn from_ipc_decodes_known_kinds() {
        assert_eq!(
            SemanticMarkKind::from_ipc("A"),
            Some(SemanticMarkKind::PromptStart)
        );
        assert_eq!(
            SemanticMarkKind::from_ipc("B"),
            Some(SemanticMarkKind::CommandStart)
        );
        assert_eq!(
            SemanticMarkKind::from_ipc("C"),
            Some(SemanticMarkKind::OutputStart)
        );
        assert_eq!(
            SemanticMarkKind::from_ipc("D"),
            Some(SemanticMarkKind::CommandEnd)
        );
        assert_eq!(SemanticMarkKind::from_ipc("E"), None);
        assert_eq!(SemanticMarkKind::from_ipc(""), None);
    }

    #[test]
    fn make_block_id_is_pane_then_row() {
        let id = make_block_id(0xAABBCCDD, 0x1234);
        assert_eq!(id, 0xAABBCCDD_00001234);
    }

    fn sample_blocks() -> Vec<CommandBlock> {
        let marks = vec![
            m(0, SemanticMarkKind::PromptStart, None),
            m(2, SemanticMarkKind::CommandEnd, Some(0)),
            m(10, SemanticMarkKind::PromptStart, None),
            m(15, SemanticMarkKind::CommandEnd, Some(1)),
            m(20, SemanticMarkKind::PromptStart, None),
            m(25, SemanticMarkKind::CommandEnd, Some(0)),
        ];
        extract_command_blocks(1, &marks)
    }

    #[test]
    fn next_block_id_from_none_returns_first() {
        let blocks = sample_blocks();
        assert_eq!(next_block_id(&blocks, None), Some(blocks[0].id));
    }

    #[test]
    fn next_block_id_walks_forward() {
        let blocks = sample_blocks();
        assert_eq!(
            next_block_id(&blocks, Some(blocks[0].id)),
            Some(blocks[1].id)
        );
        assert_eq!(
            next_block_id(&blocks, Some(blocks[1].id)),
            Some(blocks[2].id)
        );
        assert_eq!(next_block_id(&blocks, Some(blocks[2].id)), None);
    }

    #[test]
    fn prev_block_id_from_none_returns_last() {
        let blocks = sample_blocks();
        assert_eq!(prev_block_id(&blocks, None), Some(blocks[2].id));
    }

    #[test]
    fn prev_block_id_walks_backward() {
        let blocks = sample_blocks();
        assert_eq!(
            prev_block_id(&blocks, Some(blocks[2].id)),
            Some(blocks[1].id)
        );
        assert_eq!(
            prev_block_id(&blocks, Some(blocks[1].id)),
            Some(blocks[0].id)
        );
        assert_eq!(prev_block_id(&blocks, Some(blocks[0].id)), None);
    }

    #[test]
    fn navigation_returns_none_when_current_is_unknown() {
        let blocks = sample_blocks();
        assert_eq!(next_block_id(&blocks, Some(0xDEADBEEF)), None);
        assert_eq!(prev_block_id(&blocks, Some(0xDEADBEEF)), None);
    }

    #[test]
    fn find_block_by_id_returns_match() {
        let blocks = sample_blocks();
        let id = blocks[1].id;
        assert_eq!(find_block_by_id(&blocks, id).map(|b| b.id), Some(id));
        assert!(find_block_by_id(&blocks, 0xDEADBEEF).is_none());
    }

    #[test]
    fn sanitize_replay_accepts_plain_command() {
        assert_eq!(
            sanitize_replay_command("ls -la"),
            Some("ls -la".to_string())
        );
    }

    #[test]
    fn sanitize_replay_trims_whitespace_and_trailing_newline() {
        assert_eq!(
            sanitize_replay_command("  echo hi  \n"),
            Some("echo hi".to_string())
        );
        assert_eq!(
            sanitize_replay_command("echo hi\r\n"),
            Some("echo hi".to_string())
        );
    }

    #[test]
    fn sanitize_replay_rejects_empty() {
        assert_eq!(sanitize_replay_command(""), None);
        assert_eq!(sanitize_replay_command("   \n  "), None);
    }

    #[test]
    fn sanitize_replay_rejects_escape_sequences() {
        // ESC + CSI ; deny anything that could re-arm a control sequence.
        assert_eq!(sanitize_replay_command("ls\x1b[31m"), None);
        assert_eq!(sanitize_replay_command("ls\x07"), None);
        assert_eq!(sanitize_replay_command("ls\u{9b}H"), None);
    }

    #[test]
    fn sanitize_replay_rejects_embedded_newlines() {
        // Replaying multi-line content would execute every line at once.
        assert_eq!(sanitize_replay_command("echo a\necho b"), None);
        assert_eq!(sanitize_replay_command("echo a\recho b"), None);
    }

    #[test]
    fn sanitize_replay_allows_tab_and_utf8() {
        assert_eq!(
            sanitize_replay_command("grep\tfoo bar"),
            Some("grep\tfoo bar".to_string())
        );
        assert_eq!(
            sanitize_replay_command("echo 日本語"),
            Some("echo 日本語".to_string())
        );
    }

    #[test]
    fn sanitize_replay_rejects_del() {
        assert_eq!(sanitize_replay_command("ls\x7f"), None);
    }

    // ---- compute_block_overlay_lines / BlockStatus ----

    fn finished(
        id: BlockId,
        pane: u32,
        start: usize,
        end: usize,
        exit: Option<i32>,
    ) -> CommandBlock {
        CommandBlock {
            id,
            pane_id: pane,
            prompt_row: start,
            command_row: start,
            output_row: start + 1,
            end_row: Some(end),
            exit_code: exit,
            collapsed: false,
        }
    }

    fn running(id: BlockId, pane: u32, start: usize) -> CommandBlock {
        CommandBlock {
            id,
            pane_id: pane,
            prompt_row: start,
            command_row: start,
            output_row: start + 1,
            end_row: None,
            exit_code: None,
            collapsed: false,
        }
    }

    #[test]
    fn block_status_distinguishes_running_success_failure() {
        assert_eq!(
            BlockStatus::of(&running(make_block_id(1, 0), 1, 0)),
            BlockStatus::Running
        );
        assert_eq!(
            BlockStatus::of(&finished(make_block_id(1, 0), 1, 0, 4, Some(0))),
            BlockStatus::Success
        );
        assert_eq!(
            BlockStatus::of(&finished(make_block_id(1, 0), 1, 0, 4, Some(127))),
            BlockStatus::Failure
        );
        // D arrived but no exit code → still running (ambiguous).
        assert_eq!(
            BlockStatus::of(&finished(make_block_id(1, 0), 1, 0, 4, None)),
            BlockStatus::Running
        );
    }

    #[test]
    fn overlay_returns_empty_when_viewport_is_zero_rows() {
        let blocks = vec![finished(1, 1, 0, 4, Some(0))];
        assert!(compute_block_overlay_lines(&blocks, None, 0, 0).is_empty());
    }

    #[test]
    fn overlay_drops_block_entirely_above_viewport() {
        // Block lives at rows 0..=4, viewport is 100..120 → no overlap.
        let blocks = vec![finished(1, 1, 0, 4, Some(0))];
        assert!(compute_block_overlay_lines(&blocks, None, 100, 20).is_empty());
    }

    #[test]
    fn overlay_drops_block_entirely_below_viewport() {
        // Block at rows 200..210, viewport 0..50 → no overlap.
        let blocks = vec![finished(1, 1, 200, 210, Some(0))];
        assert!(compute_block_overlay_lines(&blocks, None, 0, 50).is_empty());
    }

    #[test]
    fn overlay_maps_block_fully_inside_viewport() {
        // Block at rows 10..=15, viewport 0..24.
        let blocks = vec![finished(7, 1, 10, 15, Some(0))];
        let lines = compute_block_overlay_lines(&blocks, None, 0, 24);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.block_id, 7);
        assert_eq!(line.visual_row_start, 10);
        assert_eq!(line.visual_row_end, 15);
        assert_eq!(line.status, BlockStatus::Success);
        assert!(!line.selected);
        assert_eq!(line.prompt_visual_row, Some(10));
    }

    #[test]
    fn overlay_clips_block_starting_above_viewport_off_screen() {
        // Block at scrollback rows 5..=30 with viewport 20..40.
        // Block still overlaps (rows 20-30 visible). prompt_row=5 is above
        // viewport so prompt_visual_row is None.
        let blocks = vec![finished(99, 1, 5, 30, Some(1))];
        let lines = compute_block_overlay_lines(&blocks, None, 20, 20);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.visual_row_start, -15);
        assert_eq!(line.visual_row_end, 10);
        assert!(
            line.prompt_visual_row.is_none(),
            "prompt sits above viewport"
        );
        assert_eq!(line.status, BlockStatus::Failure);
    }

    #[test]
    fn overlay_running_block_extends_to_viewport_bottom() {
        // Running block at row 5, viewport 0..24 → end clamps to row 23.
        let blocks = vec![running(42, 1, 5)];
        let lines = compute_block_overlay_lines(&blocks, None, 0, 24);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.visual_row_start, 5);
        assert_eq!(line.visual_row_end, 23);
        assert_eq!(line.status, BlockStatus::Running);
    }

    #[test]
    fn overlay_propagates_selection_flag() {
        let blocks = vec![
            finished(1, 1, 0, 2, Some(0)),
            finished(2, 1, 10, 12, Some(0)),
        ];
        let lines = compute_block_overlay_lines(&blocks, Some(2), 0, 24);
        assert_eq!(lines.len(), 2);
        let selected: Vec<_> = lines.iter().filter(|l| l.selected).collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].block_id, 2);
    }

    // ---- is_row_collapsed -------------------------------------------------

    fn collapsed(id: u64, pane: u32, prompt: usize, output: usize, end: usize) -> CommandBlock {
        let mut b = finished(id, pane, prompt, end, Some(0));
        b.command_row = prompt;
        b.output_row = output;
        b.collapsed = true;
        b
    }

    #[test]
    fn is_row_collapsed_returns_false_for_empty_blocks() {
        assert!(!is_row_collapsed(&[], 0));
        assert!(!is_row_collapsed(&[], 999));
    }

    #[test]
    fn is_row_collapsed_returns_false_when_block_not_collapsed() {
        let blocks = vec![finished(1, 1, 10, 20, Some(0))];
        for r in 10..=20 {
            assert!(
                !is_row_collapsed(&blocks, r),
                "row {} should not be elided",
                r
            );
        }
    }

    #[test]
    fn is_row_collapsed_returns_false_for_running_block_even_if_collapsed() {
        // No end_row → block still streaming output → never elide.
        let mut b = running(1, 1, 10);
        b.collapsed = true;
        b.output_row = 11;
        assert!(!is_row_collapsed(&[b], 12));
    }

    #[test]
    fn is_row_collapsed_keeps_prompt_and_first_output_rows_visible() {
        let blocks = vec![collapsed(1, 1, 10, 11, 20)];
        assert!(!is_row_collapsed(&blocks, 10), "prompt row visible");
        assert!(!is_row_collapsed(&blocks, 11), "first output row visible");
    }

    #[test]
    fn is_row_collapsed_elides_inner_output_rows() {
        let blocks = vec![collapsed(1, 1, 10, 11, 20)];
        for r in 12..=20 {
            assert!(is_row_collapsed(&blocks, r), "row {} should be elided", r);
        }
    }

    #[test]
    fn is_row_collapsed_ignores_rows_outside_block_range() {
        let blocks = vec![collapsed(1, 1, 10, 11, 20)];
        assert!(!is_row_collapsed(&blocks, 9));
        assert!(!is_row_collapsed(&blocks, 21));
    }

    #[test]
    fn is_row_collapsed_handles_multiple_blocks() {
        let blocks = vec![
            collapsed(1, 1, 0, 1, 5),
            finished(2, 1, 10, 15, Some(0)), // not collapsed
            collapsed(3, 1, 20, 21, 30),
        ];
        // First collapsed block elides rows 2..=5
        assert!(is_row_collapsed(&blocks, 3));
        // Middle block: not collapsed → all rows visible
        assert!(!is_row_collapsed(&blocks, 12));
        // Third collapsed block elides rows 22..=30
        assert!(is_row_collapsed(&blocks, 25));
        assert!(!is_row_collapsed(&blocks, 20));
        assert!(!is_row_collapsed(&blocks, 21));
    }

    // ---- overlay ordering -------------------------------------------------

    #[test]
    fn overlay_orders_lines_in_block_order() {
        let blocks = vec![
            finished(10, 1, 0, 2, Some(0)),
            finished(11, 1, 5, 7, Some(0)),
            finished(12, 1, 15, 17, Some(0)),
        ];
        let lines = compute_block_overlay_lines(&blocks, None, 0, 24);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].block_id, 10);
        assert_eq!(lines[1].block_id, 11);
        assert_eq!(lines[2].block_id, 12);
    }
}
