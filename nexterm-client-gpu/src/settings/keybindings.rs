//! Key-binding list entries, the allowed action list, and the
//! Keybindings category's list management (add/delete/select) and
//! action cycling. In-flight key/leader text editing lives in
//! `keybindings_edit.rs`.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::{SettingsPanel, TextInputState};
use nexterm_i18n::fl;

/// Key binding entry (Phase 5-11-9 Sub-phase A: editable inside the settings panel).
///
/// A lightweight mirror of `nexterm-config::KeyBinding`. Sub-phase A populates
/// the list from `Config.keys` for display only; Sub-phase B/C/D add Record-mode
/// key capture, Action ComboBox cycling, and Add/Delete UI respectively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindingEntry {
    /// Key string (e.g. `"ctrl+shift+p"`, `"ctrl+b d"`). Matches the format
    /// accepted by `nexterm_client_gpu::key_map::config_key_matches_token`.
    pub key: String,
    /// Action name (e.g. `"CommandPalette"`). Must be one of the 27 actions
    /// dispatched by `execute_action` in `renderer::input_handler::action`.
    pub action: String,
}

impl KeyBindingEntry {
    /// Build the one-line label rendered / announced by the UI / SR.
    /// Example: `"ctrl+shift+p → CommandPalette"`.
    pub fn label(&self) -> String {
        let key = if self.key.is_empty() {
            fl!("settings-keybinding-unbound")
        } else {
            self.key.clone()
        };
        let action = if self.action.is_empty() {
            fl!("settings-keybinding-none")
        } else {
            self.action.clone()
        };
        format!("{} → {}", key, action)
    }
}

/// Phase 5-11-9 Sub-phase B: in-flight edit state for the key string of the
/// currently selected binding (`selected_key_index`).
///
/// Two modes:
///   - `Record`: the next non-modifier key press is captured via
///     `format_key_event` and committed to the binding. Useful for simple
///     combos like `ctrl+shift+p`.
///   - `Text(state)`: free-form text editing. Required for prefix bindings
///     (e.g. `"ctrl+b d"`) that cannot be expressed by a single physical
///     press. Tab toggles between modes; Enter commits Text mode; Esc cancels.
#[derive(Debug, Clone)]
pub enum KeyEditMode {
    /// Awaiting the next physical key press.
    Record,
    /// Free-form text edit (cursor + IME preedit aware).
    Text(TextInputState),
}

/// Allowed action names (Phase 5-11-9 Sub-phase A).
///
/// Mirror of the 27 `match` arms in `renderer::input_handler::action::execute_action`.
/// Used by Sub-phase C to populate the Action ComboBox.
/// Q2 decision: fixed list (no free-form input) to prevent silent typos.
pub const KEYBINDING_ACTIONS: &[&str] = &[
    "Quit",
    "SearchScrollback",
    "SplitVertical",
    "SplitHorizontal",
    "FocusNextPane",
    "FocusPrevPane",
    "ClosePane",
    "NewWindow",
    "Detach",
    "CommandPalette",
    "SetBroadcastOn",
    "SetBroadcastOff",
    "ToggleZoom",
    "QuickSelect",
    "SwapPaneNext",
    "SwapPanePrev",
    "BreakPane",
    "ShowSettings",
    "ShowHostManager",
    "ShowMacroPicker",
    "SftpUploadDialog",
    "SftpDownloadDialog",
    "ConnectSerialPrompt",
    "JumpPrevPrompt",
    "JumpNextPrompt",
    "DetachToNewWindow",
    "CloseOsWindow",
];

impl SettingsPanel {
    /// Start Record mode for the currently selected binding's key field.
    /// Returns `true` when edit mode actually started; `false` if no binding
    /// is selected.
    pub fn begin_key_record(&mut self) -> bool {
        if self.keybindings.is_empty() {
            return false;
        }
        if self.selected_key_index >= self.keybindings.len() {
            return false;
        }
        self.key_editing = Some(KeyEditMode::Record);
        true
    }

