//! Configuration for interactive input — shell, serial ports, macros, and key bindings.

use serde::{Deserialize, Serialize};

/// Shell configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    /// Path to the shell executable.
    pub program: String,
    /// Arguments passed to the shell.
    pub args: Vec<String>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        #[cfg(windows)]
        {
            // Scan `%ProgramFiles%\PowerShell\*` dynamically and pick the
            // latest version. Sprint 5-12 Phase 2: the previous implementation
            // compared `PathBuf`s with `>`, which is lexicographic. That made
            // "7" > "10", so PowerShell 7 was incorrectly judged "newer" than
            // PowerShell 10. The fix parses the directory name as `u32` and
            // compares numerically.
            let prog_files =
                std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
            let ps_root = std::path::Path::new(&prog_files).join("PowerShell");
            if let Ok(entries) = std::fs::read_dir(&ps_root) {
                let mut pwsh: Option<std::path::PathBuf> = None;
                for e in entries.flatten() {
                    let candidate = e.path().join("pwsh.exe");
                    if candidate.exists()
                        && pwsh.as_ref().is_none_or(|p| {
                            pwsh_version_number(&candidate) > pwsh_version_number(p)
                        })
                    {
                        pwsh = Some(candidate);
                    }
                }
                if let Some(path) = pwsh {
                    return Self {
                        program: path.to_string_lossy().into_owned(),
                        args: vec!["-NoLogo".to_string()],
                    };
                }
            }
            // Fallback to PowerShell 5.
            let ps5 = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
            if std::path::Path::new(ps5).exists() {
                return Self {
                    program: ps5.to_string(),
                    args: vec!["-NoLogo".to_string()],
                };
            }
            // Final fallback: cmd.exe.
            Self {
                program: "C:\\Windows\\System32\\cmd.exe".to_string(),
                args: vec![],
            }
        }

        #[cfg(not(windows))]
        Self {
            program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            args: vec![],
        }
    }
}

/// Extracts the installation directory name (the version number) from a full
/// path to `pwsh.exe` as a number.
///
/// For example: `C:\Program Files\PowerShell\7\pwsh.exe` → `7`,
/// `C:\Program Files\PowerShell\10\pwsh.exe` → `10`.
/// Returns `0` when the parse fails (e.g. a preview build named `7-preview`),
/// so any numeric version takes precedence when one is available.
///
/// Sprint 5-12 Phase 2: introduced to work around the lexicographic `PathBuf`
/// comparison bug (`"7" > "10"`).
#[cfg(windows)]
pub(crate) fn pwsh_version_number(pwsh_exe_path: &std::path::Path) -> u32 {
    pwsh_exe_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Serial-port configuration (a connection preset).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SerialPortConfig {
    /// Display name.
    pub name: String,
    /// Device path (e.g. `/dev/ttyUSB0`, `COM3`).
    pub port: String,
    /// Baud rate.
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    /// Data bits: 5, 6, 7, or 8.
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    /// Stop bits: 1 or 2.
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    /// Parity: `"none"`, `"odd"`, or `"even"`.
    #[serde(default = "default_parity")]
    pub parity: String,
}

fn default_baud_rate() -> u32 {
    115200
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_parity() -> String {
    "none".to_string()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Sprint 5-12 Phase 2: regression test for the lexicographic-order bug.
    /// The previous implementation compared `PathBuf`s with `>`, which made
    /// `"7" > "10"` so PowerShell 7 was chosen even when PowerShell 10 was
    /// installed.
    #[test]
    fn pwsh_version_number_extracts_numeric_directory_name() {
        let p7 = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
        let p10 = PathBuf::from(r"C:\Program Files\PowerShell\10\pwsh.exe");
        assert_eq!(pwsh_version_number(&p7), 7);
        assert_eq!(pwsh_version_number(&p10), 10);
        // Numerically, v10 > v7 (regression test for the lex-order bug).
        assert!(pwsh_version_number(&p10) > pwsh_version_number(&p7));
    }

    #[test]
    fn pwsh_version_number_returns_zero_for_non_numeric_directory() {
        let path = PathBuf::from(r"C:\Program Files\PowerShell\7-preview\pwsh.exe");
        // When numeric parsing fails, fall back to 0.
        assert_eq!(pwsh_version_number(&path), 0);
    }

    #[test]
    fn pwsh_version_number_handles_missing_parent() {
        let path = PathBuf::from("pwsh.exe");
        assert_eq!(pwsh_version_number(&path), 0);
    }

    #[test]
    fn pwsh_version_number_numeric_beats_non_numeric() {
        // Because the candidate-comparison loop uses `unwrap_or(0)`, a numeric
        // version always wins over a non-numeric one (the smallest numeric
        // value, 1, is still greater than 0).
        let numeric = PathBuf::from(r"C:\Program Files\PowerShell\1\pwsh.exe");
        let preview = PathBuf::from(r"C:\Program Files\PowerShell\7-preview\pwsh.exe");
        assert!(pwsh_version_number(&numeric) > pwsh_version_number(&preview));
    }
}

/// Lua macro definition (registered as `[[macros]]` in the configuration file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroConfig {
    /// Name shown in the command palette / macro picker.
    pub name: String,
    /// Optional description of the macro.
    #[serde(default)]
    pub description: String,
    /// Name of the Lua function in `nexterm.lua`.
    /// The function must have the signature
    /// `function(session: string, pane_id: number) -> string`.
    pub lua_fn: String,
}

/// Key-binding definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// Key string (e.g. `"ctrl+shift+p"`).
    pub key: String,
    /// Action name (e.g. `"CommandPalette"`).
    pub action: String,
}
