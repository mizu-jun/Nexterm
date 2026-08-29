"""Subset the Fluent icon font and emit the Rust codepoint table.

Driven by `scripts/subset-icon-font.sh`, which pins and verifies the upstream
revision and provides fonttools; this script does the resolution, subsetting,
renaming and code generation. It is not meant to be run directly.

Every path here is a maintenance-time path: correctness matters, speed does not.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont


def env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        sys.exit(f"error: {name} is not set (run scripts/subset-icon-font.sh)")
    return value


def parse_icon_set(path: Path) -> list[tuple[str, str]]:
    """Read `<RUST_CONST> <upstream-name>` pairs, preserving file order."""
    entries: list[tuple[str, str]] = []
    seen: set[str] = set()
    for lineno, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) != 2:
            sys.exit(f"error: {path}:{lineno}: expected two fields, got {len(parts)}")
        const, upstream = parts
        if const in seen:
            sys.exit(f"error: {path}:{lineno}: duplicate constant {const}")
        seen.add(const)
        entries.append((const, upstream))
    if not entries:
        sys.exit(f"error: {path} declares no icons")
    return entries


def rename_family(font: TTFont, family: str) -> None:
    """Point every family/full/PostScript name record at our own family.

    fontdb reads the family from the name table, so this is what makes
    `Family::Name("Nexterm Icons")` resolve — and what keeps the subset from
    being confused with a user-installed FluentSystemIcons.
    """
    postscript = family.replace(" ", "")
    for record in font["name"].names:
        if record.nameID in (1, 16):  # family, typographic family
            record.string = family
        elif record.nameID == 4:  # full name
            record.string = f"{family} Regular"
        elif record.nameID == 6:  # PostScript name
            record.string = f"{postscript}-Regular"


def emit_rust(entries: list[tuple[str, str, int]], repo: str, sha: str, family: str) -> str:
    lines = [
        "//! Chrome icon codepoints (UI/UX v3 P4a).",
        "//!",
        "//! GENERATED FILE — do not edit. Regenerate with",
        "//! `scripts/subset-icon-font.sh` after editing",
        "//! `assets/fonts/icon-set.txt`.",
        "//!",
        f"//! Source: {repo} @ {sha[:12]} (MIT; see THIRD-PARTY-NOTICES.md).",
        "//!",
        "//! Every codepoint below lives in the Private Use Area, which overlaps the",
        "//! Nerd Font range `tab_icons.rs` uses for terminal-content icons. They are",
        "//! only ever safe to draw through [`FontRole::Icon`], which is what keeps the",
        "//! two sets from resolving against each other.",
        "",
        "// The per-icon constants are a generated catalogue. Which of them a call",
        "// site draws is a review question, not a compiler one, so per-constant",
        "// dead-code analysis says nothing useful here; `ALL_ICONS` keeps the set",
        "// itself live and the font-coverage test keeps it honest.",
        "#![allow(dead_code)]",
        "",
        "/// Family name the bundled subset is registered under.",
        f'pub const ICON_FAMILY: &str = "{family}";',
        "",
        "/// The subsetted icon font, embedded in the binary.",
        'pub const ICON_FONT: &[u8] = include_bytes!("../../assets/fonts/NextermIcons-Regular.ttf");',
        "",
    ]
    for const, upstream, cp in entries:
        lines.append(f"/// `{upstream}`")
        lines.append(f"pub const {const}: char = '\\u{{{cp:x}}}';")
        lines.append("")

    lines.append("/// Every codepoint this module exposes, for coverage tests.")
    lines.append("///")
    lines.append("/// Order matches `assets/fonts/icon-set.txt`. Entries may repeat a")
    lines.append("/// codepoint when two sites share one icon (close is both a tab button")
    lines.append("/// and a caption button).")
    lines.append("pub const ALL_ICONS: &[char] = &[")
    for entry in entries:
        lines.append(f"    {entry[0]},")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    icon_set = Path(env("ICON_SET"))
    upstream_json = Path(env("UPSTREAM_JSON"))
    upstream_ttf = Path(env("UPSTREAM_TTF"))
    out_ttf = Path(env("OUT_TTF"))
    out_rs = Path(env("OUT_RS"))
    family = env("FAMILY_NAME")
    repo = env("UPSTREAM_REPO")
    sha = env("UPSTREAM_SHA")

    codepoint_map = json.loads(upstream_json.read_text(encoding="utf-8"))

    resolved: list[tuple[str, str, int]] = []
    for const, upstream in parse_icon_set(icon_set):
        if upstream not in codepoint_map:
            sys.exit(
                f"error: {upstream} is not in the upstream codepoint map. "
                "It may have been renamed or removed; check the pinned revision."
            )
        resolved.append((const, upstream, codepoint_map[upstream]))

    unicodes = sorted({entry[2] for entry in resolved})
    print(f"    {len(resolved)} icons -> {len(unicodes)} distinct codepoints")

    out_ttf.parent.mkdir(parents=True, exist_ok=True)
    args = [
        str(upstream_ttf),
        f"--unicodes={','.join(f'U+{cp:04X}' for cp in unicodes)}",
        f"--output-file={out_ttf}",
        # Keep the name table so the rename below survives; drop everything
        # else we do not draw with.
        "--name-IDs=1,2,3,4,6,16,17",
        "--drop-tables+=DSIG",
        "--no-hinting",
        # Deterministic output. fontTools' boolean options take a `--no-` form;
        # spelling this `--recalc-timestamp=0` *enables* it (the value is
        # parsed as a string, not a flag) and every regeneration then produces
        # a byte-different font for identical inputs.
        "--no-recalc-timestamp",
    ]
    subset.main(args)

    # `recalcTimestamp=False` keeps `head.modified` as the subsetter left it.
    # With the default (True), saving stamps the current time and every
    # regeneration produces a byte-different font for identical inputs.
    font = TTFont(out_ttf, recalcTimestamp=False)
    rename_family(font, family)
    font.save(out_ttf)

    out_rs.write_text(emit_rust(resolved, repo, sha, family), encoding="utf-8")


if __name__ == "__main__":
    main()
