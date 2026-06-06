//! Virtual screen — grid plus dirty flags plus cursor state.

use nexterm_proto::{Attrs, Cell, Color, DirtyRow, Grid};
use unicode_width::UnicodeWidthChar;

use crate::image::{decode_kitty, decode_sixel};

/// Maximum size of the DCS Sixel buffer.
///
/// Mitigates the DoS where a malicious PTY streams an unterminated DCS forever
/// (CRITICAL #7). 16 MiB accommodates one page of a large Sixel image plus
/// some margin.
const MAX_DCS_BUF_LEN: usize = 16 * 1024 * 1024;

/// Maximum size of a Kitty multi-chunk transfer payload.
///
/// Mitigates the DoS in which a peer keeps sending `m=1` chunks forever
/// (CRITICAL #7). 64 MiB is the upper bound for the combined chunks.
const MAX_KITTY_CHUNK_LEN: usize = 64 * 1024 * 1024;

/// Maximum length (in bytes) of an OSC title string.
///
/// Prevents a very long title from causing a DoS in the terminal bar or window
/// manager (mitigates CRITICAL #13). 256 bytes covers virtually every terminal's
/// practical range.
const MAX_TITLE_LEN: usize = 256;

/// Maximum length (in bytes) for the title/body of an OSC 9 desktop notification.
const MAX_NOTIFICATION_LEN: usize = 1024;

/// Maximum length (in bytes) of an OSC 8 hyperlink URI.
const MAX_HYPERLINK_URI_LEN: usize = 2048;

/// Maximum length (in bytes) of an OSC 7 CWD (working-directory) path.
///
/// Set to 4096 (Linux `PATH_MAX` equivalent). Longer paths are truncated as a
/// DoS countermeasure.
const MAX_CWD_LEN: usize = 4096;

/// URI schemes allowed in OSC 8 hyperlinks.
///
/// The previous implementation accepted every scheme — including
/// `javascript:` and `file:` — which allowed a malicious SSH destination to
/// trigger clickjacking or local-file access through the terminal (CRITICAL #13).
const ALLOWED_HYPERLINK_SCHEMES: &[&str] = &[
    "http://", "https://", "mailto:", "ftp://", "ftps://", "ssh://",
];

/// Sanitizes a string sourced from an OSC sequence.
///
/// - Strips control characters (C0/C1) to prevent log injection and forged
///   line breaks.
/// - Truncates to the length cap, respecting UTF-8 boundaries (e.g. CJK).
fn sanitize_osc_string(s: String, max_len: usize) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| {
            // Drop C0 (0x00..=0x1F, except TAB/LF/CR), DEL, and C1 (0x80..=0x9F).
            let cp = *c as u32;
            !(cp <= 0x1F && *c != '\t' && *c != '\n' && *c != '\r')
                && cp != 0x7F
                && !(0x80..=0x9F).contains(&cp)
        })
        .collect();

    if cleaned.len() <= max_len {
        return cleaned;
    }
    // Truncate safely on a byte boundary.
    let mut end = max_len;
    while end > 0 && !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    cleaned[..end].to_string()
}

/// Validates an OSC 8 hyperlink URI.
///
/// Returns `None` when the URI is too long or does not use an allowed scheme
/// (i.e. the link is disabled).
pub(crate) fn validate_hyperlink_uri(uri: &str) -> Option<String> {
    if uri.is_empty() || uri.len() > MAX_HYPERLINK_URI_LEN {
        return None;
    }
    let lower = uri.to_lowercase();
    if !ALLOWED_HYPERLINK_SCHEMES
        .iter()
        .any(|s| lower.starts_with(s))
    {
        tracing::warn!(
            "OSC 8: disallowed URI scheme — disabling link: {}",
            &uri[..uri.len().min(80)]
        );
        return None;
    }
    // Strip control characters.
    let cleaned: String = uri
        .chars()
        .filter(|c| {
            let cp = *c as u32;
            cp >= 0x20 && cp != 0x7F && !(0x80..=0x9F).contains(&cp)
        })
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned)
}

/// Extracts the CWD path from the OSC 7 form `file://[host]/percent-encoded-path`.
///
/// Accepted inputs include:
/// - `file:///home/user/proj` (host omitted; the leading `/` is kept).
/// - `file://hostname/home/user` (host present; ignored, only the path is taken).
/// - `file:///C:/Users/foo` (Windows-style; the leading `/` is stripped and the
///   value is normalized to `C:/Users/foo`).
/// - `/home/user/proj` (no scheme; passed through for compatibility).
///
/// Control characters are stripped, and the result is truncated to
/// [`MAX_CWD_LEN`]. Returns `None` if the result becomes completely empty.
pub(crate) fn parse_osc7_cwd(input: &str) -> Option<String> {
    if input.is_empty() || input.len() > MAX_CWD_LEN * 4 {
        // Reject excessively large inputs up front for DoS reasons (this runs
        // before percent decoding, so 4× the cap is a reasonable margin).
        return None;
    }

    // Strip the `file://` prefix and skip the host segment (`//host/path`).
    let after_scheme = if let Some(rest) = input.strip_prefix("file://") {
        // Advance to the `/path` part (the first `/`, regardless of whether the
        // host segment is empty).
        match rest.find('/') {
            Some(idx) => &rest[idx..],
            None => return None, // No path.
        }
    } else {
        input
    };

    // Percent-decode.
    let decoded = percent_decode(after_scheme);

    // Windows path support: `/C:/Users/foo` → `C:/Users/foo`.
    #[cfg(windows)]
    let decoded = if decoded.len() >= 3
        && decoded.starts_with('/')
        && decoded.as_bytes()[2] == b':'
        && decoded.as_bytes()[1].is_ascii_alphabetic()
    {
        decoded[1..].to_string()
    } else {
        decoded
    };

    // Strip control characters and apply the length cap.
    let cleaned = sanitize_osc_string(decoded, MAX_CWD_LEN);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Minimal `%XX` percent-decoder (just enough for OSC 7).
///
/// Malformed `%XX` triplets are passed through (so we do not corrupt path names
/// that legitimately contain a `%`).
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Decode invalid UTF-8 lossily (OSC 7 paths are normally UTF-8 anyway).
    String::from_utf8_lossy(&out).into_owned()
}

