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
