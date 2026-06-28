//! Session management — manages the lifecycle of sessions and windows.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Result, bail};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{info, warn};

use nexterm_proto::{ServerToClient, SessionInfo, WindowInfo, WorkspaceInfo};

use crate::snapshot::{
    DEFAULT_WORKSPACE, SNAPSHOT_VERSION, SNAPSHOT_VERSION_MIN, ServerSnapshot, SessionSnapshot,
};
use crate::window::Window;

static NEXT_WINDOW_ID: AtomicU32 = AtomicU32::new(1);

fn new_window_id() -> u32 {
    NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed)
}

/// Update the window ID counter after restoring from a snapshot.
pub fn set_min_window_id(min_id: u32) {
    NEXT_WINDOW_ID.fetch_max(min_id, Ordering::Relaxed);
}

/// Session.
pub struct Session {
    pub name: String,
    /// Window list (ID -> Window).
    windows: HashMap<u32, Window>,
    /// Currently focused window ID.
    focused_window_id: u32,
    /// Broadcast send channel for PTY output (delivers to every client simultaneously).
    broadcast_tx: broadcast::Sender<ServerToClient>,
    /// Default shell.
    shell: String,
    /// Default shell arguments.
    shell_args: Vec<String>,
    /// Default terminal size.
    pub cols: u16,
    pub rows: u16,
    /// Broadcast mode flag (forward input to every pane).
    broadcast: bool,
    /// Owning workspace name (Sprint 5-7 / Phase 2-1).
    /// Defaults to `"default"`. Change via the `SessionManager`.
    pub workspace_name: String,
}

impl Session {
    /// Construct a session with a single initial window (belonging to the default workspace).
    ///
    /// Thin wrapper around `new_in_workspace(.., DEFAULT_WORKSPACE)`.
    /// Retained for tests and backward-compatible API.
    #[allow(dead_code)]
    pub fn new(
        name: String,
        cols: u16,
        rows: u16,
        shell: String,
        shell_args: Vec<String>,
    ) -> Result<Self> {
        Self::new_in_workspace(
            name,
            cols,
            rows,
            shell,
            shell_args,
            DEFAULT_WORKSPACE.to_string(),
        )
    }

    /// Construct a session that belongs to the specified workspace (Sprint 5-7 / Phase 2-1).
    pub fn new_in_workspace(
        name: String,
        cols: u16,
        rows: u16,
        shell: String,
        shell_args: Vec<String>,
        workspace_name: String,
    ) -> Result<Self> {
        let (broadcast_tx, _) = broadcast::channel::<ServerToClient>(2048);
        let window_id = new_window_id();
        let window = Window::new(
            window_id,
            "window-1".to_string(),
            cols,
            rows,
            broadcast_tx.clone(),
            &shell,
            &shell_args,
        )?;
        let mut windows = HashMap::new();
        windows.insert(window_id, window);

        Ok(Self {
            name,
            windows,
            focused_window_id: window_id,
            broadcast_tx,
            shell,
            shell_args,
            cols,
            rows,
            broadcast: false,
            workspace_name,
        })
    }