    /// Toggle between Record and Text mode. No-op if not editing.
    /// On Record → Text, the current binding's key value seeds the buffer.
    /// On Text → Record, the in-flight buffer is discarded.
    pub fn toggle_key_edit_mode(&mut self) {
        match self.key_editing.take() {
            Some(KeyEditMode::Record) => {
                let initial = self
                    .keybindings
                    .get(self.selected_key_index)
                    .map(|kb| kb.key.clone())
                    .unwrap_or_default();
                self.key_editing = Some(KeyEditMode::Text(TextInputState::new(initial)));
            }
            Some(KeyEditMode::Text(_)) => {
                self.key_editing = Some(KeyEditMode::Record);
            }
            None => {}
        }
    }

    /// In Record mode, set the binding's key to `formatted` (e.g. the result
    /// of `format_key_event`) and leave edit mode. No-op outside Record mode.
    /// Returns `true` when the binding was updated.
    pub fn capture_key_record(&mut self, formatted: String) -> bool {
        if !matches!(self.key_editing, Some(KeyEditMode::Record)) {
            return false;
        }
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            self.key_editing = None;
            return false;
        };
        kb.key = formatted;
        self.key_editing = None;
        self.dirty = true;
        true
    }

    /// P3 (WT-like UX): the binding whose key chord collides with the
    /// currently selected one, if any — see [`find_key_conflict`]. Drives
    /// the warning line under the keybindings list.
    pub fn selected_key_conflict(&self) -> Option<&KeyBindingEntry> {
        find_key_conflict(&self.keybindings, self.selected_key_index)
            .and_then(|i| self.keybindings.get(i))
    }

    /// Cycle the selected binding's `action` to the next entry in
    /// `KEYBINDING_ACTIONS`. Returns `true` when the value was updated.
    /// No-op when no binding is selected.
    pub fn next_key_action(&mut self) -> bool {
        let actions = KEYBINDING_ACTIONS;
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        let current = actions.iter().position(|&a| a == kb.action);
        let next_index = match current {
            Some(i) => (i + 1) % actions.len(),
            // Unknown action: snap to the first known entry rather than
            // staying silently invalid.
            None => 0,
        };
        kb.action = actions[next_index].to_string();
        self.dirty = true;
        true
    }

    /// Cycle the selected binding's `action` to the previous entry in
    /// `KEYBINDING_ACTIONS`. Returns `true` when the value was updated.
    /// Unknown values snap to the last known action.
    pub fn prev_key_action(&mut self) -> bool {
        let actions = KEYBINDING_ACTIONS;
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        let prev_index = match actions.iter().position(|&a| a == kb.action) {
            Some(i) => (i + actions.len() - 1) % actions.len(),
            // Unknown action: snap to the last known entry.
            None => actions.len() - 1,
        };
        kb.action = actions[prev_index].to_string();
        self.dirty = true;
        true
    }

    /// Append a fresh key binding with safe defaults and start Record-mode
    /// editing on the key field.
    pub fn add_key_binding(&mut self) {
        let new_binding = KeyBindingEntry {
            key: String::new(),
            action: KEYBINDING_ACTIONS[0].to_string(),
        };
        self.keybindings.push(new_binding);
        self.selected_key_index = self.keybindings.len() - 1;
        self.key_field_focus = 1;
        // Immediately enter Record mode — the next key press becomes the binding.
        self.key_editing = Some(KeyEditMode::Record);
        self.dirty = true;
    }

    /// Open the delete-confirmation dialog. No-op when the list is empty
    /// (treated as disabled). Default focus is Cancel.
    pub fn open_key_delete_dialog(&mut self) {
        if self.keybindings.is_empty() {
            return;
        }
        self.key_delete_dialog_open = true;
        self.key_delete_dialog_confirm_focused = false;
    }

    /// Close the delete-confirmation dialog without deleting.
    pub fn cancel_key_delete_dialog(&mut self) {
        self.key_delete_dialog_open = false;
        self.key_delete_dialog_confirm_focused = false;
    }

    /// Delete the selected binding and close the dialog.
    ///
    /// Selection clamp: if the deleted index was the tail, fall back to n-1.
    /// When the list becomes empty, reset focus to the ListBox (`key_field_focus = 0`).
    pub fn confirm_key_delete_dialog(&mut self) {
        if self.selected_key_index < self.keybindings.len() {
            self.keybindings.remove(self.selected_key_index);
            if !self.keybindings.is_empty() && self.selected_key_index >= self.keybindings.len() {
                self.selected_key_index = self.keybindings.len() - 1;
            }
            if self.keybindings.is_empty() {
                self.selected_key_index = 0;
                self.key_field_focus = 0;
            }
            self.dirty = true;
        }
        self.key_delete_dialog_open = false;
        self.key_delete_dialog_confirm_focused = false;
    }

    /// Toggle focus in the delete-confirmation dialog (Confirm ↔ Cancel).
    pub fn toggle_key_delete_dialog_focus(&mut self) {
        self.key_delete_dialog_confirm_focused = !self.key_delete_dialog_confirm_focused;
    }

    /// Convenience predicate: returns `true` when the key field is in Record mode.
    pub fn is_key_recording(&self) -> bool {
        matches!(self.key_editing, Some(KeyEditMode::Record))
    }

    /// Phase 5-11-9 Sub-phase E: directly overwrite the selected binding's key
    /// string. Used by the AccessKit `Action::SetValue` path so screen-reader
    /// users can write a key spelling like `"ctrl+b d"` without entering
    /// Record/Text mode. Cancels any in-flight edit mode. Returns `true` when
    /// the binding was updated.
    pub fn set_keybinding_key_direct(&mut self, value: String) -> bool {
        if self.keybindings.is_empty() {
            return false;
        }
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        kb.key = value;
        self.key_editing = None;
        self.dirty = true;
        true
    }

    /// Phase 5-11-9 Sub-phase E: directly overwrite the selected binding's
    /// action string. Used by the AccessKit `Action::SetValue` path on the
    /// Action ComboBox. The caller is expected to pass a string that appears in
    /// `KEYBINDING_ACTIONS`; values outside that list are accepted but flagged
    /// as a no-op by returning `false`.
    pub fn set_keybinding_action_direct(&mut self, value: &str) -> bool {
        if !KEYBINDING_ACTIONS.contains(&value) {
            return false;
        }
        if self.keybindings.is_empty() {
            return false;
        }
        let Some(kb) = self.keybindings.get_mut(self.selected_key_index) else {
            return false;
        };
        kb.action = value.to_string();
        self.dirty = true;
        true
    }

    /// Convenience predicate: returns `true` when the key field is in Text mode.
    pub fn is_key_text_editing(&self) -> bool {
        matches!(self.key_editing, Some(KeyEditMode::Text(_)))
    }
}

