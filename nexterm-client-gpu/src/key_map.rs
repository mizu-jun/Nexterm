//! Key mapping — converts between winit key codes and `nexterm_proto` key codes.

use nexterm_proto::KeyCode as ProtoKeyCode;
use winit::keyboard::{KeyCode as WKeyCode, ModifiersState, PhysicalKey};

/// Convert a winit `PhysicalKey` into a `nexterm_proto::KeyCode`.
///
/// Only special keys without text input (arrows, function keys, Ctrl+letter) are
/// translated. Regular character input is handled through the IME, so this
/// returns `None` for it.
pub(crate) fn physical_to_proto_key(
    key: PhysicalKey,
    mods: ModifiersState,
) -> Option<ProtoKeyCode> {
    let ctrl = mods.control_key();
    let PhysicalKey::Code(code) = key else {
        return None;
    };

    match code {
        WKeyCode::Enter => Some(ProtoKeyCode::Enter),
        WKeyCode::Backspace => Some(ProtoKeyCode::Backspace),
        WKeyCode::Delete => Some(ProtoKeyCode::Delete),
        WKeyCode::Escape => Some(ProtoKeyCode::Escape),
        WKeyCode::Tab => {
            if mods.shift_key() {
                Some(ProtoKeyCode::BackTab)
            } else {
                Some(ProtoKeyCode::Tab)
            }
        }
        WKeyCode::ArrowUp => Some(ProtoKeyCode::Up),
        WKeyCode::ArrowDown => Some(ProtoKeyCode::Down),
        WKeyCode::ArrowLeft => Some(ProtoKeyCode::Left),
        WKeyCode::ArrowRight => Some(ProtoKeyCode::Right),
        WKeyCode::Home => Some(ProtoKeyCode::Home),
        WKeyCode::End => Some(ProtoKeyCode::End),
        WKeyCode::PageUp => Some(ProtoKeyCode::PageUp),
        WKeyCode::PageDown => Some(ProtoKeyCode::PageDown),
        WKeyCode::Insert => Some(ProtoKeyCode::Insert),
        WKeyCode::F1 => Some(ProtoKeyCode::F(1)),
        WKeyCode::F2 => Some(ProtoKeyCode::F(2)),
        WKeyCode::F3 => Some(ProtoKeyCode::F(3)),
        WKeyCode::F4 => Some(ProtoKeyCode::F(4)),
        WKeyCode::F5 => Some(ProtoKeyCode::F(5)),
        WKeyCode::F6 => Some(ProtoKeyCode::F(6)),
        WKeyCode::F7 => Some(ProtoKeyCode::F(7)),
        WKeyCode::F8 => Some(ProtoKeyCode::F(8)),
        WKeyCode::F9 => Some(ProtoKeyCode::F(9)),
        WKeyCode::F10 => Some(ProtoKeyCode::F(10)),
        WKeyCode::F11 => Some(ProtoKeyCode::F(11)),
        WKeyCode::F12 => Some(ProtoKeyCode::F(12)),
        // Ctrl+letter: when `text` is None (the OS does not generate text).
        c if ctrl => winit_code_to_char(c).map(ProtoKeyCode::Char),
        _ => None,
    }
}

/// Convert a winit key code into a lowercase ASCII letter (for Ctrl sequences).
pub(crate) fn winit_code_to_char(code: WKeyCode) -> Option<char> {
    match code {
        WKeyCode::KeyA => Some('a'),
        WKeyCode::KeyB => Some('b'),
        WKeyCode::KeyC => Some('c'),
        WKeyCode::KeyD => Some('d'),
        WKeyCode::KeyE => Some('e'),
        WKeyCode::KeyF => Some('f'),
        WKeyCode::KeyG => Some('g'),
        WKeyCode::KeyH => Some('h'),
        WKeyCode::KeyI => Some('i'),
        WKeyCode::KeyJ => Some('j'),
        WKeyCode::KeyK => Some('k'),
        WKeyCode::KeyL => Some('l'),
        WKeyCode::KeyM => Some('m'),
        WKeyCode::KeyN => Some('n'),
        WKeyCode::KeyO => Some('o'),
        WKeyCode::KeyP => Some('p'),
        WKeyCode::KeyQ => Some('q'),
        WKeyCode::KeyR => Some('r'),
        WKeyCode::KeyS => Some('s'),
        WKeyCode::KeyT => Some('t'),
        WKeyCode::KeyU => Some('u'),
        WKeyCode::KeyV => Some('v'),
        WKeyCode::KeyW => Some('w'),
        WKeyCode::KeyX => Some('x'),
        WKeyCode::KeyY => Some('y'),
        WKeyCode::KeyZ => Some('z'),
        _ => None,
    }
}

