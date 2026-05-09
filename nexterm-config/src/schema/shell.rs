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
            // %ProgramFiles%\PowerShell\* を動的スキャンして最新バージョンを選択する
            let prog_files =
                std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
            let ps_root = std::path::Path::new(&prog_files).join("PowerShell");
            if let Ok(entries) = std::fs::read_dir(&ps_root) {
                let mut pwsh: Option<std::path::PathBuf> = None;
                for e in entries.flatten() {
                    let candidate = e.path().join("pwsh.exe");
                    if candidate.exists() && pwsh.as_ref().is_none_or(|p| candidate > *p) {
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

/// Lua マクロ定義（設定ファイルで [[macros]] として登録する）
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