    /// Return the session info.
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            name: self.name.clone(),
            window_count: self.windows.len() as u32,
            attached: self.broadcast_tx.receiver_count() > 0,
            workspace_name: self.workspace_name.clone(),
        }
    }

    /// Return a reference to the focused window.
    pub fn focused_window(&self) -> Option<&Window> {
        self.windows.get(&self.focused_window_id)
    }

    /// Return a reference to the window with the specified `window_id` (Sprint 5-8 Phase 4-3).
    ///
    /// Used by callers that need a specific window (e.g. building `LayoutChanged` for each window
    /// after a move, or obtaining FullRefresh).
    pub fn window(&self, window_id: u32) -> Option<&Window> {
        self.windows.get(&window_id)
    }

    /// Return a mutable reference to the focused window.
    pub fn focused_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.get_mut(&self.focused_window_id)
    }

    /// Attach a client and return its `broadcast::Receiver`.
    ///
    /// Supports multiple simultaneous clients. PTY output is automatically delivered to every
    /// receiver via the `broadcast::Sender`; no fan-out task is required.
    pub fn attach(&self) -> broadcast::Receiver<ServerToClient> {
        self.broadcast_tx.subscribe()
    }

    /// Detach a single client — for broadcast, dropping the receiver is sufficient (no-op).
    pub fn detach_one(&mut self, _tx: &mpsc::Sender<ServerToClient>) {
        // `broadcast::Receiver` unsubscribes automatically on drop.
    }

    /// Detach every client — for broadcast, dropping all receivers is sufficient (no-op).
    pub fn detach_all(&mut self) {
        // The broadcast channel keeps living as long as the sender is alive.
        // `receiver_count()` becomes 0 once every client drops its receiver.
    }

    /// Return whether any client is attached (judged by the broadcast receiver count).
    #[allow(dead_code)]
    pub fn is_attached(&self) -> bool {
        self.broadcast_tx.receiver_count() > 0
    }

    /// Return the broadcast::Sender (used when creating new panes/windows).
    pub fn broadcast_sender(&self) -> broadcast::Sender<ServerToClient> {
        self.broadcast_tx.clone()
    }

    /// Return the default shell.
    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// Return the default shell arguments.
    pub fn shell_args(&self) -> &[String] {
        &self.shell_args
    }

    /// Add a new window.
    pub fn add_window(&mut self) -> Result<u32> {
        let window_id = new_window_id();
        let name = format!("window-{}", window_id);
        let window = Window::new(
            window_id,
            name,
            self.cols,
            self.rows,
            self.broadcast_tx.clone(),
            &self.shell,
            &self.shell_args,
        )?;
        self.windows.insert(window_id, window);
        self.focused_window_id = window_id;
        Ok(window_id)
    }

    /// Remove the specified window (the last remaining window cannot be removed).
    pub fn remove_window(&mut self, window_id: u32) -> Result<()> {
        if self.windows.len() <= 1 {
            return Err(anyhow::anyhow!("cannot remove the last remaining window"));
        }
        if !self.windows.contains_key(&window_id) {
            return Err(anyhow::anyhow!("window {} not found", window_id));
        }
        self.windows.remove(&window_id);
        // If focus was on the removed window, move it to one of the remaining windows.
        if self.focused_window_id == window_id {
            self.focused_window_id = *self
                .windows
                .keys()
                .next()
                .expect("windows is non-empty; verified by len() > 1");
        }
        Ok(())
    }

    /// Move focus to the specified window.
    pub fn focus_window(&mut self, window_id: u32) -> Result<()> {
        if !self.windows.contains_key(&window_id) {
            return Err(anyhow::anyhow!("window {} not found", window_id));
        }
        self.focused_window_id = window_id;
        Ok(())
    }

    /// Rename the specified window.
    pub fn rename_window(&mut self, window_id: u32, name: String) -> Result<()> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or_else(|| anyhow::anyhow!("window {} not found", window_id))?;
        window.name = name;
        Ok(())
    }

    /// Return the list of window info.
    pub fn window_list(&self) -> Vec<WindowInfo> {
        let mut list: Vec<WindowInfo> = self
            .windows
            .values()
            .map(|w| WindowInfo {
                window_id: w.id,
                name: w.name.clone(),
                pane_count: w.pane_count() as u32,
                is_focused: w.id == self.focused_window_id,
            })
            .collect();
        list.sort_by_key(|w| w.window_id);
        list
    }

    /// Break the focused pane out into a new window (break-pane).
    ///
    /// Returns the new window ID on success.
    /// Returns `Err` when the focused window has only a single pane.
    pub fn break_pane(&mut self) -> Result<u32> {
        let cols = self.cols;
        let rows = self.rows;
        let pane = {
            let w = self
                .focused_window_mut()
                .ok_or_else(|| anyhow::anyhow!("focused window not found"))?;
            w.take_focused_pane(cols, rows)
                .ok_or_else(|| anyhow::anyhow!("cannot break out the last remaining pane"))?
        };
        let new_window_id = new_window_id();
        let new_window = Window::new_with_pane(new_window_id, "window-broken".to_string(), pane)?;
        self.windows.insert(new_window_id, new_window);
        self.focused_window_id = new_window_id;
        Ok(new_window_id)
    }

    /// Move the pane with the given `pane_id` to another window (Sprint 5-8 Phase 4-3 / 4-4,
    /// used by tab tearing).
    ///
    /// Invoked from `ClientToServer::MovePaneToWindow` when the client drops a tab onto another OS
    /// window or outside any OS window.
    ///
    /// Behavior:
    /// - `target_window_id == 0`: **create a new window**. Behaves like `break_pane`: create a
    ///   new window via `Window::new_with_pane` and switch `focused_window_id`.
    /// - `target_window_id != 0`: move into an existing window.
    ///   - If `insert_at` is `Some`, use `Window::insert_pane_at` for position-specified insertion.
    ///   - If `None`, use the existing `insert_pane` (right after the focused pane).
    /// - **When the source window has only one pane** (Phase 4-4):
    ///   - The window itself is removed from `self.windows` (consumed via `into_single_pane`).
    ///   - This keeps invariants intact when "the last pane is moved" via a tab-out drop.
    ///   - `WindowListChanged` automatically reflects the deleted window.
    ///
    /// Returns `(source_window_id, new_window_id, moved_pane_id)`.
    /// - `source_window_id`: source window ID (returned even if the window was removed; the caller
    ///   can detect removal via `Session::window(src_id)` returning `None`).
    /// - `new_window_id`: destination window ID (a newly generated ID when `target_window_id == 0`).
    /// - `moved_pane_id`: moved pane ID (equal to the `pane_id` argument).
    ///
    /// Error conditions:
    /// - `pane_id` is not found in any window.
    /// - `target_window_id != 0` and the window does not exist.
    /// - The last remaining pane of the source window is a serial pane (cannot be taken).
    pub fn move_pane(
        &mut self,
        pane_id: u32,
        target_window_id: u32,
        insert_at: Option<u32>,
    ) -> Result<(u32, u32, u32)> {
        let cols = self.cols;
        let rows = self.rows;

        // Identify the source window that contains the pane.
        let source_window_id = self
            .windows
            .iter()
            .find(|(_, w)| w.pane_ids().contains(&pane_id))
            .map(|(id, _)| *id)
            .ok_or_else(|| anyhow::anyhow!("pane {} not found", pane_id))?;

        // Moving from a window to itself is a no-op error.
        if source_window_id == target_window_id {
            return Err(anyhow::anyhow!(
                "pane {} is already in window {}",
                pane_id,
                target_window_id
            ));
        }

        // Check the source window's pane count (== 1 means we delete the window itself).
        let source_pane_count = self
            .windows
            .get(&source_window_id)
            .map(|w| w.pane_count())
            .unwrap_or(0);

        // Take the pane out (consume the window if it was the last pane).
        let pane = if source_pane_count <= 1 {
            // Last pane: remove the window and extract its only pane.
            let source = self
                .windows
                .remove(&source_window_id)
                .ok_or_else(|| anyhow::anyhow!("source window not found"))?;
            source.into_single_pane().ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot extract the last pane of window {} (unsupported pane such as serial)",
                    source_window_id
                )
            })?
        } else {
            let source = self
                .windows
                .get_mut(&source_window_id)
                .ok_or_else(|| anyhow::anyhow!("source window not found"))?;
            source.take_pane_by_id(pane_id, cols, rows).ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot take pane {} out of window {}",
                    pane_id,
                    source_window_id
                )
            })?
        };

        // Prepare the destination window.
        let new_window_id = if target_window_id == 0 {
            // Create a new window (same pattern as break_pane).
            let new_id = new_window_id();
            let new_window = Window::new_with_pane(new_id, "window-torn".to_string(), pane)?;
            self.windows.insert(new_id, new_window);
            new_id
        } else {
            // Add to an existing window.
            let target = self
                .windows
                .get_mut(&target_window_id)
                .ok_or_else(|| anyhow::anyhow!("window {} not found", target_window_id))?;
            // `Some(insert_at)` uses position-specified insertion, otherwise append (Phase 4-4).
            let position = insert_at.map(|i| i as usize);
            target.insert_pane_at(
                pane,
                cols,
                rows,
                crate::window::SplitDir::Vertical,
                position,
            );
            target_window_id
        };

        // Move focus to the destination (safe even if the source window was removed).
        self.focused_window_id = new_window_id;

        Ok((source_window_id, new_window_id, pane_id))
    }

    /// Move the focused pane to the specified window (join-pane).
    ///
    /// Returns the moved pane ID on success.
    pub fn join_pane(&mut self, target_window_id: u32) -> Result<u32> {
        let cols = self.cols;
        let rows = self.rows;
        // Stash the focused window ID (to satisfy the borrow checker).
        let focused_win_id = self.focused_window_id;
        if focused_win_id == target_window_id {
            return Err(anyhow::anyhow!(
                "destination is the same as the current window"
            ));
        }
        // Take the pane out.
        let pane = {
            let w = self
                .windows
                .get_mut(&focused_win_id)
                .ok_or_else(|| anyhow::anyhow!("focused window not found"))?;
            w.take_focused_pane(cols, rows)
                .ok_or_else(|| anyhow::anyhow!("cannot move the last remaining pane"))?
        };
        let pane_id = pane.id;
        // Insert into the destination window.
        let target = self
            .windows
            .get_mut(&target_window_id)
            .ok_or_else(|| anyhow::anyhow!("window {} not found", target_window_id))?;
        target.insert_pane(pane, cols, rows, crate::window::SplitDir::Vertical);
        self.focused_window_id = target_window_id;
        Ok(pane_id)
    }

    /// Set the broadcast mode.
    pub fn set_broadcast(&mut self, enabled: bool) {
        self.broadcast = enabled;
    }

    /// Return whether broadcast mode is active.
    #[allow(dead_code)]
    pub fn is_broadcast(&self) -> bool {
        self.broadcast
    }

    /// Broadcast mode: write to every pane of the focused window.
    pub fn write_to_all(&self, data: &[u8]) -> Result<()> {
        let window = self
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("focused window not found"))?;
        for pane_id in window.pane_ids() {
            if let Some(pane) = window.pane(pane_id) {
                let _ = pane.write_input(data);
            }
        }
        Ok(())
    }

    /// Write input to the focused pane of the focused window.
    pub fn write_to_focused(&self, data: &[u8]) -> Result<()> {
        if self.broadcast {
            self.write_to_all(data)
        } else {
            self.focused_window()
                .ok_or_else(|| anyhow::anyhow!("focused window not found"))?
                .write_to_focused(data)
        }
    }

    /// Return whether bracketed-paste mode is enabled on the focused pane.
    pub fn focused_bracketed_paste_mode(&self) -> bool {
        self.focused_window()
            .map(|w| w.focused_bracketed_paste_mode())
            .unwrap_or(false)
    }

    /// Return the focused pane's mouse-reporting mode (0 = disabled).
    pub fn focused_mouse_mode(&self) -> u8 {
        self.focused_window()
            .map(|w| w.focused_mouse_mode())
            .unwrap_or(0)
    }

    /// Return the focused pane's Kitty keyboard protocol flags (0 = disabled).
    pub fn focused_keyboard_protocol_flags(&self) -> u8 {
        self.focused_window()
            .map(|w| w.focused_keyboard_protocol_flags())
            .unwrap_or(0)
    }

    /// Resize the whole window (recompute every pane via BSP).
    pub fn resize_focused(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.cols = cols;
        self.rows = rows;
        let window = self
            .focused_window_mut()
            .ok_or_else(|| anyhow::anyhow!("focused window not found"))?;
        window.resize_all_panes(cols, rows);
        Ok(())
    }

    // ---- Snapshot ----

    /// Convert the session into a snapshot.
    pub fn to_snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            name: self.name.clone(),
            shell: self.shell.clone(),
            shell_args: self.shell_args.clone(),
            cols: self.cols,
            rows: self.rows,
            windows: self.windows.values().map(|w| w.to_snapshot()).collect(),
            focused_window_id: self.focused_window_id,
            session_title: None,
            workspace_name: self.workspace_name.clone(),
        }
    }

    /// Restore a session from a snapshot.
    ///
    /// Restore happens with no clients attached.
    /// `attach()` is called by the IPC layer once a client connects.
    pub fn restore_from_snapshot(snap: &SessionSnapshot) -> Result<Self> {
        // Create the broadcast channel (no receivers yet because no client is attached).
        let (broadcast_tx, _) = broadcast::channel::<ServerToClient>(2048);

        let mut windows = HashMap::new();
        for win_snap in &snap.windows {
            match Window::restore_from_snapshot(
                win_snap,
                &broadcast_tx,
                &snap.shell,
                snap.cols,
                snap.rows,
            ) {
                Ok(window) => {
                    windows.insert(win_snap.id, window);
                }
                Err(e) => {
                    // `{:#}` prints the full anyhow error chain so context added
                    // around the underlying ConPTY / spawn_command error (shell,
                    // cwd, cols, rows) shows up in the log.
                    warn!("failed to restore window '{}': {:#}", win_snap.name, e);
                }
            }
        }

        if windows.is_empty() {
            bail!("no window could be restored for session '{}'", snap.name);
        }

        Ok(Self {
            name: snap.name.clone(),
            windows,
            focused_window_id: snap.focused_window_id,
            broadcast_tx,
            shell: snap.shell.clone(),
            shell_args: snap.shell_args.clone(),
            cols: snap.cols,
            rows: snap.rows,
            broadcast: false,
            workspace_name: if snap.workspace_name.is_empty() {
                DEFAULT_WORKSPACE.to_string()
            } else {
                snap.workspace_name.clone()
            },
        })
    }
}

