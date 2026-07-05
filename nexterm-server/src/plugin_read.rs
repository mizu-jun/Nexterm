//! Plugin read API data extraction and policy gate (F3 / ADR-0008).
//!
//! Bridges the WASM plugin host's `read_pane` / `read_grid` / `read_scrollback`
//! host imports to live pane state. The server installs the [`ReadFn`] built
//! here via `PluginManager::set_read_fn` at startup.
//!
//! Guard order follows ADR-0008: the **policy** gate runs first (fail-safe —
//! only an explicit `allow` enables reads; `deny` and `prompt` both deny), then
//! the **pane existence** gate. The per-hook pane allow-list gate lives in the
//! plugin host itself.

use std::sync::Arc;

use nexterm_config::ConsentPolicy;
use nexterm_plugin::{ReadFn, ReadKind, ReadOutcome};
use nexterm_proto::{Attrs, Cell, Color, Grid};

use crate::session::SessionManager;

/// Build the plugin read callback wired to live pane state.
///
/// `max_bytes` caps a single read (DoS / egress guard); `scrollback_lines` is
/// the configured retention that `read_scrollback`'s `max_lines` is clamped to.
pub fn build_read_fn(
    manager: Arc<SessionManager>,
    policy: ConsentPolicy,
    max_bytes: usize,
    scrollback_lines: usize,
) -> ReadFn {
    Arc::new(move |pane_id: u32, kind: ReadKind| {
        // Policy gate first (ADR-0008). `prompt` has no synchronous UI path for
        // a server-side plugin call, so it is treated as `deny` (fail-safe).
        if policy != ConsentPolicy::Allow {
            return ReadOutcome::Denied;
        }
        // Existence gate (also fails closed if the sessions map is momentarily
        // contended — see `SessionManager::pane_snapshot`).
        let Some((grid, scrollback)) = manager.pane_snapshot(pane_id) else {
            return ReadOutcome::UnknownPane;
        };
        let bytes = match kind {
            ReadKind::PaneText => cap_text(grid_to_text(&grid), max_bytes).into_bytes(),
            ReadKind::Grid => cap_grid_dump(grid_to_dump(&grid), max_bytes),
            ReadKind::Scrollback {
                start_line,
                max_lines,
            } => {
                let text = scrollback_to_text(
                    &scrollback,
                    start_line as usize,
                    max_lines as usize,
                    scrollback_lines,
                );
                cap_text(text, max_bytes).into_bytes()
            }
        };
        ReadOutcome::Data(bytes)
    })
}

/// Render the visible grid as text: each row's cells joined, trailing blanks
/// trimmed per row, and trailing blank rows dropped.
fn grid_to_text(grid: &Grid) -> String {
    let mut lines: Vec<String> = grid
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| c.ch)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Serialize the visible grid to the ADR-0008 §3 wire format:
/// `u16 cols, u16 rows`, then row-major cells of
/// `u32 codepoint, u8 fg_index, u8 bg_index, u8 attr_bits, u8 reserved`
/// (all little-endian).
fn grid_to_dump(grid: &Grid) -> Vec<u8> {
    let cols = grid.width;
    let rows = grid.height;
    let mut out = Vec::with_capacity(4 + rows as usize * cols as usize * 8);
    out.extend_from_slice(&cols.to_le_bytes());
    out.extend_from_slice(&rows.to_le_bytes());
    for r in 0..rows as usize {
        for c in 0..cols as usize {
            let cell = grid.rows.get(r).and_then(|row| row.get(c));
            let (ch, fg, bg, attrs) = match cell {
                Some(cell) => (
                    cell.ch,
                    color_index(cell.fg),
                    color_index(cell.bg),
                    attr_bits(cell.attrs),
                ),
                None => (' ', 0xFF, 0xFF, 0),
            };
            out.extend_from_slice(&(ch as u32).to_le_bytes());
            out.push(fg);
            out.push(bg);
            out.push(attrs);
            out.push(0); // reserved
        }
    }
    out
}

/// Cap a grid dump at `max` bytes **on a whole-row boundary**, rewriting the
/// header's `rows` field so it stays consistent with the retained payload.
///
/// A naive byte truncation could split an 8-byte cell record or leave the
/// header claiming more rows than are present, making a parser that reads
/// `cols * rows` cells overrun the buffer. Capping at a whole row and updating
/// `rows` keeps the dump self-consistent. ADR-0008 §3.
fn cap_grid_dump(mut dump: Vec<u8>, max: usize) -> Vec<u8> {
    const HEADER: usize = 4; // u16 cols + u16 rows
    const CELL: usize = 8; // u32 codepoint + fg + bg + attrs + reserved
    if dump.len() <= max {
        return dump;
    }
    if max < HEADER {
        // Cannot even hold the header; return nothing rather than a fragment.
        return Vec::new();
    }
    let cols = u16::from_le_bytes([dump[0], dump[1]]) as usize;
    let row_bytes = cols.saturating_mul(CELL);
    if row_bytes == 0 {
        // Zero-width grid: only the header is meaningful.
        dump.truncate(HEADER);
        return dump;
    }
    let kept_rows = (max - HEADER) / row_bytes;
    // `kept_rows <= actual rows <= u16::MAX` here, so the cast cannot wrap:
    // truncation only runs when the payload already exceeded `max`.
    let rows_le = (kept_rows as u16).to_le_bytes();
    dump[2] = rows_le[0];
    dump[3] = rows_le[1];
    dump.truncate(HEADER + kept_rows * row_bytes);
    dump
}