/// OSC 133 semantic-zone mark kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticMarkKind {
    /// Prompt start (A).
    PromptStart,
    /// Command input start (B).
    CommandStart,
    /// Command execution / output start (C).
    OutputStart,
    /// Command end (D).
    CommandEnd,
}

/// OSC 133 semantic-zone mark (row + kind).
#[derive(Debug, Clone)]
pub struct SemanticMark {
    /// Grid row (0-based).
    pub row: u16,
    /// Mark kind.
    pub kind: SemanticMarkKind,
    /// Exit code on command end (only `Some` for a `D` mark).
    pub exit_code: Option<i32>,
}

/// Pending image (before being forwarded to the client).
pub struct PendingImage {
    /// Image ID (used by the Kitty protocol).
    pub id: u32,
    /// Placement column (character cells).
    pub col: u16,
    /// Placement row (character cells).
    pub row: u16,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// RGBA pixel data (`width × height × 4` bytes).
    pub rgba: Vec<u8>,
}

/// Screen-buffer contents (shared between the primary and alternate screens).
struct ScreenBuffer {
    rows: Vec<Vec<Cell>>,
    cursor_col: u16,
    cursor_row: u16,
}

/// Virtual screen (internal state reflecting the PTY output).
pub struct Screen {
    /// Cell array and dimensions.
    grid: Grid,
    /// Per-row dirty flags (`true` = changed).
    dirty: Vec<bool>,
    /// Cursor column (0-based).
    cursor_col: u16,
    /// Cursor row (0-based).
    cursor_row: u16,
    /// Current foreground color.
    current_fg: Color,
    /// Current background color.
    current_bg: Color,
    /// Current character attributes.
    current_attrs: Attrs,
    /// Top row of the scrolling region (0-based).
    scroll_top: u16,
    /// Bottom row of the scrolling region (0-based).
    scroll_bottom: u16,
    // ---- DCS / Sixel reception state ----
    /// Whether we are receiving a Sixel DCS.
    dcs_sixel_active: bool,
    /// DCS data buffer.
    dcs_buf: Vec<u8>,
    /// Cursor position when the Sixel started.
    dcs_cursor: (u16, u16),
    /// Queue of completed images (drained via `take_pending_images`).
    pending_images: Vec<PendingImage>,
    /// Next image ID.
    next_image_id: u32,
    /// Accumulator for Kitty multi-chunk transfers (`m=1` chunks).
    kitty_chunk_payload: Vec<u8>,
    /// Parameters of the first Kitty chunk (saved at `m=1`).
    kitty_chunk_params: Option<Vec<u8>>,
    /// BEL pending flag (drained via `take_pending_bell`).
    pending_bell: bool,
    /// Title change pending (set by OSC 0/1/2).
    pending_title: Option<String>,
    /// Desktop notification pending (set by OSC 9).
    pending_notification: Option<(String, String)>,
    /// CWD change pending (set by OSC 7, with `file://` stripped and
    /// percent-decoded).
    pending_cwd: Option<String>,
    /// Alternate screen buffer (`None` = primary mode).
    alt_screen: Option<Box<ScreenBuffer>>,
    /// Whether the alternate screen is active.
    pub alt_mode: bool,
    /// Whether bracketed paste mode (DEC ?2004) is enabled.
    bracketed_paste: bool,
    /// Whether synchronized output mode (DEC ?2026) is enabled (holds back
    /// dirty flags while active).
    synchronized_output: bool,
    /// Mouse reporting mode (X11 ?1000 = 1, SGR ?1006 = 2, 0 = off).
    pub mouse_mode: u8,
    /// OSC 133 semantic-zone marks (row + kind).
    pub semantic_marks: Vec<SemanticMark>,
    /// Currently active OSC 8 hyperlink URL (`None` = no link).
    current_hyperlink_url: Option<String>,
    /// Start column of the OSC 8 hyperlink (valid when
    /// `current_hyperlink_url` is `Some`).
    hyperlink_start_col: u16,
    /// Start row of the OSC 8 hyperlink.
    hyperlink_start_row: u16,
    /// Queue of OSC 52 clipboard-write requests (Sprint 4-1).
    /// Processed on the client side according to
    /// `SecurityConfig.osc52_clipboard`.
    pending_clipboard_writes: Vec<String>,
    /// Kitty keyboard protocol progressive-enhancement flags (bitmask).
    /// Set by `CSI > flags u` (push) and restored by `CSI < n u` (pop).
    keyboard_protocol_flags: u8,
    /// Stack for the Kitty keyboard protocol push/pop mechanism.
    keyboard_protocol_stack: Vec<u8>,
}