/// Session manager (manages every session).
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    /// Default shell configuration (loaded from config files).
    shell_config: nexterm_config::ShellConfig,
    /// WASM plugin manager (accepts load/unload commands over IPC).
    pub plugin_manager: Arc<std::sync::Mutex<Option<nexterm_plugin::PluginManager>>>,
    /// Workspace management state (Sprint 5-7 / Phase 2-1).
    ///
    /// Holds the set of known workspaces (always includes `default`) and the active name.
    workspace_state: Arc<Mutex<WorkspaceState>>,
    /// Queue of warning messages emitted at startup (Sprint 5-12 Phase 4).
    ///
    /// When `run_server` detects a minor error (e.g. config load failure), it stashes it here.
    /// On a client's first Attach, `take_startup_warnings()` drains the queue and emits the
    /// messages as `ServerToClient::Error`. A sync `Mutex` is used because this can be set from
    /// outside any Tokio task (early during `run_server`).
    startup_warnings: Arc<std::sync::Mutex<Vec<String>>>,
}

/// Internal workspace state held by `SessionManager`.
struct WorkspaceState {
    /// Set of known workspace names (`Vec` to preserve insertion order).
    known: Vec<String>,
    /// Currently active workspace name.
    current: String,
}

impl WorkspaceState {
    fn new() -> Self {
        Self {
            known: vec![DEFAULT_WORKSPACE.to_string()],
            current: DEFAULT_WORKSPACE.to_string(),
        }
    }
}

