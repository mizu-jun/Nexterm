//! Runtime config — supports hot-reloading from the configuration file.
//!
//! [`RuntimeConfig`] holds the subset of configuration referenced from the dispatch layer.
//! It is shared with every client handler through [`SharedRuntimeConfig`]
//! (= `Arc<ArcSwap<RuntimeConfig>>`); [`spawn_watcher`] atomically swaps in the new value
//! whenever `config.toml` changes.
//!
//! Note: only hooks, log config, and hosts are hot-reloadable. The following require a server
//! restart:
//! - `web` (the listener that is already started cannot be reconfigured).
//! - `plugins` (replacing running WASM instances loses their state).
//! - `shell` (affects only new sessions; existing PTYs are unaffected).
//! - `lua_runner` (regenerating the LuaWorker thread is required).

use std::sync::Arc;

use arc_swap::ArcSwap;
use nexterm_config::{Config, HooksConfig, HostConfig, LogConfig, TabBarConfig};
use tokio::sync::mpsc;
use tracing::{debug, info};

/// Subset of configuration that can be hot-reloaded at runtime.
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// Lua hook configuration.
    pub hooks: Arc<HooksConfig>,
    /// Logging / recording configuration.
    pub log_config: Arc<LogConfig>,
    /// SSH host configuration.
    pub hosts: Arc<Vec<HostConfig>>,
    /// Tab-bar configuration (Phase 2c: drives `show_process_icon`,
    /// which the per-second polling ticker checks to decide whether
    /// to inspect each pane's foreground process).
    pub tab_bar: Arc<TabBarConfig>,
}

impl RuntimeConfig {
    /// Extract the hot-reloadable subset from a full [`Config`].
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            hooks: Arc::new(cfg.hooks.clone()),
            log_config: Arc::new(cfg.log.clone()),
            hosts: Arc::new(cfg.hosts.clone()),
            tab_bar: Arc::new(cfg.tab_bar.clone()),
        }
    }

    /// Assemble from individual fields (for tests and initialization).
    #[cfg(test)]
    fn new(
        hooks: Arc<HooksConfig>,
        log_config: Arc<LogConfig>,
        hosts: Arc<Vec<HostConfig>>,
        tab_bar: Arc<TabBarConfig>,
    ) -> Self {
        Self {
            hooks,
            log_config,
            hosts,
            tab_bar,
        }
    }
}

/// Runtime config handle shared across the entire IPC layer.
pub type SharedRuntimeConfig = Arc<ArcSwap<RuntimeConfig>>;

/// Wrap an initial [`RuntimeConfig`] in a shared handle.
pub fn shared(initial: RuntimeConfig) -> SharedRuntimeConfig {
    Arc::new(ArcSwap::from(Arc::new(initial)))
}

/// Spawn a background task that keeps reading [`Config`] values from the channel and updates the
/// [`SharedRuntimeConfig`]. Separated from the watcher itself so it can be tested.
pub fn spawn_runtime_updater(shared: SharedRuntimeConfig, mut rx: mpsc::Receiver<Config>) {
    tokio::spawn(async move {
        while let Some(new_cfg) = rx.recv().await {
            let new_runtime = RuntimeConfig::from_config(&new_cfg);
            shared.store(Arc::new(new_runtime));
            info!(
                "updated runtime config (hosts={}, auto_log={})",
                new_cfg.hosts.len(),
                new_cfg.log.auto_log
            );
        }
        // The channel closes when the watcher is dropped, which happens during a
        // normal shutdown — this is expected, so log at debug rather than warn to
        // avoid a spurious warning on every clean exit.
        debug!("config watcher channel closed; stopping hot-reload.");
    });
}

/// Watch `config.toml` and spawn a background task that updates the runtime config on changes.
///
/// The returned `RecommendedWatcher` stops watching when dropped, so the caller must bind it to a
/// variable like `_watcher` to keep it alive (held within `run_server`'s scope).
pub fn spawn_watcher(shared: SharedRuntimeConfig) -> anyhow::Result<notify::RecommendedWatcher> {
    let (tx, rx) = mpsc::channel::<Config>(8);
    let watcher = nexterm_config::watch_config(tx)?;
    spawn_runtime_updater(shared, rx);
    Ok(watcher)
}

