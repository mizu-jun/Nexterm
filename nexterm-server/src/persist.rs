//! Session persistence — save and restore snapshots on disk.
//!
//! Storage location (snapshot):
//!   `~/.local/state/nexterm/snapshot.json` (Unix)
//!   `%APPDATA%\nexterm\snapshot.json` (Windows)

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{info, instrument, warn};

use crate::snapshot::{SNAPSHOT_VERSION, SNAPSHOT_VERSION_MIN, ServerSnapshot};

// ---- atomic write helper ----

/// Atomically write a file (via tempfile -> rename); on Unix, force owner-only R/W
/// permissions (0600).
///
/// Prevents both snapshot corruption on crashes and secret reads by other users on a shared host.
///
/// # Arguments
/// - `path`: destination path.
/// - `content`: bytes to write.
///
/// # Errors
/// - The parent directory cannot be obtained.
/// - Writing the temporary file failed.
/// - The rename failed.
pub fn write_atomic_secure(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("cannot obtain parent directory: {:?}", path))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create directory: {:?}", parent))?;

    // Avoid collisions with a PID + per-process unique suffix.
    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("nexterm"),
        std::process::id()
    );
    let tmp_path = parent.join(tmp_name);

    // RAII guard used for cleanup (cancelled on successful rename).
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

    // Create the temp file and (on Unix) set 0600 permissions while writing.
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
                .with_context(|| format!("failed to create temp file: {:?}", tmp_path))?
        };
        #[cfg(windows)]
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to create temp file: {:?}", tmp_path))?;

        file.write_all(content)
            .with_context(|| format!("failed to write: {:?}", tmp_path))?;
        file.sync_all()
            .with_context(|| format!("fsync failed: {:?}", tmp_path))?;
    }

    // Atomic rename.
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("rename failed: {:?} -> {:?}", tmp_path, path))?;
    guard.cancelled = true;

    Ok(())
}

// ---- Path helpers ----

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
        // Prefer XDG_STATE_HOME if set (useful for test isolation as well).
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

// ---- Snapshot save / load ----

/// Save the snapshot to a JSON file.
///
/// Uses atomic write (temp file -> rename) and forces 0600 permissions on Unix.
/// Prevents corruption on crashes and secret reads by other users on a shared host.
#[instrument(name = "save_snapshot", skip(snap), fields(version = snap.version, sessions = snap.sessions.len()))]
pub fn save_snapshot(snap: &ServerSnapshot) -> Result<()> {
    let path = snapshot_path();
    let json = serde_json::to_string_pretty(snap)?;
    write_atomic_secure(&path, json.as_bytes())?;
    info!("saved snapshot: {:?}", path);
    Ok(())
}

/// Load the snapshot from a JSON file.
///
/// Returns `None` when the file does not exist or parsing fails.
/// Older snapshots (v1) are automatically migrated.
#[instrument(name = "load_snapshot")]
pub fn load_snapshot() -> Option<ServerSnapshot> {
    let path = snapshot_path();
    if !path.exists() {
        return None;
    }
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => {
            warn!("failed to read snapshot file: {}", e);
            return None;
        }
    };

    // Try the normal deserialization first.
    match serde_json::from_str::<ServerSnapshot>(&json) {
        Ok(snap) => {
            // Version range check (too old -> discard).
            if snap.version < SNAPSHOT_VERSION_MIN {
                warn!(
                    "snapshot version is too old (got={}, min={}); discarding",
                    snap.version, SNAPSHOT_VERSION_MIN
                );
                return None;
            }
            // v1 -> v2 / v2 -> v3 / v3 -> v4 migration: bump the version field to the current
            // value and return.
            //
            // v1 -> v2: add `session_title` (`#[serde(default)]` so older files get `None`).
            // v2 -> v3: add `SessionSnapshot.workspace_name` and `ServerSnapshot.current_workspace`
            //          (`#[serde(default = "default_workspace")]` so older files get `"default"`).
            // v3 -> v4: add `ServerSnapshot.client_os_windows`
            //          (`#[serde(default)]` so older files get an empty `Vec`; restored as a
            //          single-OS-window setup at startup).
            if snap.version < SNAPSHOT_VERSION {
                info!(
                    "migrating snapshot v{} to v{}",
                    snap.version, SNAPSHOT_VERSION
                );
                let migrated = ServerSnapshot {
                    version: SNAPSHOT_VERSION,
                    ..snap
                };
                info!("loaded snapshot (migrated): {:?}", path);
                return Some(migrated);
            }
            info!("loaded snapshot: {:?}", path);
            Some(snap)
        }
        Err(e) => {
            warn!("failed to parse snapshot: {}", e);
            None
        }
    }
}

/// Delete the snapshot file (used on clean shutdown).
#[allow(dead_code)]
pub fn clear_snapshot() {
    let path = snapshot_path();
    if path.exists()
        && let Err(e) = std::fs::remove_file(&path)
    {
        warn!("failed to delete snapshot file: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SNAPSHOT_VERSION;

    #[test]
    fn snapshot_save_and_load() {
        let snap = ServerSnapshot {
            version: SNAPSHOT_VERSION,
            sessions: Vec::new(),
            saved_at: 0,
            current_workspace: "default".to_string(),
            client_os_windows: Vec::new(),
        };

        // Write to a temp file.
        let tmp = std::env::temp_dir().join("nexterm_test_snapshot.json");
        let json = serde_json::to_string_pretty(&snap).unwrap();
        std::fs::write(&tmp, &json).unwrap();

        // Read it back and verify.
        let loaded: ServerSnapshot =
            serde_json::from_str(&std::fs::read_to_string(&tmp).unwrap()).unwrap();
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert!(loaded.sessions.is_empty());

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn atomic_write_writes_file() {
        let tmp =
            std::env::temp_dir().join(format!("nexterm_test_atomic_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        write_atomic_secure(&tmp, b"hello\n").unwrap();
        let content = std::fs::read(&tmp).unwrap();
        assert_eq!(content, b"hello\n");

        // Overwrite.
        write_atomic_secure(&tmp, b"world\n").unwrap();
        let content = std::fs::read(&tmp).unwrap();
        assert_eq!(content, b"world\n");

        std::fs::remove_file(&tmp).ok();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_sets_permissions_to_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp =
            std::env::temp_dir().join(format!("nexterm_test_perm_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        write_atomic_secure(&tmp, b"secret\n").unwrap();
        let mode = std::fs::metadata(&tmp).unwrap().permissions().mode();
        // Extract the lower 9 bits (rwxrwxrwx). 0o600 = owner-only R/W.
        assert_eq!(
            mode & 0o777,
            0o600,
            "post-atomic-write permission is not 0600: {:o}",
            mode & 0o777
        );

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let tmp =
            std::env::temp_dir().join(format!("nexterm_test_cleanup_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        write_atomic_secure(&tmp, b"final\n").unwrap();

        // Verify no `.<filename>.tmp.<pid>` style file remains in the parent directory.
        let parent = tmp.parent().unwrap();
        let tmp_pattern = format!(".{}.tmp.", tmp.file_name().unwrap().to_str().unwrap());
        for entry in std::fs::read_dir(parent).unwrap().flatten() {
            let name = entry.file_name();
            let name_str = name.to_str().unwrap_or("");
            assert!(
                !name_str.starts_with(&tmp_pattern),
                "temp file left behind: {}",
                name_str
            );
        }

        std::fs::remove_file(&tmp).ok();
    }
}
