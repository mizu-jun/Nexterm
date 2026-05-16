//! メニュー / ダイアログ系 — コンテキストメニュー、ファイル転送、Quick Select
//!
//! `state/mod.rs` から抽出した:
//! - `ContextMenuAction` / `ContextMenuItem` / `ContextMenu` — 右クリックメニュー
//! - `FileTransferDialog` — SFTP アップロード / ダウンロードダイアログ
//! - `QuickSelectMatch` / `QuickSelectState` — グリッド上の URL / Email / Path 等を
//!   ラベル付きでハイライトして高速選択する Quick Select モード
//! - `find_quick_select_matches` — 正規表現でグリッド全体からマッチを抽出（優先度重複制御あり）

// ---- コンテキストメニュー ----

/// コンテキストメニューの各項目が実行するアクション
#[derive(Debug, Clone, PartialEq)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    SelectAll,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    InlineSearch,
    OpenSettings,
    /// プロファイル名を指定してシェルを開く
    OpenProfile {
        profile_name: String,
    },
    /// セパレーター（クリック不可）
    Separator,
}

/// コンテキストメニューの1項目
#[derive(Debug, Clone)]
pub struct ContextMenuItem {
    pub label: String,
    /// キーヒント（右端に薄く表示）
    pub hint: String,
    pub action: ContextMenuAction,
}

impl ContextMenuItem {
    fn new(label: impl Into<String>, action: ContextMenuAction) -> Self {
        Self {
            label: label.into(),
            hint: String::new(),
            action,
        }
    }

    fn with_hint(
        label: impl Into<String>,
        hint: impl Into<String>,
        action: ContextMenuAction,
    ) -> Self {
        Self {
            label: label.into(),
            hint: hint.into(),
            action,
        }
    }

    fn separator() -> Self {
        Self {
            label: String::new(),
            hint: String::new(),
            action: ContextMenuAction::Separator,
        }
    }
}

/// 右クリックで表示するコンテキストメニュー
#[derive(Debug, Clone)]
pub struct ContextMenu {
    /// メニューを表示するピクセル座標（左上）
    pub x: f32,
    pub y: f32,
    pub items: Vec<ContextMenuItem>,
    /// 現在ホバー中の項目インデックス
    pub hovered: Option<usize>,
}

impl ContextMenu {
    /// 標準メニュー項目を持つコンテキストメニューを生成する
    /// profiles: プロファイル名とアイコンのペア一覧
    pub fn new_default(x: f32, y: f32, profiles: &[(String, String)]) -> Self {
        let mut items = vec![
            ContextMenuItem::with_hint("コピー", "Ctrl+C", ContextMenuAction::Copy),
            ContextMenuItem::with_hint("貼り付け", "Ctrl+V", ContextMenuAction::Paste),
            ContextMenuItem::with_hint("すべて選択", "Ctrl+A", ContextMenuAction::SelectAll),
            ContextMenuItem::separator(),
            ContextMenuItem::with_hint("垂直分割", "Ctrl+B  %", ContextMenuAction::SplitVertical),
            ContextMenuItem::with_hint(
                "水平分割",
                "Ctrl+B  \"",
                ContextMenuAction::SplitHorizontal,
            ),
            ContextMenuItem::with_hint("ペインを閉じる", "Ctrl+B  x", ContextMenuAction::ClosePane),
        ];

        // プロファイルが登録されていればサブセクションを追加する
        if !profiles.is_empty() {
            items.push(ContextMenuItem::separator());
            for (name, icon) in profiles {
                let label = if icon.is_empty() {
                    format!("> {}", name)
                } else {
                    format!("{} {}", icon, name)
                };
                items.push(ContextMenuItem::new(
                    label,
                    ContextMenuAction::OpenProfile {
                        profile_name: name.clone(),
                    },
                ));
            }
        }

        items.push(ContextMenuItem::separator());
        items.push(ContextMenuItem::with_hint(
            "検索...",
            "Ctrl+F",
            ContextMenuAction::InlineSearch,
        ));
        items.push(ContextMenuItem::with_hint(
            "設定...",
            "Ctrl+,",
            ContextMenuAction::OpenSettings,
        ));

        Self {
            x,
            y,
            items,
            hovered: None,
        }
    }
}

// ---- ファイル転送ダイアログ ----

/// ファイル転送ダイアログの状態
pub struct FileTransferDialog {
    pub is_open: bool,
    /// "upload" または "download"
    pub mode: String,
    /// 入力フィールドのインデックス（0 = ホスト名, 1 = ローカルパス, 2 = リモートパス）
    pub field: usize,
    pub host_name: String,
    pub local_path: String,
    pub remote_path: String,
}

impl FileTransferDialog {
    pub fn new() -> Self {
        Self {
            is_open: false,
            mode: "upload".to_string(),
            field: 0,
            host_name: String::new(),
            local_path: String::new(),
            remote_path: String::new(),
        }
    }

    pub fn open_upload(&mut self) {
        self.mode = "upload".to_string();
        self.field = 0;
        self.host_name.clear();
        self.local_path.clear();
        self.remote_path.clear();
        self.is_open = true;
    }

    pub fn open_download(&mut self) {
        self.mode = "download".to_string();
        self.field = 0;
        self.host_name.clear();
        self.local_path.clear();
        self.remote_path.clear();
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn current_field_mut(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.host_name,
            1 => &mut self.local_path,
            _ => &mut self.remote_path,
        }
    }

    pub fn next_field(&mut self) {
        self.field = (self.field + 1).min(2);
    }

