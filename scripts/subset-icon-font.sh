#!/usr/bin/env bash
#
# Regenerate the bundled chrome icon font and its Rust codepoint table.
#
# This is a maintenance tool, not a build step: CI never runs it, and the two
# outputs are committed. Run it only when `assets/fonts/icon-set.txt` changes or
# when bumping the pinned upstream revision below.
#
#   Inputs   assets/fonts/icon-set.txt          (the icon set — edit this)
#   Outputs  assets/fonts/NextermIcons-Regular.ttf
#            nexterm-client-gpu/src/icons.rs
#
# Upstream is microsoft/fluentui-system-icons (MIT), pinned to a commit rather
# than a tag: upstream tags are per-npm-package and name nothing about the font
# files. See THIRD-PARTY-NOTICES.md.
#
# Requires: curl, python3 (with venv), sha256sum.

set -euo pipefail

# ── Pinned upstream ───────────────────────────────────────────────────────────
UPSTREAM_REPO="microsoft/fluentui-system-icons"
UPSTREAM_SHA="fb047fb395f45ccf1129f8eaee672c9dfa99152e"
UPSTREAM_TTF_SHA256="9c55ac8e041aa905d2a09d4a7e57a156dece1df99cd64952467348da0e158db4"
# Pinned so a regeneration cannot silently pick up a new fonttools release.
FONTTOOLS_VERSION="4.55.3"
# The family name the subset is renamed to. `FontManager` asks for this exact
# string, and renaming also keeps the subset from colliding with a
# user-installed FluentSystemIcons.
FAMILY_NAME="Nexterm Icons"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
icon_set="$repo_root/assets/fonts/icon-set.txt"
out_ttf="$repo_root/assets/fonts/NextermIcons-Regular.ttf"
out_rs="$repo_root/nexterm-client-gpu/src/icons.rs"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# ── Fetch and verify upstream ─────────────────────────────────────────────────
base="https://raw.githubusercontent.com/$UPSTREAM_REPO/$UPSTREAM_SHA/fonts"
echo "==> fetching upstream at ${UPSTREAM_SHA:0:12}"
curl -sSL -o "$work/upstream.ttf" "$base/FluentSystemIcons-Regular.ttf"
curl -sSL -o "$work/upstream.json" "$base/FluentSystemIcons-Regular.json"

actual="$(sha256sum "$work/upstream.ttf" | cut -d' ' -f1)"
if [[ "$actual" != "$UPSTREAM_TTF_SHA256" ]]; then
    echo "error: upstream TTF checksum mismatch" >&2
    echo "  expected $UPSTREAM_TTF_SHA256" >&2
    echo "  actual   $actual" >&2
    echo "The pinned commit should be immutable; investigate before proceeding." >&2
    exit 1
fi
echo "    checksum ok ($(wc -c <"$work/upstream.ttf") bytes)"

# ── fonttools in a throwaway venv ─────────────────────────────────────────────
# A venv rather than a global install: the devcontainer's Python is
# externally managed, so `pip install` outside a venv fails.
echo "==> preparing fonttools $FONTTOOLS_VERSION"
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install --quiet "fonttools==$FONTTOOLS_VERSION"

# ── Resolve the icon set, subset, rename, emit the Rust table ─────────────────
echo "==> subsetting"
ICON_SET="$icon_set" UPSTREAM_JSON="$work/upstream.json" \
UPSTREAM_TTF="$work/upstream.ttf" OUT_TTF="$out_ttf" OUT_RS="$out_rs" \
UPSTREAM_REPO="$UPSTREAM_REPO" UPSTREAM_SHA="$UPSTREAM_SHA" \
FAMILY_NAME="$FAMILY_NAME" \
"$work/venv/bin/python" "$repo_root/scripts/subset_icon_font.py"

echo "==> wrote $(realpath --relative-to="$repo_root" "$out_ttf") ($(wc -c <"$out_ttf") bytes)"
echo "==> wrote $(realpath --relative-to="$repo_root" "$out_rs")"
echo
echo "Remember to run \`cargo fmt\` and to re-check THIRD-PARTY-NOTICES.md if the"
echo "pinned revision changed."