/// P3 (WT-like UX): index of another binding using the same key chord as
/// `bindings[idx]` (case-insensitive, whitespace-normalized), or `None`.
/// Pure so the duplicate rule can be pinned by tests.
pub fn find_key_conflict(bindings: &[KeyBindingEntry], idx: usize) -> Option<usize> {
    let target = bindings.get(idx)?;
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let key = norm(&target.key);
    if key.is_empty() {
        return None;
    }
    bindings
        .iter()
        .enumerate()
        .find(|(i, b)| *i != idx && norm(&b.key) == key)
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn keybinding_entry_label_normal() {
        nexterm_i18n::set_locale("en");
        let kb = KeyBindingEntry {
            key: "ctrl+shift+p".to_string(),
            action: "CommandPalette".to_string(),
        };
        assert_eq!(kb.label(), "ctrl+shift+p → CommandPalette");
    }

    #[test]
    fn keybinding_entry_label_empty_key_or_action() {
        nexterm_i18n::set_locale("en");
        let kb = KeyBindingEntry {
            key: String::new(),
            action: "Quit".to_string(),
        };
        assert_eq!(kb.label(), "(unbound) → Quit");
        let kb2 = KeyBindingEntry {
            key: "ctrl+b d".to_string(),
            action: String::new(),
        };
        assert_eq!(kb2.label(), "ctrl+b d → (none)");
    }

    #[test]
    fn keybindings_loaded_from_default_config() {
        let config = Config::default();
        let panel = SettingsPanel::new(&config);
        // The default config defines several key bindings; the panel must
        // mirror them 1:1 with matching length, key strings, and action names.
        assert_eq!(panel.keybindings.len(), config.keys.len());
        for (i, kb) in panel.keybindings.iter().enumerate() {
            assert_eq!(kb.key, config.keys[i].key);
            assert_eq!(kb.action, config.keys[i].action);
        }
        assert_eq!(panel.selected_key_index, 0);
        assert_eq!(panel.key_field_focus, 0);
    }

    #[test]
    fn keybindings_empty_when_config_keys_empty() {
        let mut config = Config::default();
        config.keys.clear();
        let panel = SettingsPanel::new(&config);
        assert!(panel.keybindings.is_empty());
        assert_eq!(panel.selected_key_index, 0);
        assert_eq!(panel.key_field_focus, 0);
    }

    #[test]
    fn keybinding_actions_contains_known_actions() {
        // Sanity check: a representative subset of actions exists in the table.
        for name in [
            "Quit",
            "CommandPalette",
            "SplitVertical",
            "DetachToNewWindow",
            "CloseOsWindow",
        ] {
            assert!(
                KEYBINDING_ACTIONS.contains(&name),
                "KEYBINDING_ACTIONS must include `{name}`"
            );
        }
        // No duplicates allowed.
        let mut sorted: Vec<&&str> = KEYBINDING_ACTIONS.iter().collect();
        sorted.sort();
        let dedup_len = {
            let mut s = sorted.clone();
            s.dedup();
            s.len()
        };
        assert_eq!(sorted.len(), dedup_len, "KEYBINDING_ACTIONS has duplicates");
    }

    fn panel_with_one_binding() -> SettingsPanel {
        SettingsPanel {
            keybindings: vec![KeyBindingEntry {
                key: "ctrl+shift+p".to_string(),
                action: "CommandPalette".to_string(),
            }],
            selected_key_index: 0,
            ..Default::default()
        }
    }

    #[test]
    fn begin_key_record_starts_record_mode() {
        let mut panel = panel_with_one_binding();
        assert!(panel.begin_key_record());
        assert!(panel.is_key_recording());
        assert!(!panel.is_key_text_editing());
    }

    #[test]
    fn begin_key_record_noop_when_empty() {
        let mut panel = SettingsPanel::default();
        panel.keybindings.clear();
        assert!(!panel.begin_key_record());
        assert!(panel.key_editing.is_none());
    }

    #[test]
    fn capture_key_record_writes_back_and_exits() {
        let mut panel = panel_with_one_binding();
        panel.begin_key_record();
        assert!(panel.capture_key_record("ctrl+q".to_string()));
        assert_eq!(panel.keybindings[0].key, "ctrl+q");
        assert!(panel.key_editing.is_none());
        assert!(panel.dirty);
    }

    #[test]
    fn capture_key_record_noop_when_not_recording() {
        let mut panel = panel_with_one_binding();
        // Without begin_key_record, capture must do nothing.
        assert!(!panel.capture_key_record("ctrl+q".to_string()));
        assert_eq!(panel.keybindings[0].key, "ctrl+shift+p");
    }

    #[test]
    fn next_key_action_cycles_forward_through_full_list() {
        let mut panel = panel_with_one_binding();
        // Seed with the first action so the cycle is deterministic.
        panel.keybindings[0].action = KEYBINDING_ACTIONS[0].to_string();
        panel.dirty = false;
        for i in 0..KEYBINDING_ACTIONS.len() {
            assert!(panel.next_key_action());
            let expected = KEYBINDING_ACTIONS[(i + 1) % KEYBINDING_ACTIONS.len()];
            assert_eq!(panel.keybindings[0].action, expected);
        }
        // After a full cycle we are back at index 0.
        assert_eq!(panel.keybindings[0].action, KEYBINDING_ACTIONS[0]);
        assert!(panel.dirty);
    }

    #[test]
    fn prev_key_action_cycles_backward_through_full_list() {
        let mut panel = panel_with_one_binding();
        panel.keybindings[0].action = KEYBINDING_ACTIONS[0].to_string();
        panel.dirty = false;
        // First step wraps to the last action.
        assert!(panel.prev_key_action());
        assert_eq!(
            panel.keybindings[0].action,
            KEYBINDING_ACTIONS[KEYBINDING_ACTIONS.len() - 1]
        );
        assert!(panel.dirty);
    }

    #[test]
    fn next_key_action_snaps_unknown_to_first() {
        let mut panel = panel_with_one_binding();
        panel.keybindings[0].action = "BogusAction".to_string();
        panel.dirty = false;
        assert!(panel.next_key_action());
        assert_eq!(panel.keybindings[0].action, KEYBINDING_ACTIONS[0]);
        assert!(panel.dirty);
    }

    #[test]
    fn prev_key_action_snaps_unknown_to_last() {
        let mut panel = panel_with_one_binding();
        panel.keybindings[0].action = "TypoHere".to_string();
        panel.dirty = false;
        assert!(panel.prev_key_action());
        assert_eq!(
            panel.keybindings[0].action,
            KEYBINDING_ACTIONS[KEYBINDING_ACTIONS.len() - 1]
        );
        assert!(panel.dirty);
    }

    #[test]
    fn key_action_cycles_noop_when_empty() {
        let mut panel = SettingsPanel::default();
        panel.keybindings.clear();
        assert!(!panel.next_key_action());
        assert!(!panel.prev_key_action());
        assert!(!panel.dirty);
    }

    #[test]
    fn next_key_action_does_not_touch_key_field() {
        let mut panel = panel_with_one_binding();
        let key_before = panel.keybindings[0].key.clone();
        panel.next_key_action();
        // Only `action` should change. The key field is owned by Sub-phase B.
        assert_eq!(panel.keybindings[0].key, key_before);
    }

    #[test]
    fn add_key_binding_appends_with_defaults_and_enters_record_mode() {
        let mut panel = SettingsPanel::default();
        panel.keybindings.clear();
        panel.dirty = false;
        panel.add_key_binding();
        assert_eq!(panel.keybindings.len(), 1);
        assert_eq!(panel.keybindings[0].key, "");
        assert_eq!(panel.keybindings[0].action, KEYBINDING_ACTIONS[0]);
        assert_eq!(panel.selected_key_index, 0);
        assert_eq!(panel.key_field_focus, 1);
        assert!(panel.is_key_recording());
        assert!(panel.dirty);
    }

    #[test]
    fn add_key_binding_extends_existing_list() {
        let mut panel = panel_with_one_binding();
        panel.add_key_binding();
        assert_eq!(panel.keybindings.len(), 2);
        assert_eq!(panel.selected_key_index, 1);
        assert!(panel.is_key_recording());
    }

    #[test]
    fn open_key_delete_dialog_noop_when_empty() {
        let mut panel = SettingsPanel::default();
        panel.keybindings.clear();
        panel.open_key_delete_dialog();
        assert!(
            !panel.key_delete_dialog_open,
            "must not open dialog for empty list"
        );
    }

    #[test]
    fn open_key_delete_dialog_defaults_to_cancel_focus() {
        let mut panel = panel_with_one_binding();
        panel.open_key_delete_dialog();
        assert!(panel.key_delete_dialog_open);
        assert!(
            !panel.key_delete_dialog_confirm_focused,
            "default focus must be Cancel (accident guard)"
        );
    }

    #[test]
    fn cancel_key_delete_dialog_clears_state_and_keeps_binding() {
        let mut panel = panel_with_one_binding();
        panel.open_key_delete_dialog();
        panel.key_delete_dialog_confirm_focused = true;
        panel.cancel_key_delete_dialog();
        assert_eq!(panel.keybindings.len(), 1);
        assert!(!panel.key_delete_dialog_open);
        assert!(!panel.key_delete_dialog_confirm_focused);
    }

    #[test]
    fn confirm_key_delete_dialog_removes_at_end_clamps_to_prev() {
        let mut panel = panel_with_one_binding();
        panel.add_key_binding();
        // selected_key_index is now 1 (last). Delete it.
        panel.open_key_delete_dialog();
        panel.confirm_key_delete_dialog();
        assert_eq!(panel.keybindings.len(), 1);
        // Selection clamps to n-1 = 0.
        assert_eq!(panel.selected_key_index, 0);
        assert!(!panel.key_delete_dialog_open);
    }

    #[test]
    fn confirm_key_delete_dialog_in_middle_keeps_index() {
        let mut panel = panel_with_one_binding();
        panel.add_key_binding();
        panel.add_key_binding();
        // Three entries; select middle (index 1).
        panel.selected_key_index = 1;
        panel.open_key_delete_dialog();
        panel.confirm_key_delete_dialog();
        assert_eq!(panel.keybindings.len(), 2);
        // Middle delete shifts later entries up; selection stays at 1.
        assert_eq!(panel.selected_key_index, 1);
    }

    #[test]
    fn confirm_key_delete_dialog_emptying_resets_focus() {
        let mut panel = panel_with_one_binding();
        panel.key_field_focus = 4;
        panel.open_key_delete_dialog();
        panel.confirm_key_delete_dialog();
        assert!(panel.keybindings.is_empty());
        assert_eq!(panel.selected_key_index, 0);
        assert_eq!(
            panel.key_field_focus, 0,
            "empty list must restore ListBox focus"
        );
    }

    #[test]
    fn toggle_key_delete_dialog_focus_alternates() {
        let mut panel = panel_with_one_binding();
        panel.open_key_delete_dialog();
        assert!(!panel.key_delete_dialog_confirm_focused);
        panel.toggle_key_delete_dialog_focus();
        assert!(panel.key_delete_dialog_confirm_focused);
        panel.toggle_key_delete_dialog_focus();
        assert!(!panel.key_delete_dialog_confirm_focused);
    }

    #[test]
    fn close_panel_resets_key_delete_dialog() {
        let mut panel = panel_with_one_binding();
        panel.open_key_delete_dialog();
        panel.key_delete_dialog_confirm_focused = true;
        panel.close();
        assert!(!panel.key_delete_dialog_open);
        assert!(!panel.key_delete_dialog_confirm_focused);
    }
}

