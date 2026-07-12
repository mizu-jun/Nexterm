//! SSH host list entries and the SSH category's field editing / add /
//! delete-confirmation flows.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::{SettingsPanel, TextInputState};

/// SSH host entry (Phase 5-11-8 Step 8-1: display-only inside the settings panel).
///
/// A lightweight subset of `nexterm-config::HostConfig` that keeps only the
/// fields needed for SR / settings-panel display. Step 8-2 / 8-3 will extend
/// the struct when edit functionality lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHostEntry {
    /// Display name (`HostConfig.name`).
    pub name: String,
    /// Hostname or IP address.
    pub host: String,
    /// SSH port.
    pub port: u16,
    /// Username.
    pub username: String,
    /// Authentication type (`"password"` / `"key"` / `"agent"`).
    pub auth_type: String,
}

impl SshHostEntry {
    /// Build the one-line label rendered / announced by the UI / SR.
    /// Example: `"myhost (alice@example.com:2222)"`.
    pub fn label(&self) -> String {
        let user_part = if self.username.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.username, self.host)
        };
        let endpoint = if self.port == 22 || self.port == 0 {
            user_part
        } else {
            format!("{}:{}", user_part, self.port)
        };
        if self.name.is_empty() {
            endpoint
        } else {
            format!("{} ({})", self.name, endpoint)
        }
    }
}

