//! Automatic WSL distro detection (Sprint 5-4 / E1).
//!
//! On Windows, run `wsl.exe -l -q` to enumerate the available distros and
//! convert each one into a `Profile`. On every other platform this always
//! returns an empty vector.
//!
//! # Output encoding
//!
//! The standard output of `wsl.exe -l -q` is **UTF-16LE** with CR/LF line
//! endings. This implementation converts UTF-16LE to UTF-8 with or without
//! a BOM.
//!
//! # Example
//!
//! ```no_run
//! use nexterm_config::wsl::detect_distros;
//! let distros = detect_distros();
//! for d in distros {
//!     println!("found: {}", d.name);
//! }
//! ```

use crate::schema::Profile;
#[cfg(windows)]
use crate::schema::ShellConfig;

/// Returns the detected WSL distros as a vector of `Profile`s.
///
/// On every platform other than Windows this returns an empty vector. It also
/// returns an empty vector when `wsl.exe` is not installed or detection fails.
pub fn detect_distros() -> Vec<Profile> {
    #[cfg(windows)]
    {
        detect_distros_windows()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn detect_distros_windows() -> Vec<Profile> {
    use std::process::Command;

    let output = match Command::new("wsl.exe").args(["-l", "-q"]).output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    // `wsl.exe -l -q` writes UTF-16LE to stdout.
    let names = parse_wsl_list_output(&output.stdout);

    names
        .into_iter()
        .map(|name| Profile {
            name: format!("WSL: {}", name),
            icon: "🐧".to_string(),
            shell: Some(ShellConfig {
                program: "wsl.exe".to_string(),
                args: vec!["-d".to_string(), name],
            }),
            ..Profile::default()
        })
        .collect()
}

/// Extracts the distro names from a UTF-16LE byte string.
///
/// Tolerates a BOM (`0xFF 0xFE`). Empty lines and null characters are dropped.
/// Exposed regardless of the host OS to make it easy to test.
pub fn parse_wsl_list_output(bytes: &[u8]) -> Vec<String> {
    // Skip the BOM.
    let payload = if bytes.starts_with(&[0xFF, 0xFE]) {
        &bytes[2..]
    } else {
        bytes
    };

    // Decode pairs of bytes as UTF-16LE.
    let utf16: Vec<u16> = payload
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();

    let text = String::from_utf16_lossy(&utf16);

    text.lines()
        .map(|line| line.trim().trim_end_matches('\0').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_input_returns_empty() {
        assert!(parse_wsl_list_output(&[]).is_empty());
    }

    #[test]
    fn parse_with_bom_strips_bom() {
        // Build `BOM + "Ubuntu\r\n"` as UTF-16LE.
        let mut bytes = vec![0xFF, 0xFE];
        for c in "Ubuntu\r\n".encode_utf16() {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let result = parse_wsl_list_output(&bytes);
        assert_eq!(result, vec!["Ubuntu"]);
    }

    #[test]
    fn parse_multiple_distros() {
        // Build `"Ubuntu\r\nDebian\r\nAlpine\r\n"` as UTF-16LE.
        let mut bytes = Vec::new();
        for c in "Ubuntu-22.04\r\nDebian\r\nAlpine\r\n".encode_utf16() {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let result = parse_wsl_list_output(&bytes);
        assert_eq!(result, vec!["Ubuntu-22.04", "Debian", "Alpine"]);
    }

    #[test]
    fn parse_skips_empty_lines() {
        let mut bytes = Vec::new();
        for c in "Ubuntu\r\n\r\nDebian\r\n".encode_utf16() {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let result = parse_wsl_list_output(&bytes);
        assert_eq!(result, vec!["Ubuntu", "Debian"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn detect_distros_returns_empty_on_non_windows() {
        assert!(detect_distros().is_empty());
    }
}
