//! ホストマネージャ UI — Ctrl+Shift+H でフローティングリストを表示する
//!
//! 設定ファイルの `[[hosts]]` エントリと ~/.ssh/config のエントリを一覧表示し、
//! Enter で選択したホストへ SSH 接続を開始する。
//! タグ・グループフィルター・接続履歴による並び替えに対応する。

use std::collections::HashMap;

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use nexterm_config::HostConfig;

/// ~/.ssh/config を解析して HostConfig の一覧を返す
///
/// ワイルドカード（`Host *`）は無視する。
/// `Hostname` が省略された場合は `Host` エイリアスをそのまま使う。
pub fn load_ssh_config() -> Vec<HostConfig> {
    let path = {
        #[cfg(windows)]
        {
            std::env::var("USERPROFILE")
                .ok()
                .map(|p| std::path::PathBuf::from(p).join(".ssh").join("config"))
        }
        #[cfg(not(windows))]
        {
            std::env::var("HOME")
                .ok()
                .map(|p| std::path::PathBuf::from(p).join(".ssh").join("config"))
        }
    };

    let Some(path) = path else {
        return Vec::new();
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    parse_ssh_config(&content)
}

/// SSH config テキストを解析する
fn parse_ssh_config(content: &str) -> Vec<HostConfig> {
    let mut hosts: Vec<HostConfig> = Vec::new();
    // 現在処理中のブロック
    let mut current_alias: Option<String> = None;
    let mut current_hostname: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_port: u16 = 22;
    let mut current_key: Option<String> = None;

    let flush = |hosts: &mut Vec<HostConfig>,
                 alias: &Option<String>,
                 hostname: &Option<String>,
                 user: &Option<String>,
                 port: u16,
                 key: &Option<String>| {
        let Some(alias) = alias.clone() else {
            return;
        };
        // ワイルドカードは除外する
        if alias.contains('*') || alias.contains('?') {
            return;
        }
        let host = hostname.clone().unwrap_or_else(|| alias.clone());
        let username = user.clone().unwrap_or_else(|| "root".to_string());
        let auth_type = if key.is_some() {
            "key".to_string()
        } else {
            "agent".to_string()
        };
        // ~/.ssh/ プレフィックスを展開する
        let key_path = key.as_ref().map(|k| {
            if k.starts_with("~/") {
                #[cfg(windows)]
                let home = std::env::var("USERPROFILE").unwrap_or_default();
                #[cfg(not(windows))]
                let home = std::env::var("HOME").unwrap_or_default();
                format!("{}{}", home, &k[1..])
            } else {
                k.clone()
            }
        });
        hosts.push(HostConfig {
            name: format!("{} (ssh config)", alias),
            host,
            port,
            username,
            auth_type,
            key_path,
            forward_local: Vec::new(),
            forward_remote: Vec::new(),
            proxy_jump: None,
            x11_forward: false,
            x11_trusted: false,
            group: String::new(),
            tags: Vec::new(),
        });
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // `keyword value` または `keyword=value` 形式を解析する
        let (keyword, value) = if let Some(eq) = trimmed.find('=') {
            let k = trimmed[..eq].trim();
            let v = trimmed[eq + 1..].trim();
            (k, v)
        } else if let Some(sp) = trimmed.find(char::is_whitespace) {
            let k = trimmed[..sp].trim();
            let v = trimmed[sp..].trim();
            (k, v)
        } else {
            continue;
        };

        match keyword.to_lowercase().as_str() {
            "host" => {
                // 前のブロックを確定する
                flush(
                    &mut hosts,
                    &current_alias,
                    &current_hostname,
                    &current_user,
                    current_port,
                    &current_key,
                );
                current_alias = Some(value.to_string());
                current_hostname = None;
                current_user = None;
                current_port = 22;
                current_key = None;
            }
            "hostname" => {
                current_hostname = Some(value.to_string());
            }
            "user" => {
                current_user = Some(value.to_string());
            }
            "port" => {
                current_port = value.parse().unwrap_or(22);
            }
            "identityfile" => {
                current_key = Some(value.to_string());
            }
            _ => {}
        }
    }

    // 最後のブロックを確定する
    flush(
        &mut hosts,
        &current_alias,
        &current_hostname,
        &current_user,
        current_port,
        &current_key,
    );

    hosts
}

/// 接続履歴エントリ
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// ホストの一意キー（"username@host:port"）
    pub key: String,
    /// 接続回数
    pub count: u32,
    /// 最終接続時刻（Unix エポック秒）
    pub last_connected: u64,
}

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
    /// アクティブなタグフィルター（空 = フィルターなし）
    pub tag_filter: Option<String>,
    /// アクティブなグループフィルター（空 = フィルターなし）
    pub group_filter: Option<String>,
    /// 接続履歴（ホストキー → エントリ）
    history: HashMap<String, HistoryEntry>,
    /// Fuzzy マッチャー
    matcher: SkimMatcherV2,
}

