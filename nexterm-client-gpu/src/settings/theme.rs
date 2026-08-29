//! Theme category: color-scheme selection and light/dark/follow-system
//! toggling, plus the scheme <-> index helpers shared with the renderer's
//! live theme preview.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::SettingsPanel;

impl SettingsPanel {
    pub fn next_scheme(&mut self) {
        self.scheme_index = (self.scheme_index + 1) % scheme_count();
        self.dirty = true;
    }

    pub fn prev_scheme(&mut self) {
        self.scheme_index = if self.scheme_index == 0 {
            scheme_count() - 1
        } else {
            self.scheme_index - 1
        };
        self.dirty = true;
    }

    /// Return the scheme name for the current `scheme_index`.
    pub fn scheme_name(&self) -> &str {
        nexterm_config::BuiltinScheme::all()[self.scheme_index % scheme_count()].toml_name()
    }

    /// Toggle `colors_follow_system`.
    pub fn toggle_colors_follow_system(&mut self) {
        self.colors_follow_system = !self.colors_follow_system;
        self.dirty = true;
    }
}

/// Number of selectable built-in schemes — the cycler, the swatch strip and
/// the index helpers all wrap on it. Read from `BuiltinScheme::all()` so adding
/// a scheme cannot leave one of them a slot behind (UI/UX v3 P5c).
pub(crate) fn scheme_count() -> usize {
    nexterm_config::BuiltinScheme::all().len()
}

/// Convert a color scheme into its index.
pub(crate) fn scheme_name_to_index(colors: &nexterm_config::ColorScheme) -> usize {
    use nexterm_config::ColorScheme;
    match colors {
        ColorScheme::Builtin(b) => nexterm_config::BuiltinScheme::all()
            .iter()
            .position(|s| s == b)
            .unwrap_or(0),
        ColorScheme::Custom(_) => 0,
    }
}

/// Inverse of `scheme_name_to_index`: map a slot to a `BuiltinScheme`. Used by
/// Phase 3b live theme preview to derive a `ColorScheme` value from a hovered
/// dot index. Pure helper so it can be unit-tested without instantiating a
/// renderer.
///
/// Out-of-range inputs wrap modulo the scheme count so the caller doesn't need
/// to clamp ahead of time.
pub fn index_to_builtin_scheme(idx: usize) -> nexterm_config::BuiltinScheme {
    let all = nexterm_config::BuiltinScheme::all();
    all[idx % all.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::Config;

    #[test]
    fn scheme_wraps() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        let last = scheme_count() - 1;
        panel.scheme_index = last;
        panel.next_scheme();
        assert_eq!(
            panel.scheme_index, 0,
            "the slot after the last one wraps to 0"
        );

        panel.scheme_index = 0;
        panel.prev_scheme();
        assert_eq!(
            panel.scheme_index, last,
            "the slot before index 0 wraps to the last one"
        );
    }

    #[test]
    fn save_writes_colors_follow_system() {
        let config = Config::default();
        let mut panel = SettingsPanel::new(&config);
        panel.toggle_colors_follow_system();
        assert!(panel.colors_follow_system);
        let toml_str = panel.apply_to_toml_string("");
        assert!(toml_str.contains("colors_follow_system = true"));
    }
}

#[cfg(test)]
mod theme_preview_tests {
    //! Phase 3b (UI/UX v2): live theme preview helpers.
    use super::*;
    use nexterm_config::BuiltinScheme;

    /// `index_to_builtin_scheme` must round-trip with the existing
    /// `scheme_name_to_index` inverse for every slot so the live preview
    /// cannot select a scheme that the commit path then drops.
    #[test]
    fn index_to_scheme_round_trips_with_name_to_index() {
        for idx in 0..scheme_count() {
            let scheme = index_to_builtin_scheme(idx);
            let back = scheme_name_to_index(&nexterm_config::ColorScheme::Builtin(scheme));
            assert_eq!(back, idx, "round-trip mismatch at idx={}", idx);
        }
    }

    /// Out-of-range inputs must wrap modulo the scheme count rather than
    /// panic — the renderer passes the field value verbatim and we don't
    /// want stray hover state to crash the frame.
    #[test]
    fn index_to_scheme_wraps_out_of_range() {
        let n = scheme_count();
        assert_eq!(index_to_builtin_scheme(n), BuiltinScheme::Dark);
        assert_eq!(
            index_to_builtin_scheme(2 * n - 1),
            index_to_builtin_scheme(n - 1)
        );
        assert_eq!(
            index_to_builtin_scheme(usize::MAX),
            index_to_builtin_scheme(usize::MAX % n)
        );
    }

    /// A fresh panel must start with no hover preview so the first
    /// open frame uses the configured scheme, not a stale value left
    /// over from a previous session.
    #[test]
    fn fresh_panel_has_no_hover_preview() {
        let panel = SettingsPanel::default();
        assert_eq!(panel.theme_hover_preview, None);
    }

    /// `close()` must drop any in-flight hover preview so the next
    /// open starts on the configured scheme even when the user
    /// dismissed the panel mid-hover.
    #[test]
    fn close_clears_hover_preview() {
        let mut panel = SettingsPanel {
            is_open: true,
            theme_hover_preview: Some(3),
            ..SettingsPanel::default()
        };
        panel.close(
            std::time::Instant::now(),
            &nexterm_config::AnimationsConfig::default(),
        );
        assert_eq!(panel.theme_hover_preview, None);
        assert!(!panel.is_open);
    }

    /// Commit (setting `scheme_index` from a click handler) must NOT
    /// rely on `theme_hover_preview` being kept in sync — the commit
    /// path moves the value into `scheme_index`, and 3b's renderer
    /// then falls back to the configured scheme on the next frame.
    /// This guards the click handler's "clear after commit" semantics.
    #[test]
    fn scheme_index_is_independent_of_preview() {
        let panel = SettingsPanel {
            theme_hover_preview: Some(5),
            scheme_index: 2,
            ..SettingsPanel::default()
        };
        assert_eq!(panel.scheme_index, 2);
        assert_eq!(panel.theme_hover_preview, Some(5));
    }
}
