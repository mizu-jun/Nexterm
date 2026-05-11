//! オーバーレイ描画で共用するヘルパー関数群。
//!
//! 主に `consent_dialog` で使用される。

/// 同意ダイアログの種別から要求元ペイン ID を取り出す
pub(super) fn pane_id_for(kind: &crate::state::ConsentKind) -> Option<u32> {
    use crate::state::ConsentKind;
    match kind {
        ConsentKind::OpenUrl(_) => None,
        ConsentKind::ClipboardWrite { source_pane, .. } => *source_pane,
        ConsentKind::Notification { source_pane, .. } => Some(*source_pane),
    }
}

/// 同意ダイアログの種別からプレビューに表示する文字列を返す
pub(super) fn preview_text(kind: &crate::state::ConsentKind) -> String {
    use crate::state::ConsentKind;
    match kind {
        ConsentKind::OpenUrl(url) => url.clone(),
        ConsentKind::ClipboardWrite { text, .. } => {
            // 制御文字や改行は安全のため空白に置換
            let safe: String = text
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            // バイト長で切り詰める
            if safe.len() > 200 {
                let mut end = 200;
                while end > 0 && !safe.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &safe[..end])
            } else {
                safe
            }
        }
        ConsentKind::Notification { title, body, .. } => format!("{title}: {body}"),
    }
}

/// テキストを指定列幅で複数行に折り返す（CJK 全角は 2 列分カウント）
pub(super) fn wrap_text(s: &str, max_cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_cols = 0usize;
    for c in s.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if current_cols + w > max_cols && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current_cols = 0;
        }
        current.push(c);
        current_cols += w;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