impl HostManager {
    /// 設定からホスト一覧を受け取ってマネージャを生成する
    ///
    /// nexterm.toml の `[[hosts]]` に加えて ~/.ssh/config のエントリも取り込む。
    pub fn new(hosts: Vec<HostConfig>) -> Self {
        let mut merged = hosts;
        let ssh_hosts = load_ssh_config();
        // nexterm.toml に同名ホストがなければ追加する
        for ssh_host in ssh_hosts {
            let already = merged
                .iter()
                .any(|h| h.host == ssh_host.host && h.port == ssh_host.port);
            if !already {
                merged.push(ssh_host);
            }
        }
        Self {
            hosts: merged,
            query: String::new(),
            is_open: false,
            selected: 0,
            tag_filter: None,
            group_filter: None,
            history: HashMap::new(),
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
            self.selected = if self.selected == 0 {
                count - 1
            } else {
                self.selected - 1
            };
        }
    }

    /// 現在選択中のホスト設定を返す
    pub fn selected_host(&self) -> Option<&HostConfig> {
        self.filtered().into_iter().nth(self.selected)
    }

    /// ホストへの接続を記録する（接続回数・最終接続時刻を更新）
    pub fn record_connection(&mut self, host: &HostConfig) {
        let key = format!("{}@{}:{}", host.username, host.host, host.port);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = self.history.entry(key.clone()).or_insert(HistoryEntry {
            key,
            count: 0,
            last_connected: 0,
        });
        entry.count += 1;
        entry.last_connected = now;
    }

    /// 指定ホストの接続履歴エントリを返す
    pub fn history_for(&self, host: &HostConfig) -> Option<&HistoryEntry> {
        let key = format!("{}@{}:{}", host.username, host.host, host.port);
        self.history.get(&key)
    }

    /// 登録済みタグの重複なし一覧を返す（タグフィルター UI 用）
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .hosts
            .iter()
            .flat_map(|h| h.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// 登録済みグループの重複なし一覧を返す（グループフィルター UI 用）
    pub fn all_groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = self
            .hosts
            .iter()
            .filter(|h| !h.group.is_empty())
            .map(|h| h.group.clone())
            .collect();
        groups.sort();
        groups.dedup();
        groups
    }

    /// タグフィルターを設定する（None でフィルター解除）
    pub fn set_tag_filter(&mut self, tag: Option<String>) {
        self.tag_filter = tag;
        self.selected = 0;
    }

    /// グループフィルターを設定する（None でフィルター解除）
    pub fn set_group_filter(&mut self, group: Option<String>) {
        self.group_filter = group;
        self.selected = 0;
    }

