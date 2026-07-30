//! Menus / dialogs — context menu, file transfer, Quick Select
//!
//! Extracted from `state/mod.rs`:
//! - `ContextMenuAction` / `ContextMenuItem` / `ContextMenu` — right-click menu
//! - `FileTransferDialog` — SFTP upload / download dialog
//! - `QuickSelectMatch` / `QuickSelectState` — Quick Select mode that highlights URL /
//!   Email / Path etc. on the grid with labels for fast selection
//! - `find_quick_select_matches` — extract matches from the entire grid via regex
//!   (with priority-based overlap control)

use nexterm_i18n::fl;

// ---- Context menu ----

/// Action executed by each context menu entry
#[derive(Debug, Clone, PartialEq)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    SelectAll,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    InlineSearch,
    OpenSettings,
    /// Open a shell using the named profile
    OpenProfile {
        profile_name: String,
    },
    /// Separator (not clickable)
    Separator,
    /// Detach the current pane into a new OS Window (Sprint 5-8 Phase 4-5, Wayland alt UX #1)
    DetachToNewWindow,
    /// Close only the current OS Window (Sprint 5-8 Phase 4-5, CloseOsWindow path #1)
    CloseOsWindow,
    // ---- Custom title bar system menu (non-Windows fallback) ----
    /// Minimize the OS window.
    MinimizeWindow,
    /// Maximize the OS window, or restore it when already maximized.
    ToggleMaximizeWindow,
    /// Close the OS window through the native-close path, so
    /// `window.close_action` (prompt / detach / quit) still applies —
    /// unlike [`Self::CloseOsWindow`], which destroys the window directly.
    RequestCloseWindow,
    // ---- Phase 2c-follow-up: command-block items (block-aware right-click) ----
    /// Copy the prompt + output of the right-clicked block to the clipboard.
    /// The `block_id` is the `BlockId` (u64) of the block under the cursor.
    CopyBlock {
        block_id: u64,
    },
    /// Replay the right-clicked block's command line into the focused pane.
    ReplayBlock {
        block_id: u64,
    },
    /// Toggle the collapsed flag on the right-clicked block.
    ToggleBlockCollapse {
        block_id: u64,
    },
    /// Open the block-name input modal pre-populated for the right-clicked
    /// block. Reuses `ClientState::open_block_name_modal_for`.
    SetBlockName {
        block_id: u64,
    },
    /// Remove the persisted name for the right-clicked block. No-op if none.
    RemoveBlockName {
        block_id: u64,
    },
}

/// A single entry in the context menu
#[derive(Debug, Clone)]
pub struct ContextMenuItem {
    pub label: String,
    /// Key hint (shown faintly on the right)
    pub hint: String,
    pub action: ContextMenuAction,
}

impl ContextMenuItem {
    fn new(label: impl Into<String>, action: ContextMenuAction) -> Self {
        Self {
            label: label.into(),
            hint: String::new(),
            action,
        }
    }

    fn with_hint(
        label: impl Into<String>,
        hint: impl Into<String>,
        action: ContextMenuAction,
    ) -> Self {
        Self {
            label: label.into(),
            hint: hint.into(),
            action,
        }
    }

    fn separator() -> Self {
        Self {
            label: String::new(),
            hint: String::new(),
            action: ContextMenuAction::Separator,
        }
    }
}

/// Context menu shown via right-click
#[derive(Debug, Clone)]
pub struct ContextMenu {
    /// Pixel coordinates where the menu is displayed (top-left)
    pub x: f32,
    pub y: f32,
    pub items: Vec<ContextMenuItem>,
    /// Currently hovered item index
    pub hovered: Option<usize>,
}

