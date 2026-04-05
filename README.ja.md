# nexterm — 日本語ドキュメント

> **English documentation:** [README.md](README.md)

Rust 製のターミナルマルチプレクサ。tmux/zellij インスパイアで、wgpu による GPU レンダリングと Lua 設定システムを搭載する。

## v0.5.5 の新機能

**Windows — GPU クライアントのフォントレンダリングを修正**

GPU クライアントで "Wi ndows Power She l l" のように文字間に余分なスペースが入っていた問題を修正。

- `cell_w = font_size × 0.6` という固定係数を廃止し、基準文字 `'0'` を実際に
  ラスタライズして advance width を計測する方式に変更。
- `FontManager::new()` に `scale_factor: f32` を追加し、DPI 拡大率（125%・150%）に
  応じた正確な物理ピクセルサイズを算出するようになった。
- `rasterize_char` クロージャ内の負値座標ラップバグ（`x as u32`）を修正。
- `WindowEvent::ScaleFactorChanged` イベントでフォントとグリフアトラスを自動再生成。

**Windows 11 — Acrylic すりガラス背景**

- ウィンドウ作成後に `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMWCP_ACRYLIC)` を呼び出し、Windows Terminal に似たすりガラス効果を適用。
- wgpu Surface の composite alpha mode を `PreMultiplied` に設定。
- Windows 10 や非 Windows 環境には影響なし（`#[cfg(windows)]` ガード済み）。

## v0.5.4 の新機能

**Windows — 起動時の余分なコンソールウィンドウを解消**

`nexterm.exe`、`nexterm-server`、`nexterm-client-gpu` にリリースビルド限定で `windows_subsystem = "windows"` を追加。エクスプローラーや MSI から起動しても黒いコンソールが開かなくなった。

- ログは `%LOCALAPPDATA%\nexterm\nexterm-server.log` / `nexterm-client.log` に出力。
- エラーは `MessageBoxW` ダイアログで通知。

**macOS — バイナリを ad-hoc 署名 & Intel Mac 対応**

- すべての macOS リリースバイナリを `codesign --sign -` で署名。
  `xattr -dr com.apple.quarantine <ファイル>` だけで Gatekeeper をバイパスして起動可能。
- `macos-13`（Intel ランナー）で `x86_64-apple-darwin` バイナリを追加。

## v0.5.1 の新機能

**Windows バグ修正** — v0.5.0 で Windows バイナリが生成されなかった原因となったビルド・テスト失敗 4 件を修正するパッチリリース。

- `nexterm-launcher`: `windows-sys 0.59` に必要な `Win32_Security` フィーチャーの追加、`GENERIC_READ` のインポート先を `Win32::Foundation` へ修正
- `nexterm-server/pane.rs`: `portable_pty` インポートの `#[cfg(unix)]` ガードを削除 — Windows (ConPTY) でも同型が必要
- `nexterm-server/ipc.rs`: パス検証テストをプラットフォーム別に分岐（Windows は `%TEMP%\nexterm\…` 形式）
- `nexterm-client-gpu/host_manager.rs`: テストヘルパーの `HostConfig` 初期化子に `x11_forward` / `x11_trusted` フィールドを追加

`x86_64-pc-windows-msvc` で全 93 テスト合格。詳細は [CHANGELOG.md](CHANGELOG.md) を参照。

## v0.5.0 の新機能

**SSH・接続**
- SSH 多重接続 UI — SSH ホストマネージャ（`Ctrl+Shift+H`）で Enter すると各ホストを新タブで開く
- X11 フォワーディング — `[[hosts]]` に `x11_forward = true`（`ssh -X`）/ `x11_trusted = true`（`ssh -Y`）を追加

**UX**
- 設定 GUI パネル — `Ctrl+,` で Font / Colors / Window タブのパネルを開く。変更は `nexterm.toml` に即時書き戻し
- コマンドパレットに ShowSettings アクション追加（計 17 アクション）

**Web ターミナル**
- ブラウザから接続できる組み込み Web ターミナル — `[web] enabled = true` で有効化
- xterm.js を埋め込み配信、`ws://localhost:7681` でアクセス
- トークン認証対応（`token = "..."` 設定時）、デフォルト無効

**パッケージ配布**
- Homebrew tap Formula（`pkg/homebrew/nexterm.rb`）
- Scoop バケットマニフェスト（`pkg/scoop/nexterm.json`）
- winget マニフェスト（`pkg/winget/mizu-jun.Nexterm.yaml`）
- GitHub Pages ドキュメントサイト CI 自動デプロイ（`mizu-jun.github.io/Nexterm`）

