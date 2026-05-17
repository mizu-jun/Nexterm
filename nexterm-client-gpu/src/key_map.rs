//! キーマッピング — winit キーコードと nexterm_proto キーコードの相互変換

use nexterm_proto::KeyCode as ProtoKeyCode;
use winit::keyboard::{KeyCode as WKeyCode, ModifiersState, PhysicalKey};

/// winit の PhysicalKey を nexterm_proto の KeyCode に変換する
///
/// テキスト入力のない特殊キー（矢印・Fn・Ctrl+文字）のみ変換する。
/// 通常の文字入力は IME 経由で処理するため None を返す。
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
        // Ctrl+文字: text が None のケース（OS がテキストを生成しない場合）
        c if ctrl => winit_code_to_char(c).map(ProtoKeyCode::Char),
        _ => None,
    }
}

/// winit のキーコードを英小文字に変換する（Ctrl シーケンス用）
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

/// winit の ModifiersState を nexterm_proto の Modifiers に変換する
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

/// 設定キー文字列（例: "ctrl+shift+p"）と winit キーイベントを照合する
///
/// フォーマット: 修飾キー（ctrl/shift/alt/meta）と最終キー文字を `+` で区切る。
///
/// **重要**: 単発キー専用。スペース区切りのプレフィックスバインド（tmux 風 "ctrl+b d"）は
/// false を返す。prefix 系バインドは呼び出し側で第1トークン（leader）と残りトークンに
/// 分解してから本関数を 2 段階で呼ぶこと（[`input_handler::check_config_keybindings`] 参照）。
///
/// 過去のバグ: 旧実装は `split_whitespace().last()` で末尾トークンのみ評価していたため、
/// `"ctrl+b d"` バインド設定下で `d` 単独押下にもマッチして誤発火していた。
pub(crate) fn config_key_matches(key_str: &str, code: WKeyCode, mods: ModifiersState) -> bool {
    // スペース区切りのプレフィックスバインドは本関数では扱わない（呼び出し側で分解する）
    if key_str.split_whitespace().count() > 1 {
        return false;
    }
    let token = key_str.trim();
    if token.is_empty() {
        return false;
    }
    config_key_matches_token(token, code, mods)
}

/// `+` 区切りの単一キー仕様（例: "ctrl+shift+p"）を照合する内部関数。
/// スペースを含まない前提（呼び出し側で保証）。
pub(crate) fn config_key_matches_token(token: &str, code: WKeyCode, mods: ModifiersState) -> bool {
    // `+` で分割して修飾キーとメインキーを取得する
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
        .expect("parts は split() で少なくとも1要素ある");

    for part in &parts[..parts.len() - 1] {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => need_ctrl = true,
            "shift" => need_shift = true,
            "alt" | "option" => need_alt = true,
            "meta" | "super" | "cmd" | "command" => need_meta = true,
            _ => {}
        }
    }

    // 修飾キーが一致しなければ false
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

    // メインキー文字列を winit KeyCode に変換して比較する
    key_str_to_keycode(main_key) == Some(code)
}

/// キー文字列を winit の KeyCode に変換する（簡易実装）
pub(crate) fn key_str_to_keycode(s: &str) -> Option<WKeyCode> {
    // 1 文字の場合は英数字として処理する
    if s.len() == 1 {
        let ch = s.chars().next().expect("s.len() == 1 なので必ず1文字ある");
        return char_to_keycode(ch);
    }
    // 特殊キー名
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

/// 1文字を winit の KeyCode に変換する
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

    // === 単発バインドの正常系 ===

    #[test]
    fn 単発_ctrl_shift_p_は_ctrlshift_p_で_true() {
        assert!(config_key_matches(
            "ctrl+shift+p",
            WKeyCode::KeyP,
            mods_ctrl_shift()
        ));
    }

    #[test]
    fn 単発_ctrl_b_は_ctrl_b_で_true() {
        assert!(config_key_matches("ctrl+b", WKeyCode::KeyB, mods_ctrl()));
    }

    #[test]
    fn 単発_ctrl_b_は_b_単独_で_false() {
        assert!(!config_key_matches("ctrl+b", WKeyCode::KeyB, mods_none()));
    }

    // === バグ回帰防止: スペース区切りは常に false ===

    #[test]
    fn prefix_バインド_ctrl_b_d_は_d_単独_で_false() {
        // 旧バグ: split_whitespace().last() で "d" だけ評価して true を返していた
        assert!(!config_key_matches("ctrl+b d", WKeyCode::KeyD, mods_none()));
    }

    #[test]
    fn prefix_バインド_ctrl_b_pct_は_5_単独_で_false() {
        // 旧バグ: "%" → Digit5 で 5 単独押下にマッチしていた
        assert!(!config_key_matches(
            "ctrl+b %",
            WKeyCode::Digit5,
            mods_none()
        ));
    }

    #[test]
    fn prefix_バインド_ctrl_b_d_は_ctrl_b_押下_でも_false() {
        // prefix モード突入のキーには別ロジックで対応するため、本関数は常に false
        assert!(!config_key_matches("ctrl+b d", WKeyCode::KeyB, mods_ctrl()));
    }

    #[test]
    fn prefix_バインド_leader_d_は_d_単独_で_false() {
        // <leader> 展開後の文字列も同様
        assert!(!config_key_matches("ctrl+b d", WKeyCode::KeyD, mods_none()));
    }

    // === エッジケース ===

    #[test]
    fn 空文字列は_false() {
        assert!(!config_key_matches("", WKeyCode::KeyA, mods_none()));
    }

    #[test]
    fn 空白のみは_false() {
        assert!(!config_key_matches("   ", WKeyCode::KeyA, mods_none()));
    }

    #[test]
    fn 修飾子のミスマッチは_false() {
        // shift が要求されているが押されていない
        assert!(!config_key_matches(
            "ctrl+shift+p",
            WKeyCode::KeyP,
            mods_ctrl()
        ));
    }

    #[test]
    fn 余分な修飾子は_false() {
        // ctrl+p バインドに対して Ctrl+Shift+p を押した場合
        assert!(!config_key_matches(
            "ctrl+p",
            WKeyCode::KeyP,
            mods_ctrl_shift()
        ));
    }

    // === 内部関数 config_key_matches_token のスペース不可前提を確認 ===

    #[test]
    fn token_関数は_p_と_ctrl_p_で_true() {
        assert!(config_key_matches_token(
            "ctrl+p",
            WKeyCode::KeyP,
            mods_ctrl()
        ));
    }

    #[test]
    fn token_関数は_前後空白を含むと_main_key_変換失敗で_false() {
        // 呼び出し側で trim 済みを前提にしているため、空白入りの token は意図的に通さない
        assert!(!config_key_matches_token(
            " ctrl+p ",
            WKeyCode::KeyP,
            mods_ctrl()
        ));
    }
}
