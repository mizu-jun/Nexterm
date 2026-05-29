//! Theme import command + parsers (iTerm2 / Alacritty YAML / Base16 TOML).

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::cmd::util::remove_toml_section;

/// Color palette (internal representation used during import).
struct ImportedPalette {
    foreground: String,
    background: String,
    cursor: String,
    /// 16 ANSI colors (black, red, green, yellow, blue, magenta, cyan, white, bright×8).
    ansi: Vec<String>,
}

/// Import a theme file and write it into the config as a custom palette.
pub(crate) fn cmd_theme_import(path: String) -> Result<()> {
    let file_path = Path::new(&path);
    if !file_path.exists() {
        bail!("file not found: {}", path);
    }

    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("failed to read file: {}", path))?;

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let palette = match ext.as_str() {
        "itermcolors" => parse_iterm_colors(&content)?,
        "yaml" | "yml" => parse_alacritty_yaml(&content)?,
        "toml" => parse_base16_toml(&content)?,
        other => bail!(
            "unsupported file extension: .{} (supported: .itermcolors, .yaml, .yml, .toml)",
            other
        ),
    };

    // Config file path.
    let config_path = {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        format!("{}/.config/nexterm/config.toml", home)
    };

    write_custom_palette(&config_path, &palette)?;

    // Print the imported palette.
    println!("imported theme: {}", path);
    println!("  foreground: {}", palette.foreground);
    println!("  background: {}", palette.background);
    println!("  cursor:     {}", palette.cursor);
    println!("  ANSI 16 colors:");
    let names = [
        "black  ",
        "red    ",
        "green  ",
        "yellow ",
        "blue   ",
        "magenta",
        "cyan   ",
        "white  ",
        "br-black  ",
        "br-red    ",
        "br-green  ",
        "br-yellow ",
        "br-blue   ",
        "br-magenta",
        "br-cyan   ",
        "br-white  ",
    ];
    for (i, color) in palette.ansi.iter().enumerate() {
        let label = names.get(i).copied().unwrap_or("?");
        println!("    [{}] {}: {}", i, label, color);
    }
    println!("wrote to config file: {}", config_path);

    Ok(())
}

// ---------------------------------------------------------------------------
// iTerm2 .itermcolors parser
// ---------------------------------------------------------------------------