impl SettingsPanel {
    /// Allowed auth_type values (matches the `HostConfig` serde spec).
    pub const SSH_AUTH_TYPES: &'static [&'static str] = &["password", "key", "agent"];

    /// Return a mutable reference to the currently-selected host (if any).
    fn selected_ssh_host_mut(&mut self) -> Option<&mut SshHostEntry> {
        self.ssh_hosts.get_mut(self.selected_host_index)
    }

    /// Update the `name` field (TextInput SetValue path).
    pub fn set_ssh_host_name(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.name = text;
            self.dirty = true;
        }
    }

    /// Update the `host` field (TextInput SetValue path).
    pub fn set_ssh_host_host(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.host = text;
            self.dirty = true;
        }
    }

    /// Update the `username` field (TextInput SetValue path).
    pub fn set_ssh_host_username(&mut self, text: String) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.username = text;
            self.dirty = true;
        }
    }

    /// Update the `port` field (SpinButton SetValue path).
    /// Clamps f64 to u16 (1..=65535).
    pub fn set_ssh_host_port_value(&mut self, v: f64) {
        let clamped = v.round().clamp(1.0, 65535.0) as u16;
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = clamped;
            self.dirty = true;
        }
    }

    /// Increment `port` by 1 (SpinButton Increment path; saturates at 65535).
    /// `u16::saturating_add` saturates at 65535 automatically, so `.min()` is unnecessary.
    pub fn increase_ssh_host_port(&mut self) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = host.port.saturating_add(1);
            self.dirty = true;
        }
    }

    /// Decrement `port` by 1 (SpinButton Decrement path; saturates at 1).
    pub fn decrease_ssh_host_port(&mut self) {
        if let Some(host) = self.selected_ssh_host_mut() {
            host.port = host.port.saturating_sub(1).max(1);
            self.dirty = true;
        }
    }

    /// Advance `auth_type` to the next value (ComboBox Click / Increment path).
    /// Cycles through `SSH_AUTH_TYPES`. If the current value is unknown, resets
    /// to the first entry.
    pub fn next_ssh_auth_type(&mut self) {
        let types = Self::SSH_AUTH_TYPES;
        if let Some(host) = self.selected_ssh_host_mut() {
            let current = types.iter().position(|&t| t == host.auth_type).unwrap_or(0);
            host.auth_type = types[(current + 1) % types.len()].to_string();
            self.dirty = true;
        }
    }

    /// Move `auth_type` to the previous value (ComboBox Decrement path).
    pub fn prev_ssh_auth_type(&mut self) {
        let types = Self::SSH_AUTH_TYPES;
        if let Some(host) = self.selected_ssh_host_mut() {
            let current = types.iter().position(|&t| t == host.auth_type).unwrap_or(0);
            host.auth_type = types[(current + types.len() - 1) % types.len()].to_string();
            self.dirty = true;
        }
    }

    /// Append a new SSH host and start editing it (the Add button path).
    ///
    /// Default values: `name=""`, `host=""`, `port=22`, `username=""`,
    /// `auth_type="password"`. After appending, the selection moves to
    /// `selected_host_index = ssh_hosts.len() - 1`, `ssh_field_focus` becomes
    /// 1 (name), and `begin_ssh_field_edit()` is called so SR users can start
    /// typing the name immediately.
    pub fn add_ssh_host(&mut self) {
        let new_host = SshHostEntry {
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            auth_type: "password".to_string(),
        };
        self.ssh_hosts.push(new_host);
        self.selected_host_index = self.ssh_hosts.len() - 1;
        self.ssh_field_focus = 1;
        // Immediately enter edit mode on the name field.
        self.ssh_field_editing = Some(TextInputState::new(String::new()));
        self.dirty = true;
    }

    /// Open the delete-confirmation dialog (the Delete button path).
    ///
    /// No-op when the host list is empty (treated as disabled). The default
    /// focus is on the Cancel button — the standard UX guard against
    /// accidental deletions.
    pub fn open_ssh_delete_dialog(&mut self) {
        if self.ssh_hosts.is_empty() {
            return;
        }
        self.ssh_delete_dialog_open = true;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// Close the delete-confirmation dialog (the Cancel button or Esc path).
    /// Leaves the host unchanged.
    pub fn cancel_ssh_delete_dialog(&mut self) {
        self.ssh_delete_dialog_open = false;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// Confirm "delete" in the delete-confirmation dialog (Confirm button or Enter).
    ///
    /// Deletes the selected host and closes the dialog. Post-deletion selection
    /// clamps to n:
    /// - With `selected_host_index = n` before the delete and `ssh_hosts.len() = L`,
    ///   the new upper bound is `L - 1`; clamp to 0 otherwise.
    /// - When `n` was the last entry, the new selection is `n - 1`.
    /// - When the list becomes empty, reset `selected_host_index = 0` and
    ///   `ssh_field_focus = 0`.
    pub fn confirm_ssh_delete_dialog(&mut self) {
        if self.selected_host_index < self.ssh_hosts.len() {
            self.ssh_hosts.remove(self.selected_host_index);
            // n clamp: when the deleted index was the tail, fall back to n-1.
            if !self.ssh_hosts.is_empty() && self.selected_host_index >= self.ssh_hosts.len() {
                self.selected_host_index = self.ssh_hosts.len() - 1;
            }
            // When the list is empty, return focus to the ListBox.
            if self.ssh_hosts.is_empty() {
                self.selected_host_index = 0;
                self.ssh_field_focus = 0;
            }
            self.dirty = true;
        }
        self.ssh_delete_dialog_open = false;
        self.ssh_delete_dialog_confirm_focused = false;
    }

    /// Toggle focus in the delete-confirmation dialog (Confirm ↔ Cancel)
    /// via Left/Right.
    pub fn toggle_ssh_delete_dialog_focus(&mut self) {
        self.ssh_delete_dialog_confirm_focused = !self.ssh_delete_dialog_confirm_focused;
    }

    /// Start TextInput edit mode for the current `ssh_field_focus` value.
    ///
    /// Returns `true` if edit mode actually started; `false` when the field
    /// is not a TextInput (port / auth_type / ListBox) or no host is selected.
    pub fn begin_ssh_field_edit(&mut self) -> bool {
        let initial = {
            let Some(host) = self.ssh_hosts.get(self.selected_host_index) else {
                return false;
            };
            match self.ssh_field_focus {
                1 => host.name.clone(),
                2 => host.host.clone(),
                4 => host.username.clone(),
                _ => return false,
            }
        };
        self.ssh_field_editing = Some(TextInputState::new(initial));
        true
    }

    /// Commit the in-flight buffer back to the host field and leave edit mode.
    /// Returns `true` when a write-back happened.
    pub fn commit_ssh_field_edit(&mut self) -> bool {
        let Some(state) = self.ssh_field_editing.take() else {
            return false;
        };
        let text = state.buffer;
        match self.ssh_field_focus {
            1 => self.set_ssh_host_name(text),
            2 => self.set_ssh_host_host(text),
            4 => self.set_ssh_host_username(text),
            _ => return false,
        }
        true
    }

    /// Discard the in-flight buffer and leave edit mode.
    /// Returns `true` if edit mode was active.
    pub fn cancel_ssh_field_edit(&mut self) -> bool {
        self.ssh_field_editing.take().is_some()
    }

    /// Insert one character at the cursor inside the in-flight buffer.
    /// No-op when not in edit mode.
    pub fn ssh_field_insert_char(&mut self, ch: char) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.insert_char(ch);
        }
    }

    /// Insert a string at the cursor inside the in-flight buffer (IME Commit path).
    pub fn ssh_field_insert_str(&mut self, s: &str) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.insert_str(s);
        }
    }

    /// Delete the character immediately before the cursor (Backspace).
    pub fn ssh_field_backspace(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.backspace();
        }
    }

    /// Delete the character immediately after the cursor (Delete).
    pub fn ssh_field_delete(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.delete_forward();
        }
    }

    /// Move the in-flight cursor one character left.
    pub fn ssh_field_move_left(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_left();
        }
    }

    /// Move the in-flight cursor one character right.
    pub fn ssh_field_move_right(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_right();
        }
    }

    /// Move the in-flight cursor to the start of the buffer.
    pub fn ssh_field_move_home(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_home();
        }
    }

    /// Move the in-flight cursor to the end of the buffer.
    pub fn ssh_field_move_end(&mut self) {
        if let Some(state) = self.ssh_field_editing.as_mut() {
            state.move_end();
        }
    }
}

