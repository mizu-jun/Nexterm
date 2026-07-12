//! Sidebar category enum, per-category field metadata, and the fuzzy
//! category/field search used by the settings panel's search box.
//!
//! Moved out of `settings_panel.rs` (Phase B6 mechanical split).

use super::SettingsPanel;
use nexterm_i18n::fl;

/// Sidebar category.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsCategory {
    Startup,
    Font,
    Theme,
    Window,
    Ssh,
    Keybindings,
    Profiles,
    /// `[blocks]` config section — fully editable via mouse click (see
    /// `mouse.rs`'s `BlocksRow` handler, which toggles/cycles the value and
    /// saves immediately).
    Blocks,
    /// `[security]` consent policies (external URL / OSC 52 clipboard /
    /// OSC 9-777 notification / plugin read) plus their byte-size caps.
    Security,
}

impl SettingsCategory {
    pub const ALL: &'static [SettingsCategory] = &[
        SettingsCategory::Startup,
        SettingsCategory::Font,
        SettingsCategory::Theme,
        SettingsCategory::Window,
        SettingsCategory::Ssh,
        SettingsCategory::Keybindings,
        SettingsCategory::Profiles,
        SettingsCategory::Blocks,
        SettingsCategory::Security,
    ];

    pub fn label(&self) -> String {
        match self {
            SettingsCategory::Startup => fl!("settings-category-startup"),
            SettingsCategory::Font => fl!("settings-category-font"),
            SettingsCategory::Theme => fl!("settings-category-theme"),
            SettingsCategory::Window => fl!("settings-category-window"),
            SettingsCategory::Ssh => fl!("settings-category-ssh"),
            SettingsCategory::Keybindings => fl!("settings-category-keybindings"),
            SettingsCategory::Profiles => fl!("settings-category-profiles"),
            SettingsCategory::Blocks => fl!("settings-category-blocks"),
            SettingsCategory::Security => fl!("settings-category-security"),
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            SettingsCategory::Startup => "▶",
            SettingsCategory::Font => "Aa",
            SettingsCategory::Theme => "◐",
            SettingsCategory::Window => "▢",
            SettingsCategory::Ssh => "⊞",
            SettingsCategory::Keybindings => "⌨",
            SettingsCategory::Profiles => "◉",
            SettingsCategory::Blocks => "▤",
            SettingsCategory::Security => "⚿",
        }
    }
}

impl SettingsPanel {
    /// Activate the search input. The next character event lands in
    /// `search_query` instead of being treated as a panel hotkey.
    pub fn focus_search(&mut self) {
        self.search_focused = true;
    }

    /// Deactivate the search input but keep the query (so the filter
    /// remains visible). Use `clear_search` to drop the query too.
    pub fn unfocus_search(&mut self) {
        self.search_focused = false;
    }

    /// Clear the query and defocus. Triggered by Esc while the search field
    /// is focused.
    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_focused = false;
    }

    /// Append a character to the query.
    pub fn push_search_char(&mut self, ch: char) {
        if !ch.is_control() {
            self.search_query.push(ch);
        }
    }

    /// Remove the last character from the query.
    pub fn pop_search_char(&mut self) {
        self.search_query.pop();
    }

    /// Whether the search query is currently filtering categories.
    pub fn is_search_filtering(&self) -> bool {
        !self.search_query.trim().is_empty()
    }

    /// Categories visible in the sidebar given the current query. Empty
    /// query returns every category in the canonical order. Pure function so
    /// tests can pin the ranking behaviour.
    pub fn filtered_categories(&self) -> Vec<SettingsCategory> {
        filter_categories(&self.search_query, SettingsCategory::ALL)
    }

    /// Number of fields in `cat` that match the current `search_query`.
    /// Returns `0` when the query is empty so the sidebar can suppress
    /// the badge cleanly. Phase 4b: drives the `(N)` hit-count after
    /// the category label.
    pub fn field_hit_count(&self, cat: &SettingsCategory) -> usize {
        field_hit_count(cat, &self.search_query)
    }
}

/// A single searchable field inside a category. Phase 4b extends the
/// Phase 4 category-level filter so a query like "font size" highlights
/// the Font category with a hit-count badge, instead of only matching
/// against a small curated keyword list.
///
/// `label` is the human-readable label as shown (or close to what is
/// shown) in the settings panel. `aliases` are extra search terms that
/// should also hit this field (e.g. "shell" for the `command` field
/// inside Profiles).
#[derive(Debug, Clone, Copy)]
pub struct FieldEntry {
    pub label: &'static str,
    pub aliases: &'static [&'static str],
}

