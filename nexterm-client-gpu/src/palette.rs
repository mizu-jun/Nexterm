//! コマンドパレット — Ctrl+Shift+P でフローティング UI を表示する

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

/// パレットに登録できるアクション
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteAction {
    /// 表示ラベル
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
    /// デフォルトアクション付きでパレットを生成する
    pub fn new() -> Self {
        let actions = vec![
            PaletteAction {
                label: "垂直分割".to_string(),
                action: "SplitVertical".to_string(),
            },
            PaletteAction {
                label: "水平分割".to_string(),
                action: "SplitHorizontal".to_string(),
            },
            PaletteAction {
                label: "次のペインへ".to_string(),
                action: "FocusNextPane".to_string(),
            },
            PaletteAction {
                label: "前のペインへ".to_string(),
                action: "FocusPrevPane".to_string(),
            },
            PaletteAction {
                label: "デタッチ".to_string(),
                action: "Detach".to_string(),
            },
            PaletteAction {
                label: "スクロールバック検索".to_string(),
                action: "SearchScrollback".to_string(),
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
    fn パレット生成時にデフォルトアクションがある() {
        let palette = CommandPalette::new();
        assert!(!palette.actions.is_empty());
    }

    #[test]
    fn クエリなしで全アクションが返る() {
        let palette = CommandPalette::new();
        assert_eq!(palette.filtered().len(), palette.actions.len());
    }

    #[test]
    fn fuzzyマッチが動作する() {
        let palette = CommandPalette::new();
        // "分割" で SplitVertical / SplitHorizontal がマッチする
        let mut p = CommandPalette::new();
        p.query = "分割".to_string();
        let results = p.filtered();
        assert!(results.len() >= 2);
        assert!(results.iter().any(|a| a.action == "SplitVertical"));
        assert!(results.iter().any(|a| a.action == "SplitHorizontal"));
    }

    #[test]
    fn 選択移動が折り返す() {
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
    fn カスタムアクション登録() {
        let mut p = CommandPalette::new();
        let before = p.actions.len();
        p.register(PaletteAction {
            label: "カスタム".to_string(),
            action: "Custom".to_string(),
        });
        assert_eq!(p.actions.len(), before + 1);
    }
}
