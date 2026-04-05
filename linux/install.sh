#!/usr/bin/env bash
# Nexterm インストールスクリプト（Linux）
#
# 使い方:
#   ./install.sh          # ~/.local/bin にインストール（管理者不要）
#   sudo ./install.sh     # /usr/local/bin にインストール
#
# アンインストール:
#   ./install.sh --uninstall
#   sudo ./install.sh --uninstall

set -euo pipefail

BINARIES=(nexterm nexterm-server nexterm-client-gpu nexterm-client-tui nexterm-ctl)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# インストール先を決定（root なら /usr/local/bin、それ以外は ~/.local/bin）
if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    BIN_DIR="/usr/local/bin"
    DESKTOP_DIR="/usr/share/applications"
else
    BIN_DIR="${HOME}/.local/bin"
    DESKTOP_DIR="${HOME}/.local/share/applications"
fi

# アンインストール処理
if [[ "${1:-}" == "--uninstall" ]]; then
    echo "Nexterm をアンインストールしています..."
    for bin in "${BINARIES[@]}"; do
        if [[ -f "${BIN_DIR}/${bin}" ]]; then
            rm -f "${BIN_DIR}/${bin}"
            echo "  削除: ${BIN_DIR}/${bin}"
        fi
    done
    if [[ -f "${DESKTOP_DIR}/nexterm.desktop" ]]; then
        rm -f "${DESKTOP_DIR}/nexterm.desktop"
        echo "  削除: ${DESKTOP_DIR}/nexterm.desktop"
    fi
    echo "アンインストール完了。"
    exit 0
fi

# バイナリのインストール
echo "Nexterm を ${BIN_DIR} にインストールしています..."
mkdir -p "${BIN_DIR}"

for bin in "${BINARIES[@]}"; do
    src="${SCRIPT_DIR}/${bin}"
    if [[ -f "${src}" ]]; then
        install -m 755 "${src}" "${BIN_DIR}/${bin}"
        echo "  インストール: ${BIN_DIR}/${bin}"
    else
        echo "  スキップ（見つかりません）: ${src}"
    fi
done

# .desktop ファイルのインストール
echo "デスクトップエントリをインストールしています..."
mkdir -p "${DESKTOP_DIR}"
if [[ -f "${SCRIPT_DIR}/nexterm.desktop" ]]; then
    install -m 644 "${SCRIPT_DIR}/nexterm.desktop" "${DESKTOP_DIR}/nexterm.desktop"
    echo "  インストール: ${DESKTOP_DIR}/nexterm.desktop"
    # データベースを更新（コマンドが存在する場合のみ）
    if command -v update-desktop-database &>/dev/null; then
        update-desktop-database "${DESKTOP_DIR}" 2>/dev/null || true
    fi
fi

# PATH 確認
if [[ ":${PATH}:" != *":${BIN_DIR}:"* ]]; then
    echo ""
    echo "注意: ${BIN_DIR} が PATH に含まれていません。"
    echo "以下の行をシェルの設定ファイル（~/.bashrc や ~/.zshrc）に追加してください:"
    echo ""
    echo "  export PATH=\"\${HOME}/.local/bin:\${PATH}\""
    echo ""
fi

echo ""
echo "インストール完了！"
echo "起動コマンド: nexterm"