/// Update the `[[hosts]]` array in place (Phase 5-11-8 Step 8-2).
///
/// Keeps the existing `ArrayOfTables` and overwrites only the 5 fields
/// managed by `SettingsPanel` (name / host / port / username / auth_type).
/// Unmanaged fields such as `key_path` / `forward_local` / `proxy_jump` /
/// `tags` are left untouched (so user-edited TOML values are not lost).
///
/// Length adjustments:
/// - `ssh_hosts.len() > arr.len()`: append a new Table at the tail (used by
///   Step 8-3 Add).
/// - `ssh_hosts.len() < arr.len()`: remove the trailing Table(s) (used by
///   Step 8-3 Delete).
/// - Equal: in-place updates only.
pub(crate) fn write_ssh_hosts_back(doc: &mut toml_edit::DocumentMut, hosts: &[SshHostEntry]) {
    // Get the existing hosts entry as `ArrayOfTables` (create one if absent).
    let entry = doc
        .entry("hosts")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));

    // If the existing item is not an `ArrayOfTables` (broken by manual
    // editing), rebuild it.
    if !entry.is_array_of_tables() {
        *entry = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }

    let Some(arr) = entry.as_array_of_tables_mut() else {
        return;
    };

    // Overwrite the 5 managed fields per index.
    for (i, host) in hosts.iter().enumerate() {
        if i < arr.len() {
            let t = arr.get_mut(i).expect("length was already checked");
            t.insert("name", toml_edit::value(host.name.as_str()));
            t.insert("host", toml_edit::value(host.host.as_str()));
            t.insert("port", toml_edit::value(host.port as i64));
            t.insert("username", toml_edit::value(host.username.as_str()));
            t.insert("auth_type", toml_edit::value(host.auth_type.as_str()));
        } else {
            // Append a new entry (used by Step 8-3 Add).
            let mut t = toml_edit::Table::new();
            t.insert("name", toml_edit::value(host.name.as_str()));
            t.insert("host", toml_edit::value(host.host.as_str()));
            t.insert("port", toml_edit::value(host.port as i64));
            t.insert("username", toml_edit::value(host.username.as_str()));
            t.insert("auth_type", toml_edit::value(host.auth_type.as_str()));
            arr.push(t);
        }
    }
    // Pop surplus entries from the tail (used by Step 8-3 Delete).
    while arr.len() > hosts.len() {
        arr.remove(arr.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    fn panel_with_one_host() -> SettingsPanel {
        let mut panel = SettingsPanel::new(&Config::default());
        panel.ssh_hosts.push(SshHostEntry {
            name: "myhost".to_string(),
            host: "example.com".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth_type: "password".to_string(),
        });
        panel.selected_host_index = 0;
        panel
    }

    #[test]
    fn ssh_field_edit_begin_commit_lifecycle() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 1; // name

        assert!(panel.begin_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_some());
        let state = panel.ssh_field_editing.as_ref().unwrap();
        assert_eq!(state.buffer, "myhost");

        // Edit a character.
        panel.ssh_field_insert_char('!');
        assert_eq!(panel.ssh_field_editing.as_ref().unwrap().buffer, "myhost!");

        // Commit writes back to the host.
        assert!(panel.commit_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_none());
        assert_eq!(panel.ssh_hosts[0].name, "myhost!");
        assert!(panel.dirty);
    }

    #[test]
    fn ssh_field_edit_cancel_discards_changes() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 2; // host
        panel.begin_ssh_field_edit();
        panel.ssh_field_insert_char('X');

        assert!(panel.cancel_ssh_field_edit());
        assert!(panel.ssh_field_editing.is_none());
        // The host is unchanged.
        assert_eq!(panel.ssh_hosts[0].host, "example.com");
    }

    #[test]
    fn ssh_field_edit_begin_returns_false_for_non_text_fields() {
        let mut panel = panel_with_one_host();
        // port (3) / auth_type (5) / ListBox (0) are not TextInputs, so begin
        // must return false.
        for focus in [0u8, 3, 5, 6, 7] {
            panel.ssh_field_focus = focus;
            assert!(
                !panel.begin_ssh_field_edit(),
                "focus={focus} is not a TextInput, so begin_ssh_field_edit should return false"
            );
            assert!(panel.ssh_field_editing.is_none());
        }
    }

    #[test]
    fn add_ssh_host_appends_with_defaults_and_enters_edit_mode() {
        let mut panel = SettingsPanel::new(&Config::default());
        assert!(panel.ssh_hosts.is_empty());

        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 1);
        let new_host = &panel.ssh_hosts[0];
        assert_eq!(new_host.name, "");
        assert_eq!(new_host.host, "");
        assert_eq!(new_host.port, 22);
        assert_eq!(new_host.username, "");
        assert_eq!(new_host.auth_type, "password");

        assert_eq!(panel.selected_host_index, 0);
        assert_eq!(panel.ssh_field_focus, 1, "focus moves to the name field");
        assert!(
            panel.ssh_field_editing.is_some(),
            "name edit mode should start immediately"
        );
        assert_eq!(
            panel.ssh_field_editing.as_ref().unwrap().buffer,
            "",
            "the name of a new host is initialised to an empty string"
        );
        assert!(panel.dirty);
    }

    #[test]
    fn add_ssh_host_extends_existing_list() {
        let mut panel = panel_with_one_host();
        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(
            panel.selected_host_index, 1,
            "the newly added trailing host is selected"
        );
    }

    #[test]
    fn open_ssh_delete_dialog_noop_when_empty() {
        let mut panel = SettingsPanel::new(&Config::default());
        assert!(panel.ssh_hosts.is_empty());
        panel.open_ssh_delete_dialog();
        assert!(
            !panel.ssh_delete_dialog_open,
            "no dialog opens when the list is empty"
        );
    }

    #[test]
    fn open_ssh_delete_dialog_defaults_to_cancel_focus() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        assert!(panel.ssh_delete_dialog_open);
        assert!(
            !panel.ssh_delete_dialog_confirm_focused,
            "accidental-deletion guard: Cancel is the default focused button"
        );
    }

    #[test]
    fn cancel_ssh_delete_dialog_clears_state_and_keeps_host() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        panel.ssh_delete_dialog_confirm_focused = true;
        panel.cancel_ssh_delete_dialog();

        assert!(!panel.ssh_delete_dialog_open);
        assert!(!panel.ssh_delete_dialog_confirm_focused);
        assert_eq!(panel.ssh_hosts.len(), 1, "nothing is deleted");
    }

    #[test]
    fn confirm_ssh_delete_dialog_removes_at_end_clamps_to_prev() {
        let mut panel = panel_with_one_host();
        // Set up 2 hosts and delete the tail.
        panel.add_ssh_host();
        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(panel.selected_host_index, 1);

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert_eq!(panel.ssh_hosts.len(), 1);
        assert_eq!(
            panel.selected_host_index, 0,
            "deleting the tail clamps the index to n-1=0"
        );
        assert!(!panel.ssh_delete_dialog_open);
        assert!(panel.dirty);
    }

    #[test]
    fn confirm_ssh_delete_dialog_middle_index_keeps_position() {
        let mut panel = panel_with_one_host();
        panel.add_ssh_host();
        panel.add_ssh_host(); // 3 hosts in total
        panel.selected_host_index = 1; // select the middle one

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert_eq!(panel.ssh_hosts.len(), 2);
        assert_eq!(
            panel.selected_host_index, 1,
            "deleting the middle entry shifts the tail and leaves index=1 unchanged"
        );
    }

    #[test]
    fn confirm_ssh_delete_dialog_empty_after_resets_focus() {
        let mut panel = panel_with_one_host();
        panel.ssh_field_focus = 3; // any non-zero value

        panel.open_ssh_delete_dialog();
        panel.confirm_ssh_delete_dialog();

        assert!(panel.ssh_hosts.is_empty());
        assert_eq!(panel.selected_host_index, 0);
        assert_eq!(
            panel.ssh_field_focus, 0,
            "the focus returns to the ListBox once the list is empty"
        );
    }

    #[test]
    fn toggle_ssh_delete_dialog_focus_alternates() {
        let mut panel = panel_with_one_host();
        panel.open_ssh_delete_dialog();
        assert!(!panel.ssh_delete_dialog_confirm_focused);

        panel.toggle_ssh_delete_dialog_focus();
        assert!(panel.ssh_delete_dialog_confirm_focused);

        panel.toggle_ssh_delete_dialog_focus();
        assert!(!panel.ssh_delete_dialog_confirm_focused);
    }
}
