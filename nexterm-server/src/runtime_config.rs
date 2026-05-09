//! ランタイム設定 — 設定ファイルのホットリロード対応
//!
//! [`RuntimeConfig`] はディスパッチ層から参照される設定のサブセットを保持する。
//! [`SharedRuntimeConfig`] (= `Arc<ArcSwap<RuntimeConfig>>`) を介して全クライアント
//! ハンドラに共有され、`config.toml` 変更時に [`spawn_watcher`] が
//! アトミックに新しい設定へ差し替える。
//!
//! 注意: フック (Hooks)、ログ (LogConfig)、ホスト (Hosts) のみホットリロード対象。
//! 以下はサーバー再起動が必要:
//! - `web` (起動済みリスナーは変更不可)
//! - `plugins` (動作中の WASM インスタンスを差し替えると状態を失う)
//! - `shell` (新規セッションのみに影響、既存 PTY には影響しない)
//! - `lua_runner` (LuaWorker スレッドの再生成が必要)

use std::sync::Arc;

use arc_swap::ArcSwap;
use nexterm_config::{Config, HooksConfig, HostConfig, LogConfig};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// ランタイム中にホットリロード可能な設定のサブセット
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// Lua フック設定
    pub hooks: Arc<HooksConfig>,
    /// ログ・録画設定
    pub log_config: Arc<LogConfig>,
    /// SSH ホスト設定
    pub hosts: Arc<Vec<HostConfig>>,
}

impl RuntimeConfig {
    /// 完全な [`Config`] からホットリロード対象のサブセットを抽出する
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            hooks: Arc::new(cfg.hooks.clone()),
            log_config: Arc::new(cfg.log.clone()),
            hosts: Arc::new(cfg.hosts.clone()),
        }
    }

    /// 個別フィールドから組み立てる（テストおよび初期化用）
    #[cfg(test)]
    fn new(
        hooks: Arc<HooksConfig>,
        log_config: Arc<LogConfig>,
        hosts: Arc<Vec<HostConfig>>,
    ) -> Self {
        Self {
            hooks,
            log_config,
            hosts,
        }
    }
}

/// IPC レイヤー全体で共有されるランタイム設定ハンドル
pub type SharedRuntimeConfig = Arc<ArcSwap<RuntimeConfig>>;

/// 初期 [`RuntimeConfig`] を共有ハンドルにラップする
pub fn shared(initial: RuntimeConfig) -> SharedRuntimeConfig {
    Arc::new(ArcSwap::from(Arc::new(initial)))
}

/// 受信チャネルから [`Config`] を読み続けて [`SharedRuntimeConfig`] を更新する
/// バックグラウンドタスクを起動する。watcher 本体とは分離してテスト可能にしている。
pub fn spawn_runtime_updater(shared: SharedRuntimeConfig, mut rx: mpsc::Receiver<Config>) {
    tokio::spawn(async move {
        while let Some(new_cfg) = rx.recv().await {
            let new_runtime = RuntimeConfig::from_config(&new_cfg);
            shared.store(Arc::new(new_runtime));
            info!(
                "ランタイム設定を更新しました（hosts={}件、auto_log={}）",
                new_cfg.hosts.len(),
                new_cfg.log.auto_log
            );
        }
        warn!("config watcher チャネルが閉じました。ホットリロードを停止します。");
    });
}

/// `config.toml` の変更を監視してランタイム設定を更新するバックグラウンドタスクを起動する。
///
/// 戻り値の `RecommendedWatcher` は drop されると監視を停止するため、呼び出し元で
/// `_watcher` などにバインドして保持する必要がある（`run_server` のスコープで保持される）。
pub fn spawn_watcher(shared: SharedRuntimeConfig) -> anyhow::Result<notify::RecommendedWatcher> {
    let (tx, rx) = mpsc::channel::<Config>(8);
    let watcher = nexterm_config::watch_config(tx)?;
    spawn_runtime_updater(shared, rx);
    Ok(watcher)
}

