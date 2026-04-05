//! ステータスバーウィジェット評価器
//!
//! ウィジェットは以下の 2 種類をサポートする：
//!
//! 1. **ビルトインキーワード** — Rust ネイティブで高速に評価する
//!    - `"time"` → `HH:MM:SS`
//!    - `"date"` → `YYYY-MM-DD`
//!    - `"hostname"` → システムのホスト名
//!    - `"session"` → 現在のセッション名（IPC から受信した値）
//!    - `"pane_id"` → フォーカスペインの ID
//!
//! 2. **Lua 式** — バックグラウンドスレッドで評価する
//!    - `'os.date("%H:%M")'` → Lua の `os.date` を実行した結果
//!    - `'"custom text"'` → 文字列リテラル
//!
//! # 設定例（nexterm.lua）
//!
//! ```lua
//! return {
//!   status_bar = {
//!     enabled = true,
//!     widgets = { "session", "pane_id" },
//!     right_widgets = { "time" },
//!   }
//! }
//! ```

use crate::loader::lua_path;
use crate::lua_worker::LuaWorker;

// ---- ビルトインウィジェット評価 -------------------------------------------

/// 現在のコンテキスト（セッション名・ペイン ID）
///
/// `evaluate_builtin` に渡すことで動的なビルトインウィジェットを評価できる。
#[derive(Debug, Clone, Default)]
pub struct WidgetContext {
    /// 現在のセッション名
    pub session_name: Option<String>,
    /// フォーカス中のペイン ID
    pub pane_id: Option<u32>,
}

/// ビルトインウィジェットキーワードを評価する
///
/// 未知のキーワード（Lua 式）は `None` を返す。
pub fn evaluate_builtin(keyword: &str, ctx: &WidgetContext) -> Option<String> {
    match keyword {
        "time" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let secs = now.as_secs();
            // UTC 秒から HH:MM:SS を計算する（libc 不使用のポータブル実装）
            let hms = secs % 86400;
            let h = hms / 3600;
            let m = (hms % 3600) / 60;
            let s = hms % 60;
            Some(format!("{:02}:{:02}:{:02}", h, m, s))
        }
        "date" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            // UTC の日付を計算する（うるう年を正確に処理）
            let days = now.as_secs() / 86400;
            let (y, mo, d) = days_to_ymd(days);
            Some(format!("{:04}-{:02}-{:02}", y, mo, d))
        }
        "hostname" => {
            // 環境変数 HOSTNAME を試み、なければ fallback する
            let name = std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "localhost".to_string());
            Some(name)
        }
        "session" => Some(ctx.session_name.clone().unwrap_or_else(|| "—".to_string())),
        "pane_id" => Some(
            ctx.pane_id
                .map(|id| format!("pane:{}", id))
                .unwrap_or_else(|| "pane:—".to_string()),
        ),
        _ => None, // Lua 式として扱う
    }
}

/// エポック日数 (1970-01-01 = 0) を (year, month, day) に変換する
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // 400年サイクル = 146097 日
    let years400 = days / 146097;
    days %= 146097;
    let years100 = (days / 36524).min(3);
    days -= years100 * 36524;
    let years4 = days / 1461;
    days %= 1461;
    let years1 = (days / 365).min(3);
    days -= years1 * 365;

    let year = years400 * 400 + years100 * 100 + years4 * 4 + years1 + 1970;

    // 月ごとの日数（うるう年を考慮）
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }

    (year, month, days + 1)
}

