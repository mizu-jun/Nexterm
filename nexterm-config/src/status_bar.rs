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
//!
//! # 実装メモ
//!
//! `LuaWorker` によってバックグラウンドスレッドで Lua を評価する。
//! `evaluate_widgets()` はキャッシュから即座に返すため、winit イベントループを
//! ブロックしない。初回呼び出しは空文字列を返し、次のフレームから結果が表示される。

use crate::loader::lua_path;
use crate::lua_worker::LuaWorker;

/// Lua ウィジェット式を評価してステータスバーテキストを生成する
///
/// 内部の `LuaWorker` がバックグラウンドスレッドで Lua を実行するため、
/// `evaluate_widgets()` はメインスレッドをブロックしない。
pub struct StatusBarEvaluator {
    worker: LuaWorker,
}

impl StatusBarEvaluator {
    /// 評価器を生成する（nexterm.lua が存在すれば読み込む）
    ///
    /// Lua 読み込みエラーは警告ログのみで、パニックしない。
    pub fn new() -> Self {
        let path = lua_path();
        let lua_script_path = if path.exists() { Some(path) } else { None };
        Self {
            worker: LuaWorker::new(lua_script_path),
        }
    }

    /// ウィジェット式リストの評価をリクエストし、キャッシュ済み結果を返す
    ///
    /// - バックグラウンドスレッドが非同期に評価し、結果をキャッシュに書き込む
    /// - 本メソッドはブロックしない（キャッシュを即座に返す）
    /// - 各式の評価エラーは空文字列で置換する（パニックしない）
    /// - 結果は `"  "` で区切って連結する
    pub fn evaluate_widgets(&self, widgets: &[String]) -> String {
        self.worker.eval_widgets(widgets)
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
    use std::time::Duration;

    /// バックグラウンドスレッドの評価完了を待つ
    fn wait_for_eval() {
        std::thread::sleep(Duration::from_millis(150));
    }

    #[test]
    fn lua式を評価できる() {
        let eval = StatusBarEvaluator::new();
        // 最初の呼び出しでリクエストを送信する
        eval.evaluate_widgets(&["\"hello\"".to_string()]);
        wait_for_eval();
        // バックグラウンド評価完了後にキャッシュから結果を取得する
        let result = eval.evaluate_widgets(&["\"hello\"".to_string()]);
        assert_eq!(result, "hello");
    }

    #[test]
    fn 複数ウィジェットをスペース区切りで連結する() {
        let eval = StatusBarEvaluator::new();
        eval.evaluate_widgets(&["\"foo\"".to_string(), "\"bar\"".to_string()]);
        wait_for_eval();
        let result =
            eval.evaluate_widgets(&["\"foo\"".to_string(), "\"bar\"".to_string()]);
        assert_eq!(result, "foo  bar");
    }

    #[test]
    fn 評価エラーは空文字列に置換される() {
        let eval = StatusBarEvaluator::new();
        // 存在しない変数を参照するとエラーになる
        eval.evaluate_widgets(&["undefined_variable_xyz".to_string()]);
        wait_for_eval();
        let result = eval.evaluate_widgets(&["undefined_variable_xyz".to_string()]);
        // エラーでも空文字列が返りパニックしないことを確認する
        assert_eq!(result, "");
    }

    #[test]
    fn 空リストは空文字列を返す() {
        let eval = StatusBarEvaluator::new();
        eval.evaluate_widgets(&[]);
        wait_for_eval();
        let result = eval.evaluate_widgets(&[]);
        assert!(result.is_empty());
    }
}
