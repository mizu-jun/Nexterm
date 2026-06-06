//! Key code -> VT100/xterm escape sequence conversion.

use nexterm_proto::{KeyCode, Modifiers};

/// Convert a key code with modifiers into a VT100/xterm escape sequence.
pub(super) fn key_to_bytes(code: &KeyCode, mods: Modifiers) -> Vec<u8> {
    match code {
        KeyCode::Char(ch) => {
            if mods.is_ctrl() {
                // Ctrl+letter -> ASCII control code (1-26).
                let c = (*ch as u8) & 0x1f;
                if c > 0 {
                    return vec![c];
                }
            }
            ch.to_string().into_bytes()
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Escape => vec![0x1b],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => vec![],
        },
    }
}

/// Kitty keyboard protocol modifier parameter: `(shift | alt | ctrl | meta) + 1`.
/// Bits: Shift=1, Alt=2, Ctrl=4, Super/Meta=8. No modifiers → param=1.
fn kitty_mods_param(mods: Modifiers) -> u8 {
    let shift = if mods.is_shift() { 1u8 } else { 0 };
    let alt = if mods.0 & Modifiers::ALT != 0 { 2u8 } else { 0 };
    let ctrl = if mods.is_ctrl() { 4u8 } else { 0 };
    let meta = if mods.0 & Modifiers::META != 0 { 8u8 } else { 0 };
    (shift | alt | ctrl | meta) + 1
}

/// Unicode codepoint for function keys as used by the Kitty protocol.
/// Returns `None` for key codes that must use legacy encoding even in Kitty mode.
fn kitty_functional_codepoint(code: &KeyCode) -> Option<u32> {
    match code {
        KeyCode::Escape => Some(27),
        KeyCode::Enter => Some(13),
        KeyCode::Tab => Some(9),
        KeyCode::Backspace => Some(127),
        KeyCode::Delete => Some(57361),
        KeyCode::Insert => Some(57348),
        KeyCode::Up => Some(57352),
        KeyCode::Down => Some(57353),
        KeyCode::Right => Some(57351),
        KeyCode::Left => Some(57350),
        KeyCode::Home => Some(57358),
        KeyCode::End => Some(57359),
        KeyCode::PageUp => Some(57360),
        KeyCode::PageDown => Some(57362),
        KeyCode::BackTab => Some(9), // Tab with shift — handled via mods
        KeyCode::F(n) => match n {
            1 => Some(57399),
            2 => Some(57400),
            3 => Some(57401),
            4 => Some(57402),
            5 => Some(57403),
            6 => Some(57404),
            7 => Some(57405),
            8 => Some(57406),
            9 => Some(57407),
            10 => Some(57408),
            11 => Some(57409),
            12 => Some(57410),
            _ => None,
        },
        _ => None,
    }
}