// ---- Lua ステータスバーウィジェット評価器 ----------------------------------

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

    /// ウィジェットリストを評価して区切り文字で連結した文字列を返す
    ///
    /// - ビルトインキーワードはネイティブで評価する（ブロックなし）
    /// - Lua 式はバックグラウンドスレッドで評価する（ブロックなし）
    /// - 各式の評価エラーは空文字列で置換する
    pub fn evaluate_widgets(&self, widgets: &[String]) -> String {
        self.evaluate_with_context(widgets, &WidgetContext::default(), "  ")
    }

    /// コンテキストと区切り文字を指定して評価する
    pub fn evaluate_with_context(
        &self,
        widgets: &[String],
        ctx: &WidgetContext,
        separator: &str,
    ) -> String {
        if widgets.is_empty() {
            return String::new();
        }

        // ビルトインキーワードと Lua 式を分離する
        let mut lua_exprs: Vec<String> = Vec::new();
        let mut has_lua = false;
        for w in widgets {
            if evaluate_builtin(w, ctx).is_none() {
                lua_exprs.push(w.clone());
                has_lua = true;
            }
        }

        // Lua 式をバックグラウンドで評価する（キャッシュ更新のみ）
        if has_lua {
            self.worker.eval_widgets(&lua_exprs);
        }

        // 結果を構築する
        let mut parts: Vec<String> = Vec::with_capacity(widgets.len());
        let mut lua_idx = 0usize;
        for w in widgets {
            if let Some(builtin) = evaluate_builtin(w, ctx) {
                if !builtin.is_empty() {
                    parts.push(builtin);
                }
            } else {
                // Lua 式のキャッシュ済み結果を取得する（lua_idx 番目）
                let result = self.worker.eval_widgets(&lua_exprs);
                // eval_widgets は全式を連結して返すが、個別取得が必要なので
                // worker から個別に取得できるようにする（既存 API の制約で全体を取得）
                // 簡単化のため全 Lua 式を連結した値を使う
                let _ = lua_idx;
                let _ = result;
                lua_idx += 1;
            }
        }

        // Lua 式が単独の場合は worker の出力をそのまま使う
        if widgets.len() == 1 && has_lua {
            return self.worker.eval_widgets(widgets);
        }

        // 混在の場合: ビルトイン部分のみを繋いで Lua 部分を末尾に付加する
        // TODO: ウィジェット個別の Lua 評価（現 API では全体連結のみ対応）
        let lua_part = if has_lua {
            self.worker.eval_widgets(&lua_exprs)
        } else {
            String::new()
        };

        let mut all_parts: Vec<String> = parts;
        if !lua_part.is_empty() {
            all_parts.push(lua_part);
        }

        all_parts.join(separator)
    }
}

impl Default for StatusBarEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

// ---- テスト ---------------------------------------------------------------

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
        let result = eval.evaluate_widgets(&["\"foo\"".to_string(), "\"bar\"".to_string()]);
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

    #[test]
    fn ビルトインtime_はhh_mm_ss形式を返す() {
        let ctx = WidgetContext::default();
        let result = evaluate_builtin("time", &ctx).unwrap();
        // HH:MM:SS 形式であること
        assert_eq!(result.len(), 8);
        assert_eq!(&result[2..3], ":");
        assert_eq!(&result[5..6], ":");
    }

    #[test]
    fn ビルトインdate_はyyyy_mm_dd形式を返す() {
        let ctx = WidgetContext::default();
        let result = evaluate_builtin("date", &ctx).unwrap();
        // YYYY-MM-DD 形式であること
        assert_eq!(result.len(), 10);
        assert_eq!(&result[4..5], "-");
        assert_eq!(&result[7..8], "-");
    }

    #[test]
    fn ビルトインhostname_は空でない文字列を返す() {
        let ctx = WidgetContext::default();
        let result = evaluate_builtin("hostname", &ctx).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn ビルトインsession_はコンテキストのセッション名を返す() {
        let ctx = WidgetContext {
            session_name: Some("my-session".to_string()),
            pane_id: None,
        };
        assert_eq!(evaluate_builtin("session", &ctx).unwrap(), "my-session");
    }

    #[test]
    fn ビルトインpane_id_はフォーカスペイン番号を返す() {
        let ctx = WidgetContext {
            session_name: None,
            pane_id: Some(42),
        };
        assert_eq!(evaluate_builtin("pane_id", &ctx).unwrap(), "pane:42");
    }

    #[test]
    fn 未知キーワードはnoneを返す() {
        let ctx = WidgetContext::default();
        assert!(evaluate_builtin("unknown_widget", &ctx).is_none());
    }

    #[test]
    fn days_to_ymd_epoch_is_1970_01_01() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 = 1970-01-01 から 19723 日後
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!(y, 2024);
        assert_eq!(m, 1);
        assert_eq!(d, 1);
    }
}
