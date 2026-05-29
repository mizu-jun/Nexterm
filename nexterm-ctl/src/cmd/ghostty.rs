//! Ghostty config import command + parser.

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::cmd::util::remove_toml_section;

/// Read a Ghostty config file and convert it to nexterm's `config.toml`.
pub(crate) fn cmd_import_ghostty(path: Option<String>, output: Option<String>) -> Result<()> {
    // Default input path: ~/.config/ghostty/config
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());

    let input_path = path.unwrap_or_else(|| format!("{}/.config/ghostty/config", home));

    if !Path::new(&input_path).exists() {
        bail!(
            "Ghostty config file not found: {}\n\
             Pass the path explicitly: nexterm-ctl import-ghostty <path>",
            input_path
        );
    }

    let content = std::fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read Ghostty config file: {}", input_path))?;

    let converted = parse_ghostty_config(&content)?;

    // Default output path: ~/.config/nexterm/config.toml
    let output_path = output.unwrap_or_else(|| format!("{}/.config/nexterm/config.toml", home));

    // Create the output directory.
    if let Some(parent) = Path::new(&output_path).parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    // Merge the converted Ghostty settings into any existing `config.toml`.
    let existing = if Path::new(&output_path).exists() {
        std::fs::read_to_string(&output_path)
            .with_context(|| format!("failed to read existing config file: {}", output_path))?
    } else {
        String::new()
    };

    let merged = merge_ghostty_config(&existing, &converted);

    std::fs::write(&output_path, &merged)
        .with_context(|| format!("failed to write config file: {}", output_path))?;

    println!("imported Ghostty config");
    println!("  input:  {}", input_path);
    println!("  output: {}", output_path);
    if !converted.notes.is_empty() {
        println!("\nconversion notes (require manual review):");
        for note in &converted.notes {
            println!("  ! {}", note);
        }
    }

    Ok(())
}

/// Conversion result for a Ghostty config.
struct GhosttyConverted {
    /// TOML fragment for the `[font]` section.
    font_toml: Option<String>,
    /// TOML fragment for the `[color-scheme.custom]` section (when a palette is set).
    palette_toml: Option<String>,
    /// TOML fragment for the `[window]` section.
    window_toml: Option<String>,
    /// Items that require manual review.
    notes: Vec<String>,
}

