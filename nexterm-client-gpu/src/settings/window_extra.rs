//! Window category (part 2): the value dispatchers for the focused field
//! (`window_field_increase` / `*_decrease`), cursor blink, scrollback length,
//! tab-bar toggles, and animation settings. Split out of `window.rs` to stay
//! under the 800-line-per-file limit. Focus movement itself is not here — it is
//! derived from the descriptors in `renderer/overlay/widgets/navigation.rs`.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::SettingsPanel;
use nexterm_i18n::fl;

impl SettingsPanel {
    /// Increment the focused field's value (Right arrow, or the Up arrow's
    /// fallback inside the Window category).
    pub fn window_field_increase(&mut self) {
        match self.focused_widget_index {
            0 => self.increase_opacity(),
            1 => self.next_cursor_style(),
            2 => self.increase_padding_x(),
            3 => self.increase_padding_y(),
            4 => self.next_present_mode(),
            5 => self.toggle_cursor_blink(),
            6 => self.increase_scrollback_lines(),
            7 => self.toggle_show_tab_number(),
            8 => self.toggle_show_new_tab_button(),
            9 => self.toggle_animations_enabled(),
            10 => self.next_animations_intensity(),
            11 => self.next_window_decorations(),
            12 => self.next_window_close_action(),
            13 => self.increase_fps_limit(),
            14 => self.toggle_in_app_blur(),
            15 => self.increase_in_app_blur_strength(),
            _ => {}
        }
    }

    /// Decrement the focused field's value.
    pub fn window_field_decrease(&mut self) {
        match self.focused_widget_index {
            0 => self.decrease_opacity(),
            1 => self.prev_cursor_style(),
            2 => self.decrease_padding_x(),
            3 => self.decrease_padding_y(),
            4 => self.prev_present_mode(),
            5 => self.toggle_cursor_blink(),
            6 => self.decrease_scrollback_lines(),
            7 => self.toggle_show_tab_number(),
            8 => self.toggle_show_new_tab_button(),
            9 => self.toggle_animations_enabled(),
            10 => self.prev_animations_intensity(),
            11 => self.prev_window_decorations(),
            12 => self.prev_window_close_action(),
            13 => self.decrease_fps_limit(),
            14 => self.toggle_in_app_blur(),
            15 => self.decrease_in_app_blur_strength(),
            _ => {}
        }
    }

    /// Toggle `[cursor].blink_enabled`. Left/Right both toggle (it is a
    /// boolean, not a range), matching the existing `toggle_auto_check_update`
    /// convention.
    pub fn toggle_cursor_blink(&mut self) {
        self.cursor_blink_enabled = !self.cursor_blink_enabled;
        self.dirty = true;
    }

    /// Toggle `[window].in_app_blur_enabled` (P2b). Left/Right both toggle,
    /// mirroring `toggle_cursor_blink`.
    pub fn toggle_in_app_blur(&mut self) {
        self.in_app_blur_enabled = !self.in_app_blur_enabled;
        self.dirty = true;
    }

    /// Minimum allowed `scrollback_lines` (UI-enforced; the config schema
    /// itself accepts any `usize`).
    pub const SCROLLBACK_MIN: usize = 100;

    /// Maximum allowed `scrollback_lines`.
    pub const SCROLLBACK_MAX: usize = 1_000_000;

    /// Step size for the ←/→ adjustment.
    pub const SCROLLBACK_STEP: usize = 1_000;

    pub fn increase_scrollback_lines(&mut self) {
        self.scrollback_lines = (self.scrollback_lines + Self::SCROLLBACK_STEP)
            .clamp(Self::SCROLLBACK_MIN, Self::SCROLLBACK_MAX);
        self.dirty = true;
    }

    pub fn decrease_scrollback_lines(&mut self) {
        self.scrollback_lines = self
            .scrollback_lines
            .saturating_sub(Self::SCROLLBACK_STEP)
            .max(Self::SCROLLBACK_MIN);
        self.dirty = true;
    }

    /// Toggle `[tab_bar].show_tab_number`.
    pub fn toggle_show_tab_number(&mut self) {
        self.tab_show_tab_number = !self.tab_show_tab_number;
        self.dirty = true;
    }

