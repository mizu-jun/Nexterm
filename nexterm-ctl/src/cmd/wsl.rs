//! WSL ディストロインポートコマンド（Sprint 5-4 / E1）。
//!
//! Windows 上で `wsl.exe -l -q` を実行して利用可能な WSL ディストロを検出し、
//! それぞれを `[[profiles]]` セクションとして `config.toml` に追記する。

use anyhow::{Context, Result, bail};
use std::path::Path;

/// `nexterm-ctl wsl import-profiles` の実装
pub(crate) fn cmd_wsl_import_profiles(dry_run: bool) -> Result<()> {
    let distros = nexterm_config::wsl::detect_distros();

    if distros.is_empty() {
        if cfg!(windows) {
            bail!(
                "WSL ディストロが見つかりませんでした。\n\
                 wsl.exe がインストールされ、`wsl --list --quiet` でディストロが\n\
                 表示されることを確認してください。"
            );
        } else {
            bail!("WSL ディストロインポートは Windows でのみサポートされています。");
        }
    }

    println!("検出された WSL ディストロ ({} 件):", distros.len());
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
        println!("\n(--dry-run 指定のため、設定ファイルへの書き込みは行いません)");
        return Ok(());
    }

    // 設定ファイルパス
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let config_path = format!("{}/.config/nexterm/config.toml", home);

    write_profiles_to_config(&config_path, &distros)?;
    println!("\n設定ファイルに書き込みました: {}", config_path);
    println!("反映するには nexterm-server を再起動してください。");

    Ok(())
}

/// 既存の config.toml に Profile を追記する。
///
/// 同名 Profile が既に存在する場合はスキップする（重複追加防止）。
fn write_profiles_to_config(config_path: &str, profiles: &[nexterm_config::Profile]) -> Result<()> {
    let existing = if Path::new(config_path).exists() {
        std::fs::read_to_string(config_path)
            .with_context(|| format!("設定ファイルの読み込みに失敗しました: {}", config_path))?
    } else {
        // 親ディレクトリを作成
        if let Some(parent) = Path::new(config_path).parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("設定ディレクトリの作成に失敗しました: {}", parent.display())
            })?;
        }
        String::new()
    };

    // 既存 Profile 名を抽出（重複検出）
    let existing_names = extract_existing_profile_names(&existing);
    let mut added = 0;
    let mut skipped = 0;
    let mut to_append = String::new();

    for p in profiles {
        if existing_names.contains(&p.name) {
            println!("  スキップ（既存）: {}", p.name);
            skipped += 1;
            continue;
        }
        to_append.push_str(&profile_to_toml(p));
        added += 1;
    }

    if to_append.is_empty() {
        println!("\n追加対象なし（{} 件全てスキップ）", skipped);
        return Ok(());
    }

    let mut new_content = existing;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&to_append);

    std::fs::write(config_path, new_content)
        .with_context(|| format!("設定ファイルへの書き込みに失敗しました: {}", config_path))?;

    println!("\n追加: {} 件 / スキップ: {} 件", added, skipped);
    Ok(())
}

/// TOML テキストから既存 Profile 名（`name = "..."` 行）を抽出する。
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

/// `name = "Foo"` の形式から `Foo` を抽出する（簡易パーサ）
fn parse_toml_name_value(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with("name") {
        return None;
    }
    let after_eq = line.split_once('=')?.1.trim();
    // 前後の引用符を除去
    let unquoted = after_eq.trim_matches('"').trim_matches('\'');
    Some(unquoted.to_string())
}

/// Profile を TOML テーブル形式に変換する
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

/// TOML 文字列内のダブルクオート・バックスラッシュをエスケープする
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
