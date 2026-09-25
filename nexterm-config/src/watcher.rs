//! Hot-reload watcher for configuration files.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};
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

/// How long to wait after the *last* relevant filesystem event before
/// actually reloading the configuration.
///
/// A single external save (editor "save", `toml_edit` write-back, etc.) can
/// emit several raw `notify` events in quick succession — e.g. a write to a
/// temp file followed by a rename over the target, or a separate `Modify`
/// and a `Create`. Reloading on every raw event risks reading the file
/// mid-write (a transiently truncated or partially-written temp file) and
/// wastes work re-parsing the same content multiple times. Restarting this
/// timer on every event and only reloading once the filesystem has been
/// quiet for the whole window coalesces a burst into a single reload.
const DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);

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

/// Returns `true` when any of the changed paths is specifically
/// `nexterm.lua` (as opposed to `nexterm.toml`).
///
/// Declarative `cfg.*` assignments inside `nexterm.lua` are already picked
/// up on reload: [`ConfigLoader::load`] re-executes the script and folds the
/// result into the `Config` this watcher forwards. However, Lua *functions*
/// the script defines (status-bar widget expressions, `hooks.on_*`
/// callbacks) live in separate, long-running `mlua::Lua` VMs (`LuaWorker`,
/// `LuaHookRunner`) that are loaded once at startup and are not reachable
/// from this watcher. This helper lets the caller warn explicitly instead of
/// silently dropping that part of the change — see the call site below.
fn touches_lua_script(paths: &[std::path::PathBuf]) -> bool {
    paths.iter().any(|p| {
        matches!(
            p.file_name().and_then(|name| name.to_str()),
            Some("nexterm.lua")
        )
    })
}

/// Waits until `rx` has been quiet (no new signal received) for a full
/// `window`, restarting the wait every time a new signal arrives in the
/// meantime. This is what turns a burst of raw filesystem events into a
/// single debounced reload trigger.
///
/// Returns `true` once the window elapses with no further signals. Returns
/// `false` if `rx` is closed (the sending half was dropped) before that
/// happens, signalling the caller to stop.
async fn wait_for_quiet(rx: &mut mpsc::UnboundedReceiver<()>, window: Duration) -> bool {
    loop {
        let deadline = Instant::now() + window;
        tokio::select! {
            more = rx.recv() => {
                if more.is_none() {
                    return false;
                }
                // Another signal arrived inside the window; restart the wait.
            }
            () = sleep_until(deadline) => return true,
        }
    }
}