    /// Toggle `[tab_bar].show_new_tab_button`.
    pub fn toggle_show_new_tab_button(&mut self) {
        self.tab_show_new_tab_button = !self.tab_show_new_tab_button;
        self.dirty = true;
    }

    /// Toggle `[animations].enabled`.
    pub fn toggle_animations_enabled(&mut self) {
        self.animations_enabled = !self.animations_enabled;
        self.dirty = true;
    }

    /// Cycle `[animations].intensity` forward: off -> subtle -> normal -> energetic -> off.
    pub fn next_animations_intensity(&mut self) {
        use nexterm_config::AnimationIntensity::*;
        self.animations_intensity = match self.animations_intensity {
            Off => Subtle,
            Subtle => Normal,
            Normal => Energetic,
            Energetic => Off,
        };
        self.dirty = true;
    }

    /// Cycle `[animations].intensity` backward.
    pub fn prev_animations_intensity(&mut self) {
        use nexterm_config::AnimationIntensity::*;
        self.animations_intensity = match self.animations_intensity {
            Off => Energetic,
            Subtle => Off,
            Normal => Subtle,
            Energetic => Normal,
        };
        self.dirty = true;
    }

    /// UI display label for the current animation intensity.
    pub fn animations_intensity_label(&self) -> String {
        use nexterm_config::AnimationIntensity::*;
        match self.animations_intensity {
            Off => fl!("settings-value-animation-off"),
            Subtle => fl!("settings-value-animation-subtle"),
            Normal => fl!("settings-value-animation-normal"),
            Energetic => fl!("settings-value-animation-energetic"),
        }
    }

    /// Lowercase TOML key for write-back (matches `serde`'s `rename_all = "lowercase"`).
    pub fn animations_intensity_toml_key(&self) -> &'static str {
        use nexterm_config::AnimationIntensity::*;
        match self.animations_intensity {
            Off => "off",
            Subtle => "subtle",
            Normal => "normal",
            Energetic => "energetic",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn toggle_cursor_blink_flips_and_marks_dirty() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        let initial = panel.cursor_blink_enabled;
        panel.toggle_cursor_blink();
        assert_eq!(panel.cursor_blink_enabled, !initial);
        assert!(panel.dirty);
    }

