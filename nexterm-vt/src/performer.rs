//! `vte::Perform` implementation — applies VT sequences to a `Screen`.

use vte::Perform;

use crate::screen::{Screen, SemanticMarkKind};

impl Perform for Screen {
    /// Writes a printable character.
    fn print(&mut self, c: char) {
        self.write_char(c);
    }

    /// Handles a control character (C0/C1).
    fn execute(&mut self, byte: u8) {
        match byte {
            // BEL — raise the pending-notification flag.
            0x07 => {
                self.set_pending_bell();
            }
            // BS (backspace).
            0x08 if self.cursor().0 > 0 => {
                let (col, row) = self.cursor();
                self.move_cursor(col - 1, row);
            }
            // HT (horizontal tab) — move to the next multiple of 8.
            0x09 => {
                let (col, row) = self.cursor();
                let next_tab = ((col / 8) + 1) * 8;
                self.move_cursor(next_tab.min(self.grid().width.saturating_sub(1)), row);
            }
            // LF / VT / FF (line-break family).
            0x0A..=0x0C => {
                self.advance_line();
            }
            // CR (carriage return).
            0x0D => {
                let (_, row) = self.cursor();
                self.move_cursor(0, row);
            }
            _ => {} // Ignore every other control character.
        }
    }

    /// Handles a CSI (escape-code) sequence.
    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        // Handle DEC private modes (those carrying a `?` prefix).
        if intermediates.first() == Some(&b'?') {
            match action {
                'h' => {
                    self.dec_private_mode(params, true);
                    return;
                }
                'l' => {
                    self.dec_private_mode(params, false);
                    return;
                }
                _ => {}
            }
        }

        // Flatten the parameter list into a `Vec<u16>`.
        let p: Vec<u16> = params
            .iter()
            .map(|sub| sub.first().copied().unwrap_or(0))
            .collect();

        // Helpers that resolve the first/second parameters with their default values.
        let p1 = |default: u16| {
            if p.is_empty() || p[0] == 0 {
                default
            } else {
                p[0]
            }
        };
        let p2 = |default: u16| {
            if p.len() < 2 || p[1] == 0 {
                default
            } else {
                p[1]
            }
        };

