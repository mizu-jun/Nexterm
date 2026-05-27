#![warn(missing_docs)]
//! nexterm-vt — VT sequence parser plus virtual-grid implementation.
//!
//! Uses the `vte` crate to parse terminal escape sequences and applies them to a
//! two-dimensional cell array (the virtual grid).

pub mod image;
mod performer;
mod screen;

pub use screen::{PendingImage, Screen, SemanticMark, SemanticMarkKind};

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
}
