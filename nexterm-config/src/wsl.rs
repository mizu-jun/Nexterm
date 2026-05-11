//! WSL ディストロ自動検出（Sprint 5-4 / E1）。
//!
//! Windows 環境で `wsl.exe -l -q` を実行して利用可能なディストロを列挙し、
//! それぞれを `Profile` に変換する。Windows 以外では常に空ベクタを返す。
//!
//! # 出力エンコーディング
//!
//! `wsl.exe -l -q` の標準出力は **UTF-16LE** で、行末は CR/LF。
//! 本実装は BOM の有無に関わらず UTF-16LE を UTF-8 に変換する。
//!
//! # 使用例
//!
//! ```no_run
//! use nexterm_config::wsl::detect_distros;
//! let distros = detect_distros();
//! for d in distros {
//!     println!("発見: {}", d.name);
//! }
//! ```

use crate::schema::{Profile, ShellConfig};

/// 検出された WSL ディストロを `Profile` のベクタとして返す。
///
/// Windows 以外では常に空ベクタを返す。
/// `wsl.exe` がインストールされていない、または検出に失敗した場合も空ベクタを返す。
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

    // wsl.exe -l -q は UTF-16LE で出力する
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

/// UTF-16LE バイト列からディストロ名のリストを抽出する。
///
/// BOM (0xFF 0xFE) は許容する。空行・null 文字を除去する。
/// テスト容易化のため OS 非依存で公開する。
pub fn parse_wsl_list_output(bytes: &[u8]) -> Vec<String> {
    // BOM をスキップ
    let payload = if bytes.starts_with(&[0xFF, 0xFE]) {
        &bytes[2..]
    } else {
        bytes
    };

    // 2 バイトずつ UTF-16LE として decode
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
        // BOM + "Ubuntu\r\n" を UTF-16LE で構築
        let mut bytes = vec![0xFF, 0xFE];
        for c in "Ubuntu\r\n".encode_utf16() {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let result = parse_wsl_list_output(&bytes);
        assert_eq!(result, vec!["Ubuntu"]);
    }

    #[test]
    fn parse_multiple_distros() {
        // "Ubuntu\r\nDebian\r\nAlpine\r\n" を UTF-16LE で構築
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
