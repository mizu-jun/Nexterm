//! Persistence layer for user-assigned command-block names.
//!
//! Mirrors the `palette_history` storage pattern: a single JSON file written
//! atomically with mode 0600 on Unix. The default location is
//! `~/.local/state/nexterm/named_blocks.json` (Unix) or
//! `%APPDATA%\nexterm\named_blocks.json` (Windows). Tests can override the
//! path via the `__NEXTERM_TEST_NAMED_BLOCKS_PATH__` environment variable.
//!
//! On load failure (missing, unreadable, or malformed file) the store falls
//! back to empty rather than propagating the error — block naming is a
//! convenience feature, not a correctness feature, so a corrupt file must not
//! brick the client.
//!
//! NOTE: this module is fully unit-tested but its public surface is only
//! consumed by the renderer / palette work that lands in Phase 2 of the
//! command-blocks feature. `dead_code` is silenced here for that reason and
//! the attribute should be removed once Phase 2 wires the store into the UI.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::command_blocks::BlockId;

/// Soft cap on the number of remembered names.
///
/// Once exceeded, [`NamedBlockStore::save`] sheds the oldest entries (by
/// `last_used_unix`) so the file does not grow without bound.
pub const MAX_NAMED_BLOCKS: usize = 10_000;

/// Current on-disk schema version.
const SCHEMA_VERSION: u32 = 1;

/// Persisted entry for a named block.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct NamedBlockEntry {
    /// User-assigned name, trimmed and non-empty.
    pub name: String,
    /// Unix epoch seconds at the last user interaction with this block.
    pub last_used_unix: i64,
}

/// On-disk wrapper around the names map.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct OnDisk {
    #[serde(default = "default_schema_version")]
    version: u32,
    #[serde(default)]
    names: HashMap<BlockId, NamedBlockEntry>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for OnDisk {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            names: HashMap::new(),
        }
    }
}

/// In-memory store of user-assigned block names.
///
/// Keyed by [`BlockId`] so a name survives across the same `(pane_id, prompt_row)`
/// pair. If scrollback rotation eventually wraps the row counter the user simply
/// re-assigns the name; we deliberately keep the scheme simple.
#[derive(Clone, Debug, Default)]
pub struct NamedBlockStore {
    names: HashMap<BlockId, NamedBlockEntry>,
}

impl NamedBlockStore {
    /// Create an empty store (no disk read).
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the on-disk file. Returns an empty store on any failure.
    pub fn load() -> Self {
        let path = storage_path();
        if !path.exists() {
            return Self::new();
        }
        let json = match std::fs::read_to_string(&path) {
            Ok(j) => j,
            Err(e) => {
                warn!("failed to read named-blocks store at {:?}: {}", path, e);
                return Self::new();
            }
        };
        match serde_json::from_str::<OnDisk>(&json) {
            Ok(disk) => Self { names: disk.names },
            Err(e) => {
                warn!("failed to parse named-blocks store at {:?}: {}", path, e);
                Self::new()
            }
        }
    }

    /// Persist the store. Logs (but does not return) errors.
    pub fn save(&self) {
        let path = storage_path();
        let payload = OnDisk {
            version: SCHEMA_VERSION,
            names: self.names.clone(),
        };
        let json = match serde_json::to_string_pretty(&payload) {
            Ok(j) => j,
            Err(e) => {
                warn!("failed to serialise named-blocks store: {}", e);
                return;
            }
        };
        if let Err(e) = write_atomic_secure(&path, json.as_bytes()) {
            warn!("failed to save named-blocks store at {:?}: {}", path, e);
        }
    }

    /// Look up the name assigned to a block, if any.
    pub fn get(&self, id: BlockId) -> Option<&str> {
        self.names.get(&id).map(|e| e.name.as_str())
    }

    /// All block names in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = (BlockId, &NamedBlockEntry)> {
        self.names.iter().map(|(id, entry)| (*id, entry))
    }

    /// Number of named blocks currently held.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the store contains no entries.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Assign or rename. Empty / whitespace-only names are treated as a remove.
    /// Returns `true` when the in-memory state changed.
    pub fn set(&mut self, id: BlockId, name: &str) -> bool {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return self.remove(id);
        }
        let now = unix_now();
        match self.names.get_mut(&id) {
            Some(existing) => {
                let changed = existing.name != trimmed;
                existing.name = trimmed.to_string();
                existing.last_used_unix = now;
                if self.names.len() > MAX_NAMED_BLOCKS {
                    self.evict_oldest();
                }
                changed
            }
            None => {
                self.names.insert(
                    id,
                    NamedBlockEntry {
                        name: trimmed.to_string(),
                        last_used_unix: now,
                    },
                );
                if self.names.len() > MAX_NAMED_BLOCKS {
                    self.evict_oldest();
                }
                true
            }
        }
    }

    /// Remove a name. Returns `true` when an entry was actually removed.
    pub fn remove(&mut self, id: BlockId) -> bool {
        self.names.remove(&id).is_some()
    }

    /// Refresh `last_used_unix` without changing the name.
    pub fn touch(&mut self, id: BlockId) {
        if let Some(entry) = self.names.get_mut(&id) {
            entry.last_used_unix = unix_now();
        }
    }

    /// Evict the oldest 10% of entries (rounded up) by `last_used_unix`.
    fn evict_oldest(&mut self) {
        let target = self.names.len().saturating_sub(MAX_NAMED_BLOCKS);
        // Always shed at least 10% so we are not on the edge of the cap.
        let extra = MAX_NAMED_BLOCKS / 10;
        let to_drop = target + extra;
        let mut by_age: Vec<(BlockId, i64)> = self
            .names
            .iter()
            .map(|(id, e)| (*id, e.last_used_unix))
            .collect();
        by_age.sort_by_key(|&(_, ts)| ts);
        for (id, _) in by_age.iter().take(to_drop) {
            self.names.remove(id);
        }
    }
}

fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolve the storage path, honouring the test override.
fn storage_path() -> PathBuf {
    if let Ok(test_path) = std::env::var("__NEXTERM_TEST_NAMED_BLOCKS_PATH__") {
        return PathBuf::from(test_path);
    }

    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("nexterm").join("named_blocks.json")
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            return PathBuf::from(xdg).join("nexterm").join("named_blocks.json");
        }
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        home.join(".local")
            .join("state")
            .join("nexterm")
            .join("named_blocks.json")
    }
}

/// Write a file atomically with mode 0600 on Unix.
fn write_atomic_secure(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("could not obtain parent directory: {:?}", path),
        )
    })?;
    std::fs::create_dir_all(parent)?;

    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("nexterm"),
        std::process::id()
    );
    let tmp_path = parent.join(tmp_name);

    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Tests mutate the `__NEXTERM_TEST_NAMED_BLOCKS_PATH__` env var, so we
    // serialise them to avoid cross-test interference.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    struct TestPath {
        _guard: std::sync::MutexGuard<'static, ()>,
        path: PathBuf,
    }

    impl TestPath {
        fn new(name: &str) -> Self {
            let guard = ENV_MUTEX.lock().expect("test mutex poisoned");
            let mut path = std::env::temp_dir();
            path.push(format!("nexterm-named-blocks-test-{}.json", name));
            // Start clean
            let _ = std::fs::remove_file(&path);
            // SAFETY: tests are serialised by ENV_MUTEX, so the env mutation is
            // race-free for this process.
            unsafe {
                std::env::set_var("__NEXTERM_TEST_NAMED_BLOCKS_PATH__", &path);
            }
            Self {
                _guard: guard,
                path,
            }
        }
    }

    impl Drop for TestPath {
        fn drop(&mut self) {
            // SAFETY: tests are serialised by ENV_MUTEX.
            unsafe {
                std::env::remove_var("__NEXTERM_TEST_NAMED_BLOCKS_PATH__");
            }
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn load_returns_empty_when_file_absent() {
        let _t = TestPath::new("empty");
        let store = NamedBlockStore::load();
        assert!(store.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let _t = TestPath::new("roundtrip");
        let mut store = NamedBlockStore::new();
        assert!(store.set(0x0000_0001_0000_0010, "deploy"));
        assert!(store.set(0x0000_0001_0000_0020, "lint"));
        store.save();

        let loaded = NamedBlockStore::load();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get(0x0000_0001_0000_0010), Some("deploy"));
        assert_eq!(loaded.get(0x0000_0001_0000_0020), Some("lint"));
    }

    #[test]
    fn empty_name_removes_entry() {
        let _t = TestPath::new("empty-name");
        let mut store = NamedBlockStore::new();
        store.set(42, "build");
        assert_eq!(store.get(42), Some("build"));
        assert!(store.set(42, "   "));
        assert!(store.get(42).is_none());
    }

    #[test]
    fn remove_returns_false_when_absent() {
        let mut store = NamedBlockStore::new();
        assert!(!store.remove(99));
    }

    #[test]
    fn malformed_json_falls_back_to_empty() {
        let t = TestPath::new("malformed");
        std::fs::write(&t.path, b"not a valid json {{{").unwrap();
        let store = NamedBlockStore::load();
        assert!(store.is_empty());
    }

    #[test]
    fn set_trims_whitespace() {
        let mut store = NamedBlockStore::new();
        store.set(1, "  deploy  ");
        assert_eq!(store.get(1), Some("deploy"));
    }

    #[test]
    fn touch_updates_last_used() {
        let mut store = NamedBlockStore::new();
        store.set(1, "x");
        let initial = store
            .iter()
            .find(|(id, _)| *id == 1)
            .map(|(_, e)| e.last_used_unix)
            .unwrap();
        // Force a delta by waiting one second isn't reliable in tests; instead
        // overwrite the timestamp and confirm `touch` raises it.
        if let Some(entry) = store.names.get_mut(&1) {
            entry.last_used_unix = 0;
        }
        store.touch(1);
        let after = store.names.get(&1).unwrap().last_used_unix;
        assert!(
            after >= initial,
            "touch must not move the timestamp backwards"
        );
        assert!(after > 0, "touch must produce a positive timestamp");
    }

    #[test]
    fn set_existing_with_same_name_returns_false() {
        let mut store = NamedBlockStore::new();
        assert!(store.set(7, "name"));
        assert!(!store.set(7, "name"));
    }
}