impl SessionManager {
    pub fn new(shell_config: nexterm_config::ShellConfig) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            shell_config,
            plugin_manager: Arc::new(std::sync::Mutex::new(None)),
            workspace_state: Arc::new(Mutex::new(WorkspaceState::new())),
            startup_warnings: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Set the plugin manager (called at server startup).
    pub fn set_plugin_manager(&self, mgr: nexterm_plugin::PluginManager) {
        let mut lock = self.plugin_manager.lock().expect("plugin_manager poisoned");
        *lock = Some(mgr);
    }

    /// Replace the startup warning messages (Sprint 5-12 Phase 4).
    ///
    /// Called when `run_server` fails to load configuration, for example.
    /// Normally drained on the first attach via `take_startup_warnings()`.
    pub fn set_startup_warnings(&self, warnings: Vec<String>) {
        if let Ok(mut guard) = self.startup_warnings.lock() {
            *guard = warnings;
        }
    }

    /// Take the accumulated startup warnings and empty the queue (Sprint 5-12 Phase 4).
    ///
    /// Called by the IPC handler on the first attach so the messages can be forwarded as
    /// `ServerToClient::Error`. If multiple clients attach simultaneously, only the first one
    /// receives them (the messages are transient and not persisted in the UI).
    pub fn take_startup_warnings(&self) -> Vec<String> {
        self.startup_warnings
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default()
    }

    /// Return an `Arc` to the sessions map (used by IPC handlers).
    pub fn sessions(&self) -> Arc<Mutex<HashMap<String, Session>>> {
        Arc::clone(&self.sessions)
    }