    pub fn prev_field(&mut self) {
        self.field = self.field.saturating_sub(1);
    }
}

// ---- Quick Select ----

/// Quick Select モードのマッチ結果
#[derive(Debug, Clone)]
pub struct QuickSelectMatch {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub text: String,
    /// 選択ラベル（a, b, c, ... / aa, ab, ...）
    pub label: String,
}

/// Quick Select モードの状態
pub struct QuickSelectState {
    pub is_active: bool,
    pub matches: Vec<QuickSelectMatch>,
    /// 現在タイプ中のラベル
    pub typed_label: String,
}

impl QuickSelectState {
    pub(super) fn new() -> Self {
        Self {
            is_active: false,
            matches: Vec::new(),
            typed_label: String::new(),
        }
    }

    pub fn enter(&mut self, grid_rows: &[Vec<nexterm_proto::Cell>]) {
        self.is_active = true;
        self.typed_label.clear();
        self.matches = find_quick_select_matches(grid_rows);
    }

    pub fn exit(&mut self) {
        self.is_active = false;
        self.matches.clear();
        self.typed_label.clear();
    }

    /// タイプされたラベルが一致するマッチを返す
    pub fn accept(&self) -> Option<&QuickSelectMatch> {
        if self.typed_label.is_empty() {
            return None;
        }
        self.matches.iter().find(|m| m.label == self.typed_label)
    }
}

/// グリッドから Quick Select マッチを検索する。
///
/// パターンは Sprint 5-4 / D1 で拡充済み。マッチ範囲が重複した場合は、
/// 先頭にあるパターン（より具体的なもの）を優先して残す。
pub(super) fn find_quick_select_matches(
    rows: &[Vec<nexterm_proto::Cell>],
) -> Vec<QuickSelectMatch> {
    // 優先順位順（先頭ほど高優先）:
    //   1. URL（必ず先に取り、後段の path/IPv4 にマッチを奪われないようにする）
    //   2. Email
    //   3. UUID
    //   4. file:line:col 形式（行番号付き、エディタジャンプ用）
    //   5. Jira チケット (`PROJ-123`)
    //   6. Unix path
    //   7. Windows path (`C:\foo\bar`)
    //   8. IPv4 / IPv6
    //   9. SHA / Git ハッシュ
    //  10. 単独数字（最後 — 他のどれにも引っかからなかったもののみ）
    let patterns: &[&str] = &[
        // URL (http/https/ftp)
        r#"\b(?:https?|ftp)://[^\s<>"'\]]+"#,
        // Email
        r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b",
        // UUID v1〜v5 (8-4-4-4-12 hex)
        r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
        // file:line[:col] 形式 (例: src/main.rs:42 or src/main.rs:42:10)
        r"[A-Za-z0-9_./\\-]+\.[A-Za-z0-9]+:\d+(?::\d+)?",
        // Jira / 課題チケット ID (例: PROJ-123, ABC-9999)
        r"\b[A-Z][A-Z0-9]{1,9}-\d+\b",
        // Unix パス
        r"(?:^|[\s(])((?:/[^\s/:]+)+/?)",
        // Windows パス (例: C:\foo\bar)
        r#"\b[A-Za-z]:\\[^\s<>:"|?*]+"#,
        // IPv4 アドレス（ポート省略可）
        r"\b(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?\b",
        // IPv6 アドレス（簡易: 16 進グループ 2 個以上 + コロン区切り）
        r"\b(?:[0-9a-fA-F]{1,4}:){2,7}[0-9a-fA-F]{1,4}\b",
        // SHA / Git ハッシュ (7-40 hex)
        r"\b[0-9a-f]{7,40}\b",
        // 単独数字
        r"\b\d+\b",
    ];

    // 正規表現は一度だけコンパイルする（パターン件数 × 行数の重複コンパイルを防ぐ）
    let compiled: Vec<regex::Regex> = patterns
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect();

    let mut all_matches: Vec<QuickSelectMatch> = Vec::new();

    for (row_idx, cells) in rows.iter().enumerate() {
        let line: String = cells.iter().map(|c| c.ch).collect();
        // 行ごとに「占有済み列範囲」を管理し、優先順位の高いパターンが取った範囲を
        // 後段のパターンが奪わないようにする。
        let mut occupied: Vec<(usize, usize)> = Vec::new();

        for re in &compiled {
            for m in re.find_iter(&line) {
                let (start, end) = (m.start(), m.end());
                // 既存マッチと重複したらスキップ（より高優先のパターンを優先）
                let overlaps = occupied.iter().any(|(s, e)| !(end <= *s || start >= *e));
                if overlaps {
                    continue;
                }
                occupied.push((start, end));
                all_matches.push(QuickSelectMatch {
                    row: row_idx as u16,
                    col_start: start as u16,
                    col_end: end as u16,
                    text: m.as_str().to_string(),
                    label: String::new(), // 後段で割り当てる
                });
            }
        }
    }

    // ラベルを割り当てる（a, b, ..., z, aa, ab, ...）
    let label_chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
    let n = all_matches.len();
    for (i, m) in all_matches.iter_mut().enumerate() {
        m.label = index_to_label(i, n, &label_chars);
    }

    all_matches
}

fn index_to_label(i: usize, total: usize, chars: &[char]) -> String {
    let base = chars.len();
    if total <= base {
        return chars[i % base].to_string();
    }
    let second = i / base;
    let first = i % base;
    if second == 0 {
        chars[first].to_string()
    } else {
        format!("{}{}", chars[second - 1], chars[first])
    }
}
