//! ホストマネージャ UI — Ctrl+Shift+H でフローティングリストを表示する
//!
//! 設定ファイルの `[[hosts]]` エントリと ~/.ssh/config のエントリを一覧表示し、
//! Enter で選択したホストへ SSH 接続を開始する。
//! タグ・グループフィルター・接続履歴による並び替えに対応する。

use std::collections::HashMap;
use std::path::PathBuf;

use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use nexterm_config::HostConfig;
use serde::{Deserialize, Serialize};
use tracing::warn;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// ホストの一意キー（"username@host:port"）
    pub key: String,
    /// 接続回数
    pub count: u32,
    /// 最終接続時刻（Unix エポック秒）
    pub last_connected: u64,
}

// ---- 接続履歴の永続化 ----

/// 接続履歴の保存先パスを返す
///
/// Unix: `~/.local/state/nexterm/host_history.json`
/// Windows: `%APPDATA%\nexterm\host_history.json`
fn history_path() -> PathBuf {
    // テスト環境では環境変数でパスを上書き可能にする
    if let Ok(test_path) = std::env::var("__NEXTERM_TEST_HOST_HISTORY_PATH__") {
        return PathBuf::from(test_path);
    }

    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("nexterm").join("host_history.json")
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            return PathBuf::from(xdg).join("nexterm").join("host_history.json");
        }
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        home.join(".local")
            .join("state")
            .join("nexterm")
            .join("host_history.json")
    }
}

/// 接続履歴を JSON ファイルから読み込む（ファイルがなければ空マップを返す）
pub fn load_history() -> HashMap<String, HistoryEntry> {
    let path = history_path();
    if !path.exists() {
        return HashMap::new();
    }
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => {
            warn!("接続履歴の読み込みに失敗しました: {}", e);
            return HashMap::new();
        }
    };
    match serde_json::from_str(&json) {
        Ok(map) => map,
        Err(e) => {
            warn!("接続履歴のパースに失敗しました: {}", e);
            HashMap::new()
        }
    }
}

/// 接続履歴を JSON ファイルに保存する
///
/// atomic write（一時ファイル → rename）で書き込み、Unix では 0600 パーミッションを
/// 強制する。ホスト名・ユーザー名は機密性のある情報のため、共有ホストで他ユーザー
/// から読み取られないよう保護する。
pub fn save_history(history: &HashMap<String, HistoryEntry>) {
    let path = history_path();
    let json = match serde_json::to_string_pretty(history) {
        Ok(j) => j,
        Err(e) => {
            warn!("接続履歴のシリアライズに失敗しました: {}", e);
            return;
        }
    };
    if let Err(e) = write_atomic_secure(&path, json.as_bytes()) {
        warn!("接続履歴の保存に失敗しました: {}", e);
    }
}

/// ファイルをアトミックに書き込み、Unix では 0600 パーミッションを強制する。
///
/// nexterm-server/src/persist.rs::write_atomic_secure と同等の実装。
/// クレート間依存を避けるためローカルに複製している。
fn write_atomic_secure(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("親ディレクトリが取得できません: {:?}", path),
        )
    })?;
    std::fs::create_dir_all(parent)?;

    let tmp_name = format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("nexterm"),
        std::process::id()
    );
    let tmp_path = parent.join(tmp_name);

    {
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp_path)?
        };
        #[cfg(windows)]
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;

        if let Err(e) = file.write_all(content) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
        if let Err(e) = file.sync_all() {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
    }

    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