impl ContextMenu {
    /// Build a context menu populated with the default entries.
    /// `profiles`: list of (profile name, icon) pairs
    pub fn new_default(x: f32, y: f32, profiles: &[(String, String)]) -> Self {
        let mut items = vec![
            ContextMenuItem::with_hint("Copy", "Ctrl+C", ContextMenuAction::Copy),
            ContextMenuItem::with_hint("Paste", "Ctrl+V", ContextMenuAction::Paste),
            ContextMenuItem::with_hint("Select All", "Ctrl+A", ContextMenuAction::SelectAll),
            ContextMenuItem::separator(),
            ContextMenuItem::with_hint(
                "Split Vertical",
                "Ctrl+B  %",
                ContextMenuAction::SplitVertical,
            ),
            ContextMenuItem::with_hint(
                "Split Horizontal",
                "Ctrl+B  \"",
                ContextMenuAction::SplitHorizontal,
            ),
            ContextMenuItem::with_hint("Close Pane", "Ctrl+B  x", ContextMenuAction::ClosePane),
        ];

        // Append a sub-section if any profiles are registered.
        if !profiles.is_empty() {
            items.push(ContextMenuItem::separator());
            for (name, icon) in profiles {
                let label = if icon.is_empty() {
                    format!("> {}", name)
                } else {
                    format!("{} {}", icon, name)
                };
                items.push(ContextMenuItem::new(
                    label,
                    ContextMenuAction::OpenProfile {
                        profile_name: name.clone(),
                    },
                ));
            }
        }

        items.push(ContextMenuItem::separator());
        items.push(ContextMenuItem::with_hint(
            "Search...",
            "Ctrl+F",
            ContextMenuAction::InlineSearch,
        ));
        items.push(ContextMenuItem::with_hint(
            "Settings...",
            "Ctrl+,",
            ContextMenuAction::OpenSettings,
        ));

        // Sprint 5-8 / Phase 4-5: tab-tearing-related entries (Wayland alternative UX).
        // 8-language support via the i18n keys; no hint (no key binding assigned).
        items.push(ContextMenuItem::separator());
        items.push(ContextMenuItem::new(
            fl!("context-menu-detach-to-new-window"),
            ContextMenuAction::DetachToNewWindow,
        ));
        items.push(ContextMenuItem::new(
            fl!("context-menu-close-this-os-window"),
            ContextMenuAction::CloseOsWindow,
        ));

        Self {
            x,
            y,
            items,
            hovered: None,
        }
    }

    /// Phase 2c follow-up: like `new_default` but adds a block-actions
    /// sub-section at the top when the right-click landed inside a known
    /// block. `block_id` identifies the target; `has_name` controls whether
    /// the "Remove name" entry is shown (no point offering it if no name is
    /// stored). The block-action labels go through the existing i18n keys.
    pub fn new_for_block(
        x: f32,
        y: f32,
        profiles: &[(String, String)],
        block_id: u64,
        has_name: bool,
    ) -> Self {
        let mut menu = Self::new_default(x, y, profiles);
        // Prepend block actions + a separator at the top of the menu so the
        // block-specific entries are the first thing the user sees.
        let mut block_items: Vec<ContextMenuItem> = Vec::with_capacity(6);
        block_items.push(ContextMenuItem::with_hint(
            fl!("context-menu-block-copy"),
            "Ctrl+Shift+C",
            ContextMenuAction::CopyBlock { block_id },
        ));
        block_items.push(ContextMenuItem::with_hint(
            fl!("context-menu-block-replay"),
            "Ctrl+Shift+R",
            ContextMenuAction::ReplayBlock { block_id },
        ));
        block_items.push(ContextMenuItem::with_hint(
            fl!("context-menu-block-toggle-collapse"),
            "Ctrl+Shift+/",
            ContextMenuAction::ToggleBlockCollapse { block_id },
        ));
        block_items.push(ContextMenuItem::with_hint(
            fl!("context-menu-block-set-name"),
            "Ctrl+Shift+L",
            ContextMenuAction::SetBlockName { block_id },
        ));
        if has_name {
            block_items.push(ContextMenuItem::with_hint(
                fl!("context-menu-block-remove-name"),
                "Ctrl+Shift+X",
                ContextMenuAction::RemoveBlockName { block_id },
            ));
        }
        block_items.push(ContextMenuItem::separator());

        // Splice the block items at the start of the existing items list.
        menu.items.splice(0..0, block_items);
        menu
    }

