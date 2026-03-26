//! キー入力処理 — crossterm イベントを nexterm-proto のメッセージに変換する

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode as CKC, KeyEventKind, KeyModifiers};

use nexterm_proto::{ClientToServer, KeyCode, Modifiers};

/// UI アクション（イベントループへ返す）
pub enum Action {
    /// アプリケーション終了
    Quit,
    /// サーバーへ送るキーイベント
    SendKey(ClientToServer),
    /// 端末リサイズ
    Resize(u16, u16),
}

/// キー入力を non-blocking でポーリングする
pub fn poll_input() -> Result<Option<Action>> {
    if !event::poll(std::time::Duration::from_millis(0))? {
        return Ok(None);
    }

    match event::read()? {
        Event::Key(key_event) => {
            // key press のみ処理する（release/repeat は無視）
            if key_event.kind != KeyEventKind::Press {
                return Ok(None);
            }

            let modifiers = convert_modifiers(key_event.modifiers);

            // Ctrl+Q で終了
            if modifiers.0 & Modifiers::CTRL != 0 {
                if let CKC::Char('q') = key_event.code {
                    return Ok(Some(Action::Quit));
                }
            }

            // キーコードを変換する
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

/// crossterm の KeyModifiers を nexterm の Modifiers に変換する
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

/// crossterm の KeyCode を nexterm の KeyCode に変換する
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
    fn ctrl修飾キーが正しく変換される() {
        let m = KeyModifiers::CONTROL;
        let converted = convert_modifiers(m);
        assert!(converted.0 & Modifiers::CTRL != 0);
        assert!(converted.0 & Modifiers::SHIFT == 0);
    }

    #[test]
    fn エンターキーが変換される() {
        let code = convert_key_code(CKC::Enter);
        assert!(matches!(code, Some(KeyCode::Enter)));
    }

    #[test]
    fn 通常文字が変換される() {
        let code = convert_key_code(CKC::Char('a'));
        assert!(matches!(code, Some(KeyCode::Char('a'))));
    }
}
