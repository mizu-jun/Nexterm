//! nexterm ローカライズ基盤
//!
//! 起動時に [`init`] を呼び出してシステムロケールを検出し、
//! [`fl`] マクロで翻訳済み文字列を取得する。
//!
//! # 使用例
//!
//! ```rust,no_run
//! nexterm_i18n::init();
//! println!("{}", nexterm_i18n::fl!("ctl-no-sessions"));
//! println!("{}", nexterm_i18n::fl!("ctl-session-created", name = "main"));
//! ```

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

// 翻訳 JSON ファイルをコンパイル時に埋め込む
static LOCALE_DATA: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en.json")),
    ("fr", include_str!("../locales/fr.json")),
    ("de", include_str!("../locales/de.json")),
    ("es", include_str!("../locales/es.json")),
    ("it", include_str!("../locales/it.json")),
    ("zh-CN", include_str!("../locales/zh-CN.json")),
    ("ja", include_str!("../locales/ja.json")),
    ("ko", include_str!("../locales/ko.json")),
];

/// ロケールコード → 翻訳マップ（起動時に一度だけ構築する）
static TRANSLATIONS: LazyLock<HashMap<&'static str, HashMap<String, String>>> =
    LazyLock::new(|| {
        LOCALE_DATA
            .iter()
            .map(|(locale, json)| {
                let map: HashMap<String, String> = serde_json::from_str(json)
                    .unwrap_or_else(|e| {
                        eprintln!("nexterm-i18n: failed to parse locale '{locale}': {e}");
                        HashMap::new()
                    });
                (*locale, map)
            })
            .collect()
    });

/// 現在のロケール（スレッドセーフ）
static CURRENT_LOCALE: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new("en".to_string()));

// ---- 公開 API ----

/// i18n を初期化し、システムのロケールを検出して適用する。
///
/// 優先順位:
/// 1. `NEXTERM_LANG` 環境変数 (例: `NEXTERM_LANG=ja`)
/// 2. OS 標準ロケール (`sys-locale` 経由)
/// 3. フォールバック: `en`
pub fn init() {
    let detected = if let Ok(lang) = std::env::var("NEXTERM_LANG") {
        if !lang.is_empty() {
            lang
        } else {
            detect_os_locale()
        }
    } else {
        detect_os_locale()
    };
    set_locale(&detected);
}

/// ロケールを手動で設定する（テストやオーバーライド用）。
pub fn set_locale(locale: &str) {
    let normalized = normalize_locale(locale);
    if let Ok(mut current) = CURRENT_LOCALE.write() {
        *current = normalized;
    }
}

/// 現在のロケールコードを返す。
pub fn locale() -> String {
    CURRENT_LOCALE
        .read()
        .map(|g| g.clone())
        .unwrap_or_else(|_| "en".to_string())
}

/// キーを翻訳する。
///
/// 現在のロケールで見つからない場合は `en` にフォールバックし、
/// それも失敗した場合はキーをそのまま返す。
pub fn t(key: &str) -> String {
    let loc = CURRENT_LOCALE
        .read()
        .map(|g| g.clone())
        .unwrap_or_else(|_| "en".to_string());

    TRANSLATIONS
        .get(loc.as_str())
        .and_then(|m| m.get(key))
        .or_else(|| TRANSLATIONS.get("en").and_then(|m| m.get(key)))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// 変数を含むキーを翻訳する。`{name}` 形式のプレースホルダーを置換する。
pub fn t_args(key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut result = t(key);
    for (name, value) in args {
        result = result.replace(&format!("{{{}}}", name), &value.to_string());
    }
    result
}

// ---- マクロ ----

/// 翻訳マクロ。
///
/// - `fl!("key")` — 引数なし
/// - `fl!("key", name = value, other = val2)` — 変数補間（`{name}` プレースホルダー）
#[macro_export]
macro_rules! fl {
    ($key:literal) => {
        $crate::t($key)
    };
    ($key:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::t_args(
            $key,
            &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),+],
        )
    };
}

// ---- 内部ヘルパー ----

/// OS のロケールを取得する。取得できない場合は `"en"` を返す。
fn detect_os_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en".to_string())
}

/// ロケール文字列を nexterm がサポートする形式に正規化する。
///
/// - `"ja-JP"` → `"ja"`
/// - `"zh-Hans-CN"` / `"zh-CN"` → `"zh-CN"`
/// - `"zh-TW"` → `"en"` (繁体字は未サポートのため英語フォールバック)
/// - 未サポートのロケール → `"en"`
fn normalize_locale(locale: &str) -> String {
    let parts: Vec<&str> = locale.splitn(3, ['-', '_']).collect();
    let lang = parts[0].to_lowercase();
    let region = parts.get(1).map(|s| s.to_uppercase());

    match lang.as_str() {
        "zh" => match region.as_deref() {
            Some("CN") | Some("HANS") | Some("SG") => "zh-CN".to_string(),
            _ => "en".to_string(),
        },
        "en" | "fr" | "de" | "es" | "it" | "ja" | "ko" => lang,
        _ => "en".to_string(),
    }
}

// ---- テスト ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_t_fallback_to_key() {
        // 存在しないキーはキー自体を返す
        let result = t("nonexistent-key-xyz");
        assert_eq!(result, "nonexistent-key-xyz");
    }

    #[test]
    fn test_t_en_translation() {
        set_locale("en");
        let result = t("ctl-no-sessions");
        assert!(!result.is_empty());
        assert_ne!(result, "ctl-no-sessions");
    }

    #[test]
    fn test_t_args_interpolation() {
        set_locale("en");
        let result = t_args("ctl-session-created", &[("name", &"test" as &dyn std::fmt::Display)]);
        assert!(result.contains("test"));
    }

    #[test]
    fn test_normalize_locale() {
        assert_eq!(normalize_locale("ja-JP"), "ja");
        assert_eq!(normalize_locale("zh-CN"), "zh-CN");
        assert_eq!(normalize_locale("zh-Hans"), "zh-CN");
        assert_eq!(normalize_locale("zh-TW"), "en");
        assert_eq!(normalize_locale("fr-FR"), "fr");
        assert_eq!(normalize_locale("de-DE"), "de");
        assert_eq!(normalize_locale("pt-BR"), "en"); // 未サポート
    }

    #[test]
    fn test_set_and_get_locale() {
        set_locale("ja");
        assert_eq!(locale(), "ja");
        set_locale("en"); // テスト後にリセット
    }

    #[test]
    fn test_fl_macro_no_args() {
        set_locale("en");
        let result = fl!("ctl-no-sessions");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_fl_macro_with_args() {
        set_locale("en");
        let name = "my-session";
        let result = fl!("ctl-session-created", name = name);
        assert!(result.contains("my-session"));
    }

    #[test]
    fn test_all_locales_parse() {
        // 全ロケールの JSON が正しくパースできることを確認する
        let translations = &*TRANSLATIONS;
        assert!(translations.contains_key("en"));
        assert!(translations.contains_key("ja"));
        assert!(translations.contains_key("fr"));
        assert!(translations.contains_key("de"));
        assert!(translations.contains_key("es"));
        assert!(translations.contains_key("it"));
        assert!(translations.contains_key("zh-CN"));
        assert!(translations.contains_key("ko"));
    }
}