/// Pure catalogue of the searchable fields per category. Used by the
/// Phase 4b field-level search; intentionally curated rather than
/// auto-derived from the renderer because the renderer is hand-written
/// per category and there is no single struct of record. The labels do
/// not need to match the renderer byte-for-byte; they exist so that
/// fuzzy queries against the field name or its aliases land on the
/// right category.
pub fn category_fields(cat: &SettingsCategory) -> &'static [FieldEntry] {
    match cat {
        SettingsCategory::Startup => &[
            FieldEntry {
                label: "Language",
                aliases: &["locale", "i18n", "translation"],
            },
            FieldEntry {
                label: "Check for updates on startup",
                aliases: &["update", "release", "auto-update"],
            },
            FieldEntry {
                label: "Restore previous session",
                aliases: &["session", "snapshot", "persist"],
            },
        ],
        SettingsCategory::Font => &[
            FieldEntry {
                label: "Family",
                aliases: &["font", "typeface"],
            },
            FieldEntry {
                label: "Size",
                aliases: &["font size", "pt", "px"],
            },
            FieldEntry {
                label: "Ligatures",
                aliases: &["ligature", "harfbuzz"],
            },
        ],
        SettingsCategory::Theme => &[
            FieldEntry {
                label: "Theme",
                aliases: &["color scheme", "palette", "colors"],
            },
            FieldEntry {
                label: "Light theme",
                aliases: &["light", "day"],
            },
            FieldEntry {
                label: "Dark theme",
                aliases: &["dark", "night"],
            },
            FieldEntry {
                label: "Follow system theme",
                aliases: &["os theme", "system", "auto"],
            },
        ],
        SettingsCategory::Window => &[
            FieldEntry {
                label: "Opacity",
                aliases: &["transparency", "alpha"],
            },
            FieldEntry {
                label: "Horizontal padding",
                aliases: &["padding x", "margin"],
            },
            FieldEntry {
                label: "Vertical padding",
                aliases: &["padding y", "margin"],
            },
            FieldEntry {
                label: "Cursor style",
                aliases: &["caret", "block", "beam", "underline"],
            },
            FieldEntry {
                label: "Present mode",
                aliases: &["vsync", "fifo", "mailbox"],
            },
            FieldEntry {
                label: "Acrylic blur",
                aliases: &["blur", "acrylic", "windows 11"],
            },
            FieldEntry {
                label: "Background image",
                aliases: &["wallpaper", "image"],
            },
        ],
        SettingsCategory::Ssh => &[
            FieldEntry {
                label: "SSH hosts",
                aliases: &["remote", "ssh", "host"],
            },
            FieldEntry {
                label: "Name",
                aliases: &["host name", "alias"],
            },
            FieldEntry {
                label: "Host",
                aliases: &["hostname", "address"],
            },
            FieldEntry {
                label: "Port",
                aliases: &["tcp", "ssh port"],
            },
            FieldEntry {
                label: "Username",
                aliases: &["user", "login"],
            },
            FieldEntry {
                label: "Auth type",
                aliases: &["authentication", "key", "password", "agent"],
            },
        ],
        SettingsCategory::Keybindings => &[
            FieldEntry {
                label: "Key bindings",
                aliases: &["shortcut", "hotkey", "binding"],
            },
            FieldEntry {
                label: "Action",
                aliases: &["command"],
            },
            FieldEntry {
                label: "Modifiers",
                aliases: &["ctrl", "shift", "alt", "cmd"],
            },
        ],
        SettingsCategory::Profiles => &[
            FieldEntry {
                label: "Profiles",
                aliases: &["shell", "session profile"],
            },
            FieldEntry {
                label: "Name",
                aliases: &["profile name"],
            },
            FieldEntry {
                label: "Command",
                aliases: &["shell", "executable", "bash", "powershell", "zsh"],
            },
            FieldEntry {
                label: "Working directory",
                aliases: &["cwd", "start dir"],
            },
            FieldEntry {
                label: "Environment",
                aliases: &["env", "variable"],
            },
        ],
        SettingsCategory::Blocks => &[
            FieldEntry {
                label: "Command blocks",
                aliases: &["warp", "osc133", "block"],
            },
            FieldEntry {
                label: "Enable blocks",
                aliases: &["toggle", "on", "off"],
            },
            FieldEntry {
                label: "Block divider style",
                aliases: &["divider", "separator"],
            },
        ],
        SettingsCategory::Security => &[
            FieldEntry {
                label: "External URL policy",
                aliases: &["consent", "hyperlink", "osc8", "open url"],
            },
            FieldEntry {
                label: "OSC 52 clipboard policy",
                aliases: &["consent", "clipboard", "copy", "osc52"],
            },
            FieldEntry {
                label: "Notification policy",
                aliases: &["consent", "notify", "osc9", "osc777"],
            },
            FieldEntry {
                label: "Plugin read policy",
                aliases: &["consent", "plugin", "read", "read_pane", "privacy"],
            },
            FieldEntry {
                label: "OSC 52 max bytes",
                aliases: &["clipboard limit", "size cap"],
            },
            FieldEntry {
                label: "Notification max bytes",
                aliases: &["notification limit", "size cap"],
            },
            FieldEntry {
                label: "Plugin read max bytes",
                aliases: &["plugin limit", "read cap", "egress"],
            },
        ],
    }
}

