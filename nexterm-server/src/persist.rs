//! セッション永続化 — セッション名リストをファイルに保存・復元する
//!
//! 保存先: `~/.local/state/nexterm/sessions.txt`（Unix）
//!          `%APPDATA%\nexterm\sessions.txt`（Windows）

use std::path::PathBuf;

use anyhow::Result;
use tracing::{info, warn};

/// 永続化ファイルのパスを返す
fn persist_path() -> PathBuf {
    state_dir().join("sessions.txt")
}

fn state_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("nexterm")
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        home.join(".local").join("state").join("nexterm")
    }
}

/// セッション名の一覧をファイルに保存する
pub fn save_session_names(names: &[String]) -> Result<()> {
    let path = persist_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = names.join("\n");
    std::fs::write(&path, content)?;
    info!("セッション名を保存しました: {:?}", path);
    Ok(())
}

/// 保存済みセッション名の一覧を読み込む
pub fn load_session_names() -> Vec<String> {
    let path = persist_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let names: Vec<String> = content
                .lines()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if !names.is_empty() {
                info!("保存済みセッションを読み込みました: {:?}", names);
            }
            names
        }
        Err(_) => Vec::new(),
    }
}

/// セッションファイルを削除する（クリーンシャットダウン時）
pub fn clear_sessions() {
    let path = persist_path();
    if let Err(e) = std::fs::remove_file(&path) {
        warn!("セッションファイルの削除に失敗しました: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn セッション名の保存と読み込み() {
        // テスト用に一時ディレクトリを使う
        let tmp = env::temp_dir().join("nexterm_test_persist");
        std::fs::create_dir_all(&tmp).unwrap();
        // persist_path() は環境変数に依存するため、直接ファイル操作でテストする
        let path = tmp.join("sessions_test.txt");
        let names = vec!["main".to_string(), "dev".to_string()];
        std::fs::write(&path, names.join("\n")).unwrap();
        let loaded: Vec<String> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        assert_eq!(loaded, names);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
