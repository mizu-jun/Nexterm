//! セッション永続化 — スナップショットをファイルに保存・復元する
//!
//! 保存先（スナップショット）:
//!   `~/.local/state/nexterm/snapshot.json`（Unix）
//!   `%APPDATA%\nexterm\snapshot.json`（Windows）

use std::path::PathBuf;

use anyhow::Result;
use tracing::{info, warn};

use crate::snapshot::ServerSnapshot;

// ---- パスヘルパー ----

fn state_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("nexterm")
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        home.join(".local").join("state").join("nexterm")
    }
}

fn snapshot_path() -> PathBuf {
    state_dir().join("snapshot.json")
}

// ---- スナップショット保存・読み込み ----

/// スナップショットを JSON ファイルに保存する
pub fn save_snapshot(snap: &ServerSnapshot) -> Result<()> {
    let path = snapshot_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(snap)?;
    std::fs::write(&path, json)?;
    info!("スナップショットを保存しました: {:?}", path);
    Ok(())
}

/// スナップショットを JSON ファイルから読み込む
///
/// ファイルが存在しない場合や解析エラーの場合は `None` を返す。
pub fn load_snapshot() -> Option<ServerSnapshot> {
    let path = snapshot_path();
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str::<ServerSnapshot>(&json) {
            Ok(snap) => {
                info!("スナップショットを読み込みました: {:?}", path);
                Some(snap)
            }
            Err(e) => {
                warn!("スナップショットの解析に失敗しました: {}", e);
                None
            }
        },
        Err(e) => {
            warn!("スナップショットファイルの読み込みに失敗しました: {}", e);
            None
        }
    }
}

/// スナップショットファイルを削除する（クリーンシャットダウン時）
#[allow(dead_code)]
pub fn clear_snapshot() {
    let path = snapshot_path();
    if path.exists()
        && let Err(e) = std::fs::remove_file(&path) {
            warn!("スナップショットファイルの削除に失敗しました: {}", e);
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SNAPSHOT_VERSION;

    #[test]
    fn スナップショットの保存と読み込み() {
        let snap = ServerSnapshot {
            version: SNAPSHOT_VERSION,
            sessions: Vec::new(),
            saved_at: 0,
        };

        // 一時ファイルに書き込む
        let tmp = std::env::temp_dir().join("nexterm_test_snapshot.json");
        let json = serde_json::to_string_pretty(&snap).unwrap();
        std::fs::write(&tmp, &json).unwrap();

        // 読み込んで内容を確認する
        let loaded: ServerSnapshot = serde_json::from_str(&std::fs::read_to_string(&tmp).unwrap()).unwrap();
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert!(loaded.sessions.is_empty());

        std::fs::remove_file(&tmp).ok();
    }
}