    /// Build the new-tab dropdown opened by the tab-bar `▾` button
    /// (Windows-Terminal-like profile dropdown).
    ///
    /// `profiles`: (name, icon) pairs — the configured `Config.profiles`
    /// followed by the WSL distros detected at startup. The return value is
    /// stored in `ClientState.context_menu`, so it reuses the existing
    /// context-menu rendering and hit-testing machinery unchanged.
    pub fn new_tab_dropdown(x: f32, y: f32, profiles: &[(String, String)]) -> Self {
        let mut items = vec![ContextMenuItem::with_hint(
            fl!("tab-dropdown-new-tab"),
            "Ctrl+B  %",
            ContextMenuAction::SplitVertical,
        )];
        if !profiles.is_empty() {
            items.push(ContextMenuItem::separator());
            for (name, icon) in profiles {
                let label = if icon.is_empty() {
                    format!("> {}", name)
                } else {
                    format!("{} {}", icon, name)
                };
                items.push(ContextMenuItem::new(
                    label,
                    ContextMenuAction::OpenProfile {
                        profile_name: name.clone(),
                    },
                ));
            }
        }
        items.push(ContextMenuItem::separator());
        items.push(ContextMenuItem::with_hint(
            fl!("palette-show-settings"),
            "Ctrl+,",
            ContextMenuAction::OpenSettings,
        ));
        Self {
            x,
            y,
            items,
            hovered: None,
        }
    }

    /// System-menu replacement for the custom title bar on platforms where
    /// winit's `show_window_menu` is unsupported (everything but Windows).
    ///
    /// Mirrors the native menu's core: maximize *or* restore (depending on
    /// the current state), minimize, and close. "Move" / "Size" are
    /// intentionally omitted — they would need a keyboard-driven move/resize
    /// mode that is not worth the cost next to the drag/edge affordances.
    pub fn new_window_system_menu(x: f32, y: f32, is_maximized: bool) -> Self {
        let maximize_label = if is_maximized {
            fl!("context-menu-window-restore")
        } else {
            fl!("context-menu-window-maximize")
        };
        let items = vec![
            ContextMenuItem::new(maximize_label, ContextMenuAction::ToggleMaximizeWindow),
            ContextMenuItem::new(
                fl!("context-menu-window-minimize"),
                ContextMenuAction::MinimizeWindow,
            ),
            ContextMenuItem::separator(),
            ContextMenuItem::new(
                fl!("context-menu-close-this-os-window"),
                ContextMenuAction::RequestCloseWindow,
            ),
        ];
        Self {
            x,
            y,
            items,
            hovered: None,
        }
    }
}

/// Resolve a configuration profile into the IPC message that opens a new tab
/// running it (PROTOCOL_VERSION 11 `SplitWithShell`).
///
/// * Profiles that override nothing spawn-related (no `[profiles.shell]`,
///   no `working_dir`, no `env`) fall back to a plain `SplitVertical` —
///   font/color-only profiles still get a new tab with the session shell.
/// * Otherwise the profile's shell (or `default_shell` when absent) is sent
///   together with the cwd / env overrides. `env` is sorted because
///   `HashMap` iteration order is unstable.
pub fn split_message_for_profile(
    profile: &nexterm_config::Profile,
    default_shell: &nexterm_config::ShellConfig,
) -> nexterm_proto::ClientToServer {
    if profile.shell.is_none() && profile.working_dir.is_none() && profile.env.is_empty() {
        return nexterm_proto::ClientToServer::SplitVertical;
    }
    let shell = profile.shell.as_ref().unwrap_or(default_shell);
    let mut env: Vec<(String, String)> = profile
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    env.sort();
    nexterm_proto::ClientToServer::SplitWithShell {
        program: shell.program.clone(),
        args: shell.args.clone(),
        cwd: profile.working_dir.clone(),
        env,
    }
}

// ---- File transfer dialog ----

