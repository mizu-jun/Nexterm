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

/// Upper bound on the snapshot file size we are willing to read back.
///
/// This is a resource guard, not a security boundary: a snapshot is JSON describing
/// sessions/panes/windows, so a legitimate file is at most a few hundred KB even for a
/// large number of sessions. 16 MiB gives generous headroom while still refusing to
/// `read_to_string` an arbitrarily large (corrupted, or maliciously placed) file into
/// memory before we've even validated it.
const MAX_SNAPSHOT_BYTES: u64 = 16 * 1024 * 1024;

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

/// Validate the snapshot file's size and (on Unix) permissions before reading it.
///
/// Architecture-comparison audit follow-up (2026-09, item #3): `write_atomic_secure`
/// forces 0600 on every write, but the read path performed no equivalent check at
/// all — an asymmetry. This mirrors OpenSSH's "permissions are too open" check on
/// `known_hosts`-style files: on a shared host, a group/other-readable-or-writable
/// snapshot means another local user could have read or tampered with session state
/// (working directories, window titles, host lists) before Nexterm loads it. A size
/// cap is checked first so we never `read_to_string` an unbounded file into memory.
/// Returns `Some(metadata)` when the file passes both checks, or `None` (having
/// already logged why) when it should be treated as absent — matching
/// `load_snapshot`'s existing fail-closed pattern of degrading to a fresh session
/// rather than propagating an error.
fn validate_snapshot_metadata(path: &Path) -> Option<std::fs::Metadata> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            warn!("failed to stat snapshot file: {}", e);
            return None;
        }
    };

    if metadata.len() > MAX_SNAPSHOT_BYTES {
        warn!(
            "snapshot file ({} bytes) exceeds the {} byte cap; refusing to load",
            metadata.len(),
            MAX_SNAPSHOT_BYTES
        );
        return None;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            warn!(
                "snapshot file permissions are too open (mode {:o}, expected 0600); \
                 refusing to load. Run `chmod 600` on the snapshot file to fix this.",
                mode
            );
            return None;
        }
    }

    Some(metadata)
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
    validate_snapshot_metadata(&path)?;
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
            known_workspaces: vec![],
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
    fn validate_snapshot_metadata_accepts_a_0600_file_within_the_size_cap() {
        let tmp = std::env::temp_dir().join(format!(
            "nexterm_test_validate_ok_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        write_atomic_secure(&tmp, b"{}").unwrap();

        assert!(validate_snapshot_metadata(&tmp).is_some());

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn validate_snapshot_metadata_rejects_a_file_over_the_size_cap() {
        let tmp = std::env::temp_dir().join(format!(
            "nexterm_test_validate_toobig_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        // One byte over the cap is enough to trigger the rejection.
        let oversized = vec![b'a'; (MAX_SNAPSHOT_BYTES + 1) as usize];
        write_atomic_secure(&tmp, &oversized).unwrap();

        assert!(
            validate_snapshot_metadata(&tmp).is_none(),
            "a file larger than MAX_SNAPSHOT_BYTES must be rejected"
        );

        std::fs::remove_file(&tmp).ok();
    }

    #[cfg(unix)]
    #[test]
    fn validate_snapshot_metadata_rejects_a_group_readable_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!(
            "nexterm_test_validate_perm_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        write_atomic_secure(&tmp, b"{}").unwrap();
        // Widen permissions past 0600, simulating a shared host / careless `chmod`.
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(
            validate_snapshot_metadata(&tmp).is_none(),
            "a group/other-readable snapshot file must be rejected"
        );

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn load_snapshot_returns_none_for_an_oversized_file() {
        // Exercise the full `load_snapshot()` path (not just the metadata helper) by
        // pointing XDG_STATE_HOME at an isolated tmpdir, matching the pattern used by
        // the integration tests in `tests/snapshot_roundtrip.rs`.
        let dir = std::env::temp_dir().join(format!(
            "nexterm_test_load_oversized_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let nexterm_dir = dir.join("nexterm");
        std::fs::create_dir_all(&nexterm_dir).unwrap();
        let snapshot_file = nexterm_dir.join("snapshot.json");
        let oversized = vec![b'a'; (MAX_SNAPSHOT_BYTES + 1) as usize];
        std::fs::write(&snapshot_file, &oversized).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&snapshot_file, std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }

        // SAFETY: this test does not run concurrently with another test that reads
        // XDG_STATE_HOME (no other test in this file's `#[cfg(test)] mod tests`
        // touches it), and the value is restored before returning.
        let old_xdg = std::env::var("XDG_STATE_HOME").ok();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", &dir);
        }

        let loaded = load_snapshot();

        unsafe {
            match old_xdg {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            loaded.is_none(),
            "an oversized snapshot file must be rejected before parsing"
        );
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