impl Screen {
    /// Creates a screen with the given dimensions.
    pub fn new(cols: u16, rows: u16) -> Self {
        let scroll_bottom = rows.saturating_sub(1);
        Self {
            grid: Grid::new(cols, rows),
            dirty: vec![false; rows as usize],
            cursor_col: 0,
            cursor_row: 0,
            current_fg: Color::Default,
            current_bg: Color::Default,
            current_attrs: Attrs::default(),
            scroll_top: 0,
            scroll_bottom,
            dcs_sixel_active: false,
            dcs_buf: Vec::new(),
            dcs_cursor: (0, 0),
            pending_images: Vec::new(),
            next_image_id: 1,
            kitty_chunk_payload: Vec::new(),
            kitty_chunk_params: None,
            pending_bell: false,
            pending_title: None,
            pending_notification: None,
            pending_cwd: None,
            alt_screen: None,
            alt_mode: false,
            bracketed_paste: false,
            synchronized_output: false,
            mouse_mode: 0,
            semantic_marks: Vec::new(),
            current_hyperlink_url: None,
            hyperlink_start_col: 0,
            hyperlink_start_row: 0,
            pending_clipboard_writes: Vec::new(),
            keyboard_protocol_flags: 0,
            keyboard_protocol_stack: Vec::new(),
        }
    }

    /// Switches to the alternate screen buffer (SMCUP / DEC Private Mode
    /// 47/1047/1049).
    pub(crate) fn switch_to_alt(&mut self) {
        if self.alt_mode {
            return;
        }
        // Save the current contents of the primary screen.
        let saved = ScreenBuffer {
            rows: self.grid.rows.clone(),
            cursor_col: self.cursor_col,
            cursor_row: self.cursor_row,
        };
        self.alt_screen = Some(Box::new(saved));
        // Initialize the alternate buffer with blanks.
        let cols = self.grid.width;
        let rows = self.grid.height;
        self.grid = Grid::new(cols, rows);
        self.dirty = vec![true; rows as usize];
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.scroll_top = 0;
        self.scroll_bottom = rows.saturating_sub(1);
        self.alt_mode = true;
    }

    /// Returns to the primary screen buffer (RMCUP / resets DEC Private Mode
    /// 47/1047/1049).
    pub(crate) fn switch_to_primary(&mut self) {
        if !self.alt_mode {
            return;
        }
        if let Some(saved) = self.alt_screen.take() {
            self.grid.rows = saved.rows;
            self.cursor_col = saved.cursor_col;
            self.cursor_row = saved.cursor_row;
            self.dirty = vec![true; self.grid.height as usize];
        }
        self.scroll_top = 0;
        self.scroll_bottom = self.grid.height.saturating_sub(1);
        self.alt_mode = false;
    }

    /// Returns a reference to the grid.
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Returns the cursor position as `(col, row)`.
    pub fn cursor(&self) -> (u16, u16) {
        (self.cursor_col, self.cursor_row)
    }

    /// Returns whether the given row is dirty.
    pub fn is_dirty(&self, row: u16) -> bool {
        self.dirty.get(row as usize).copied().unwrap_or(false)
    }

    /// Clears every dirty flag.
    pub fn clear_dirty(&mut self) {
        self.dirty.fill(false);
    }

    /// Collects and returns only the dirty rows (used for differential transfer).
    ///
    /// Returns empty while synchronized output mode (DEC ?2026) is enabled and
    /// flushes the accumulated rows once the mode is disabled.
    pub fn take_dirty_rows(&mut self) -> Vec<DirtyRow> {
        // Hold off rendering while synchronized output is active.
        if self.synchronized_output {
            return Vec::new();
        }
        let mut result = Vec::new();
        for (row_idx, dirty) in self.dirty.iter_mut().enumerate() {
            if *dirty {
                let cells = self.grid.rows[row_idx].clone();
                result.push(DirtyRow {
                    row: row_idx as u16,
                    cells,
                });
                *dirty = false;
            }
        }
        result
    }

    /// Returns the synchronized-output (DEC ?2026) state.
    pub fn synchronized_output_mode(&self) -> bool {
        self.synchronized_output
    }

    /// Sets the synchronized-output state (called from the performer).
    pub(crate) fn set_synchronized_output(&mut self, enabled: bool) {
        self.synchronized_output = enabled;
    }

    /// Full-screen snapshot (used for a full refresh).
    pub fn full_refresh_grid(&self) -> Grid {
        let mut g = self.grid.clone();
        g.cursor_col = self.cursor_col;
        g.cursor_row = self.cursor_row;
        g
    }