/// Convert an RGB triple of floats (0.0–1.0) into `#RRGGBB`.
fn rgb_float_to_hex(r: f64, g: f64, b: f64) -> String {
    let ri = (r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let gi = (g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let bi = (b.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", ri, gi, bi)
}

/// Extract a value from the XML block following a `<key>K</key>`.
fn iterm_extract_color(xml: &str, color_key: &str) -> Option<String> {
    // Look for keys such as "Ansi 0 Color".
    let search = format!("<key>{}</key>", color_key);
    let start = xml.find(&search)?;
    let after_key = &xml[start + search.len()..];
    // Then find the following <dict>.
    let dict_start = after_key.find("<dict>")?;
    let dict_content = &after_key[dict_start..];
    let dict_end = dict_content.find("</dict>")?;
    let dict = &dict_content[..dict_end + 7];

    let r = iterm_extract_component(dict, "Red Component")?;
    let g = iterm_extract_component(dict, "Green Component")?;
    let b = iterm_extract_component(dict, "Blue Component")?;
    Some(rgb_float_to_hex(r, g, b))
}

fn iterm_extract_component(dict: &str, component_key: &str) -> Option<f64> {
    let key_tag = format!("<key>{}</key>", component_key);
    let pos = dict.find(&key_tag)?;
    let after = &dict[pos + key_tag.len()..];
    // <real>...</real> or <integer>...</integer>.
    let val_str = if let Some(real_start) = after.find("<real>") {
        let inner = &after[real_start + 6..];
        let end = inner.find("</real>")?;
        &inner[..end]
    } else if let Some(int_start) = after.find("<integer>") {
        let inner = &after[int_start + 9..];
        let end = inner.find("</integer>")?;
        &inner[..end]
    } else {
        return None;
    };
    val_str.trim().parse::<f64>().ok()
}

fn parse_iterm_colors(content: &str) -> Result<ImportedPalette> {
    // Mapping for ANSI 0–15.
    let ansi_key_names = [
        "Ansi 0 Color",
        "Ansi 1 Color",
        "Ansi 2 Color",
        "Ansi 3 Color",
        "Ansi 4 Color",
        "Ansi 5 Color",
        "Ansi 6 Color",
        "Ansi 7 Color",
        "Ansi 8 Color",
        "Ansi 9 Color",
        "Ansi 10 Color",
        "Ansi 11 Color",
        "Ansi 12 Color",
        "Ansi 13 Color",
        "Ansi 14 Color",
        "Ansi 15 Color",
    ];

    let mut ansi = Vec::with_capacity(16);
    for key in &ansi_key_names {
        ansi.push(iterm_extract_color(content, key).unwrap_or_else(|| "#000000".to_string()));
    }

    let foreground =
        iterm_extract_color(content, "Foreground Color").unwrap_or_else(|| "#c5c8c6".to_string());
    let background =
        iterm_extract_color(content, "Background Color").unwrap_or_else(|| "#1d1f21".to_string());
    let cursor = iterm_extract_color(content, "Cursor Color").unwrap_or_else(|| foreground.clone());

    Ok(ImportedPalette {
        foreground,
        background,
        cursor,
        ansi,
    })
}

// ---------------------------------------------------------------------------
// Alacritty YAML parser
// ---------------------------------------------------------------------------

/// Extract `key: '#RRGGBB'` or `key: '#RGB'` from a line.
fn yaml_extract_hex(line: &str) -> Option<String> {
    // Find a line containing '#xxxxxx' or '#xxx'.
    let hash_pos = line.find('#')?;
    let after_hash = &line[hash_pos + 1..];
    // Take hex digits up to the next quote/whitespace.
    let hex: String = after_hash
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if hex.len() == 6 {
        Some(format!("#{}", hex.to_uppercase()))
    } else if hex.len() == 3 {
        // Expand shorthand notation.
        let r = &hex[0..1];
        let g = &hex[1..2];
        let b = &hex[2..3];
        Some(format!("#{}{}{}{}{}{}", r, r, g, g, b, b).to_uppercase())
    } else {
        None
    }
}

fn parse_alacritty_yaml(content: &str) -> Result<ImportedPalette> {
    let mut foreground = "#c5c8c6".to_string();
    let mut background = "#1d1f21".to_string();
    let cursor = "#c5c8c6".to_string();
    let mut ansi = vec!["#000000".to_string(); 16];

    // ANSI color name → index mapping.
    let normal_map: &[(&str, usize)] = &[
        ("black", 0),
        ("red", 1),
        ("green", 2),
        ("yellow", 3),
        ("blue", 4),
        ("magenta", 5),
        ("cyan", 6),
        ("white", 7),
    ];
    let bright_map: &[(&str, usize)] = &[
        ("black", 8),
        ("red", 9),
        ("green", 10),
        ("yellow", 11),
        ("blue", 12),
        ("magenta", 13),
        ("cyan", 14),
        ("white", 15),
    ];

    let mut in_primary = false;
    let mut in_normal = false;
    let mut in_bright = false;
    let mut in_cursor_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        // Section detection (ignore leading indent).
        if trimmed.starts_with("primary:") {
            in_primary = true;
            in_normal = false;
            in_bright = false;
            in_cursor_section = false;
            continue;
        }
        if trimmed.starts_with("normal:") {
            in_primary = false;
            in_normal = true;
            in_bright = false;
            in_cursor_section = false;
            continue;
        }
        if trimmed.starts_with("bright:") {
            in_primary = false;
            in_normal = false;
            in_bright = true;
            in_cursor_section = false;
            continue;
        }
        if trimmed.starts_with("cursor:") && !trimmed.contains('#') {
            in_primary = false;
            in_normal = false;
            in_bright = false;
            in_cursor_section = true;
            continue;
        }
        // The top-level `colors:` heading resets all sections.
        if trimmed == "colors:" {
            in_primary = false;
            in_normal = false;
            in_bright = false;
            in_cursor_section = false;
            continue;
        }

        if in_primary {
            if trimmed.starts_with("background:") {
                if let Some(hex) = yaml_extract_hex(trimmed) {
                    background = hex;
                }
            } else if trimmed.starts_with("foreground:")
                && let Some(hex) = yaml_extract_hex(trimmed)
            {
                foreground = hex;
            }
        }

        if in_normal {
            for (name, idx) in normal_map {
                if trimmed.starts_with(name)
                    && let Some(hex) = yaml_extract_hex(trimmed)
                {
                    ansi[*idx] = hex;
                }
            }
        }

        if in_bright {
            for (name, idx) in bright_map {
                if trimmed.starts_with(name)
                    && let Some(hex) = yaml_extract_hex(trimmed)
                {
                    ansi[*idx] = hex;
                }
            }
        }

        let _ = in_cursor_section;
    }

    Ok(ImportedPalette {
        foreground,
        background,
        cursor,
        ansi,
    })
}

// ---------------------------------------------------------------------------
// base16 TOML parser
// ---------------------------------------------------------------------------

fn parse_base16_toml(content: &str) -> Result<ImportedPalette> {
    // Extract base00–base0F.
    // base00 = background, base05 = foreground.
    // ANSI mapping: base16 → 16 colors.
    let base_keys = [
        "base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08",
        "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F",
    ];

    let mut bases: Vec<String> = vec!["#000000".to_string(); 16];

    for line in content.lines() {
        let trimmed = line.trim();
        // Accept either case.
        for (i, key) in base_keys.iter().enumerate() {
            let key_lower = key.to_lowercase();
            let trimmed_lower = trimmed.to_lowercase();
            if trimmed_lower.starts_with(&key_lower) && trimmed_lower.contains('=') {
                // Extract the value: `base00 = "282828"` or `base00 = "#282828"`.
                if let Some(eq_pos) = trimmed.find('=') {
                    let val = trimmed[eq_pos + 1..]
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'');
                    let hex = val.trim_start_matches('#');
                    if hex.len() == 6 {
                        bases[i] = format!("#{}", hex.to_uppercase());
                    }
                }
            }
        }
    }

    // base16 → ANSI 16-color mapping (standard base16 terminal mapping).
    // 0:black=base00, 1:red=base08, 2:green=base0B, 3:yellow=base0A,
    // 4:blue=base0D, 5:magenta=base0E, 6:cyan=base0C, 7:white=base05,
    // 8:br-black=base03, 9:br-red=base08, 10:br-green=base0B, 11:br-yellow=base0A,
    // 12:br-blue=base0D, 13:br-magenta=base0E, 14:br-cyan=base0C, 15:br-white=base07
    let ansi = vec![
        bases[0x00].clone(),
        bases[0x08].clone(),
        bases[0x0B].clone(),
        bases[0x0A].clone(),
        bases[0x0D].clone(),
        bases[0x0E].clone(),
        bases[0x0C].clone(),
        bases[0x05].clone(),
        bases[0x03].clone(),
        bases[0x08].clone(),
        bases[0x0B].clone(),
        bases[0x0A].clone(),
        bases[0x0D].clone(),
        bases[0x0E].clone(),
        bases[0x0C].clone(),
        bases[0x07].clone(),
    ];

    let background = bases[0x00].clone();
    let foreground = bases[0x05].clone();
    let cursor = bases[0x05].clone();

    Ok(ImportedPalette {
        foreground,
        background,
        cursor,
        ansi,
    })
}

// ---------------------------------------------------------------------------
// Config-file writer
// ---------------------------------------------------------------------------

/// Update the `[color-scheme.custom]` section of `~/.config/nexterm/config.toml`
/// (or create the file if missing).
fn write_custom_palette(config_path: &str, palette: &ImportedPalette) -> Result<()> {
    // Ensure the directory exists.
    if let Some(parent) = Path::new(config_path).parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory: {}", parent.display()))?;
    }

    // Read the existing file (or treat it as empty if missing).
    let existing = if Path::new(config_path).exists() {
        std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read config file: {}", config_path))?
    } else {
        String::new()
    };

    // Build the TOML fragment.
    let ansi_array = palette
        .ansi
        .iter()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(", ");

    let new_section = format!(
        "\n[color-scheme.custom]\nforeground = \"{}\"\nbackground = \"{}\"\ncursor = \"{}\"\nansi = [{}]\n",
        palette.foreground, palette.background, palette.cursor, ansi_array
    );

    // Remove any pre-existing [color-scheme.custom] section before appending.
    let cleaned = remove_toml_section(&existing, "color-scheme.custom");
    // Standalone [colors] / [color-scheme] sections are intentionally left in place.
    let final_content = format!("{}{}", cleaned.trim_end(), new_section);

    std::fs::write(config_path, final_content)
        .with_context(|| format!("failed to write config file: {}", config_path))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Sprint 5-4 / D4: theme gallery — list & apply built-in schemes
// ---------------------------------------------------------------------------

/// `nexterm-ctl theme list` — print the list of available built-in themes.
pub(crate) fn cmd_theme_list() -> Result<()> {
    println!("available built-in themes:");
    println!();
    for scheme in nexterm_config::BuiltinScheme::all() {
        println!("  {:12}  ({})", scheme.toml_name(), scheme.display_name());
    }
    println!();
    println!("to apply:        nexterm-ctl theme apply <name>");
    println!("to import custom: nexterm-ctl theme import <path>");
    Ok(())
}

/// `nexterm-ctl theme apply <name>` — write a built-in theme into `config.toml`.
pub(crate) fn cmd_theme_apply(name: String) -> Result<()> {
    let scheme = nexterm_config::BuiltinScheme::from_toml_name(&name).ok_or_else(|| {
        let names: Vec<&str> = nexterm_config::BuiltinScheme::all()
            .iter()
            .map(|s| s.toml_name())
            .collect();
        anyhow::anyhow!(
            "unknown theme name: '{}'\navailable: {}",
            name,
            names.join(", ")
        )
    })?;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let config_path = format!("{}/.config/nexterm/config.toml", home);

    apply_builtin_scheme_to_config(&config_path, scheme)?;

    println!(
        "applied theme: {} ({})",
        scheme.toml_name(),
        scheme.display_name()
    );
    println!("config file: {}", config_path);
    println!("restart nexterm-server for the change to take effect.");
    Ok(())
}

/// Replace the `[colors]` section in `config.toml` with `colors = "..."`.
fn apply_builtin_scheme_to_config(
    config_path: &str,
    scheme: nexterm_config::BuiltinScheme,
) -> Result<()> {
    let existing = if Path::new(config_path).exists() {
        std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read config file: {}", config_path))?
    } else {
        if let Some(parent) = Path::new(config_path).parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory: {}", parent.display())
            })?;
        }
        String::new()
    };

    // Remove any existing [colors] / [color-scheme] / [color-scheme.custom] sections.
    let mut cleaned = remove_toml_section(&existing, "colors");
    cleaned = remove_toml_section(&cleaned, "color-scheme");
    cleaned = remove_toml_section(&cleaned, "color-scheme.custom");
    // Also strip any single-line `colors = "..."` assignments.
    let cleaned = cleaned
        .lines()
        .filter(|line| !line.trim_start().starts_with("colors ="))
        .collect::<Vec<_>>()
        .join("\n");

    let new_line = format!("\ncolors = \"{}\"\n", scheme.toml_name());
    let final_content = format!("{}{}", cleaned.trim_end(), new_line);

    std::fs::write(config_path, final_content)
        .with_context(|| format!("failed to write config file: {}", config_path))?;
    Ok(())
}

