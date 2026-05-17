//! コマンドパレット — Ctrl+Shift+P でフローティング UI を表示する
//!
//! Sprint 5-7 / Phase 3-3: 履歴永続化（`palette_history.json`）と「最近使った
//! アクションを優先表示」ロジックを追加。
//!
//! - クエリ空: 履歴の `last_used` 降順 → `use_count` 降順 → 元の登録順
//! - クエリ有: Fuzzy スコア + 履歴ボーナス（最大 +200）でソート
//! - 履歴は `~/.local/state/nexterm/palette_history.json`（Unix）/
//!   `%APPDATA%\nexterm\palette_history.json`（Windows）に atomic write + 0600

use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use nexterm_i18n::fl;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

/// パレットに登録できるアクション
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteAction {
    /// 表示ラベル（現在のロケールで翻訳済み）
    pub label: String,
    /// 実行アクション識別子
    pub action: String,
}

/// 履歴 1 件あたりのエントリ
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteHistoryEntry {
    /// 最終使用時刻（UNIX 秒）
    pub last_used: u64,
    /// 累計使用回数
    pub use_count: u32,
}

/// アクション履歴（`action 識別子` → 履歴エントリ）
pub type PaletteHistory = HashMap<String, PaletteHistoryEntry>;

/// コマンドパレットの状態
pub struct CommandPalette {
    /// 登録済みアクション一覧
    actions: Vec<PaletteAction>,
    /// 現在の検索クエリ
    pub query: String,
    /// パレットが開いているか
    pub is_open: bool,
    /// 選択中のインデックス
    pub selected: usize,
    /// Fuzzy マッチャー
    matcher: SkimMatcherV2,
    /// アクション履歴（永続化対象）
    history: PaletteHistory,
}

impl CommandPalette {
    /// デフォルトアクション付きでパレットを生成する（現在のロケールで翻訳する）
    pub fn new() -> Self {
        let actions = default_actions();
        Self {
            actions,
            query: String::new(),
            is_open: false,
            selected: 0,
            matcher: SkimMatcherV2::default(),
            history: PaletteHistory::new(),
        }
    }

    /// 永続化された履歴をロードしてマージしたパレットを返す。
    ///
    /// 履歴ファイルが存在しない・破損している場合は空履歴で起動する（クラッシュなし）。
    pub fn new_with_history() -> Self {
        let mut palette = Self::new();
        palette.history = load_history();
        palette
    }

    /// パレットを開く
    pub fn open(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.is_open = true;
    }

