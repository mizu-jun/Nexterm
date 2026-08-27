//! Keeps `docs/CONFIGURATION.md` honest about what `Config` actually accepts.
//!
//! The parser does not reject unknown keys, so a documented key that does not
//! exist produces no error at runtime — it simply does nothing. That is how the
//! document accumulated ten keys that were never in the code (a whole
//! `[terminal]` table among them), each of which read as a working setting.
//! Nothing but a test can catch that class of drift.

use nexterm_config::Config;

const DOC: &str = include_str!("../../docs/CONFIGURATION.md");

/// Pull the fenced TOML block that follows the "Complete nexterm.toml Example"
/// heading. Returns the block body without its fences.
fn complete_example() -> String {
    let after_heading = DOC
        .split_once("## Complete nexterm.toml Example")
        .expect("the doc must keep its complete-example heading")
        .1;
    let body = after_heading
        .split_once("```toml")
        .expect("the complete-example section must contain a toml block")
        .1;
    body.split_once("```")
        .expect("the complete-example toml block must be closed")
        .0
        .to_string()
}

/// The documented example must parse. A stale key would not fail this on its
/// own (unknown keys are ignored), but malformed TOML or a type that changed
/// underneath the document would.
#[test]
fn complete_example_parses() {
    let example = complete_example();
    let parsed: Config = toml::from_str(&example)
        .expect("the complete example in docs/CONFIGURATION.md must parse as Config");

    // Spot-check keys across the example's range, so a renamed field surfaces
    // here instead of being silently ignored. `scrollback_lines` is set away
    // from its 50_000 default, which makes it a real signal that a top-level
    // scalar still lands.
    assert_eq!(
        parsed.scrollback_lines, 10_000,
        "top-level scalars must still apply"
    );
    assert_eq!(parsed.gpu.fps_limit, 60);
    assert!(
        parsed.log.file_name_template.is_some(),
        "the log section must use `file_name_template`, not the removed `log_template`"
    );
    assert!(parsed.cursor.blink_enabled);
    assert_eq!(parsed.scrolling.multiplier, 3.0);
    assert_eq!(
        parsed.security.plugin_read,
        nexterm_config::ConsentPolicy::Deny,
        "the documented plugin_read example must keep the fail-safe default"
    );
}

/// Every top-level table/key in the documented example must be a real `Config`
/// field. This is the check that would have caught `[terminal]`.
#[test]
fn complete_example_uses_only_real_config_keys() {
    let example = complete_example();
    let documented: toml::Table =
        toml::from_str(&example).expect("the complete example must be valid TOML");

    let default_as_value = toml::Value::try_from(Config::default())
        .expect("Config::default() must be serializable to TOML");
    let known = default_as_value
        .as_table()
        .expect("a serialized Config is a TOML table");

    for key in documented.keys() {
        assert!(
            known.contains_key(key),
            "`{key}` is documented in the complete example but is not a Config field — \
             the parser ignores it silently, so the documentation would be describing \
             a setting that does nothing"
        );
    }
}

/// Keys that lived in the document for a long time without ever existing in
/// the code, plus fields that did exist but were removed. They are named here
/// so that copying an old revision back in fails loudly instead of quietly
/// documenting a no-op again.
///
/// The prose that explains their removal is allowed to mention them; only
/// key-like usage (a table row, or an assignment inside an example) counts.
#[test]
fn removed_phantom_keys_do_not_return() {
    const PHANTOM: &[&str] = &[
        // The former `[terminal]` table.
        "alt_screen_buffer",
        "dec_mode_47_1047_1049",
        "osc_window_title",
        "osc_notifications",
        "cjk_width",
        "ime_support",
        // Logging.
        "max_log_size",
        "log_template",
        // SSH.
        "socks5_proxy",
        "local_forwards",
        // Window: existed as a field but never had a reader; removed in P2c
        // and replaced by `backdrop`.
        "macos_window_background_blur",
    ];

    for (lineno, line) in DOC.lines().enumerate() {
        let trimmed = line.trim_start();
        // Explanatory notes are block quotes; they may name the old keys.
        if trimmed.starts_with('>') {
            continue;
        }
        let is_table_row = trimmed.starts_with("| `");
        let is_assignment = trimmed.contains(" = ") || trimmed.contains('=');
        if !is_table_row && !is_assignment {
            continue;
        }
        for phantom in PHANTOM {
            let as_row = format!("| `{phantom}`");
            let as_assignment = format!("{phantom} =");
            assert!(
                !trimmed.starts_with(&as_row) && !trimmed.starts_with(&as_assignment),
                "docs/CONFIGURATION.md:{} documents `{phantom}`, which is not a Config \
                 field. Unknown keys are ignored silently, so this would read as a \
                 working setting.",
                lineno + 1
            );
        }
    }
}
