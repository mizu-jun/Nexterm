//! Serializing the settings panel's in-memory state back to the
//! on-disk `config.toml` (`toml_edit`, preserving comments/formatting).
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::{SettingsPanel, write_ssh_hosts_back};
use anyhow::Result;
use nexterm_config::toml_path;

impl SettingsPanel {
    /// Save the current settings to `nexterm.toml`.
    pub fn save_to_toml(&self) -> Result<()> {
        let path = toml_path();

        // Read the existing TOML (start from an empty string if missing).
        let existing = if path.exists() {
            std::fs::read_to_string(&path)?
        } else {
            String::new()
        };

        let updated = self.apply_to_toml_string(&existing);

        // Create the parent directory if necessary.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&path, updated)?;
        Ok(())
    }

    /// Pure document-transform half of [`Self::save_to_toml`]: parses
    /// `existing` (an empty string is treated as an empty document), writes
    /// every managed field back into it, and returns the rendered TOML text.
    /// No filesystem access — kept separate so tests can exercise the
    /// write-back logic without touching the real `nexterm.toml`.
    pub(crate) fn apply_to_toml_string(&self, existing: &str) -> String {
        let mut doc: toml_edit::DocumentMut = existing.parse().unwrap_or_default();

        // [font].family
        if !self.font_family.is_empty() {
            doc["font"]["family"] = toml_edit::value(self.font_family.as_str());
        }

        // [font].size
        doc["font"]["size"] = toml_edit::value(self.font_size as f64);

        // [colors].scheme
        doc["colors"]["scheme"] = toml_edit::value(self.scheme_name());

        // [window].background_opacity
        doc["window"]["background_opacity"] = toml_edit::value(self.opacity as f64);

        // [window].padding_x / padding_y (Phase 5-11-6 #6).
        doc["window"]["padding_x"] = toml_edit::value(self.padding_x as i64);
        doc["window"]["padding_y"] = toml_edit::value(self.padding_y as i64);

        // [gpu].present_mode (Phase 5-11-6 #6).
        doc["gpu"]["present_mode"] = toml_edit::value(self.present_mode_toml_key());

        // cursor_style (Phase 5-11-6 #6).
        doc["cursor_style"] = toml_edit::value(self.cursor_style_toml_key());

        // language
        doc["language"] = toml_edit::value(self.language_code());

        // auto_check_update
        doc["auto_check_update"] = toml_edit::value(self.auto_check_update);

        // [blocks].enabled / border_width_px / show_exit_code_badge (Phase 2c-G follow-up).
        doc["blocks"]["enabled"] = toml_edit::value(self.blocks_enabled);
        doc["blocks"]["border_width_px"] = toml_edit::value(self.blocks_border_width_px as i64);
        doc["blocks"]["show_exit_code_badge"] = toml_edit::value(self.blocks_show_exit_code_badge);

        // [cursor].blink_enabled (Phase B4).
        doc["cursor"]["blink_enabled"] = toml_edit::value(self.cursor_blink_enabled);

        // scrollback_lines (Phase B4).
        doc["scrollback_lines"] = toml_edit::value(self.scrollback_lines as i64);

        // [tab_bar].show_tab_number / show_new_tab_button (Phase B4).
        doc["tab_bar"]["show_tab_number"] = toml_edit::value(self.tab_show_tab_number);
        doc["tab_bar"]["show_new_tab_button"] = toml_edit::value(self.tab_show_new_tab_button);

        // [animations].enabled / intensity (Phase B4).
        doc["animations"]["enabled"] = toml_edit::value(self.animations_enabled_toml_value());
        doc["animations"]["intensity"] = toml_edit::value(self.animations_intensity_toml_key());

        // [window].decorations / close_action, [gpu].fps_limit (Phase B4-P2).
        doc["window"]["decorations"] = toml_edit::value(self.window_decorations_toml_key());
        doc["window"]["close_action"] = toml_edit::value(self.window_close_action_toml_key());
        doc["gpu"]["fps_limit"] = toml_edit::value(self.fps_limit as i64);

        // [window].in_app_blur_enabled / in_app_blur_strength (P2b).
        doc["window"]["in_app_blur_enabled"] = toml_edit::value(self.in_app_blur_enabled);
        doc["window"]["in_app_blur_strength"] = toml_edit::value(self.in_app_blur_strength as f64);

        // [window].backdrop (P2c).
        doc["window"]["backdrop"] = toml_edit::value(self.window_backdrop_toml_key());

        // colors_follow_system (Phase B4-P2).
        doc["colors_follow_system"] = toml_edit::value(self.colors_follow_system);

        // [font].ligatures / font_fallbacks (Phase B4-P2).
        doc["font"]["ligatures"] = toml_edit::value(self.font_ligatures);
        let font_fallbacks: toml_edit::Array = self.font_fallbacks_list().into_iter().collect();
        doc["font"]["font_fallbacks"] = toml_edit::value(font_fallbacks);

        // leader_key (Phase B4-P2).
        if !self.leader_key.is_empty() {
            doc["leader_key"] = toml_edit::value(self.leader_key.as_str());
        }

        // [shell].program / args (Phase B4). `shell_args` is edited as a
        // single space-separated string and split back into a TOML array on
        // save (mirrors how it is joined for editing in `Self::new`).
        if !self.shell_program.is_empty() {
            doc["shell"]["program"] = toml_edit::value(self.shell_program.as_str());
        }
        let shell_args: toml_edit::Array = self
            .shell_args
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        doc["shell"]["args"] = toml_edit::value(shell_args);

        // active_profile (Phase B4): `None` removes the key so the config
        // falls back to no active profile; `Some(name)` writes it verbatim.
        match self.active_profile_name() {
            Some(name) => doc["active_profile"] = toml_edit::value(name),
            None => {
                doc.as_table_mut().remove("active_profile");
            }
        }

        // [security] consent policies + byte caps.
        doc["security"]["external_url"] =
            toml_edit::value(Self::consent_toml_key(self.sec_external_url));
        doc["security"]["osc52_clipboard"] =
            toml_edit::value(Self::consent_toml_key(self.sec_osc52_clipboard));
        doc["security"]["osc_notification"] =
            toml_edit::value(Self::consent_toml_key(self.sec_osc_notification));
        doc["security"]["plugin_read"] =
            toml_edit::value(Self::consent_toml_key(self.sec_plugin_read));
        doc["security"]["osc52_max_bytes"] = toml_edit::value(self.sec_osc52_max_bytes as i64);
        doc["security"]["notification_max_bytes"] =
            toml_edit::value(self.sec_notification_max_bytes as i64);
        doc["security"]["plugin_read_max_bytes"] =
            toml_edit::value(self.sec_plugin_read_max_bytes as i64);

        // Phase 5-11-8 Step 8-2: in-place write-back to `[[hosts]]`.
        //
        // When the existing `ArrayOfTables` is present we update only the
        // managed fields per index, preserving unmanaged fields such as
        // `key_path` / `forward_local` / `proxy_jump`. When the array length
        // diverges from `self.ssh_hosts` (after Step 8-3 Add/Delete) we
        // adjust the tail diff only.
        write_ssh_hosts_back(&mut doc, &self.ssh_hosts);

        doc.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn save_writes_blocks_fields() {
        // Regression guard for the Blocks category (already wired before
        // Phase B4, but pinned here alongside the other new fields).
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.blocks_enabled = false;
        panel.blocks_border_width_px = 5;
        panel.blocks_show_exit_code_badge = false;
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("enabled = false"));
        assert!(toml_str.contains("border_width_px = 5"));
        assert!(toml_str.contains("show_exit_code_badge = false"));
    }

    #[test]
    fn in_app_blur_settings_round_trip_through_save() {
        // P2b: end-to-end check that the toggle, the slider setter and the
        // TOML write-back all agree, mirroring `save_writes_blocks_fields`.
        let mut sp = SettingsPanel::new(&Config::default());
        assert!(!sp.in_app_blur_enabled);
        sp.toggle_in_app_blur();
        assert!(sp.in_app_blur_enabled);
        sp.set_in_app_blur_strength_value(0.3);
        assert!((sp.in_app_blur_strength - 0.3).abs() < 0.05);
        let toml_str = sp.apply_to_toml_string("");
        assert!(toml_str.contains("in_app_blur_enabled = true"));
    }
}