#[cfg(test)]
mod conflict_tests {
    //! P3: duplicate-chord detection tests.
    use super::*;

    fn kb(key: &str, action: &str) -> KeyBindingEntry {
        KeyBindingEntry {
            key: key.to_string(),
            action: action.to_string(),
        }
    }

    #[test]
    fn detects_case_insensitive_duplicates() {
        let bindings = vec![
            kb("Ctrl+Shift+P", "CommandPalette"),
            kb("ctrl+shift+p", "Quit"),
        ];
        assert_eq!(find_key_conflict(&bindings, 0), Some(1));
        assert_eq!(find_key_conflict(&bindings, 1), Some(0));
    }

    #[test]
    fn normalizes_prefix_chord_whitespace() {
        let bindings = vec![kb("ctrl+b  d", "Detach"), kb("ctrl+b d", "ClosePane")];
        assert_eq!(find_key_conflict(&bindings, 0), Some(1));
    }

    #[test]
    fn unique_keys_and_empty_keys_do_not_conflict() {
        let bindings = vec![
            kb("ctrl+a", "SelectAll"),
            kb("ctrl+b", "Quit"),
            kb("", "Detach"),
        ];
        assert_eq!(find_key_conflict(&bindings, 0), None);
        // An empty (still-unset) key must never report a conflict.
        assert_eq!(find_key_conflict(&bindings, 2), None);
        // Out-of-range index is a no-op.
        assert_eq!(find_key_conflict(&bindings, 9), None);
    }
}