/// Convert winit `ModifiersState` into `nexterm_proto::Modifiers`.
pub(crate) fn proto_modifiers(state: ModifiersState) -> nexterm_proto::Modifiers {
    let mut bits = 0u8;
    if state.shift_key() {
        bits |= nexterm_proto::Modifiers::SHIFT;
    }
    if state.control_key() {
        bits |= nexterm_proto::Modifiers::CTRL;
    }
    if state.alt_key() {
        bits |= nexterm_proto::Modifiers::ALT;
    }
    if state.super_key() {
        bits |= nexterm_proto::Modifiers::META;
    }
    nexterm_proto::Modifiers(bits)
}

/// Match a config keybinding string (e.g. `"ctrl+shift+p"`) against a winit key event.
///
/// Format: zero or more modifier names (`ctrl`/`shift`/`alt`/`meta`) followed by a
/// trailing key, all separated by `+`.
///
/// **Important**: single-binding only. Space-separated prefix bindings (tmux-style
/// `"ctrl+b d"`) return `false`. Prefix bindings must be split by the caller into
/// the first token (leader) and the remaining tokens, then matched in two stages
/// (see [`input_handler::check_config_keybindings`]).
///
/// Historical bug: the old implementation evaluated only `split_whitespace().last()`,
/// which meant `"ctrl+b d"` would incorrectly match a bare `d` press.
pub(crate) fn config_key_matches(key_str: &str, code: WKeyCode, mods: ModifiersState) -> bool {
    // Space-separated prefix bindings are out of scope here (the caller splits them).
    if key_str.split_whitespace().count() > 1 {
        return false;
    }
    let token = key_str.trim();
    if token.is_empty() {
        return false;
    }
    config_key_matches_token(token, code, mods)
}

/// Internal helper: match a single `+`-delimited key spec (e.g. `"ctrl+shift+p"`).
/// Assumes the input does not contain whitespace (the caller guarantees this).
pub(crate) fn config_key_matches_token(token: &str, code: WKeyCode, mods: ModifiersState) -> bool {
    // Split on `+` to separate the modifiers from the main key.
    let parts: Vec<&str> = token.split('+').collect();
    if parts.is_empty() {
        return false;
    }

    let mut need_ctrl = false;
    let mut need_shift = false;
    let mut need_alt = false;
    let mut need_meta = false;
    let main_key = parts
        .last()
        .expect("split() always yields at least one element");

    for part in &parts[..parts.len() - 1] {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => need_ctrl = true,
            "shift" => need_shift = true,
            "alt" | "option" => need_alt = true,
            "meta" | "super" | "cmd" | "command" => need_meta = true,
            _ => {}
        }
    }

    // Modifier mismatch → no match.
    if need_ctrl != mods.control_key() {
        return false;
    }
    if need_shift != mods.shift_key() {
        return false;
    }
    if need_alt != mods.alt_key() {
        return false;
    }
    if need_meta != mods.super_key() {
        return false;
    }

    // Compare the main key string against the winit KeyCode.
    key_str_to_keycode(main_key) == Some(code)
}