/// Build a [`SharedRuntimeConfig`] from the current [`Config`].
pub fn build_shared(cfg: &Config) -> SharedRuntimeConfig {
    shared(RuntimeConfig::from_config(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::{HooksConfig, LogConfig};

    #[test]
    fn shared_wraps_and_returns_runtime_config() {
        let rc = RuntimeConfig::new(
            Arc::new(HooksConfig::default()),
            Arc::new(LogConfig::default()),
            Arc::new(Vec::new()),
            Arc::new(TabBarConfig::default()),
        );
        let s = shared(rc);
        let snapshot = s.load();
        assert!(snapshot.hosts.is_empty());
    }

    #[test]
    fn store_atomically_swaps_value() {
        let rc = RuntimeConfig::new(
            Arc::new(HooksConfig::default()),
            Arc::new(LogConfig::default()),
            Arc::new(Vec::new()),
            Arc::new(TabBarConfig::default()),
        );
        let s = shared(rc);
        assert!(s.load().hosts.is_empty());

        let updated = RuntimeConfig::new(
            Arc::new(HooksConfig::default()),
            Arc::new(LogConfig::default()),
            Arc::new(vec![HostConfig {
                name: "test".into(),
                host: "localhost".into(),
                ..Default::default()
            }]),
            Arc::new(TabBarConfig::default()),
        );
        s.store(Arc::new(updated));
        assert_eq!(s.load().hosts.len(), 1);
    }

    #[test]
    fn from_config_extracts_only_required_fields() {
        let mut cfg = Config::default();
        cfg.hosts.push(HostConfig {
            name: "h1".into(),
            host: "1.2.3.4".into(),
            port: 2222,
            username: "alice".into(),
            ..Default::default()
        });
        let rc = RuntimeConfig::from_config(&cfg);
        assert_eq!(rc.hosts.len(), 1);
        assert_eq!(rc.hosts[0].name, "h1");
    }

    #[tokio::test]
    async fn updater_reflects_config_received_via_channel() {
        // Hosts start empty.
        let shared = build_shared(&Config::default());
        assert!(shared.load().hosts.is_empty());

        // Spawn the updater task.
        let (tx, rx) = mpsc::channel::<Config>(4);
        spawn_runtime_updater(Arc::clone(&shared), rx);

        // Send a Config with one host added.
        let mut updated = Config::default();
        updated.hosts.push(HostConfig {
            name: "h1".into(),
            host: "h1.example.com".into(),
            ..Default::default()
        });
        tx.send(updated).await.expect("send failed");

        // Wait for the updater to apply the change (brief polling).
        for _ in 0..50 {
            if shared.load().hosts.len() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(shared.load().hosts.len(), 1);
        assert_eq!(shared.load().hosts[0].name, "h1");

        // Send a second update with two hosts.
        let mut updated2 = Config::default();
        updated2.hosts.push(HostConfig {
            name: "h2".into(),
            ..Default::default()
        });
        updated2.hosts.push(HostConfig {
            name: "h3".into(),
            ..Default::default()
        });
        tx.send(updated2).await.expect("send failed");

        for _ in 0..50 {
            if shared.load().hosts.len() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(shared.load().hosts.len(), 2);
    }

    #[tokio::test]
    async fn multiple_readers_share_arcswap_and_see_latest_value() {
        let shared = build_shared(&Config::default());
        let (tx, rx) = mpsc::channel::<Config>(4);
        spawn_runtime_updater(Arc::clone(&shared), rx);

        // Read in another task while we write from the main one.
        let reader = Arc::clone(&shared);
        let handle = tokio::spawn(async move {
            for _ in 0..200 {
                let _snap = reader.load_full();
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        });

        let mut updated = Config::default();
        updated.hosts.push(HostConfig {
            name: "concurrent".into(),
            ..Default::default()
        });
        tx.send(updated).await.expect("send failed");

        handle.await.expect("reader task failed");
        assert_eq!(shared.load().hosts.len(), 1);
    }
}
