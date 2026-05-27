//! Observability-related configuration (logging, status bar, etc.).

use serde::{Deserialize, Serialize};

/// Status-bar configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    /// Whether to show the status bar.
    pub enabled: bool,
    /// Widgets displayed on the left (a built-in keyword or a Lua expression).
    ///
    /// Built-in keywords: `"time"`, `"date"`, `"hostname"`, `"session"`,
    /// `"pane_id"`. Anything else is evaluated as a Lua expression.
    #[serde(default)]
    pub widgets: Vec<String>,
    /// Widgets displayed on the right.
    #[serde(default)]
    pub right_widgets: Vec<String>,
    /// Background color of the status bar (`RRGGBB`; uses the default when omitted).
    #[serde(default)]
    pub background_color: Option<String>,
    /// Widget separator (default: `"  "`).
    #[serde(default = "default_widget_separator")]
    pub separator: String,
}

fn default_widget_separator() -> String {
    "  ".to_string()
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            widgets: vec![],
            right_widgets: vec!["time".to_string()],
            background_color: None,
            separator: default_widget_separator(),
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
pub struct LogConfig {
    /// Enables automatic logging.
    #[serde(default)]
    pub auto_log: bool,
    /// Directory where logs are stored.
    pub log_dir: Option<String>,
    /// Prefixes each log line with a timestamp.
    #[serde(default)]
    pub timestamp: bool,
    /// Strips ANSI escape sequences from the log.
    #[serde(default)]
    pub strip_ansi: bool,
    /// Log file-name template.
    ///
    /// Available placeholders:
    ///   `{session}`  — session name
    ///   `{pane}`     — pane ID
    ///   `{datetime}` — start time (`YYYYMMDD_HHMMSS`)
    ///
    /// Example: `"{session}_{pane}_{datetime}.log"`.
    /// Default: `None` (directory + a fixed file name).
    pub file_name_template: Option<String>,
    /// Whether to also write raw PTY bytes to a binary file (`.bin`).
    #[serde(default)]
    pub binary_log: bool,
}
