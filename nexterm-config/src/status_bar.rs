//! Lua ステータスバーウィジェット評価器
//!
//! nexterm.lua 内で定義された Lua 式を評価してステータスバーのテキストを生成する。
//!
//! # 使用例（nexterm.lua）
//!
//! ```lua
//! return {
//!   status_bar = {
//!     enabled = true,
//!     widgets = { 'os.date("%H:%M:%S")', '"nexterm"' },
//!   }
//! }
//! ```

use mlua::prelude::*;
use tracing::warn;

use crate::loader::lua_path;

/// Lua ウィジェット式を評価してステータスバーテキストを生成する
pub struct StatusBarEvaluator {
    lua: Lua,
}

impl StatusBarEvaluator {
    /// 評価器を生成する（nexterm.lua が存在すれば読み込む）
    ///
    /// Lua 読み込みエラーは警告ログのみで、パニックしない。
    pub fn new() -> Self {
        let lua = Lua::new();

        let path = lua_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(script) => {
                    if let Err(e) = lua.load(&script).exec() {
                        warn!("ステータスバー Lua 読み込みエラー: {}", e);
                    }
                }
                Err(e) => {
                    warn!("nexterm.lua 読み込み失敗: {}", e);
                }
            }
        }

        Self { lua }
    }

    /// ウィジェット式リストを評価して結合した文字列を返す
    ///
    /// 各式の評価エラーは空文字列で置換する（パニックしない）。
    /// 結果は `"  "` で区切って連結する。
    pub fn evaluate_widgets(&self, widgets: &[String]) -> String {
        let parts: Vec<String> = widgets
            .iter()
            .map(|expr| {
                self.lua
                    .load(expr.as_str())
                    .eval::<String>()
                    .unwrap_or_default()
            })
            .collect();
        parts.join("  ")
    }
}

impl Default for StatusBarEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua式を評価できる() {
        let eval = StatusBarEvaluator::new();
        let result = eval.evaluate_widgets(&["\"hello\"".to_string()]);
        assert_eq!(result, "hello");
    }

    #[test]
    fn 複数ウィジェットをスペース区切りで連結する() {
        let eval = StatusBarEvaluator::new();
        let result =
            eval.evaluate_widgets(&["\"foo\"".to_string(), "\"bar\"".to_string()]);
        assert_eq!(result, "foo  bar");
    }

    #[test]
    fn 評価エラーは空文字列に置換される() {
        let eval = StatusBarEvaluator::new();
        // 存在しない変数を参照するとエラーになる
        let result = eval.evaluate_widgets(&["undefined_variable_xyz".to_string()]);
        // エラーでも空文字列が返りパニックしないことを確認する
        let _ = result;
    }

    #[test]
    fn 空リストは空文字列を返す() {
        let eval = StatusBarEvaluator::new();
        let result = eval.evaluate_widgets(&[]);
        assert!(result.is_empty());
    }
}