    #[test]
    fn toggle_in_app_blur_flips_and_marks_dirty() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert!(!panel.in_app_blur_enabled, "default is false (Task 1)");
        panel.toggle_in_app_blur();
        assert!(panel.in_app_blur_enabled);
        assert!(panel.dirty);
    }

    #[test]
    fn scrollback_lines_step_and_clamp() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.scrollback_lines = 50_000;
        panel.increase_scrollback_lines();
        assert_eq!(panel.scrollback_lines, 51_000);
        panel.decrease_scrollback_lines();
        assert_eq!(panel.scrollback_lines, 50_000);

        panel.scrollback_lines = SettingsPanel::SCROLLBACK_MAX;
        panel.increase_scrollback_lines();
        assert_eq!(
            panel.scrollback_lines,
            SettingsPanel::SCROLLBACK_MAX,
            "must not exceed the maximum"
        );

        panel.scrollback_lines = SettingsPanel::SCROLLBACK_MIN;
        panel.decrease_scrollback_lines();
        assert_eq!(
            panel.scrollback_lines,
            SettingsPanel::SCROLLBACK_MIN,
            "must not fall below the minimum"
        );
    }

    #[test]
    fn tab_bar_toggles_flip_and_mark_dirty() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        let initial_number = panel.tab_show_tab_number;
        panel.toggle_show_tab_number();
        assert_eq!(panel.tab_show_tab_number, !initial_number);
        assert!(panel.dirty);

        let initial_button = panel.tab_show_new_tab_button;
        panel.toggle_show_new_tab_button();
        assert_eq!(panel.tab_show_new_tab_button, !initial_button);
    }

    #[test]
    fn animations_enabled_toggle() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        let initial = panel.animations_enabled;
        panel.toggle_animations_enabled();
        assert_eq!(panel.animations_enabled, !initial);
        assert!(panel.dirty);
    }

    #[test]
    fn animations_intensity_cycles_forward_and_back() {
        use nexterm_config::AnimationIntensity::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.animations_intensity = Off;

        panel.next_animations_intensity();
        assert_eq!(panel.animations_intensity, Subtle);
        panel.next_animations_intensity();
        assert_eq!(panel.animations_intensity, Normal);
        panel.next_animations_intensity();
        assert_eq!(panel.animations_intensity, Energetic);
        panel.next_animations_intensity();
        assert_eq!(panel.animations_intensity, Off, "wraps back to Off");

        panel.prev_animations_intensity();
        assert_eq!(panel.animations_intensity, Energetic);
    }

    #[test]
    fn window_field_increase_decrease_dispatch_to_all_11_fields() {
        // Regression guard: every field index 0..=10 must be wired into both
        // `window_field_increase` and `window_field_decrease` (a missing arm
        // would silently no-op instead of panicking, so assert observable change).
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);

        panel.focused_widget_index = 5;
        let before = panel.cursor_blink_enabled;
        panel.window_field_increase();
        assert_eq!(panel.cursor_blink_enabled, !before);

        panel.focused_widget_index = 6;
        panel.scrollback_lines = 10_000;
        panel.window_field_increase();
        assert_eq!(panel.scrollback_lines, 11_000);
        panel.window_field_decrease();
        assert_eq!(panel.scrollback_lines, 10_000);

        panel.focused_widget_index = 7;
        let before = panel.tab_show_tab_number;
        panel.window_field_increase();
        assert_eq!(panel.tab_show_tab_number, !before);

        panel.focused_widget_index = 8;
        let before = panel.tab_show_new_tab_button;
        panel.window_field_increase();
        assert_eq!(panel.tab_show_new_tab_button, !before);

        panel.focused_widget_index = 9;
        let before = panel.animations_enabled;
        panel.window_field_increase();
        assert_eq!(panel.animations_enabled, !before);

        panel.focused_widget_index = 10;
        use nexterm_config::AnimationIntensity::Normal;
        panel.animations_intensity = Normal;
        panel.window_field_increase();
        assert_eq!(
            panel.animations_intensity,
            nexterm_config::AnimationIntensity::Energetic
        );
    }

    #[test]
    fn save_writes_cursor_blink_enabled() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.cursor_blink_enabled = false;
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("blink_enabled = false"));
    }

    #[test]
    fn save_writes_in_app_blur_enabled() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.toggle_in_app_blur();
        assert!(panel.in_app_blur_enabled);
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("in_app_blur_enabled = true"));
    }

    #[test]
    fn window_field_increase_decrease_dispatch_in_app_blur_rows() {
        // The physical arrow-key handler calls `window_field_increase` /
        // `window_field_decrease` directly (bypassing `apply_window_action`),
        // so the new P2b rows need their own arms here to be keyboard
        // reachable at all, exactly like every other Window row.
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);

        panel.focused_widget_index = 14;
        let before = panel.in_app_blur_enabled;
        panel.window_field_increase();
        assert_eq!(panel.in_app_blur_enabled, !before);
        panel.window_field_decrease();
        assert_eq!(panel.in_app_blur_enabled, before);

        panel.focused_widget_index = 15;
        panel.in_app_blur_strength = 0.5;
        panel.window_field_increase();
        assert!((panel.in_app_blur_strength - 0.55).abs() < 0.001);
        panel.window_field_decrease();
        assert!((panel.in_app_blur_strength - 0.5).abs() < 0.001);
    }

    #[test]
    fn save_writes_scrollback_lines() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.scrollback_lines = 123_000;
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("scrollback_lines = 123000"));
    }

    #[test]
    fn save_writes_tab_bar_toggles() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.tab_show_tab_number = true;
        panel.tab_show_new_tab_button = false;
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("show_tab_number = true"));
        assert!(toml_str.contains("show_new_tab_button = false"));
    }

    #[test]
    fn save_writes_animations_enabled_and_intensity() {
        use nexterm_config::AnimationIntensity;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.animations_enabled = false;
        panel.animations_intensity = AnimationIntensity::Subtle;
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("enabled = false"));
        assert!(toml_str.contains("intensity = \"subtle\""));
    }
}
