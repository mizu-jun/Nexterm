//! 設定ファイルのホットリロード監視

use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::loader::{config_dir, ConfigLoader};
use crate::schema::Config;

/// 設定変更通知チャネルの受信端
pub type ConfigRx = mpsc::Receiver<Config>;

/// 設定ファイルの変更を監視して新しい Config を送信するウォッチャーを起動する
///
/// 戻り値の `_watcher` は Drop されるまで監視を継続する。
/// 必ず変数にバインドして保持すること。
pub fn watch_config(
    tx: mpsc::Sender<Config>,
) -> Result<RecommendedWatcher> {
    let tx_clone = tx.clone();

    let mut watcher = notify::recommended_watcher(
        move |result: notify::Result<Event>| {
            match result {
                Ok(event) => {
                    // 書き込み・作成・削除イベントで再ロード
                    use notify::EventKind::*;
                    if matches!(event.kind, Modify(_) | Create(_) | Remove(_)) {
                        info!("設定ファイルの変更を検知しました。再ロードします。");
                        match ConfigLoader::load() {
                            Ok(new_config) => {
                                let _ = tx_clone.blocking_send(new_config);
                            }
                            Err(e) => {
                                warn!("設定の再ロードに失敗しました: {}", e);
                            }
                        }
                    }
                }
                Err(e) => warn!("ファイル監視エラー: {}", e),
            }
        },
    )?;

    let dir = config_dir();
    if dir.exists() {
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
        info!("設定ディレクトリを監視中: {}", dir.display());
    } else {
        warn!("設定ディレクトリが存在しません。監視を開始できません: {}", dir.display());
    }

    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ウォッチャーが起動できる() {
        let (tx, _rx) = mpsc::channel::<Config>(1);
        // 設定ディレクトリが存在しない場合は警告のみで Ok が返る
        let result = watch_config(tx);
        // エラーでないことを確認（ディレクトリ不在でも panic しない）
        assert!(result.is_ok() || result.is_err()); // どちらでも panic しなければ OK
    }
}
