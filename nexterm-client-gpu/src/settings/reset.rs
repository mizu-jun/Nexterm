//! P2-A (WT-like UX): per-category "reset to defaults".
//!
//! Resets the *panel state* of the current category to the values a
//! pristine `Config::default()` would produce; the change lands on disk
//! only through the existing save path, so Cancel still reverts it.
//!
//! List-based categories (SSH hosts / Keybindings / Profiles) are
//! deliberately not resettable: "defaults" there means deleting user
//! data, which needs a stronger confirmation flow than a footer link.

use super::{SettingsCategory, SettingsPanel};

impl SettingsPanel {
    /// Whether the current category supports the footer "reset to
    /// defaults" link.
    pub fn category_resettable(&self) -> bool {
        !matches!(
            self.category,
            SettingsCategory::Ssh | SettingsCategory::Keybindings | SettingsCategory::Profiles
        )
    }

    /// Reset the current category's fields to their `Config::default()`
    /// values and mark the panel dirty. Returns `false` (and changes
    /// nothing) for the list-based categories.
    pub fn reset_category_to_defaults(&mut self) -> bool {
        if !self.category_resettable() {
            return false;
        }
        let def = SettingsPanel::default();
        match self.category {
            SettingsCategory::Startup => {
                self.language_index = def.language_index;
                self.auto_check_update = def.auto_check_update;
                self.shell_program = def.shell_program;
                self.shell_args = def.shell_args;
                self.shell_field_editing = None;
            }
            SettingsCategory::Font => {
                self.font_family = def.font_family;
                self.font_size = def.font_size;
                self.font_ligatures = def.font_ligatures;
                self.font_fallbacks_text = def.font_fallbacks_text;
                self.font_family_editing = false;
                self.font_fallbacks_editing = None;
            }
            SettingsCategory::Theme => {
                self.scheme_index = def.scheme_index;
                self.colors_follow_system = def.colors_follow_system;
                self.theme_hover_preview = None;
            }
            SettingsCategory::Window => {
                self.opacity = def.opacity;
                self.cursor_style = def.cursor_style;
                self.padding_x = def.padding_x;
                self.padding_y = def.padding_y;
                self.present_mode = def.present_mode;
                self.cursor_blink_enabled = def.cursor_blink_enabled;
                self.scrollback_lines = def.scrollback_lines;
                self.tab_show_tab_number = def.tab_show_tab_number;
                self.tab_show_new_tab_button = def.tab_show_new_tab_button;
                self.animations_enabled = def.animations_enabled;
                self.animations_intensity = def.animations_intensity;
                self.window_decorations = def.window_decorations;
                self.window_close_action = def.window_close_action;
                self.fps_limit = def.fps_limit;
            }
            SettingsCategory::Blocks => {
                self.blocks_enabled = def.blocks_enabled;
                self.blocks_border_width_px = def.blocks_border_width_px;
                self.blocks_show_exit_code_badge = def.blocks_show_exit_code_badge;
            }
            SettingsCategory::Security => {
                self.sec_external_url = def.sec_external_url;
                self.sec_osc52_clipboard = def.sec_osc52_clipboard;
                self.sec_osc_notification = def.sec_osc_notification;
                self.sec_plugin_read = def.sec_plugin_read;
                self.sec_osc52_max_bytes = def.sec_osc52_max_bytes;
                self.sec_notification_max_bytes = def.sec_notification_max_bytes;
                self.sec_plugin_read_max_bytes = def.sec_plugin_read_max_bytes;
                self.security_field_editing = None;
            }
            SettingsCategory::Ssh | SettingsCategory::Keybindings | SettingsCategory::Profiles => {
                unreachable!("guarded by category_resettable above")
            }
        }
        self.dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a panel from a config that deviates from the defaults in every
    /// value category, so each reset assertion is meaningful.
    fn modified_panel() -> SettingsPanel {
        let mut sp = SettingsPanel::default();
        sp.opacity = 0.5;
        sp.padding_x = 17;
        sp.scrollback_lines = 42_000;
        sp.font_size = 33.0;
        sp.font_family = "Comic Mono".to_string();
        sp.blocks_border_width_px = 7;
        sp.sec_osc52_max_bytes = 1;
        sp.auto_check_update = !sp.auto_check_update;
        sp.dirty = false;
        sp
    }

    #[test]
    fn window_reset_restores_defaults_and_marks_dirty() {
        let mut sp = modified_panel();
        sp.category = SettingsCategory::Window;
        assert!(sp.reset_category_to_defaults());
        let def = SettingsPanel::default();
        assert_eq!(sp.opacity, def.opacity);
        assert_eq!(sp.padding_x, def.padding_x);
        assert_eq!(sp.scrollback_lines, def.scrollback_lines);
        assert!(sp.dirty);
        // Other categories stay untouched.
        assert_eq!(sp.font_size, 33.0);
        assert_eq!(sp.blocks_border_width_px, 7);
    }

    #[test]
    fn font_reset_only_touches_font_fields() {
        let mut sp = modified_panel();
        sp.category = SettingsCategory::Font;
        assert!(sp.reset_category_to_defaults());
        let def = SettingsPanel::default();
        assert_eq!(sp.font_size, def.font_size);
        assert_eq!(sp.font_family, def.font_family);
        assert_eq!(sp.opacity, 0.5);
        assert_eq!(sp.sec_osc52_max_bytes, 1);
    }

    #[test]
    fn list_categories_are_not_resettable() {
        for cat in [
            SettingsCategory::Ssh,
            SettingsCategory::Keybindings,
            SettingsCategory::Profiles,
        ] {
            let mut sp = modified_panel();
            sp.category = cat;
            assert!(!sp.category_resettable());
            assert!(!sp.reset_category_to_defaults());
            assert!(!sp.dirty, "a refused reset must not mark the panel dirty");
        }
    }

    #[test]
    fn security_reset_restores_policies_and_caps() {
        let mut sp = modified_panel();
        sp.category = SettingsCategory::Security;
        assert!(sp.reset_category_to_defaults());
        let def = SettingsPanel::default();
        assert_eq!(sp.sec_osc52_max_bytes, def.sec_osc52_max_bytes);
        assert_eq!(sp.sec_external_url, def.sec_external_url);
    }
}