/// パスワード入力モーダルの状態
///
/// HIGH H-6 対策: 入力中のパスワード文字列を `Zeroizing<String>` でラップし、
/// drop 時に確実にメモリ上の内容をゼロクリアする。これによりキーロガー
/// やメモリスクレイプによるパスワード漏洩のリスクを低減する。
///
/// Sprint 3-2 後半: OS キーチェーン統合
/// - `new()` 時に `nexterm_config::keyring::get_password()` で既存パスワードを
///   プリフィル（無ければ空文字のまま）
/// - `remember` フラグが true で `take_password()` 呼出時に
///   `keyring::store_password()` で保存（失敗してもログのみで処理続行）
/// - host_history.json には引き続きパスワードを書き込まない（HistoryEntry に
///   password フィールドが無いことで保証）
pub struct PasswordModal {
    /// 対象ホスト設定
    pub host: HostConfig,
    /// 入力中のパスワード（表示しない、drop 時にゼロクリア）
    input: zeroize::Zeroizing<String>,
    /// エラーメッセージ（認証失敗時）
    pub error: Option<String>,
    /// keyring から事前に取得できたか（プリフィル状態の表示用）
    pub prefilled: bool,
    /// パスワードを OS キーチェーンに保存するか（Tab キーでトグル）
    pub remember: bool,
}

impl PasswordModal {
    pub fn new(host: HostConfig) -> Self {
        // 起動時に keyring からパスワード取得を試行する。失敗しても無視する。
        let (input, prefilled) =
            match nexterm_config::keyring::get_password(&host.name, &host.username) {
                Ok(stored) => (zeroize::Zeroizing::new((*stored).clone()), true),
                Err(_) => (zeroize::Zeroizing::new(String::new()), false),
            };
        Self {
            host,
            input,
            error: None,
            prefilled,
            // プリフィル成功時は remember=true をデフォルトに（既存利用継続）
            remember: prefilled,
        }
    }

    pub fn push_char(&mut self, ch: char) {
        self.input.push(ch);
    }

    pub fn pop_char(&mut self) {
        self.input.pop();
    }

    /// 現在の入力長を取得する（マスク表示用）
    pub fn input_len(&self) -> usize {
        self.input.chars().count()
    }

    /// remember フラグをトグルする（UI から Tab キーで呼ぶ）
    pub fn toggle_remember(&mut self) {
        self.remember = !self.remember;
    }

    /// 入力済みパスワードを取り出してクリアする（接続送信後に呼ぶ）
    ///
    /// 戻り値も `Zeroizing<String>` として返し、呼び出し側でも drop 時
    /// ゼロクリアされるようにする。
    ///
    /// `remember` が true の場合は OS キーチェーンへの保存を試みる。
    /// 保存失敗時は `tracing::warn!` でログのみ出して処理を続行する
    /// （keyring サービスが利用できない環境への配慮）。
    pub fn take_password(&mut self) -> zeroize::Zeroizing<String> {
        let taken = std::mem::take(&mut *self.input);

        if self.remember && !taken.is_empty() {
            if let Err(e) = nexterm_config::keyring::store_password(
                &self.host.name,
                &self.host.username,
                &taken,
            ) {
                warn!(
                    "OS キーチェーンへのパスワード保存に失敗しました（host={}, user={}）: {}",
                    self.host.name, self.host.username, e
                );
            }
        } else if !self.remember && self.prefilled {
            // remember を OFF にした場合は既存の保存パスワードを削除する
            if let Err(e) =
                nexterm_config::keyring::delete_password(&self.host.name, &self.host.username)
            {
                warn!(
                    "OS キーチェーンからのパスワード削除に失敗しました（host={}, user={}）: {}",
                    self.host.name, self.host.username, e
                );
            }
        }

        zeroize::Zeroizing::new(taken)
    }
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
    /// パスワード入力モーダル（auth_type=="password" のホスト選択時に開く）
    pub password_modal: Option<PasswordModal>,
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
            history: load_history(),
            matcher: SkimMatcherV2::default(),
            password_modal: None,
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

