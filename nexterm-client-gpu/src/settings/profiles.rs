//! Session-profile list entries and the active-profile selector.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::SettingsPanel;

/// Profile entry (editable inside the settings panel).
#[derive(Debug, Clone)]
pub struct ProfileEntry {
    pub name: String,
    pub icon: String,
    #[allow(dead_code)]
    pub shell_program: String,
    #[allow(dead_code)]
    pub working_dir: String,
}

impl Default for ProfileEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            icon: ">".to_string(),
            shell_program: String::new(),
            working_dir: String::new(),
        }
    }
}

impl SettingsPanel {
    /// Cycle to the next active-profile candidate (wraps).
    pub fn next_active_profile(&mut self) {
        let len = self.profiles.len();
        if len == 0 {
            return;
        }
        self.active_profile_index = (self.active_profile_index + 1) % (len + 1);
        self.dirty = true;
    }

    /// Cycle to the previous active-profile candidate (wraps).
    pub fn prev_active_profile(&mut self) {
        let len = self.profiles.len();
        if len == 0 {
            return;
        }
        self.active_profile_index = (self.active_profile_index + len) % (len + 1);
        self.dirty = true;
    }

    /// The active profile's name, or `None` when `active_profile_index == 0`
    /// or it is out of range (a stale index after external profile removal).
    pub fn active_profile_name(&self) -> Option<&str> {
        if self.active_profile_index == 0 {
            return None;
        }
        self.profiles
            .get(self.active_profile_index - 1)
            .map(|p| p.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn active_profile_cycle_wraps_through_none_and_each_profile() {
        let mut config = Config::default();
        config.profiles.push(nexterm_config::Profile {
            name: "work".to_string(),
            ..Default::default()
        });
        config.profiles.push(nexterm_config::Profile {
            name: "personal".to_string(),
            ..Default::default()
        });
        let mut panel = SettingsPanel::new(&config);
        assert_eq!(panel.active_profile_name(), None);

        panel.next_active_profile();
        assert_eq!(panel.active_profile_name(), Some("work"));
        panel.next_active_profile();
        assert_eq!(panel.active_profile_name(), Some("personal"));
        panel.next_active_profile();
        assert_eq!(
            panel.active_profile_name(),
            None,
            "wraps back to no active profile"
        );

        panel.prev_active_profile();
        assert_eq!(panel.active_profile_name(), Some("personal"));
    }

    #[test]
    fn active_profile_cycle_is_a_no_op_when_no_profiles_exist() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.next_active_profile();
        assert_eq!(panel.active_profile_index, 0);
        assert!(!panel.dirty);
    }

    #[test]
    fn new_reads_active_profile_index_from_config() {
        let mut config = Config::default();
        config.profiles.push(nexterm_config::Profile {
            name: "work".to_string(),
            ..Default::default()
        });
        config.active_profile = Some("work".to_string());
        let panel = SettingsPanel::new(&config);
        assert_eq!(panel.active_profile_index, 1);
        assert_eq!(panel.active_profile_name(), Some("work"));
    }

    #[test]
    fn save_writes_active_profile_when_set() {
        let mut config = Config::default();
        config.profiles.push(nexterm_config::Profile {
            name: "work".to_string(),
            ..Default::default()
        });
        let mut panel = SettingsPanel::new(&config);
        panel.next_active_profile();
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("active_profile = \"work\""));
    }

    #[test]
    fn save_removes_active_profile_when_none() {
        let config = Config::default();
        let panel = SettingsPanel::new(&config);
        let existing = "active_profile = \"stale\"\n";
        let toml_str = panel.apply_to_toml_string(existing);
        assert!(
            !toml_str.contains("active_profile"),
            "an unset active profile must remove the stale key: {toml_str}"
        );
    }
}
