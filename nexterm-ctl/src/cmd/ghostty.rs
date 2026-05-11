//! Ghostty 設定インポートコマンド + パーサ。

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::cmd::util::remove_toml_section;

/// Ghostty 設定ファイルを読み込んで nexterm の config.toml に変換する
pub(crate) fn cmd_import_ghostty(path: Option<String>, output: Option<String>) -> Result<()> {
    // 入力パスのデフォルト: ~/.config/ghostty/config
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());

    let input_path = path.unwrap_or_else(|| format!("{}/.config/ghostty/config", home));

    if !Path::new(&input_path).exists() {
        bail!(
            "Ghostty 設定ファイルが見つかりません: {}\n\
             パスを明示的に指定してください: nexterm-ctl import-ghostty <path>",
            input_path
        );
    }

    let content = std::fs::read_to_string(&input_path).with_context(|| {
        format!(
            "Ghostty 設定ファイルの読み込みに失敗しました: {}",
            input_path
        )
    })?;

    let converted = parse_ghostty_config(&content)?;

    // 出力パスのデフォルト: ~/.config/nexterm/config.toml
    let output_path = output.unwrap_or_else(|| format!("{}/.config/nexterm/config.toml", home));

    // 出力ディレクトリを作成する
    if let Some(parent) = Path::new(&output_path).parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("ディレクトリの作成に失敗しました: {}", parent.display()))?;
    }

    // 既存の config.toml に Ghostty から変換した設定をマージする
    let existing = if Path::new(&output_path).exists() {
        std::fs::read_to_string(&output_path)
            .with_context(|| format!("既存設定ファイルの読み込みに失敗しました: {}", output_path))?
    } else {
        String::new()
    };

    let merged = merge_ghostty_config(&existing, &converted);

    std::fs::write(&output_path, &merged)
        .with_context(|| format!("設定ファイルの書き込みに失敗しました: {}", output_path))?;

    println!("Ghostty 設定をインポートしました");
    println!("  入力: {}", input_path);
    println!("  出力: {}", output_path);
    if !converted.notes.is_empty() {
        println!("\n変換メモ（手動確認が必要な項目）:");
        for note in &converted.notes {
            println!("  ⚠ {}", note);
        }
    }

    Ok(())
}

/// Ghostty 設定の変換結果
struct GhosttyConverted {
    /// [font] セクションの TOML フラグメント
    font_toml: Option<String>,
    /// [color-scheme.custom] セクションの TOML フラグメント（パレット設定時）
    palette_toml: Option<String>,
    /// [window] セクションの TOML フラグメント
    window_toml: Option<String>,
    /// 手動確認が必要な項目
    notes: Vec<String>,
}

