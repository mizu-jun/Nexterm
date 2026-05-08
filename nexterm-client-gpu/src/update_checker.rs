//! バックグラウンド更新チェッカー
//!
//! 起動後 5 秒待機してから GitHub Releases API をポーリングし、
//! 現在バージョンより新しいリリースがあれば `tokio::sync::watch` 経由で通知する。

use tokio::sync::watch;
use tracing::{info, warn};

/// GitHub Releases API レスポンスの最小フィールド
#[derive(serde::Deserialize)]
struct GhRelease {
    tag_name: String,
}

/// バックグラウンド更新チェックを開始する。
///
/// 戻り値: 最新バージョン文字列 (例: "0.9.15") を受信する watch::Receiver。
/// `auto_check_update` が false の場合は即座に None のままの Receiver を返す。
pub fn start(current_version: &str, enabled: bool) -> watch::Receiver<Option<String>> {
    let (tx, rx) = watch::channel(None);

    if !enabled {
        return rx;
    }

    let current = current_version.to_string();
    tokio::spawn(async move {
        // 起動直後のリソース競合を避けるため 5 秒待機する
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        match fetch_latest_version().await {
            Ok(latest) if is_newer(&latest, &current) => {
                info!("新しいバージョンが利用可能: v{}", latest);
                let _ = tx.send(Some(latest));
            }
            Ok(latest) => {
                info!("最新バージョン v{} を使用中（更新不要）", latest);
            }
            Err(e) => {
                warn!("更新チェックに失敗しました: {}", e);
            }
        }
    });

    rx
}

/// GitHub Releases API から最新リリースのタグ名を取得する
async fn fetch_latest_version() -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("nexterm/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let release: GhRelease = client
        .get("https://api.github.com/repos/mizu-jun/nexterm/releases/latest")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // タグ名の先頭 "v" を除去して返す（例: "v0.9.15" → "0.9.15"）
    Ok(release.tag_name.trim_start_matches('v').to_string())
}

/// `latest` が `current` より新しいかどうかをセマンティックバージョン比較で判定する。
/// パースできない場合は false を返す（安全側に倒す）。
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() < 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].split('-').next()?.parse().ok()?,
        ))
    };

    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_true() {
        assert!(is_newer("0.9.15", "0.9.14"));
        assert!(is_newer("1.0.0", "0.9.99"));
        assert!(is_newer("0.10.0", "0.9.99"));
    }

    #[test]
    fn test_is_newer_false() {
        assert!(!is_newer("0.9.14", "0.9.14")); // 同じバージョン
        assert!(!is_newer("0.9.13", "0.9.14")); // 古いバージョン
    }

    #[test]
    fn test_is_newer_prerelease_suffix() {
        // "-beta" など suffix は無視してパッチ番号のみ比較する
        assert!(is_newer("0.9.15", "0.9.14-beta"));
        assert!(!is_newer("0.9.14-beta", "0.9.14"));
    }

    #[test]
    fn test_is_newer_invalid() {
        assert!(!is_newer("invalid", "0.9.14"));
        assert!(!is_newer("0.9.15", "not-semver"));
    }
}
