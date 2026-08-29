//! Chrome icon codepoints (UI/UX v3 P4a).
//!
//! GENERATED FILE — do not edit. Regenerate with
//! `scripts/subset-icon-font.sh` after editing
//! `assets/fonts/icon-set.txt`.
//!
//! Source: microsoft/fluentui-system-icons @ fb047fb395f4 (MIT; see THIRD-PARTY-NOTICES.md).
//!
//! Every codepoint below lives in the Private Use Area, which overlaps the
//! Nerd Font range `tab_icons.rs` uses for terminal-content icons. They are
//! only ever safe to draw through [`FontRole::Icon`], which is what keeps the
//! two sets from resolving against each other.

// The per-icon constants are a generated catalogue. Which of them a call
// site draws is a review question, not a compiler one, so per-constant
// dead-code analysis says nothing useful here; `ALL_ICONS` keeps the set
// itself live and the font-coverage test keeps it honest.
#![allow(dead_code)]

/// Family name the bundled subset is registered under.
pub const ICON_FAMILY: &str = "Nexterm Icons";

/// The subsetted icon font, embedded in the binary.
pub const ICON_FONT: &[u8] = include_bytes!("../../assets/fonts/NextermIcons-Regular.ttf");

/// `ic_fluent_play_20_regular`
pub const SETTINGS_STARTUP: char = '\u{f605}';

/// `ic_fluent_text_font_20_regular`
pub const SETTINGS_FONT: char = '\u{f7e4}';

/// `ic_fluent_dark_theme_20_regular`
pub const SETTINGS_THEME: char = '\u{e452}';

/// `ic_fluent_window_20_regular`
pub const SETTINGS_WINDOW: char = '\u{f8b5}';

/// `ic_fluent_server_20_regular`
pub const SETTINGS_SSH: char = '\u{f769}';

/// `ic_fluent_keyboard_20_regular`
pub const SETTINGS_KEYBINDINGS: char = '\u{f4b8}';

/// `ic_fluent_person_20_regular`
pub const SETTINGS_PROFILES: char = '\u{f5bd}';

/// `ic_fluent_text_bullet_list_square_20_regular`
pub const SETTINGS_BLOCKS: char = '\u{ece2}';

/// `ic_fluent_shield_20_regular`
pub const SETTINGS_SECURITY: char = '\u{f6be}';

/// `ic_fluent_arrow_export_16_regular`
pub const TAB_TEAR_OUT: char = '\u{f027f}';

/// `ic_fluent_dismiss_16_regular`
pub const TAB_CLOSE: char = '\u{f368}';

/// `ic_fluent_chevron_down_16_regular`
pub const CHEVRON_DOWN: char = '\u{f2a2}';

/// `ic_fluent_chevron_left_16_regular`
pub const CHEVRON_LEFT: char = '\u{f2a9}';

/// `ic_fluent_chevron_right_16_regular`
pub const CHEVRON_RIGHT: char = '\u{f2af}';

/// `ic_fluent_subtract_16_regular`
pub const WINDOW_MINIMIZE: char = '\u{ebcf}';

/// `ic_fluent_maximize_16_regular`
pub const WINDOW_MAXIMIZE: char = '\u{f533}';

/// `ic_fluent_square_multiple_16_regular`
pub const WINDOW_RESTORE: char = '\u{eb95}';

/// `ic_fluent_dismiss_16_regular`
pub const WINDOW_CLOSE: char = '\u{f368}';

/// Every codepoint this module exposes, for coverage tests.
///
/// Order matches `assets/fonts/icon-set.txt`. Entries may repeat a
/// codepoint when two sites share one icon (close is both a tab button
/// and a caption button).
pub const ALL_ICONS: &[char] = &[
    SETTINGS_STARTUP,
    SETTINGS_FONT,
    SETTINGS_THEME,
    SETTINGS_WINDOW,
    SETTINGS_SSH,
    SETTINGS_KEYBINDINGS,
    SETTINGS_PROFILES,
    SETTINGS_BLOCKS,
    SETTINGS_SECURITY,
    TAB_TEAR_OUT,
    TAB_CLOSE,
    CHEVRON_DOWN,
    CHEVRON_LEFT,
    CHEVRON_RIGHT,
    WINDOW_MINIMIZE,
    WINDOW_MAXIMIZE,
    WINDOW_RESTORE,
    WINDOW_CLOSE,
];