    /// ホストへの接続を記録する（接続回数・最終接続時刻を更新し、ファイルに保存する）
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
        save_history(&self.history);
    }

    /// 指定ホストの接続履歴エントリを返す
    pub fn history_for(&self, host: &HostConfig) -> Option<&HistoryEntry> {
        let key = format!("{}@{}:{}", host.username, host.host, host.port);
        self.history.get(&key)
    }

    /// 登録済みタグの重複なし一覧を返す（タグフィルター UI 用）
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn set_tag_filter(&mut self, tag: Option<String>) {
        self.tag_filter = tag;
        self.selected = 0;
    }

    /// グループフィルターを設定する（None でフィルター解除）
    #[allow(dead_code)]
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
        // テスト用に一時ファイルパスを設定 (リークを避けるため明示的削除)
        let temp_file =
            std::env::temp_dir().join(format!("host_history_{}.json", std::process::id()));
        unsafe {
            std::env::set_var("__NEXTERM_TEST_HOST_HISTORY_PATH__", &temp_file);
        }

        {
            let host = make_host("prod", "prod.example.com", "ubuntu");
            let mut mgr = HostManager::new(vec![host.clone()]);
            assert!(mgr.history_for(&host).is_none());

            mgr.record_connection(&host);
            mgr.record_connection(&host);

            let entry = mgr.history_for(&host).unwrap();
            assert_eq!(entry.count, 2);
        }

        // クリーンアップ
        let _ = std::fs::remove_file(&temp_file);
        unsafe {
            std::env::remove_var("__NEXTERM_TEST_HOST_HISTORY_PATH__");
        }
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

    // ---- PasswordModal: keyring 統合（Sprint 3-2 後半）----
    //
    // 注: keyring は OS キーチェーンに依存するため、本番のキーチェーンへの
    // 副作用を避けるためにテスト用ホスト名は他テストと衝突しない一意の
    // 名前を使い、テスト終了時に delete_password で必ず後始末する。
    // CI 環境で keyring が利用できない場合は new() 時に prefilled=false に
    // フォールバックするだけで panic しない設計のため、テストは安全に通る。

    fn make_test_host_for_modal(name: &str) -> HostConfig {
        HostConfig {
            name: name.to_string(),
            host: "127.0.0.1".to_string(),
            port: 22,
            username: "testuser".to_string(),
            auth_type: "password".to_string(),
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

    #[test]
    fn password_modal_新規生成は空入力で開始する() {
        // 衝突しない一意のホスト名（テスト名のハッシュ的な接尾辞）
        let host = make_test_host_for_modal("__pm_test_init__");
        // 事前削除（前のテスト失敗の影響を排除する）
        let _ = nexterm_config::keyring::delete_password(&host.name, &host.username);

        let modal = PasswordModal::new(host.clone());
        assert_eq!(modal.input_len(), 0);
        assert!(!modal.prefilled);
        assert!(!modal.remember);
    }

    #[test]
    fn password_modal_push_pop_と入力長が一貫している() {
        let host = make_test_host_for_modal("__pm_test_input__");
        let _ = nexterm_config::keyring::delete_password(&host.name, &host.username);

        let mut modal = PasswordModal::new(host);
        modal.push_char('a');
        modal.push_char('b');
        modal.push_char('c');
        assert_eq!(modal.input_len(), 3);
        modal.pop_char();
        assert_eq!(modal.input_len(), 2);
    }

    #[test]
    fn password_modal_toggle_remember_でフラグが反転する() {
        let host = make_test_host_for_modal("__pm_test_toggle__");
        let _ = nexterm_config::keyring::delete_password(&host.name, &host.username);

        let mut modal = PasswordModal::new(host);
        assert!(!modal.remember);
        modal.toggle_remember();
        assert!(modal.remember);
        modal.toggle_remember();
        assert!(!modal.remember);
    }

    #[test]
    fn password_modal_take_password_で入力がクリアされる() {
        let host = make_test_host_for_modal("__pm_test_take__");
        let _ = nexterm_config::keyring::delete_password(&host.name, &host.username);

        let mut modal = PasswordModal::new(host);
        modal.push_char('p');
        modal.push_char('w');
        // remember=false なので keyring 副作用なし
        let pw = modal.take_password();
        assert_eq!(&*pw, "pw");
        assert_eq!(modal.input_len(), 0);
    }
}