/// Parse a Ghostty config file and convert it to a nexterm-compatible config.
fn parse_ghostty_config(content: &str) -> Result<GhosttyConverted> {
    // Ghostty's format is `key = value` (TOML-like but custom).
    let mut font_family: Option<String> = None;
    let mut font_size: Option<f32> = None;
    let mut background: Option<String> = None;
    let mut foreground: Option<String> = None;
    let mut cursor_color: Option<String> = None;
    let mut background_opacity: Option<f32> = None;
    let mut ansi: Vec<Option<String>> = vec![None; 16];
    let mut notes = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        // Skip comments and blank lines.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Split on `=`.
        let Some(eq_pos) = trimmed.find('=') else {
            continue;
        };
        let key = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..]
            .trim()
            .trim_matches('"')
            .trim_matches('\'');

        match key {
            "font-family" => font_family = Some(value.to_string()),
            "font-size" => font_size = value.parse::<f32>().ok(),
            "background" => background = Some(normalize_color(value)),
            "foreground" => foreground = Some(normalize_color(value)),
            "cursor-color" => cursor_color = Some(normalize_color(value)),
            "background-opacity" => background_opacity = value.parse::<f32>().ok(),
            // ANSI palette: `palette = N=#RRGGBB`
            "palette" => {
                if let Some((idx_str, color)) = value.split_once('=')
                    && let Ok(idx) = idx_str.trim().parse::<usize>()
                    && idx < 16
                {
                    ansi[idx] = Some(normalize_color(color.trim()));
                }
            }
            // Unsupported keys: emit a note.
            "theme" => notes.push(format!(
                "theme = \"{}\" must be converted manually to a nexterm color-scheme",
                value
            )),
            "keybind" => notes.push(format!(
                "keybind = {} must be mapped manually into nexterm's [keybindings]",
                value
            )),
            "shell-integration" | "shell-integration-features" => {
                notes.push(format!("{} is integrated automatically by nexterm", key))
            }
            "window-decoration" => {
                // Ghostty's `window-decoration` → nexterm's `window.decorations`.
                // "false" = none, "true"/"client"/"server" = full.
            }
            _ => {
                // Only the more impactful keys are noted; minor keys are silently ignored.
                if !matches!(
                    key,
                    "cursor-style"
                        | "cursor-style-blink"
                        | "scrollback-limit"
                        | "clipboard-read"
                        | "clipboard-write"
                        | "mouse-hide-while-typing"
                ) && !key.starts_with("gtk-")
                    && !key.starts_with("macos-")
                    && !key.starts_with("linux-")
                    && !key.starts_with("windows-")
                {
                    // Unsupported keys are ignored (warning on every one would be noisy).
                }
            }
        }
    }

    // Build the `[font]` section.
    let font_toml = if font_family.is_some() || font_size.is_some() {
        let mut s = String::from("[font]\n");
        if let Some(family) = &font_family {
            s.push_str(&format!("family = \"{}\"\n", family));
        }
        if let Some(size) = font_size {
            s.push_str(&format!("size = {}\n", size));
        }
        Some(s)
    } else {
        None
    };

    // Build the `[color-scheme.custom]` section.
    let palette_toml = if background.is_some()
        || foreground.is_some()
        || ansi.iter().any(|a| a.is_some())
    {
        let bg = background.clone().unwrap_or_else(|| "#1d1f21".to_string());
        let fg = foreground.clone().unwrap_or_else(|| "#c5c8c6".to_string());
        let cur = cursor_color.clone().unwrap_or_else(|| fg.clone());
        let ansi_arr: Vec<String> = ansi
            .iter()
            .enumerate()
            .map(|(i, a)| {
                a.clone().unwrap_or_else(|| {
                    // Default ANSI color.
                    DEFAULT_ANSI_COLORS[i % 16].to_string()
                })
            })
            .collect();
        let ansi_str = ansi_arr
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "[color-scheme.custom]\nforeground = \"{}\"\nbackground = \"{}\"\ncursor = \"{}\"\nansi = [{}]\n",
            fg, bg, cur, ansi_str
        ))
    } else {
        None
    };

    // Build the `[window]` section.
    let window_toml = background_opacity
        .map(|opacity| format!("[window]\nbackground_opacity = {:.2}\n", opacity));

    Ok(GhosttyConverted {
        font_toml,
        palette_toml,
        window_toml,
        notes,
    })
}

/// Normalize a color string ("RRGGBB" → "#RRGGBB"; pass through if already prefixed with "#").
fn normalize_color(s: &str) -> String {
    let s = s.trim_matches('"').trim_matches('\'');
    if s.starts_with('#') {
        s.to_uppercase()
    } else {
        format!("#{}", s.to_uppercase())
    }
}

/// Default ANSI 16-color palette (used as a fallback).
const DEFAULT_ANSI_COLORS: &[&str] = &[
    "#2E3440", "#BF616A", "#A3BE8C", "#EBCB8B", "#81A1C1", "#B48EAD", "#88C0D0", "#E5E9F0",
    "#4C566A", "#BF616A", "#A3BE8C", "#EBCB8B", "#81A1C1", "#B48EAD", "#8FBCBB", "#ECEFF4",
];

/// Merge the converted Ghostty settings into an existing `config.toml`.
///
/// If a section already exists in the existing file it is replaced; otherwise the
/// section is appended at the end.
fn merge_ghostty_config(existing: &str, converted: &GhosttyConverted) -> String {
    let mut result = existing.to_string();

    if let Some(font) = &converted.font_toml {
        result = remove_toml_section(&result, "font");
        result = format!("{}\n{}", result.trim_end(), font);
    }

    if let Some(palette) = &converted.palette_toml {
        result = remove_toml_section(&result, "color-scheme.custom");
        result = format!("{}\n{}", result.trim_end(), palette);
    }

    if let Some(window) = &converted.window_toml {
        result = remove_toml_section(&result, "window");
        result = format!("{}\n{}", result.trim_end(), window);
    }

    result
}