    /// パレットを閉じる
    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
    }

    /// クエリ文字を追加する
    #[allow(dead_code)]
    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
        self.selected = 0;
    }

    /// クエリの末尾を削除する
    #[allow(dead_code)]
    pub fn pop_char(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// 選択を下に移動する
    pub fn select_next(&mut self) {
        let count = self.filtered().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    /// 選択を上に移動する
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

    /// 現在選択中のアクションを返す
    pub fn selected_action(&self) -> Option<&PaletteAction> {
        self.filtered().into_iter().nth(self.selected)
    }

    /// クエリにマッチするアクションをスコア降順で返す。
    ///
    /// - クエリ空: 履歴順（last_used 降順 → use_count 降順）→ 元の登録順
    /// - クエリ有: Fuzzy スコア + 履歴ボーナスで降順ソート
    pub fn filtered(&self) -> Vec<&PaletteAction> {
        rank_actions(&self.actions, &self.query, &self.history, &self.matcher)
    }

    /// 履歴に「使った」記録を追加し、ファイルへ保存する（Sprint 5-7 / Phase 3-3）。
    pub fn record_use(&mut self, action: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = self
            .history
            .entry(action.to_string())
            .or_insert(PaletteHistoryEntry {
                last_used: now,
                use_count: 0,
            });
        entry.last_used = now;
        entry.use_count = entry.use_count.saturating_add(1);
        save_history(&self.history);
    }

    /// カスタムアクションを登録する
    #[allow(dead_code)]
    pub fn register(&mut self, action: PaletteAction) {
        self.actions.push(action);
    }
}

/// パレットのデフォルトアクション一覧を返す（i18n 翻訳済み）。
///
/// Sprint 5-7 / Phase 3-3: `execute_action` でディスパッチ済みの全アクションを
/// 網羅する（ClosePane / NewWindow / QuickSelect / SetBroadcastOn/Off / Quit を追加）。
fn default_actions() -> Vec<PaletteAction> {
    vec![
        PaletteAction {
            label: fl!("palette-split-vertical"),
            action: "SplitVertical".to_string(),
        },
        PaletteAction {
            label: fl!("palette-split-horizontal"),
            action: "SplitHorizontal".to_string(),
        },
        PaletteAction {
            label: fl!("palette-focus-next"),
            action: "FocusNextPane".to_string(),
        },
        PaletteAction {
            label: fl!("palette-focus-prev"),
            action: "FocusPrevPane".to_string(),
        },
        // Sprint 5-7 / Phase 3-3: 追加分（execute_action にはあったがパレット未登録）
        PaletteAction {
            label: fl!("palette-close-pane"),
            action: "ClosePane".to_string(),
        },
        PaletteAction {
            label: fl!("palette-new-window"),
            action: "NewWindow".to_string(),
        },
        PaletteAction {
            label: fl!("palette-detach"),
            action: "Detach".to_string(),
        },
        PaletteAction {
            label: fl!("palette-search-scrollback"),
            action: "SearchScrollback".to_string(),
        },
        PaletteAction {
            label: fl!("palette-display-panes"),
            action: "DisplayPanes".to_string(),
        },
        PaletteAction {
            label: fl!("palette-toggle-zoom"),
            action: "ToggleZoom".to_string(),
        },
        PaletteAction {
            label: fl!("palette-quick-select"),
            action: "QuickSelect".to_string(),
        },
        PaletteAction {
            label: fl!("palette-swap-pane-next"),
            action: "SwapPaneNext".to_string(),
        },
        PaletteAction {
            label: fl!("palette-swap-pane-prev"),
            action: "SwapPanePrev".to_string(),
        },
        PaletteAction {
            label: fl!("palette-break-pane"),
            action: "BreakPane".to_string(),
        },
        PaletteAction {
            label: fl!("palette-set-broadcast-on"),
            action: "SetBroadcastOn".to_string(),
        },
        PaletteAction {
            label: fl!("palette-set-broadcast-off"),
            action: "SetBroadcastOff".to_string(),
        },
        PaletteAction {
            label: fl!("palette-connect-serial"),
            action: "ConnectSerialPrompt".to_string(),
        },
        PaletteAction {
            label: fl!("palette-show-host-manager"),
            action: "ShowHostManager".to_string(),
        },
        PaletteAction {
            label: fl!("palette-show-macro-picker"),
            action: "ShowMacroPicker".to_string(),
        },
        PaletteAction {
            label: fl!("palette-sftp-upload"),
            action: "SftpUploadDialog".to_string(),
        },
        PaletteAction {
            label: fl!("palette-sftp-download"),
            action: "SftpDownloadDialog".to_string(),
        },
        PaletteAction {
            label: fl!("palette-show-settings"),
            action: "ShowSettings".to_string(),
        },
        // Sprint 5-2 / B1: OSC 133 セマンティックマークによるプロンプトジャンプ
        PaletteAction {
            label: fl!("palette-jump-prev-prompt"),
            action: "JumpPrevPrompt".to_string(),
        },
        PaletteAction {
            label: fl!("palette-jump-next-prompt"),
            action: "JumpNextPrompt".to_string(),
        },
        PaletteAction {
            label: fl!("palette-quit"),
            action: "Quit".to_string(),
        },
    ]
}

/// パレットの並び替えロジックを純関数として切り出したもの（テスト容易性のため）。
///
/// - クエリ空: 履歴順（`last_used` 降順 → `use_count` 降順）→ 履歴なしは末尾に登録順で残す
/// - クエリ有: Fuzzy スコア + `history_bonus` を加算してから降順ソート
///   - `history_bonus` = `use_count * 10 + (last_used が新しいほど +200 まで)`
pub fn rank_actions<'a>(
    actions: &'a [PaletteAction],
    query: &str,
    history: &PaletteHistory,
    matcher: &SkimMatcherV2,
) -> Vec<&'a PaletteAction> {
    if query.is_empty() {
        // クエリ空: 履歴順 → 履歴なしは元順
        let mut indexed: Vec<(usize, &PaletteAction)> = actions.iter().enumerate().collect();
        indexed.sort_by(|a, b| {
            let ha = history.get(&a.1.action);
            let hb = history.get(&b.1.action);
            match (ha, hb) {
                (Some(ea), Some(eb)) => eb
                    .last_used
                    .cmp(&ea.last_used)
                    .then_with(|| eb.use_count.cmp(&ea.use_count))
                    .then_with(|| a.0.cmp(&b.0)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.0.cmp(&b.0),
            }
        });
        return indexed.into_iter().map(|(_, a)| a).collect();
    }

    let mut scored: Vec<(i64, usize, &PaletteAction)> = actions
        .iter()
        .enumerate()
        .filter_map(|(idx, a)| {
            matcher.fuzzy_match(&a.label, query).map(|score| {
                let bonus = history_bonus(history.get(&a.action));
                (score + bonus, idx, a)
            })
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, a)| a).collect()
}