/// Score `query` against a single field (max of label-score and
/// best alias-score). Returns `0` when nothing matched.
fn score_field(
    matcher: &fuzzy_matcher::skim::SkimMatcherV2,
    field: &FieldEntry,
    query: &str,
) -> i64 {
    use fuzzy_matcher::FuzzyMatcher;
    let label_score = matcher.fuzzy_match(field.label, query).unwrap_or(0);
    let alias_score = field
        .aliases
        .iter()
        .filter_map(|a| matcher.fuzzy_match(a, query))
        .max()
        .unwrap_or(0);
    label_score.max(alias_score)
}

/// Best field score in a category for the given query. `0` when no
/// field matched. Used both by `filter_categories` (for ranking) and
/// `field_hit_count` (for the sidebar badge).
fn best_field_score(cat: &SettingsCategory, query: &str) -> i64 {
    use fuzzy_matcher::skim::SkimMatcherV2;
    let matcher = SkimMatcherV2::default();
    category_fields(cat)
        .iter()
        .map(|f| score_field(&matcher, f, query))
        .max()
        .unwrap_or(0)
}

/// Number of fields in `cat` that match `query` with a positive score.
/// Drives the `(N)` badge in the sidebar when filtering is active.
/// Pure function so tests can pin the count behaviour.
pub fn field_hit_count(cat: &SettingsCategory, query: &str) -> usize {
    let q = query.trim();
    if q.is_empty() {
        return 0;
    }
    use fuzzy_matcher::skim::SkimMatcherV2;
    let matcher = SkimMatcherV2::default();
    category_fields(cat)
        .iter()
        .filter(|f| score_field(&matcher, f, q) > 0)
        .count()
}

