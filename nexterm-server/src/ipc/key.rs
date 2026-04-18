//! キーコード → VT100/xterm エスケープシーケンス変換

use nexterm_proto::{KeyCode, Modifiers};

/// キーコードと修飾キーを VT100/xterm エスケープシーケンスに変換する
pub(super) fn key_to_bytes(code: &KeyCode, mods: Modifiers) -> Vec<u8> {
    match code {
        KeyCode::Char(ch) => {
            if mods.is_ctrl() {
                // Ctrl+文字 → ASCII コントロールコード (1–26)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_a_returns_0x01() {
        let mods = Modifiers {
            ctrl: true,
            ..Default::default()
        };
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