    /// クエリ・タグ・グループにマッチするホストを返す
    ///
    /// 並び順: 接続頻度の高い順（接続履歴あり） → アルファベット順
    pub fn filtered(&self) -> Vec<&HostConfig> {
        // タグフィルターを適用する
        let tag_filtered: Vec<&HostConfig> = self
            .hosts
            .iter()
            .filter(|h| {
                if let Some(tag) = &self.tag_filter {
                    h.tags.contains(tag)
                } else {
                    true
                }
            })
            .filter(|h| {
                if let Some(group) = &self.group_filter {
                    &h.group == group
                } else {
                    true
                }
            })
            .collect();

        // Fuzzy クエリフィルターを適用する
        let mut scored: Vec<(i64, u32, &HostConfig)> = if self.query.is_empty() {
            tag_filtered
                .into_iter()
                .map(|h| {
                    let freq = self.history_for(h).map(|e| e.count).unwrap_or(0);
                    (0i64, freq, h)
                })
                .collect()
        } else {
            tag_filtered
                .into_iter()
                .filter_map(|h| {
                    // 表示名・ホスト名・ユーザー名・タグ・グループをまとめてマッチする
                    let haystack = format!(
                        "{} {}@{} {} {}",
                        h.name,
                        h.username,
                        h.host,
                        h.tags.join(" "),
                        h.group
                    );
                    self.matcher
                        .fuzzy_match(&haystack, &self.query)
                        .map(|score| {
                            let freq = self.history_for(h).map(|e| e.count).unwrap_or(0);
                            (score, freq, h)
                        })
                })
                .collect()
        };

        // スコア降順 → 接続頻度降順 → 名前昇順 でソートする
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(b.1.cmp(&a.1))
                .then(a.2.name.cmp(&b.2.name))
        });
        scored.into_iter().map(|(_, _, h)| h).collect()
    }

    /// ホスト一覧を更新する（設定ファイルリロード時に使用）
    #[allow(dead_code)]
    pub fn reload(&mut self, hosts: Vec<HostConfig>) {
        let mut merged = hosts;
        let ssh_hosts = load_ssh_config();
        for ssh_host in ssh_hosts {
            let already = merged
                .iter()
                .any(|h| h.host == ssh_host.host && h.port == ssh_host.port);
            if !already {
                merged.push(ssh_host);
            }
        }
        self.hosts = merged;
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
            x11_forward: false,
            x11_trusted: false,
            group: String::new(),
            tags: Vec::new(),
        }
    }

    fn make_host_with_tags(name: &str, host: &str, group: &str, tags: &[&str]) -> HostConfig {
        HostConfig {
            name: name.to_string(),
            host: host.to_string(),
            port: 22,
            username: "ubuntu".to_string(),
            auth_type: "agent".to_string(),
            key_path: None,
            forward_local: vec![],
            forward_remote: vec![],
            proxy_jump: None,
            x11_forward: false,
            x11_trusted: false,
            group: group.to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
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

    #[test]
    fn タグフィルターが機能する() {
        let mut mgr = HostManager::new(vec![
            make_host_with_tags("web", "web.example.com", "prod", &["web", "prod"]),
            make_host_with_tags("db", "db.example.com", "prod", &["db", "prod"]),
            make_host_with_tags("dev", "dev.example.com", "dev", &["dev"]),
        ]);
        mgr.set_tag_filter(Some("prod".to_string()));
        assert_eq!(mgr.filtered().len(), 2);
        mgr.set_tag_filter(Some("web".to_string()));
        assert_eq!(mgr.filtered().len(), 1);
        assert_eq!(mgr.filtered()[0].name, "web");
        mgr.set_tag_filter(None);
        assert_eq!(mgr.filtered().len(), 3);
    }

    #[test]
    fn グループフィルターが機能する() {
        let mut mgr = HostManager::new(vec![
            make_host_with_tags("web", "web.example.com", "production", &[]),
            make_host_with_tags("db", "db.example.com", "production", &[]),
            make_host_with_tags("dev", "dev.example.com", "development", &[]),
        ]);
        mgr.set_group_filter(Some("production".to_string()));
        assert_eq!(mgr.filtered().len(), 2);
        mgr.set_group_filter(Some("development".to_string()));
        assert_eq!(mgr.filtered().len(), 1);
    }

    #[test]
    fn all_tags_が重複なしで返る() {
        let mgr = HostManager::new(vec![
            make_host_with_tags("a", "a.example.com", "", &["web", "prod"]),
            make_host_with_tags("b", "b.example.com", "", &["db", "prod"]),
        ]);
        let tags = mgr.all_tags();
        // "db", "prod", "web" の 3 件（ソート済み・重複なし）
        assert_eq!(tags, vec!["db", "prod", "web"]);
    }

    #[test]
    fn all_groups_が重複なしで返る() {
        let mgr = HostManager::new(vec![
            make_host_with_tags("a", "a.example.com", "prod", &[]),
            make_host_with_tags("b", "b.example.com", "prod", &[]),
            make_host_with_tags("c", "c.example.com", "dev", &[]),
        ]);
        let groups = mgr.all_groups();
        assert_eq!(groups, vec!["dev", "prod"]);
    }

    #[test]
    fn 接続履歴が記録される() {
        let host = make_host("prod", "prod.example.com", "ubuntu");
        let mut mgr = HostManager::new(vec![host.clone()]);
        assert!(mgr.history_for(&host).is_none());
        mgr.record_connection(&host);
        mgr.record_connection(&host);
        let entry = mgr.history_for(&host).unwrap();
        assert_eq!(entry.count, 2);
    }

    #[test]
    fn 接続頻度が高いホストが上位に並ぶ() {
        let host_a = make_host("alpha", "alpha.example.com", "user");
        let host_b = make_host("beta", "beta.example.com", "user");
        let mut mgr = HostManager::new(vec![host_a.clone(), host_b.clone()]);
        // beta を 3 回接続する
        for _ in 0..3 {
            mgr.record_connection(&host_b);
        }
        // alpha を 1 回接続する
        mgr.record_connection(&host_a);
        // クエリなしのフィルターで beta が先頭に来るはず
        let results = mgr.filtered();
        assert_eq!(results[0].name, "beta");
        assert_eq!(results[1].name, "alpha");
    }

    #[test]
    fn タグ検索がfuzzyクエリと組み合わせられる() {
        let mut mgr = HostManager::new(vec![
            make_host_with_tags("web-prod", "web.example.com", "prod", &["web", "prod"]),
            make_host_with_tags("db-prod", "db.example.com", "prod", &["db", "prod"]),
        ]);
        mgr.set_tag_filter(Some("prod".to_string()));
        mgr.query = "web".to_string();
        let results = mgr.filtered();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "web-prod");
    }

    #[test]
    fn ssh_config_基本的なホストを解析できる() {
        let config = r#"
Host myserver
    Hostname 192.168.1.100
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_rsa
"#;
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "192.168.1.100");
        assert_eq!(hosts[0].username, "admin");
        assert_eq!(hosts[0].port, 2222);
        assert_eq!(hosts[0].auth_type, "key");
    }

    #[test]
    fn ssh_config_hostnameなしはaliasをhostに使う() {
        let config = r#"
Host myalias
    User alice
"#;
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "myalias");
    }

    #[test]
    fn ssh_config_ワイルドカードは除外される() {
        let config = r#"
Host *
    ServerAliveInterval 60

Host real-server
    Hostname srv.example.com
"#;
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "srv.example.com");
    }

    #[test]
    fn ssh_config_複数ホストを解析できる() {
        let config = r#"
Host web
    Hostname web.example.com
    User ubuntu

Host db
    Hostname db.example.com
    User postgres
    Port 2222
"#;
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].host, "web.example.com");
        assert_eq!(hosts[1].port, 2222);
    }

    #[test]
    fn ssh_config_コメント行は無視される() {
        let config = r#"
# これはコメント
Host myhost
    # ホスト名コメント
    Hostname 10.0.0.1
"#;
        let hosts = parse_ssh_config(config);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "10.0.0.1");
    }
}