/// State of the file transfer dialog
pub struct FileTransferDialog {
    pub is_open: bool,
    /// "upload" or "download"
    pub mode: String,
    /// Input field index (0 = host name, 1 = local path, 2 = remote path)
    pub field: usize,
    pub host_name: String,
    pub local_path: String,
    pub remote_path: String,
}

impl FileTransferDialog {
    pub fn new() -> Self {
        Self {
            is_open: false,
            mode: "upload".to_string(),
            field: 0,
            host_name: String::new(),
            local_path: String::new(),
            remote_path: String::new(),
        }
    }

    pub fn open_upload(&mut self) {
        self.mode = "upload".to_string();
        self.field = 0;
        self.host_name.clear();
        self.local_path.clear();
        self.remote_path.clear();
        self.is_open = true;
    }

    pub fn open_download(&mut self) {
        self.mode = "download".to_string();
        self.field = 0;
        self.host_name.clear();
        self.local_path.clear();
        self.remote_path.clear();
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn current_field_mut(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.host_name,
            1 => &mut self.local_path,
            _ => &mut self.remote_path,
        }
    }

    pub fn next_field(&mut self) {
        self.field = (self.field + 1).min(2);
    }

    pub fn prev_field(&mut self) {
        self.field = self.field.saturating_sub(1);
    }
}

// ---- Quick Select ----

/// Match result in Quick Select mode
#[derive(Debug, Clone)]
pub struct QuickSelectMatch {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub text: String,
    /// Selection label (a, b, c, ... / aa, ab, ...)
    pub label: String,
}

/// State of Quick Select mode
pub struct QuickSelectState {
    pub is_active: bool,
    pub matches: Vec<QuickSelectMatch>,
    /// Label currently being typed
    pub typed_label: String,
}

impl QuickSelectState {
    pub(super) fn new() -> Self {
        Self {
            is_active: false,
            matches: Vec::new(),
            typed_label: String::new(),
        }
    }

    pub fn enter(&mut self, grid_rows: &[Vec<nexterm_proto::Cell>]) {
        self.is_active = true;
        self.typed_label.clear();
        self.matches = find_quick_select_matches(grid_rows);
    }

    pub fn exit(&mut self) {
        self.is_active = false;
        self.matches.clear();
        self.typed_label.clear();
    }

    /// Returns the match whose label equals the typed label
    pub fn accept(&self) -> Option<&QuickSelectMatch> {
        if self.typed_label.is_empty() {
            return None;
        }
        self.matches.iter().find(|m| m.label == self.typed_label)
    }
}

