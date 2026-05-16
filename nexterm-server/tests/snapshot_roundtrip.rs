//! スナップショット保存・復元の統合テスト
//!
//! `persist::save_snapshot` / `persist::load_snapshot` の
//! ファイルへの書き込み→読み込みラウンドトリップを検証する。

use nexterm_server::snapshot::{
    SNAPSHOT_VERSION, SNAPSHOT_VERSION_MIN, ServerSnapshot, SessionSnapshot, SplitDirSnapshot,
    SplitNodeSnapshot, WindowSnapshot,
};
use std::sync::Mutex;

/// 環境変数 (`XDG_STATE_HOME` / `APPDATA`) を書き換えるテストをシリアル化する。
///
/// `std::env::set_var` はプロセスグローバルなため、cargo の test thread 並列実行で
/// 競合する。特に Windows CI で `test_persist_save_and_load` と
/// `test_load_snapshot_returns_none_when_missing` の `APPDATA` 書き換えが衝突して
/// snapshot_path() が想定外の場所を返し、save 直後の load が None を返す flaky に
/// なっていた。このグローバル Mutex で env 触るテストの相互排他を保証する。
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn minimal_snapshot() -> ServerSnapshot {
    ServerSnapshot {
        version: SNAPSHOT_VERSION,
        sessions: vec![],
        saved_at: 1_700_000_000,
        current_workspace: "default".to_string(),
    }
}

fn session_with_single_pane() -> SessionSnapshot {
    SessionSnapshot {
        name: "test".to_string(),
        shell: "/bin/bash".to_string(),
        shell_args: vec![],
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
        session_title: None,
        workspace_name: "default".to_string(),
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
        current_workspace: "default".to_string(),
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
    // ENV_LOCK は env var 触る他テストとの相互排他に必要（CI flaky 対策）
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let dir = tempfile::tempdir().expect("tmpdir 作成失敗");

    // state_dir() は OS によって参照する環境変数が異なるため両方を tmpdir に向ける:
    //   Unix:    XDG_STATE_HOME（無ければ HOME/.local/state/nexterm）
    //   Windows: APPDATA/nexterm
    let old_xdg = std::env::var("XDG_STATE_HOME").ok();
    let old_appdata = std::env::var("APPDATA").ok();
    // SAFETY: テスト内での環境変数書き換え。ENV_LOCK でシリアル化済みなので
    // 並列テストとの競合はない。テスト末尾で必ず元値に復元する。
    unsafe {
        std::env::set_var("XDG_STATE_HOME", dir.path());
        std::env::set_var("APPDATA", dir.path());
    }

    let snap = ServerSnapshot {
        version: SNAPSHOT_VERSION,
        sessions: vec![session_with_single_pane()],
        saved_at: 999,
        current_workspace: "default".to_string(),
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
        match old_xdg {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        match old_appdata {
            Some(v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
    }
}

#[test]
fn test_load_snapshot_returns_none_when_missing() {
    // ENV_LOCK は env var 触る他テストとの相互排他に必要（CI flaky 対策）
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // state_dir() は OS によって参照する環境変数が異なるため両方を tmpdir に向ける:
    //   Unix:    XDG_STATE_HOME（無ければ HOME/.local/state/nexterm）
    //   Windows: APPDATA/nexterm
    let dir = tempfile::tempdir().expect("tmpdir 作成失敗");

    let old_home = std::env::var("HOME").ok();
    let old_xdg = std::env::var("XDG_STATE_HOME").ok();
    let old_appdata = std::env::var("APPDATA").ok();

    // SAFETY: テスト内環境変数書き換え。ENV_LOCK でシリアル化済みなので並列競合なし。
    unsafe {
        std::env::set_var("HOME", dir.path());
        std::env::set_var("XDG_STATE_HOME", dir.path());
        std::env::set_var("APPDATA", dir.path());
    }

    // スナップショットファイルが存在しない → None を返す
    let result = nexterm_server::persist::load_snapshot();
    assert!(result.is_none(), "ファイルが存在しない場合は None のはず");

    unsafe {
        match old_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        match old_xdg {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
        match old_appdata {
            Some(v) => std::env::set_var("APPDATA", v),
            None => std::env::remove_var("APPDATA"),
        }
    }
}

#[test]
fn test_snapshot_version_is_current() {
    assert_eq!(
        SNAPSHOT_VERSION, 3,
        "スキーマバージョンが変更された場合は移行処理を追加すること"
    );
    assert_eq!(SNAPSHOT_VERSION_MIN, 1, "最低サポートバージョンは 1 のまま");
}

#[test]
fn test_v1_snapshot_migrates_to_v2() {
    // v1 形式のスナップショット JSON（session_title / workspace_name / current_workspace なし）
    let v1_json = r#"{
        "version": 1,
        "sessions": [],
        "saved_at": 12345
    }"#;

    let snap: ServerSnapshot = serde_json::from_str(v1_json).expect("v1 デシリアライズ失敗");
    // serde(default) により v1 でも正常に読めるはず
    assert_eq!(snap.version, 1);
    assert!(snap.sessions.is_empty());
    // v3 で追加された current_workspace は default を補完される
    assert_eq!(snap.current_workspace, "default");
}

#[test]
fn test_v2_snapshot_migrates_to_v3() {
    // v2 形式のスナップショット JSON（workspace_name / current_workspace を含まない）
    let v2_json = r#"{
        "version": 2,
        "sessions": [
            {
                "name": "main",
                "shell": "/bin/bash",
                "shell_args": [],
                "cols": 80,
                "rows": 24,
                "windows": [],
                "focused_window_id": 0,
                "session_title": null
            }
        ],
        "saved_at": 99
    }"#;

    let snap: ServerSnapshot = serde_json::from_str(v2_json).expect("v2 デシリアライズ失敗");
    assert_eq!(snap.version, 2);
    assert_eq!(snap.current_workspace, "default");
    assert_eq!(snap.sessions[0].workspace_name, "default");
}

#[test]
fn test_session_title_defaults_to_none() {
    let session = session_with_single_pane();
    assert!(session.session_title.is_none());

    // JSON に session_title がなくても正常にデシリアライズできる
    let json = r#"{
        "name": "test",
        "shell": "/bin/bash",
        "shell_args": [],
        "cols": 80,
        "rows": 24,
        "windows": [],
        "focused_window_id": 0
    }"#;
    let s: SessionSnapshot = serde_json::from_str(json).expect("デシリアライズ失敗");
    assert!(s.session_title.is_none());
}