/// Convert a key name string into a winit `KeyCode` (simple implementation).
pub(crate) fn key_str_to_keycode(s: &str) -> Option<WKeyCode> {
    // Single-character inputs are treated as alphanumerics.
    if s.len() == 1 {
        let ch = s.chars().next().expect("s.len() == 1 guarantees one char");
        return char_to_keycode(ch);
    }
    // Special key names.
    match s.to_lowercase().as_str() {
        "enter" | "return" => Some(WKeyCode::Enter),
        "backspace" => Some(WKeyCode::Backspace),
        "delete" | "del" => Some(WKeyCode::Delete),
        "escape" | "esc" => Some(WKeyCode::Escape),
        "tab" => Some(WKeyCode::Tab),
        "space" => Some(WKeyCode::Space),
        "up" => Some(WKeyCode::ArrowUp),
        "down" => Some(WKeyCode::ArrowDown),
        "left" => Some(WKeyCode::ArrowLeft),
        "right" => Some(WKeyCode::ArrowRight),
        "home" => Some(WKeyCode::Home),
        "end" => Some(WKeyCode::End),
        "pageup" => Some(WKeyCode::PageUp),
        "pagedown" => Some(WKeyCode::PageDown),
        "insert" => Some(WKeyCode::Insert),
        "f1" => Some(WKeyCode::F1),
        "f2" => Some(WKeyCode::F2),
        "f3" => Some(WKeyCode::F3),
        "f4" => Some(WKeyCode::F4),
        "f5" => Some(WKeyCode::F5),
        "f6" => Some(WKeyCode::F6),
        "f7" => Some(WKeyCode::F7),
        "f8" => Some(WKeyCode::F8),
        "f9" => Some(WKeyCode::F9),
        "f10" => Some(WKeyCode::F10),
        "f11" => Some(WKeyCode::F11),
        "f12" => Some(WKeyCode::F12),
        _ => None,
    }
}