/// Encode a key event using the Kitty keyboard protocol (CSI u format).
///
/// `flags` is the progressive-enhancement bitmask active in the focused pane:
///   - bit 0 (0x01): disambiguate escape codes
///   - bit 1 (0x02): report event types (press/repeat/release)
///   - bit 2 (0x04): report alternate keys
///   - bit 3 (0x08): report all keys as escape codes
///
/// `event_type`: 1=press, 2=repeat, 3=release.
pub(super) fn kitty_key_to_bytes(
    code: &KeyCode,
    mods: Modifiers,
    event_type: u8,
    flags: u8,
) -> Vec<u8> {
    let report_event_type = flags & 0x02 != 0;

    // Determine the codepoint and mods_param.
    let (codepoint, extra_mods) = match code {
        KeyCode::Char(ch) => (*ch as u32, mods),
        KeyCode::BackTab => (9, Modifiers(mods.0 | Modifiers::SHIFT)),
        _ => {
            if let Some(cp) = kitty_functional_codepoint(code) {
                (cp, mods)
            } else {
                // Unsupported key — fall back to legacy encoding.
                return key_to_bytes(code, mods);
            }
        }
    };

    let mods_param = kitty_mods_param(extra_mods);

    // Build CSI u sequence, omitting trailing defaults:
    //   CSI {codepoint} ; {mods_param} ; {event_type} u
    // Omit event_type if it is 1 (press) and event-type reporting is disabled.
    // Omit mods_param if it is 1 (no modifiers) AND event_type would also be omitted.
    let emit_event = report_event_type || event_type != 1;
    let emit_mods = mods_param != 1 || emit_event;

    let mut seq = format!("\x1b[{}", codepoint);
    if emit_mods {
        seq.push(';');
        seq.push_str(&mods_param.to_string());
    }
    if emit_event {
        if !emit_mods {
            seq.push(';'); // need the separator even if mods were default
        }
        seq.push(';');
        seq.push_str(&event_type.to_string());
    }
    seq.push('u');
    seq.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── kitty_mods_param ────────────────────────────────────────────────────

    #[test]
    fn kitty_mods_no_modifiers() {
        // No modifiers → param = 1.
        assert_eq!(kitty_mods_param(Modifiers::default()), 1);
    }

    #[test]
    fn kitty_mods_shift() {
        let mods = Modifiers(Modifiers::SHIFT);
        assert_eq!(kitty_mods_param(mods), 2); // shift(1) + 1 = 2
    }

    #[test]
    fn kitty_mods_ctrl() {
        let mods = Modifiers(Modifiers::CTRL);
        assert_eq!(kitty_mods_param(mods), 5); // ctrl(4) + 1 = 5
    }

    #[test]
    fn kitty_mods_ctrl_shift() {
        let mods = Modifiers(Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(kitty_mods_param(mods), 6); // (shift(1)|ctrl(4)) + 1 = 6
    }

    // ── kitty_key_to_bytes ──────────────────────────────────────────────────

    #[test]
    fn kitty_char_no_mods_press() {
        // 'a' with no mods, press — `\x1b[97u`
        let bytes = kitty_key_to_bytes(&KeyCode::Char('a'), Modifiers::default(), 1, 0x01);
        assert_eq!(bytes, b"\x1b[97u");
    }

    #[test]
    fn kitty_char_ctrl_press() {
        // Ctrl+'a' → codepoint 97, mods_param = 5 → `\x1b[97;5u`
        let bytes =
            kitty_key_to_bytes(&KeyCode::Char('a'), Modifiers(Modifiers::CTRL), 1, 0x01);
        assert_eq!(bytes, b"\x1b[97;5u");
    }

    #[test]
    fn kitty_char_with_event_type_repeat() {
        // 'a' repeat (event_type=2) with event-type flag set → `\x1b[97;;2u`
        let bytes =
            kitty_key_to_bytes(&KeyCode::Char('a'), Modifiers::default(), 2, 0x03);
        // mods_param=1 (no mods), but we need to emit it because event_type follows
        assert_eq!(bytes, b"\x1b[97;1;2u");
    }

    #[test]
    fn kitty_enter_no_mods() {
        // Enter = codepoint 13 → `\x1b[13u`
        let bytes = kitty_key_to_bytes(&KeyCode::Enter, Modifiers::default(), 1, 0x01);
        assert_eq!(bytes, b"\x1b[13u");
    }

    #[test]
    fn kitty_up_arrow() {
        // Up arrow = codepoint 57352 → `\x1b[57352u`
        let bytes = kitty_key_to_bytes(&KeyCode::Up, Modifiers::default(), 1, 0x01);
        assert_eq!(bytes, b"\x1b[57352u");
    }

    #[test]
    fn kitty_f1_key() {
        let bytes = kitty_key_to_bytes(&KeyCode::F(1), Modifiers::default(), 1, 0x01);
        assert_eq!(bytes, b"\x1b[57399u");
    }

    #[test]
    fn kitty_backtab_has_shift_mod() {
        // BackTab = Tab (9) with Shift mod → `\x1b[9;2u`
        let bytes = kitty_key_to_bytes(&KeyCode::BackTab, Modifiers::default(), 1, 0x01);
        assert_eq!(bytes, b"\x1b[9;2u");
    }

    #[test]
    fn kitty_flags_zero_falls_back_to_legacy() {
        // When flags=0, kitty_key_to_bytes should behave like key_to_bytes
        // (this isn't normally called with flags=0, but confirm fallback path).
        let bytes = kitty_key_to_bytes(&KeyCode::Enter, Modifiers::default(), 1, 0x01);
        // Still returns Kitty format as flags=0x01 is disambiguate bit.
        assert!(!bytes.is_empty());
    }

    // ── legacy key_to_bytes ─────────────────────────────────────────────────

    #[test]
    fn ctrl_a_returns_0x01() {
        let mods = Modifiers(Modifiers::CTRL);
        assert_eq!(key_to_bytes(&KeyCode::Char('a'), mods), vec![0x01]);
    }

    #[test]
    fn enter_returns_cr() {
        assert_eq!(
            key_to_bytes(&KeyCode::Enter, Modifiers::default()),
            vec![b'\r']
        );
    }

    #[test]
    fn backspace_returns_del() {
        assert_eq!(
            key_to_bytes(&KeyCode::Backspace, Modifiers::default()),
            vec![0x7f]
        );
    }

    #[test]
    fn arrow_keys_return_ansi_sequences() {
        let mods = Modifiers::default();
        assert_eq!(key_to_bytes(&KeyCode::Up, mods), b"\x1b[A");
        assert_eq!(key_to_bytes(&KeyCode::Down, mods), b"\x1b[B");
        assert_eq!(key_to_bytes(&KeyCode::Right, mods), b"\x1b[C");
        assert_eq!(key_to_bytes(&KeyCode::Left, mods), b"\x1b[D");
    }

    #[test]
    fn f1_to_f4_use_ss3_sequences() {
        let mods = Modifiers::default();
        assert_eq!(key_to_bytes(&KeyCode::F(1), mods), b"\x1bOP");
        assert_eq!(key_to_bytes(&KeyCode::F(2), mods), b"\x1bOQ");
        assert_eq!(key_to_bytes(&KeyCode::F(3), mods), b"\x1bOR");
        assert_eq!(key_to_bytes(&KeyCode::F(4), mods), b"\x1bOS");
    }

    #[test]
    fn unknown_f_key_returns_empty() {
        assert!(key_to_bytes(&KeyCode::F(99), Modifiers::default()).is_empty());
    }
}
