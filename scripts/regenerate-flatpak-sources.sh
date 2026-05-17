#!/usr/bin/env bash
# Cargo.lock から Flatpak 用の vendor 済みソース定義 (cargo-sources.json) を再生成する。
#
# 前提:
#   - Python 3.9+ がインストール済み
#   - aiohttp / PyYAML / tomlkit が pip install 済み（または venv で確保）
#
# 使い方:
#   bash scripts/regenerate-flatpak-sources.sh
#
# 出力:
#   pkg/flatpak/cargo-sources.json (上書き)
#
# Cargo.lock を変更したら必ず本スクリプトを実行し、生成された JSON を併せてコミットすること。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

GENERATOR_VERSION="master"
GENERATOR_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/${GENERATOR_VERSION}/cargo/flatpak-cargo-generator.py"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

GENERATOR_PATH="$TMP_DIR/flatpak-cargo-generator.py"

echo "==> flatpak-cargo-generator.py を取得中..."
curl -fsSL -o "$GENERATOR_PATH" "$GENERATOR_URL"

echo "==> 必要な Python 依存を確認..."
python_cmd="${PYTHON:-python3}"
if ! command -v "$python_cmd" >/dev/null 2>&1; then
    python_cmd="python"
fi

# 依存が無ければインストールを促す（自動インストールはユーザー環境への影響を避けるため任意）
if ! "$python_cmd" -c "import aiohttp, yaml, tomlkit" 2>/dev/null; then
    echo "ERROR: 必要な Python パッケージ (aiohttp, PyYAML, tomlkit) が見つかりません。"
    echo "       次のコマンドでインストールしてください:"
    echo "         $python_cmd -m pip install --user 'aiohttp>=3.9.5,<4.0.0' 'PyYAML>=6.0.2,<7.0.0' 'tomlkit>=0.13.3,<1.0'"
    exit 1
fi

OUTPUT="pkg/flatpak/cargo-sources.json"

echo "==> Cargo.lock から $OUTPUT を生成中..."
"$python_cmd" "$GENERATOR_PATH" Cargo.lock -o "$OUTPUT"

LINE_COUNT="$(wc -l <"$OUTPUT")"
SIZE_BYTES="$(wc -c <"$OUTPUT")"
echo "==> 完了: $OUTPUT ($LINE_COUNT 行, $SIZE_BYTES バイト)"
echo ""
echo "差分を確認してコミットしてください:"
echo "  git diff $OUTPUT"
