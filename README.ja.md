# nexterm

tmux / zellij にインスパイアされた Rust 製ターミナルマルチプレクサ。wgpu による GPU 描画と Lua 設定システムを備えます。

> **English documentation:** [README.md](README.md)

[![CI](https://github.com/mizu-jun/nexterm/actions/workflows/ci.yml/badge.svg)](https://github.com/mizu-jun/nexterm/actions/workflows/ci.yml)
[![Coverage](https://github.com/mizu-jun/nexterm/actions/workflows/coverage.yml/badge.svg)](https://github.com/mizu-jun/nexterm/actions/workflows/coverage.yml)

リリース履歴は [CHANGELOG.md](CHANGELOG.md) と [GitHub Releases](https://github.com/mizu-jun/Nexterm/releases) を参照してください。
破壊的変更を含むバージョン間のアップグレード手順は [docs/MIGRATION.md](docs/MIGRATION.md) にまとめてあります。

---

## 主な特徴

- **デーモンレス** — サーバが PTY を保持し、クライアント切断後もセッションが生存。tmux 風の attach・複数クライアント共有。
- **GPU 描画** — wgpu + cosmic-text グリフアトラス。代替スクリーンバッファ、CJK 幅、リガチャ、フォントフォールバックチェーン。
- **SSH 内蔵** — russh ベースの SSH クライアント。ホストレジストリ、agent 認証、known_hosts 検証、ポートフォワード (-L/-R)、ProxyJump、SOCKS5、X11 転送、OS キーチェーン連携。SFTP アップロード/ダウンロード（進捗表示付き）。
- **BSP ペイン分割** — 任意の深さの分割、ペイン swap・zoom・break/join、ドラッグリサイズ。
- **コマンドブロック（Warp 風）** — OSC 133 プロンプトマーカーにより、prompt → command → output → exit-code を 1 ブロックに集約し、ブロック単位で navigation・copy・replay・rename が可能。WezTerm / kitty / Ghostty と互換。
- **画像プロトコル** — Sixel、Kitty、iTerm2 inline image。
- **Vim copy mode + Vi mode** — 選択・検索・モーション。現在のモードはステータスバーに表示。
- **Lua + TOML 設定** — ホットリロード、ステータスバーウィジェット、イベントフック、キーバインド、マクロ。
- **設定 GUI** — `Ctrl+,` で 7 カテゴリのパネルを開き、`toml_edit` 経由で `nexterm.toml` に書き戻し。
- **WASM プラグインランタイム** — wasmi サンドボックス（fuel + メモリ制限）。安定 Plugin API v2、ランタイム load/unload/reload。
- **スクリーンリーダー対応** — AccessKit によりタブ・ペイン・ダイアログ・ターミナルグリッドが NVDA / VoiceOver / Orca から操作可能。
- **Web ターミナル** — axum WebSocket + xterm.js 内蔵。トークン / OAuth / TOTP 認証、TLS オプション。
- **記録** — `nexterm-ctl record` で raw PTY ログと asciicast v2 を保存。
- **クロスプラットフォーム** — Linux / macOS / Windows（ConPTY + Named Pipe）、UI は 8 言語対応。
- **配布** — Homebrew、Scoop、winget、MSI、Flatpak、tarball。
- **セキュリティ & サプライチェーン** — Lua/WASM サンドボックス、機密操作の同意プロンプト、CI での cargo-deny、CycloneDX SBOM、SLSA build provenance、minisign 署名検証、STRIDE 脅威モデル。

機能一覧の詳細: [docs/src/features/](docs/src/features/)

---

## クイックスタート

```sh
# macOS
brew install mizu-jun/nexterm/nexterm && nexterm

# Linux (tarball)
tar xzf nexterm-vX.Y.Z-linux-x86_64.tar.gz && ./install.sh && nexterm

# Windows
# Releases ページから MSI をインストール後:
nexterm.exe
```

`nexterm` は単一バイナリで、サーバは内部 tokio タスクとして起動します。詳細なインストール・初期セットアップ・トラブルシューティングはユーザーガイドを参照:

- [Installation](docs/src/install.md) · [Quick Start](docs/src/quickstart.md) · [Windows Quick Start](docs/src/windows.md) · [Troubleshooting](docs/src/troubleshooting.md)

Windows は **10 1809+** が必須（ConPTY）。GPU クライアントは DirectX 11 対応アダプタが必要です。

---

## ソースからのビルド

```bash
# 前提: Rust 1.85+（workspace edition = "2024"）
# Linux: sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev

cargo build --release
cargo test
cargo clippy -- -D warnings        # PR マージに必須
cargo fmt --check
```

開発ワークフロー・コーディング規約・PR ガイドラインは [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) と [CLAUDE.md](CLAUDE.md) を参照。

---

## クレート構成

```
nexterm/
├── nexterm-proto         # IPC メッセージ型（postcard）
├── nexterm-vt            # VT100 パーサ、仮想スクリーン、画像デコード
├── nexterm-server        # PTY サーバ（IPC + セッション管理）
├── nexterm-config        # 設定ローダー（TOML + Lua）+ ステータスバー評価
├── nexterm-client-core   # 共通 IPC 接続レイヤー
├── nexterm-client-tui    # TUI クライアント（ratatui + crossterm）
├── nexterm-client-gpu    # GPU クライアント（wgpu + winit + cosmic-text）。ビルド bin は `nexterm`
├── nexterm-ctl           # セッション / プラグイン管理 CLI
├── nexterm-i18n          # 多言語対応（8 言語）
├── nexterm-ssh           # SSH クライアント（russh）
└── nexterm-plugin        # WASM プラグインホスト（wasmi、API v2）
```

クレート依存グラフ、プロセス構成、BSP レイアウトエンジン、IPC フレーミング、描画パイプライン、スレッドモデルの詳細は [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) を参照。

---

## キーバインド

主要なショートカット:

| Key | Action |
|-----|--------|
| `Ctrl+,` | 設定パネルを開く |
| `Ctrl+Shift+P` | コマンドパレット |
| `Ctrl+F` | スクロールバック検索 |
| `Ctrl+[` | Vim コピーモード |
| `Ctrl+Shift+C` / `V` | コピー / ペースト |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | フォントサイズ拡大 / 縮小 / リセット |
| `Ctrl+Shift+H` | SSH ホストマネージャ |
| `Ctrl+Shift+M` | Lua マクロピッカー |
| `Ctrl+B Z` | フォーカスペインの zoom |
| `Ctrl+Shift+ArrowUp/Down` | コマンドブロック間ジャンプ |

完全なリファレンス: [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md)

---

## nexterm-ctl

```bash
nexterm-ctl list                              # セッション一覧
nexterm-ctl new work                          # セッション作成
nexterm-ctl attach work                       # attach コマンドを表示
nexterm-ctl kill work                         # セッション終了

nexterm-ctl record start work output.log     # raw PTY 記録
nexterm-ctl record start-cast work cast.cast # asciicast v2 記録

nexterm-ctl theme import ~/.iTerm2/colorscheme.itermcolors
nexterm-ctl plugin {list,load,unload,reload} # WASM プラグイン制御
```

`NEXTERM_LANG=ja nexterm-ctl list` で UI ロケールを強制できます。対応: `en`, `fr`, `de`, `es`, `it`, `zh-CN`, `ja`, `ko`

---

## 設定

設定ファイルの探索パス:

| OS | パス |
|----|------|
| Linux / macOS | `~/.config/nexterm/config.toml` |
| Windows | `%APPDATA%\nexterm\config.toml` |

```toml
# 最小例
scrollback_lines = 50000

[font]
family = "JetBrains Mono"
size = 14.0
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji"]

[colors]
scheme = "tokyonight"

[shell]
program = "/usr/bin/fish"

[window]
background_opacity = 0.95
```

同じ場所に置く Lua オーバーライドファイル（`nexterm.lua`）でランタイムに値を変更可能。両方とも保存時にホットリロードされます。

完全なリファレンス:
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) — TOML/Lua の全キー
- [docs/src/config/snippets.md](docs/src/config/snippets.md) — コピペ用レシピ
- [docs/src/config/lua-recipes.md](docs/src/config/lua-recipes.md) — Lua マクロ・フック・ステータスバー
- [docs/shell-integration.md](docs/shell-integration.md) — OSC 133 コマンドブロック用の bash / zsh / fish スニペット

---

## ドキュメントマップ

| ドキュメント | 内容 |
|--------------|------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | クレート構成、プロセスモデル、描画パイプライン、IPC、BSP |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | IPC プロトコル仕様（メッセージ型、フレーミング、ハンドシェイク） |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | TOML / Lua 設定の完全リファレンス |
| [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md) | キーバインド完全リファレンス |
| [docs/MIGRATION.md](docs/MIGRATION.md) | バージョン間アップグレード手順 |
| [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | STRIDE 脅威モデル（9 trust boundary） |
| [docs/SBOM.md](docs/SBOM.md) | サプライチェーン・SBOM ポリシー |
| [docs/TESTING_STRATEGY.md](docs/TESTING_STRATEGY.md) | テスト戦略 + QA × ISO/IEC 25010 カバレッジマトリクス |
| [docs/plugin-api.md](docs/plugin-api.md) | WASM Plugin API v2 |
| [docs/shell-integration.md](docs/shell-integration.md) | コマンドブロック用シェル統合スニペット |
| [docs/benchmarks.md](docs/benchmarks.md) | VT スループット / キーストロークレイテンシのベンチマーク |
| [docs/adr/](docs/adr/README.md) | Architecture Decision Records |
| [docs/src/](docs/src/README.md) | mdBook ユーザーガイド（install / features / config / troubleshooting） |
| [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) | ビルド手順、コーディング規約、PR ガイドライン |
| [SECURITY.md](SECURITY.md) | セキュリティポリシー・報告手順 |

---

## ライセンス

MIT OR Apache-2.0
