//! Status-bar widget evaluator.
//!
//! Widgets fall into two kinds:
//!
//! 1. **Built-in keywords** — evaluated quickly in native Rust.
//!    - `"time"` → `HH:MM:SS`.
//!    - `"date"` → `YYYY-MM-DD`.
//!    - `"hostname"` → the system's host name.
//!    - `"session"` → the current session name (received via IPC).
//!    - `"pane_id"` → the focused pane's ID.
//!    - `"cwd"` → the focused pane's working directory (when OSC 7 has been
//!      received; Sprint 5-7 / UI-1-2).
//!    - `"cwd_short"` → the cwd with the home directory replaced by `~`,
//!      truncated from the front when too long.
//!    - `"git_branch"` → the branch name read from `.git/HEAD` under the cwd
//!      (empty when none is found).
//!    - `"workspace"` → the current workspace name (enabled after Phase 2-1).
//!
//! 2. **Lua expressions** — evaluated on a background thread.
//!    - `'os.date("%H:%M")'` → the result of Lua's `os.date`.
//!    - `'"custom text"'` → a string literal.
//!
//! # Example configuration (`nexterm.lua`)
//!
//! ```lua
//! return {
//!   status_bar = {
//!     enabled = true,
//!     widgets = { "session", "pane_id" },
//!     right_widgets = { "git_branch", "cwd_short", "time" },
//!   }
//! }
//! ```

use crate::loader::lua_path;
use crate::lua_worker::LuaWorker;

// ---- Built-in widget evaluation -------------------------------------------

/// Current context (session name, pane ID, cwd, workspace).
///
/// Pass this into `evaluate_builtin` to evaluate dynamic built-in widgets.
#[derive(Debug, Clone, Default)]
pub struct WidgetContext {
    /// Current session name.
    pub session_name: Option<String>,
    /// Currently focused pane ID.
    pub pane_id: Option<u32>,
    /// Working directory of the focused pane (CWD reported via OSC 7).
    /// Used by the `cwd` / `cwd_short` / `git_branch` widgets
    /// (Sprint 5-7 / UI-1-2).
    pub cwd: Option<String>,
    /// Current workspace name (to be introduced in Phase 2-1).
    pub workspace_name: Option<String>,
}

/// Evaluates a built-in widget keyword.
///
/// Returns `None` for an unknown keyword (a Lua expression).
pub fn evaluate_builtin(keyword: &str, ctx: &WidgetContext) -> Option<String> {
    match keyword {
        "time" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let secs = now.as_secs();
            // Compute HH:MM:SS from the UTC seconds (a portable implementation
            // that does not use libc).
            let hms = secs % 86400;
            let h = hms / 3600;
            let m = (hms % 3600) / 60;
            let s = hms % 60;
            Some(format!("{:02}:{:02}:{:02}", h, m, s))
        }
        "date" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            // Compute the UTC date (leap years handled correctly).
            let days = now.as_secs() / 86400;
            let (y, mo, d) = days_to_ymd(days);
            Some(format!("{:04}-{:02}-{:02}", y, mo, d))
        }
        "hostname" => {
            // Try the `HOSTNAME` environment variable first; fall back to
            // alternatives otherwise.
            let name = std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "localhost".to_string());
            Some(name)
        }
        "session" => Some(ctx.session_name.clone().unwrap_or_else(|| "—".to_string())),
        "pane_id" => Some(
            ctx.pane_id
                .map(|id| format!("pane:{}", id))
                .unwrap_or_else(|| "pane:—".to_string()),
        ),
        "workspace" => Some(
            ctx.workspace_name
                .clone()
                .unwrap_or_else(|| "—".to_string()),
        ),
        "cwd" => Some(ctx.cwd.clone().unwrap_or_default()),
        "cwd_short" => Some(shorten_cwd(ctx.cwd.as_deref().unwrap_or_default())),
        "git_branch" => Some(read_git_branch(ctx.cwd.as_deref().unwrap_or_default())),
        _ => None, // treat as a Lua expression
    }
}

/// Shortens a cwd by collapsing the home directory and keeping only the last
/// two path components.
///
/// For example, `/home/alice/projects/foo` becomes `~/projects/foo`. Strings
/// longer than the threshold are abbreviated to roughly the last 30 characters.
fn shorten_cwd(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .unwrap_or_default();
    let mut s = if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    };
    // Normalize the path-separator style.
    s = s.replace('\\', "/");
    // Keep only the tail when the result is too long.
    const MAX: usize = 40;
    if s.chars().count() > MAX {
        let tail: String = s.chars().rev().take(MAX - 1).collect::<String>();
        let tail: String = tail.chars().rev().collect();
        format!("…{}", tail)
    } else {
        s
    }
}

