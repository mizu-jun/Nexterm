//! スナップショット保存・復元の統合テスト
//!
//! `persist::save_snapshot` / `persist::load_snapshot` の
//! ファイルへの書き込み→読み込みラウンドトリップを検証する。

use nexterm_server::snapshot::{
    ServerSnapshot, SessionSnapshot, SplitDirSnapshot, SplitNodeSnapshot, WindowSnapshot,
    SNAPSHOT_VERSION,
};

fn minimal_snapshot() -> ServerSnapshot {
    ServerSnapshot {
        version: SNAPSHOT_VERSION,
        sessions: vec![],
        saved_at: 1_700_000_000,
    }
}

fn session_with_single_pane() -> SessionSnapshot {
    SessionSnapshot {
        name: "test".to_string(),
        shell: "/bin/bash".to_string(),
        cols: 80,
        rows: 24,
        windows: vec![WindowSnapshot {
            id: 1,
            name: "main".to_string(),
            focused_pane_id: 1,
            layout: SplitNodeSnapshot::Pane {
                pane_id: 1,
                cwd: None,
            },
        }],
        focused_window_id: 1,
    }
}

// ── JSON ラウンドトリップ ────────────────────────────────────────────────────

#[test]
fn test_empty_snapshot_json_roundtrip() {
    let original = minimal_snapshot();
    let json = serde_json::to_string_pretty(&original).expect("シリアライズ失敗");
    let restored: ServerSnapshot = serde_json::from_str(&json).expect("デシリアライズ失敗");

    assert_eq!(restored.version, SNAPSHOT_VERSION);
    assert!(restored.sessions.is_empty());
    assert_eq!(restored.saved_at, 1_700_000_000);
}

#[test]
fn test_session_snapshot_roundtrip() {
    let session = session_with_single_pane();
    let snap = ServerSnapshot {
        version: SNAPSHOT_VERSION,
        sessions: vec![session],
        saved_at: 42,
    };

    let json = serde_json::to_string_pretty(&snap).expect("シリアライズ失敗");
    let restored: ServerSnapshot = serde_json::from_str(&json).expect("デシリアライズ失敗");

    assert_eq!(restored.sessions.len(), 1);
    let s = &restored.sessions[0];
    assert_eq!(s.name, "test");
    assert_eq!(s.cols, 80);
    assert_eq!(s.rows, 24);
    assert_eq!(s.windows.len(), 1);
    assert_eq!(s.windows[0].focused_pane_id, 1);
}

#[test]
fn test_bsp_split_node_roundtrip() {
    let node = SplitNodeSnapshot::Split {
        dir: SplitDirSnapshot::Horizontal,
        ratio: 0.5,
        left: Box::new(SplitNodeSnapshot::Pane {
            pane_id: 1,
            cwd: None,
        }),
        right: Box::new(SplitNodeSnapshot::Pane {
            pane_id: 2,
            cwd: Some("/tmp".into()),
        }),
    };

    let json = serde_json::to_string(&node).expect("シリアライズ失敗");
    let restored: SplitNodeSnapshot = serde_json::from_str(&json).expect("デシリアライズ失敗");

    match restored {
        SplitNodeSnapshot::Split {
            ratio, left, right, ..
        } => {
            assert!((ratio - 0.5).abs() < f32::EPSILON);
            assert!(matches!(*left, SplitNodeSnapshot::Pane { pane_id: 1, .. }));
            assert!(matches!(*right, SplitNodeSnapshot::Pane { pane_id: 2, .. }));
        }
        _ => panic!("Split ノードであるべき"),
    }
}

// ── ファイル永続化 ─────────────────────────────────────────────────────────

#[test]
fn test_persist_save_and_load() {
    let dir = tempfile::tempdir().expect("tmpdir 作成失敗");

    // HOME を tmpdir に上書きしてスナップショットパスを隔離する
    let old_home = std::env::var("HOME").ok();
    // SAFETY: シングルスレッドテスト内での環境変数書き換え
    unsafe { std::env::set_var("HOME", dir.path()) };

    let snap = ServerSnapshot {
        version: SNAPSHOT_VERSION,
        sessions: vec![session_with_single_pane()],
        saved_at: 999,
    };

    nexterm_server::persist::save_snapshot(&snap).expect("保存失敗");
    let loaded =
        nexterm_server::persist::load_snapshot().expect("保存したスナップショットが読み込めるはず");

    assert_eq!(loaded.version, SNAPSHOT_VERSION);
    assert_eq!(loaded.saved_at, 999);
    assert_eq!(loaded.sessions.len(), 1);
    assert_eq!(loaded.sessions[0].name, "test");

    // 環境変数を元に戻す
    unsafe {
        match old_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }
}

#[test]
fn test_load_snapshot_returns_none_when_missing() {
    let dir = tempfile::tempdir().expect("tmpdir 作成失敗");
    let old_home = std::env::var("HOME").ok();
    unsafe { std::env::set_var("HOME", dir.path()) };

    // スナップショットファイルが存在しない → None を返す
    let result = nexterm_server::persist::load_snapshot();
    assert!(result.is_none(), "ファイルが存在しない場合は None のはず");

    unsafe {
        match old_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }
}

#[test]
fn test_snapshot_version_is_current() {
    assert_eq!(
        SNAPSHOT_VERSION, 1,
        "スキーマバージョンが変更された場合は移行処理を追加すること"
    );
}