    /// Handles a resize (content is copied as much as the new size allows).
    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        let mut new_grid = Grid::new(new_cols, new_rows);
        let copy_rows = self.grid.height.min(new_rows) as usize;
        let copy_cols = self.grid.width.min(new_cols) as usize;
        for r in 0..copy_rows {
            for c in 0..copy_cols {
                new_grid.rows[r][c] = self.grid.rows[r][c].clone();
            }
        }
        self.grid = new_grid;
        self.dirty = vec![true; new_rows as usize]; // Every row is dirty after a resize.
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);
    }

    /// Writes a character at the cursor and advances it.
    pub(crate) fn write_char(&mut self, ch: char) {
        // Determine the character's display width (CJK fullwidth = 2, others = 1).
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(1) as u16;

        if self.cursor_col >= self.grid.width {
            // Wrap at the end of the line.
            self.cursor_col = 0;
            self.advance_line();
        }

        // If a wide character would overflow the line, wrap to the next row.
        if char_width == 2 && self.cursor_col + 1 >= self.grid.width {
            self.cursor_col = 0;
            self.advance_line();
        }

        let cell = Cell {
            ch,
            fg: self.current_fg,
            bg: self.current_bg,
            attrs: self.current_attrs,
        };
        let col = self.cursor_col;
        let row = self.cursor_row;
        self.grid.set(col, row, cell);
        self.mark_dirty(row);
        self.cursor_col += 1;

        // For wide characters, drop a placeholder cell into the next column.
        if char_width == 2 && self.cursor_col < self.grid.width {
            let placeholder = Cell {
                ch: ' ',
                fg: self.current_fg,
                bg: self.current_bg,
                attrs: self.current_attrs,
            };
            self.grid.set(self.cursor_col, row, placeholder);
            self.cursor_col += 1;
        }
    }

    /// Advances the cursor to the next row, scrolling if necessary.
    pub(crate) fn advance_line(&mut self) {
        if self.cursor_row >= self.scroll_bottom {
            self.scroll_up();
        } else {
            self.cursor_row += 1;
        }
    }

    /// Scrolls the region up by one row.
    fn scroll_up(&mut self) {
        let top = self.scroll_top as usize;
        let bottom = self.scroll_bottom as usize;
        // Copy each row in the region up by one (avoiding direct index access
        // to prevent panics).
        for r in top..bottom {
            self.grid.copy_row(r as u16, (r + 1) as u16);
            self.mark_dirty(r as u16);
        }
        // Clear the bottom row.
        self.grid.clear_row(bottom as u16);
        self.mark_dirty(bottom as u16);
    }

    /// Marks the given row as dirty.
    fn mark_dirty(&mut self, row: u16) {
        if let Some(d) = self.dirty.get_mut(row as usize) {
            *d = true;
        }
    }

    /// Applies an SGR (Select Graphic Rendition) attribute list.
    pub(crate) fn apply_sgr(&mut self, params: &[u16]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => {
                    // Reset.
                    self.current_fg = Color::Default;
                    self.current_bg = Color::Default;
                    self.current_attrs = Attrs::default();
                }
                1 => self.current_attrs.0 |= Attrs::BOLD,
                3 => self.current_attrs.0 |= Attrs::ITALIC,
                4 => self.current_attrs.0 |= Attrs::UNDERLINE,
                5 => self.current_attrs.0 |= Attrs::BLINK,
                7 => self.current_attrs.0 |= Attrs::REVERSE,
                9 => self.current_attrs.0 |= Attrs::STRIKETHROUGH,
                22 => self.current_attrs.0 &= !Attrs::BOLD,
                24 => self.current_attrs.0 &= !Attrs::UNDERLINE,
                27 => self.current_attrs.0 &= !Attrs::REVERSE,
                // Foreground: 30..=37 (ANSI), 38 (extended), 39 (default).
                30..=37 => self.current_fg = Color::Indexed(params[i] as u8 - 30),
                38 => {
                    if params.get(i + 1) == Some(&5) && params.get(i + 2).is_some() {
                        self.current_fg = Color::Indexed(params[i + 2] as u8);
                        i += 2;
                    } else if params.get(i + 1) == Some(&2) && i + 4 < params.len() {
                        self.current_fg = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                39 => self.current_fg = Color::Default,
                // Background: 40..=47 (ANSI), 48 (extended), 49 (default).
                40..=47 => self.current_bg = Color::Indexed(params[i] as u8 - 40),
                48 => {
                    if params.get(i + 1) == Some(&5) && params.get(i + 2).is_some() {
                        self.current_bg = Color::Indexed(params[i + 2] as u8);
                        i += 2;
                    } else if params.get(i + 1) == Some(&2) && i + 4 < params.len() {
                        self.current_bg = Color::Rgb(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                49 => self.current_bg = Color::Default,
                // Bright foreground: 90..=97.
                90..=97 => self.current_fg = Color::Indexed(params[i] as u8 - 90 + 8),
                // Bright background: 100..=107.
                100..=107 => self.current_bg = Color::Indexed(params[i] as u8 - 100 + 8),
                _ => {} // Ignore unsupported attributes.
            }
            i += 1;
        }
    }

    /// Moves the cursor to the given position (0-based coordinates).
    pub(crate) fn move_cursor(&mut self, col: u16, row: u16) {
        self.cursor_col = col.min(self.grid.width.saturating_sub(1));
        self.cursor_row = row.min(self.grid.height.saturating_sub(1));
    }

    /// Sets the scrolling region (DECSTBM).
    pub(crate) fn set_scroll_region(&mut self, top: u16, bottom: u16) {
        let max = self.grid.height.saturating_sub(1);
        self.scroll_top = top.min(max);
        self.scroll_bottom = bottom.min(max);
        // DECSTBM moves the cursor to the home position.
        self.cursor_col = 0;
        self.cursor_row = 0;
    }

    /// Erases part or all of the current row.
    pub(crate) fn erase_in_line(&mut self, mode: u16) {
        let row = self.cursor_row;
        let width = self.grid.width as usize;
        match mode {
            0 => {
                // Clear from the cursor to the end of the line (Grid::set bounds-checks).
                for c in self.cursor_col as usize..width {
                    self.grid.set(c as u16, row, Cell::default());
                }
            }
            1 => {
                // Clear from the start of the line to the cursor (Grid::set bounds-checks).
                for c in 0..=self.cursor_col as usize {
                    self.grid.set(c as u16, row, Cell::default());
                }
            }
            2 => {
                // Clear the entire row (Grid::clear_row bounds-checks).
                self.grid.clear_row(row);
            }
            _ => {}
        }
        self.mark_dirty(row);
    }

    /// Erases part or all of the display.
    pub(crate) fn erase_in_display(&mut self, mode: u16) {
        let height = self.grid.height as usize;
        match mode {
            0 => {
                // Clear from the cursor to the end of the display
                // (Grid::clear_row bounds-checks).
                self.erase_in_line(0);
                for r in (self.cursor_row as usize + 1)..height {
                    self.grid.clear_row(r as u16);
                    self.mark_dirty(r as u16);
                }
            }
            1 => {
                // Clear from the top of the display to the cursor
                // (Grid::clear_row bounds-checks).
                for r in 0..self.cursor_row as usize {
                    self.grid.clear_row(r as u16);
                    self.mark_dirty(r as u16);
                }
                self.erase_in_line(1);
            }
            2 | 3 => {
                // Clear the entire display (Grid::clear_row bounds-checks).
                for r in 0..height {
                    self.grid.clear_row(r as u16);
                    self.mark_dirty(r as u16);
                }
                if mode == 2 {
                    self.cursor_col = 0;
                    self.cursor_row = 0;
                }
            }
            _ => {}
        }
    }

    // ---- DCS / Sixel / Kitty image processing ----

    /// Begins receiving a Sixel DCS (called from `hook`).
    pub(crate) fn start_sixel(&mut self) {
        self.dcs_sixel_active = true;
        self.dcs_buf.clear();
        self.dcs_cursor = (self.cursor_col, self.cursor_row);
    }

    /// Appends a DCS byte to the buffer (called from `put`).
    ///
    /// On overflow, the buffer is cleared and the DCS state is dropped so normal
    /// parsing can resume (CRITICAL #7: defense against a malicious PTY that
    /// streams an unterminated DCS forever).
    pub(crate) fn push_dcs_byte(&mut self, byte: u8) {
        if !self.dcs_sixel_active {
            return;
        }
        if self.dcs_buf.len() >= MAX_DCS_BUF_LEN {
            tracing::warn!(
                "DCS Sixel buffer exceeded the limit ({} bytes); discarding the sequence.",
                MAX_DCS_BUF_LEN
            );
            self.dcs_buf.clear();
            self.dcs_sixel_active = false;
            return;
        }
        self.dcs_buf.push(byte);
    }

    /// Finishes the Sixel DCS, decodes it, and pushes onto `pending_images`
    /// (called from `unhook`).
    pub(crate) fn finish_sixel(&mut self) {
        if !self.dcs_sixel_active {
            return;
        }
        self.dcs_sixel_active = false;
        if let Some(img) = decode_sixel(&self.dcs_buf) {
            let id = self.next_image_id;
            self.next_image_id += 1;
            self.pending_images.push(PendingImage {
                id,
                col: self.dcs_cursor.0,
                row: self.dcs_cursor.1,
                width: img.width,
                height: img.height,
                rgba: img.rgba,
            });
        }
        self.dcs_buf.clear();
    }

    /// Processes a Kitty APC image sequence and pushes the result onto
    /// `pending_images`.
    ///
    /// Supports the Kitty graphics protocol's chunked transfer (`m=1` / `m=0`).
    /// `data` is the APC content with ESC `_` and ESC `\` stripped (so the
    /// first byte is `'G'`).
    pub(crate) fn handle_kitty_apc(&mut self, data: &[u8]) {
        if data.first() != Some(&b'G') {
            return;
        }
        let inner = &data[1..];
        let sep = inner.iter().position(|&b| b == b';').unwrap_or(inner.len());
        let params_bytes = &inner[..sep];
        let payload = if sep < inner.len() {
            &inner[sep + 1..]
        } else {
            &[] as &[u8]
        };

        // Check the `m=1` flag (more_data: more chunks to come).
        let more_data = params_bytes.split(|&b| b == b',').any(|p| p == b"m=1");

        if more_data {
            // In the middle of a chunked transfer: save the first chunk's
            // parameters and accumulate the payload.
            if self.kitty_chunk_params.is_none() {
                self.kitty_chunk_params = Some(params_bytes.to_vec());
            }
            // Bounds check: discard once the accumulated payload exceeds
            // MAX_KITTY_CHUNK_LEN.
            if self.kitty_chunk_payload.len() + payload.len() > MAX_KITTY_CHUNK_LEN {
                tracing::warn!(
                    "Kitty chunked transfer payload exceeded the limit ({} bytes); discarding the sequence.",
                    MAX_KITTY_CHUNK_LEN
                );
                self.kitty_chunk_payload.clear();
                self.kitty_chunk_params = None;
                return;
            }
            self.kitty_chunk_payload.extend_from_slice(payload);
        } else {
            // Final chunk (or a single chunk): decode and register the image.
            let (decode_params, full_payload) =
                if let Some(first_params) = self.kitty_chunk_params.take() {
                    // Final chunk of a chunked transfer — combine with the accumulator.
                    self.kitty_chunk_payload.extend_from_slice(payload);
                    let combined_payload = std::mem::take(&mut self.kitty_chunk_payload);
                    (first_params, combined_payload)
                } else {
                    // Single chunk.
                    (params_bytes.to_vec(), payload.to_vec())
                };

            // Assemble the form expected by `decode_kitty`: `G<params>;<payload>`.
            let mut full_apc = Vec::with_capacity(decode_params.len() + full_payload.len() + 2);
            full_apc.push(b'G');
            full_apc.extend_from_slice(&decode_params);
            full_apc.push(b';');
            full_apc.extend_from_slice(&full_payload);

            if let Some(img) = decode_kitty(&full_apc) {
                let id = self.next_image_id;
                self.next_image_id += 1;
                self.pending_images.push(PendingImage {
                    id,
                    col: self.cursor_col,
                    row: self.cursor_row,
                    width: img.width,
                    height: img.height,
                    rgba: img.rgba,
                });
            }
        }
    }

    /// Handles an iTerm2 inline image sequence (OSC 1337).
    ///
    /// Format: `ESC ] 1337 ; File=[key=value;...] : [base64-data] BEL/ST`
    ///
    /// The vte crate splits the OSC payload on each `;`, so the file arguments
    /// and base64 data arrive spread across `params[1..]`. This method
    /// reconstructs the full string, locates the `:` separator, and parses the
    /// key-value arguments. Images are only placed when `inline=1` is present.
    ///
    /// Payloads larger than 16 MiB (base64) or images larger than 256 MiB
    /// (decoded RGBA) are silently discarded.
    pub(crate) fn handle_iterm2_osc(&mut self, params: &[&[u8]]) {
        // Reconstruct the full payload string; vte splits on every ';'.
        let mut full: Vec<u8> = Vec::new();
        for (i, p) in params[1..].iter().enumerate() {
            if i > 0 {
                full.push(b';');
            }
            full.extend_from_slice(p);
        }

        // Locate the ':' that separates the File arguments from the base64 data.
        let colon_pos = match full.iter().position(|&b| b == b':') {
            Some(p) => p,
            None => return,
        };
        let args_bytes = &full[..colon_pos];
        let data_bytes = full[colon_pos + 1..].trim_ascii();

        // Cap the raw base64 payload as a DoS defence (≈12 MiB decoded).
        const MAX_ITERM2_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
        if data_bytes.len() > MAX_ITERM2_PAYLOAD_BYTES {
            return;
        }

        // Parse key=value pairs.  Strip the leading "File=" prefix first, then
        // scan the semicolon-delimited items from all reconstructed fragments.
        let args_str = std::str::from_utf8(args_bytes).unwrap_or("").trim();
        let args = args_str.trim_start_matches("File=");
        let mut inline = false;
        for kv in args.split(';') {
            if kv.trim().eq_ignore_ascii_case("inline=1") {
                inline = true;
            }
        }
        // Sequences without inline=1 must not display (spec requirement).
        if !inline {
            return;
        }

        // Base64-decode the image data.
        let raw = match crate::image::base64_decode(data_bytes) {
            Some(r) => r,
            None => return,
        };

        // Decode to RGBA via the image crate (PNG, JPEG, …).
        if let Some(img) = crate::image::decode_iterm2(&raw) {
            let id = self.next_image_id;
            self.next_image_id += 1;
            self.pending_images.push(PendingImage {
                id,
                col: self.cursor_col,
                row: self.cursor_row,
                width: img.width,
                height: img.height,
                rgba: img.rgba,
            });
        }
    }

    /// Drains the accumulated images and clears the queue.
    pub fn take_pending_images(&mut self) -> Vec<PendingImage> {
        std::mem::take(&mut self.pending_images)
    }

    /// Drains the BEL flag and clears it.
    pub fn take_pending_bell(&mut self) -> bool {
        std::mem::replace(&mut self.pending_bell, false)
    }

    /// Sets the BEL flag (called from the performer).
    pub(crate) fn set_pending_bell(&mut self) {
        self.pending_bell = true;
    }

    /// Sets a pending title change (called from the performer).
    ///
    /// OSC 0/1/2 strings are sanitized (control characters stripped, length
    /// capped) before being stored (CRITICAL #13).
    pub(crate) fn set_pending_title(&mut self, title: String) {
        self.pending_title = Some(sanitize_osc_string(title, MAX_TITLE_LEN));
    }

    /// Drains the pending title and clears it.
    pub fn take_pending_title(&mut self) -> Option<String> {
        self.pending_title.take()
    }

    /// Sets a pending desktop notification (called from the performer).
    ///
    /// OSC 9 strings are sanitized (control characters stripped, length
    /// capped) (CRITICAL #13).
    pub(crate) fn set_pending_notification(&mut self, title: String, body: String) {
        self.pending_notification = Some((
            sanitize_osc_string(title, MAX_NOTIFICATION_LEN),
            sanitize_osc_string(body, MAX_NOTIFICATION_LEN),
        ));
    }

    /// Drains the pending desktop notification and clears it.
    pub fn take_pending_notification(&mut self) -> Option<(String, String)> {
        self.pending_notification.take()
    }

    /// Sets a pending CWD change (called by the performer on OSC 7).
    ///
    /// Expects `path` to already be `file://`-stripped and percent-decoded by
    /// `parse_osc7_cwd`. An additional sanitization step (control characters
    /// stripped, length capped) is applied here.
    pub(crate) fn set_pending_cwd(&mut self, path: String) {
        self.pending_cwd = Some(sanitize_osc_string(path, MAX_CWD_LEN));
    }

    /// Drains the pending CWD change and clears it.
    pub fn take_pending_cwd(&mut self) -> Option<String> {
        self.pending_cwd.take()
    }

    /// Queues an OSC 52 clipboard-write request (Sprint 4-1).
    ///
    /// Multiple OSC 52 requests may arrive in a single flush, so they are
    /// accumulated in a `Vec`. Control characters are stripped, and the length
    /// is capped at 1024× MAX_NOTIFICATION_LEN (≈ 1 MiB). The actual cap is
    /// re-checked on the client side via `SecurityConfig.osc52_max_bytes`.
    pub(crate) fn queue_clipboard_write(&mut self, text: String) {
        const MAX_CLIPBOARD_LEN: usize = MAX_NOTIFICATION_LEN * 1024; // ≈ 1 MiB
        let cleaned = sanitize_osc_string(text, MAX_CLIPBOARD_LEN);
        self.pending_clipboard_writes.push(cleaned);
    }

    /// Drains the pending OSC 52 clipboard-write requests and clears the queue.
    pub fn take_pending_clipboard_writes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_clipboard_writes)
    }

    /// Returns the bracketed paste (DEC ?2004) state.
    pub fn bracketed_paste_mode(&self) -> bool {
        self.bracketed_paste
    }

    /// Sets the bracketed paste state (called from the performer).
    pub(crate) fn set_bracketed_paste(&mut self, enabled: bool) {
        self.bracketed_paste = enabled;
    }

    /// Returns the active Kitty keyboard protocol progressive-enhancement flags.
    pub fn keyboard_protocol_flags(&self) -> u8 {
        self.keyboard_protocol_flags
    }

    /// Pushes the current flags onto the stack and activates `flags` (CSI > flags u).
    pub(crate) fn push_keyboard_protocol_flags(&mut self, flags: u8) {
        self.keyboard_protocol_stack.push(self.keyboard_protocol_flags);
        self.keyboard_protocol_flags = flags;
    }

    /// Pops `n` levels from the stack, restoring the previous flags (CSI < n u).
    pub(crate) fn pop_keyboard_protocol_flags(&mut self, n: usize) {
        for _ in 0..n {
            if let Some(prev) = self.keyboard_protocol_stack.pop() {
                self.keyboard_protocol_flags = prev;
            } else {
                self.keyboard_protocol_flags = 0;
                break;
            }
        }
    }

    /// Records an OSC 133 semantic-zone mark.
    pub(crate) fn add_semantic_mark(&mut self, kind: SemanticMarkKind, exit_code: Option<i32>) {
        self.semantic_marks.push(SemanticMark {
            row: self.cursor_row,
            kind,
            exit_code,
        });
    }

    /// Drains the accumulated semantic marks and clears the buffer.
    pub fn take_semantic_marks(&mut self) -> Vec<SemanticMark> {
        std::mem::take(&mut self.semantic_marks)
    }

    /// Handles the start (`url` is `Some`) or end (`None`) of an OSC 8 hyperlink.
    ///
    /// Disabling URIs whose scheme is not in the allow list (e.g. `javascript:`
    /// or `file:`) (CRITICAL #13: defends against clickjacking from a malicious
    /// SSH peer).
    pub(crate) fn set_hyperlink(&mut self, url: Option<String>) {
        if let Some(active_url) = self.current_hyperlink_url.take() {
            // Finalize the existing link and push it onto `grid.hyperlinks`.
            let col_end = self.cursor_col;
            let row = self.hyperlink_start_row;
            if self.hyperlink_start_row == row && col_end > self.hyperlink_start_col {
                use nexterm_proto::HyperlinkSpan;
                self.grid.hyperlinks.push(HyperlinkSpan {
                    row,
                    col_start: self.hyperlink_start_col,
                    col_end,
                    url: active_url,
                });
            }
        }
        if let Some(url) = url {
            // Validate the scheme and length — disable the link on failure
            // (treat as `None`).
            let validated = validate_hyperlink_uri(&url);
            if validated.is_none() {
                self.current_hyperlink_url = None;
                return;
            }
            // Record the start position of the new link.
            self.current_hyperlink_url = validated;
            self.hyperlink_start_col = self.cursor_col;
            self.hyperlink_start_row = self.cursor_row;
        }
    }
}