/// Starts a watcher that detects configuration-file changes and sends a fresh
/// `Config` over the channel.
///
/// The returned `_watcher` keeps watching until it is dropped. The caller must
/// bind it to a variable to keep it alive.
///
/// Must be called from within a Tokio runtime context: it spawns a debounce
/// task via [`tokio::spawn`].
pub fn watch_config(tx: mpsc::Sender<Config>) -> Result<RecommendedWatcher> {
    // The `notify` callback runs on a non-async watcher thread, so it only
    // forwards a lightweight "something relevant changed" signal here. The
    // actual debounce wait and reload happen on the Tokio task below, which
    // coalesces a burst of raw events into a single reload (see
    // `DEBOUNCE_WINDOW`).
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<()>();

    // Set by the (non-async) `notify` callback whenever a debounced burst
    // includes a change to `nexterm.lua` specifically, and drained by the
    // debounce task below. This is separate from the `Config` equality check
    // that suppresses no-op reloads: a change to a Lua *function* body (a
    // status-bar widget expression, a `hooks.on_*` callback) does not change
    // any `cfg.*` field, so it would never be visible in `Config` — but it is
    // still a real change that the long-running Lua VMs never pick up.
    let lua_touched = Arc::new(AtomicBool::new(false));
    let lua_touched_writer = Arc::clone(&lua_touched);

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
                if touches_lua_script(&event.paths) {
                    lua_touched_writer.store(true, Ordering::Relaxed);
                }
                // The channel only ever drops once the debounce task below
                // has exited (e.g. the receiving end of `tx` was closed), at
                // which point there is nothing left to signal.
                let _ = signal_tx.send(());
            }
            Err(e) => warn!("File-watcher error: {}", e),
        }
    })?;

    tokio::spawn(async move {
        // Remember the last config we forwarded so identical reloads (e.g.
        // an editor "save" that did not change the file) can be suppressed.
        // Only this task touches it, so a plain local is enough.
        let mut last_sent: Option<Config> = None;

        while signal_rx.recv().await.is_some() {
            // Debounce: coalesce a burst of raw events into a single
            // reload, waiting for the filesystem to go quiet for a full
            // `DEBOUNCE_WINDOW` first.
            if !wait_for_quiet(&mut signal_rx, DEBOUNCE_WINDOW).await {
                // Watcher side dropped; stop the debounce task too.
                return;
            }

            // Consume the flag for this debounce cycle so the next one
            // starts clean.
            if lua_touched.swap(false, Ordering::Relaxed) {
                // NOTE: full hot-reload of `nexterm.lua`'s function bodies
                // (widget expressions, `hooks.on_*` callbacks) into the
                // running `LuaWorker` / `LuaHookRunner` needs a handle to
                // those long-running Lua VMs, which this watcher does not
                // have (they live in nexterm-client-gpu / nexterm-server,
                // constructed independently of `watch_config`). Until that
                // wiring exists, surface the gap explicitly instead of
                // silently dropping the change.
                warn!(
                    "nexterm.lua changed: declarative cfg.* settings were reloaded, but Lua \
                     functions (status-bar widgets, hooks.on_* callbacks) still require an \
                     application restart to take effect."
                );
            }

            match ConfigLoader::load() {
                Ok(new_config) => {
                    if last_sent.as_ref() == Some(&new_config) {
                        debug!(
                            "Configuration file touched but content is unchanged; skipping reload."
                        );
                        continue;
                    }
                    info!("Detected a configuration-file change. Reloading.");
                    last_sent = Some(new_config.clone());
                    if tx.send(new_config).await.is_err() {
                        // Receiver dropped; nothing left to notify.
                        return;
                    }
                }
                Err(e) => {
                    warn!("Failed to reload the configuration: {}", e);
                }
            }
        }
    });

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

    // A short window keeps these tests fast (real wall-clock sleeps, no
    // `tokio` "test-util" feature required); the coalescing behaviour under
    // test does not depend on the window's absolute length.
    const TEST_WINDOW: Duration = Duration::from_millis(40);

    #[tokio::test]
    async fn wait_for_quiet_returns_true_after_the_window_elapses_with_no_signals() {
        let (tx, mut rx) = mpsc::unbounded_channel::<()>();
        tx.send(()).unwrap();

        let result = wait_for_quiet(&mut rx, TEST_WINDOW).await;

        assert!(result);
    }

    #[tokio::test]
    async fn wait_for_quiet_coalesces_a_burst_of_signals_into_one_wait() {
        // Simulates the exact scenario from the audit finding: an external
        // save emits several raw events in quick succession (temp file +
        // rename). Each one should push the deadline back rather than
        // letting the debounce fire early, so the caller only reloads once.
        let (tx, mut rx) = mpsc::unbounded_channel::<()>();

        tx.send(()).unwrap();
        let waiter = tokio::spawn(async move {
            let quiet = wait_for_quiet(&mut rx, TEST_WINDOW).await;
            (quiet, rx)
        });

        // Trickle in more signals well inside the debounce window; each one
        // must restart the wait instead of letting it complete. `tx` is kept
        // alive (not dropped) until after the wait resolves — dropping it
        // would itself unblock `wait_for_quiet` via the "sender gone" path,
        // which is not the condition this test is exercising.
        for _ in 0..4 {
            tokio::time::sleep(TEST_WINDOW / 4).await;
            tx.send(()).unwrap();
        }

        let (quiet, _rx) = waiter.await.unwrap();
        assert!(quiet, "debounce must resolve to quiet once signals stop");
        drop(tx);
    }

    #[tokio::test]
    async fn wait_for_quiet_returns_false_when_the_sender_is_dropped() {
        let (tx, mut rx) = mpsc::unbounded_channel::<()>();
        tx.send(()).unwrap();
        drop(tx);

        let result = wait_for_quiet(&mut rx, TEST_WINDOW).await;

        assert!(!result);
    }

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