/// 履歴エントリから fuzzy スコアに加算するボーナスを計算する。
///
/// - 使用回数による加点: `use_count * 10`（上限 100）
/// - 最近性による加点: `last_used` が直近（1 日以内）なら +100、1 週間以内なら +50、それ以前は 0
fn history_bonus(entry: Option<&PaletteHistoryEntry>) -> i64 {
    let Some(e) = entry else { return 0 };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let use_bonus = (e.use_count as i64 * 10).min(100);
    let age_secs = now.saturating_sub(e.last_used);
    let recency_bonus = if age_secs < 86_400 {
        100
    } else if age_secs < 86_400 * 7 {
        50
    } else {
        0
    };
    use_bonus + recency_bonus
}

// ---- 履歴の永続化 ----

/// 履歴ファイルパスを返す
///
/// Unix: `~/.local/state/nexterm/palette_history.json`
/// Windows: `%APPDATA%\nexterm\palette_history.json`
fn history_path() -> PathBuf {
    if let Ok(test_path) = std::env::var("__NEXTERM_TEST_PALETTE_HISTORY_PATH__") {
        return PathBuf::from(test_path);
    }

    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("nexterm").join("palette_history.json")
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            return PathBuf::from(xdg)
                .join("nexterm")
                .join("palette_history.json");
        }
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        home.join(".local")
            .join("state")
            .join("nexterm")
            .join("palette_history.json")
    }
}

/// 履歴を JSON ファイルから読み込む（ファイルがなければ空マップを返す）
fn load_history() -> PaletteHistory {
    let path = history_path();
    if !path.exists() {
        return PaletteHistory::new();
    }
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => {
            warn!("コマンドパレット履歴の読み込みに失敗しました: {}", e);
            return PaletteHistory::new();
        }
    };
    match serde_json::from_str(&json) {
        Ok(map) => map,
        Err(e) => {
            warn!("コマンドパレット履歴のパースに失敗しました: {}", e);
            PaletteHistory::new()
        }
    }
}

/// 履歴を JSON ファイルに保存する（atomic write、Unix では 0600）
fn save_history(history: &PaletteHistory) {
    let path = history_path();
    let json = match serde_json::to_string_pretty(history) {
        Ok(j) => j,
        Err(e) => {
            warn!("コマンドパレット履歴のシリアライズに失敗しました: {}", e);
            return;
        }
    };
    if let Err(e) = write_atomic_secure(&path, json.as_bytes()) {
        warn!("コマンドパレット履歴の保存に失敗しました: {}", e);
    }
}

