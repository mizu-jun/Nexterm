//! Phase 2c (UI/UX v2): Nerd Font glyph map for per-tab process icons.
//!
//! The server polls each pane's foreground process at 1 Hz and pushes
//! the executable name via `ServerToClient::ProcessChanged`. This
//! module owns the pure mapping from those names to Nerd Font
//! codepoints. The tab bar renderer calls `glyph_for_process` while
//! building each tab label; an `Option<&'static str>` is returned so
//! unknown processes render with no icon (the absence is intentionally
//! a signal — "nothing notable is running").
//!
//! All codepoints come from the [Nerd Font v3 cheat sheet]
//! (`nerd-fonts/cheat-sheet`). They live in the U+E000–U+F8FF Private
//! Use Area and in the U+F0000+ supplementary range, so they will
//! render as tofu on a regular system font — which is why
//! `TabBarConfig.show_process_icon` defaults to `false`.
//!
//! [Nerd Font v3 cheat sheet]: https://www.nerdfonts.com/cheat-sheet

/// Look up the Nerd Font glyph for a process name. Returns `None` when
/// no entry matches; callers should render no icon in that case rather
/// than fall back to a generic placeholder so the icon's presence keeps
/// signal value.
///
/// Matching rules:
/// - The lookup is case-insensitive (`Code` → `code`).
/// - Names are matched against their trailing component: any path
///   prefix is dropped by the server before broadcasting.
/// - Linux `comm` truncates to 15 chars, so map entries stay within
///   that limit.
pub fn glyph_for_process(name: &str) -> Option<&'static str> {
    let key = name.trim().to_ascii_lowercase();
    match key.as_str() {
        // Editors & IDEs
        "vim" | "nvim" | "vi" | "view" => Some("\u{e62b}"),
        "emacs" | "emacsclient" => Some("\u{e632}"),
        "nano" | "pico" => Some("\u{f14b}"),
        "code" | "code-insiders" | "code-oss" => Some("\u{f1354}"),

        // Network / remote
        "ssh" | "sshd" | "mosh" | "mosh-client" => Some("\u{f489}"),

        // Source control
        "git" | "tig" | "lazygit" | "gitui" => Some("\u{f1d3}"),

        // Runtimes
        "node" | "deno" | "bun" | "ts-node" => Some("\u{e718}"),
        "python" | "python3" | "ipython" | "ipython3" => Some("\u{e606}"),
        "ruby" | "irb" => Some("\u{e21e}"),
        "cargo" | "rustc" | "rustup" => Some("\u{e7a8}"),
        "go" | "gopls" => Some("\u{e626}"),

        // Containers / orchestration
        "docker" | "docker-compose" | "podman" => Some("\u{f308}"),
        "kubectl" | "k9s" | "helm" => Some("\u{f10fe}"),

        // Multiplexers / system
        "tmux" | "screen" | "byobu" => Some("\u{f120}"),
        "htop" | "btop" | "top" => Some("\u{f0ec1}"),
        "less" | "more" | "bat" => Some("\u{f15c}"),
        "man" | "info" => Some("\u{f02d}"),

        // Shells (fallback — when nothing else is running, show the shell)
        "bash" | "zsh" | "fish" | "dash" | "sh" | "ksh" | "tcsh" => Some("\u{f120}"),
        "pwsh" | "powershell" | "powershell.exe" => Some("\u{f0a0a}"),
        "cmd" | "cmd.exe" => Some("\u{f17a}"),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every documented mapping must return `Some` for the canonical
    /// lowercase key. Regression guard for typos in the match arms.
    #[test]
    fn canonical_keys_resolve() {
        for name in [
            "vim", "nvim", "ssh", "git", "node", "python", "cargo", "go", "docker", "tmux", "bash",
            "zsh", "pwsh",
        ] {
            assert!(
                glyph_for_process(name).is_some(),
                "expected a glyph for {:?}",
                name
            );
        }
    }

    /// The lookup must ignore case so Windows-style `Code.exe` lands
    /// on the same glyph as `code`. The server strips `.exe` before
    /// broadcasting; this test pins the case-fold expectation.
    #[test]
    fn case_insensitive_match() {
        let lower = glyph_for_process("code");
        let upper = glyph_for_process("CODE");
        assert!(lower.is_some());
        assert_eq!(lower, upper);
    }

    /// Whitespace must be trimmed so a stray `"vim\n"` from `/proc/
    /// {pid}/comm` still hits the map.
    #[test]
    fn whitespace_is_trimmed() {
        assert!(glyph_for_process("  vim  ").is_some());
        assert!(glyph_for_process("vim\n").is_some());
    }

    /// Unknown processes return `None` rather than fall through to a
    /// placeholder — the absence of a glyph carries meaning.
    #[test]
    fn unknown_returns_none() {
        assert_eq!(glyph_for_process("totally-unknown-process"), None);
        assert_eq!(glyph_for_process(""), None);
        assert_eq!(glyph_for_process("   "), None);
    }

    /// Matched glyphs must be exactly one non-empty grapheme so the
    /// tab-bar layout reserves the right amount of space. Regression
    /// guard against accidentally pasting multi-codepoint sequences
    /// into the map.
    #[test]
    fn glyphs_are_single_non_empty_strings() {
        for name in ["vim", "ssh", "git", "node", "bash", "pwsh"] {
            let glyph = glyph_for_process(name).expect("known name");
            assert!(!glyph.is_empty(), "glyph for {:?} is empty", name);
            let char_count = glyph.chars().count();
            assert_eq!(char_count, 1, "expected 1 codepoint for {:?}", name);
        }
    }
}
