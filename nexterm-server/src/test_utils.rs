//! テストユーティリティ — 共通テストヘルパー関数

use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use tempfile::TempDir;

static TEST_PORT: AtomicU16 = AtomicU16::new(9000);

/// ユニークなテスト用パスを生成する
pub fn temp_json_path(name: &str) -> PathBuf {
    let temp = TempDir::new().expect("一時ディレクトリの作成に失敗");
    temp.path()
        .join(format!("{}_{}.json", name, std::process::id()))
}

/// テスト用に一時的に環境変数を設定し、スコープ終了時に自動クリーンアップする
pub struct TempEnvVar {
    key: String,
    original: Option<String>,
}

impl TempEnvVar {
    /// 環境変数を一時的に設定
    pub fn set(key: &str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key: key.to_string(),
            original,
        }
    }
}

impl Drop for TempEnvVar {
    fn drop(&mut self) {
        if let Some(ref val) = self.original {
            unsafe {
                std::env::set_var(&self.key, val);
            }
        } else {
            unsafe {
                std::env::remove_var(&self.key);
            }
        }
    }
}

/// テスト用の一時ファイルパスを管理し、自動クリーンアップする
pub struct TempFile {
    /// 一時ファイルへのパス（ファイル本体は呼び出し側で書き込む）
    pub path: PathBuf,
    _temp_dir: TempDir,
}

impl TempFile {
    /// 一時ディレクトリ配下に `<name>_<pid>.json` パスを生成する（ファイルは作らない）
    pub fn new(name: &str) -> Self {
        let temp_dir = TempDir::new().expect("一時ディレクトリの作成に失敗");
        let path = temp_dir
            .path()
            .join(format!("{}_{}.json", name, std::process::id()));
        Self {
            path,
            _temp_dir: temp_dir,
        }
    }
}

/// 一意のテスト用ポート番号を取得
pub fn next_test_port() -> u16 {
    TEST_PORT.fetch_add(1, Ordering::SeqCst)
}

/// テスト用の有効期限付きの一時ディレクトリ
pub fn temp_dir() -> TempDir {
    TempDir::new().expect("一時ディレクトリの作成に失敗")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_creates_and_cleans_up() {
        let path = {
            let temp = TempFile::new("test");
            let p = temp.path.clone();
            assert!(!p.exists());
            std::fs::write(&p, "test data").unwrap();
            assert!(p.exists());
            p
        };
        assert!(!path.exists());
    }

    #[test]
    fn temp_env_var_restores_original() {
        let key = "___TEST_VAR___";
        unsafe {
            std::env::remove_var(key);
        }

        {
            let _env = TempEnvVar::set(key, "test_value");
            assert_eq!(std::env::var(key).unwrap(), "test_value");
        }

        assert!(std::env::var(key).is_err());
    }

    #[test]
    fn unique_test_ports() {
        let p1 = next_test_port();
        let p2 = next_test_port();
        assert_ne!(p1, p2);
    }
}