/// Convert a single character into a winit `KeyCode`.
pub(crate) fn char_to_keycode(ch: char) -> Option<WKeyCode> {
    match ch {
        'a' | 'A' => Some(WKeyCode::KeyA),
        'b' | 'B' => Some(WKeyCode::KeyB),
        'c' | 'C' => Some(WKeyCode::KeyC),
        'd' | 'D' => Some(WKeyCode::KeyD),
        'e' | 'E' => Some(WKeyCode::KeyE),
        'f' | 'F' => Some(WKeyCode::KeyF),
        'g' | 'G' => Some(WKeyCode::KeyG),
        'h' | 'H' => Some(WKeyCode::KeyH),
        'i' | 'I' => Some(WKeyCode::KeyI),
        'j' | 'J' => Some(WKeyCode::KeyJ),
        'k' | 'K' => Some(WKeyCode::KeyK),
        'l' | 'L' => Some(WKeyCode::KeyL),
        'm' | 'M' => Some(WKeyCode::KeyM),
        'n' | 'N' => Some(WKeyCode::KeyN),
        'o' | 'O' => Some(WKeyCode::KeyO),
        'p' | 'P' => Some(WKeyCode::KeyP),
        'q' | 'Q' => Some(WKeyCode::KeyQ),
        'r' | 'R' => Some(WKeyCode::KeyR),
        's' | 'S' => Some(WKeyCode::KeyS),
        't' | 'T' => Some(WKeyCode::KeyT),
        'u' | 'U' => Some(WKeyCode::KeyU),
        'v' | 'V' => Some(WKeyCode::KeyV),
        'w' | 'W' => Some(WKeyCode::KeyW),
        'x' | 'X' => Some(WKeyCode::KeyX),
        'y' | 'Y' => Some(WKeyCode::KeyY),
        'z' | 'Z' => Some(WKeyCode::KeyZ),
        '0' => Some(WKeyCode::Digit0),
        '1' => Some(WKeyCode::Digit1),
        '2' => Some(WKeyCode::Digit2),
        '3' => Some(WKeyCode::Digit3),
        '4' => Some(WKeyCode::Digit4),
        '5' => Some(WKeyCode::Digit5),
        '6' => Some(WKeyCode::Digit6),
        '7' => Some(WKeyCode::Digit7),
        '8' => Some(WKeyCode::Digit8),
        '9' => Some(WKeyCode::Digit9),
        '%' => Some(WKeyCode::Digit5), // Shift+5 = %
        '"' => Some(WKeyCode::Quote),
        '\'' => Some(WKeyCode::Quote),
        '[' => Some(WKeyCode::BracketLeft),
        ']' => Some(WKeyCode::BracketRight),
        '\\' => Some(WKeyCode::Backslash),
        '/' => Some(WKeyCode::Slash),
        '-' => Some(WKeyCode::Minus),
        '=' => Some(WKeyCode::Equal),
        ',' => Some(WKeyCode::Comma),
        '.' => Some(WKeyCode::Period),
        ';' => Some(WKeyCode::Semicolon),
        '`' => Some(WKeyCode::Backquote),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{KeyCode as WKeyCode, ModifiersState};

    fn mods_none() -> ModifiersState {
        ModifiersState::empty()
    }

    fn mods_ctrl() -> ModifiersState {
        ModifiersState::CONTROL
    }

    fn mods_ctrl_shift() -> ModifiersState {
        ModifiersState::CONTROL | ModifiersState::SHIFT
    }

    // === Single-binding happy paths ===

    #[test]
    fn single_ctrl_shift_p_matches_ctrl_shift_p_press() {
        assert!(config_key_matches(
            "ctrl+shift+p",
            WKeyCode::KeyP,
            mods_ctrl_shift()
        ));
    }

    #[test]
    fn single_ctrl_b_matches_ctrl_b_press() {
        assert!(config_key_matches("ctrl+b", WKeyCode::KeyB, mods_ctrl()));
    }

    #[test]
    fn single_ctrl_b_does_not_match_bare_b_press() {
        assert!(!config_key_matches("ctrl+b", WKeyCode::KeyB, mods_none()));
    }

    // === Regression guard: space-separated bindings always return false ===

    #[test]
    fn prefix_binding_ctrl_b_d_does_not_match_bare_d() {
        // Old bug: split_whitespace().last() evaluated only "d" and returned true.
        assert!(!config_key_matches("ctrl+b d", WKeyCode::KeyD, mods_none()));
    }

    #[test]
    fn prefix_binding_ctrl_b_pct_does_not_match_bare_5() {
        // Old bug: '%' → Digit5 matched a bare 5 press.
        assert!(!config_key_matches(
            "ctrl+b %",
            WKeyCode::Digit5,
            mods_none()
        ));
    }

    #[test]
    fn prefix_binding_ctrl_b_d_does_not_match_ctrl_b() {
        // Entering prefix mode is handled by a separate code path, so this returns false.
        assert!(!config_key_matches("ctrl+b d", WKeyCode::KeyB, mods_ctrl()));
    }

    #[test]
    fn prefix_binding_leader_d_does_not_match_bare_d() {
        // The same applies to <leader>-expanded strings.
        assert!(!config_key_matches("ctrl+b d", WKeyCode::KeyD, mods_none()));
    }

    // === Edge cases ===

    #[test]
    fn empty_string_returns_false() {
        assert!(!config_key_matches("", WKeyCode::KeyA, mods_none()));
    }

    #[test]
    fn whitespace_only_returns_false() {
        assert!(!config_key_matches("   ", WKeyCode::KeyA, mods_none()));
    }

    #[test]
    fn modifier_mismatch_returns_false() {
        // Shift is required but not pressed.
        assert!(!config_key_matches(
            "ctrl+shift+p",
            WKeyCode::KeyP,
            mods_ctrl()
        ));
    }

    #[test]
    fn extra_modifier_returns_false() {
        // Pressing Ctrl+Shift+p when the binding is just `ctrl+p`.
        assert!(!config_key_matches(
            "ctrl+p",
            WKeyCode::KeyP,
            mods_ctrl_shift()
        ));
    }

    // === Verify config_key_matches_token assumes no whitespace ===

    #[test]
    fn token_helper_matches_ctrl_p_when_ctrl_p_pressed() {
        assert!(config_key_matches_token(
            "ctrl+p",
            WKeyCode::KeyP,
            mods_ctrl()
        ));
    }

    #[test]
    fn token_helper_returns_false_when_main_key_has_surrounding_spaces() {
        // The caller is expected to trim() before invoking; whitespace-laden tokens
        // are intentionally rejected here.
        assert!(!config_key_matches_token(
            " ctrl+p ",
            WKeyCode::KeyP,
            mods_ctrl()
        ));
    }
}
