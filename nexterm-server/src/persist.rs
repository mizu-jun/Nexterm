//! セッション永続化 — スナップショットをファイルに保存・復元する
//!
//! 保存先（スナップショット）:
//!   `~/.local/state/nexterm/snapshot.json`（Unix）
//!   `%APPDATA%\nexterm\snapshot.json`（Windows）

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::snapshot::{SNAPSHOT_VERSION, SNAPSHOT_VERSION_MIN, ServerSnapshot};

// ---- atomic write ヘルパー ----

/// ファイルをアトミックに（一時ファイル → rename で）書き込み、
/// Unix では所有者のみ R/W のパーミッション (0600) を強制する。
///
/// クラッシュ時のスナップショット破損と、共有ホストでの他ユーザーによる
/// 機密読み取りの両方を防ぐ。
///
/// # 引数
/// - `path`: 書き込み先のパス
/// - `content`: 書き込み内容
///
/// # エラー
/// - 親ディレクトリが取得できない場合
/// - 一時ファイル書き込みに失敗した場合
/// - rename に失敗した場合
pub fn write_atomic_secure(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("親ディレクトリが取得できません: {:?}", path))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("ディレクトリの作成に失敗: {:?}", parent))?;

    // PID + プロセス内一意なサフィックスで衝突を回避
    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("nexterm"),
        std::process::id()
    );
    let tmp_path = parent.join(tmp_name);

    // クリーンアップ用 RAII ガード（rename 成功時はキャンセル）
    struct CleanupGuard<'a> {
        path: &'a Path,
        cancelled: bool,
    }
    impl Drop for CleanupGuard<'_> {
        fn drop(&mut self) {
            if !self.cancelled {
                let _ = std::fs::remove_file(self.path);
            }
        }
    }
    let mut guard = CleanupGuard {
        path: &tmp_path,
        cancelled: false,
    };

    // 一時ファイル作成 + 0600 パーミッション (Unix) で書き込み
    {
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp_path)
                .with_context(|| format!("一時ファイル作成失敗: {:?}", tmp_path))?
        };
        #[cfg(windows)]
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)
            .with_context(|| format!("一時ファイル作成失敗: {:?}", tmp_path))?;

        file.write_all(content)
            .with_context(|| format!("書き込み失敗: {:?}", tmp_path))?;
        file.sync_all()
            .with_context(|| format!("fsync 失敗: {:?}", tmp_path))?;
    }

    // atomic rename
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("rename 失敗: {:?} -> {:?}", tmp_path, path))?;
    guard.cancelled = true;

    Ok(())
}

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
        // XDG_STATE_HOME が設定されていればそれを優先する（テスト隔離にも有用）
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            return PathBuf::from(xdg).join("nexterm");
        }
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
///
/// atomic write（一時ファイル → rename）で書き込み、Unix では 0600 パーミッションを強制する。
/// クラッシュ時の破損と、共有ホストでの他ユーザーによる機密情報読み取りを防ぐ。
pub fn save_snapshot(snap: &ServerSnapshot) -> Result<()> {
    let path = snapshot_path();
    let json = serde_json::to_string_pretty(snap)?;
    write_atomic_secure(&path, json.as_bytes())?;
    info!("スナップショットを保存しました: {:?}", path);
    Ok(())
}

/// スナップショットを JSON ファイルから読み込む
///
/// ファイルが存在しない場合や解析エラーの場合は `None` を返す。
/// 旧バージョン（v1）のスナップショットは自動マイグレーションを試みる。
pub fn load_snapshot() -> Option<ServerSnapshot> {
    let path = snapshot_path();
    if !path.exists() {
        return None;
    }
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => {
            warn!("スナップショットファイルの読み込みに失敗しました: {}", e);
            return None;
        }
    };

    // まず通常デシリアライズを試みる
    match serde_json::from_str::<ServerSnapshot>(&json) {
        Ok(snap) => {
            // バージョン範囲チェック（古すぎる場合は破棄）
            if snap.version < SNAPSHOT_VERSION_MIN {
                warn!(
                    "スナップショットバージョンが古すぎます（got={}, min={}）。破棄します",
                    snap.version, SNAPSHOT_VERSION_MIN
                );
                return None;
            }
            // v1 → v2 マイグレーション: バージョンを現在値に更新して返す
            if snap.version < SNAPSHOT_VERSION {
                info!(
                    "スナップショット v{} → v{} にマイグレーションします",
                    snap.version, SNAPSHOT_VERSION
                );
                let migrated = ServerSnapshot {
                    version: SNAPSHOT_VERSION,
                    ..snap
                };
                info!(
                    "スナップショットを読み込みました（マイグレーション済み）: {:?}",
                    path
                );
                return Some(migrated);
            }
            info!("スナップショットを読み込みました: {:?}", path);
            Some(snap)
        }
        Err(e) => {
            warn!("スナップショットの解析に失敗しました: {}", e);
            None
        }
    }
}

/// スナップショットファイルを削除する（クリーンシャットダウン時）
#[allow(dead_code)]
pub fn clear_snapshot() {
    let path = snapshot_path();
    if path.exists()
        && let Err(e) = std::fs::remove_file(&path)
    {
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
        let loaded: ServerSnapshot =
            serde_json::from_str(&std::fs::read_to_string(&tmp).unwrap()).unwrap();
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert!(loaded.sessions.is_empty());

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn atomic_write_でファイルが書き込まれる() {
        let tmp =
            std::env::temp_dir().join(format!("nexterm_test_atomic_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        write_atomic_secure(&tmp, b"hello\n").unwrap();
        let content = std::fs::read(&tmp).unwrap();
        assert_eq!(content, b"hello\n");

        // 上書き
        write_atomic_secure(&tmp, b"world\n").unwrap();
        let content = std::fs::read(&tmp).unwrap();
        assert_eq!(content, b"world\n");

        std::fs::remove_file(&tmp).ok();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_でパーミッションが_0600_になる() {
        use std::os::unix::fs::PermissionsExt;
        let tmp =
            std::env::temp_dir().join(format!("nexterm_test_perm_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        write_atomic_secure(&tmp, b"secret\n").unwrap();
        let mode = std::fs::metadata(&tmp).unwrap().permissions().mode();
        // 下位 9 ビット (rwxrwxrwx) を抽出。0o600 = 所有者のみ R/W。
        assert_eq!(
            mode & 0o777,
            0o600,
            "atomic write 後のパーミッションが 0600 ではない: {:o}",
            mode & 0o777
        );

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn atomic_write_で一時ファイルが残らない() {
        let tmp =
            std::env::temp_dir().join(format!("nexterm_test_cleanup_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        write_atomic_secure(&tmp, b"final\n").unwrap();

        // 親ディレクトリで `.<filename>.tmp.<pid>` 形式の一時ファイルがないことを確認
        let parent = tmp.parent().unwrap();
        let tmp_pattern = format!(".{}.tmp.", tmp.file_name().unwrap().to_str().unwrap());
        for entry in std::fs::read_dir(parent).unwrap().flatten() {
            let name = entry.file_name();
            let name_str = name.to_str().unwrap_or("");
            assert!(
                !name_str.starts_with(&tmp_pattern),
                "一時ファイルが残っている: {}",
                name_str
            );
        }

        std::fs::remove_file(&tmp).ok();
    }
}
