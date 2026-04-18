//! IPC プロトコルのエンコード・フレーミング統合テスト
//!
//! bincode シリアライズ + 4 バイト LE 長さプレフィックスフレーミングを検証する。
//! サーバープロセスを起動せずにプロトコル仕様の整合性を確認する。

use nexterm_proto::{ClientToServer, KeyCode, Modifiers, ServerToClient};

// ── bincode ラウンドトリップ ─────────────────────────────────────────────────

#[test]
fn test_ping_encode_decode() {
    let msg = ClientToServer::Ping;
    let encoded = bincode::serialize(&msg).expect("Ping のシリアライズ失敗");
    let decoded: ClientToServer =
        bincode::deserialize(&encoded).expect("Ping のデシリアライズ失敗");
    assert!(matches!(decoded, ClientToServer::Ping));
}

#[test]
fn test_pong_encode_decode() {
    let msg = ServerToClient::Pong;
    let encoded = bincode::serialize(&msg).expect("Pong のシリアライズ失敗");
    let decoded: ServerToClient =
        bincode::deserialize(&encoded).expect("Pong のデシリアライズ失敗");
    assert!(matches!(decoded, ServerToClient::Pong));
}

#[test]
fn test_attach_roundtrip() {
    let msg = ClientToServer::Attach {
        session_name: "test-session".to_string(),
    };

    let encoded = bincode::serialize(&msg).expect("Attach のシリアライズ失敗");
    let decoded: ClientToServer = bincode::deserialize(&encoded).expect("デシリアライズ失敗");

    match decoded {
        ClientToServer::Attach { session_name } => {
            assert_eq!(session_name, "test-session");
        }
        _ => panic!("Attach であるべき"),
    }
}

#[test]
fn test_paste_text_roundtrip() {
    let msg = ClientToServer::PasteText {
        text: "hello world".to_string(),
    };

    let encoded = bincode::serialize(&msg).expect("PasteText のシリアライズ失敗");
    let decoded: ClientToServer = bincode::deserialize(&encoded).expect("デシリアライズ失敗");

    match decoded {
        ClientToServer::PasteText { text } => {
            assert_eq!(text, "hello world");
        }
        _ => panic!("PasteText であるべき"),
    }
}

#[test]
fn test_list_sessions_roundtrip() {
    let msg = ClientToServer::ListSessions;
    let encoded = bincode::serialize(&msg).expect("ListSessions のシリアライズ失敗");
    let decoded: ClientToServer = bincode::deserialize(&encoded).expect("デシリアライズ失敗");
    assert!(matches!(decoded, ClientToServer::ListSessions));
}

#[test]
fn test_key_event_roundtrip() {
    let msg = ClientToServer::KeyEvent {
        code: KeyCode::Char('a'),
        modifiers: Modifiers(Modifiers::CTRL),
    };

    let encoded = bincode::serialize(&msg).expect("KeyEvent のシリアライズ失敗");
    let decoded: ClientToServer = bincode::deserialize(&encoded).expect("デシリアライズ失敗");

    match decoded {
        ClientToServer::KeyEvent { code, modifiers } => {
            assert_eq!(code, KeyCode::Char('a'));
            assert!(modifiers.is_ctrl());
            assert!(!modifiers.is_shift());
        }
        _ => panic!("KeyEvent であるべき"),
    }
}

// ── 4 バイト LE フレーミング ─────────────────────────────────────────────────

/// IPC フレームを組み立てる（[len: u32 LE][payload]）
fn frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut framed = len.to_le_bytes().to_vec();
    framed.extend_from_slice(payload);
    framed
}

#[test]
fn test_frame_length_prefix_ping() {
    let payload = bincode::serialize(&ClientToServer::Ping).expect("Ping のシリアライズ失敗");
    let framed = frame(&payload);

    assert!(framed.len() >= 4);
    let declared_len = u32::from_le_bytes(framed[..4].try_into().unwrap()) as usize;
    assert_eq!(declared_len, payload.len());

    let body = &framed[4..];
    let decoded: ClientToServer = bincode::deserialize(body).expect("デシリアライズ失敗");
    assert!(matches!(decoded, ClientToServer::Ping));
}

#[test]
fn test_frame_multiple_messages() {
    let msgs: Vec<ClientToServer> = vec![ClientToServer::Ping, ClientToServer::ListSessions];

    let mut buffer: Vec<u8> = Vec::new();
    for msg in &msgs {
        let payload = bincode::serialize(msg).expect("シリアライズ失敗");
        buffer.extend_from_slice(&frame(&payload));
    }

    // バッファから順番に 2 メッセージを読み出す
    let mut pos = 0;
    for expected in &msgs {
        assert!(buffer.len() >= pos + 4);
        let len = u32::from_le_bytes(buffer[pos..pos + 4].try_into().unwrap()) as usize;
        let payload = &buffer[pos + 4..pos + 4 + len];
        let decoded: ClientToServer = bincode::deserialize(payload).expect("デシリアライズ失敗");
        assert_eq!(
            std::mem::discriminant(&decoded),
            std::mem::discriminant(expected),
            "メッセージの順序が一致しない"
        );
        pos += 4 + len;
    }
    assert_eq!(pos, buffer.len(), "バッファに未処理データが残っている");
}

// ── キーコード・モディファイア ──────────────────────────────────────────────

#[test]
fn test_modifiers_default_has_no_flags() {
    let mods = Modifiers::default();
    assert!(!mods.is_ctrl());
    assert!(!mods.is_shift());
}

#[test]
fn test_modifiers_ctrl_flag() {
    let mods = Modifiers(Modifiers::CTRL);
    assert!(mods.is_ctrl());
    assert!(!mods.is_shift());
}

#[test]
fn test_modifiers_combined_flags() {
    let mods = Modifiers(Modifiers::CTRL | Modifiers::SHIFT);
    assert!(mods.is_ctrl());
    assert!(mods.is_shift());
}

#[test]
fn test_resize_message_roundtrip() {
    let msg = ClientToServer::Resize {
        cols: 120,
        rows: 40,
    };
    let encoded = bincode::serialize(&msg).expect("Resize のシリアライズ失敗");
    let decoded: ClientToServer = bincode::deserialize(&encoded).expect("デシリアライズ失敗");
    match decoded {
        ClientToServer::Resize { cols, rows } => {
            assert_eq!(cols, 120);
            assert_eq!(rows, 40);
        }
        _ => panic!("Resize であるべき"),
    }
}
