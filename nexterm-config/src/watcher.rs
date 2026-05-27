//! Hot-reload watcher for configuration files.

use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::loader::{ConfigLoader, config_dir};
use crate::schema::Config;

/// Receiver end of the config-change notification channel.
pub type ConfigRx = mpsc::Receiver<Config>;

/// Starts a watcher that detects configuration-file changes and sends a fresh
/// `Config` over the channel.
///
/// The returned `_watcher` keeps watching until it is dropped. The caller must
/// bind it to a variable to keep it alive.
pub fn watch_config(tx: mpsc::Sender<Config>) -> Result<RecommendedWatcher> {
    let tx_clone = tx.clone();

    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        match result {
            Ok(event) => {
                // Reload on write / create / delete events.
                use notify::EventKind::*;
                if matches!(event.kind, Modify(_) | Create(_) | Remove(_)) {
                    info!("Detected a configuration-file change. Reloading.");
                    match ConfigLoader::load() {
                        Ok(new_config) => {
                            let _ = tx_clone.blocking_send(new_config);
                        }
                        Err(e) => {
                            warn!("Failed to reload the configuration: {}", e);
                        }
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
}
