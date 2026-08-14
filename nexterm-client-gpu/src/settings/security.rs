//! Security category: consent-policy (external URL / OSC 52 clipboard /
//! OSC 9-777 notification / plugin read) cycling and byte-cap editing.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::{SettingsPanel, TextInputState};
use nexterm_i18n::fl;

impl SettingsPanel {
    /// Total number of fields in the Security category.
    pub const SECURITY_FIELD_COUNT: u16 = 7;

    /// Human-readable label for a consent policy value.
    pub fn consent_label(p: nexterm_config::ConsentPolicy) -> &'static str {
        use nexterm_config::ConsentPolicy::*;
        match p {
            Allow => "allow",
            Deny => "deny",
            Prompt => "prompt",
        }
    }

    /// TOML key for a consent policy value (serde `rename_all = "lowercase"`).
    pub fn consent_toml_key(p: nexterm_config::ConsentPolicy) -> &'static str {
        Self::consent_label(p)
    }

    /// Translated UI display label for a consent policy value. Distinct from
    /// [`Self::consent_label`], which must stay a raw ASCII string because
    /// [`Self::consent_toml_key`] delegates to it for TOML write-back.
    pub fn consent_display_label(p: nexterm_config::ConsentPolicy) -> String {
        use nexterm_config::ConsentPolicy::*;
        match p {
            Allow => fl!("settings-consent-allow"),
            Deny => fl!("settings-consent-deny"),
            Prompt => fl!("settings-consent-prompt"),
        }
    }

    /// Cycle a consent policy allow -> deny -> prompt (forward) or the reverse.
    fn cycle_consent(
        p: nexterm_config::ConsentPolicy,
        forward: bool,
    ) -> nexterm_config::ConsentPolicy {
        use nexterm_config::ConsentPolicy::*;
        if forward {
            match p {
                Allow => Deny,
                Deny => Prompt,
                Prompt => Allow,
            }
        } else {
            match p {
                Allow => Prompt,
                Deny => Allow,
                Prompt => Deny,
            }
        }
    }

    /// The consent policy at field index 0..=3 (`None` for numeric fields).
    pub fn security_policy_at(&self, idx: u16) -> Option<nexterm_config::ConsentPolicy> {
        match idx {
            0 => Some(self.sec_external_url),
            1 => Some(self.sec_osc52_clipboard),
            2 => Some(self.sec_osc_notification),
            3 => Some(self.sec_plugin_read),
            _ => None,
        }
    }

    /// The byte-cap value at field index 4..=6 (`None` for policy fields).
    pub fn security_bytes_at(&self, idx: u16) -> Option<usize> {
        match idx {
            4 => Some(self.sec_osc52_max_bytes),
            5 => Some(self.sec_notification_max_bytes),
            6 => Some(self.sec_plugin_read_max_bytes),
            _ => None,
        }
    }

    /// Static row label for a Security field index.
    pub fn security_field_label(idx: u16) -> String {
        match idx {
            0 => fl!("settings-security-field-external-url"),
            1 => fl!("settings-security-field-osc52-clipboard"),
            2 => fl!("settings-security-field-notification"),
            3 => fl!("settings-security-field-plugin-read"),
            4 => fl!("settings-security-field-osc52-max-bytes"),
            5 => fl!("settings-security-field-notification-max-bytes"),
            6 => fl!("settings-security-field-plugin-read-max-bytes"),
            _ => String::new(),
        }
    }

    /// Cycle the focused policy field forward (Right arrow). No-op on numerics.
    pub fn security_field_increase(&mut self) {
        self.cycle_security_policy(true);
    }

    /// Cycle the focused policy field backward (Left arrow). No-op on numerics.
    pub fn security_field_decrease(&mut self) {
        self.cycle_security_policy(false);
    }

    fn cycle_security_policy(&mut self, forward: bool) {
        let field = match self.focused_widget_index {
            0 => &mut self.sec_external_url,
            1 => &mut self.sec_osc52_clipboard,
            2 => &mut self.sec_osc_notification,
            3 => &mut self.sec_plugin_read,
            _ => return, // numeric fields are edited via begin_security_edit
        };
        *field = Self::cycle_consent(*field, forward);
        self.dirty = true;
    }

    /// Begin editing the focused byte-cap field (focus 4..=6). No-op otherwise.
    pub fn begin_security_edit(&mut self) {
        if let Some(v) = self.security_bytes_at(self.focused_widget_index) {
            self.security_field_editing = Some(TextInputState::new(v.to_string()));
        }
    }

    /// Commit the in-flight byte-cap edit, parsing decimal digits. Empty or
    /// invalid input is discarded (the previous value is kept).
    pub fn commit_security_edit(&mut self) {
        if let Some(state) = self.security_field_editing.take()
            && let Ok(parsed) = state.buffer.trim().parse::<usize>()
        {
            match self.focused_widget_index {
                4 => self.sec_osc52_max_bytes = parsed,
                5 => self.sec_notification_max_bytes = parsed,
                6 => self.sec_plugin_read_max_bytes = parsed,
                _ => {}
            }
            self.dirty = true;
        }
    }

    /// Cancel the in-flight byte-cap edit without applying it.
    pub fn cancel_security_edit(&mut self) {
        self.security_field_editing = None;
    }

    /// Insert a character into the in-flight byte-cap edit. Only ASCII digits
    /// are accepted (byte caps are decimal); everything else is ignored.
    pub fn security_field_insert_char(&mut self, ch: char) {
        if ch.is_ascii_digit()
            && let Some(state) = self.security_field_editing.as_mut()
        {
            state.insert_char(ch);
        }
    }

    /// Delete the digit before the cursor in the in-flight byte-cap edit.
    pub fn security_field_backspace(&mut self) {
        if let Some(state) = self.security_field_editing.as_mut() {
            state.backspace();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn new_reads_config_security_policies_and_caps() {
        let mut config = Config::default();
        config.security.external_url = nexterm_config::ConsentPolicy::Allow;
        config.security.plugin_read = nexterm_config::ConsentPolicy::Deny;
        config.security.plugin_read_max_bytes = 4096;

        let panel = SettingsPanel::new(&config);
        assert_eq!(panel.sec_external_url, nexterm_config::ConsentPolicy::Allow);
        assert_eq!(panel.sec_plugin_read, nexterm_config::ConsentPolicy::Deny);
        assert_eq!(panel.sec_plugin_read_max_bytes, 4096);
    }

    #[test]
    fn consent_cycle_wraps_forward_and_back() {
        use nexterm_config::ConsentPolicy::*;
        // forward: allow -> deny -> prompt -> allow
        assert_eq!(SettingsPanel::cycle_consent(Allow, true), Deny);
        assert_eq!(SettingsPanel::cycle_consent(Deny, true), Prompt);
        assert_eq!(SettingsPanel::cycle_consent(Prompt, true), Allow);
        // backward: allow -> prompt -> deny -> allow
        assert_eq!(SettingsPanel::cycle_consent(Allow, false), Prompt);
        assert_eq!(SettingsPanel::cycle_consent(Prompt, false), Deny);
        assert_eq!(SettingsPanel::cycle_consent(Deny, false), Allow);
    }

    #[test]
    fn consent_label_and_toml_key_match_serde() {
        use nexterm_config::ConsentPolicy::*;
        assert_eq!(SettingsPanel::consent_toml_key(Allow), "allow");
        assert_eq!(SettingsPanel::consent_toml_key(Deny), "deny");
        assert_eq!(SettingsPanel::consent_toml_key(Prompt), "prompt");
    }

    #[test]
    fn security_field_focus_cycles_only_policy_fields() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        // Focus the plugin_read policy (index 3) and cycle it.
        panel.focused_widget_index = 3;
        let before = panel.sec_plugin_read;
        panel.security_field_increase();
        assert_ne!(panel.sec_plugin_read, before, "policy fields cycle");
        assert!(panel.dirty);

        // A numeric field (index 4) does not cycle as a policy.
        panel.dirty = false;
        panel.focused_widget_index = 4;
        let bytes_before = panel.sec_osc52_max_bytes;
        panel.security_field_increase();
        assert_eq!(panel.sec_osc52_max_bytes, bytes_before);
        assert!(!panel.dirty, "numeric fields ignore policy cycling");
    }

    #[test]
    fn security_byte_cap_edit_parses_and_commits() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.focused_widget_index = 6; // plugin_read_max_bytes
        panel.begin_security_edit();
        assert!(panel.security_field_editing.is_some());
        // Replace the buffer with a fresh number, digits only.
        panel.security_field_editing = Some(TextInputState::new(String::new()));
        for ch in "2048x9".chars() {
            panel.security_field_insert_char(ch); // 'x' is rejected
        }
        panel.commit_security_edit();
        assert_eq!(panel.sec_plugin_read_max_bytes, 20489);
        assert!(panel.security_field_editing.is_none());
    }

    #[test]
    fn security_byte_cap_edit_cancel_keeps_previous() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        let original = panel.sec_notification_max_bytes;
        panel.focused_widget_index = 5;
        panel.begin_security_edit();
        panel.security_field_insert_char('9');
        panel.cancel_security_edit();
        assert_eq!(panel.sec_notification_max_bytes, original);
        assert!(panel.security_field_editing.is_none());
    }
}
