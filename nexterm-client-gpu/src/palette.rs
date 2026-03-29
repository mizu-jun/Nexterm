//! コマンドパレット — Ctrl+Shift+P でフローティング UI を表示する

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use nexterm_i18n::fl;

/// パレットに登録できるアクション
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteAction {
    /// 表示ラベル（現在のロケールで翻訳済み）
    pub label: String,
    /// 実行アクション識別子
    pub action: String,
}

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
}

impl CommandPalette {
    /// デフォルトアクション付きでパレットを生成する（現在のロケールで翻訳する）
    pub fn new() -> Self {
        let actions = vec![
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
        ];

        Self {
            actions,
            query: String::new(),
            is_open: false,
            selected: 0,
            matcher: SkimMatcherV2::default(),
        }
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
    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
        self.selected = 0;
    }

    /// クエリの末尾を削除する
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

    /// クエリにマッチするアクションをスコア降順で返す
    pub fn filtered(&self) -> Vec<&PaletteAction> {
        if self.query.is_empty() {
            return self.actions.iter().collect();
        }

        let mut scored: Vec<(i64, &PaletteAction)> = self
            .actions
            .iter()
            .filter_map(|a| {
                self.matcher
                    .fuzzy_match(&a.label, &self.query)
                    .map(|score| (score, a))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, a)| a).collect()
    }

    /// カスタムアクションを登録する
    pub fn register(&mut self, action: PaletteAction) {
        self.actions.push(action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_actions_exist() {
        let palette = CommandPalette::new();
        assert!(!palette.actions.is_empty());
    }

    #[test]
    fn no_query_returns_all_actions() {
        let palette = CommandPalette::new();
        assert_eq!(palette.filtered().len(), palette.actions.len());
    }

    #[test]
    fn fuzzy_match_works() {
        // "split" は Split Vertical / Split Horizontal にマッチする（英語ロケール）
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
}