    /// Create a new session.
    #[allow(dead_code)]
    pub async fn create_session(&self, name: String, cols: u16, rows: u16) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(&name) {
            bail!("session '{}' already exists", name);
        }
        let shell = self.shell_config.program.clone();
        let args = self.shell_config.args.clone();
        let workspace = self.workspace_state.lock().await.current.clone();
        let session = Session::new_in_workspace(name.clone(), cols, rows, shell, args, workspace)?;
        sessions.insert(name.clone(), session);
        info!("created session '{}'", name);
        Ok(())
    }

    /// Attach to an existing session (returns a `broadcast::Receiver`).
    #[allow(dead_code)]
    pub async fn attach_session(&self, name: &str) -> Result<broadcast::Receiver<ServerToClient>> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("session '{}' not found", name))?;
        let rx = session.attach();
        info!("attached to session '{}'", name);
        Ok(rx)
    }

    /// Return the list of sessions.
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|s| s.info()).collect()
    }

    /// Phase 2c (UI/UX v2): inspect each pane's foreground process and
    /// broadcast a `ServerToClient::ProcessChanged` whenever the name
    /// changes versus `last_seen`. Called once per second by the
    /// server-wide polling task spawned in `run_server`.
    ///
    /// Iterates every session × every window × every pane (PTY panes
    /// only — serial panes have no shell to inspect). For each pane:
    ///   1. Run `Pane::foreground_process_name()` while holding the
    ///      session lock. OS-specific cost: ~µs on Linux, ~1 ms on
    ///      Windows, ~20 ms on macOS (one `ps` invocation per pane).
    ///   2. Diff against `last_seen.get(&pane_id)`. Broadcast only on
    ///      change; identical ticks are silent.
    ///   3. Clean up stale entries (panes that no longer exist) so the
    ///      map does not grow unbounded across the server's lifetime.
    ///
    /// The `last_seen` map is owned by the polling task; passing it in
    /// keeps `SessionManager` stateless on this axis (no extra `Mutex`).
    pub async fn poll_foreground_processes(
        &self,
        last_seen: &mut std::collections::HashMap<u32, Option<String>>,
    ) {
        // Snapshot (pane_id, current_name, broadcast_sender) tuples while
        // we hold the lock so we can run all broadcasts after dropping it
        // (broadcast::Sender::send is non-blocking, but we still avoid
        // holding the lock across the network/async boundary).
        let mut current: Vec<(u32, Option<String>, broadcast::Sender<ServerToClient>)> = Vec::new();
        {
            let sessions = self.sessions.lock().await;
            for session in sessions.values() {
                let tx = session.broadcast_sender();
                for window in session.windows.values() {
                    for pane_id in window.pane_ids() {
                        if let Some(pane) = window.pane(pane_id) {
                            let name = pane.foreground_process_name();
                            current.push((pane_id, name, tx.clone()));
                        }
                    }
                }
            }
        }

        // Track which pane IDs we observed so the cleanup pass can drop
        // entries for panes that have been removed since the last tick.
        let mut seen_this_tick: std::collections::HashSet<u32> =
            std::collections::HashSet::with_capacity(current.len());

        for (pane_id, name, tx) in current {
            seen_this_tick.insert(pane_id);
            let changed = last_seen.get(&pane_id) != Some(&name);
            if changed {
                last_seen.insert(pane_id, name.clone());
                // Broadcast::send fails only when there are no receivers,
                // which is fine — the next attach will pick up the latest
                // value from `last_seen` semantics via the standard
                // attach flow. Errors are ignored intentionally.
                let _ = tx.send(ServerToClient::ProcessChanged {
                    pane_id,
                    process_name: name,
                });
            }
        }

        // Drop entries for vanished panes so `last_seen` stays bounded.
        last_seen.retain(|pane_id, _| seen_this_tick.contains(pane_id));
    }

    /// Create the session if it does not exist; otherwise re-attach to it.
    pub async fn get_or_create_and_attach(&self, name: &str, cols: u16, rows: u16) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(name) {
            info!("re-attached to session '{}'", name);
        } else {
            let shell = self.shell_config.program.clone();
            let args = self.shell_config.args.clone();
            let workspace = self.workspace_state.lock().await.current.clone();
            let session =
                Session::new_in_workspace(name.to_string(), cols, rows, shell, args, workspace)?;
            sessions.insert(name.to_string(), session);
            info!("created new session '{}'", name);
        }
        Ok(())
    }

    /// Force-terminate a session (its PTYs are closed via `Drop`).
    pub async fn kill_session(&self, name: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if sessions.remove(name).is_some() {
            info!("terminated session '{}'", name);
            Ok(())
        } else {
            bail!("session '{}' not found", name)
        }
    }

    /// Start recording the session's focused pane.
    pub async fn start_recording(&self, name: &str, path: &str) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("session '{}' not found", name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("window not found"))?;
        let pane_id = window.start_recording(path)?;
        Ok(pane_id)
    }

    /// Start recording using log configuration (templates and binary log).
    ///
    /// When `log_config.file_name_template` is set, expand the template to generate the filename.
    pub async fn start_recording_with_log_config(
        &self,
        session_name: &str,
        base_dir: &str,
        log_config: &nexterm_config::LogConfig,
    ) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_name)
            .ok_or_else(|| anyhow::anyhow!("session '{}' not found", session_name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("window not found"))?;
        let pane = window
            .pane(window.focused_pane_id())
            .ok_or_else(|| anyhow::anyhow!("focused pane not found"))?;
        pane.start_recording_with_config(base_dir, session_name, log_config)?;
        Ok(pane.id)
    }

    /// Stop recording the session's focused pane (fully implemented in Phase 5-A).
    pub async fn stop_recording(&self, name: &str) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("session '{}' not found", name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("window not found"))?;
        let pane_id = window.stop_recording()?;
        Ok(pane_id)
    }

    /// Start an asciicast recording on the session's focused pane.
    pub async fn start_asciicast(&self, name: &str, path: &str) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("session '{}' not found", name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("window not found"))?;
        let pane_id = window.start_asciicast(path)?;
        Ok(pane_id)
    }

    /// Stop the asciicast recording on the session's focused pane.
    pub async fn stop_asciicast(&self, name: &str) -> Result<u32> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("session '{}' not found", name))?;
        let window = session
            .focused_window()
            .ok_or_else(|| anyhow::anyhow!("window not found"))?;
        let pane_id = window.stop_asciicast()?;
        Ok(pane_id)
    }

    /// Create a serial port pane and add it to the focused window.
    pub async fn connect_serial(
        &self,
        session_name: &str,
        port: &str,
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: &str,
    ) -> Result<u32> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_name)
            .ok_or_else(|| anyhow::anyhow!("session '{}' not found", session_name))?;
        let cols = session.cols;
        let rows = session.rows;
        let tx = session.broadcast_sender();
        let window = session
            .focused_window_mut()
            .ok_or_else(|| anyhow::anyhow!("focused window not found"))?;
        window.add_serial_pane(
            cols,
            rows,
            tx,
            port,
            baud_rate,
            data_bits,
            stop_bits,
            parity,
            crate::window::SplitDir::Vertical,
        )
    }

    // ---- Quake mode (Sprint 5-7 / Phase 2-2) ----

    /// Broadcast a Quake toggle request to every attached GPU client.
    ///
    /// Sends `ServerToClient::QuakeToggleRequest` through each session's `broadcast::Sender`.
    /// Sessions with no receivers (no attached clients) silently ignore the send.
    /// Returns the number of broadcast channels reached (= session count).
    pub async fn broadcast_quake_request(&self, action: &str) -> usize {
        let sessions = self.sessions.lock().await;
        let mut delivered = 0;
        for session in sessions.values() {
            let _ = session
                .broadcast_sender()
                .send(ServerToClient::QuakeToggleRequest {
                    action: action.to_string(),
                });
            delivered += 1;
        }
        delivered
    }

    // ---- Workspace management (Sprint 5-7 / Phase 2-1) ----

    /// Return the currently active workspace name (for tests and future hooks).
    #[allow(dead_code)]
    pub async fn current_workspace(&self) -> String {
        self.workspace_state.lock().await.current.clone()
    }

    /// Return info for every workspace (used by IPC `ListWorkspaces`).
    ///
    /// Session counts per workspace are aggregated from `sessions` via `workspace_name`.
    /// Known workspaces remain in the set even with zero sessions (kept until explicitly deleted).
    pub async fn list_workspaces(&self) -> (String, Vec<WorkspaceInfo>) {
        let state = self.workspace_state.lock().await;
        let sessions = self.sessions.lock().await;

        // Aggregate session counts.
        let mut counts: HashMap<String, u32> = HashMap::new();
        for session in sessions.values() {
            *counts.entry(session.workspace_name.clone()).or_insert(0) += 1;
        }

        let workspaces = state
            .known
            .iter()
            .map(|name| WorkspaceInfo {
                name: name.clone(),
                session_count: counts.get(name).copied().unwrap_or(0),
                is_active: name == &state.current,
            })
            .collect();
        (state.current.clone(), workspaces)
    }

    /// Create a new workspace.
    pub async fn create_workspace(&self, name: &str) -> Result<()> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            bail!("workspace name is empty");
        }
        let mut state = self.workspace_state.lock().await;
        if state.known.iter().any(|w| w == trimmed) {
            bail!("workspace '{}' already exists", trimmed);
        }
        state.known.push(trimmed.to_string());
        info!("created workspace '{}'", trimmed);
        Ok(())
    }

    /// Switch the currently active workspace.
    ///
    /// Returns the post-switch workspace name.
    pub async fn switch_workspace(&self, name: &str) -> Result<String> {
        let mut state = self.workspace_state.lock().await;
        if !state.known.iter().any(|w| w == name) {
            bail!("workspace '{}' not found", name);
        }
        state.current = name.to_string();
        info!("switched to workspace '{}'", name);
        Ok(state.current.clone())
    }

    /// Rename a workspace.
    ///
    /// Updates the `workspace_name` of every session that belongs to it.
    pub async fn rename_workspace(&self, from: &str, to: &str) -> Result<()> {
        let to_trimmed = to.trim();
        if to_trimmed.is_empty() {
            bail!("new workspace name is empty");
        }
        if from == to_trimmed {
            return Ok(());
        }
        let mut state = self.workspace_state.lock().await;
        if !state.known.iter().any(|w| w == from) {
            bail!("workspace '{}' not found", from);
        }
        if state.known.iter().any(|w| w == to_trimmed) {
            bail!("workspace '{}' already exists", to_trimmed);
        }
        if from == DEFAULT_WORKSPACE {
            bail!(
                "cannot rename the default workspace '{}'",
                DEFAULT_WORKSPACE
            );
        }

        // Replace the name inside `known`.
        for w in state.known.iter_mut() {
            if w == from {
                *w = to_trimmed.to_string();
            }
        }
        // Update `current` as well.
        if state.current == from {
            state.current = to_trimmed.to_string();
        }
        // Update `workspace_name` on every session too.
        let mut sessions = self.sessions.lock().await;
        for session in sessions.values_mut() {
            if session.workspace_name == from {
                session.workspace_name = to_trimmed.to_string();
            }
        }
        info!("renamed workspace '{}' to '{}'", from, to_trimmed);
        Ok(())
    }

    /// Delete a workspace.
    ///
    /// `default` cannot be deleted. If sessions still belong to it, pass `force=true` to migrate
    /// them to `default` and force the deletion. When the deleted workspace was current, current
    /// reverts to `default`.
    pub async fn delete_workspace(&self, name: &str, force: bool) -> Result<()> {
        if name == DEFAULT_WORKSPACE {
            bail!(
                "cannot delete the default workspace '{}'",
                DEFAULT_WORKSPACE
            );
        }
        let mut state = self.workspace_state.lock().await;
        if !state.known.iter().any(|w| w == name) {
            bail!("workspace '{}' not found", name);
        }
        // Check the number of sessions belonging to it.
        let mut sessions = self.sessions.lock().await;
        let session_count = sessions
            .values()
            .filter(|s| s.workspace_name == name)
            .count();
        if session_count > 0 && !force {
            bail!(
                "workspace '{}' still has {} session(s); retry with force=true",
                name,
                session_count
            );
        }
        // force=true: migrate sessions to default.
        if force {
            for session in sessions.values_mut() {
                if session.workspace_name == name {
                    session.workspace_name = DEFAULT_WORKSPACE.to_string();
                }
            }
        }
        // Remove from `known`.
        state.known.retain(|w| w != name);
        // If it was current, revert to default.
        if state.current == name {
            state.current = DEFAULT_WORKSPACE.to_string();
        }
        info!("deleted workspace '{}' (force={})", name, force);
        Ok(())
    }

    // ---- Snapshot ----

    /// Convert every session into a snapshot.
    pub async fn to_snapshot(&self) -> ServerSnapshot {
        let sessions = self.sessions.lock().await;
        let current_workspace = self.workspace_state.lock().await.current.clone();
        let saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        ServerSnapshot {
            version: SNAPSHOT_VERSION,
            sessions: sessions.values().map(|s| s.to_snapshot()).collect(),
            saved_at,
            current_workspace,
            // OS window placement is client-side state, so server-side `to_snapshot` returns an
            // empty Vec. Actual placement should be filled in by the IPC layer using values
            // received from the client (for now an empty array preserves v4 compatibility; a
            // dedicated IPC for this will land later).
            client_os_windows: Vec::new(),
        }
    }

    /// Restore every session from a snapshot.
    ///
    /// Sessions with a version mismatch or restore error are logged as warnings and skipped.
    /// Returns the names of sessions that were restored successfully.
    pub async fn restore_from_snapshot(&self, snap: &ServerSnapshot) -> Vec<String> {
        // `persist::load_snapshot()` already migrates, so only check the MIN..MAX range here.
        if snap.version < SNAPSHOT_VERSION_MIN || snap.version > SNAPSHOT_VERSION {
            warn!(
                "snapshot version out of supported range (got={}, supported={}..{}); skipping restore",
                snap.version, SNAPSHOT_VERSION_MIN, SNAPSHOT_VERSION
            );
            return Vec::new();
        }

        let mut sessions = self.sessions.lock().await;
        let mut restored = Vec::new();
        // Workspaces from restored sessions (added to the known set later).
        let mut restored_workspaces: Vec<String> = Vec::new();

        for sess_snap in &snap.sessions {
            if sessions.contains_key(&sess_snap.name) {
                info!("session '{}' already exists; skipping", sess_snap.name);
                continue;
            }
            match Session::restore_from_snapshot(sess_snap) {
                Ok(session) => {
                    let ws = session.workspace_name.clone();
                    if !ws.is_empty() && !restored_workspaces.contains(&ws) {
                        restored_workspaces.push(ws);
                    }
                    sessions.insert(sess_snap.name.clone(), session);
                    restored.push(sess_snap.name.clone());
                    info!("restored session '{}'", sess_snap.name);
                }
                Err(e) => {
                    warn!("failed to restore session '{}': {}", sess_snap.name, e);
                }
            }
        }
        drop(sessions);

        // Restore workspace state.
        let mut state = self.workspace_state.lock().await;
        for ws in restored_workspaces {
            if !state.known.iter().any(|w| w == &ws) {
                state.known.push(ws);
            }
        }
        // Restore `current_workspace` (use the value if it's in the known set; otherwise keep `default`).
        if !snap.current_workspace.is_empty()
            && state.known.iter().any(|w| w == &snap.current_workspace)
        {
            state.current = snap.current_workspace.clone();
        }

        restored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_config_default_is_non_empty() {
        let cfg = nexterm_config::ShellConfig::default();
        assert!(!cfg.program.is_empty());
    }

    #[tokio::test]
    async fn session_list_starts_empty() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let list = manager.list_sessions().await;
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn session_lookup_for_missing_name_returns_none() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let arc = manager.sessions();
        let sessions = arc.lock().await;
        assert!(sessions.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn kill_session_for_missing_name_returns_err() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let result = manager.kill_session("nonexistent").await;
        assert!(
            result.is_err(),
            "killing a nonexistent session should return Err"
        );
    }

    #[tokio::test]
    async fn session_list_starts_empty_at_init() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        let list = manager.list_sessions().await;
        assert_eq!(list.len(), 0, "the list must be empty at init");
    }

    // ---- Workspace API unit tests (no PTY required) ----

    #[tokio::test]
    async fn workspace_initial_state() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        assert_eq!(manager.current_workspace().await, DEFAULT_WORKSPACE);
        let (current, list) = manager.list_workspaces().await;
        assert_eq!(current, DEFAULT_WORKSPACE);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, DEFAULT_WORKSPACE);
        assert!(list[0].is_active);
        assert_eq!(list[0].session_count, 0);
    }

    #[tokio::test]
    async fn workspace_create_and_switch() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("dev").await.unwrap();

        let (_, list) = manager.list_workspaces().await;
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|w| w.name == "dev"));

        manager.switch_workspace("dev").await.unwrap();
        assert_eq!(manager.current_workspace().await, "dev");

        // Switching to a nonexistent workspace returns an error.
        assert!(manager.switch_workspace("unknown").await.is_err());
    }

    #[tokio::test]
    async fn workspace_duplicate_create_is_error() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("dev").await.unwrap();
        assert!(manager.create_workspace("dev").await.is_err());
        assert!(manager.create_workspace("").await.is_err());
        assert!(manager.create_workspace("   ").await.is_err());
    }

    #[tokio::test]
    async fn workspace_delete() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("tmp").await.unwrap();
        manager.delete_workspace("tmp", false).await.unwrap();
        let (_, list) = manager.list_workspaces().await;
        assert_eq!(list.len(), 1);

        // Default cannot be deleted.
        assert!(
            manager
                .delete_workspace(DEFAULT_WORKSPACE, true)
                .await
                .is_err()
        );

        // Deleting a nonexistent workspace returns an error.
        assert!(manager.delete_workspace("ghost", false).await.is_err());
    }

    #[tokio::test]
    async fn workspace_delete_reverts_active_to_default() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("dev").await.unwrap();
        manager.switch_workspace("dev").await.unwrap();
        manager.delete_workspace("dev", false).await.unwrap();
        assert_eq!(manager.current_workspace().await, DEFAULT_WORKSPACE);
    }

    #[tokio::test]
    async fn workspace_rename() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager.create_workspace("old").await.unwrap();
        manager.switch_workspace("old").await.unwrap();
        manager.rename_workspace("old", "new").await.unwrap();
        assert_eq!(manager.current_workspace().await, "new");
        let (_, list) = manager.list_workspaces().await;
        assert!(list.iter().any(|w| w.name == "new"));
        assert!(!list.iter().any(|w| w.name == "old"));

        // Renaming default is forbidden.
        assert!(
            manager
                .rename_workspace(DEFAULT_WORKSPACE, "x")
                .await
                .is_err()
        );

        // Renaming a nonexistent name returns an error.
        assert!(manager.rename_workspace("ghost", "y").await.is_err());

        // Collision with an existing name returns an error.
        manager.create_workspace("a").await.unwrap();
        manager.create_workspace("b").await.unwrap();
        assert!(manager.rename_workspace("a", "b").await.is_err());
    }
    // ── Tests that spawn an actual PTY ─────────────────────────────────────────
    //
    // The following 4 tests actually spawn a shell (PowerShell / $SHELL); when the test ends,
    // `Pane::Drop` closes the `MasterPty`.
    //
    // Interactive shells do not receive a termination command, so the close wait loops forever
    // and the `#[tokio::test]` runtime hangs while waiting on the blocking task. (Observed on
    // Windows ConPTY and Linux PTY; only on macOS does EOF occasionally propagate fast enough
    // that the test happens to pass.)
    //
    // To keep CI green, these are marked `#[ignore]` and skipped from the default run. For
    // local/manual verification use:
    //   `cargo test --workspace --all-targets -- --include-ignored`.
    //
    // A proper fix requires avoiding the portable-pty Drop hang (e.g. introducing an explicit
    // `kill_child` API or process isolation). Once that is in place we can drop `#[ignore]`.

    #[tokio::test]
    #[ignore = "spawns a PTY; hangs on interactive shell close in regular CI"]
    async fn session_new_creates_valid_session() {
        let shell = nexterm_config::ShellConfig::default();
        let session = Session::new(
            "test-session".to_string(),
            80,
            24,
            shell.program,
            shell.args,
        )
        .unwrap();

        assert_eq!(session.name, "test-session");
        assert_eq!(session.cols, 80);
        assert_eq!(session.rows, 24);
        assert_eq!(session.windows.len(), 1);
        assert!(!session.broadcast);
    }

    #[tokio::test]
    #[ignore = "spawns a PTY; hangs on interactive shell close in regular CI"]
    async fn session_info_returns_correct_metadata() {
        let shell = nexterm_config::ShellConfig::default();
        let session = Session::new("test".to_string(), 80, 24, shell.program, shell.args).unwrap();

        let info = session.info();
        assert_eq!(info.name, "test");
        assert_eq!(info.window_count, 1);
        assert!(!info.attached);
        assert_eq!(info.workspace_name, DEFAULT_WORKSPACE);
    }

    #[tokio::test]
    #[ignore = "spawns a PTY; hangs on interactive shell close in regular CI"]
    async fn session_manager_create_new_session() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());

        let result = manager
            .get_or_create_and_attach("new-session", 80, 24)
            .await;
        assert!(result.is_ok());

        let list = manager.list_sessions().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "new-session");
    }

    #[tokio::test]
    #[ignore = "spawns a PTY; hangs on interactive shell close in regular CI"]
    async fn session_manager_kill_existing_session() {
        let manager = SessionManager::new(nexterm_config::ShellConfig::default());
        manager
            .get_or_create_and_attach("to-kill", 80, 24)
            .await
            .unwrap();

        assert_eq!(manager.list_sessions().await.len(), 1);

        let result = manager.kill_session("to-kill").await;
        assert!(result.is_ok());

        assert_eq!(manager.list_sessions().await.len(), 0);
    }
}