#[cfg(test)]
mod osc_security_tests {
    use super::*;

    #[test]
    fn control_characters_are_stripped_from_osc_strings() {
        let input = "Title\x00\x01\x07\x1bWith Control".to_string();
        let cleaned = sanitize_osc_string(input, MAX_TITLE_LEN);
        assert!(!cleaned.contains('\x00'));
        assert!(!cleaned.contains('\x01'));
        assert!(!cleaned.contains('\x07'));
        assert!(!cleaned.contains('\x1b'));
        assert_eq!(cleaned, "TitleWith Control");
    }

    #[test]
    fn osc_strings_are_truncated_at_max_len() {
        let input = "a".repeat(1000);
        let cleaned = sanitize_osc_string(input, 100);
        assert_eq!(cleaned.len(), 100);
    }

    #[test]
    fn cjk_characters_are_truncated_on_utf8_boundaries() {
        let input = "あいうえお".repeat(100);
        let cleaned = sanitize_osc_string(input, 10);
        assert!(cleaned.len() <= 10);
        // Still valid UTF-8.
        assert!(cleaned.chars().count() > 0);
    }

    #[test]
    fn osc_strings_preserve_tab_lf_cr() {
        let input = "Hello\tWorld\nNext\rLine".to_string();
        let cleaned = sanitize_osc_string(input, 100);
        assert_eq!(cleaned, "Hello\tWorld\nNext\rLine");
    }