/// Pure helper that ranks categories for the given fuzzy query. Empty / blank
/// queries fall through to the canonical order so the sidebar reverts cleanly
/// when the user clears the search. Match score is the max across the
/// category label and the per-field score (label + aliases) from
/// `category_fields`; categories without any positive score are
/// dropped. Stable sort on `(-score, original_index)` keeps the canonical
/// order as a tiebreaker.
pub fn filter_categories(query: &str, all: &[SettingsCategory]) -> Vec<SettingsCategory> {
    let q = query.trim();
    if q.is_empty() {
        return all.to_vec();
    }
    use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(i64, usize, SettingsCategory)> = all
        .iter()
        .enumerate()
        .filter_map(|(idx, cat)| {
            let label_score = matcher.fuzzy_match(&cat.label(), q).unwrap_or(0);
            let field_score = best_field_score(cat, q);
            let best = label_score.max(field_score);
            if best > 0 {
                Some((best, idx, cat.clone()))
            } else {
                None
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, c)| c).collect()
}

#[cfg(test)]
mod search_tests {
    //! Phase 4 (UI/UX v2): category-search fuzzy-filter tests.
    use super::*;

    /// Empty / blank queries must return every category in canonical order so
    /// the sidebar behaves identically to the pre-Phase-4 build.
    #[test]
    fn empty_query_returns_canonical_order() {
        let out = filter_categories("", SettingsCategory::ALL);
        assert_eq!(out, SettingsCategory::ALL.to_vec());
        let out = filter_categories("   ", SettingsCategory::ALL);
        assert_eq!(out, SettingsCategory::ALL.to_vec());
    }

    /// Exact-label matches must rank the target category first.
    #[test]
    fn exact_label_match_ranks_first() {
        nexterm_i18n::set_locale("en");
        let out = filter_categories("Theme", SettingsCategory::ALL);
        assert!(!out.is_empty());
        assert_eq!(out[0], SettingsCategory::Theme);
    }

    /// Keyword matches (color → Theme) prove the synonym table is consulted.
    #[test]
    fn keyword_match_finds_synonym() {
        let out = filter_categories("color", SettingsCategory::ALL);
        assert!(
            out.contains(&SettingsCategory::Theme),
            "color should match Theme via keyword, got {:?}",
            out
        );
        let out = filter_categories("shell", SettingsCategory::ALL);
        assert!(
            out.contains(&SettingsCategory::Profiles),
            "shell should match Profiles via keyword, got {:?}",
            out
        );
    }

    /// Queries with no fuzzy hit anywhere produce an empty result (so the
    /// sidebar collapses to "no matches" instead of silently showing
    /// everything).
    #[test]
    fn unmatched_query_returns_empty() {
        let out = filter_categories("xyzqq_nomatch", SettingsCategory::ALL);
        assert!(out.is_empty(), "expected empty, got {:?}", out);
    }

    /// Wiring sanity check: the struct method routes to the free helper and
    /// returns the same result.
    #[test]
    fn struct_method_matches_helper() {
        let panel = SettingsPanel {
            search_query: "block".to_string(),
            ..SettingsPanel::default()
        };
        let via_method = panel.filtered_categories();
        let via_helper = filter_categories("block", SettingsCategory::ALL);
        assert_eq!(via_method, via_helper);
    }

    /// Activation toggles must move `search_focused` without mutating the
    /// query (so the user can defocus to hit Tab then refocus later).
    #[test]
    fn focus_helpers_preserve_query() {
        let mut panel = SettingsPanel::default();
        panel.push_search_char('c');
        panel.push_search_char('o');
        panel.unfocus_search();
        assert!(!panel.search_focused);
        assert_eq!(panel.search_query, "co");
        panel.focus_search();
        assert!(panel.search_focused);
        assert_eq!(panel.search_query, "co");
        panel.clear_search();
        assert!(!panel.search_focused);
        assert!(panel.search_query.is_empty());
    }

    /// Control characters must not land in the query (regression guard for
    /// when the keyboard handler forwards Enter / Backspace as text).
    #[test]
    fn control_chars_are_skipped() {
        let mut panel = SettingsPanel::default();
        panel.push_search_char('\n');
        panel.push_search_char('\t');
        panel.push_search_char('a');
        assert_eq!(panel.search_query, "a");
    }

    // ---- Phase 4b: field-level search tests ----

    /// Every category must declare at least one field; otherwise the
    /// hit-count badge would never fire for that category.
    #[test]
    fn every_category_has_at_least_one_field() {
        for cat in SettingsCategory::ALL {
            let fields = category_fields(cat);
            assert!(
                !fields.is_empty(),
                "category {:?} has no searchable fields",
                cat
            );
        }
    }

    /// Searching for a field label (e.g. "Opacity") must hit the
    /// matching category through `filter_categories` even when the
    /// category label itself does not contain the query.
    #[test]
    fn field_label_match_finds_category() {
        let out = filter_categories("opacity", SettingsCategory::ALL);
        assert!(
            out.contains(&SettingsCategory::Window),
            "opacity should reach Window via field label, got {:?}",
            out
        );
    }

    /// Aliases declared on `FieldEntry` (e.g. "bash" for Profiles
    /// command) must also route the query to the right category.
    #[test]
    fn field_alias_match_finds_category() {
        let out = filter_categories("bash", SettingsCategory::ALL);
        assert!(
            out.contains(&SettingsCategory::Profiles),
            "bash should reach Profiles via the command field alias, got {:?}",
            out
        );
    }

    /// `field_hit_count` must return 0 for empty / blank queries so the
    /// sidebar can suppress the badge entirely when the user has not
    /// typed anything.
    #[test]
    fn field_hit_count_is_zero_for_empty_query() {
        for cat in SettingsCategory::ALL {
            assert_eq!(field_hit_count(cat, ""), 0);
            assert_eq!(field_hit_count(cat, "   "), 0);
        }
    }

    /// Hit count must be positive on the matching category and zero on
    /// an unrelated one, so the badge appears only where useful.
    #[test]
    fn field_hit_count_is_positive_for_matching_category() {
        let n = field_hit_count(&SettingsCategory::Window, "opacity");
        assert!(n >= 1, "expected ≥1 hit on Window for 'opacity', got {}", n);
        assert_eq!(
            field_hit_count(&SettingsCategory::Ssh, "opacity"),
            0,
            "SSH should not match 'opacity'"
        );
    }

    /// Hit count must rise when the query is broad enough to match
    /// multiple fields inside the same category (regression guard so we
    /// do not collapse to bool semantics).
    #[test]
    fn field_hit_count_aggregates_multiple_fields() {
        // "padding" appears in two field labels (Horizontal padding /
        // Vertical padding) inside Window.
        let n = field_hit_count(&SettingsCategory::Window, "padding");
        assert!(
            n >= 2,
            "expected ≥2 hits on Window for 'padding', got {}",
            n
        );
    }

    /// `SettingsPanel::field_hit_count` must agree with the free helper
    /// (wiring sanity check, mirrors `struct_method_matches_helper`).
    #[test]
    fn field_hit_count_struct_method_matches_helper() {
        let panel = SettingsPanel {
            search_query: "padding".to_string(),
            ..SettingsPanel::default()
        };
        let via_method = panel.field_hit_count(&SettingsCategory::Window);
        let via_helper = field_hit_count(&SettingsCategory::Window, "padding");
        assert_eq!(via_method, via_helper);
    }
}
