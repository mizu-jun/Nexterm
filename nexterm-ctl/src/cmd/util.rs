//! cmd/ 内のサブモジュール間で共有するユーティリティ。

/// TOML テキストから指定されたセクション `[section_name]` を削除する
pub(crate) fn remove_toml_section(content: &str, section_name: &str) -> String {
    let search = format!("[{}]", section_name);
    let mut result = Vec::new();
    let mut skip = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == search {
            skip = true;
            continue;
        }
        // 次のセクション見出しが来たらスキップ終了
        if skip && trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            skip = false;
        }
        if !skip {
            result.push(line);
        }
    }

    result.join("\n")
}
