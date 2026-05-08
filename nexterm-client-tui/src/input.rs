//! キー入力処理 — crossterm イベントを nexterm-proto のメッセージに変換する

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode as CKC, KeyEventKind, KeyModifiers};

use nexterm_proto::{ClientToServer, KeyCode, Modifiers};

use crate::state::PrefixMode;

/// UI アクション（イベントループへ返す）
pub enum Action {
    /// アプリケーション終了
    Quit,
    /// サーバーへ送るキーイベント
    SendKey(ClientToServer),
    /// 端末リサイズ
    Resize(u16, u16),
    /// Ctrl+B プレフィックスモードを開始する
    EnterPrefix,
    /// プレフィックスモードのセカンドキーを処理してアクションを返す
    PrefixCommand(ClientToServer),
    /// ヘルプオーバーレイのトグル
    ToggleHelp,
    /// プレフィックスモードをキャンセルする（Esc）
    CancelPrefix,
}

/// キー入力を non-blocking でポーリングする（プレフィックスモードを考慮する）
pub fn poll_input(prefix_mode: PrefixMode) -> Result<Option<Action>> {
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

            match prefix_mode {
                PrefixMode::Help => {
                    // ヘルプ表示中は任意のキーで閉じる
                    return Ok(Some(Action::ToggleHelp));
                }
                PrefixMode::CtrlB => {
                    // プレフィックスモード：セカンドキーを処理する
                    return handle_prefix_key(key_event.code, modifiers);
                }
                PrefixMode::None => {
                    // 通常モード
                }
            }

            // Ctrl+Q で終了
            if modifiers.is_ctrl() {
                if let CKC::Char('q') = key_event.code {
                    return Ok(Some(Action::Quit));
                }
                // Ctrl+B でプレフィックスモードへ
                if let CKC::Char('b') = key_event.code {
                    return Ok(Some(Action::EnterPrefix));
                }
            }

            // Esc でヘルプ/プレフィックスキャンセル（通常モードでは無視）
            if key_event.code == CKC::Esc {
                return Ok(None);
            }

            // キーコードを変換してサーバーへ送る
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

/// Ctrl+B プレフィックス後のセカンドキーを処理する
fn handle_prefix_key(code: CKC, _modifiers: Modifiers) -> Result<Option<Action>> {
    let action = match code {
        // % → 垂直分割
        CKC::Char('%') => Action::PrefixCommand(ClientToServer::SplitVertical),
        // " → 水平分割
        CKC::Char('"') => Action::PrefixCommand(ClientToServer::SplitHorizontal),
        // x → フォーカスペインを閉じる
        CKC::Char('x') => Action::PrefixCommand(ClientToServer::ClosePane),
        // n → 次のペイン
        CKC::Char('n') => Action::PrefixCommand(ClientToServer::FocusNextPane),
        // p → 前のペイン
        CKC::Char('p') => Action::PrefixCommand(ClientToServer::FocusPrevPane),
        // z → ズームトグル
        CKC::Char('z') => Action::PrefixCommand(ClientToServer::ToggleZoom),
        // ? → ヘルプ
        CKC::Char('?') => Action::ToggleHelp,
        // Esc → プレフィックスキャンセル
        CKC::Esc => Action::CancelPrefix,
        // それ以外 → プレフィックスキャンセル（不明なキーは無視）
        _ => Action::CancelPrefix,
    };
    Ok(Some(action))
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
        assert!(converted.is_ctrl());
        assert!(!converted.is_shift());
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

    #[test]
    fn プレフィックスキー_縦分割() {
        let result = handle_prefix_key(CKC::Char('%'), Modifiers(0)).unwrap();
        assert!(matches!(
            result,
            Some(Action::PrefixCommand(ClientToServer::SplitVertical))
        ));
    }

    #[test]
    fn プレフィックスキー_横分割() {
        let result = handle_prefix_key(CKC::Char('"'), Modifiers(0)).unwrap();
        assert!(matches!(
            result,
            Some(Action::PrefixCommand(ClientToServer::SplitHorizontal))
        ));
    }

    #[test]
    fn プレフィックスキー_閉じる() {
        let result = handle_prefix_key(CKC::Char('x'), Modifiers(0)).unwrap();
        assert!(matches!(
            result,
            Some(Action::PrefixCommand(ClientToServer::ClosePane))
        ));
    }

    #[test]
    fn プレフィックスキー_ヘルプ() {
        let result = handle_prefix_key(CKC::Char('?'), Modifiers(0)).unwrap();
        assert!(matches!(result, Some(Action::ToggleHelp)));
    }

    #[test]
    fn プレフィックスキー_esc_キャンセル() {
        let result = handle_prefix_key(CKC::Esc, Modifiers(0)).unwrap();
        assert!(matches!(result, Some(Action::CancelPrefix)));
    }

    #[test]
    fn プレフィックスキー_未知はキャンセル() {
        let result = handle_prefix_key(CKC::Char('Z'), Modifiers(0)).unwrap();
        assert!(matches!(result, Some(Action::CancelPrefix)));
    }
}