/// Find Quick Select matches in the grid.
///
/// The pattern set was expanded in Sprint 5-4 / D1. When match ranges overlap,
/// the earlier (more specific) pattern wins.
pub(super) fn find_quick_select_matches(
    rows: &[Vec<nexterm_proto::Cell>],
) -> Vec<QuickSelectMatch> {
    // In priority order (earliest = highest):
    //   1. URL (taken first so later path/IPv4 patterns don't steal matches)
    //   2. Email
    //   3. UUID
    //   4. file:line:col form (with line number, for editor jump)
    //   5. Jira ticket (`PROJ-123`)
    //   6. Unix path
    //   7. Windows path (`C:\foo\bar`)
    //   8. IPv4 / IPv6
    //   9. SHA / Git hash
    //  10. Standalone number (last — only when nothing else matched)
    let patterns: &[&str] = &[
        // URL (http/https/ftp)
        r#"\b(?:https?|ftp)://[^\s<>"'\]]+"#,
        // Email
        r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b",
        // UUID v1-v5 (8-4-4-4-12 hex)
        r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
        // file:line[:col] form (e.g. src/main.rs:42 or src/main.rs:42:10)
        r"[A-Za-z0-9_./\\-]+\.[A-Za-z0-9]+:\d+(?::\d+)?",
        // Jira / issue ticket ID (e.g. PROJ-123, ABC-9999)
        r"\b[A-Z][A-Z0-9]{1,9}-\d+\b",
        // Unix path
        r"(?:^|[\s(])((?:/[^\s/:]+)+/?)",
        // Windows path (e.g. C:\foo\bar)
        r#"\b[A-Za-z]:\\[^\s<>:"|?*]+"#,
        // IPv4 address (port optional)
        r"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?\b",
        // IPv6 address (loose: at least two hex groups separated by colons)
        r"\b(?:[0-9a-fA-F]{1,4}:){2,7}[0-9a-fA-F]{1,4}\b",
        // SHA / Git hash (7-40 hex)
        r"\b[0-9a-f]{7,40}\b",
        // Standalone number
        r"\b\d+\b",
    ];

    // Compile the regexes once (avoid pattern-count x row-count recompilation).
    let compiled: Vec<regex::Regex> = patterns
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect();

    let mut all_matches: Vec<QuickSelectMatch> = Vec::new();

    for (row_idx, cells) in rows.iter().enumerate() {
        let line: String = cells.iter().map(|c| c.ch).collect();
        // Track "occupied column ranges" per row so a later pattern does not steal
        // a range already claimed by an earlier, higher-priority pattern.
        let mut occupied: Vec<(usize, usize)> = Vec::new();

        for re in &compiled {
            for m in re.find_iter(&line) {
                let (start, end) = (m.start(), m.end());
                // Skip if it overlaps an existing match (prefer higher-priority pattern)
                let overlaps = occupied.iter().any(|(s, e)| !(end <= *s || start >= *e));
                if overlaps {
                    continue;
                }
                occupied.push((start, end));
                all_matches.push(QuickSelectMatch {
                    row: row_idx as u16,
                    col_start: start as u16,
                    col_end: end as u16,
                    text: m.as_str().to_string(),
                    label: String::new(), // assigned later
                });
            }
        }
    }

    // Assign labels (a, b, ..., z, aa, ab, ...)
    let label_chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
    let n = all_matches.len();
    for (i, m) in all_matches.iter_mut().enumerate() {
        m.label = index_to_label(i, n, &label_chars);
    }

    all_matches
}

