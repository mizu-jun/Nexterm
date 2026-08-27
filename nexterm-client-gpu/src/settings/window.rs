//! Window category (part 1): opacity, padding, cursor style, present
//! mode, window decorations/close action, and FPS limit. The field-index
//! navigation dispatcher, cursor blink, scrollback length, tab-bar
//! toggles, and animation settings live in `window_extra.rs`.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::SettingsPanel;
use nexterm_i18n::fl;

impl SettingsPanel {
    /// Set the opacity from a slider X coordinate (used by mouse clicks/drags).
    pub fn set_opacity_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        // Opacity range: 0.1..=1.0 (snapped to 5% steps).
        let raw = 0.1 + ratio * 0.9;
        self.opacity = (raw * 20.0).round() / 20.0;
        self.dirty = true;
    }

    /// Phase 5-11-6 #6: set `padding_x` (0–32 px) from a slider X coordinate.
    pub fn set_padding_x_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        self.padding_x = (ratio * 32.0).round() as u32;
        self.dirty = true;
    }

    /// Phase 5-11-6 #6: set `padding_y` (0–32 px) from a slider X coordinate.
    pub fn set_padding_y_from_slider(&mut self, cursor_x: f32, track_x: f32, track_w: f32) {
        let ratio = ((cursor_x - track_x) / track_w).clamp(0.0, 1.0);
        self.padding_y = (ratio * 32.0).round() as u32;
        self.dirty = true;
    }

    pub fn increase_opacity(&mut self) {
        self.opacity = (self.opacity + 0.05).min(1.0);
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn decrease_opacity(&mut self) {
        self.opacity = (self.opacity - 0.05).max(0.1);
        self.dirty = true;
    }

    /// Used by SR via `Action::SetValue(NumericValue)`: clamp the f64 value to
    /// `0.1..=1.0`, snap to 0.05 steps, and store it as the opacity.
    pub fn set_opacity_value(&mut self, v: f64) {
        let raw = (v as f32).clamp(0.1, 1.0);
        self.opacity = (raw * 20.0).round() / 20.0;
        self.dirty = true;
    }

    /// Increment `[window].in_app_blur_strength` (P2b), mirroring `increase_opacity`.
    pub fn increase_in_app_blur_strength(&mut self) {
        self.in_app_blur_strength = (self.in_app_blur_strength + 0.05).min(1.0);
        self.dirty = true;
    }

    /// Decrement `[window].in_app_blur_strength` (P2b), mirroring `decrease_opacity`.
    pub fn decrease_in_app_blur_strength(&mut self) {
        self.in_app_blur_strength = (self.in_app_blur_strength - 0.05).max(0.0);
        self.dirty = true;
    }

    /// Used by SR via `Action::SetValue(NumericValue)`: clamp the f64 value to
    /// `0.0..=1.0`, snap to 0.05 steps, and store it as the in-app blur
    /// strength. Mirrors `set_opacity_value`'s clamp-then-quantize shape.
    pub fn set_in_app_blur_strength_value(&mut self, v: f64) {
        let raw = (v as f32).clamp(0.0, 1.0);
        self.in_app_blur_strength = (raw * 20.0).round() / 20.0;
        self.dirty = true;
    }

    pub fn next_cursor_style(&mut self) {
        use nexterm_config::CursorStyle::*;
        self.cursor_style = match self.cursor_style {
            Block => Beam,
            Beam => Underline,
            Underline => Block,
        };
        self.dirty = true;
    }

    pub fn prev_cursor_style(&mut self) {
        use nexterm_config::CursorStyle::*;
        self.cursor_style = match self.cursor_style {
            Block => Underline,
            Beam => Block,
            Underline => Beam,
        };
        self.dirty = true;
    }

    /// Enumeration index (0=Block, 1=Beam, 2=Underline). Used for UI drawing
    /// and the AccessKit `Action::SetValue` path (currently only via tests).
    #[allow(dead_code)]
    pub fn cursor_style_index(&self) -> usize {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => 0,
            Beam => 1,
            Underline => 2,
        }
    }

    /// UI display label.
    pub fn cursor_style_label(&self) -> String {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => fl!("settings-value-cursor-block"),
            Beam => fl!("settings-value-cursor-beam"),
            Underline => fl!("settings-value-cursor-underline"),
        }
    }

    /// Lowercase TOML key for write-back (matches `serde`'s `rename_all = "lowercase"`).
    pub fn cursor_style_toml_key(&self) -> &'static str {
        use nexterm_config::CursorStyle::*;
        match self.cursor_style {
            Block => "block",
            Beam => "beam",
            Underline => "underline",
        }
    }

    pub fn set_padding_x_value(&mut self, v: f64) {
        self.padding_x = (v.round().clamp(0.0, 32.0)) as u32;
        self.dirty = true;
    }

    pub fn increase_padding_x(&mut self) {
        self.padding_x = (self.padding_x + 1).min(32);
        self.dirty = true;
    }

    pub fn decrease_padding_x(&mut self) {
        self.padding_x = self.padding_x.saturating_sub(1);
        self.dirty = true;
    }

    pub fn set_padding_y_value(&mut self, v: f64) {
        self.padding_y = (v.round().clamp(0.0, 32.0)) as u32;
        self.dirty = true;
    }

    pub fn increase_padding_y(&mut self) {
        self.padding_y = (self.padding_y + 1).min(32);
        self.dirty = true;
    }

    pub fn decrease_padding_y(&mut self) {
        self.padding_y = self.padding_y.saturating_sub(1);
        self.dirty = true;
    }

    pub fn next_present_mode(&mut self) {
        use nexterm_config::PresentModeConfig::*;
        self.present_mode = match self.present_mode {
            Fifo => Mailbox,
            Mailbox => Auto,
            Auto => Fifo,
        };
        self.dirty = true;
    }

    pub fn prev_present_mode(&mut self) {
        use nexterm_config::PresentModeConfig::*;
        self.present_mode = match self.present_mode {
            Fifo => Auto,
            Mailbox => Fifo,
            Auto => Mailbox,
        };
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn present_mode_index(&self) -> usize {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => 0,
            Mailbox => 1,
            Auto => 2,
        }
    }

    pub fn present_mode_label(&self) -> String {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => fl!("settings-value-present-fifo"),
            Mailbox => fl!("settings-value-present-mailbox"),
            Auto => fl!("settings-value-present-auto"),
        }
    }

    pub fn present_mode_toml_key(&self) -> &'static str {
        use nexterm_config::PresentModeConfig::*;
        match self.present_mode {
            Fifo => "fifo",
            Mailbox => "mailbox",
            Auto => "auto",
        }
    }

    pub fn next_window_decorations(&mut self) {
        use nexterm_config::WindowDecorations::*;
        self.window_decorations = match self.window_decorations {
            Full => None,
            None => NoTitle,
            NoTitle => Full,
        };
        self.dirty = true;
    }

    pub fn prev_window_decorations(&mut self) {
        use nexterm_config::WindowDecorations::*;
        self.window_decorations = match self.window_decorations {
            Full => NoTitle,
            None => Full,
            NoTitle => None,
        };
        self.dirty = true;
    }

    pub fn window_decorations_label(&self) -> String {
        use nexterm_config::WindowDecorations::*;
        match self.window_decorations {
            Full => fl!("settings-value-decorations-full"),
            None => fl!("settings-value-decorations-none"),
            NoTitle => fl!("settings-value-decorations-notitle"),
        }
    }

    pub fn window_decorations_toml_key(&self) -> &'static str {
        use nexterm_config::WindowDecorations::*;
        match self.window_decorations {
            Full => "full",
            None => "none",
            NoTitle => "notitle",
        }
    }

    // This task (P2c-2 Task 7) lands only the panel state and the cycler;
    // the Window-tab row that calls `next_window_backdrop` /
    // `prev_window_backdrop` / `window_backdrop_label` lands in the next
    // commit (Task 8), so nothing outside the tests below calls them yet.
    // `#[allow(dead_code)]` follows the same staged-landing precedent as
    // `OsWindowBounds` in `drop_target.rs`.
    #[allow(dead_code)]
    pub fn next_window_backdrop(&mut self) {
        use nexterm_config::WindowBackdrop::*;
        self.window_backdrop = match self.window_backdrop {
            Auto => Mica,
            Mica => MicaAlt,
            MicaAlt => Acrylic,
            Acrylic => None,
            None => Auto,
        };
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn prev_window_backdrop(&mut self) {
        use nexterm_config::WindowBackdrop::*;
        self.window_backdrop = match self.window_backdrop {
            Auto => None,
            Mica => Auto,
            MicaAlt => Mica,
            Acrylic => MicaAlt,
            None => Acrylic,
        };
        self.dirty = true;
    }

    #[allow(dead_code)]
    pub fn window_backdrop_label(&self) -> String {
        use nexterm_config::WindowBackdrop::*;
        match self.window_backdrop {
            Auto => fl!("settings-value-backdrop-auto"),
            Mica => fl!("settings-value-backdrop-mica"),
            MicaAlt => fl!("settings-value-backdrop-mica-alt"),
            Acrylic => fl!("settings-value-backdrop-acrylic"),
            None => fl!("settings-value-backdrop-none"),
        }
    }

    pub fn window_backdrop_toml_key(&self) -> &'static str {
        use nexterm_config::WindowBackdrop::*;
        match self.window_backdrop {
            Auto => "auto",
            Mica => "mica",
            MicaAlt => "mica-alt",
            Acrylic => "acrylic",
            None => "none",
        }
    }

    pub fn next_window_close_action(&mut self) {
        use nexterm_config::CloseAction::*;
        self.window_close_action = match self.window_close_action {
            Prompt => Detach,
            Detach => Kill,
            Kill => Prompt,
        };
        self.dirty = true;
    }

    pub fn prev_window_close_action(&mut self) {
        use nexterm_config::CloseAction::*;
        self.window_close_action = match self.window_close_action {
            Prompt => Kill,
            Detach => Prompt,
            Kill => Detach,
        };
        self.dirty = true;
    }

    pub fn window_close_action_label(&self) -> String {
        use nexterm_config::CloseAction::*;
        match self.window_close_action {
            Prompt => fl!("settings-value-close-prompt"),
            Detach => fl!("settings-value-close-detach"),
            Kill => fl!("settings-value-close-kill"),
        }
    }

    pub fn window_close_action_toml_key(&self) -> &'static str {
        use nexterm_config::CloseAction::*;
        match self.window_close_action {
            Prompt => "prompt",
            Detach => "detach",
            Kill => "kill",
        }
    }

    #[allow(dead_code)]
    pub const FPS_LIMIT_MIN: u32 = 0;

    pub const FPS_LIMIT_MAX: u32 = 480;

    pub const FPS_LIMIT_STEP: u32 = 10;

    pub fn increase_fps_limit(&mut self) {
        self.fps_limit = (self.fps_limit + Self::FPS_LIMIT_STEP).min(Self::FPS_LIMIT_MAX);
        self.dirty = true;
    }

    pub fn decrease_fps_limit(&mut self) {
        self.fps_limit = self.fps_limit.saturating_sub(Self::FPS_LIMIT_STEP);
        self.dirty = true;
    }

    /// UI display label: "unlimited" at 0, otherwise the numeric value.
    pub fn fps_limit_label(&self) -> String {
        if self.fps_limit == 0 {
            fl!("settings-value-fps-unlimited")
        } else {
            self.fps_limit.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn cursor_style_cycle_forward_and_back() {
        use nexterm_config::CursorStyle::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        // Default is Block.
        assert_eq!(panel.cursor_style, Block);
        assert_eq!(panel.cursor_style_index(), 0);
        assert_eq!(panel.cursor_style_toml_key(), "block");

        panel.next_cursor_style();
        assert_eq!(panel.cursor_style, Beam);
        assert_eq!(panel.cursor_style_index(), 1);
        assert_eq!(panel.cursor_style_toml_key(), "beam");

        panel.next_cursor_style();
        assert_eq!(panel.cursor_style, Underline);
        assert_eq!(panel.cursor_style_toml_key(), "underline");

        panel.next_cursor_style();
        assert_eq!(
            panel.cursor_style, Block,
            "the slot after Underline wraps to Block"
        );

        // Reverse direction.
        panel.prev_cursor_style();
        assert_eq!(
            panel.cursor_style, Underline,
            "the slot before Block is Underline"
        );
        panel.prev_cursor_style();
        assert_eq!(panel.cursor_style, Beam);
        panel.prev_cursor_style();
        assert_eq!(panel.cursor_style, Block);

        assert!(panel.dirty);
    }

    #[test]
    fn cursor_style_labels_are_human_readable() {
        use nexterm_config::CursorStyle::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.cursor_style = Block;
        assert!(panel.cursor_style_label().contains("Block"));
        panel.cursor_style = Beam;
        assert!(panel.cursor_style_label().contains("Beam"));
        panel.cursor_style = Underline;
        assert!(panel.cursor_style_label().contains("Underline"));
    }

    #[test]
    fn padding_x_increase_decrease_clamps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert_eq!(panel.padding_x, 0, "the default is 0");

        // Clamps at the upper bound 32.
        for _ in 0..40 {
            panel.increase_padding_x();
        }
        assert_eq!(panel.padding_x, 32);

        // Clamps at the lower bound 0.
        for _ in 0..40 {
            panel.decrease_padding_x();
        }
        assert_eq!(panel.padding_x, 0);

        assert!(panel.dirty);
    }

    #[test]
    fn padding_y_increase_decrease_clamps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        for _ in 0..50 {
            panel.increase_padding_y();
        }
        assert_eq!(panel.padding_y, 32, "upper bound");
        for _ in 0..50 {
            panel.decrease_padding_y();
        }
        assert_eq!(panel.padding_y, 0, "lower bound");
    }

    #[test]
    fn padding_set_value_clamps_and_rounds() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.set_padding_x_value(-5.0);
        assert_eq!(panel.padding_x, 0, "negative values clamp to 0");
        panel.set_padding_x_value(100.0);
        assert_eq!(
            panel.padding_x, 32,
            "values above the upper bound clamp to 32"
        );
        panel.set_padding_x_value(15.7);
        assert_eq!(panel.padding_x, 16, "values at or above .5 round up");
        panel.set_padding_x_value(15.3);
        assert_eq!(panel.padding_x, 15, "values below .5 round down");

        panel.set_padding_y_value(7.5);
        assert_eq!(
            panel.padding_y, 8,
            ".5 may round either bankers/half-up depending on the implementation"
        );
    }

    #[test]
    fn in_app_blur_strength_value_clamps_and_rounds() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.set_in_app_blur_strength_value(-0.5);
        assert_eq!(
            panel.in_app_blur_strength, 0.0,
            "negative values clamp to 0.0"
        );
        panel.set_in_app_blur_strength_value(2.0);
        assert_eq!(
            panel.in_app_blur_strength, 1.0,
            "values above 1.0 clamp to 1.0"
        );
        panel.set_in_app_blur_strength_value(0.3);
        assert!(
            (panel.in_app_blur_strength - 0.3).abs() < 0.05,
            "slider step-rounded, same tolerance as opacity's test"
        );
        assert!(panel.dirty);
    }

    #[test]
    fn in_app_blur_strength_increase_decrease_clamps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.in_app_blur_strength = 0.98;
        panel.increase_in_app_blur_strength();
        assert_eq!(panel.in_app_blur_strength, 1.0, "clamps at the upper bound");

        panel.in_app_blur_strength = 0.02;
        panel.decrease_in_app_blur_strength();
        assert_eq!(panel.in_app_blur_strength, 0.0, "clamps at the lower bound");
    }

    #[test]
    fn present_mode_cycle_forward_and_back() {
        use nexterm_config::PresentModeConfig::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        // The default is Mailbox (changed in Sprint 5-3 / C3).
        assert_eq!(panel.present_mode, Mailbox);
        assert_eq!(panel.present_mode_index(), 1);
        assert_eq!(panel.present_mode_toml_key(), "mailbox");

        panel.next_present_mode();
        assert_eq!(panel.present_mode, Auto);
        panel.next_present_mode();
        assert_eq!(panel.present_mode, Fifo);
        panel.next_present_mode();
        assert_eq!(panel.present_mode, Mailbox);

        // Reverse direction.
        panel.prev_present_mode();
        assert_eq!(panel.present_mode, Fifo);

        assert!(panel.dirty);
    }

    #[test]
    fn present_mode_labels_are_human_readable() {
        use nexterm_config::PresentModeConfig::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.present_mode = Fifo;
        assert!(panel.present_mode_label().contains("Fifo"));
        panel.present_mode = Mailbox;
        assert!(panel.present_mode_label().contains("Mailbox"));
        panel.present_mode = Auto;
        assert!(panel.present_mode_label().contains("Auto"));
    }

    #[test]
    fn new_reads_config_window_padding_and_present_mode() {
        let mut config = Config::default();
        config.window.padding_x = 12;
        config.window.padding_y = 4;
        config.gpu.present_mode = nexterm_config::PresentModeConfig::Fifo;
        config.cursor_style = nexterm_config::CursorStyle::Beam;

        let panel = SettingsPanel::new(&config);
        assert_eq!(panel.padding_x, 12);
        assert_eq!(panel.padding_y, 4);
        assert_eq!(panel.present_mode, nexterm_config::PresentModeConfig::Fifo);
        assert_eq!(panel.cursor_style, nexterm_config::CursorStyle::Beam);
    }

    #[test]
    fn new_clamps_oversized_padding_from_config() {
        let mut config = Config::default();
        config.window.padding_x = 1000;
        let panel = SettingsPanel::new(&config);
        assert_eq!(
            panel.padding_x, 32,
            "out-of-range config values are clamped to 32 in `new`"
        );
    }

    #[test]
    fn save_writes_window_decorations() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        // Start from an explicit variant: the default is platform-dependent
        // (notitle; full on macOS), and this test is about the cycle order
        // and the TOML write-back, not the default.
        panel.window_decorations = nexterm_config::WindowDecorations::Full;
        panel.next_window_decorations();
        assert_eq!(
            panel.window_decorations,
            nexterm_config::WindowDecorations::None
        );
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("decorations = \"none\""));
    }

    #[test]
    fn save_writes_window_close_action() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.next_window_close_action();
        assert_eq!(
            panel.window_close_action,
            nexterm_config::CloseAction::Detach
        );
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("close_action = \"detach\""));
    }

    #[test]
    fn save_writes_fps_limit() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.fps_limit = 144;
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("fps_limit = 144"));
    }

    #[test]
    fn save_writes_in_app_blur_strength() {
        // 0.5 is exactly representable in f32, so the f64-cast TOML value
        // round-trips without float-precision noise in the string check.
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.set_in_app_blur_strength_value(0.5);
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("in_app_blur_strength = 0.5"));
    }

    #[test]
    fn fps_limit_label_shows_unlimited_at_zero() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.fps_limit = 0;
        assert_eq!(panel.fps_limit_label(), "unlimited");
        panel.increase_fps_limit();
        assert_eq!(panel.fps_limit, 10);
        assert_eq!(panel.fps_limit_label(), "10");
    }

    #[test]
    fn fps_limit_clamps_to_max() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.fps_limit = SettingsPanel::FPS_LIMIT_MAX;
        panel.increase_fps_limit();
        assert_eq!(panel.fps_limit, SettingsPanel::FPS_LIMIT_MAX);
    }

    #[test]
    fn backdrop_cycles_and_writes_back() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        assert_eq!(panel.window_backdrop, nexterm_config::WindowBackdrop::Auto);

        panel.next_window_backdrop();
        assert_eq!(panel.window_backdrop, nexterm_config::WindowBackdrop::Mica);

        let toml_str = panel.apply_to_toml_string("");
        assert!(
            toml_str.contains("backdrop = \"mica\""),
            "the cycled value must reach the file: {toml_str}"
        );
    }

    #[test]
    fn backdrop_cycling_is_a_closed_loop_in_both_directions() {
        use nexterm_config::WindowBackdrop::*;
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        for expected in [Mica, MicaAlt, Acrylic, None, Auto] {
            panel.next_window_backdrop();
            assert_eq!(panel.window_backdrop, expected);
        }
        for expected in [None, Acrylic, MicaAlt, Mica, Auto] {
            panel.prev_window_backdrop();
            assert_eq!(panel.window_backdrop, expected);
        }
    }
}
