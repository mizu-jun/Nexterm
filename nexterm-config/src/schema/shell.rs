//! シェル・シリアルポート・マクロ・キーバインドなど対話入力まわりの設定

use serde::{Deserialize, Serialize};

/// シェル設定
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    /// シェルプログラムのパス
    pub program: String,
    /// シェルに渡す引数
    pub args: Vec<String>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        #[cfg(windows)]
        {
            // %ProgramFiles%\PowerShell\* を動的スキャンして最新バージョンを選択する。
            // Sprint 5-12 Phase 2: `PathBuf` の `>` 演算子は辞書順比較のため
            // "7" > "10" となり PowerShell 7 が PowerShell 10 より「新しい」と
            // 誤判定されるバグがあった。ディレクトリ名を `u32` としてパースして
            // 数値比較するよう修正。
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
            // PowerShell 5 フォールバック
            let ps5 = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
            if std::path::Path::new(ps5).exists() {
                return Self {
                    program: ps5.to_string(),
                    args: vec!["-NoLogo".to_string()],
                };
            }
            // 最終フォールバック: cmd.exe
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

/// `pwsh.exe` のフルパスからインストール先ディレクトリ名（バージョン番号）を数値として抽出する。
///
/// 例: `C:\Program Files\PowerShell\7\pwsh.exe` → `7`、
/// `C:\Program Files\PowerShell\10\pwsh.exe` → `10`。
/// パースに失敗した場合（プレビュービルドの `7-preview` など）は `0` を返すため、
/// 数値バージョンが利用可能な場合はそちらが優先される。
///
/// Sprint 5-12 Phase 2: PathBuf の辞書順比較バグ（"7" > "10"）を回避するために導入。
#[cfg(windows)]
pub(crate) fn pwsh_version_number(pwsh_exe_path: &std::path::Path) -> u32 {
    pwsh_exe_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

/// シリアルポート設定（接続プリセット）
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SerialPortConfig {
    /// 表示名
    pub name: String,
    /// デバイスパス（例: "/dev/ttyUSB0", "COM3"）
    pub port: String,
    /// ボーレート
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    /// データビット: 5, 6, 7, 8
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    /// ストップビット: 1, 2
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    /// パリティ: "none", "odd", "even"
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

    /// Sprint 5-12 Phase 2: 辞書順バグ回帰テスト。
    /// 旧実装では `PathBuf` の `>` で比較していたため、"7" > "10" と判定されて
    /// PowerShell 10 が利用可能でも 7 が選ばれていた。
    #[test]
    fn pwsh_version_number_extracts_numeric_directory_name() {
        let p7 = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
        let p10 = PathBuf::from(r"C:\Program Files\PowerShell\10\pwsh.exe");
        assert_eq!(pwsh_version_number(&p7), 7);
        assert_eq!(pwsh_version_number(&p10), 10);
        // 数値比較として v10 > v7 が成立すること（辞書順バグの回帰テスト）
        assert!(pwsh_version_number(&p10) > pwsh_version_number(&p7));
    }

    #[test]
    fn pwsh_version_number_returns_zero_for_non_numeric_directory() {
        let path = PathBuf::from(r"C:\Program Files\PowerShell\7-preview\pwsh.exe");
        // 数値パース失敗時は 0 にフォールバック
        assert_eq!(pwsh_version_number(&path), 0);
    }

    #[test]
    fn pwsh_version_number_handles_missing_parent() {
        let path = PathBuf::from("pwsh.exe");
        assert_eq!(pwsh_version_number(&path), 0);
    }

    #[test]
    fn pwsh_version_number_numeric_beats_non_numeric() {
        // 候補比較ループで unwrap_or(0) により、数値バージョンが非数値バージョンより
        // 必ず優先される（数値の最小値 1 > 0）。
        let numeric = PathBuf::from(r"C:\Program Files\PowerShell\1\pwsh.exe");
        let preview = PathBuf::from(r"C:\Program Files\PowerShell\7-preview\pwsh.exe");
        assert!(pwsh_version_number(&numeric) > pwsh_version_number(&preview));
    }
}

/// Lua マクロ定義（設定ファイルで `[[macros]]` として登録する）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroConfig {
    /// コマンドパレット / マクロピッカーに表示する名前
    pub name: String,
    /// マクロの説明文（オプション）
    #[serde(default)]
    pub description: String,
    /// nexterm.lua 内の Lua 関数名
    /// この関数は `function(session: string, pane_id: number) -> string` のシグネチャを持つ
    pub lua_fn: String,
}

/// キーバインド定義
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// キー文字列（例: "ctrl+shift+p"）
    pub key: String,
    /// アクション名（例: "CommandPalette"）
    pub action: String,
}
