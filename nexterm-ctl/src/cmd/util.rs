//! Utilities shared between the submodules in `cmd/`.

/// Remove the `[section_name]` section from a TOML string.
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
        // Stop skipping when the next section heading appears.
        if skip && trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            skip = false;
        }
        if !skip {
            result.push(line);
        }
    }

    result.join("\n")
}
