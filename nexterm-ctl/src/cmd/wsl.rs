//! WSL distro import command (Sprint 5-4 / E1).
//!
//! On Windows, this runs `wsl.exe -l -q` to detect installed WSL distros and
//! appends each one to `config.toml` as a `[[profiles]]` entry.

use anyhow::{Context, Result, bail};
use std::path::Path;

/// Implementation of `nexterm-ctl wsl import-profiles`.
pub(crate) fn cmd_wsl_import_profiles(dry_run: bool) -> Result<()> {
    let distros = nexterm_config::wsl::detect_distros();

    if distros.is_empty() {
        if cfg!(windows) {
            bail!(
                "no WSL distros were found.\n\
                 Verify that wsl.exe is installed and that `wsl --list --quiet`\n\
                 lists at least one distro."
            );
        } else {
            bail!("WSL distro import is only supported on Windows.");
        }
    }

    println!("detected WSL distros ({}):", distros.len());
    for p in &distros {
        let prog = p.shell.as_ref().map(|s| s.program.as_str()).unwrap_or("?");
        let args = p
            .shell
            .as_ref()
            .map(|s| s.args.join(" "))
            .unwrap_or_default();
        println!("  - {} ({} {})", p.name, prog, args);
    }

    if dry_run {
        println!("\n(--dry-run was specified; the config file is not modified)");
        return Ok(());
    }

    // Config file path.
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let config_path = format!("{}/.config/nexterm/config.toml", home);

    write_profiles_to_config(&config_path, &distros)?;
    println!("\nwrote to config file: {}", config_path);
    println!("restart nexterm-server for the change to take effect.");

    Ok(())
}

/// Append profiles to an existing `config.toml`.
///
/// Profiles whose name already exists are skipped (to prevent duplicates).
fn write_profiles_to_config(config_path: &str, profiles: &[nexterm_config::Profile]) -> Result<()> {
    let existing = if Path::new(config_path).exists() {
        std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read config file: {}", config_path))?
    } else {
        // Create the parent directory if necessary.
        if let Some(parent) = Path::new(config_path).parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory: {}", parent.display())
            })?;
        }
        String::new()
    };

    // Extract the existing profile names (used for duplicate detection).
    let existing_names = extract_existing_profile_names(&existing);
    let mut added = 0;
    let mut skipped = 0;
    let mut to_append = String::new();

    for p in profiles {
        if existing_names.contains(&p.name) {
            println!("  skipped (already present): {}", p.name);
            skipped += 1;
            continue;
        }
        to_append.push_str(&profile_to_toml(p));
        added += 1;
    }

    if to_append.is_empty() {
        println!("\nnothing to add (skipped all {} entries)", skipped);
        return Ok(());
    }

    let mut new_content = existing;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&to_append);

    std::fs::write(config_path, new_content)
        .with_context(|| format!("failed to write config file: {}", config_path))?;

    println!("\nadded: {}, skipped: {}", added, skipped);
    Ok(())
}

/// Extract existing profile names (lines like `name = "..."`) from the TOML text.
fn extract_existing_profile_names(content: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_profile_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[[profiles]]") {
            in_profile_section = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_profile_section = false;
            continue;
        }
        if in_profile_section && let Some(name) = parse_toml_name_value(trimmed) {
            names.push(name);
        }
    }
    names
}

/// Extract `Foo` from a line such as `name = "Foo"` (simple parser).
fn parse_toml_name_value(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with("name") {
        return None;
    }
    let after_eq = line.split_once('=')?.1.trim();
    // Strip the surrounding quotes.
    let unquoted = after_eq.trim_matches('"').trim_matches('\'');
    Some(unquoted.to_string())
}

/// Convert a Profile into a TOML table fragment.
fn profile_to_toml(p: &nexterm_config::Profile) -> String {
    let mut s = String::new();
    s.push_str("\n[[profiles]]\n");
    s.push_str(&format!("name = \"{}\"\n", escape_toml(&p.name)));
    if !p.icon.is_empty() {
        s.push_str(&format!("icon = \"{}\"\n", escape_toml(&p.icon)));
    }
    if let Some(shell) = &p.shell {
        s.push_str("\n[profiles.shell]\n");
        s.push_str(&format!("program = \"{}\"\n", escape_toml(&shell.program)));
        let args_str = shell
            .args
            .iter()
            .map(|a| format!("\"{}\"", escape_toml(a)))
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!("args = [{}]\n", args_str));
    }
    s
}

/// Escape double quotes and backslashes inside a TOML basic string.
fn escape_toml(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_existing_profile_names_finds_them() {
        let content = r##"
[shell]
program = "/bin/sh"

[[profiles]]
name = "alpha"
icon = "🚀"

[[profiles]]
name = "beta"

[colors]
foreground = "#fff"
"##;
        let names = extract_existing_profile_names(content);
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn extract_existing_profile_names_returns_empty_for_empty_content() {
        assert!(extract_existing_profile_names("").is_empty());
    }

    #[test]
    fn profile_to_toml_includes_shell_command() {
        let p = nexterm_config::Profile {
            name: "WSL: Ubuntu".to_string(),
            icon: "🐧".to_string(),
            shell: Some(nexterm_config::ShellConfig {
                program: "wsl.exe".to_string(),
                args: vec!["-d".to_string(), "Ubuntu".to_string()],
            }),
            ..nexterm_config::Profile::default()
        };
        let toml = profile_to_toml(&p);
        assert!(toml.contains("name = \"WSL: Ubuntu\""));
        assert!(toml.contains("icon = \"🐧\""));
        assert!(toml.contains("program = \"wsl.exe\""));
        assert!(toml.contains("args = [\"-d\", \"Ubuntu\"]"));
    }

    #[test]
    fn escape_toml_handles_quotes() {
        assert_eq!(escape_toml("a\"b"), "a\\\"b");
        assert_eq!(escape_toml("a\\b"), "a\\\\b");
        assert_eq!(escape_toml("plain"), "plain");
    }

    #[test]
    fn parse_toml_name_value_extracts_value() {
        assert_eq!(
            parse_toml_name_value(r#"name = "foo""#),
            Some("foo".to_string())
        );
        assert_eq!(
            parse_toml_name_value(r#"name="bar""#),
            Some("bar".to_string())
        );
        assert_eq!(parse_toml_name_value(r#"icon = "🚀""#), None);
    }
}
