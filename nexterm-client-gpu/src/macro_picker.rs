//! Lua マクロピッカー UI — Ctrl+Shift+M でフローティングリストを表示する
//!
//! 設定ファイルの `[[macros]]` エントリを一覧表示し、
//! Enter で選択したマクロを実行する（RunMacro メッセージをサーバーに送信する）。

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use nexterm_config::MacroConfig;

/// マクロピッカーの表示/操作状態
pub struct MacroPicker {
    /// 登録済みマクロ一覧（設定ファイルから読み込む）
    macros: Vec<MacroConfig>,
    /// 現在の検索クエリ
    pub query: String,
    /// パネルが開いているか
    pub is_open: bool,
    /// 選択中のインデックス（フィルタ後リスト上）
    pub selected: usize,
    /// Fuzzy マッチャー
    matcher: SkimMatcherV2,
}

impl MacroPicker {
    /// 設定からマクロ一覧を受け取ってピッカーを生成する
    pub fn new(macros: Vec<MacroConfig>) -> Self {
        Self {
            macros,
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

    /// 現在選択中のマクロ設定を返す
    pub fn selected_macro(&self) -> Option<&MacroConfig> {
        self.filtered().into_iter().nth(self.selected)
    }

    /// クエリにマッチするマクロをスコア降順で返す
    pub fn filtered(&self) -> Vec<&MacroConfig> {
        if self.query.is_empty() {
            return self.macros.iter().collect();
        }

        let mut scored: Vec<(i64, &MacroConfig)> = self
            .macros
            .iter()
            .filter_map(|m| {
                let haystack = format!("{} {}", m.name, m.description);
                self.matcher
                    .fuzzy_match(&haystack, &self.query)
                    .map(|score| (score, m))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, m)| m).collect()
    }

    /// マクロ一覧を更新する（設定ファイルリロード時に使用）
    pub fn reload(&mut self, macros: Vec<MacroConfig>) {
        self.macros = macros;
        self.selected = 0;
    }
}