## v0.4.0 の新機能

**SSH・接続**
- SSH ホストマネージャ — fuzzy 検索でホストを一発接続（`Ctrl+Shift+H`）
- SFTP アップロード / ダウンロードダイアログ（`Ctrl+Shift+U/D`）とリアルタイム進捗表示
- SSH セッション経由のリモートポートフォワーディング（`-R`）
- シリアルポート接続（コマンドパレット `ConnectSerial`）

**UX・ペイン管理**
- コマンドパレット（Ctrl+Shift+P）に 16 アクションを追加（SFTP・ホストマネージャ等）
- Lua マクロピッカー — fuzzy 検索でマクロを一発実行（`Ctrl+Shift+M`）
- Quick Select モード（`Ctrl+Shift+Space`）— URL・パス・IP・ハッシュをハイライト選択
- ペインズームトグル（`Ctrl+B Z`）— フォーカスペインをフルスクリーン表示
- ペイン入れ替え（`Ctrl+B {` / `Ctrl+B }`）
- ペインをウィンドウに切り出し（`Ctrl+B !`）

**自動化**
- Lua イベントフック: `on_session_start`、`on_attach`、`on_pane_open` で Lua コールバックを実行
- Lua マクロエンジン: TOML に `[[macros]]` を定義し、ピッカーから実行。出力をアクティブペインに送信

**ログ**
- ログファイル名テンプレート（`{session}`、`{date}`、`{time}` プレースホルダー）
- バイナリ PTY ログモード — テキストログと並行して生バイト列を記録

**Windows**
- WiX Toolset v3 による MSI インストーラー（CI 自動化）
- Windows Service インストール・アンインストールスクリプト
- `signtool.exe` による自動コード署名（CI Secrets 設定時）
- `nexterm-launcher` — `nexterm.exe` 1つでサーバー自動起動 + GPU クライアント起動

## 特徴