    #[test]
    fn http_and_https_uris_are_allowed() {
        assert!(validate_hyperlink_uri("https://example.com").is_some());
        assert!(validate_hyperlink_uri("http://example.com/path").is_some());
        assert!(validate_hyperlink_uri("HTTPS://EXAMPLE.COM").is_some());
    }

    #[test]
    fn javascript_uris_are_rejected() {
        // CRITICAL #13: core test for the clickjacking defense.
        assert!(validate_hyperlink_uri("javascript:alert(1)").is_none());
        assert!(validate_hyperlink_uri("JavaScript:void(0)").is_none());
    }

    #[test]
    fn file_uris_are_rejected() {
        assert!(validate_hyperlink_uri("file:///etc/passwd").is_none());
        assert!(validate_hyperlink_uri("FILE:///c:/windows/system32").is_none());
    }

    #[test]
    fn data_uris_are_rejected() {
        assert!(validate_hyperlink_uri("data:text/html,<script>alert(1)</script>").is_none());
    }

    #[test]
    fn excessively_long_uris_are_rejected() {
        let long = "https://".to_string() + &"a".repeat(MAX_HYPERLINK_URI_LEN);
        assert!(validate_hyperlink_uri(&long).is_none());
    }

    #[test]
    fn control_characters_inside_a_uri_are_stripped() {
        // Tabs and newlines inside a URI are also stripped (strict policy).
        let result = validate_hyperlink_uri("https://example.com/\x00path").unwrap();
        assert!(!result.contains('\x00'));
    }