#[cfg(test)]
mod theme_gallery_tests {
    use super::*;

    #[test]
    fn apply_builtin_scheme_writes_colors_line() {
        let tmp = std::env::temp_dir().join("nexterm-test-theme-apply.toml");
        let _ = std::fs::remove_file(&tmp);
        // Initial content (includes an existing `colors` line).
        std::fs::write(
            &tmp,
            "[shell]\nprogram = \"/bin/sh\"\n\ncolors = \"dark\"\n",
        )
        .unwrap();

        apply_builtin_scheme_to_config(tmp.to_str().unwrap(), nexterm_config::BuiltinScheme::Nord)
            .unwrap();

        let content = std::fs::read_to_string(&tmp).unwrap();
        assert!(content.contains("colors = \"nord\""));
        // The old `colors` line must have been removed.
        let dark_count = content.matches("colors = \"dark\"").count();
        assert_eq!(dark_count, 0);
        // The [shell] section must be preserved.
        assert!(content.contains("[shell]"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn apply_builtin_scheme_creates_file_when_absent() {
        let tmp = std::env::temp_dir().join("nexterm-test-theme-new.toml");
        let _ = std::fs::remove_file(&tmp);
        apply_builtin_scheme_to_config(
            tmp.to_str().unwrap(),
            nexterm_config::BuiltinScheme::Dracula,
        )
        .unwrap();
        let content = std::fs::read_to_string(&tmp).unwrap();
        assert!(content.contains("colors = \"dracula\""));
        let _ = std::fs::remove_file(&tmp);
    }
}