- **GPU レンダリング** — wgpu + cosmic-text による高速フォントレンダリング
- **デーモンレス設計** — サーバープロセスが PTY を保持し、クライアント再接続時にセッションを復元
- **BSP 分割レイアウト** — Binary Space Partition で任意深さのペイン分割に対応
- **Lua + TOML 設定** — TOML でデフォルト値、Lua で動的オーバーライド。ファイル変更を自動検知してリアルタイム反映
- **Lua イベントフック** — `on_session_start`・`on_attach`・`on_pane_open` で Lua コールバックを設定
- **Lua マクロピッカー** — TOML に `[[macros]]` を定義し `Ctrl+Shift+M` で fuzzy 検索・一発実行
- **設定 GUI パネル** — `Ctrl+,` で Font / Colors / Window タブを開き、`nexterm.toml` に即時書き戻し
- **Quick Select モード** — `Ctrl+Shift+Space` で URL・パス・IP・ハッシュをハイライト選択
- **Web ターミナル** — `[web] enabled = true` でブラウザから xterm.js 端末にアクセス（`ws://localhost:7681`）
- **マウス操作** — クリックでペインフォーカス切り替え、ホイールでスクロールバック操作、Ctrl+Click で URL を開く
- **クリップボード統合** — Ctrl+Shift+C でコピー、Ctrl+Shift+V でペースト（arboard）
- **コピーモード** — Vim 風テキスト選択（Ctrl+[, hjkl, v, y）
- **Lua ステータスバー** — `os.date()` 等の Lua 式をステータスラインに 1 秒ごとに表示
- **タブバー** — WezTerm スタイルのタブバー（ペインラベル + `❯` セパレータ）
- **セッション録画** — `nexterm-ctl record start/stop` で PTY 出力をファイルに保存
- **BEL 通知** — VT BEL (`\x07`) 受信時に OS ウィンドウ注目要求をトリガー
- **フォントサイズ変更** — Ctrl+= / Ctrl+- / Ctrl+0 でランタイムにサイズを変更
- **ウィンドウ透過** — 不透明度・ボーダーレス・macOS ぼかしを設定ファイルで制御
- **URL 検出** — グリッド内の URL をアンダーライン表示し Ctrl+Click でブラウザ起動
- **nexterm-ctl** — セッションの一覧・作成・終了・録画を行う CLI ツール
- **画像プロトコル** — Sixel / Kitty 両形式の画像表示に対応
- **TUI フォールバック** — GPU クライアントが使えない環境向けに ratatui ベースの TUI クライアントを同梱
- **クロスプラットフォーム** — Linux / macOS / Windows 対応（Windows は ConPTY + Named Pipe を使用）
- **多言語対応** — UI を 8 言語に対応（英語・フランス語・ドイツ語・スペイン語・イタリア語・簡体字中国語・日本語・韓国語）
- **SSH クライアント** — russh による組み込み SSH 接続。パスワード・公開鍵認証対応。TOML にホストを登録可能
- **SSH ホストマネージャ** — `Ctrl+Shift+H` で fuzzy 検索可能なホスト一覧を開き、各ホストを新タブで接続
- **X11 フォワーディング** — `[[hosts]]` に `x11_forward = true`（`-X`）/ `x11_trusted = true`（`-Y`）で設定
- **OS キーチェーン統合** — macOS Keychain / Windows Credential Store に SSH パスワードを安全保存
- **リモートポートフォワーディング** — SSH セッション経由の `-R` 方向フォワーディング
- **SFTP ファイル転送** — `Ctrl+Shift+U/D` でアップロード/ダウンロードダイアログ（進捗表示付き）
- **シリアルポート接続** — コマンドパレットから `ConnectSerial` でシリアルデバイスに接続
- **マウス選択コピー** — 左ドラッグで範囲選択し、リリース時に自動クリップボードコピー（青色ハイライト）
- **ペイン閉鎖** — フォーカスペインを閉じ、兄弟ノードを BSP ツリーで昇格させる
- **ペインリサイズ** — キーボードで分割比率を調整可能
- **ウィンドウ管理完全化** — IPC 経由でウィンドウの作成・削除・切替・リネームが可能
- **ブロードキャスト入力** — 全ペインへのキー入力同時送信（tmux の synchronize-panes 相当）
- **タイムスタンプ付きログ** — 行ごとの `[HH:MM:SS]` タイムスタンプ + ANSI ストリップオプション
- **OSC 通知** — OSC 0/2 によるウィンドウタイトル変更、OSC 9 によるデスクトップ通知
- **カスタムカラースキーム** — TOML の `[colors.custom]` で 16 色パレットを自由定義
- **CJK 幅計算** — 全角文字（CJK・絵文字）を正確に 2 カラム幅で処理
- **フォントフォールバックチェーン** — `font_fallbacks` で絵文字・CJK フォントを順番に指定
- **macOS セッション復元** — `lsof` による作業ディレクトリ保持

## 実装状況

### Phase 1: コア基盤（完成）

| コンポーネント | 内容 | 状態 |
|--------------|------|------|
| `nexterm-proto` | IPC プロトコル型定義 (bincode) | ✅ |
| `nexterm-vt` | VT100/ANSI パーサ + Sixel/Kitty デコード | ✅ |
| `nexterm-server` | PTY サーバー (セッション・ウィンドウ・ペイン管理) | ✅ |
| `nexterm-client-tui` | TUI クライアント (ratatui + crossterm) | ✅ |

### Phase 2: GPU クライアント & 設定（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 2-1 | nexterm-config — TOML + Lua 設定 + ホットリロード | ✅ |
| 2-2 | nexterm-client-gpu — wgpu レンダラー基盤 | ✅ |
| 2-3 | Sixel / Kitty 画像プロトコル対応 | ✅ |
| 2-4 | スクロールバック & インクリメンタル検索 | ✅ |
| 2-5 | コマンドパレット (fuzzy マッチ) | ✅ |

### Phase 3: マルチペイン & 拡張（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 3-1 | サーバー側 BSP レイアウトモデル | ✅ |
| 3-2 | プロトコル拡張 (LayoutChanged / FocusPane / PasteText) | ✅ |
| 3-3 | GPU クライアント マルチペイン分割表示 | ✅ |
| 3-4 | マウスサポート (クリックフォーカス / ホイールスクロール) | ✅ |
| 3-5 | クリップボード統合 (Ctrl+Shift+C/V) | ✅ |
| 3-6 | nexterm-ctl CLI ツール (list / new / attach / kill) | ✅ |
| 3-7 | 設定ホットリロード → GPU クライアント即時反映 | ✅ |
| 3-8 | Lua ステータスバーウィジェット | ✅ |

### Phase 4: ローカライゼーション & プロジェクト構造（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 4-1 | 標準 OSS ディレクトリ構造（.github, examples, tests） | ✅ |
| 4-2 | nexterm-i18n クレート（JSON ロケール埋め込み, sys-locale 検出） | ✅ |
| 4-3 | UI 文字列の 8 言語対応 | ✅ |
| 4-4 | 英語 README・ドキュメント整備 | ✅ |

### Phase 5: UX 強化（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 5-A | セッション録画（`nexterm-ctl record start/stop`） | ✅ |
| 5-B | WezTerm スタイル タブバー（`❯` セパレータ） | ✅ |
| 5-C | ウィンドウ透過・ぼかし・ボーダーレス | ✅ |
| 5-D | Vim 風コピーモード（Ctrl+[, hjkl, v で選択, y でヤンク） | ✅ |
| 5-E | ランタイム フォントサイズ変更（Ctrl+= / Ctrl+- / Ctrl+0） | ✅ |
| 5-F | URL 検出 + Ctrl+Click でブラウザ起動 | ✅ |
| 5-G | VT BEL 通知 → OS ウィンドウ注目要求 | ✅ |

### Phase 6: セキュリティ & 信頼性（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 6-1 | IPC ピア UID 検証（Linux: SO_PEERCRED、macOS: getpeereid） | ✅ |
| 6-2 | `StartRecording` のパストラバーサル防止 | ✅ |
| 6-3 | Windows Named Pipe: リモートクライアント拒否 | ✅ |
| 6-4 | LuaWorker バックグラウンドスレッド（メインスレッドのブロックを解消） | ✅ |
| 6-5 | セッションスナップショット永続化（JSON、自動保存・復元） | ✅ |

### Phase 7: 競合ツール比較に基づく機能追加（完成）

rlogin、Tera Term、WezTerm、tmux との比較で不足していた機能を実装。

| ステップ | 内容 | 状態 |
|---------|------|------|
| 7-1 | ペイン閉鎖・リサイズ（BSP ノード削除・比率調整） | ✅ |
| 7-2 | ウィンドウ操作完全化（作成・削除・フォーカス・リネーム） | ✅ |
| 7-3 | マウスドラッグ選択 → クリップボード自動コピー | ✅ |
| 7-4 | macOS lsof による CWD 保持 | ✅ |
| 7-5 | SSH クライアント（nexterm-ssh、russh 0.58、パスワード + 公開鍵） | ✅ |
| 7-6 | SSH ホスト登録（TOML の `[[hosts]]`） | ✅ |
| 7-7 | OS キーチェーン統合（keyring crate） | ✅ |
| 7-8 | タイムスタンプ付きログ（ANSI ストリップ対応） | ✅ |
| 7-9 | ブロードキャスト入力（全ペイン同時入力） | ✅ |
| 7-10 | OSC 0/2 タイトル + OSC 9 デスクトップ通知 | ✅ |
| 7-11 | TOML で定義するカスタム 16 色パレット | ✅ |
| 7-12 | CJK・全角文字の幅計算（unicode-width） | ✅ |
| 7-13 | フォントフォールバックチェーン（font_fallbacks 設定） | ✅ |

### Phase 8: 高度な UX（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 8-1 | ペインズームトグル（toggle-zoom、`Ctrl+B Z`） | ✅ |
| 8-2 | Quick Select モード — URL・パス・IP・ハッシュをハイライト（`Ctrl+Shift+Space`） | ✅ |
| 8-3 | SSH ホストマネージャ UI — fuzzy 検索・一発接続（`Ctrl+Shift+H`） | ✅ |
| 8-4 | Lua マクロ実行エンジン — TOML `[[macros]]` + ピッカー UI（`Ctrl+Shift+M`） | ✅ |

### Phase 9: ペイン管理（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 9-1 | ペイン入れ替え（`Ctrl+B {` / `Ctrl+B }`） | ✅ |
| 9-2 | ペインをウィンドウに切り出し / 結合（`Ctrl+B !`） | ✅ |

### Phase 10: 接続（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 10-1 | SSH セッション経由のリモートポートフォワーディング（`-R`） | ✅ |
| 10-3 | SFTP アップロード / ダウンロードダイアログ（進捗付き、`Ctrl+Shift+U/D`） | ✅ |
| 10-4 | シリアルポート接続（コマンドパレット `ConnectSerial`） | ✅ |

### Phase 11: ログ & フック（完成）

| ステップ | 内容 | 状態 |
|---------|------|------|
| 11-1 | Lua イベントフック — `on_session_start`、`on_attach`、`on_pane_open` | ✅ |
| 11-3 | ログファイル名テンプレート（`{session}`、`{date}`、`{time}`） | ✅ |
| 11-4 | バイナリ PTY ログモード | ✅ |

### Windows（完成）

| 項目 | 内容 | 状態 |
|------|------|------|
| W-1 | MSI インストーラー（WiX Toolset v3、CI 自動化） | ✅ |
| W-2 | コード署名ワークフロー（`signtool.exe`、CI Secrets） | ✅ |
| W-3 | `nexterm-launcher` — `nexterm.exe` 単一エントリーポイント | ✅ |
| W-4 | Windows Service インストール・アンインストールスクリプト | ✅ |
| W-5 | PowerShell `-NoLogo` オプション + cmd.exe フォールバック | ✅ |
| W-7 | Windows クイックスタートドキュメント | ✅ |
| W-10 | スナップショット保存先を `%APPDATA%\nexterm` に統一 | ✅ |

### Phase 12: 配布・拡張性（完成）

| 項目 | 内容 | 状態 |
|------|------|------|
| 12-1 | SSH 多重接続 UI（ホストマネージャ → 新タブ + ConnectSsh） | ✅ |
| 12-2 | X11 フォワーディング（ホストごとに `x11_forward` / `x11_trusted` 設定） | ✅ |
| 12-3 | 設定 GUI パネル（`Ctrl+,`、`toml_edit` で nexterm.toml に書き戻し） | ✅ |
| 12-4 | 組み込み Web ターミナル（axum WebSocket + xterm.js、トークン認証） | ✅ |
| 12-5 | Homebrew tap Formula | ✅ |
| 12-6 | Scoop バケットマニフェスト | ✅ |
| 12-7 | winget マニフェスト | ✅ |
| 12-8 | GitHub Pages ドキュメントサイト（mdBook、CI 自動デプロイ） | ✅ |

**テスト**: 100 件以上、全通過

## macOS クイックスタート

> **要点** — `nexterm` を実行するだけです。他の実行ファイルを手動で起動する必要はありません。

### Homebrew でインストール（推奨）

```sh
brew install mizu-jun/nexterm/nexterm
nexterm
```

### tarball からインストール

1. [Releases](https://github.com/mizu-jun/Nexterm/releases) ページから  
   Apple Silicon → `nexterm-vX.Y.Z-macos-arm64.tar.gz`  
   Intel Mac  → `nexterm-vX.Y.Z-macos-x86_64.tar.gz`  
   をダウンロードする。

2. 展開して quarantine フラグを除去する:
   ```sh
   tar xzf nexterm-vX.Y.Z-macos-arm64.tar.gz
   xattr -dr com.apple.quarantine Nexterm.app
   ```

3. **方法 A — Finder から起動:** `Nexterm.app` を `/Applications` に移動してダブルクリック。

4. **方法 B — ターミナルから起動:**
   ```sh
   sudo cp nexterm nexterm-server nexterm-client-gpu nexterm-client-tui nexterm-ctl /usr/local/bin/
   nexterm
   ```

`nexterm` コマンドが `nexterm-server` をバックグラウンドで自動起動し、`nexterm-client-gpu` を開きます。  
他のバイナリ（`nexterm-server`、`nexterm-client-gpu` など）は上級者向けです。

---

## Linux クイックスタート

> **要点** — `nexterm` を実行するだけです。他の実行ファイルを手動で起動する必要はありません。

### tarball からインストール

1. [Releases](https://github.com/mizu-jun/Nexterm/releases) ページから  
   `nexterm-vX.Y.Z-linux-x86_64.tar.gz` をダウンロードする。

2. 展開してインストールスクリプトを実行する:
   ```sh
   tar xzf nexterm-vX.Y.Z-linux-x86_64.tar.gz
   ./install.sh
   ```
   全バイナリが `~/.local/bin/` にインストールされ、`.desktop` エントリが登録されて  
   アプリランチャーに Nexterm が表示されるようになります。  
   システム全体へのインストールは `sudo ./install.sh` を使用してください（`/usr/local/bin/` に配置）。

3. 起動:
   ```sh
   nexterm
   ```

### アンインストール

```sh
./install.sh --uninstall       # ユーザーインストールの場合
sudo ./install.sh --uninstall  # システム全体インストールの場合
```

---

## Windows クイックスタート

### 動作要件

| 項目 | 最小要件 |
|------|---------|
| Windows バージョン | **Windows 10 1809（2018年10月更新）以降** |
| アーキテクチャ | x86-64 |
| GPU | DirectX 11 対応（wgpu 要件） |

> Windows 10 1809 以降に搭載された **ConPTY**（Pseudo Console）API を使用するため、それ以前の Windows は非対応です。

### MSI インストーラーを使う（推奨）

1. [Releases](https://github.com/kusanagi-jn/nexterm/releases) ページから `nexterm-vX.Y.Z-windows-x86_64.msi` をダウンロードする。
2. MSI をダブルクリックしてインストーラーを起動する。
3. 指示に従ってインストールを完了する（`C:\Program Files\Nexterm` にインストールされ、`PATH` に自動登録される）。
4. **スタートメニュー** または `nexterm` コマンドで起動する。

> **SmartScreen 警告について**: Nexterm はオープンソースプロジェクトであり、商用コード署名証明書を使用していないため、Windows Defender SmartScreen に「発行元不明」と表示されることがあります。「詳細情報」→「実行」でインストールを続行できます。Releases ページに公開している SHA-256 チェックサムで正規バイナリであることを確認できます。

### ZIP（ポータブル版）を使う

1. Releases ページから `nexterm-vX.Y.Z-windows-x86_64.zip` をダウンロードして展開する。
2. 展開先ディレクトリ（例: `C:\Nexterm`）を `PATH` に追加する（任意）。

### 起動方法

```powershell
# 1コマンドで起動（サーバー自動起動＋GPU クライアント表示）
nexterm.exe
```

`nexterm.exe` は**ランチャー**です。`nexterm-server` が未起動であればバックグラウンドで自動起動し、その後 `nexterm-client-gpu` を開きます。

サーバーを個別に起動する場合:

```powershell
# サーバーをバックグラウンドで起動
Start-Process -NoNewWindow nexterm-server.exe

# GPU クライアントを起動
nexterm-client-gpu.exe

# GPU が使えない環境は TUI クライアント
nexterm-client-tui.exe
```

### Windows Service として登録する（任意）

`nexterm-server` を Windows Service として登録すると、ユーザーログイン不要で OS 起動時に自動起動します:

```powershell
# 管理者権限で実行
.\install-service.ps1

# 停止 / 起動 / アンインストール
Stop-Service NextermServer
Start-Service NextermServer
.\uninstall-service.ps1
```

### デフォルトシェルの優先順位

| 優先度 | シェル | パス |
|--------|--------|------|
| 1 | PowerShell 7 | `C:\Program Files\PowerShell\7\pwsh.exe` |
| 2 | PowerShell 5 | `C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe` |
| 3 | cmd.exe | `C:\Windows\System32\cmd.exe` |

変更する場合は設定ファイルに記述します:

```toml
# %APPDATA%\nexterm\nexterm.toml
[shell]
program = "C:\\Windows\\System32\\cmd.exe"
```

### コード署名について

Nexterm のバイナリはデフォルトでは商用コード署名されていません。社内配布など SmartScreen 警告を避けたい場合は `signtool.exe` で自己署名できます:

```powershell
$cert = New-SelfSignedCertificate -Subject "CN=Nexterm" -Type CodeSigning -CertStoreLocation Cert:\CurrentUser\My
Set-AuthenticodeSignature -FilePath nexterm.exe -Certificate $cert
```

フォーク先の GitHub Actions で自動署名を行う場合は以下の Secrets を設定してください:

| Secret | 内容 |
|--------|------|
| `WINDOWS_CERTIFICATE` | Base64 エンコードした `.pfx` 証明書 |
| `WINDOWS_CERTIFICATE_PASSWORD` | 証明書のパスワード |

---

## クレート構成

```
nexterm/
├── nexterm-proto        # IPC メッセージ型・シリアライズ
├── nexterm-vt           # VT100 パーサ・仮想スクリーン・画像デコード
├── nexterm-server       # PTY サーバー (IPC + セッション管理)
├── nexterm-config       # 設定ロード (TOML + Lua) + StatusBarEvaluator
├── nexterm-client-tui   # TUI クライアント
├── nexterm-client-gpu   # GPU クライアント (wgpu + winit)
├── nexterm-launcher     # nexterm.exe — サーバー自動起動＋GPU クライアント統合
├── nexterm-ctl          # セッション制御 CLI
├── nexterm-i18n         # 多言語対応 (8 言語)
└── nexterm-ssh          # SSH クライアント (russh) — 接続・認証・PTY チャネル
```

## ビルド

### 前提条件

- Rust 1.80 以上
- Windows の場合: Visual Studio Build Tools (C++ コンパイラ)
- Linux の場合: `libx11-dev`, `libxkbcommon-dev`, `libwayland-dev`（winit 依存）

### ビルド

```bash
# 全クレートをビルド
cargo build --release

# サーバーのみ
cargo build --release -p nexterm-server

# GPU クライアントのみ
cargo build --release -p nexterm-client-gpu
```

### テスト

```bash
cargo test
```

## 使い方

### サーバーを起動する

```bash
# デバッグログ付きで起動
NEXTERM_LOG=info nexterm-server

# Windows
set NEXTERM_LOG=info && nexterm-server.exe
```

サーバーは以下のソケットをリッスンする：

| OS | パス |
|----|------|
| Linux / macOS | `$XDG_RUNTIME_DIR/nexterm.sock` |
| Windows | `\\.\pipe\nexterm-<USERNAME>` |

### GPU クライアントを起動する

```bash
nexterm-client-gpu
```

起動時に自動的にサーバーへ接続し、`main` セッションにアタッチする。サーバーが起動していない場合はオフラインモードで起動する。

### TUI クライアントを起動する

```bash
nexterm-client-tui
```

## キーバインド（GPU クライアント）

### 全般

| キー | 動作 |
|------|------|
| `Ctrl+,` | 設定パネルを開く |
| `Ctrl+Shift+P` | コマンドパレットを開く／閉じる |
| `Ctrl+F` | スクロールバック検索を開始する |
| `PageUp` | スクロールバックを上にスクロール |
| `PageDown` | スクロールバックを下にスクロール |
| `Escape` | 検索・パレットを閉じる |
| `Enter`（検索中） | 次のマッチへ移動 |
| `Ctrl+G` | ディスプレイペインモード（ペイン番号表示） |
| `Ctrl+Shift+H` | SSH ホストマネージャを開く |
| `Ctrl+Shift+M` | Lua マクロピッカーを開く |
| `Ctrl+Shift+U` | SFTP アップロードダイアログを開く |
| `Ctrl+Shift+D` | SFTP ダウンロードダイアログを開く |
| `Ctrl+Shift+Space` | Quick Select モード（URL・パス・IP・ハッシュ） |
| `Ctrl+B Z` | フォーカスペインのズームをトグル |
| `Ctrl+B {` | フォーカスペインを前のペインと入れ替え |
| `Ctrl+B }` | フォーカスペインを次のペインと入れ替え |
| `Ctrl+B !` | フォーカスペインを新ウィンドウに切り出し |
| 通常のキー入力 | フォーカスペインの PTY へ転送 |

### フォントサイズ

| キー | 動作 |
|------|------|
| `Ctrl+=` | フォントサイズを 1pt 増やす |
| `Ctrl+-` | フォントサイズを 1pt 減らす |
| `Ctrl+0` | フォントサイズを設定値にリセット |

### クリップボード

| キー | 動作 |
|------|------|
| `Ctrl+Shift+C` | フォーカスペインの可視グリッドをクリップボードにコピー |
| `Ctrl+Shift+V` | クリップボードの内容をフォーカスペインにペースト |

### コピーモード（Vim 風）

| キー | 動作 |
|------|------|
| `Ctrl+[` | コピーモードに入る |
| `h` / `j` / `k` / `l` | カーソルを左 / 下 / 上 / 右に移動 |
| `v` | 選択開始をトグル |
| `y` | 選択範囲をクリップボードにコピーしてモード終了 |
| `q` / `Escape` | コピーモードを終了 |

### マウス操作

| 操作 | 動作 |
|------|------|
| 左クリック | クリックしたペインにフォーカスを移動 |
| 左ドラッグ | テキスト選択（青色ハイライト）。ボタンリリース時に自動クリップボードコピー |
| `Ctrl` + 左クリック | カーソル下の URL をブラウザで開く |
| ホイール上 | スクロールバックを上にスクロール（3行単位） |
| ホイール下 | スクロールバックを下にスクロール（3行単位） |

### ペイン操作（サーバー側プロトコル経由）

| メッセージ | 動作 |
|-----------|------|
| `SplitVertical` | フォーカスペインを左右に分割 |
| `SplitHorizontal` | フォーカスペインを上下に分割 |
| `FocusNextPane` | 次のペインにフォーカス移動 |
| `FocusPrevPane` | 前のペインにフォーカス移動 |
| `FocusPane { pane_id }` | 指定ペインにフォーカス移動（マウスクリック時） |
| `ClosePane` | フォーカスペインを閉じる（兄弟ノードを昇格） |
| `ToggleZoom` | フォーカスペインのズームをトグル |
| `SwapPaneNext` | フォーカスペインを次の兄弟と入れ替え |
| `SwapPanePrev` | フォーカスペインを前の兄弟と入れ替え |
| `BreakPane` | フォーカスペインを新ウィンドウに切り出し |
| `ConnectSerial { path, baud }` | シリアルデバイスに接続 |
| `SftpUpload { ... }` | SFTP アップロード |
| `SftpDownload { ... }` | SFTP ダウンロード |
| `RunMacro { macro_fn, display_name }` | Lua マクロを実行 |

## nexterm-ctl の使い方

サーバーが起動しているときに使用できるセッション制御 CLI。

```bash
# セッション一覧を表示する
nexterm-ctl list

# 新規セッション 'work' を作成する
nexterm-ctl new work

# セッション 'work' へのアタッチ方法を確認する
nexterm-ctl attach work

# セッション 'work' を強制終了する
nexterm-ctl kill work

# PTY 出力をファイルに録画する
nexterm-ctl record start work output.log

# 録画を停止する
nexterm-ctl record stop work

# asciicast v2 形式で録画する
nexterm-ctl record start-cast work cast.cast
nexterm-ctl record stop-cast work

# カラースキームをインポートする
nexterm-ctl theme import ~/.iTerm2/colorscheme.itermcolors
nexterm-ctl theme import ~/.config/alacritty/color.yaml
nexterm-ctl theme import ~/.config/base16.toml
```

## 設定

設定ファイルは以下のパスから自動検索される：

| OS | パス |
|----|------|
| Linux / macOS | `~/.config/nexterm/config.toml` |
| Windows | `%APPDATA%\nexterm\config.toml` |

### nexterm.toml の例

```toml
scrollback_lines = 50000

[font]
family = "JetBrains Mono"
size = 14.0
ligatures = true
font_fallbacks = ["Noto Sans CJK JP", "Noto Color Emoji"]

[colors]
scheme = "tokyonight"

[shell]
program = "/usr/bin/fish"

[status_bar]
enabled = true
widgets = ['os.date("%H:%M:%S")', '"nexterm"']

[window]
background_opacity = 0.95
decorations = "full"   # "full" | "none" | "notitle"

[tab_bar]
enabled = true
height = 28
active_tab_bg = "#ae8b2d"
inactive_tab_bg = "#5c6d74"
separator = "❯"

[[hosts]]
name = "本番サーバー"
host = "192.168.1.100"
port = 22
username = "deploy"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

[[macros]]
name = "top"
description = "フォーカスペインで top を実行"
lua_fn = "macro_top"

[[macros]]
name = "git status"
description = "git status を表示"
lua_fn = "macro_git_status"

[log]
auto_log = false
timestamp = true
strip_ansi = true
log_dir = "~/nexterm-logs"
```

### nexterm.lua オーバーライドの例

```lua
-- ~/.config/nexterm/nexterm.lua
local cfg = require("nexterm")

-- フォントサイズをランタイムで変更する
cfg.font.size = 16.0

-- ステータスバーに時刻とセッション名を表示する
cfg.status_bar.enabled = true
cfg.status_bar.widgets = { 'os.date("%H:%M")', '"main"' }

return cfg
```

> 設定ファイルを保存するとリアルタイムで反映されます（ホットリロード）。

## アーキテクチャ概要

```
┌─────────────────────────────────────┐
│         nexterm-client-gpu          │
│  wgpu レンダラー / winit イベントループ  │
└──────────────┬──────────────────────┘
               │ IPC (bincode / Named Pipe / Unix Socket)
┌──────────────▼──────────────────────┐
│         nexterm-server              │
│  セッション → ウィンドウ → ペイン(PTY) │
│  BSP レイアウトエンジン               │
└──────────────┬──────────────────────┘
               │ portable-pty
┌──────────────▼──────────────────────┐
│       OS PTY (ConPTY / Unix)        │
│       シェル / アプリケーション        │
└─────────────────────────────────────┘
```

詳細は各ドキュメントを参照：

| ドキュメント | 内容 |
|------------|------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | クレート構成・データフロー・レンダリングパイプライン |
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | IPC プロトコル仕様（メッセージ型・フレーミング・シーケンス図） |
| [docs/DESIGN.md](docs/DESIGN.md) | 基本設計書・ADR（設計判断の記録） |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | TOML / Lua 設定の全フィールドリファレンス |
| [CONTRIBUTING.md](CONTRIBUTING.md) | ビルド手順・コーディング規約・PR ガイドライン |

## ライセンス

MIT OR Apache-2.0