    #[test]
    fn mailto_ssh_and_ftp_uris_are_allowed() {
        assert!(validate_hyperlink_uri("mailto:user@example.com").is_some());
        assert!(validate_hyperlink_uri("ssh://server.example.com").is_some());
        assert!(validate_hyperlink_uri("ftp://files.example.com").is_some());
    }

    #[test]
    fn empty_string_uri_is_rejected() {
        assert!(validate_hyperlink_uri("").is_none());
    }

    // ---- Tests for OSC 7 (CWD reporting), Sprint 5-2 / B2 ----

    #[test]
    fn osc_7_extracts_a_path_from_a_file_uri() {
        assert_eq!(
            parse_osc7_cwd("file:///home/user/projects"),
            Some("/home/user/projects".to_string())
        );
    }

    #[test]
    fn osc_7_ignores_the_host_segment() {
        // `file://hostname/path` form — the host is ignored; only the path is used.
        assert_eq!(
            parse_osc7_cwd("file://example.host/home/user"),
            Some("/home/user".to_string())
        );
    }

    #[test]
    fn osc_7_passes_through_paths_without_a_scheme() {
        assert_eq!(
            parse_osc7_cwd("/home/user/proj"),
            Some("/home/user/proj".to_string())
        );
    }

    #[test]
    fn osc_7_decodes_percent_encoding() {
        // `" "` (0x20) → `%20`.
        assert_eq!(
            parse_osc7_cwd("file:///home/user/dir%20with%20space"),
            Some("/home/user/dir with space".to_string())
        );
        // Japanese path (UTF-8: あ = E3 81 82).
        assert_eq!(parse_osc7_cwd("file:///%E3%81%82"), Some("/あ".to_string()));
    }

