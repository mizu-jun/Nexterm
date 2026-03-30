//! ホストマネージャ UI — Ctrl+Shift+H でフローティングリストを表示する
//!
//! 設定ファイルの `[[hosts]]` エントリを一覧表示し、
//! Enter で選択したホストへ SSH 接続を開始する。

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use nexterm_config::HostConfig;

/// ホストマネージャの表示/操作状態
pub struct HostManager {
    /// 登録済みホスト一覧（設定ファイルから読み込む）
    hosts: Vec<HostConfig>,
    /// 現在の検索クエリ
    pub query: String,
    /// パネルが開いているか
    pub is_open: bool,
    /// 選択中のインデックス（フィルタ後リスト上）
    pub selected: usize,
    /// Fuzzy マッチャー
    matcher: SkimMatcherV2,
}

impl HostManager {
    /// 設定からホスト一覧を受け取ってマネージャを生成する
    pub fn new(hosts: Vec<HostConfig>) -> Self {
        Self {
            hosts,
            query: String::new(),
            is_open: false,
            selected: 0,
            matcher: SkimMatcherV2::default(),
        }
    }

    /// パネルを開いてクエリ・選択をリセットする
    pub fn open(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.is_open = true;
    }

    /// パネルを閉じる
    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
    }

    /// 検索クエリに文字を追加する
    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
        self.selected = 0;
    }

    /// 検索クエリの末尾を削除する
    pub fn pop_char(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// 選択を下に移動する（循環）
    pub fn select_next(&mut self) {
        let count = self.filtered().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    /// 選択を上に移動する（循環）
    pub fn select_prev(&mut self) {
        let count = self.filtered().len();
        if count > 0 {
            self.selected = if self.selected == 0 { count - 1 } else { self.selected - 1 };
        }
    }

    /// 現在選択中のホスト設定を返す
    pub fn selected_host(&self) -> Option<&HostConfig> {
        self.filtered().into_iter().nth(self.selected)
    }

    /// クエリにマッチするホストをスコア降順で返す
    pub fn filtered(&self) -> Vec<&HostConfig> {
        if self.query.is_empty() {
            return self.hosts.iter().collect();
        }

        let mut scored: Vec<(i64, &HostConfig)> = self
            .hosts
            .iter()
            .filter_map(|h| {
                // 表示名・ホスト名・ユーザー名をまとめてマッチする
                let haystack = format!("{} {}@{}", h.name, h.username, h.host);
                self.matcher
                    .fuzzy_match(&haystack, &self.query)
                    .map(|score| (score, h))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, h)| h).collect()
    }

    /// ホスト一覧を更新する（設定ファイルリロード時に使用）
    #[allow(dead_code)]
    pub fn reload(&mut self, hosts: Vec<HostConfig>) {
        self.hosts = hosts;
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_host(name: &str, host: &str, username: &str) -> HostConfig {
        HostConfig {
            name: name.to_string(),
            host: host.to_string(),
            port: 22,
            username: username.to_string(),
            auth_type: "key".to_string(),
            key_path: None,
            forward_local: vec![],
            forward_remote: vec![],
            proxy_jump: None,
        }
    }

    #[test]
    fn empty_query_returns_all() {
        let mgr = HostManager::new(vec![
            make_host("web", "192.168.1.1", "admin"),
            make_host("db", "192.168.1.2", "admin"),
        ]);
        assert_eq!(mgr.filtered().len(), 2);
    }

    #[test]
    fn fuzzy_filter_works() {
        let mut mgr = HostManager::new(vec![
            make_host("web-server", "web.example.com", "ubuntu"),
            make_host("db-server", "db.example.com", "postgres"),
        ]);
        mgr.query = "web".to_string();
        let results = mgr.filtered();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "web-server");
    }

    #[test]
    fn selection_wraps() {
        let mut mgr = HostManager::new(vec![
            make_host("a", "a.local", "user"),
            make_host("b", "b.local", "user"),
        ]);
        mgr.selected = 1;
        mgr.select_next();
        assert_eq!(mgr.selected, 0);
        mgr.select_prev();
        assert_eq!(mgr.selected, 1);
    }
}
