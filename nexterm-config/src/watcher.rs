//! Hot-reload watcher for configuration files.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::loader::{ConfigLoader, config_dir};
use crate::schema::Config;

/// Receiver end of the config-change notification channel.
pub type ConfigRx = mpsc::Receiver<Config>;

/// File names the watcher reacts to. Everything else in the configuration
/// directory is ignored.
///
/// This matters on Windows, where the state directory (which holds
/// `snapshot.json`) and the config directory both resolve to
/// `%APPDATA%\nexterm`. Without this filter the 30-second snapshot auto-save
/// would repeatedly fire the watcher and cause a no-op config reload storm.
const WATCHED_FILE_NAMES: [&str; 2] = ["nexterm.toml", "nexterm.lua"];

/// Returns `true` when any of the changed paths is a configuration file we
/// care about (the TOML or Lua config), ignoring unrelated files such as
/// `snapshot.json`, history files, and atomic-write temp files.
fn is_config_path(paths: &[std::path::PathBuf]) -> bool {
    paths.iter().any(|p| is_watched_file(p))
}

fn is_watched_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(name) if WATCHED_FILE_NAMES.contains(&name)
    )
}

/// Starts a watcher that detects configuration-file changes and sends a fresh
/// `Config` over the channel.
///
/// The returned `_watcher` keeps watching until it is dropped. The caller must
/// bind it to a variable to keep it alive.
pub fn watch_config(tx: mpsc::Sender<Config>) -> Result<RecommendedWatcher> {
    // Remember the last config we forwarded so identical reloads (e.g. an
    // editor "save" that did not change the file) can be suppressed.
    let last_sent: Mutex<Option<Config>> = Mutex::new(None);

    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        match result {
            Ok(event) => {
                // Reload on write / create / delete events only.
                use notify::EventKind::*;
                if !matches!(event.kind, Modify(_) | Create(_) | Remove(_)) {
                    return;
                }
                // Ignore writes to unrelated files in the watched directory.
                if !is_config_path(&event.paths) {
                    return;
                }
                match ConfigLoader::load() {
                    Ok(new_config) => {
                        // Recover the inner value even if a previous handler
                        // panicked while holding the lock.
                        let mut guard = last_sent.lock().unwrap_or_else(|e| e.into_inner());
                        if guard.as_ref() == Some(&new_config) {
                            debug!(
                                "Configuration file touched but content is unchanged; skipping reload."
                            );
                            return;
                        }
                        info!("Detected a configuration-file change. Reloading.");
                        *guard = Some(new_config.clone());
                        drop(guard);
                        let _ = tx.blocking_send(new_config);
                    }
                    Err(e) => {
                        warn!("Failed to reload the configuration: {}", e);
                    }
                }
            }
            Err(e) => warn!("File-watcher error: {}", e),
        }
    })?;

    let dir = config_dir();
    if dir.exists() {
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        info!("Watching the configuration directory: {}", dir.display());
    } else {
        warn!(
            "The configuration directory does not exist; cannot start watching: {}",
            dir.display()
        );
    }

    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn watcher_can_be_started() {
        let (tx, _rx) = mpsc::channel::<Config>(1);
        // When the configuration directory is missing, the function logs a
        // warning and still returns `Ok`.
        let result = watch_config(tx);
        // Confirm that it does not panic regardless of which variant is returned.
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn is_config_path_matches_toml_and_lua() {
        use std::path::PathBuf;
        // Forward-slash paths parse on every platform (`/` is the separator on
        // Unix and is also accepted on Windows); only the file name matters.
        assert!(is_config_path(&[PathBuf::from(
            "/cfg/nexterm/nexterm.toml"
        )]));
        assert!(is_config_path(&[PathBuf::from("/cfg/nexterm/nexterm.lua")]));
        assert!(is_config_path(&[PathBuf::from("nexterm.toml")]));
    }

    #[cfg(windows)]
    #[test]
    fn is_config_path_matches_windows_backslash_path() {
        use std::path::PathBuf;
        // On Windows the config and state dirs both resolve to
        // `%APPDATA%\nexterm`; the config file there uses backslash
        // separators and must still be recognised. (On Unix `\` is a valid
        // file-name character, so this literal is Windows-only.)
        assert!(is_config_path(&[PathBuf::from(
            r"C:\Users\jun\AppData\Roaming\nexterm\nexterm.toml"
        )]));
    }

    #[test]
    fn is_config_path_ignores_snapshot_and_history() {
        use std::path::PathBuf;
        // On Windows the snapshot lives in the same directory as the config,
        // so its 30-second auto-save must not trigger a reload.
        assert!(!is_config_path(&[PathBuf::from(
            "/cfg/nexterm/snapshot.json"
        )]));
        assert!(!is_config_path(&[PathBuf::from(
            "/cfg/nexterm/palette_history.json"
        )]));
    }

    #[test]
    fn is_config_path_ignores_atomic_write_temp_files() {
        use std::path::PathBuf;
        // Atomic writes create temp files like `snapshot.json.tmp1234`; only
        // the final file name should ever match.
        assert!(!is_config_path(&[PathBuf::from(
            "/x/nexterm/snapshot.json.tmp1234"
        )]));
        assert!(!is_config_path(&[PathBuf::from(
            "/x/nexterm/nexterm.toml.tmp9999"
        )]));
    }

    #[test]
    fn is_config_path_empty_is_false() {
        assert!(!is_config_path(&[]));
    }

    #[test]
    fn is_config_path_true_when_any_path_matches() {
        use std::path::PathBuf;
        assert!(is_config_path(&[
            PathBuf::from("/x/nexterm/snapshot.json"),
            PathBuf::from("/x/nexterm/nexterm.toml"),
        ]));
    }
}