/// ファイルをアトミックに書き込み、Unix では 0600 パーミッションを強制する。
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
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// ロケール変更テストの排他制御（グローバルロケールのレース防止）
    static LOCALE_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn default_actions_exist() {
        let palette = CommandPalette::new();
        assert!(!palette.actions.is_empty());
    }

    #[test]
    fn default_actions_网羅性_新規追加分が含まれる() {
        // Sprint 5-7 / Phase 3-3 で追加した 6 アクションが全て登録されていること
        let palette = CommandPalette::new();
        let ids: Vec<&str> = palette.actions.iter().map(|a| a.action.as_str()).collect();
        for expected in [
            "ClosePane",
            "NewWindow",
            "QuickSelect",
            "SetBroadcastOn",
            "SetBroadcastOff",
            "Quit",
        ] {
            assert!(
                ids.contains(&expected),
                "action {} がパレットに登録されていない",
                expected
            );
        }
    }

    #[test]
    fn no_query_returns_all_actions() {
        let palette = CommandPalette::new();
        assert_eq!(palette.filtered().len(), palette.actions.len());
    }

    #[test]
    fn fuzzy_match_works() {
        // "split" は Split Vertical / Split Horizontal にマッチする（英語ロケール）
        let _guard = LOCALE_MUTEX.lock().unwrap();
        nexterm_i18n::set_locale("en");
        let mut p = CommandPalette::new();
        p.query = "split".to_string();
        let results = p.filtered();
        assert!(results.len() >= 2);
        assert!(results.iter().any(|a| a.action == "SplitVertical"));
        assert!(results.iter().any(|a| a.action == "SplitHorizontal"));
    }

    #[test]
    fn fuzzy_match_works_with_japanese_locale() {
        // 日本語ロケールで "分割" がマッチすることを確認する
        let _guard = LOCALE_MUTEX.lock().unwrap();
        nexterm_i18n::set_locale("ja");
        let mut p = CommandPalette::new();
        p.query = "分割".to_string();
        let results = p.filtered();
        nexterm_i18n::set_locale("en"); // テスト後にリセット
        assert!(results.len() >= 2);
        assert!(results.iter().any(|a| a.action == "SplitVertical"));
        assert!(results.iter().any(|a| a.action == "SplitHorizontal"));
    }

    #[test]
    fn selection_wraps_around() {
        let mut p = CommandPalette::new();
        let total = p.filtered().len();
        // 末尾から次へ → 先頭に戻る
        p.selected = total - 1;
        p.select_next();
        assert_eq!(p.selected, 0);
        // 先頭から前へ → 末尾に戻る
        p.select_prev();
        assert_eq!(p.selected, total - 1);
    }

    #[test]
    fn register_custom_action() {
        let mut p = CommandPalette::new();
        let before = p.actions.len();
        p.register(PaletteAction {
            label: "Custom".to_string(),
            action: "Custom".to_string(),
        });
        assert_eq!(p.actions.len(), before + 1);
    }

    // ---- Sprint 5-7 / Phase 3-3: 履歴ロジックのテスト ----

    fn dummy_actions() -> Vec<PaletteAction> {
        vec![
            PaletteAction {
                label: "Alpha".to_string(),
                action: "Alpha".to_string(),
            },
            PaletteAction {
                label: "Beta".to_string(),
                action: "Beta".to_string(),
            },
            PaletteAction {
                label: "Gamma".to_string(),
                action: "Gamma".to_string(),
            },
        ]
    }

    #[test]
    fn rank_actions_クエリ空_履歴なしは登録順() {
        let actions = dummy_actions();
        let history = PaletteHistory::new();
        let matcher = SkimMatcherV2::default();
        let ranked = rank_actions(&actions, "", &history, &matcher);
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].action, "Alpha");
        assert_eq!(ranked[1].action, "Beta");
        assert_eq!(ranked[2].action, "Gamma");
    }

    #[test]
    fn rank_actions_クエリ空_履歴ありは履歴順() {
        let actions = dummy_actions();
        let mut history = PaletteHistory::new();
        // Beta を最近 1 回、Gamma を昔 5 回使った想定
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        history.insert(
            "Beta".to_string(),
            PaletteHistoryEntry {
                last_used: now,
                use_count: 1,
            },
        );
        history.insert(
            "Gamma".to_string(),
            PaletteHistoryEntry {
                last_used: now - 3600 * 24 * 30, // 30 日前
                use_count: 5,
            },
        );
        let matcher = SkimMatcherV2::default();
        let ranked = rank_actions(&actions, "", &history, &matcher);
        assert_eq!(ranked[0].action, "Beta", "最新使用が先頭");
        assert_eq!(ranked[1].action, "Gamma", "次に履歴のある Gamma");
        assert_eq!(ranked[2].action, "Alpha", "履歴なしは末尾");
    }

    #[test]
    fn rank_actions_クエリ有_履歴ボーナスでブースト() {
        let actions = dummy_actions();
        let matcher = SkimMatcherV2::default();
        // クエリ "a" で Alpha / Gamma がマッチする想定
        let no_hist = PaletteHistory::new();
        let ranked_no = rank_actions(&actions, "a", &no_hist, &matcher);

        let mut hist = PaletteHistory::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Gamma を最近 10 回使った
        hist.insert(
            "Gamma".to_string(),
            PaletteHistoryEntry {
                last_used: now,
                use_count: 10,
            },
        );
        let ranked_h = rank_actions(&actions, "a", &hist, &matcher);

        // 両方とも Gamma を含むが、履歴ありの場合 Gamma が上位に来る
        assert!(ranked_no.iter().any(|a| a.action == "Gamma"));
        assert!(ranked_h.iter().any(|a| a.action == "Gamma"));
        // 履歴ありで Gamma の方が Alpha より上に来ること
        let pos_gamma = ranked_h.iter().position(|a| a.action == "Gamma").unwrap();
        let pos_alpha = ranked_h.iter().position(|a| a.action == "Alpha").unwrap();
        assert!(
            pos_gamma < pos_alpha,
            "履歴あり Gamma が Alpha より上に来るべき (pos_gamma={}, pos_alpha={})",
            pos_gamma,
            pos_alpha
        );
    }

    #[test]
    fn history_bonus_なしエントリは_0() {
        assert_eq!(history_bonus(None), 0);
    }

    #[test]
    fn history_bonus_最近使用_use_count_反映() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entry = PaletteHistoryEntry {
            last_used: now,
            use_count: 3,
        };
        // 1 日以内: recency=100, use_count*10=30 → 130
        assert_eq!(history_bonus(Some(&entry)), 130);
    }

    #[test]
    fn history_bonus_use_count_は_100_でクランプ() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entry = PaletteHistoryEntry {
            last_used: now,
            use_count: 100,
        };
        // use_count*10 = 1000 だが 100 でクランプ + recency 100 = 200
        assert_eq!(history_bonus(Some(&entry)), 200);
    }

    #[test]
    fn history_bonus_古い使用は_recency_ゼロ() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entry = PaletteHistoryEntry {
            last_used: now.saturating_sub(86_400 * 30), // 30 日前
            use_count: 5,
        };
        // recency=0, use_count*10=50 → 50
        assert_eq!(history_bonus(Some(&entry)), 50);
    }

    #[test]
    fn record_use_は_use_count_と_last_used_を更新() {
        // record_use はファイル IO を伴うため、tempdir を用意して環境変数で差し替える
        let tmp = std::env::temp_dir().join(format!(
            "nexterm-test-palette-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // SAFETY: 環境変数の操作は本テスト固有のキーで他テストに干渉しない（パスは uniq）。
        // unsafe 必要なのは Rust 2024 / std 仕様変更による
        unsafe {
            std::env::set_var(
                "__NEXTERM_TEST_PALETTE_HISTORY_PATH__",
                tmp.to_string_lossy().to_string(),
            );
        }

        let mut p = CommandPalette::new();
        p.record_use("Alpha");
        let entry_a = p.history.get("Alpha").expect("Alpha が記録されること");
        assert_eq!(entry_a.use_count, 1);
        let last_used_first = entry_a.last_used;

        // 2 回目: use_count が増えること
        p.record_use("Alpha");
        let entry_a2 = p.history.get("Alpha").unwrap();
        assert_eq!(entry_a2.use_count, 2);
        assert!(entry_a2.last_used >= last_used_first);

        // ファイルが書き込まれていること
        assert!(tmp.exists(), "履歴ファイルが書き込まれていない");

        // クリーンアップ
        let _ = std::fs::remove_file(&tmp);
        unsafe {
            std::env::remove_var("__NEXTERM_TEST_PALETTE_HISTORY_PATH__");
        }
    }
}