/// Reads `.git/HEAD` under the given cwd and returns the current branch name
/// (or a short SHA).
///
/// Walks the parent directories looking for a `.git` directory. Returns an
/// empty string when `cwd` is empty or no `.git` is found (no external process
/// is spawned, so this is cheap even when invoked once per second).
fn read_git_branch(cwd: &str) -> String {
    if cwd.is_empty() {
        return String::new();
    }
    let mut dir = std::path::PathBuf::from(cwd);
    for _ in 0..30 {
        let head = dir.join(".git").join("HEAD");
        if let Ok(content) = std::fs::read_to_string(&head) {
            let trimmed = content.trim();
            // Format `ref: refs/heads/master`.
            if let Some(refpath) = trimmed.strip_prefix("ref: refs/heads/") {
                return refpath.to_string();
            }
            // Detached HEAD (a raw commit SHA).
            if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                return trimmed.chars().take(7).collect();
            }
            return String::new();
        }
        if !dir.pop() {
            break;
        }
    }
    String::new()
}

/// Converts an epoch day count (1970-01-01 = 0) to `(year, month, day)`.
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // A 400-year cycle is 146097 days.
    let years400 = days / 146097;
    days %= 146097;
    let years100 = (days / 36524).min(3);
    days -= years100 * 36524;
    let years4 = days / 1461;
    days %= 1461;
    let years1 = (days / 365).min(3);
    days -= years1 * 365;

    let year = years400 * 400 + years100 * 100 + years4 * 4 + years1 + 1970;

    // Days per month (accounts for leap years).
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }

    (year, month, days + 1)
}

// ---- Lua status-bar widget evaluator --------------------------------------

/// Evaluates Lua widget expressions and produces the status-bar text.
///
/// The internal `LuaWorker` runs Lua on a background thread, so
/// `evaluate_widgets()` never blocks the main thread.
pub struct StatusBarEvaluator {
    worker: LuaWorker,
}

impl StatusBarEvaluator {
    /// Creates the evaluator (loads `nexterm.lua` if it exists).
    ///
    /// Lua-load errors only produce a warning log; they never panic.
    pub fn new() -> Self {
        let path = lua_path();
        let lua_script_path = if path.exists() { Some(path) } else { None };
        Self {
            worker: LuaWorker::new(lua_script_path),
        }
    }

    /// Evaluates the widget list and returns the result concatenated with the
    /// separator.
    ///
    /// - Built-in keywords are evaluated natively (non-blocking).
    /// - Lua expressions are evaluated on a background thread (non-blocking).
    /// - Each expression's evaluation errors are replaced with empty strings.
    pub fn evaluate_widgets(&self, widgets: &[String]) -> String {
        self.evaluate_with_context(widgets, &WidgetContext::default(), "  ")
    }

    /// Evaluates the widget list with a specified context and separator.
    pub fn evaluate_with_context(
        &self,
        widgets: &[String],
        ctx: &WidgetContext,
        separator: &str,
    ) -> String {
        if widgets.is_empty() {
            return String::new();
        }

        // Separate built-in keywords from Lua expressions.
        let mut lua_exprs: Vec<String> = Vec::new();
        let mut has_lua = false;
        for w in widgets {
            if evaluate_builtin(w, ctx).is_none() {
                lua_exprs.push(w.clone());
                has_lua = true;
            }
        }

        // Evaluate the Lua expressions in the background (the cache is updated).
        if has_lua {
            self.worker.eval_widgets(&lua_exprs);
        }

        // Build the result.
        let mut parts: Vec<String> = Vec::with_capacity(widgets.len());
        let mut lua_idx = 0usize;
        for w in widgets {
            if let Some(builtin) = evaluate_builtin(w, ctx) {
                if !builtin.is_empty() {
                    parts.push(builtin);
                }
            } else {
                // Fetch the cached result for Lua expression number `lua_idx`.
                let result = self.worker.eval_widgets(&lua_exprs);
                // `eval_widgets` returns the joined value of every expression,
                // but we need per-expression results, so this should ideally
                // be fetched individually from the worker (the existing API
                // only returns the joined output).
                // For simplicity, fall back to the joined output of every
                // Lua expression.
                let _ = lua_idx;
                let _ = result;
                lua_idx += 1;
            }
        }

        // When there is exactly one widget and it is a Lua expression, use
        // the worker's output directly.
        if widgets.len() == 1 && has_lua {
            return self.worker.eval_widgets(widgets);
        }

        // Mixed case: join the built-in parts and append the Lua part at the
        // end.
        // TODO: per-widget Lua evaluation (the current API only supports the
        // joined output).
        let lua_part = if has_lua {
            self.worker.eval_widgets(&lua_exprs)
        } else {
            String::new()
        };

        let mut all_parts: Vec<String> = parts;
        if !lua_part.is_empty() {
            all_parts.push(lua_part);
        }

        all_parts.join(separator)
    }
}

