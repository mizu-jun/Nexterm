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

/// 設定キー文字列（例: "ctrl+shift+p", "ctrl+b d"）と winit キーイベントを照合する
///
/// フォーマット: 修飾キー（ctrl/shift/alt/meta）と最終キー文字を `+` で区切る。
/// スペース区切りのプレフィックス（tmux 風: "ctrl+b d"）は先頭の修飾シーケンス + 末尾の単一文字として扱う。
pub(crate) fn config_key_matches(key_str: &str, code: WKeyCode, mods: ModifiersState) -> bool {
    // スペース区切りで最後のトークンをメインキーとして扱う（tmux プレフィックス互換は未実装 → 最後トークンのみ比較）
    let last_token = key_str.split_whitespace().last().unwrap_or(key_str);

    // `+` で分割して修飾キーとメインキーを取得する
    let parts: Vec<&str> = last_token.split('+').collect();
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