/// Map a color to a palette index. Indexed colors pass through; `Default` and
/// TrueColor (no palette index) map to `0xFF` (scheme default). ADR-0008 §3.
fn color_index(c: Color) -> u8 {
    match c {
        Color::Indexed(i) => i,
        _ => 0xFF,
    }
}

/// Pack the character attributes into the ADR-0008 §3 `attr_bits` byte.
fn attr_bits(a: Attrs) -> u8 {
    let mut bits = 0u8;
    if a.is_bold() {
        bits |= 0b0001;
    }
    if a.is_italic() {
        bits |= 0b0010;
    }
    if a.is_underline() {
        bits |= 0b0100;
    }
    if a.is_reverse() {
        bits |= 0b1000;
    }
    bits
}

/// Render `max_lines` scrollback lines starting at `start` as LF-joined text.
/// `max_lines` is clamped to the configured `scrollback_lines` retention.
fn scrollback_to_text(
    scrollback: &[Vec<Cell>],
    start: usize,
    max_lines: usize,
    scrollback_lines: usize,
) -> String {
    let limit = max_lines.min(scrollback_lines.max(1));
    scrollback
        .iter()
        .skip(start)
        .take(limit)
        .map(|row| {
            row.iter()
                .map(|c| c.ch)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Truncate a string at `max` bytes, respecting UTF-8 character boundaries.
fn cap_text(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(ch: char) -> Cell {
        Cell {
            ch,
            ..Cell::default()
        }
    }

    fn grid_from(lines: &[&str]) -> Grid {
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let mut g = Grid::new(width, lines.len() as u16);
        for (r, line) in lines.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                g.rows[r][c] = cell(ch);
            }
        }
        g
    }

    #[test]
    fn grid_to_text_trims_trailing_blanks() {
        let g = grid_from(&["hello", "world", "", ""]);
        assert_eq!(grid_to_text(&g), "hello\nworld");
    }

    #[test]
    fn grid_to_dump_header_and_cell_layout() {
        let g = grid_from(&["ab"]);
        let dump = grid_to_dump(&g);
        // cols=2, rows=1 header (4 bytes) + 2 cells * 8 bytes.
        assert_eq!(dump.len(), 4 + 2 * 8);
        assert_eq!(&dump[0..2], &2u16.to_le_bytes());
        assert_eq!(&dump[2..4], &1u16.to_le_bytes());
        // First cell codepoint == 'a'.
        assert_eq!(&dump[4..8], &(u32::from('a')).to_le_bytes());
    }

    #[test]
    fn scrollback_to_text_respects_start_and_clamp() {
        let sb: Vec<Vec<Cell>> = ["one", "two", "three", "four"]
            .iter()
            .map(|l| l.chars().map(cell).collect())
            .collect();
        // start=1, max_lines=2 → "two\nthree".
        assert_eq!(scrollback_to_text(&sb, 1, 2, 10_000), "two\nthree");
        // max_lines clamped to scrollback_lines=1 → only "one".
        assert_eq!(scrollback_to_text(&sb, 0, 999, 1), "one");
    }

    #[test]
    fn cap_text_truncates_on_char_boundary() {
        // "あ" is 3 bytes in UTF-8; capping at 2 bytes drops it entirely.
        assert_eq!(cap_text("あ".to_string(), 2), "");
        assert_eq!(cap_text("abc".to_string(), 2), "ab");
    }

    #[test]
    fn cap_grid_dump_keeps_whole_rows_and_rewrites_header() {
        // 3 cols x 4 rows grid dump: header (4) + 12 cells * 8 = 100 bytes.
        let g = grid_from(&["abc", "def", "ghi", "jkl"]);
        let full = grid_to_dump(&g);
        assert_eq!(full.len(), 4 + 12 * 8);

        // Cap so only 2 rows fit: header + 2*(3*8) = 52 bytes. Pick a max that
        // lands mid-row (60) to prove it rounds down to a whole-row boundary.
        let capped = cap_grid_dump(full.clone(), 60);
        let row_bytes = 3 * 8;
        assert_eq!(capped.len(), 4 + 2 * row_bytes);
        // Header cols unchanged, rows rewritten to 2.
        assert_eq!(&capped[0..2], &3u16.to_le_bytes());
        assert_eq!(&capped[2..4], &2u16.to_le_bytes());

        // No truncation when it already fits.
        assert_eq!(cap_grid_dump(full.clone(), 10_000), full);

        // Below the header size → empty (no fragment).
        assert!(cap_grid_dump(full, 3).is_empty());
    }

    #[test]
    fn attr_bits_packs_flags() {
        assert_eq!(attr_bits(Attrs::new(Attrs::BOLD)), 0b0001);
        assert_eq!(attr_bits(Attrs::new(Attrs::UNDERLINE)), 0b0100);
        assert_eq!(attr_bits(Attrs::new(Attrs::BOLD | Attrs::REVERSE)), 0b1001);
    }
}