        match action {
            // CUP / HVP — move the cursor (1-based → 0-based).
            'H' | 'f' => {
                let row = p1(1).saturating_sub(1);
                let col = p2(1).saturating_sub(1);
                self.move_cursor(col, row);
            }
            // CUU — move the cursor up.
            'A' => {
                let (col, row) = self.cursor();
                self.move_cursor(col, row.saturating_sub(p1(1)));
            }
            // CUD — move the cursor down.
            'B' => {
                let (col, row) = self.cursor();
                // Use saturating_add so huge arguments (e.g. `\x1b[99999999B`) do
                // not panic during u32 addition. The trailing `.min` then clamps the
                // value into the grid.
                let new_row = row
                    .saturating_add(p1(1))
                    .min(self.grid().height.saturating_sub(1));
                self.move_cursor(col, new_row);
            }
            // CUF — move the cursor right.
            'C' => {
                let (col, row) = self.cursor();
                // Same idea as above. Fix for the panic discovered by the late
                // Sprint 5-7 fuzz target `osc_url`.
                let new_col = col
                    .saturating_add(p1(1))
                    .min(self.grid().width.saturating_sub(1));
                self.move_cursor(new_col, row);
            }
            // CUB — move the cursor left.
            'D' => {
                let (col, row) = self.cursor();
                self.move_cursor(col.saturating_sub(p1(1)), row);
            }
            // CHA — move to the given column (1-based).
            'G' => {
                let (_, row) = self.cursor();
                self.move_cursor(p1(1).saturating_sub(1), row);
            }
            // VPA — move to the given row (1-based).
            'd' => {
                let (col, _) = self.cursor();
                self.move_cursor(col, p1(1).saturating_sub(1));
            }
            // ED — erase in display.
            'J' => {
                self.erase_in_display(p1(0));
            }
            // EL — erase in line.
            'K' => {
                self.erase_in_line(p1(0));
            }
            // SGR — set graphic rendition.
            'm' => {
                let sgr: Vec<u16> = params
                    .iter()
                    .map(|sub| sub.first().copied().unwrap_or(0))
                    .collect();
                self.apply_sgr(&sgr);
            }
            // DECSTBM — set the scrolling region.
            'r' => {
                let top = p1(1).saturating_sub(1);
                let bottom = p2(self.grid().height).saturating_sub(1);
                // Direct access to Screen is needed here, so call the helper on screen.rs.
                self.set_scroll_region(top, bottom);
            }
            // Kitty keyboard protocol: CSI > flags u (push), CSI < n u (pop).
            'u' => match intermediates.first() {
                Some(&b'>') => {
                    let flags = params
                        .iter()
                        .next()
                        .and_then(|sub| sub.first().copied())
                        .unwrap_or(0) as u8;
                    self.push_keyboard_protocol_flags(flags);
                }
                Some(&b'<') => {
                    let n = params
                        .iter()
                        .next()
                        .and_then(|sub| sub.first().copied())
                        .unwrap_or(1) as usize;
                    self.pop_keyboard_protocol_flags(n.max(1));
                }
                _ => {} // CSI ? u (query) — ignored; replies would need PTY write-back.
            },
            _ => {} // Ignore every unsupported CSI sequence.
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // `params[0]` is the code; `params[1..]` is the payload.
        if params.is_empty() {
            return;
        }
        let code = match std::str::from_utf8(params[0]) {
            Ok(s) => s.trim(),
            Err(_) => return,
        };
        match code {
            // OSC 0: set the icon name and the window title.
            // OSC 1: set the icon name (treated as the title here).
            // OSC 2: set the window title.
            "0" | "1" | "2" => {
                if let Some(title_bytes) = params.get(1)
                    && let Ok(title) = std::str::from_utf8(title_bytes)
                {
                    self.set_pending_title(title.to_string());
                }
            }
            // OSC 7: current working directory (CWD) report.
            // Format: ESC ] 7 ; file://[host]/path BEL
            // - The host part is ignored (local/remote alike use the path only).
            // - The path may be percent-encoded.
            // - Used to inherit the parent pane's CWD when spawning a new pane.
            "7" => {
                if let Some(payload_bytes) = params.get(1)
                    && let Ok(payload) = std::str::from_utf8(payload_bytes)
                    && let Some(cwd) = crate::screen::parse_osc7_cwd(payload.trim())
                {
                    self.set_pending_cwd(cwd);
                }
            }
            // OSC 8: hyperlink.
            // Format: ESC ] 8 ; <params> ; <URI> BEL
            // An empty URI string terminates the link.
            "8" => {
                // params[1] holds optional attributes (which we ignore); params[2] is the URI.
                let uri = params
                    .get(2)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("")
                    .trim();
                if uri.is_empty() {
                    self.set_hyperlink(None);
                } else {
                    self.set_hyperlink(Some(uri.to_string()));
                }
            }
            // OSC 9: iTerm2-compatible desktop notification.
            // Format: ESC ] 9 ; <message> BEL
            "9" => {
                if let Some(msg_bytes) = params.get(1)
                    && let Ok(msg) = std::str::from_utf8(msg_bytes)
                {
                    self.set_pending_notification("Nexterm".to_string(), msg.to_string());
                }
            }
            // OSC 52: clipboard write request (Sprint 4-1).
            // Format: ESC ] 52 ; <selection> ; <base64 payload> BEL/ST
            // Only the `c` (clipboard) selection is supported; `p` (primary) and `s`
            // (secondary) are ignored.
            // A payload of `"?"` is a read request, but reading is intentionally not
            // supported for security reasons (every such request is ignored).
            // The client side prompts for consent according to the
            // SecurityConfig.osc52_clipboard policy, so this handler only queues the
            // request as pending.
            "52" => {
                let selection = params
                    .get(1)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("");
                // Process only when the selection includes `c` (also accepts
                // multi-target selections such as `"cs"`).
                if !selection.contains('c') {
                    return;
                }
                let payload = params
                    .get(2)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("")
                    .trim();
                // `"?"` is a read request → reject it for security reasons.
                if payload == "?" {
                    return;
                }
                // Cap the OSC 52 payload size to mitigate DoS attacks.
                // The actual policy limit is enforced on the client side via
                // SecurityConfig.osc52_max_bytes; here the VT parser also bails out
                // at 16 MiB (about 12 MiB of decoded base64).
                const MAX_OSC52_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
                if payload.len() > MAX_OSC52_PAYLOAD_BYTES {
                    return;
                }
                if let Some(decoded) = crate::image::base64_decode(payload.as_bytes())
                    && let Ok(text) = String::from_utf8(decoded)
                {
                    self.queue_clipboard_write(text);
                }
            }
            // OSC 777: rxvt-compatible desktop notification.
            // Format: ESC ] 777 ; notify ; <title> ; <body> BEL/ST
            "777" => {
                let cmd = params
                    .get(1)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("");
                if cmd != "notify" {
                    return;
                }
                let title = params
                    .get(2)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("Nexterm")
                    .to_string();
                let body = params
                    .get(3)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("")
                    .to_string();
                self.set_pending_notification(title, body);
            }
            // OSC 1337: iTerm2 inline image protocol.
            // Format: ESC ] 1337 ; File=[key=value;...] : [base64-data] BEL/ST
            // Only images with `inline=1` are rendered; all other OSC 1337
            // sub-commands (cursor shape, etc.) are silently ignored.
            "1337" => {
                self.handle_iterm2_osc(params);
            }
            // OSC 133: semantic zone markers (prompt / command / output marking).
            // Format: ESC ] 133 ; <A|B|C|D[;exit_code]> ST
            "133" => {
                if let Some(mark_bytes) = params.get(1)
                    && let Ok(mark) = std::str::from_utf8(mark_bytes)
                {
                    match mark.trim() {
                        "A" => {
                            self.add_semantic_mark(SemanticMarkKind::PromptStart, None);
                        }
                        "B" => {
                            self.add_semantic_mark(SemanticMarkKind::CommandStart, None);
                        }
                        "C" => {
                            self.add_semantic_mark(SemanticMarkKind::OutputStart, None);
                        }
                        "D" => {
                            // ESC ] 133 ; D ; <exit_code> BEL
                            // `params[2]` is the (optional) exit code.
                            let exit_code = params
                                .get(2)
                                .and_then(|b| std::str::from_utf8(b).ok())
                                .and_then(|s| s.trim().parse::<i32>().ok());
                            self.add_semantic_mark(SemanticMarkKind::CommandEnd, exit_code);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}

    /// DCS start — when `action == 'q'`, this is a Sixel sequence.
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        if action == 'q' {
            self.start_sixel();
        }
    }

    /// DCS data byte.
    fn put(&mut self, byte: u8) {
        self.push_dcs_byte(byte);
    }

    /// DCS end — finalizes the Sixel decode.
    fn unhook(&mut self) {
        self.finish_sixel();
    }
}

impl Screen {
    /// Sets DEC private modes (CSI `h` / `l` with a `?` prefix).
    fn dec_private_mode(&mut self, params: &vte::Params, enable: bool) {
        for param in params.iter() {
            let mode = param.first().copied().unwrap_or(0);
            match mode {
                // DEC Private Mode 47 / 1047: alternate screen buffer (no cursor save).
                47 | 1047 => {
                    if enable {
                        self.switch_to_alt();
                    } else {
                        self.switch_to_primary();
                    }
                }
                // DEC Private Mode 1049: alternate screen buffer (with cursor save).
                1049 => {
                    if enable {
                        self.switch_to_alt();
                    } else {
                        self.switch_to_primary();
                    }
                }
                // DEC Private Mode 1000: X11 mouse reporting (basic click).
                1000 => {
                    self.mouse_mode = if enable { 1 } else { 0 };
                }
                // DEC Private Mode 1006: SGR extended mouse reporting.
                1006 => {
                    self.mouse_mode = if enable { 2 } else { 0 };
                }
                // DEC Private Mode 2004: bracketed paste mode.
                2004 => {
                    self.set_bracketed_paste(enable);
                }
                // DEC Private Mode 2026: synchronized output mode.
                2026 => {
                    self.set_synchronized_output(enable);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod kitty_csi_tests {
    use crate::VtParser;

    fn parse(input: &[u8]) -> VtParser {
        let mut p = VtParser::new(80, 24);
        p.advance(input);
        p
    }

    #[test]
    fn csi_push_sets_flags() {
        // CSI > 1 u → push flags=1
        let p = parse(b"\x1b[>1u");
        assert_eq!(p.screen().keyboard_protocol_flags(), 1);
    }

    #[test]
    fn csi_push_all_flags() {
        // CSI > 15 u → push flags=0x0f
        let p = parse(b"\x1b[>15u");
        assert_eq!(p.screen().keyboard_protocol_flags(), 15);
    }

    #[test]
    fn csi_pop_restores_previous() {
        // Push 1, push 3, pop 1 → should restore 1
        let mut p = VtParser::new(80, 24);
        p.advance(b"\x1b[>1u");
        p.advance(b"\x1b[>3u");
        assert_eq!(p.screen().keyboard_protocol_flags(), 3);
        p.advance(b"\x1b[<1u");
        assert_eq!(p.screen().keyboard_protocol_flags(), 1);
    }

    #[test]
    fn csi_pop_zero_without_arg_pops_one() {
        // CSI < u with no param → pop 1 level (default)
        let mut p = VtParser::new(80, 24);
        p.advance(b"\x1b[>7u");
        p.advance(b"\x1b[<u");
        assert_eq!(p.screen().keyboard_protocol_flags(), 0);
    }

    #[test]
    fn csi_u_unknown_intermediate_is_ignored() {
        // CSI ? u (query) is silently ignored
        let mut p = VtParser::new(80, 24);
        p.advance(b"\x1b[>5u");
        p.advance(b"\x1b[?u"); // query — no-op
        assert_eq!(p.screen().keyboard_protocol_flags(), 5);
    }
}