/// Ghostty の設定ファイルをパースして nexterm 互換の設定に変換する
fn parse_ghostty_config(content: &str) -> Result<GhosttyConverted> {
    // Ghostty の設定フォーマット: `key = value` （TOML に近いが独自形式）
    let mut font_family: Option<String> = None;
    let mut font_size: Option<f32> = None;
    let mut background: Option<String> = None;
    let mut foreground: Option<String> = None;
    let mut cursor_color: Option<String> = None;
    let mut background_opacity: Option<f32> = None;
    let mut ansi: Vec<Option<String>> = vec![None; 16];
    let mut notes = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        // コメント行とブランク行をスキップ
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // `key = value` を分割する
        let Some(eq_pos) = trimmed.find('=') else {
            continue;
        };
        let key = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..]
            .trim()
            .trim_matches('"')
            .trim_matches('\'');

        match key {
            "font-family" => font_family = Some(value.to_string()),
            "font-size" => font_size = value.parse::<f32>().ok(),
            "background" => background = Some(normalize_color(value)),
            "foreground" => foreground = Some(normalize_color(value)),
            "cursor-color" => cursor_color = Some(normalize_color(value)),
            "background-opacity" => background_opacity = value.parse::<f32>().ok(),
            // ANSI パレット: palette = N=#RRGGBB 形式
            "palette" => {
                if let Some((idx_str, color)) = value.split_once('=')
                    && let Ok(idx) = idx_str.trim().parse::<usize>()
                    && idx < 16
                {
                    ansi[idx] = Some(normalize_color(color.trim()));
                }
            }
            // 未対応キーはメモに追記する
            "theme" => notes.push(format!(
                "theme = \"{}\" は手動で nexterm の color-scheme に変換してください",
                value
            )),
            "keybind" => notes.push(format!(
                "keybind = {} は nexterm の [keybindings] に手動でマッピングしてください",
                value
            )),
            "shell-integration" | "shell-integration-features" => {
                notes.push(format!("{} は nexterm では自動的に統合されます", key))
            }
            "window-decoration" => {
                // Ghostty の window-decoration → nexterm の window.decorations
                // "false" = none, "true"/"client"/"server" = full
            }
            _ => {
                // 重要そうなキーのみメモ（細かいものは無視）
                if !matches!(
                    key,
                    "cursor-style"
                        | "cursor-style-blink"
                        | "scrollback-limit"
                        | "clipboard-read"
                        | "clipboard-write"
                        | "mouse-hide-while-typing"
                ) && !key.starts_with("gtk-")
                    && !key.starts_with("macos-")
                    && !key.starts_with("linux-")
                    && !key.starts_with("windows-")
                {
                    // 未対応のキーは無視（警告しすぎるとユーザーが混乱する）
                }
            }
        }
    }

    // [font] セクションの生成
    let font_toml = if font_family.is_some() || font_size.is_some() {
        let mut s = String::from("[font]\n");
        if let Some(family) = &font_family {
            s.push_str(&format!("family = \"{}\"\n", family));
        }
        if let Some(size) = font_size {
            s.push_str(&format!("size = {}\n", size));
        }
        Some(s)
    } else {
        None
    };

    // [color-scheme.custom] セクションの生成
    let palette_toml = if background.is_some()
        || foreground.is_some()
        || ansi.iter().any(|a| a.is_some())
    {
        let bg = background.clone().unwrap_or_else(|| "#1d1f21".to_string());
        let fg = foreground.clone().unwrap_or_else(|| "#c5c8c6".to_string());
        let cur = cursor_color.clone().unwrap_or_else(|| fg.clone());
        let ansi_arr: Vec<String> = ansi
            .iter()
            .enumerate()
            .map(|(i, a)| {
                a.clone().unwrap_or_else(|| {
                    // デフォルト ANSI カラー
                    DEFAULT_ANSI_COLORS[i % 16].to_string()
                })
            })
            .collect();
        let ansi_str = ansi_arr
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "[color-scheme.custom]\nforeground = \"{}\"\nbackground = \"{}\"\ncursor = \"{}\"\nansi = [{}]\n",
            fg, bg, cur, ansi_str
        ))
    } else {
        None
    };

    // [window] セクションの生成
    let window_toml = background_opacity
        .map(|opacity| format!("[window]\nbackground_opacity = {:.2}\n", opacity));

    Ok(GhosttyConverted {
        font_toml,
        palette_toml,
        window_toml,
        notes,
    })
}

/// カラー文字列を正規化する（"RRGGBB" → "#RRGGBB"、既に "#" がある場合はそのまま）
fn normalize_color(s: &str) -> String {
    let s = s.trim_matches('"').trim_matches('\'');
    if s.starts_with('#') {
        s.to_uppercase()
    } else {
        format!("#{}", s.to_uppercase())
    }
}

/// デフォルト ANSI 16色（フォールバック用）
const DEFAULT_ANSI_COLORS: &[&str] = &[
    "#2E3440", "#BF616A", "#A3BE8C", "#EBCB8B", "#81A1C1", "#B48EAD", "#88C0D0", "#E5E9F0",
    "#4C566A", "#BF616A", "#A3BE8C", "#EBCB8B", "#81A1C1", "#B48EAD", "#8FBCBB", "#ECEFF4",
];

/// 既存の config.toml に Ghostty から変換した設定をマージする
///
/// 各セクションが既に存在する場合は上書き、存在しない場合は末尾に追加する。
fn merge_ghostty_config(existing: &str, converted: &GhosttyConverted) -> String {
    let mut result = existing.to_string();

    if let Some(font) = &converted.font_toml {
        result = remove_toml_section(&result, "font");
        result = format!("{}\n{}", result.trim_end(), font);
    }

    if let Some(palette) = &converted.palette_toml {
        result = remove_toml_section(&result, "color-scheme.custom");
        result = format!("{}\n{}", result.trim_end(), palette);
    }

    if let Some(window) = &converted.window_toml {
        result = remove_toml_section(&result, "window");
        result = format!("{}\n{}", result.trim_end(), window);
    }

    result
}
