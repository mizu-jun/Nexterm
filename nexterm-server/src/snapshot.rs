//! Session snapshot type definitions.
//!
//! Defines serializable data structures used to save and restore session state across server
//! restarts.
//!
//! # Persisted state
//!
//! - Session name, shell, terminal size.
//! - Window names and focus state.
//! - BSP split tree (pane IDs, split directions, ratios).
//! - Each pane's working directory (only obtained from `/proc/{pid}/cwd` on Linux).
//!
//! # Restore limitations
//!
//! PTY processes themselves cannot be restored, so we spawn new PTY processes with the saved
//! shell and working directory.
//! Scrollback content is not saved (future work).
//!
//! # Schema version history
//!
//! - v1: initial version (`shell_args` added later, made compatible with `#[serde(default)]`).
//! - v2: add the `session_title` field. v1 snapshots can be auto-migrated.
//! - v3: Sprint 5-7 / Phase 2-1 — add `SessionSnapshot.workspace_name` so sessions can be grouped
//!   by workspace. v2 and earlier snapshots are auto-migrated into the `default` workspace.
//! - v4: Sprint 5-8 / Phase 4-5 — add `ServerSnapshot.client_os_windows` to save the client-side
//!   OS window placement (position, size, set of attached server windows). v3 and earlier
//!   snapshots fill the field with an empty `Vec` and restore as a single-OS-window setup.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Snapshot schema version.
///
/// Bumped whenever the format changes.
/// Older snapshots are migrated by `persist::load_snapshot`.
pub const SNAPSHOT_VERSION: u32 = 4;

/// Minimum supported version retained for backward-compatible reading (v1).
///
/// Planned to be bumped to `2` at v2.0.0.
/// See ADR-0007 (`docs/adr/0007-snapshot-v1-deprecation.md`) for details.
pub const SNAPSHOT_VERSION_MIN: u32 = 1;

/// Default workspace name. Used for new sessions and when restoring older snapshots.
pub const DEFAULT_WORKSPACE: &str = "default";

fn default_workspace() -> String {
    DEFAULT_WORKSPACE.to_string()
}

/// Server-wide snapshot (top-level unit of persistence).
#[derive(Debug, Serialize, Deserialize)]
pub struct ServerSnapshot {
    /// Schema version.
    pub version: u32,
    /// All sessions at save time.
    pub sessions: Vec<SessionSnapshot>,
    /// Unix timestamp (seconds) at save time.
    pub saved_at: u64,
    /// Workspace that was active at save time (added in v3; defaults to `default`).
    #[serde(default = "default_workspace")]
    pub current_workspace: String,
    /// Placement of client-side OS windows (added in v4).
    ///
    /// Saves the multi-OS-window state produced by tab tearing.
    /// An empty `Vec` restores as a single-OS-window setup (v3 and earlier compatibility).
    #[serde(default)]
    pub client_os_windows: Vec<OsWindowSnapshot>,
}

/// Snapshot of a client-side OS window (added in v4).
///
/// Records the position and size of a winit native window opened within the same process, along
/// with the set of server windows displayed in it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsWindowSnapshot {
    /// Top-left screen coordinates (x, y) in pixels.
    pub position: (i32, i32),
    /// Outer window size (width, height) in pixels.
    pub size: (u32, u32),
    /// Server window IDs assigned to this OS window (shown as tabs).
    pub server_window_ids: Vec<u32>,
    /// The server window ID that was active in this OS window.
    pub focused_server_window_id: u32,
}

/// Snapshot of a session.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Session name.
    pub name: String,
    /// Launch shell command.
    pub shell: String,
    /// Shell launch arguments (e.g. `["-NoLogo"]` for PowerShell).
    #[serde(default)]
    pub shell_args: Vec<String>,
    /// Number of terminal columns.
    pub cols: u16,
    /// Number of terminal rows.
    pub rows: u16,
    /// Window list.
    pub windows: Vec<WindowSnapshot>,
    /// Focused window ID.
    pub focused_window_id: u32,
    /// Displayed session title (added in v2; falls back to the session name when omitted).
    #[serde(default)]
    pub session_title: Option<String>,
    /// Owning workspace name (added in v3; defaults to `default`).
    #[serde(default = "default_workspace")]
    pub workspace_name: String,
}

/// Snapshot of a window.
#[derive(Debug, Serialize, Deserialize)]
pub struct WindowSnapshot {
    /// Window ID.
    pub id: u32,
    /// Window name.
    pub name: String,
    /// Focused pane ID.
    pub focused_pane_id: u32,
    /// BSP split tree.
    pub layout: SplitNodeSnapshot,
}

/// Snapshot of a BSP split tree.
#[derive(Debug, Serialize, Deserialize)]
pub enum SplitNodeSnapshot {
    /// A single pane.
    Pane {
        /// Pane ID.
        pane_id: u32,
        /// Working directory (only obtainable on Linux).
        cwd: Option<PathBuf>,
    },
    /// A split node.
    Split {
        /// Split direction.
        dir: SplitDirSnapshot,
        /// Occupancy ratio for the left/top child (0.0..1.0).
        ratio: f32,
        /// Left/top child node.
        left: Box<SplitNodeSnapshot>,
        /// Right/bottom child node.
        right: Box<SplitNodeSnapshot>,
    },
}

/// Snapshot of a split direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitDirSnapshot {
    /// Vertical split (left/right).
    Vertical,
    /// Horizontal split (top/bottom).
    Horizontal,
}
