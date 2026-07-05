#![warn(missing_docs)]
//! nexterm-vt — VT sequence parser plus virtual-grid implementation.
//!
//! Uses the `vte` crate to parse terminal escape sequences and applies them to a
//! two-dimensional cell array (the virtual grid).

mod colors;
pub mod image;
mod performer;
mod screen;

pub use screen::{
    ColorOverrides, DndRequest, PendingImage, Screen, SemanticMark, SemanticMarkKind,
};

/// Maximum APC buffer size (used for Kitty graphics).
///
/// Mitigates the vulnerability where a malicious PTY / SSH host streams an
/// unterminated APC sequence forever and exhausts process memory (CRITICAL #7).
/// On overflow the buffer is cleared and the APC state is dropped.
///
/// 4 MiB accommodates the worst case of a typical Kitty image plus its
/// base64-encoded representation.
const MAX_APC_BUF_LEN: usize = 4 * 1024 * 1024;

/// Parser that processes VT sequences and updates the grid.
pub struct VtParser {
    parser: vte::Parser,
    screen: Screen,
    /// Whether we are currently receiving an APC sequence (Kitty graphics).
    apc_active: bool,
    /// Accumulator buffer for APC data.
    apc_buf: Vec<u8>,
    /// Whether the previous byte was ESC (0x1B).
    apc_pending_esc: bool,
    /// Whether we have already logged an APC overflow warning (avoids log spam).
    apc_overflow_warned: bool,
}