/// 現在の [`Config`] から [`SharedRuntimeConfig`] を構築するヘルパー
pub fn build_shared(cfg: &Config) -> SharedRuntimeConfig {
    shared(RuntimeConfig::from_config(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_config::{HooksConfig, LogConfig};

    #[test]
    fn shared_に_runtime_config_をラップして取り出せる() {
        let rc = RuntimeConfig::new(
            Arc::new(HooksConfig::default()),
            Arc::new(LogConfig::default()),
            Arc::new(Vec::new()),
        );
        let s = shared(rc);
        let snapshot = s.load();
        assert!(snapshot.hosts.is_empty());
    }

    #[test]
    fn store_でアトミックに差し替えられる() {
        let rc = RuntimeConfig::new(
            Arc::new(HooksConfig::default()),
            Arc::new(LogConfig::default()),
            Arc::new(Vec::new()),
        );
        let s = shared(rc);
        assert!(s.load().hosts.is_empty());

        let updated = RuntimeConfig::new(
            Arc::new(HooksConfig::default()),
            Arc::new(LogConfig::default()),
            Arc::new(vec![HostConfig {
                name: "test".into(),
                host: "localhost".into(),
                ..Default::default()
            }]),
        );
        s.store(Arc::new(updated));
        assert_eq!(s.load().hosts.len(), 1);
    }

    #[test]
    fn from_config_で必要なフィールドのみ抽出される() {
        let mut cfg = Config::default();
        cfg.hosts.push(HostConfig {
            name: "h1".into(),
            host: "1.2.3.4".into(),
            port: 2222,
            username: "alice".into(),
            ..Default::default()
        });
        let rc = RuntimeConfig::from_config(&cfg);
        assert_eq!(rc.hosts.len(), 1);
        assert_eq!(rc.hosts[0].name, "h1");
    }

    #[tokio::test]
    async fn updater_がチャネル経由で受信した設定を反映する() {
        // 初期は空の hosts
        let shared = build_shared(&Config::default());
        assert!(shared.load().hosts.is_empty());

        // updater タスクを起動
        let (tx, rx) = mpsc::channel::<Config>(4);
        spawn_runtime_updater(Arc::clone(&shared), rx);

        // 1 件追加した Config を送信
        let mut updated = Config::default();
        updated.hosts.push(HostConfig {
            name: "h1".into(),
            host: "h1.example.com".into(),
            ..Default::default()
        });
        tx.send(updated).await.expect("送信失敗");

        // updater が反映するまで待つ（短時間ポーリング）
        for _ in 0..50 {
            if shared.load().hosts.len() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(shared.load().hosts.len(), 1);
        assert_eq!(shared.load().hosts[0].name, "h1");

        // 2 件目を送信
        let mut updated2 = Config::default();
        updated2.hosts.push(HostConfig {
            name: "h2".into(),
            ..Default::default()
        });
        updated2.hosts.push(HostConfig {
            name: "h3".into(),
            ..Default::default()
        });
        tx.send(updated2).await.expect("送信失敗");

        for _ in 0..50 {
            if shared.load().hosts.len() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(shared.load().hosts.len(), 2);
    }

    #[tokio::test]
    async fn 複数読者が同じ_arcswap_を共有して最新値を読める() {
        let shared = build_shared(&Config::default());
        let (tx, rx) = mpsc::channel::<Config>(4);
        spawn_runtime_updater(Arc::clone(&shared), rx);

        // 別タスクで読みつつ、メインで書く
        let reader = Arc::clone(&shared);
        let handle = tokio::spawn(async move {
            for _ in 0..200 {
                let _snap = reader.load_full();
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        });

        let mut updated = Config::default();
        updated.hosts.push(HostConfig {
            name: "concurrent".into(),
            ..Default::default()
        });
        tx.send(updated).await.expect("送信失敗");

        handle.await.expect("reader タスク失敗");
        assert_eq!(shared.load().hosts.len(), 1);
    }
}
