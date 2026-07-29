//! P2-B (WT-like UX): visible-row model for search collapse.
//!
//! While a field-level search query is active, categories with a uniform
//! row grid (Window / Security / Blocks) render only the matching rows,
//! compacted to the top — Windows Terminal 1.25 behaviour. The renderer
//! and the hit-test both derive row Y positions from the same
//! [`visible_rows`] list, so they cannot drift apart.
//!
//! Matching runs on the row's *label* (the localized string rendered with
//! its value placeholder blanked), not the live value, so a row does not
//! pop in and out while its value is edited.

use super::SettingsPanel;
use super::category::label_matches_query;
use nexterm_i18n::fl;

/// Logical indices of the rows that stay visible for `query`, in row
/// order. A blank query keeps every row (identity mapping). When the
/// query matches nothing, every row stays visible too — an all-collapsed
/// page reads as broken and gives the user nothing to act on.
pub fn visible_rows(labels: &[String], query: &str) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..labels.len()).collect();
    }
    let matched: Vec<usize> = (0..labels.len())
        .filter(|&i| label_matches_query(&labels[i], query))
        .collect();
    if matched.is_empty() {
        (0..labels.len()).collect()
    } else {
        matched
    }
}

/// Compacted slot position of logical row `idx` inside `visible`, or
/// `None` when the row is collapsed.
pub fn slot_of(visible: &[usize], idx: usize) -> Option<usize> {
    visible.iter().position(|&v| v == idx)
}

impl SettingsPanel {
    /// Rendered (localized) labels of the Window category's rows, in the
    /// exact `window_field_focus` order used by the renderer and the
    /// hit-test. Value placeholders are blanked — the label part is what
    /// search collapse matches on.
    pub fn window_row_labels(&self) -> Vec<String> {
        vec![
            fl!("settings-window-opacity", value = ""),
            fl!("settings-window-cursor-style"),
            fl!("settings-window-horizontal-padding", value = ""),
            fl!("settings-window-vertical-padding", value = ""),
            fl!("settings-window-present-mode"),
            fl!("settings-window-cursor-blink", value = ""),
            fl!("settings-window-scrollback-lines", value = ""),
            fl!("settings-window-show-tab-number", value = ""),
            fl!("settings-window-show-new-tab-button", value = ""),
            fl!("settings-window-animations-enabled", value = ""),
            fl!("settings-window-animation-intensity"),
            fl!("settings-window-decorations"),
            fl!("settings-window-close-action"),
            fl!("settings-window-fps-limit", value = ""),
        ]
    }

    /// Visible Window rows for the current search query — see
    /// [`visible_rows`].
    pub fn visible_window_rows(&self) -> Vec<usize> {
        visible_rows(&self.window_row_labels(), &self.search_query)
    }

    /// Visible Security rows (labels via `security_field_label`, in
    /// `security_field_focus` order).
    pub fn visible_security_rows(&self) -> Vec<usize> {
        let labels: Vec<String> = (0..Self::SECURITY_FIELD_COUNT)
            .map(Self::security_field_label)
            .collect();
        visible_rows(&labels, &self.search_query)
    }

    /// Visible Blocks rows (row order matches `BlocksRow`:
    /// 0 = enabled, 1 = border width, 2 = status badge).
    pub fn visible_blocks_rows(&self) -> Vec<usize> {
        let labels = vec![
            fl!("settings-blocks-enabled"),
            fl!("settings-blocks-border-width"),
            fl!("settings-blocks-status-badge"),
        ];
        visible_rows(&labels, &self.search_query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn blank_query_keeps_every_row() {
        let l = labels(&["Opacity", "Cursor style", "Scrollback"]);
        assert_eq!(visible_rows(&l, ""), vec![0, 1, 2]);
        assert_eq!(visible_rows(&l, "  "), vec![0, 1, 2]);
    }

    #[test]
    fn matching_rows_are_kept_in_order() {
        let l = labels(&["Opacity", "Cursor style", "Cursor blink", "Scrollback"]);
        assert_eq!(visible_rows(&l, "cursor"), vec![1, 2]);
    }

    #[test]
    fn no_match_falls_back_to_all_rows() {
        // An all-collapsed page reads as broken; keep everything instead.
        let l = labels(&["Opacity", "Cursor style"]);
        assert_eq!(visible_rows(&l, "zzz-no-such-field"), vec![0, 1]);
    }

    #[test]
    fn slot_of_maps_logical_to_compacted_positions() {
        let visible = vec![1usize, 2];
        assert_eq!(slot_of(&visible, 1), Some(0));
        assert_eq!(slot_of(&visible, 2), Some(1));
        assert_eq!(slot_of(&visible, 0), None);
    }

    #[test]
    fn window_labels_match_the_field_count() {
        nexterm_i18n::set_locale("en");
        let sp = SettingsPanel::default();
        assert_eq!(
            sp.window_row_labels().len() as u8,
            SettingsPanel::WINDOW_FIELD_COUNT,
            "window_row_labels must stay in sync with WINDOW_FIELD_COUNT"
        );
    }

    #[test]
    fn window_query_collapses_to_matching_rows() {
        nexterm_i18n::set_locale("en");
        let sp = SettingsPanel {
            search_query: "padding".to_string(),
            ..Default::default()
        };
        let visible = sp.visible_window_rows();
        // Rows 2 (horizontal padding) and 3 (vertical padding) must survive;
        // opacity (0) must not.
        assert!(visible.contains(&2));
        assert!(visible.contains(&3));
        assert!(!visible.contains(&0));
    }
}