impl VtParser {
    /// Creates a parser with a virtual screen of the given size.
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            parser: vte::Parser::new(),
            screen: Screen::new(cols, rows),
            apc_active: false,
            apc_buf: Vec::new(),
            apc_pending_esc: false,
            apc_overflow_warned: false,
        }
    }

    /// Appends a byte to the APC buffer; on overflow the APC state is dropped.
    fn apc_push(&mut self, byte: u8) {
        if self.apc_buf.len() >= MAX_APC_BUF_LEN {
            if !self.apc_overflow_warned {
                tracing::warn!(
                    "APC buffer exceeded the limit ({} bytes); discarding the sequence.",
                    MAX_APC_BUF_LEN
                );
                self.apc_overflow_warned = true;
            }
            // Clear the buffer, end the APC state, and resume normal parsing.
            self.apc_buf.clear();
            self.apc_active = false;
            return;
        }
        self.apc_buf.push(byte);
    }

    /// Processes a byte stream and updates the grid.
    ///
    /// vte 0.13 does not provide an APC callback, so we intercept APC sequences
    /// (Kitty graphics) here and hand the payload to the screen ourselves. Every
    /// other byte is delegated to `vte`.
    pub fn advance(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            // Decide whether ESC starts or ends an APC by inspecting the next byte.
            if self.apc_pending_esc {
                self.apc_pending_esc = false;
                match byte {
                    b'_' => {
                        // ESC _ = APC start.
                        self.apc_active = true;
                        self.apc_buf.clear();
                        continue;
                    }
                    b'\\' if self.apc_active => {
                        // ESC \ = ST (String Terminator) = APC end.
                        let data = std::mem::take(&mut self.apc_buf);
                        self.screen.handle_kitty_apc(&data);
                        self.apc_active = false;
                        continue;
                    }
                    _ => {
                        // Any other ESC sequence — forward ESC + current byte to vte.
                        if self.apc_active {
                            // A stray ESC inside an APC is appended to the buffer
                            // (subject to the overflow check).
                            self.apc_push(0x1b);
                            self.apc_push(byte);
                        } else {
                            self.parser.advance(&mut self.screen, &[0x1b]);
                            self.parser.advance(&mut self.screen, &[byte]);
                        }
                        continue;
                    }
                }
            }

            if byte == 0x1b {
                // ESC: defer the decision until we see the next byte.
                self.apc_pending_esc = true;
                continue;
            }

            if self.apc_active {
                self.apc_push(byte);
            } else {
                self.parser.advance(&mut self.screen, &[byte]);
            }
        }
    }

    /// Returns a reference to the current screen state.
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// Returns a mutable reference to the current screen state.
    pub fn screen_mut(&mut self) -> &mut Screen {
        &mut self.screen
    }

    /// Returns whether bracketed paste mode (DEC ?2004) is enabled.
    pub fn bracketed_paste_mode(&self) -> bool {
        self.screen.bracketed_paste_mode()
    }

    /// Returns whether synchronized output mode (DEC ?2026) is enabled.
    pub fn synchronized_output_mode(&self) -> bool {
        self.screen.synchronized_output_mode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_regular_characters() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Hello");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'H');
        assert_eq!(grid.get(1, 0).unwrap().ch, 'e');
        assert_eq!(grid.get(4, 0).unwrap().ch, 'o');
    }

    #[test]
    fn carriage_return_and_newline_move_the_cursor() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Line1\r\nLine2");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'L');
        assert_eq!(grid.get(0, 1).unwrap().ch, 'L');
    }

    #[test]
    fn cursor_position_escape_works() {
        let mut parser = VtParser::new(80, 24);
        // CSI 5;10H → move to row 5, column 10 (1-based).
        parser.advance(b"\x1b[5;10HA");
        let grid = parser.screen().grid();
        // 'A' lands at row 4, column 9 (0-based).
        assert_eq!(grid.get(9, 4).unwrap().ch, 'A');
    }

    #[test]
    fn dirty_flag_is_raised_on_write() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"X");
        let screen = parser.screen();
        assert!(screen.is_dirty(0), "row 0 should be dirty");
        assert!(!screen.is_dirty(1), "row 1 should be clean");
    }

    #[test]
    fn dirty_flag_can_be_cleared() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"X");
        parser.screen_mut().clear_dirty();
        assert!(!parser.screen().is_dirty(0));
    }

    #[test]
    fn resize_updates_the_grid_dimensions() {
        let mut parser = VtParser::new(80, 24);
        parser.screen_mut().resize(120, 40);
        let grid = parser.screen().grid();
        assert_eq!(grid.width, 120);
        assert_eq!(grid.height, 40);
    }

    #[test]
    fn bracketed_paste_mode_is_disabled_by_default() {
        let parser = VtParser::new(80, 24);
        assert!(!parser.bracketed_paste_mode());
    }

    #[test]
    fn bracketed_paste_mode_can_be_enabled() {
        let mut parser = VtParser::new(80, 24);
        // CSI ?2004h — enable bracketed paste mode.
        parser.advance(b"\x1b[?2004h");
        assert!(
            parser.bracketed_paste_mode(),
            "?2004h should enable the mode"
        );
    }

    #[test]
    fn synchronized_output_mode_is_disabled_by_default() {
        let parser = VtParser::new(80, 24);
        assert!(!parser.synchronized_output_mode());
    }

    #[test]
    fn synchronized_output_mode_holds_back_dirty_rows() {
        let mut parser = VtParser::new(80, 24);
        // Enable the mode.
        parser.advance(b"\x1b[?2026h");
        assert!(parser.synchronized_output_mode());
        // Write some text.
        parser.advance(b"Hello");
        // Dirty rows should be empty (held back).
        let dirty = parser.screen_mut().take_dirty_rows();
        assert!(
            dirty.is_empty(),
            "dirty rows should not be returned while synchronized output is active"
        );
        // Disable the mode and flush.
        parser.advance(b"\x1b[?2026l");
        assert!(!parser.synchronized_output_mode());
        let dirty = parser.screen_mut().take_dirty_rows();
        assert!(
            !dirty.is_empty(),
            "dirty rows should be returned after the mode is disabled"
        );
    }

    // ---- Sprint 5-2 / B5: extra tests for synchronized output (DEC ?2026) ----

    #[test]
    fn synchronized_output_flushes_multiple_rows_as_one_batch() {
        // Typical scenario: a shell repaints its TUI for the entire screen.
        // Without synchronized output, partial paints would flicker.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[?2026h");
        // Draw across 5 rows.
        parser.advance(b"Line1\r\nLine2\r\nLine3\r\nLine4\r\nLine5");
        // While synchronized, every take_dirty_rows() returns empty.
        assert!(parser.screen_mut().take_dirty_rows().is_empty());
        assert!(parser.screen_mut().take_dirty_rows().is_empty());
        // Disabling the mode flushes everything in one shot.
        parser.advance(b"\x1b[?2026l");
        let dirty = parser.screen_mut().take_dirty_rows();
        assert!(
            dirty.len() >= 5,
            "disabling synchronized output should flush all 5 rows together. actual: {} rows",
            dirty.len()
        );
    }

    #[test]
    fn synchronized_output_repeated_h_is_idempotent() {
        // Enabling the mode twice must not corrupt internal state.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[?2026h");
        parser.advance(b"X");
        parser.advance(b"\x1b[?2026h"); // duplicate enable
        parser.advance(b"Y");
        assert!(parser.synchronized_output_mode());
        assert!(parser.screen_mut().take_dirty_rows().is_empty());
        parser.advance(b"\x1b[?2026l");
        assert!(!parser.synchronized_output_mode());
        // "XY" should be visible after the mode is disabled.
        assert!(!parser.screen_mut().take_dirty_rows().is_empty());
    }

    #[test]
    fn synchronized_output_l_while_disabled_is_a_noop() {
        // Spec: disabling while already disabled is a no-op.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[?2026l"); // 'l' while inactive
        assert!(!parser.synchronized_output_mode());
        // Regular writes keep working.
        parser.advance(b"Z");
        let dirty = parser.screen_mut().take_dirty_rows();
        assert!(
            !dirty.is_empty(),
            "dirty rows must be returned normally while inactive"
        );
    }

    #[test]
    fn synchronized_output_repeated_toggling_does_not_corrupt_buffers() {
        let mut parser = VtParser::new(80, 24);
        for _ in 0..10 {
            parser.advance(b"\x1b[?2026h");
            parser.advance(b"A");
            parser.advance(b"\x1b[?2026l");
            parser.advance(b"B");
        }
        // Final state: disabled.
        assert!(!parser.synchronized_output_mode());
        // Some dirty rows should still be readable (nothing is broken).
        let dirty = parser.screen_mut().take_dirty_rows();
        assert!(!dirty.is_empty());
    }

    #[test]
    fn synchronized_output_cells_are_updated_even_if_take_dirty_is_empty() {
        // Dirty rows are held back, but the grid itself is updated.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[?2026h");
        parser.advance(b"Hi");
        // No dirty rows are returned.
        assert!(parser.screen_mut().take_dirty_rows().is_empty());
        // But the grid cells are populated.
        assert_eq!(parser.screen().grid().get(0, 0).unwrap().ch, 'H');
        assert_eq!(parser.screen().grid().get(1, 0).unwrap().ch, 'i');
    }

    #[test]
    fn bracketed_paste_mode_can_be_disabled() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[?2004h");
        assert!(parser.bracketed_paste_mode());
        // CSI ?2004l — disable.
        parser.advance(b"\x1b[?2004l");
        assert!(
            !parser.bracketed_paste_mode(),
            "?2004l should disable the mode"
        );
    }

    #[test]
    fn osc_133_semantic_zones_are_recorded() {
        let mut parser = VtParser::new(80, 24);
        // A: PromptStart → B: CommandStart → C: OutputStart → D;0: CommandEnd.
        parser.advance(b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07");
        let marks = parser.screen_mut().take_semantic_marks();
        assert_eq!(marks.len(), 4, "all 4 marks should be recorded");
        assert!(matches!(marks[0].kind, SemanticMarkKind::PromptStart));
        assert!(matches!(marks[1].kind, SemanticMarkKind::CommandStart));
        assert!(matches!(marks[2].kind, SemanticMarkKind::OutputStart));
        assert!(matches!(marks[3].kind, SemanticMarkKind::CommandEnd));
        assert_eq!(marks[3].exit_code, Some(0));
    }

    #[test]
    fn osc_133_command_failure_records_exit_code() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]133;D;1\x07");
        let marks = parser.screen_mut().take_semantic_marks();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].exit_code, Some(1));
    }

    // ---- OSC 52 clipboard write (Sprint 4-1) ----

    #[test]
    fn osc_52_clipboard_write_request_is_queued() {
        let mut parser = VtParser::new(80, 24);
        // base64("Hello") = "SGVsbG8=" (padded).
        parser.advance(b"\x1b]52;c;SGVsbG8=\x07");
        let writes = parser.screen_mut().take_pending_clipboard_writes();
        assert_eq!(writes, vec!["Hello".to_string()]);
    }

    #[test]
    fn osc_52_read_request_is_ignored() {
        let mut parser = VtParser::new(80, 24);
        // "?" is a read request → rejected for security reasons.
        parser.advance(b"\x1b]52;c;?\x07");
        let writes = parser.screen_mut().take_pending_clipboard_writes();
        assert!(writes.is_empty(), "read requests must not be queued");
    }

    #[test]
    fn osc_52_primary_selection_is_ignored() {
        let mut parser = VtParser::new(80, 24);
        // Only "p" (primary selection) → out of scope.
        parser.advance(b"\x1b]52;p;SGVsbG8=\x07");
        let writes = parser.screen_mut().take_pending_clipboard_writes();
        assert!(writes.is_empty(), "primary selection is out of scope");
    }

    #[test]
    fn osc_52_multi_target_cs_is_allowed() {
        let mut parser = VtParser::new(80, 24);
        // "cs" (clipboard + selection) contains 'c', so it is allowed.
        parser.advance(b"\x1b]52;cs;V29ybGQ=\x07");
        let writes = parser.screen_mut().take_pending_clipboard_writes();
        assert_eq!(writes, vec!["World".to_string()]);
    }

    #[test]
    fn osc_52_invalid_base64_is_ignored() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]52;c;!!invalid!!\x07");
        let writes = parser.screen_mut().take_pending_clipboard_writes();
        assert!(writes.is_empty(), "invalid base64 must be ignored");
    }

    #[test]
    fn osc_52_control_characters_are_stripped() {
        let mut parser = VtParser::new(80, 24);
        // base64("A\x01B") = "QQFC".
        parser.advance(b"\x1b]52;c;QQFC\x07");
        let writes = parser.screen_mut().take_pending_clipboard_writes();
        assert_eq!(
            writes,
            vec!["AB".to_string()],
            "C0 control characters (0x01) must be stripped"
        );
    }

    // ---- OSC 9 / 777 desktop notifications (Sprint 4-1) ----

    #[test]
    fn osc_9_iterm_compatible_notification_is_queued() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]9;Build complete\x07");
        let notif = parser.screen_mut().take_pending_notification();
        assert_eq!(
            notif,
            Some(("Nexterm".to_string(), "Build complete".to_string()))
        );
    }

    #[test]
    fn osc_777_rxvt_compatible_notification_is_queued() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]777;notify;Title;Message body\x07");
        let notif = parser.screen_mut().take_pending_notification();
        assert_eq!(
            notif,
            Some(("Title".to_string(), "Message body".to_string()))
        );
    }

    #[test]
    fn osc_777_subcommands_other_than_notify_are_ignored() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]777;custom;foo\x07");
        let notif = parser.screen_mut().take_pending_notification();
        assert!(
            notif.is_none(),
            "subcommands other than `notify` are ignored"
        );
    }

    #[test]
    fn osc_8_hyperlink_is_recorded_in_the_grid() {
        let mut parser = VtParser::new(80, 24);
        // ESC ] 8 ; ; https://example.com BEL + text + link end.
        parser.advance(b"\x1b]8;;https://example.com\x07Click\x1b]8;;\x07");
        let grid = parser.screen().grid();
        // The text is written.
        assert_eq!(grid.get(0, 0).unwrap().ch, 'C');
        assert_eq!(grid.get(4, 0).unwrap().ch, 'k');
        // A span is recorded in `hyperlinks`.
        assert!(!grid.hyperlinks.is_empty(), "a hyperlink span should exist");
        let span = &grid.hyperlinks[0];
        assert_eq!(span.url, "https://example.com");
        assert_eq!(span.row, 0);
        assert_eq!(span.col_start, 0);
        assert_eq!(span.col_end, 5); // "Click" is 5 characters.
    }

    // ---- OSC 7 CWD reporting tests (Sprint 5-2 / B2) ----

    #[test]
    fn osc_7_file_uri_stores_the_pending_cwd() {
        let mut parser = VtParser::new(80, 24);
        // ESC ] 7 ; file:///home/user/proj BEL
        parser.advance(b"\x1b]7;file:///home/user/proj\x07");
        let cwd = parser.screen_mut().take_pending_cwd();
        assert_eq!(cwd, Some("/home/user/proj".to_string()));
        // The value is cleared after `take`.
        assert!(parser.screen_mut().take_pending_cwd().is_none());
    }

    #[test]
    fn osc_7_uri_with_host_still_uses_only_the_path() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]7;file://myhost/home/user\x07");
        assert_eq!(
            parser.screen_mut().take_pending_cwd(),
            Some("/home/user".to_string())
        );
    }

    #[test]
    fn osc_7_st_termination_also_works() {
        // ESC ] 7 ; file:///tmp ST (ST = ESC \).
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]7;file:///tmp\x1b\\");
        assert_eq!(
            parser.screen_mut().take_pending_cwd(),
            Some("/tmp".to_string())
        );
    }

    #[test]
    fn osc_7_percent_encoding_is_decoded() {
        let mut parser = VtParser::new(80, 24);
        // /home/user/with space (space = %20).
        parser.advance(b"\x1b]7;file:///home/user/with%20space\x07");
        assert_eq!(
            parser.screen_mut().take_pending_cwd(),
            Some("/home/user/with space".to_string())
        );
    }

    #[test]
    fn osc_7_empty_parameter_is_ignored() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]7;\x07");
        assert!(parser.screen_mut().take_pending_cwd().is_none());
    }

    // ---- ANSI 256-color / True Color tests ----

    #[test]
    fn sgr_256_color_foreground_is_applied() {
        let mut parser = VtParser::new(80, 24);
        // SGR 38;5;196 = 256-color index 196 (bright red).
        parser.advance(b"\x1b[38;5;196mX");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'X');
        assert_eq!(cell.fg, nexterm_proto::Color::Indexed(196));
    }

    #[test]
    fn sgr_256_color_background_is_applied() {
        let mut parser = VtParser::new(80, 24);
        // SGR 48;5;21 = 256-color index 21 (blue).
        parser.advance(b"\x1b[48;5;21mY");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'Y');
        assert_eq!(cell.bg, nexterm_proto::Color::Indexed(21));
    }

    #[test]
    fn sgr_truecolor_foreground_is_applied() {
        let mut parser = VtParser::new(80, 24);
        // SGR 38;2;255;128;0 = RGB(255, 128, 0) — orange.
        parser.advance(b"\x1b[38;2;255;128;0mZ");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'Z');
        assert_eq!(cell.fg, nexterm_proto::Color::Rgb(255, 128, 0));
    }

    #[test]
    fn sgr_truecolor_background_is_applied() {
        let mut parser = VtParser::new(80, 24);
        // SGR 48;2;0;255;128 = RGB(0, 255, 128) — green.
        parser.advance(b"\x1b[48;2;0;255;128mW");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'W');
        assert_eq!(cell.bg, nexterm_proto::Color::Rgb(0, 255, 128));
    }

    #[test]
    fn sgr_256_color_grayscale_is_applied() {
        let mut parser = VtParser::new(80, 24);
        // SGR 38;5;240 = grayscale ramp (232..=255).
        parser.advance(b"\x1b[38;5;240mG");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.fg, nexterm_proto::Color::Indexed(240));
    }

    // ---- Kitty graphics protocol tests ----

    /// base64 of a 1×1 RGBA image with `[R=255, G=0, B=0, A=255]`.
    fn kitty_rgba_1x1_base64() -> &'static str {
        // base64([255, 0, 0, 255]) = "/wAA/w==".
        "/wAA/w=="
    }

    #[test]
    #[allow(non_snake_case)]
    fn kitty_single_chunk_RGBA_image_decodes() {
        let mut parser = VtParser::new(80, 24);
        // ESC _ G a=T,f=32,s=1,v=1;<base64> ESC \
        let payload = kitty_rgba_1x1_base64();
        let seq = format!("\x1b_Ga=T,f=32,s=1,v=1;{}\x1b\\", payload);
        parser.advance(seq.as_bytes());
        let images = parser.screen_mut().take_pending_images();
        assert_eq!(images.len(), 1, "exactly 1 image should be registered");
        assert_eq!(images[0].width, 1);
        assert_eq!(images[0].height, 1);
        assert_eq!(images[0].rgba[0], 255); // R
        assert_eq!(images[0].rgba[1], 0); // G
        assert_eq!(images[0].rgba[2], 0); // B
        assert_eq!(images[0].rgba[3], 255); // A
    }

    #[test]
    fn kitty_split_chunk_transfer_decodes() {
        let mut parser = VtParser::new(80, 24);
        // Send a 1×1 RGBA in two chunks.
        // Split "/wAA/w==" into "/wAA" + "/w==".
        // Chunk 1: m=1 (more to come) — carries the size parameters.
        parser.advance(b"\x1b_Ga=T,f=32,s=1,v=1,m=1;/wAA\x1b\\");
        // Chunk 2: m=0 (final chunk).
        parser.advance(b"\x1b_Gm=0;/w==\x1b\\");
        let images = parser.screen_mut().take_pending_images();
        assert_eq!(
            images.len(),
            1,
            "split chunks should be assembled into a single image"
        );
        assert_eq!(images[0].width, 1);
        assert_eq!(images[0].height, 1);
    }

    #[test]
    fn regular_text_still_works_after_a_kitty_sequence() {
        let mut parser = VtParser::new(80, 24);
        // Surround the Kitty APC with plain text.
        let payload = kitty_rgba_1x1_base64();
        let seq = format!("Hi\x1b_Ga=T,f=32,s=1,v=1;{}\x1b\\Bye", payload);
        parser.advance(seq.as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'H');
        assert_eq!(grid.get(1, 0).unwrap().ch, 'i');
        assert_eq!(grid.get(2, 0).unwrap().ch, 'B');
        assert_eq!(grid.get(3, 0).unwrap().ch, 'y');
        assert_eq!(grid.get(4, 0).unwrap().ch, 'e');
    }

    // ---- Extra VT sequence tests ----

    #[test]
    fn sgr_bold_attribute_is_applied() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[1mB");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'B');
        assert!(cell.attrs.is_bold());
    }

    #[test]
    fn sgr_reset_clears_attributes() {
        let mut parser = VtParser::new(80, 24);
        // Set BOLD, then reset.
        parser.advance(b"\x1b[1m\x1b[0mX");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'X');
        assert!(!cell.attrs.is_bold());
    }

    #[test]
    fn ed_clears_cells_on_screen_erase() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Hello");
        // CSI 2J = erase the entire screen.
        parser.advance(b"\x1b[2J");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn el_clears_cells_on_line_erase() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Hello");
        // CSI 1G moves the cursor to the start of the line.
        parser.advance(b"\x1b[1G");
        // CSI 2K = erase the entire line.
        parser.advance(b"\x1b[2K");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn long_text_wraps_at_the_end_of_a_line() {
        let mut parser = VtParser::new(10, 5); // narrow 10-column terminal
        // Writing 11 characters wraps onto the second row.
        parser.advance(b"0123456789A");
        let grid = parser.screen().grid();
        // Row 1 holds 0..=9.
        assert_eq!(grid.get(9, 0).unwrap().ch, '9');
        // Character 11 ('A') lands on row 2.
        assert_eq!(grid.get(0, 1).unwrap().ch, 'A');
    }

    #[test]
    fn vtparser_initial_cursor_position_after_new() {
        let parser = VtParser::new(80, 24);
        let grid = parser.screen().grid();
        assert_eq!(grid.cursor_col, 0);
        assert_eq!(grid.cursor_row, 0);
    }

    #[test]
    fn tab_character_moves_the_cursor_to_the_next_multiple_of_8() {
        let mut parser = VtParser::new(80, 24);
        // Write a character after a TAB and confirm the position. The TAB advances
        // the cursor to col=8 and 'X' lands there.
        parser.advance(b"\tX");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(8, 0).unwrap().ch, 'X');
    }

    // ─── CJK wide-character tests ────────────────────────────────────────────

    #[test]
    fn cjk_wide_character_occupies_two_columns() {
        let mut parser = VtParser::new(80, 24);
        // A Japanese fullwidth character (`あ`) has display width 2.
        parser.advance("あ".as_bytes());
        let grid = parser.screen().grid();
        // The leading cell holds the actual character.
        assert_eq!(grid.get(0, 0).unwrap().ch, 'あ');
        // The trailing cell is a placeholder (blank).
        assert_eq!(grid.get(1, 0).unwrap().ch, ' ');
        // The cursor advanced to col=2 (Screen.cursor_col).
        assert_eq!(parser.screen().cursor().0, 2);
    }

    #[test]
    fn cjk_consecutive_wide_characters_are_placed_in_a_row() {
        let mut parser = VtParser::new(80, 24);
        // "日本語" = 3 characters × width 2 = 6 columns.
        parser.advance("日本語".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, '日');
        assert_eq!(grid.get(2, 0).unwrap().ch, '本');
        assert_eq!(grid.get(4, 0).unwrap().ch, '語');
        // Cursor ends at col=6.
        assert_eq!(parser.screen().cursor().0, 6);
    }

    #[test]
    fn cjk_mixed_full_and_half_width() {
        let mut parser = VtParser::new(80, 24);
        // "A日B" → A(col=0), 日(col=1,2), B(col=3).
        parser.advance("A日B".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'A');
        assert_eq!(grid.get(1, 0).unwrap().ch, '日');
        assert_eq!(grid.get(3, 0).unwrap().ch, 'B');
        assert_eq!(parser.screen().cursor().0, 4);
    }

    #[test]
    fn cjk_wraps_at_the_end_of_the_line() {
        // On a 5-column terminal, a wide character that would land on the right edge
        // (col=4) wraps to the next row. "ABCD" + the wide character `あ`: when `あ`
        // (width 2) starts at col=4, `col+1=5 >= width=5`, which triggers a wrap.
        let mut parser = VtParser::new(5, 5);
        parser.advance("ABCDあ".as_bytes());
        let grid = parser.screen().grid();
        // ABCD lands at col=0..=3 on row 1.
        assert_eq!(grid.get(0, 0).unwrap().ch, 'A');
        assert_eq!(grid.get(3, 0).unwrap().ch, 'D');
        // `あ` does not fit at col=4 because of its width-2, so it wraps to row 2 col 0.
        assert_eq!(grid.get(0, 1).unwrap().ch, 'あ');
    }

    #[test]
    fn simplified_chinese_occupies_two_columns() {
        let mut parser = VtParser::new(80, 24);
        // Chinese characters ("汉字") have display width 2.
        parser.advance("汉字".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, '汉');
        assert_eq!(grid.get(2, 0).unwrap().ch, '字');
        assert_eq!(parser.screen().cursor().0, 4);
    }

    #[test]
    fn korean_hangul_occupies_two_columns() {
        let mut parser = VtParser::new(80, 24);
        // Hangul syllables ("가") have display width 2.
        parser.advance("가나다".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, '가');
        assert_eq!(grid.get(2, 0).unwrap().ch, '나');
        assert_eq!(grid.get(4, 0).unwrap().ch, '다');
        assert_eq!(parser.screen().cursor().0, 6);
    }

    #[test]
    fn halfwidth_katakana_occupies_one_column() {
        let mut parser = VtParser::new(80, 24);
        // Halfwidth katakana ("ｱｲｳ") has display width 1.
        parser.advance("ｱｲｳ".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'ｱ');
        assert_eq!(grid.get(1, 0).unwrap().ch, 'ｲ');
        assert_eq!(grid.get(2, 0).unwrap().ch, 'ｳ');
        assert_eq!(parser.screen().cursor().0, 3);
    }

    #[test]
    fn cjk_wide_characters_inherit_colors() {
        let mut parser = VtParser::new(80, 24);
        // Set red (ANSI 31) and write a wide character.
        parser.advance(b"\x1b[31m");
        parser.advance("あ".as_bytes());
        let grid = parser.screen().grid();
        // The leading cell is red.
        use nexterm_proto::Color;
        assert_eq!(grid.get(0, 0).unwrap().fg, Color::Indexed(1)); // ANSI red = index 1
        // The placeholder cell shares the same foreground color.
        assert_eq!(grid.get(1, 0).unwrap().fg, Color::Indexed(1));
    }

    #[test]
    fn cjk_wide_characters_and_sgr_reset_interact_correctly() {
        let mut parser = VtParser::new(80, 24);
        // Bold + a wide character.
        parser.advance(b"\x1b[1m");
        parser.advance("漢".as_bytes());
        // A regular character after a reset.
        parser.advance(b"\x1b[0m");
        parser.advance(b"X");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, '漢');
        assert!(grid.get(0, 0).unwrap().attrs.is_bold());
        assert_eq!(grid.get(2, 0).unwrap().ch, 'X');
        assert!(!grid.get(2, 0).unwrap().attrs.is_bold());
    }

    #[test]
    fn cjk_characters_keep_working_after_a_resize() {
        let mut parser = VtParser::new(80, 24);
        parser.advance("あいう".as_bytes());
        // Confirm wide-character writes still work after a resize.
        parser.screen.resize(40, 12);
        parser.advance("えお".as_bytes());
        let grid = parser.screen().grid();
        // `え` or `お` written after the resize must exist somewhere in the grid.
        let row0: String = grid.rows[0].iter().map(|c| c.ch).collect();
        assert!(row0.contains('え') || row0.contains('お'));
    }

    #[test]
    fn apc_buffer_overflow_does_not_exhaust_memory() {
        // CRITICAL #7: a malicious PTY streaming endless bytes inside an unterminated
        // APC must not exhaust memory; the parser clears the buffer at the limit and
        // returns to normal parsing.
        let mut parser = VtParser::new(80, 24);

        // Send ESC _ (APC start).
        parser.advance(b"\x1b_");
        // Send 5 MiB of APC payload, exceeding the limit.
        let huge = vec![b'A'; 5 * 1024 * 1024];
        parser.advance(&huge);

        // The buffer stays at or below the limit (no memory exhaustion).
        assert!(
            parser.apc_buf.len() <= MAX_APC_BUF_LEN,
            "APC buffer exceeded the limit: {}",
            parser.apc_buf.len()
        );

        // After overflow the APC state is dropped and normal parsing resumes,
        // so subsequent bytes would be written to the screen as normal characters.
        assert!(!parser.apc_active);
    }

    #[test]
    fn apc_overflow_does_not_block_subsequent_apc_sequences() {
        // After overflow-driven discard, a fresh well-formed APC must still be processed.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b_");
        let huge = vec![b'A'; 5 * 1024 * 1024];
        parser.advance(&huge);

        // Start a new APC.
        parser.advance(b"\x1b_Gtest\x1b\\");
        // We only verify that the parser does not crash; the concrete behavior
        // depends on `decode_kitty`.
    }

    // ---- v1.9.5: device attribute / DSR query responses ----
    //
    // Background: PowerShell + PSReadLine on Windows ConPTY sends
    // `CSI c` and/or `CSI 6 n` on startup and waits for a terminal reply
    // before drawing the prompt. The previous parser silently dropped these
    // queries, so the prompt never appeared. The reader thread now drains
    // `take_pending_responses` after every `advance` and writes the bytes
    // back to the PTY.

    #[test]
    fn primary_device_attributes_query_produces_vt102_reply() {
        // CSI c (or CSI 0 c) — Primary DA. Reply must identify us as a
        // VT102-class terminal: `ESC [ ? 6 c`.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[c");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b[?6c".to_vec()]);
    }

    #[test]
    fn secondary_device_attributes_query_produces_vt220_reply() {
        // CSI > c — Secondary DA. xterm-compatible reply: VT220, firmware
        // 276, no ROM cartridge.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[>c");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b[>1;276;0c".to_vec()]);
    }

    #[test]
    fn dsr_operating_status_query_produces_ok_reply() {
        // CSI 5 n — DSR operating status. Reply `ESC [ 0 n` (terminal OK).
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[5n");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b[0n".to_vec()]);
    }

    #[test]
    fn dsr_cursor_position_query_uses_one_based_indices() {
        // CSI 6 n — DSR cursor position. Reply `ESC [ row ; col R` with
        // 1-based coordinates. Move the cursor first so the reply is
        // non-trivial.
        let mut parser = VtParser::new(80, 24);
        // CUP 5;10 → row 5, col 10 (1-based).
        parser.advance(b"\x1b[5;10H");
        parser.advance(b"\x1b[6n");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b[5;10R".to_vec()]);
    }

    #[test]
    fn take_pending_responses_drains_the_queue() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[c\x1b[5n");
        let first = parser.screen_mut().take_pending_responses();
        assert_eq!(first.len(), 2);
        let second = parser.screen_mut().take_pending_responses();
        assert!(second.is_empty());
    }

    // ---- OSC 4 / 10 / 11: dynamic colors (query & set) ----
    //
    // Background: vim/neovim probe `OSC 11 ; ?` at startup to auto-detect a
    // light or dark background, and tools like pywal set palette entries via
    // OSC 4. Queries are answered through the same `pending_responses` queue
    // as DA/DSR. Replies use the xterm 16-bit-per-channel form
    // `rgb:rrrr/gggg/bbbb` and mirror the request terminator (BEL vs ST).

    #[test]
    fn osc11_query_replies_with_default_background() {
        // Fallback default background mirrors the client-side Color::Default
        // fallback (#0d0d0d).
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]11;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]11;rgb:0d0d/0d0d/0d0d\x07".to_vec()]);
    }

    #[test]
    fn osc10_query_replies_with_default_foreground() {
        // Fallback default foreground mirrors the client-side fallback (#d9d9d9).
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]10;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]10;rgb:d9d9/d9d9/d9d9\x07".to_vec()]);
    }

    #[test]
    fn osc11_query_reflects_server_configured_defaults() {
        // The server wires the active theme colors in via set_default_colors
        // so queries report what is actually rendered.
        let mut parser = VtParser::new(80, 24);
        parser
            .screen_mut()
            .set_default_colors([0xaa, 0xbb, 0xcc], [0x11, 0x22, 0x33]);
        parser.advance(b"\x1b]11;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]11;rgb:1111/2222/3333\x07".to_vec()]);
    }

    #[test]
    fn osc10_set_then_query_returns_the_new_color() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]10;#ff8800\x07");
        parser.advance(b"\x1b]10;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]10;rgb:ffff/8888/0000\x07".to_vec()]);
    }

    #[test]
    fn osc11_set_accepts_xparsecolor_rgb_form() {
        // `rgb:RR/GG/BB` (8-bit per channel) is the other common spec form.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]11;rgb:12/34/56\x07");
        parser.advance(b"\x1b]11;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]11;rgb:1212/3434/5656\x07".to_vec()]);
    }

    #[test]
    fn osc4_set_then_query_returns_the_palette_entry() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]4;1;#ff0000\x07");
        parser.advance(b"\x1b]4;1;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]4;1;rgb:ffff/0000/0000\x07".to_vec()]);
    }

    #[test]
    fn osc4_query_of_an_unset_entry_uses_the_builtin_xterm_palette() {
        // xterm 256-color palette: index 1 is #cd0000, index 196 (color cube)
        // is #ff0000, index 244 (grayscale ramp) is #808080.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]4;1;?\x07");
        parser.advance(b"\x1b]4;196;?\x07");
        parser.advance(b"\x1b]4;244;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(
            replies,
            vec![
                b"\x1b]4;1;rgb:cdcd/0000/0000\x07".to_vec(),
                b"\x1b]4;196;rgb:ffff/0000/0000\x07".to_vec(),
                b"\x1b]4;244;rgb:8080/8080/8080\x07".to_vec(),
            ]
        );
    }

    #[test]
    fn osc4_handles_multiple_pairs_in_one_sequence() {
        // OSC 4 accepts repeated `index;spec` pairs: set entry 2 and query
        // entry 3 in a single sequence.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]4;2;#00ff00;3;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]4;3;rgb:cdcd/cdcd/0000\x07".to_vec()]);
        // The set half of the pair took effect too.
        parser.advance(b"\x1b]4;2;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]4;2;rgb:0000/ffff/0000\x07".to_vec()]);
    }

    #[test]
    fn osc104_resets_a_palette_entry_and_osc104_bare_resets_all() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]4;1;#123456\x07\x1b]4;2;#654321\x07");
        // OSC 104;1 resets only entry 1.
        parser.advance(b"\x1b]104;1\x07");
        parser.advance(b"\x1b]4;1;?\x07\x1b]4;2;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(
            replies,
            vec![
                b"\x1b]4;1;rgb:cdcd/0000/0000\x07".to_vec(),
                b"\x1b]4;2;rgb:6565/4343/2121\x07".to_vec(),
            ]
        );
        // Bare OSC 104 resets everything.
        parser.advance(b"\x1b]104\x07");
        parser.advance(b"\x1b]4;2;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]4;2;rgb:0000/cdcd/0000\x07".to_vec()]);
    }

    #[test]
    fn osc110_and_111_reset_the_default_colors() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]10;#ff8800\x07\x1b]11;#001122\x07");
        parser.advance(b"\x1b]110\x07\x1b]111\x07");
        parser.advance(b"\x1b]10;?\x07\x1b]11;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(
            replies,
            vec![
                b"\x1b]10;rgb:d9d9/d9d9/d9d9\x07".to_vec(),
                b"\x1b]11;rgb:0d0d/0d0d/0d0d\x07".to_vec(),
            ]
        );
    }

    #[test]
    fn st_terminated_color_query_gets_an_st_terminated_reply() {
        // Requests terminated with ST (ESC \) must be answered with ST, not
        // BEL — some applications parse the reply terminator strictly.
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]11;?\x1b\\");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]11;rgb:0d0d/0d0d/0d0d\x1b\\".to_vec()]);
    }

    // ---- OSC 72: kitty drag-and-drop protocol (application side) ----
    //
    // Practical subset: support query (t=q, answered immediately through
    // `pending_responses`), opt-in/out (t=a / t=A), and the data-request /
    // completion messages (t=r), which are queued for the reader thread to
    // answer from the pane's stored drop payload. Motion negotiation (t=m)
    // and remote-machine drops are not implemented in this first release.

    #[test]
    fn osc72_query_is_echoed_back_with_the_id() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]72;t=q:i=3\x1b\\");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]72;t=q:i=3\x1b\\".to_vec()]);
    }

    #[test]
    fn osc72_opt_in_and_out_toggle_dnd() {
        let mut parser = VtParser::new(80, 24);
        assert!(!parser.screen().dnd_enabled());
        parser.advance(b"\x1b]72;t=a;text/uri-list\x1b\\");
        assert!(parser.screen().dnd_enabled());
        parser.advance(b"\x1b]72;t=A\x1b\\");
        assert!(!parser.screen().dnd_enabled());
    }

    #[test]
    fn osc72_data_request_and_completion_are_queued() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]72;t=r:x=1\x1b\\");
        parser.advance(b"\x1b]72;t=r:o=1\x1b\\");
        assert_eq!(
            parser.screen_mut().take_pending_dnd_requests(),
            vec![DndRequest::Data { index: 1 }, DndRequest::Complete]
        );
        // Drained after take.
        assert!(parser.screen_mut().take_pending_dnd_requests().is_empty());
    }

    #[test]
    fn osc72_pending_request_queue_is_bounded() {
        // Memory-DoS guard: a hostile app spamming data requests must not
        // grow the queue without bound.
        let mut parser = VtParser::new(80, 24);
        for _ in 0..64 {
            parser.advance(b"\x1b]72;t=r:x=1\x1b\\");
        }
        assert!(parser.screen_mut().take_pending_dnd_requests().len() <= 8);
    }

    // ---- OSC 9;4: ConEmu-style progress reporting ----
    //
    // Format: ESC ] 9 ; 4 ; state ; progress BEL/ST — state 0 removes the
    // indicator, 1 = normal, 2 = error, 3 = indeterminate, 4 = paused.
    // Adopted by Windows Terminal and iTerm2; surfaced in the tab bar.

    #[test]
    fn osc9_4_reports_progress_state_and_percentage() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]9;4;1;42\x07");
        assert_eq!(parser.screen_mut().take_pending_progress(), Some((1, 42)));
        // Drained after take.
        assert_eq!(parser.screen_mut().take_pending_progress(), None);
        // State 0 removes the indicator.
        parser.advance(b"\x1b]9;4;0;0\x07");
        assert_eq!(parser.screen_mut().take_pending_progress(), Some((0, 0)));
    }

    #[test]
    fn osc9_4_clamps_progress_and_rejects_garbage() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]9;4;1;250\x07");
        assert_eq!(parser.screen_mut().take_pending_progress(), Some((1, 100)));
        // Unknown state / non-numeric fields are ignored.
        parser.advance(b"\x1b]9;4;9;50\x07\x1b]9;4;abc;xyz\x07");
        assert_eq!(parser.screen_mut().take_pending_progress(), None);
    }

    #[test]
    fn osc9_4_does_not_leak_into_the_notification_path() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]9;4;1;42\x07");
        assert_eq!(parser.screen_mut().take_pending_notification(), None);
        // A plain OSC 9 message still raises a notification.
        parser.advance(b"\x1b]9;hello\x07");
        assert_eq!(
            parser.screen_mut().take_pending_notification(),
            Some(("Nexterm".to_string(), "hello".to_string()))
        );
    }

    // ---- OSC 99: kitty desktop notifications ----
    //
    // Practical subset of the kitty spec: `i=` identifier for multi-part
    // accumulation, `d=` completion flag (default done), `p=title|body`
    // payload types, `e=1` base64 payloads. Completed notifications feed the
    // same pending-notification path as OSC 9/777 (client consent UI applies).

    #[test]
    fn osc99_simple_payload_becomes_a_notification_title() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]99;;Hello\x07");
        assert_eq!(
            parser.screen_mut().take_pending_notification(),
            Some(("Hello".to_string(), String::new()))
        );
    }

    #[test]
    fn osc99_multi_part_title_and_body_accumulate_until_done() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]99;i=1:d=0:p=title;Build finished\x07");
        // Not complete yet.
        assert_eq!(parser.screen_mut().take_pending_notification(), None);
        parser.advance(b"\x1b]99;i=1:d=1:p=body;All 25 suites green\x07");
        assert_eq!(
            parser.screen_mut().take_pending_notification(),
            Some((
                "Build finished".to_string(),
                "All 25 suites green".to_string()
            ))
        );
    }

    #[test]
    fn osc99_base64_payload_is_decoded() {
        let mut parser = VtParser::new(80, 24);
        // "SGVsbG8=" = base64("Hello")
        parser.advance(b"\x1b]99;e=1;SGVsbG8=\x07");
        assert_eq!(
            parser.screen_mut().take_pending_notification(),
            Some(("Hello".to_string(), String::new()))
        );
    }

    #[test]
    fn osc99_empty_or_unsupported_payloads_produce_no_notification() {
        let mut parser = VtParser::new(80, 24);
        // Unsupported payload type (icon) and an empty complete notification.
        parser.advance(b"\x1b]99;p=icon;abc\x07\x1b]99;;\x07");
        assert_eq!(parser.screen_mut().take_pending_notification(), None);
    }

    #[test]
    fn osc99_pending_identifier_count_is_bounded() {
        // Memory-DoS guard: a hostile stream opening unlimited `d=0`
        // identifiers must not grow memory without bound.
        let mut parser = VtParser::new(80, 24);
        for i in 0..64 {
            let seq = format!("\x1b]99;i=id{i}:d=0:p=title;spam\x07");
            parser.advance(seq.as_bytes());
        }
        assert!(parser.screen_mut().osc99_pending_count() <= 4);
    }

    // ---- OSC 4/10/11 propagation: color-override snapshots (roadmap #10b) ----
    //
    // The reader thread drains `take_color_overrides_if_changed` after every
    // `advance` and broadcasts the full override state to clients so OSC
    // color sets become visible in the renderer.

    #[test]
    fn color_set_marks_overrides_changed_and_snapshot_reflects_it() {
        let mut parser = VtParser::new(80, 24);
        // Nothing changed yet.
        assert!(
            parser
                .screen_mut()
                .take_color_overrides_if_changed()
                .is_none()
        );

        parser.advance(b"\x1b]10;#ff8800\x07\x1b]4;1;#123456\x07");
        let snap = parser
            .screen_mut()
            .take_color_overrides_if_changed()
            .expect("set must mark the overrides changed");
        assert_eq!(snap.fg, Some([0xff, 0x88, 0x00]));
        assert_eq!(snap.bg, None);
        assert_eq!(snap.palette, vec![(1, [0x12, 0x34, 0x56])]);

        // Drained: no change until the next mutation.
        assert!(
            parser
                .screen_mut()
                .take_color_overrides_if_changed()
                .is_none()
        );

        // A reset is also a change (fg returns to None).
        parser.advance(b"\x1b]110\x07");
        let snap = parser
            .screen_mut()
            .take_color_overrides_if_changed()
            .expect("reset must mark the overrides changed");
        assert_eq!(snap.fg, None);
        assert!(snap.palette.len() == 1);
    }

    #[test]
    fn color_queries_do_not_mark_overrides_changed() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]10;?\x07\x1b]4;1;?\x07");
        assert!(
            parser
                .screen_mut()
                .take_color_overrides_if_changed()
                .is_none()
        );
    }

    // ---- OSC 22: mouse pointer shape ----
    //
    // Format: ESC ] 22 ; <shape-name> BEL/ST. An empty name resets to the
    // default shape. The name is forwarded to the client, which maps it onto
    // a winit CursorIcon (unknown names fall back to the default there).

    #[test]
    fn osc22_queues_a_pointer_shape_change() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]22;pointer\x07");
        assert_eq!(
            parser.screen_mut().take_pending_pointer_shape(),
            Some("pointer".to_string())
        );
        // Drained after take.
        assert_eq!(parser.screen_mut().take_pending_pointer_shape(), None);
    }

    #[test]
    fn osc22_with_an_empty_name_resets_to_default() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]22;\x07");
        assert_eq!(
            parser.screen_mut().take_pending_pointer_shape(),
            Some("default".to_string())
        );
    }

    #[test]
    fn osc22_caps_the_shape_name_length() {
        // Oversized names are dropped (not truncated) — no real shape name is
        // this long, so this can only be garbage or an attack.
        let mut parser = VtParser::new(80, 24);
        let long = format!("\x1b]22;{}\x07", "a".repeat(1000));
        parser.advance(long.as_bytes());
        assert_eq!(parser.screen_mut().take_pending_pointer_shape(), None);
    }

    #[test]
    fn invalid_color_specs_are_ignored() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]10;#zzz\x07\x1b]10;notacolor\x07\x1b]4;999;#ff0000\x07");
        // No replies queued, defaults untouched.
        assert!(parser.screen_mut().take_pending_responses().is_empty());
        parser.advance(b"\x1b]10;?\x07");
        let replies = parser.screen_mut().take_pending_responses();
        assert_eq!(replies, vec![b"\x1b]10;rgb:d9d9/d9d9/d9d9\x07".to_vec()]);
    }

    // ---- Scrollback emission (F3 / ADR-0008) ----

    /// Reconstruct a scrollback line's text, trimming trailing blanks.
    fn line_text(cells: &[nexterm_proto::Cell]) -> String {
        let s: String = cells.iter().map(|c| c.ch).collect();
        s.trim_end().to_string()
    }

    #[test]
    fn scrolled_off_lines_are_emitted_in_order() {
        // 3-row screen; feed 5 lines so the first two scroll off.
        let mut parser = VtParser::new(10, 3);
        parser.advance(b"a\r\nb\r\nc\r\nd\r\ne");

        let lines = parser.screen_mut().take_scrolled_off_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "a");
        assert_eq!(line_text(&lines[1]), "b");
        // The visible top row is now 'c'.
        assert_eq!(parser.screen().grid().get(0, 0).unwrap().ch, 'c');
    }

    #[test]
    fn take_scrolled_off_lines_drains_the_queue() {
        let mut parser = VtParser::new(10, 2);
        parser.advance(b"1\r\n2\r\n3"); // one line scrolls off
        assert_eq!(parser.screen_mut().take_scrolled_off_lines().len(), 1);
        // Draining leaves the queue empty until more lines scroll off.
        assert!(parser.screen_mut().take_scrolled_off_lines().is_empty());
        parser.advance(b"\r\n4"); // another line scrolls off
        assert_eq!(parser.screen_mut().take_scrolled_off_lines().len(), 1);
    }

    #[test]
    fn alternate_screen_does_not_emit_scrollback() {
        let mut parser = VtParser::new(10, 2);
        // Enter the alternate screen (DEC private mode 1049), then scroll.
        parser.advance(b"\x1b[?1049h");
        parser.advance(b"x\r\ny\r\nz");
        assert!(
            parser.screen_mut().take_scrolled_off_lines().is_empty(),
            "alt-screen scrolling must not emit scrollback"
        );
    }
}
