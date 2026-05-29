//! Key input handling — converts crossterm events into `nexterm-proto` messages.

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode as CKC, KeyEventKind, KeyModifiers};

use nexterm_proto::{ClientToServer, KeyCode, Modifiers};

use crate::state::PrefixMode;

/// UI action returned to the event loop.
pub enum Action {
    /// Quit the application.
    Quit,
    /// Key event to forward to the server.
    SendKey(ClientToServer),
    /// Terminal resize.
    Resize(u16, u16),
    /// Enter the `Ctrl+B` prefix mode.
    EnterPrefix,
    /// Second key while in prefix mode produced an action.
    PrefixCommand(ClientToServer),
    /// Toggle the help overlay.
    ToggleHelp,
    /// Cancel prefix mode (Esc).
    CancelPrefix,
}

/// Non-blocking poll of the next key event (the current prefix mode is honored).
pub fn poll_input(prefix_mode: PrefixMode) -> Result<Option<Action>> {
    if !event::poll(std::time::Duration::from_millis(0))? {
        return Ok(None);
    }

    match event::read()? {
        Event::Key(key_event) => {
            // Handle key presses only (ignore release/repeat events).
            if key_event.kind != KeyEventKind::Press {
                return Ok(None);
            }

            let modifiers = convert_modifiers(key_event.modifiers);

            match prefix_mode {
                PrefixMode::Help => {
                    // Any key closes the help overlay.
                    return Ok(Some(Action::ToggleHelp));
                }
                PrefixMode::CtrlB => {
                    // Prefix mode: process the second keystroke.
                    return handle_prefix_key(key_event.code, modifiers);
                }
                PrefixMode::None => {
                    // Normal mode.
                }
            }

            // Ctrl+Q quits.
            if modifiers.is_ctrl() {
                if let CKC::Char('q') = key_event.code {
                    return Ok(Some(Action::Quit));
                }
                // Ctrl+B enters prefix mode.
                if let CKC::Char('b') = key_event.code {
                    return Ok(Some(Action::EnterPrefix));
                }
            }

            // Esc cancels help/prefix mode (ignored in normal mode here).
            if key_event.code == CKC::Esc {
                return Ok(None);
            }

            // Convert and forward the key event to the server.
            if let Some(code) = convert_key_code(key_event.code) {
                return Ok(Some(Action::SendKey(ClientToServer::KeyEvent {
                    code,
                    modifiers,
                })));
            }
        }
        Event::Resize(cols, rows) => {
            return Ok(Some(Action::Resize(cols, rows)));
        }
        _ => {}
    }

    Ok(None)
}

/// Handle the second keystroke following the `Ctrl+B` prefix.
fn handle_prefix_key(code: CKC, _modifiers: Modifiers) -> Result<Option<Action>> {
    let action = match code {
        // % → vertical split
        CKC::Char('%') => Action::PrefixCommand(ClientToServer::SplitVertical),
        // " → horizontal split
        CKC::Char('"') => Action::PrefixCommand(ClientToServer::SplitHorizontal),
        // x → close focused pane
        CKC::Char('x') => Action::PrefixCommand(ClientToServer::ClosePane),
        // n → next pane
        CKC::Char('n') => Action::PrefixCommand(ClientToServer::FocusNextPane),
        // p → previous pane
        CKC::Char('p') => Action::PrefixCommand(ClientToServer::FocusPrevPane),
        // z → toggle zoom
        CKC::Char('z') => Action::PrefixCommand(ClientToServer::ToggleZoom),
        // ? → toggle help
        CKC::Char('?') => Action::ToggleHelp,
        // Esc → cancel prefix mode
        CKC::Esc => Action::CancelPrefix,
        // Anything else → cancel prefix mode (unknown keys are ignored).
        _ => Action::CancelPrefix,
    };
    Ok(Some(action))
}

/// Convert crossterm `KeyModifiers` into nexterm `Modifiers`.
fn convert_modifiers(m: KeyModifiers) -> Modifiers {
    let mut bits: u8 = 0;
    if m.contains(KeyModifiers::SHIFT) {
        bits |= Modifiers::SHIFT;
    }
    if m.contains(KeyModifiers::CONTROL) {
        bits |= Modifiers::CTRL;
    }
    if m.contains(KeyModifiers::ALT) {
        bits |= Modifiers::ALT;
    }
    Modifiers(bits)
}

/// Convert a crossterm `KeyCode` into a nexterm `KeyCode`.
fn convert_key_code(code: CKC) -> Option<KeyCode> {
    let k = match code {
        CKC::Char(c) => KeyCode::Char(c),
        CKC::F(n) => KeyCode::F(n),
        CKC::Enter => KeyCode::Enter,
        CKC::Backspace => KeyCode::Backspace,
        CKC::Delete => KeyCode::Delete,
        CKC::Esc => KeyCode::Escape,
        CKC::Tab => KeyCode::Tab,
        CKC::BackTab => KeyCode::BackTab,
        CKC::Up => KeyCode::Up,
        CKC::Down => KeyCode::Down,
        CKC::Left => KeyCode::Left,
        CKC::Right => KeyCode::Right,
        CKC::Home => KeyCode::Home,
        CKC::End => KeyCode::End,
        CKC::PageUp => KeyCode::PageUp,
        CKC::PageDown => KeyCode::PageDown,
        CKC::Insert => KeyCode::Insert,
        _ => return None,
    };
    Some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_modifier_converts_correctly() {
        let m = KeyModifiers::CONTROL;
        let converted = convert_modifiers(m);
        assert!(converted.is_ctrl());
        assert!(!converted.is_shift());
    }

    #[test]
    fn enter_key_converts() {
        let code = convert_key_code(CKC::Enter);
        assert!(matches!(code, Some(KeyCode::Enter)));
    }

    #[test]
    fn regular_char_converts() {
        let code = convert_key_code(CKC::Char('a'));
        assert!(matches!(code, Some(KeyCode::Char('a'))));
    }

    #[test]
    fn prefix_key_vertical_split() {
        let result = handle_prefix_key(CKC::Char('%'), Modifiers(0)).unwrap();
        assert!(matches!(
            result,
            Some(Action::PrefixCommand(ClientToServer::SplitVertical))
        ));
    }

    #[test]
    fn prefix_key_horizontal_split() {
        let result = handle_prefix_key(CKC::Char('"'), Modifiers(0)).unwrap();
        assert!(matches!(
            result,
            Some(Action::PrefixCommand(ClientToServer::SplitHorizontal))
        ));
    }

    #[test]
    fn prefix_key_close() {
        let result = handle_prefix_key(CKC::Char('x'), Modifiers(0)).unwrap();
        assert!(matches!(
            result,
            Some(Action::PrefixCommand(ClientToServer::ClosePane))
        ));
    }

    #[test]
    fn prefix_key_help() {
        let result = handle_prefix_key(CKC::Char('?'), Modifiers(0)).unwrap();
        assert!(matches!(result, Some(Action::ToggleHelp)));
    }

    #[test]
    fn prefix_key_esc_cancels() {
        let result = handle_prefix_key(CKC::Esc, Modifiers(0)).unwrap();
        assert!(matches!(result, Some(Action::CancelPrefix)));
    }

    #[test]
    fn prefix_key_unknown_cancels() {
        let result = handle_prefix_key(CKC::Char('Z'), Modifiers(0)).unwrap();
        assert!(matches!(result, Some(Action::CancelPrefix)));
    }
}