fn index_to_label(i: usize, total: usize, chars: &[char]) -> String {
    let base = chars.len();
    if total <= base {
        return chars[i % base].to_string();
    }
    let second = i / base;
    let first = i % base;
    if second == 0 {
        chars[first].to_string()
    } else {
        format!("{}{}", chars[second - 1], chars[first])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::{Profile, ShellConfig};
    use nexterm_proto::ClientToServer;

    fn default_shell() -> ShellConfig {
        ShellConfig {
            program: "/bin/bash".to_string(),
            args: vec!["-l".to_string()],
        }
    }

    // ---- split_message_for_profile ----

    #[test]
    fn font_only_profile_falls_back_to_plain_split() {
        // A profile that overrides nothing spawn-related must not force a
        // shell override; the session default shell handles it server-side.
        let profile = Profile {
            name: "big-font".to_string(),
            ..Default::default()
        };
        let msg = split_message_for_profile(&profile, &default_shell());
        assert_eq!(msg, ClientToServer::SplitVertical);
    }

    #[test]
    fn shell_profile_maps_to_split_with_shell() {
        let profile = Profile {
            name: "fish".to_string(),
            shell: Some(ShellConfig {
                program: "/usr/bin/fish".to_string(),
                args: vec!["--login".to_string()],
            }),
            working_dir: Some("/tmp".to_string()),
            ..Default::default()
        };
        let msg = split_message_for_profile(&profile, &default_shell());
        match msg {
            ClientToServer::SplitWithShell {
                program,
                args,
                cwd,
                env,
            } => {
                assert_eq!(program, "/usr/bin/fish");
                assert_eq!(args, vec!["--login".to_string()]);
                assert_eq!(cwd.as_deref(), Some("/tmp"));
                assert!(env.is_empty());
            }
            other => panic!("expected SplitWithShell, got {:?}", other),
        }
    }

    #[test]
    fn cwd_only_profile_uses_the_default_shell() {
        // working_dir without [profiles.shell] must still open a new tab in
        // that directory, running the session default shell.
        let profile = Profile {
            name: "project".to_string(),
            working_dir: Some("/home/user/project".to_string()),
            ..Default::default()
        };
        let msg = split_message_for_profile(&profile, &default_shell());
        match msg {
            ClientToServer::SplitWithShell {
                program, args, cwd, ..
            } => {
                assert_eq!(program, "/bin/bash");
                assert_eq!(args, vec!["-l".to_string()]);
                assert_eq!(cwd.as_deref(), Some("/home/user/project"));
            }
            other => panic!("expected SplitWithShell, got {:?}", other),
        }
    }

    #[test]
    fn profile_env_is_sorted_for_determinism() {
        let mut profile = Profile {
            name: "env".to_string(),
            shell: Some(default_shell()),
            ..Default::default()
        };
        profile.env.insert("ZZZ".to_string(), "1".to_string());
        profile.env.insert("AAA".to_string(), "2".to_string());
        let msg = split_message_for_profile(&profile, &default_shell());
        match msg {
            ClientToServer::SplitWithShell { env, .. } => {
                assert_eq!(
                    env,
                    vec![
                        ("AAA".to_string(), "2".to_string()),
                        ("ZZZ".to_string(), "1".to_string()),
                    ]
                );
            }
            other => panic!("expected SplitWithShell, got {:?}", other),
        }
    }

    // ---- ContextMenu::new_window_system_menu ----

    #[test]
    fn window_system_menu_toggles_between_maximize_and_restore() {
        // Not maximized: maximize / minimize / separator / close.
        let menu = ContextMenu::new_window_system_menu(10.0, 20.0, false);
        assert_eq!(menu.items.len(), 4);
        assert_eq!(
            menu.items[0].action,
            ContextMenuAction::ToggleMaximizeWindow
        );
        assert_eq!(menu.items[1].action, ContextMenuAction::MinimizeWindow);
        assert_eq!(menu.items[2].action, ContextMenuAction::Separator);
        assert_eq!(menu.items[3].action, ContextMenuAction::RequestCloseWindow);

        // Maximized: same shape, but the first label switches to "restore"
        // (same toggle action either way).
        let maximized = ContextMenu::new_window_system_menu(0.0, 0.0, true);
        assert_eq!(maximized.items.len(), 4);
        assert_eq!(
            maximized.items[0].action,
            ContextMenuAction::ToggleMaximizeWindow
        );
        assert_ne!(menu.items[0].label, maximized.items[0].label);
    }

    // ---- ContextMenu::new_tab_dropdown ----

    #[test]
    fn dropdown_without_profiles_has_new_tab_and_settings() {
        let menu = ContextMenu::new_tab_dropdown(10.0, 20.0, &[]);
        assert_eq!(menu.items.len(), 3); // new tab / separator / settings
        assert_eq!(menu.items[0].action, ContextMenuAction::SplitVertical);
        assert_eq!(menu.items[1].action, ContextMenuAction::Separator);
        assert_eq!(menu.items[2].action, ContextMenuAction::OpenSettings);
    }

    #[test]
    fn dropdown_lists_profiles_between_separators() {
        let profiles = vec![
            ("dev".to_string(), String::new()),
            ("Ubuntu".to_string(), "@".to_string()),
        ];
        let menu = ContextMenu::new_tab_dropdown(0.0, 0.0, &profiles);
        // new tab / sep / dev / Ubuntu / sep / settings
        assert_eq!(menu.items.len(), 6);
        assert_eq!(
            menu.items[2].action,
            ContextMenuAction::OpenProfile {
                profile_name: "dev".to_string()
            }
        );
        // Empty icon gets the "> " prefix; a real icon is prepended verbatim.
        assert_eq!(menu.items[2].label, "> dev");
        assert_eq!(menu.items[3].label, "@ Ubuntu");
        assert_eq!(menu.items[5].action, ContextMenuAction::OpenSettings);
    }
}