    #[test]
    fn osc_7_passes_malformed_percent_xx_through() {
        // `%ZZ` is not hex, so it is left in place rather than converted.
        assert_eq!(
            parse_osc7_cwd("file:///path/%ZZ/foo"),
            Some("/path/%ZZ/foo".to_string())
        );
    }

    #[test]
    fn osc_7_returns_none_for_empty_input() {
        assert!(parse_osc7_cwd("").is_none());
    }

    #[test]
    fn osc_7_returns_none_for_the_file_scheme_with_no_path() {
        assert!(parse_osc7_cwd("file://hostname").is_none());
    }

    #[test]
    fn osc_7_strips_control_characters() {
        let result = parse_osc7_cwd("file:///home/\x00user\x07/dir").unwrap();
        assert!(!result.contains('\x00'));
        assert!(!result.contains('\x07'));
        assert_eq!(result, "/home/user/dir");
    }

    #[test]
    fn osc_7_rejects_very_long_input_early() {
        // Inputs longer than `MAX_CWD_LEN * 4` are rejected up front for DoS reasons.
        let huge_path = format!("file:///{}", "a".repeat(MAX_CWD_LEN * 5));
        assert!(parse_osc7_cwd(&huge_path).is_none());
    }

    #[test]
    fn osc_7_truncates_results_at_max_cwd_len() {
        // For inputs within the early-rejection range, verify the result fits into MAX_CWD_LEN.
        let near_limit = format!("file:///{}", "a".repeat(MAX_CWD_LEN + 100));
        let result = parse_osc7_cwd(&near_limit)
            .expect("inputs below the early-rejection cap should return Some");
        assert!(
            result.len() <= MAX_CWD_LEN,
            "result must be truncated to MAX_CWD_LEN. actual: {}",
            result.len()
        );
    }

    #[test]
    fn osc_7_value_can_be_retrieved_via_screen_pending_cwd() {
        let mut screen = Screen::new(80, 24);
        screen.set_pending_cwd("/home/user/test".to_string());
        assert_eq!(
            screen.take_pending_cwd(),
            Some("/home/user/test".to_string())
        );
        // After `take`, the value is cleared.
        assert!(screen.take_pending_cwd().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn osc_7_strips_the_leading_slash_on_windows_paths() {
        // file:///C:/Users/foo → C:/Users/foo
        assert_eq!(
            parse_osc7_cwd("file:///C:/Users/foo"),
            Some("C:/Users/foo".to_string())
        );
    }
}

#[cfg(test)]
mod kitty_keyboard_protocol_tests {
    use super::*;

    fn make_screen() -> Screen {
        Screen::new(80, 24)
    }

    #[test]
    fn initial_flags_are_zero() {
        let s = make_screen();
        assert_eq!(s.keyboard_protocol_flags(), 0);
    }

    #[test]
    fn push_sets_new_flags() {
        let mut s = make_screen();
        s.push_keyboard_protocol_flags(0x01);
        assert_eq!(s.keyboard_protocol_flags(), 0x01);
    }

    #[test]
    fn push_saves_previous_flags_on_stack() {
        let mut s = make_screen();
        s.push_keyboard_protocol_flags(0x01);
        s.push_keyboard_protocol_flags(0x03);
        assert_eq!(s.keyboard_protocol_flags(), 0x03);
        s.pop_keyboard_protocol_flags(1);
        assert_eq!(s.keyboard_protocol_flags(), 0x01);
    }

    #[test]
    fn pop_restores_to_zero_on_empty_stack() {
        let mut s = make_screen();
        s.pop_keyboard_protocol_flags(1); // no-op on empty stack
        assert_eq!(s.keyboard_protocol_flags(), 0);
    }

    #[test]
    fn pop_n_pops_multiple_levels() {
        let mut s = make_screen();
        s.push_keyboard_protocol_flags(0x01);
        s.push_keyboard_protocol_flags(0x02);
        s.push_keyboard_protocol_flags(0x04);
        s.pop_keyboard_protocol_flags(2);
        assert_eq!(s.keyboard_protocol_flags(), 0x01);
    }

    #[test]
    fn push_pop_round_trip() {
        let mut s = make_screen();
        assert_eq!(s.keyboard_protocol_flags(), 0);
        s.push_keyboard_protocol_flags(0x0f);
        assert_eq!(s.keyboard_protocol_flags(), 0x0f);
        s.pop_keyboard_protocol_flags(1);
        assert_eq!(s.keyboard_protocol_flags(), 0);
    }
}