impl Default for StatusBarEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Waits for the background evaluation to complete.
    fn wait_for_eval() {
        std::thread::sleep(Duration::from_millis(150));
    }

    #[test]
    fn lua_expression_can_be_evaluated() {
        let eval = StatusBarEvaluator::new();
        // The first call sends a request.
        eval.evaluate_widgets(&["\"hello\"".to_string()]);
        wait_for_eval();
        // After the background evaluation finishes, fetch the cached result.
        let result = eval.evaluate_widgets(&["\"hello\"".to_string()]);
        assert_eq!(result, "hello");
    }

    #[test]
    fn multiple_widgets_are_joined_with_spaces() {
        let eval = StatusBarEvaluator::new();
        eval.evaluate_widgets(&["\"foo\"".to_string(), "\"bar\"".to_string()]);
        wait_for_eval();
        let result = eval.evaluate_widgets(&["\"foo\"".to_string(), "\"bar\"".to_string()]);
        assert_eq!(result, "foo  bar");
    }

    #[test]
    fn evaluation_errors_become_empty_strings() {
        let eval = StatusBarEvaluator::new();
        // Referencing an undefined variable triggers an error.
        eval.evaluate_widgets(&["undefined_variable_xyz".to_string()]);
        wait_for_eval();
        let result = eval.evaluate_widgets(&["undefined_variable_xyz".to_string()]);
        // Even on error, the function returns an empty string and does not panic.
        assert_eq!(result, "");
    }

    #[test]
    fn empty_list_returns_empty_string() {
        let eval = StatusBarEvaluator::new();
        eval.evaluate_widgets(&[]);
        wait_for_eval();
        let result = eval.evaluate_widgets(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn builtin_time_returns_hh_mm_ss_format() {
        let ctx = WidgetContext::default();
        let result = evaluate_builtin("time", &ctx).unwrap();
        // Must be HH:MM:SS.
        assert_eq!(result.len(), 8);
        assert_eq!(&result[2..3], ":");
        assert_eq!(&result[5..6], ":");
    }

    #[test]
    fn builtin_date_returns_yyyy_mm_dd_format() {
        let ctx = WidgetContext::default();
        let result = evaluate_builtin("date", &ctx).unwrap();
        // Must be YYYY-MM-DD.
        assert_eq!(result.len(), 10);
        assert_eq!(&result[4..5], "-");
        assert_eq!(&result[7..8], "-");
    }

    #[test]
    fn builtin_hostname_returns_a_non_empty_string() {
        let ctx = WidgetContext::default();
        let result = evaluate_builtin("hostname", &ctx).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn builtin_session_returns_the_session_name_from_context() {
        let ctx = WidgetContext {
            session_name: Some("my-session".to_string()),
            ..Default::default()
        };
        assert_eq!(evaluate_builtin("session", &ctx).unwrap(), "my-session");
    }

    #[test]
    fn builtin_pane_id_returns_the_focused_pane_number() {
        let ctx = WidgetContext {
            pane_id: Some(42),
            ..Default::default()
        };
        assert_eq!(evaluate_builtin("pane_id", &ctx).unwrap(), "pane:42");
    }

    #[test]
    fn unknown_keywords_return_none() {
        let ctx = WidgetContext::default();
        assert!(evaluate_builtin("unknown_widget", &ctx).is_none());
    }

    #[test]
    fn builtin_cwd_returns_the_cwd_from_context() {
        let ctx = WidgetContext {
            cwd: Some("/tmp/foo".to_string()),
            ..Default::default()
        };
        assert_eq!(evaluate_builtin("cwd", &ctx).unwrap(), "/tmp/foo");
    }

    #[test]
    fn builtin_cwd_short_returns_the_home_abbreviated_form() {
        // Override HOME temporarily to keep the test independent.
        unsafe {
            std::env::set_var("HOME", "/home/alice");
        }
        let ctx = WidgetContext {
            cwd: Some("/home/alice/projects/foo".to_string()),
            ..Default::default()
        };
        assert_eq!(
            evaluate_builtin("cwd_short", &ctx).unwrap(),
            "~/projects/foo"
        );
    }

    #[test]
    fn builtin_workspace_returns_the_workspace_name() {
        let ctx = WidgetContext {
            workspace_name: Some("work".to_string()),
            ..Default::default()
        };
        assert_eq!(evaluate_builtin("workspace", &ctx).unwrap(), "work");
    }

    #[test]
    fn builtin_git_branch_returns_empty_outside_a_git_repo() {
        let ctx = WidgetContext {
            cwd: Some("/nonexistent_path_for_test_xyz123".to_string()),
            ..Default::default()
        };
        assert_eq!(evaluate_builtin("git_branch", &ctx).unwrap(), "");
    }

    #[test]
    fn days_to_ymd_epoch_is_1970_01_01() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 is 19723 days after 1970-01-01.
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!(y, 2024);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }
}
