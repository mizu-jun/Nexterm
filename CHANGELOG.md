# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.4.0] - 2026-05-17

v1.3.1 の hotfix からの後続マイナーバージョン。**ユーザー影響のある破壊的変更はなし**（`nexterm`
コマンドの挙動・配布物の構成・MSI ショートカット等すべて維持）。内部構造の整理として
`nexterm-launcher` クレートを削除し workspace を 12 → 11 クレートに整理したため、PATCH ではなく
MINOR を採用した。プレフィックスバインドの誤発火バグ修正（実害あり）も同梱。

### Fixed

- **プレフィックスバインドの誤発火を修正** (`config_key_matches`): 旧実装は `split_whitespace().last()`
  で末尾トークンのみ評価していたため、`keys = [{ key = "<leader> d", action = "ClosePane" }]`
  等の設定下で `d` 単独押下にも誤マッチしてアクションが即発火していた（`"<leader> %"` は `5`
  単独押下で発火する経路もあり）。`config_key_matches` をスペース区切りなら false を返す単発キー
  専用に変更し、`ClientState.prefix_pending_until: Option<Instant>` を追加。Leader 単独押下時に
  `<leader> X` 形式バインドが 1 件以上設定されている場合のみ prefix モード突入＋ PTY 送信抑制。
  `check_config_keybindings` を prefix / 単発の 2 経路に分割、2 秒で自動解除。key_map に 13 件の
  ユニットテスト追加（バグ回帰防止 4 件 + 単発正常系 + エッジケース 9 件）

### Removed

- **`nexterm-launcher` クレートを削除**: v0.9.3 で `nexterm-client-gpu` がサーバーを内部 tokio
  タスクとして起動する**シングルバイナリ設計**（`bin name = "nexterm"`）に移行した時点で
  launcher は役目を終えていたが、削除し忘れていた。v1.3.1 まで `nexterm-launcher` と
  `nexterm-client-gpu` の両方が `bin name = "nexterm"` で衝突しており、`cargo build` のコンパイル
  順により `target/release/nexterm` がどちらか上書きされる脆い状態（実態は client-gpu 版が勝ち
  取っていた）。今回 launcher クレートを完全削除し bin name 衝突を根本解消。WiX / Flatpak /
  `release.yml` の `if [ -f ]` ガード（client-gpu 用）も整理。一般的なターミナルエミュレータ
  （Alacritty / kitty / Ghostty / WezTerm）と同様に「メインバイナリ 1 つ（`nexterm`）+ 補助 CLI
  数個（`nexterm-ctl` / `nexterm-client-tui` / `nexterm-server`）」の構成になった。ユーザーへの
  影響なし（`nexterm` コマンドの挙動・配布物の構成・MSI ショートカット等すべて維持）。workspace
  は 12 → 11 クレートに整理

### Documentation

- **公開ドキュメントの実態乖離を解消**: 新規ユーザーが誤解しやすい古い記述を一括修正。
  - `bincode` → `postcard` の更新漏れ（Sprint 5-1 / ADR-0006 で 2026-05-12 に移行済み）を
    README.md / README.ja.md / docs/ARCHITECTURE.md / docs/THREAT_MODEL.md / docs/PROTOCOL.md
    の本文 + 図表で修正。docs/DESIGN.md の ADR-004 は ADR-0006 で superseded と明示
  - `PROTOCOL_VERSION = 1` → `7`（README/README.ja の v1.1.0 セクション。"最新値は
    nexterm-proto/src/lib.rs 参照" 注釈付き）
  - Rust 最低バージョン `1.78` / `1.80` → `1.85`（workspace の `edition = "2024"` 必須）を
    README.md / README.ja.md / docs/src/install.md で訂正
  - `SNAPSHOT_VERSION = 2` → `3`（Sprint 5-7 / Phase 2-1 で `workspace_name` 追加済み）を
    CLAUDE.md / docs/THREAT_MODEL.md / docs/adr/0007-snapshot-v1-deprecation.md で訂正
  - CONTRIBUTING.md / CONTRIBUTING.ja.md の依存リスト bincode → postcard
  - nexterm-client-core/src/lib.rs のフレーミングコメント postcard 化
  - README.md のテスト数 "240+ tests" → 実測 **660+**（docs/ARCHITECTURE.md の Test Strategy 表
    にも現状値の注釈を追加）
  - README.ja.md の「デーモンレス設計」表現を「内部 tokio タスクが PTY を保持」と
    シングルバイナリ実装に合わせて明示

### Build

- workspace version `1.3.1` → `1.4.0`（破壊的変更なしの整理リリース）

## [1.3.0] - 2026-05-17

Sprint 5-6（GPU クライアントの大規模ファイル分割）と Sprint 5-7（UI/UX モダン化 Phase 1 + 2 + 3）
完了に伴うマイナーバージョンリリース。IPC プロトコルとスナップショットフォーマットに
互換性破壊を含むため、移行時は [docs/MIGRATION.md](docs/MIGRATION.md) を必ず参照すること。

### 互換性破壊サマリ（v1.2.0 → v1.3.0）

- **`PROTOCOL_VERSION` を `4` → `7` に bump**
  - `5`: ワークスペース IPC（`ListWorkspaces` / `CreateWorkspace` / `SwitchWorkspace` /
    `RenameWorkspace` / `DeleteWorkspace` + `WorkspaceList` / `WorkspaceSwitched`） (Phase 2-1)
  - `6`: Quake モード IPC（`QuakeToggle` + `QuakeToggleRequest`） (Phase 2-2)
  - `7`: タブ並べ替え IPC（`ReorderPanes`） (Phase 2-3)
- **`SNAPSHOT_VERSION` を `2` → `3` に bump**: `SessionSnapshot.workspace_name` と
  `ServerSnapshot.current_workspace` を追加。v2 JSON は `serde(default)` で自動マイグレ
- **新サーバーは旧クライアントを Hello ハンドシェイクで拒否し、旧サーバーは新クライアントを拒否**。
  クライアントとサーバーは必ず一緒にアップグレードすること

### Sprint 5-7 Phase 3 — 視覚的洗練

- **背景画像対応** (Phase 3-1): `[window.background_image]` で壁紙風の背景画像を表示。
  `fit = "cover"/"contain"/"stretch"/"center"/"tile"` の 5 モード + 不透明度調整。
  4096x4096 超は Lanczos3 で自動ダウンスケール。既存 `image_pipeline`（Sixel/Kitty 用）を再利用
- **UI アニメーション** (Phase 3-2): タブ切替 200ms（アクセントライン伸長 + フェードイン）と
  ペイン追加 250ms（白いオーバーレイフェードアウト）の ease-out アニメーション。
  `[animations] enabled = false` または `intensity = "off"` で即時反映に切替可能
  （reduced motion 対応）。intensity 4 段階: `off`/`subtle`(×0.5)/`normal`(×1.0)/`energetic`(×1.5)
- **コマンドパレット網羅 + 履歴永続化** (Phase 3-3): 不足アクション 6 件追加（Quit/ClosePane/
  NewWindow/QuickSelect/SetBroadcastOn/SetBroadcastOff）で全 25 アクション完備。
  使用履歴を `~/.local/state/nexterm/palette_history.json` (atomic write + Unix 0600) に永続化。
  クエリ空時は履歴順、クエリ有時は fuzzy スコア + history_bonus（use_count×10 上限 100 +
  24h以内 +100 / 1週間以内 +50）でランキング

### Sprint 5-7 Phase 2 — 目玉機能

- **ワークスペース機能** (Phase 2-1): セッションをグループ化する「ワークスペース」概念を導入。
  `nexterm-ctl workspace list/create/switch/rename/delete [--force]` サブコマンド追加。
  ステータスバーの `workspace` ビルトインウィジェットで現在のワークスペースを表示。
  `default` ワークスペースは削除・リネーム不可
- **Quake モード** (Phase 2-2): グローバルホットキー（既定: `Ctrl+\``）でウィンドウを画面端
  （Top/Bottom/Left/Right）から滑り出させて表示するモード。`global-hotkey` 0.8 crate を使用。
  Wayland は global hotkey API 未対応のため `nexterm-ctl quake toggle/show/hide` 経由で
  compositor の bindsym から呼ぶワークアラウンドを提供
- **タブ並べ替え** (Phase 2-3): タブバーのタブを左右にドラッグして並べ替え可能。
  6px の閾値超でドラッグ確定、未満ならクリック扱い。ドラッグ中はゴーストタブ + 挿入位置
  インジケータを描画。`pane_order: Vec<u32>` を物理レイアウトから分離して管理

### Sprint 5-7 Phase 1 — 磨き上げ群

- **タブ色分け動的化 + ホバーハイライト** (UI-1-1): `TabBarConfig` に `activity_tab_bg` /
  `active_accent_color` / `show_tab_number` / `inactive_text_brightness` / `hover_highlight`
  を追加。マウスホバー時にタブ背景を明るく
- **ステータスバー右端ウィジェット拡張** (UI-1-2): ビルトインに `cwd` / `cwd_short` /
  `git_branch` / `workspace` を追加。`WidgetContext` を拡張して focused pane の cwd を伝播
- **Leader key 対応** (UI-1-3): `Config.leader_key` で `<leader>` プレースホルダーを設定可能。
  WezTerm 風の prefix キーバインドを簡潔に記述できる
- **キーヒントオーバーレイ** (UI-1-4): Leader 単独押下で 2 秒間、prefix 系バインドを画面下部に
  半透明表示。新規 `renderer/overlay/key_hint.rs` モジュール

### Sprint 5-6 — GPU クライアントの大規模ファイル分割（リファクタリング）

機能変更なし。GPU クライアントの 4 つの巨大ファイルをサブモジュールに分割し、保守性を改善。

- `event_handler.rs` (1,318 行) → 7 サブモジュール（consent / settings_panel_hit / lifecycle /
  window / mouse / keyboard）
- `input_handler.rs` (1,377 行) → 6 サブモジュール
- `renderer/mod.rs` (1,579 行) → 6 ファイル（wgpu_init / render_frame / event_handler / 等）
- `state.rs` (1,319 行) → 7 ファイル（pane / search / selection / menus / consent /
  server_message + state/mod.rs）

### 追加された i18n エントリ

8 言語（en/ja/zh-CN/ko/de/fr/es/it）に Sprint 5-7 関連の新規 UI 文字列を追加。
コマンドパレット用 6 キー × 8 言語 = 48 件、ワークスペース・Quake・キーヒント関連を含む。



監査ラウンド 2（70 件タスク）の Sprint 5-1 〜 5-5 完了に伴うリリース。
互換性破壊を含む変更があるため、移行時は
[docs/MIGRATION.md](docs/MIGRATION.md) を必ず参照すること。

### 互換性破壊サマリ（v1.1.0 → v1.2.0）

- **`PROTOCOL_VERSION` を `1` → `4` に bump**
  - `2`: SSH パスワード IPC 平文流通の排除 (Sprint 5-1 / G1)
  - `3`: IPC ワイヤフォーマットを bincode → postcard 移行 (Sprint 5-1 / G3)
  - `4`: OSC 7 CWD reporting と `CwdChanged` イベント追加 (Sprint 5-2 / B2)
- **IPC ワイヤフォーマット**: bincode → postcard に置換
  （旧 v1.1.0 client は v1.2.0 server に接続不可）
- **GPU present mode のデフォルト**: `fifo` → `mailbox`
  （tearing を許容して 1 frame レイテンシ低減、明示的に `present_mode = "fifo"` で従来挙動に戻せる）

### Sprint 5-5 — テスト・観測性・ドキュメント (I1/I2/A6/A9/J1/J2)

- **`nexterm-ssh` 単体テスト 15 件追加** (I1): `parse_jump_spec` / `parse_socks5_credentials` /
  `parse_forward_spec` / `SshConfig` 構築 / 到達不能ポートへの fast-fail。
  完全なモック SSH サーバーは将来課題。
- **`nexterm-launcher` smoke テスト 5 件追加** (I2): `server_exe`/`client_exe`/`tui_exe` の
  OS 別拡張子・`exe_dir`・`wait_for_server` タイムアウト経路。
- **`tracing::instrument` を主要 async に付与** (A6): `SshSession::connect/authenticate/open_shell`、
  `persist::save_snapshot/load_snapshot`、IPC `dispatch_inner`。
  `dispatch_inner` は機密ペイロード保護のため `skip_all`。
- **Snapshot v1 削除タイミングを ADR-0007 で明示** (A9): v2.0.0 で `SNAPSHOT_VERSION_MIN`
  を `1` → `2` に bump 予定。`nexterm-plugin` の v1 削除タイミングも ADR-0003 へ参照を統一。
- **mdBook 骨子整備** (J1): `docs/src/troubleshooting.md` / `docs/src/adr-index.md` 新規追加。
  `SUMMARY.md` に Reference セクション。`README.md` を v1.2.0 ベースに更新。
- **rustdoc 警告 9 → 0** (J2): `[[macros]]` / `vec2<f32>` / `https://...` / `rows[y][x]` を
  backtick で囲んでリンク解釈を抑制。`cargo doc --no-deps --lib --workspace` が警告 0 で完走。

### Sprint 5-4 — アーキテクチャ整理 + UX + ADR (A1/A2/A3/D1/D4/D8/E1/F1/J3)

- **`overlay_verts.rs` (1,958 行) を 5 ファイルに分割** (A2):
  `renderer/overlay/{picker, dialog, settings, util, mod}.rs`。最大 settings.rs 795 行。
- **`nexterm-ctl/main.rs` (1,757 行) を分割** (A1):
  `main.rs` 343 行 + `ipc.rs` 96 行 + `cmd/{session, record, template, service, ghostty, theme, plugin, wsl, util, mod}.rs`。
- **`nexterm-server/web/mod.rs` (1,088 行) を分割** (A3):
  `mod.rs` 247 行 + `router.rs` 129 行 + `middleware.rs` 144 行 + `handlers/{page, login, oauth, ws, assets, mod}.rs`。
- **examples/plugins (4 件) を Plugin API v2 化** (F1): `nexterm_api_version() -> 2` を追加、
  プラグインバージョンを `0.2.0` へ bump、`examples/plugins/README.md` に v1→v2 移行ガイド。
- **WSL ディストロ自動検出 + Profile インポート** (E1):
  `nexterm-ctl wsl import-profiles [--dry-run]` で Ubuntu 等の WSL ディストロを
  自動検出して `config.toml` の `[[profiles]]` を生成。
- **Quick Select 拡充: パターン 5 → 11 種** (D1): Email / UUID / file:line / Jira /
  Windows path / IPv6 等を追加。優先順位付き + 重複排除 + 単体テスト 10 件。
- **テーマギャラリー隠れバグ修正 + `nexterm-ctl theme` サブコマンド** (D4):
  `parse_builtin_scheme` が Catppuccin / Dracula / Nord / OneDark を Dark にフォールバック
  していた問題を修正。`BuiltinScheme::all() / from_toml_name()` 追加。
- **Pane Zen mode に `Ctrl+Shift+Z` 代替バインド追加** (D8): tmux 流の `Ctrl+B Z` に加えて。
- **ADR ディレクトリ整備** (J3): `docs/adr/` に template / README index / 遡及 ADR 5 件
  (0002-0006) を整備。

### Sprint 5-3 — 性能計測基盤 (C5/C1/C2/C3/I5/J4)

- **`nexterm-vt` の criterion ベンチマーク導入** (C5): VT パーサ・スクロール・Sixel デコードの
  性能リグレッション検出が可能に。`cargo bench -p nexterm-vt`。
- **入力レイテンシ計測スクリプト追加** (C1): VT advance 1 ms / wgpu present queue サイズ
  などをスクリプト化。
- **wgpu アップグレード方針を ADR-0001 化** (C2): 22 → 26 をテスト・代替分析と共に整理。
- **present_mode のデフォルトを `mailbox` に変更** (C3): 1 frame 短縮。
  `[gpu] present_mode = "fifo"` で従来挙動に戻せる。
- **GitHub Actions に coverage ジョブ追加** (I5): `cargo-llvm-cov` で `target/coverage` を生成。
- **ベンチマーク結果を `docs/benchmarks.md` に公表** (J4): リファレンス値と再計測手順を明文化。

### Sprint 5-2 — ターミナル互換性 (B1/B2/B5)

- **OSC 133 (semantic prompt marks) + jump-to-prompt 完全対応** (B1):
  クライアントでプロンプト境界を記録し、`Ctrl+Up` / `Ctrl+Down` で前後プロンプトへジャンプ。
  コマンドパレットからも「Jump to previous prompt / next prompt」を選択可能。8 言語対応。
- **OSC 7 (CWD reporting) + 親 CWD 継承** (B2):
  `CwdChanged` IPC イベントを追加（`PROTOCOL_VERSION` 4）。
  分割で新規 pane を作成する際に親 pane の CWD を継承する。
- **Synchronized Output (DCS=2026) のテスト整備** (B5):
  既存実装の挙動を VT スナップショットテストで固定化。

### Security — Sprint 5-1 (G3) IPC ワイヤフォーマットを bincode → postcard へ移行

**互換性破壊**: `PROTOCOL_VERSION` が `2` → `3` にバンプ。詳細は
[docs/MIGRATION.md](docs/MIGRATION.md) を参照。

- **`bincode = "1"` を全クレートで撤去**し、`postcard = "1" (use-std)` に置換。
  - 対象クレート: `nexterm-proto` / `nexterm-server` / `nexterm-client-core` /
    `nexterm-client-gpu` / `nexterm-client-tui` / `nexterm-ctl`
  - 対象呼び出し: `bincode::serialize` → `postcard::to_stdvec`、
    `bincode::deserialize` → `postcard::from_bytes`（実装 3 箇所 + テスト 19 箇所）
- **`RUSTSEC-2025-0141` (bincode 1.x unmaintained) の `deny.toml` ignore を削除**。
  `cargo deny check` の `advisories` セクションが ignore 0 件で通過。
- **副次効果**: postcard の varint エンコードで IPC メッセージが平均 10〜20% 縮小。
- 効果: bincode 1.x への lock-in を解消、長期メンテ可能なサプライチェーンへ。

### Security — Sprint 5-1 (G1) SSH パスワード IPC 平文流通の排除

**互換性破壊**: `PROTOCOL_VERSION` が `1` → `2` にバンプ。詳細は
[docs/MIGRATION.md](docs/MIGRATION.md) を参照。

- **`ClientToServer::ConnectSsh` から `password: Option<String>` を削除**。
  代わりに以下を導入:
  - `password_keyring_account: Option<String>` — OS キーリングのアカウント識別子
  - `ephemeral_password: bool` — 認証完了後に keyring エントリを削除するフラグ
- **クライアント (nexterm-client-gpu)**: `connect_ssh_host_with_password()` で
  事前に `nexterm_config::keyring::store_password()` を呼んで保存し、IPC では
  account 名のみ送信する。`PasswordModal.remember=false` の場合は
  `ephemeral_password=true` を立てる。
- **サーバー (nexterm-server)**: `handle_connect_ssh()` で
  `nexterm_config::keyring::get_password()` から取得し、`Zeroizing<String>` で
  russh に渡す。`ephemeral_password=true` のときは認証完了後に削除。
- 効果: Unix Domain Socket / Named Pipe 上でパスワード平文が流れなくなり、
  HIGH H-6 (`input_handler.rs` の TODO) が解消。

### Security — Sprint 5-1 (G2) GitHub Actions SHA ピン留め

- **全 GitHub Actions を Git SHA でピン留め**（SLSA 2 要件）。
  `actions/checkout@v4` のような可変タグ参照を、対応する Git commit SHA
  + `# v4.3.1` 形式のコメントに変換した。これにより、上流アクション側で
  タグが書き換えられた場合のサプライチェーン攻撃に耐性を持つ。
  - 対象: `ci.yml` / `release.yml` / `sbom.yml` / `fuzz.yml` / `flatpak.yml` / `pages.yml`（合計 36 箇所）
  - ピン留め対象アクション 9 種:
    `actions/checkout`、`actions/upload-artifact`、`actions/upload-pages-artifact`、
    `actions/deploy-pages`、`actions/attest-build-provenance`、
    `Swatinem/rust-cache`、`EmbarkStudios/cargo-deny-action`、
    `softprops/action-gh-release`、`dtolnay/rust-toolchain` (stable/nightly)

## [1.1.0] - 2026-05-10

Sprint 1〜4 全完了の総まとめリリース。互換性破壊を含む変更があるため、移行時は
[docs/MIGRATION.md](docs/MIGRATION.md) を必ず参照すること。

### Added — Sprint 4-2 プラグイン API v2

- **`PLUGIN_API_VERSION = 2`** にバンプ。新ホスト規約:
  - **入力サニタイズ**: `nexterm_on_output` / `nexterm_on_command` に渡す前に
    ESC・OSC/CSI/DCS/APC シーケンス・C0 制御文字（`\t\r\n` 除く）を除去。
    プラグインに渡る情報をプレーンテキストに限定する。
  - **`write_pane` PaneId 許可リスト**: 呼び出しスコープごとに許可された
    pane_id でのみ書き込み可能。`nexterm_on_output(pane_id, ...)` 中は
    その `pane_id` のみ、`nexterm_on_command` 中はどのペインにも書き込めない。
- **`MIN_SUPPORTED_API_VERSION = 1`** で v1 プラグインの後方互換を維持。
  v1 プラグインは旧挙動（サニタイズなし、書き込み制限なし）で動作するが、
  ロード時に deprecation 警告がログに出る。
- **`PluginInfo.api_version`** フィールド追加（`nexterm-ctl plugin list` で表示）。
- **`sanitize_for_plugin(input: &[u8]) -> Vec<u8>`** を pub 公開（テスト・診断用）。

### Added — Sprint 4-4 プロパティテスト

- **`proptest` (1.x) を workspace 依存に追加**（`[workspace.dependencies]`）。
  `nexterm-vt` / `nexterm-server` の `[dev-dependencies]` から参照。
- **Sixel / Kitty パーサプロパティテスト** (`nexterm-vt/tests/proptest_image.rs`):
  - `decode_sixel` / `decode_kitty` が任意バイト列でパニックしないこと
  - 成功時は `rgba.len() == width * height * 4` が常に成立
  - 巨大寸法 (>= 8193x8192) は必ず None で拒否
  - VtParser に渡す経路（APC 含む）でもパニック耐性を確認
- **BSP / タイリングプロパティテスト** (`nexterm-server/src/window/tests.rs`):
  - 任意の Insert/Remove 操作シーケンスで `compute()` がパニックしないこと
  - 十分な領域がある場合、矩形が画面内に収まり・重ならず・ID 一意
  - スナップショット往復が pane ID と矩形を保存
  - タイリング不変条件（pane 数一致・領域内・ID 一致）

### Security — Sprint 1〜3 セキュリティ強化

包括的なセキュリティ監査により判明した CRITICAL / HIGH 課題を修正。
**互換性破壊を含む変更があります。詳細は [docs/MIGRATION.md](docs/MIGRATION.md) を参照。**

#### 認証・認可（Web ターミナル）

- **OAuth GitHub Org 検証バイパス修正**（CRITICAL）: 旧実装は `get_current_token()` が常に `None` を返すバグで Org メンバーシップ検証が一切実行されていなかった。`exchange_code()` の戻り値に `access_token` を含めて `is_user_allowed()` に伝播するよう修正。
- **TOTP リプレイ攻撃対策**（CRITICAL）: 同一 OTP コードの ±1 ウィンドウ内再利用を `subtle::ConstantTimeEq` 定数時間比較 + `HashSet<(window, code)>` で検出・拒否。
- **TOTP IP ベースレート制限**（CRITICAL）: `5 試行/60 秒` のブルートフォース対策を `web::rate_limit` モジュールで実装。429 + `Retry-After: 60` を返す。
- **TLS フォールバック既定禁止**（CRITICAL）: TLS 設定失敗時の HTTP サイレント降格を廃止。`[web] allow_http_fallback = true` で明示オプトインが必要。
- **OIDC userinfo_endpoint SSRF 対策**（HIGH）: HTTPS 強制 + 内部 IP 拒否 + issuer ドメイン一致検証。
- **legacy_token 定数時間比較**（HIGH）: `subtle::ConstantTimeEq` でタイミング攻撃を防止。

#### IPC / プロトコル

- **bincode メッセージサイズ上限**（CRITICAL）: `MAX_MSG_LEN = 64 MiB` でローカル OOM 攻撃を防止（サーバー / GPU/TUI クライアント / ctl の 4 箇所）。
- **プロトコル Hello + バージョニング**: 接続時に `ClientToServer::Hello { proto_version, client_kind, client_version }` を必須化。`PROTOCOL_VERSION = 1`。バージョン不一致は接続切断。

#### VT パーサ・画像

- **VT バッファ上限**（CRITICAL）: APC 4 MiB / DCS Sixel 16 MiB / Kitty 分割転送 64 MiB の上限導入。悪意ある PTY による DoS 防止。
- **画像デコード u32 オーバーフロー修正**（CRITICAL）: `width * height * 4` を u64 で計算し `MAX_IMAGE_BYTES = 256 MiB` で制限。
- **OSC 8 URI allowlist**（CRITICAL）: `javascript:` / `file:` 等のスキームを拒否、許可スキームは `http/https/mailto/ftp/ftps/ssh`。タイトル 256 / 通知 1024 / URI 2048 バイト上限。

#### サンドボックス

- **Lua サンドボックス**（CRITICAL）: `os` / `io` / `package` / `require` / `dofile` / `loadfile` / `debug` を無効化。`config.lua` 経由の RCE を阻止。
- **WASM サンドボックス強化**（CRITICAL）: wasmi の `consume_fuel(true)` + 各呼び出し前に `FUEL_PER_CALL = 10M` を供給。`MAX_MEMORY_PAGES = 256` (16 MiB) でメモリ上限。`nexterm_api_version()` でロード時バージョン検証。Mutex ポイズン回復。

#### シークレット・永続化

- **snapshot/host_history を atomic write + 0600**（CRITICAL）: 一時ファイル → fsync → rename + Unix では `mode(0o600)` 強制。クラッシュ時破損 + 機密情報漏洩を防止。
- **TLS 秘密鍵 0600 強制保存**（HIGH）: 自己署名証明書生成時の鍵ファイルを umask 非依存で 0600 化。
- **GUI PasswordModal `Zeroizing<String>`**（HIGH）: パスワード入力バッファを drop 時にメモリゼロクリア。

#### ロギング

- **アクセスログのクエリ文字列除去**（HIGH）: OAuth `?code=` / `?state=` / `?token=` 等の機密情報がアクセスログに残るのを防ぐ。

### Fixed

- **TomlConfig 機能不全修正**: 旧 `TomlConfig` 中間構造体は `window/web/hosts/macros/log/cursor_style/auto_check_update/language` 等を欠いており、ユーザーが `config.toml` に書いた設定の大半がサイレント無視されていた。`Config` を直接 deserialize する設計に変更。
- **DEFAULT_CONFIG_TOML テンプレート修正**: 初回起動時テンプレートが `[color_scheme] builtin = ...` / `[tab_bar] show = ...` 等の実装と一致しないキーを使用していた。実装に合わせたキー名に修正。
- **CI 修復**: `cargo fmt --check` が master で失敗していた状態を解消。

### Added

- **テスト**: 全 CRITICAL/HIGH 修正に核心テストを追加（合計 約 60 件、proto / vt / config / server / plugin の各クレート）。
- **`docs/MIGRATION.md`**: 互換性破壊変更（Lua サンドボックス・プロトコル Hello・TLS フォールバック既定禁止）の移行ドキュメント。

## [1.0.0] - 2026-04-27

### Added

- **v1.0.0 リリース**: 0.9.x 系の全機能が安定し、v1.0.0 として正式リリース。
  - Plugin API v1 フリーズ（`PLUGIN_API_VERSION = 1`）による安定 ABI 保証
  - WASM プラグインランタイム（wasmi）+ `nexterm-ctl plugin` CLI
  - SSH ホスト履歴永続化・パスワード認証モーダル
  - スナップショットスキーマ v2（自動マイグレーション付き）
  - GPU レンダラー（wgpu + cosmic-text）3パスレンダリングパイプライン
  - 8言語 i18n 対応（en/ja/zh-CN/ko/de/fr/es/it）
  - 自動更新通知（GitHub Releases API ポーリング）
  - 設定パネル（7カテゴリ、TOML 書き戻し、ホットリロード）
  - Web ターミナル（axum WebSocket + xterm.js）
  - シリアルポート接続サポート

### Changed

- **CI ブランチ修正**: `.github/workflows/ci.yml` のトリガーブランチを `main`/`develop` から `master` に修正。デフォルトブランチへの push・PR で CI が自動実行されるようになった。

---

## [0.9.15] - 2026-04-27

### Added

- **MSI 自動更新通知**: 起動 5 秒後にバックグラウンドで GitHub Releases API をポーリングし、現在バージョンより新しいリリースがある場合は画面上部に緑色バナーを表示する。
  - `update_checker` モジュール（`nexterm-client-gpu/src/update_checker.rs`）を新規追加
  - `tokio::sync::watch` で非同期に最新バージョンを通知
  - バナーは `Esc` で閉じる、`Enter` でデフォルトブラウザでリリースページを開く
- **`auto_check_update` 設定フィールド**: `config.toml` に `auto_check_update = true/false` を追加。デフォルト `true`。
- **設定パネル連携**: Startup カテゴリに `auto_check_update` トグルを追加（`Space` キーでトグル、`Enter` で保存）。
- **i18n 8言語対応**: `update-available` / `update-dismiss` / `update-open-releases` キーを全 8 言語に追加。

---

## [0.9.14] - 2026-04-27

### Added

- **Plugin API freeze (`PLUGIN_API_VERSION = 1`)**: Stable WASM ABI is now versioned. `nexterm-plugin` exports `PLUGIN_API_VERSION: u32 = 1` and provides `nexterm.api_version() -> i32` as a host import so plugins can verify compatibility at runtime.
- **`nexterm_meta` plugin export**: Plugins can now export `nexterm_meta(name_buf, name_max, ver_buf, ver_max) -> i32` to publish their name and version. Displayed in `nexterm-ctl plugin list`.
- **`unload` / `reload` methods on `PluginManager`**: Plugins can be unloaded (by path) or reloaded (unload + load) at runtime without restarting the server.
- **IPC plugin commands**: Four new `ClientToServer` messages: `ListPlugins`, `LoadPlugin { path }`, `UnloadPlugin { path }`, `ReloadPlugin { path }`. Corresponding `ServerToClient` responses: `PluginList { paths }`, `PluginOk { path, action }`.
- **`nexterm-ctl plugin` subcommands**: `list`, `load <path>`, `unload <path>`, `reload <path>`.
- **`PluginManager` embedded in `SessionManager`**: Plugin manager is now accessible from the IPC dispatch layer via `manager.plugin_manager`.
- **`echo-suppress` sample plugin** (`examples/plugins/echo-suppress/`): Demonstrates `nexterm_meta`, `api_version()` import, and output suppression.
- **`docs/plugin-api.md`**: Full Plugin API reference documenting all host imports, plugin exports, memory layout, and CLI management.

### Changed

- **`PluginInfo`** now includes `name: Option<String>` and `version: Option<String>` fields populated from `nexterm_meta`.
- Existing sample plugins README updated to include `echo-suppress`.

---

## [0.9.13] - 2026-04-26

### Added

- **Host history persistence**: Connection history is now saved to `~/.local/state/nexterm/host_history.json` (Unix) / `%APPDATA%\nexterm\host_history.json` (Windows). Frequently-connected hosts sort to the top across restarts.
- **Password authentication modal**: Selecting a host with `auth_type = "password"` in the SSH Host Manager now opens a password input overlay. Password characters are masked with `*`. Press Enter to connect, Esc to cancel.
- **`record_connection` wired**: Entering a host from the Host Manager now records the connection in history and persists it to disk immediately.

### Changed

- **`HostManager::new`** now calls `load_history()` on startup so previously recorded frequencies are available immediately.
- **`PasswordModal` struct** added to `host_manager` module with `push_char`, `pop_char`, and `take_password` methods.

---

## [0.9.12] - 2026-04-26

### Improved

- **Snapshot schema v2**: Added `session_title` field to `SessionSnapshot` for future display title support. Old v1 snapshots are automatically migrated on load.
- **Snapshot migration**: `persist::load_snapshot` now migrates v1 snapshots to v2 instead of discarding them. Supported version range: v1–v2.
- **Version guard**: `restore_from_snapshot` now accepts snapshots in the supported range (v1–v2) instead of requiring an exact version match.

### Added (tests)

- `test_v1_snapshot_migrates_to_v2`: Verifies that a v1 JSON snapshot deserializes correctly with `session_title` defaulting to `None`.
- `test_session_title_defaults_to_none`: Verifies backward-compat deserialization when `session_title` is absent.

---

## [0.9.11] - 2026-04-26

### Security

- **russh 0.58 → 0.59**: Mitigated pre-authentication DoS vulnerability (keyboard-interactive unbounded allocation). Updated `AgentIdentity::public_key()` call to match the new `authenticate_publickey_with` signature in russh 0.59.
- **lru 0.12 → 0.17**: Resolved `IterMut` stacked-borrows violation in the glyph atlas LRU cache.

---

## [0.9.10] - 2026-04-26

### Added

- **Cursor style**: New `cursor_style` config option (`"block"` / `"beam"` / `"underline"`) to control the cursor shape in the GPU renderer.
- **Window padding**: New `[window] padding_x` / `padding_y` config options to add pixel padding around the terminal grid.
- **Present mode**: New `[gpu] present_mode` config option (`"fifo"` / `"mailbox"` / `"auto"`) to control wgpu vsync behaviour.
- **Default color scheme**: Changed default color scheme to `TokyoNight`.

### Improved

- **Glyph atlas LRU cache**: Replaced the `HashMap`-based glyph cache with an `LruCache` to automatically evict stale entries after font changes, reducing memory waste.
- **Atlas size from config**: `[gpu] atlas_size` is now used as the maximum texture size for the glyph atlas. Initial size starts at half `atlas_size` (minimum 1024) and grows on demand.
- **Broadcast channel capacity**: Increased IPC broadcast channel capacity from 512 → 2048 to reduce dropped messages under heavy output.
- **Pane border visibility**: Increased separator width from 1 px → 2 px and adjusted border colour for better contrast with the Tokyo Night theme.

### Fixed

- **clippy lint**: Resolved `type_complexity` lint in `nexterm-server/src/web/oauth.rs` by introducing a `OAuthClient` type alias. Resolved `collapsible_if` lint in `nexterm-server/src/lib.rs`.

---

## [0.9.9] - 2026-04-25

### Fixed

- **Touchpad scrolling**: Fixed an issue where Windows touchpad scroll events (PixelDelta) were silently ignored. Added an accumulation buffer that triggers a line scroll once enough delta accumulates to equal one cell height.
- **Font ligatures**: Fixed an issue where `[font] ligatures = true` in the config file was not correctly passed through to FontManager.

### Improved

- **CI quality**: Removed `continue-on-error: true` from the Windows ConPTY integration test so that test failures now cause the build to fail.
- **WiX build stability**: Changed version injection to use `candle.exe -dVersion=X.Y.Z` flag instead of modifying the source file (`wix/main.wxs`) directly.

### Fixed (tests)

- **`window_config_default_value` test**: Fixed a mismatch where the test expected `background_opacity` to be `1.0` even after the default was changed to `0.95`.

---

## [0.9.8] - 2026-04-25

### Fixed

- **PowerShell auto-launch**: Fixed an issue where PowerShell did not start automatically on Windows. The config including the `-NoLogo` argument is now correctly propagated to all pane creation paths.
- **Window transparency**: Fixed an issue where the window background was not transparent on first launch without a config file. Changed the default opacity to 0.95.
- **Freeze on close**: Fixed a hang when closing the window with the × button. The IPC connection is now dropped before the server task is terminated.
- **Context menu text overflow**: Fixed shortcut key labels overflowing outside the menu border. Unified drawing position calculation to use `visual_width()`.

### Changed

- **Dependency update**: Updated `rand` from 0.8.6 to 0.9.4.

---

## [0.9.7] - 2026-04-20

### Added

- **Language selection UI**: Added the ability to select the UI language during installation and from the settings panel (8 languages supported).

### Fixed

- **Context menu width**: Fixed menu overflow for languages with longer translated text.
- **Freeze on window close**: Fixed a hang that occurred when attempting to close the window.
- **PowerShell detection**: Improved accuracy of automatic PowerShell shell detection.

---

## [0.9.6] - 2026-04-19

### Improved

- **nexterm-server ipc.rs module split**: Split `ipc.rs` (1707 lines) into 5 submodules for improved maintainability.
  - `ipc/platform.rs` — Unix Domain Socket / Windows Named Pipe listener and UID verification
  - `ipc/handler.rs` — Read/write loop for connected clients
  - `ipc/dispatch.rs` — Dispatch logic for 40+ IPC commands
  - `ipc/key.rs` — Keycode → VT escape sequence conversion (8 unit tests)
  - `ipc/sftp.rs` — SFTP upload/download helpers

- **Integration tests added**: Added 2 files under `nexterm-server/tests/`.
  - `ipc_integration.rs` — Round-trip tests for bincode serialization + 4-byte LE framing (14 tests)
  - `snapshot_roundtrip.rs` — JSON round-trip and persistence tests for session snapshots (6 tests)

- **`#![warn(missing_docs)]` applied workspace-wide**: Applied to 6 crates (nexterm-vt / nexterm-ssh / nexterm-plugin / nexterm-config / nexterm-server / nexterm-i18n) with missing documentation added in bulk.

### Fixed

- **Reduced `unwrap()` in production code**: Converted unsafe `unwrap()` calls in `web/mod.rs`, `web/auth.rs`, `web/oauth.rs`, `window.rs`, `nexterm-plugin`, and `nexterm-ssh` to `expect("reason")` for improved panic diagnostics.
- **`persist::state_dir()`**: Fixed to prefer the `XDG_STATE_HOME` environment variable (for test isolation and XDG compliance).

---

## [0.9.5] - 2026-04-18

### Added

- **CLAUDE.md**: Added project guide for Claude Code. Documents build commands, architecture overview, and coding conventions.
- **docs/KEYBINDINGS.md**: Extracted the complete key binding reference into a standalone file.

### Changed

- **Dependency updates**: Updated 104 packages to their latest compatible versions, including `vte` 0.13 → 0.15, `cosmic-text` 0.12 → 0.18, and `portable-pty` 0.8 → 0.9.
- **README refactor**: Reduced README.md by 32% (1019 → 690 lines). Replaced the changelog section with a link to CHANGELOG.md and moved key binding details to docs/KEYBINDINGS.md.

### Improved

- **nexterm-client-gpu module split**: Extracted 5 modules from `renderer.rs` (5553 lines) to improve maintainability.
  - `glyph_atlas.rs` — GlyphAtlas, BgVertex, TextVertex, GlyphKey
  - `shaders.rs` — WGSL shader constants
  - `color_util.rs` — ANSI 256-color and hex color conversion utilities
  - `key_map.rs` — winit keycode ↔ proto keycode conversion
  - `vertex_util.rs` — Rectangle, text, URL, and grid → text conversion utilities
- **Rustdoc expansion**: Added documentation comments to all public APIs in `nexterm-proto` (messages, types, enums). Enabled `#![warn(missing_docs)]`.
- **unsafe SAFETY comments**: Documented safety rationale for `SO_PEERCRED`/`getpeereid` in `nexterm-server/ipc.rs` and `libc::kill` in `pane.rs`.
- **Clippy warnings resolved**: Resolved all Clippy warnings across the workspace. Now compliant with CI's `-D warnings` flag.

---

## [0.9.4] - 2026-04-14

### Fixed

- **PowerShell crash fix**: Replaced direct array index accesses in `nexterm-vt`'s `erase_in_line`, `erase_in_display`, and `scroll_up` with the safe `Grid::clear_row()` / `Grid::copy_row()` methods. Prevents IndexError panics caused by complex VT sequences sent by PSReadLine.

### Added

- **Settings panel mouse interaction**: Sidebar categories, font size/opacity sliders, and theme color dots can now be clicked and dragged with the mouse. Sliders auto-save on drag release. Clicking outside the panel closes it.

### Changed

- **Terminal background transparency**: The terminal background is now 95% opaque by default (`background_opacity = 0.95`), giving a subtle see-through effect. The settings panel and context menu always remain fully opaque. Adjustable between 0.1 and 1.0 via `[window] background_opacity` in `nexterm.toml`.
- **Memory usage reduction**: Changed `cosmic-text`'s `FontSystem` initialization from a full system scan to loading only OS-specific font directories (macOS: `/System/Library/Fonts`, Windows: `C:\Windows\Fonts`). Estimated ~30–40 MB memory reduction.

---

## [0.8.0] - 2026-04-06

### Added

**Web Terminal: OAuth2 / SSO authentication**
- OAuth2/OIDC support for GitHub, Google, Azure AD, and any generic OIDC provider.
- Authorization Code Flow with CSRF protection (state parameter, 10-minute TTL).
- Access control via `allowed_emails` and `allowed_orgs` (GitHub only).
- Client secret can be set via `NEXTERM_OAUTH_CLIENT_SECRET` environment variable (recommended over storing in `nexterm.toml`).
- OAuth login button automatically injected into the login page when OAuth is enabled.

**Web Terminal: session management improvements**
- Configurable session TTL via `[web.auth] session_timeout_secs` (default: 86400 s = 24 h).
- Concurrent session limit via `[web] max_sessions` (0 = unlimited); oldest session is evicted when limit is reached.
- Explicit logout endpoint: `POST /auth/logout` revokes the session cookie.

**Web Terminal: HTTPS enforcement**
- New `[web] force_https = true` option; checks `X-Forwarded-Proto` and issues 301 redirects for HTTP requests (useful behind a TLS-terminating reverse proxy).

**Web Terminal: access log**
- New `[web.access_log]` section; logs every request (including WebSocket upgrades and failed auth attempts).
- CSV output to a configurable file path, or to the server log via `tracing` when no file is set.
- Fields: `timestamp`, `remote_addr`, `method`, `path`, `status`, `auth_method`, `user_id`.

**TUI client: multi-pane support**
- Ctrl+B prefix key system for pane management.
- Horizontal/vertical split, focus cycling, pane close, zoom.
- Status bar showing active session and pane count.
- Full help overlay (Ctrl+B ?).

**SSH host manager enhancements**
- Tag-based filtering and group management.
- Connection history with frequency-based sorting.
- Bulk operations (connect all in group, disconnect all).

**WASM plugin examples**
- Three ready-to-build sample plugins: `error-detector`, `command-counter`, `timestamp-injector`.
- Full plugin documentation including C and Rust examples.

**Documentation**
- Quickstart guide improvements, configuration snippet collection, Lua macro recipe collection.
- Full web terminal authentication reference including enterprise GitHub SSO example.

### Changed

- `[web.auth]` now contains `session_timeout_secs` field (previously hardcoded to 24 h).
- `[web]` has new fields: `max_sessions`, `force_https`, `access_log`.
- `nexterm-config`: `OAuthConfig` and `AccessLogConfig` are now publicly exported.

---

## [0.7.6] - 2026-04-06

### Added

**GPU client: TUI-parity tab bar and settings panel**
- Tab bar now displays the OSC 0/2 window title (e.g. current working directory) in each tab label, matching the TUI client behaviour.
- "⚙ Settings" button rendered on the right side of the tab bar; clicking it toggles the settings panel without a keyboard shortcut.
- Mouse click hit-testing on tab bar: clicking a tab switches the active pane; clicking the settings button opens/closes the panel.
- Settings panel font family field is now fully editable: press **F** (on Font tab) to enter edit mode, type the family name, **Backspace** to delete, **Enter** to confirm, **Escape** to cancel. Characters are intercepted before forwarding to the server.
- `PaneState` carries a `title: String` field updated by `ServerToClient::TitleChanged` messages.
- `ClientState` carries `tab_hit_rects` and `settings_tab_rect` populated each frame by `build_tab_bar_verts`.

### Changed

- `render()` and `build_tab_bar_verts()` now take `&mut ClientState` to allow per-frame hit-rect writes.

### Documentation

- All documentation converted to English as the primary language.
- Japanese translations added for user-facing docs: `shaders.ja.md`, `performance.ja.md`, `graphics.ja.md`, `plugins.ja.md`.
- `docs/ARCHITECTURE.md` and `docs/CONFIGURATION.md` fully translated to English.

---

## [0.7.5] - 2026-04-06

### Fixed

**GPU client: rendering quality pass**
- Added status bar height (`cell_h`) to the `visible_rows` calculation in scrollback view, fixing overlap between the last row and the status bar.
- `ScaleFactorChanged` (DPI change) event now recalculates `cols`/`rows` and sends a Resize notification to the server, resolving layout shift when moving to a high-DPI display.
- Applied `tab_bar_h` offset to the right-click context menu y-coordinate, fixing menu position when the tab bar is enabled.
- Added `cleared_this_frame` flag to `GlyphAtlas`; resetting the flag at the start of each frame prevents glyph corruption from stale UV coordinates after an atlas overflow mid-frame.
- Pre-declared `family_owned` for all code paths in `font.rs` to clarify lifetime structure.

---

## [0.7.4] - 2026-04-06

### Fixed

**GPU client (Windows): fix CJK full-width character spacing**
- Added `wide: bool` parameter to `rasterize_char()`; full-width characters (Unicode width ≥ 2) now render into a 2-cell buffer (`display_cols = 2.0`).
- Added `wide` field to `GlyphKey` so full-width and half-width glyphs are cached separately in the atlas.
- Japanese, Chinese, Korean, and other CJK characters are now evenly spaced and correctly rendered.

**GPU client (Windows): fix tab bar / terminal content overlap**
- Fixed tab bar (at y=0) and row-1 terminal content being drawn at the same y-coordinate.
- Added `y_offset: f32` parameter to `build_grid_verts` / `build_scrollback_verts`.
- Multi-pane `_in_rect` functions updated to use `off_y = row_offset * cell_h + tab_bar_h`.
- Pane borders and number badges now account for the tab bar height.

**GPU client (Windows): fix black band on the right side**
- The `rows` calculation was using the full window height, causing overlap with the tab bar and status bar.
- Fixed with `rows = (height - tab_bar_h - status_bar_h) / cell_h` for accurate usable row count.
- Corrected in both the initial window setup and resize event handler.
- Mouse click → cell coordinate conversion now subtracts `tab_bar_h` for accurate row targeting.

---

## [0.7.3] - 2026-04-06

### Fixed

**GPU client (Windows): fix font character spacing**
- `Attrs::new()` defaulted to `Family::SansSerif`, causing fallback to a proportional font (Segoe UI, etc.) on Windows.
- `measure_char_width` and `rasterize_char` now explicitly set `Family::Monospace` or `Family::Name(family)`.
- Config font name `"monospace"` maps to `Family::Monospace` (fontdb selects the system monospace font); specific names (`Consolas`, `JetBrains Mono`, etc.) use `Family::Name` directly.
- Cell width measurement switched from `Buffer::draw()` ink pixels to `layout_runs()` advance width, which includes right bearing for accurate character spacing.
- Eliminates the "Wi ndows PowerShe l l" extra-space rendering bug.

**Shader hot-reload, gallery, and migration tools**
- Added `WgpuState::reload_shader_pipelines()`: hot-reloads WGSL shaders on file change (no restart needed).
- `examples/shaders/`: bundled sample WGSL shaders — CRT, Matrix, Glow (background) / Grayscale, Amber (text).
- `nexterm-ctl import-ghostty`: imports a Ghostty config file and converts it to nexterm config.
- `nexterm-ctl service install/uninstall/status`: manages autostart services via systemd (Linux) / launchd (macOS).

---

## [0.7.2] - 2026-04-05

### Added

**Custom WGSL shader support**
- Added `[gpu]` section to `nexterm-config` (`custom_bg_shader` / `custom_text_shader` / `fps_limit` / `atlas_size`).
- GPU client loads WGSL files from the specified paths at startup (falls back to built-in shaders on failure).
- Enables custom effects such as CRT scanlines and glow.

**Documentation site expansion**
- `docs/src/features/graphics.md`: Sixel / Kitty graphics protocol guide.
- `docs/src/features/plugins.md`: WASM plugin development guide (with Rust sample code).
- `docs/src/advanced/shaders.md`: custom WGSL shader reference and examples.
- `docs/src/advanced/performance.md`: performance tuning guide.

### Performance

**GPU buffer reuse for rendering optimization**
- Added reusable vertex/index buffers to `WgpuState`.
- Replaced per-frame `create_buffer_init` (GPU allocation) with `queue.write_buffer` overwrites.
- Buffers are only reallocated (2× size) when capacity is exceeded; no reallocation in normal operation.
- GPU allocation count for an 80×24 terminal drops from **~4 per frame → 0 per frame**.

**FPS cap**
- `gpu.fps_limit` (default 60 FPS) controls the frame rate.
- Set to 0 for uncapped (vsync only).

**ASCII glyph pre-warming**
- ASCII printable characters (0x20–0x7E) are pre-loaded into the glyph atlas at startup in both Regular and Bold.
- Eliminates first-keystroke rasterization latency.

**Launcher startup time optimization**
- Changed `wait_for_server` polling to exponential backoff (10 ms, 10 ms, 10 ms, 20 ms, 50 ms, 100 ms).
- Average server-ready detection time reduced from **100 ms → ~30 ms** when the server starts quickly.

---

## [0.7.1] - 2026-04-05

### Fixed

**Fix ad-hoc codesign failure on macOS Intel builds**
- Signing individual binaries before signing the whole app bundle caused a subcomponent error.
- Changed to a single `codesign --force --deep --sign - dist/Nexterm.app` for the full bundle.

---

## [0.7.0] - 2026-04-05

### Added

**Floating panes**
- Added `OpenFloatingPane` / `CloseFloatingPane` / `MoveFloatingPane` / `ResizeFloatingPane` IPC commands.
- Added `FloatRect` cache and `floating_pane_rects` field to the GPU client.

**WASM plugin system**
- New `nexterm-plugin` crate (wasmi 0.38-based sandboxed WASM runtime).
- Built-in plugin API: `nexterm_on_output`, `nexterm_on_command`; host imports: `nexterm.log`, `nexterm.write_pane`.
- Added `plugin_dir` / `plugins_disabled` fields to config.

**Status bar widget enhancements**
- Built-in widgets: `"time"`, `"date"`, `"hostname"`, `"session"`, `"pane_id"`.
- Added `right_widgets` (right-aligned) and `separator` fields to `StatusBarConfig`.
- `WidgetContext` now passes session name and pane ID to widgets.

**Linux packaging**
- `linux/AppRun`: AppImage entry-point script.
- `pkg/flatpak/`: Flatpak manifest + AppStream metadata.
- Added AppImage build and upload step to GitHub Actions.
- `.github/workflows/flatpak.yml`: dedicated Flatpak build workflow.

**Test coverage improvements**
- Total test count: 145 → 178 (+33 tests).
- New tests in: nexterm-proto, nexterm-client-tui, nexterm-vt, nexterm-config, nexterm-plugin.

---

## [0.6.0] - 2026-04-05

### Added

**Four new built-in color schemes (Catppuccin / Dracula / Nord / One Dark)**

- Added `Catppuccin`, `Dracula`, `Nord`, and `OneDark` to `BuiltinScheme` in `nexterm-config`.
- Defined full fg/bg/ANSI[16] color palettes for all 9 schemes; reflected in the GPU renderer's terminal drawing.
- Settings panel (`[Colors]` tab) expanded to show all 9 scheme dots.

**Shell completion script generation**

- Added `nexterm-ctl completions <shell>` command.
  Outputs completion scripts for bash / zsh / fish / powershell / elvish to stdout.

**Man page generation**

- Added `nexterm-ctl man` command.
  Outputs a troff-format man page to stdout (`nexterm-ctl man > nexterm-ctl.1` to save).

**Bracketed paste mode (DEC ?2004)**

- VT parser now interprets `CSI ?2004h` / `CSI ?2004l` to track bracketed paste mode.
- When the mode is active, pasted text is wrapped with `ESC[200~` … `ESC[201~` before sending to the PTY.
  Prevents accidental command execution in zsh, fish, vim, and other shells/editors.

**Auto-load `~/.ssh/config`**

- Host Manager (`Ctrl+Shift+H`) now parses `~/.ssh/config` at startup and merges entries with `[[hosts]]`.
- `Host *` wildcards are excluded. Duplicate entries (same host + port already in `nexterm.toml`) are suppressed.

**Vim-compatible copy mode keys**

- `w` / `b`: word-wise forward / backward movement.
- `$`: jump to end of line.
- `Y`: yank the entire current line and exit copy mode.
- `/`: incremental search mode (Enter to confirm, n for next match, Esc to cancel).

**OSC 8 hyperlink support**

- Added `Grid.hyperlinks: Vec<HyperlinkSpan>` to `nexterm-proto`.
- VT parser interprets `ESC ] 8 ; ; <url> BEL` … `ESC ] 8 ; ; BEL` and records spans in the grid.
- GPU client's URL click (`Ctrl+Click`) now detects OSC 8 links first.

**Tab/pane activity notification**

- When output arrives in an unfocused pane, its tab shows an orange background and a `●` indicator.

**Mouse reporting (SGR ?1006 / X11 ?1000)**

- VT parser interprets `CSI ?1000h` / `CSI ?1006h` to track mouse modes.
- GPU client mouse clicks and drags are sent to the PTY as SGR escape sequences.
- Added `ClientToServer::MouseReport` message to `nexterm-proto`.

**Scrollback search UI completed**

- Added `Scrollback::search_prev()`. `Shift+Enter` or `Shift+N` moves to the previous match.
- Improved search bar UI: cursor `|`, accent line, key hint display.

**OSC 133 semantic zones**

- VT parser interprets `ESC ] 133 ; A/B/C/D BEL` to track prompt / command / output boundaries.
- Exit code of a completed command (D mark) is shown in the status bar (non-zero only).
- Added `ServerToClient::SemanticMark` message to `nexterm-proto`.

**Profiles (named configuration sets)**

- Added `Profile` struct and `Config.profiles` / `Config.active_profile` to `nexterm-config`.
- `Profile` can override font, colors, shell, scrollback, and tab bar from the base config.
- `Config::effective()` returns the config with the active profile applied.
- `Config::activate_profile(name)` / `clear_active_profile()` control profile switching.

### Changed

- `nexterm-client-gpu`: Settings panel scheme selector now supports all 9 schemes.

### Tests

- `nexterm-vt`: added bracketed paste mode enable/disable tests; OSC 8 hyperlink and OSC 133 semantic zone tests (18 tests total).
- `nexterm-server`: added BSP 4-split layout, session management API, and SSH config parser tests.
- `nexterm-config`: added profile application and TOML parse tests (17 tests total).

---

## [0.5.5] - 2026-04-05

### Fixed

**Windows — GPU client font rendering fixed**

- Replaced the `cell_w = font_size * 0.6` fixed-ratio heuristic with actual advance width measurement
  by rasterizing the reference character `'0'` at runtime via `layout_runs()`.
  Eliminates extra spaces between characters ("Wi ndows Power She l l").
- Added `scale_factor: f32` to `FontManager::new()`; passes `window.scale_factor()` from winit
  so the physical font size is correctly computed for high-DPI displays (125 %, 150 % scaling).
- Fixed a negative-coordinate wrap bug (`x as u32`) in the `rasterize_char` closure;
  added `if x < 0 || y < 0 { return; }` guard.
- `WindowEvent::ScaleFactorChanged` is now handled: font and glyph atlas are automatically regenerated on DPI change.

**Windows 11 — Acrylic frosted-glass background**

- Calls `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMWCP_ACRYLIC)` to apply
  a frosted-glass effect to the window background, similar to Windows Terminal.
- wgpu Surface composite alpha mode set to `PreMultiplied` for correct transparent blending.
- No effect on Windows 10 or non-Windows platforms; code is `#[cfg(windows)]`-guarded.

---

## [0.5.4] - 2026-04-05

### Fixed

**Windows — console window no longer appears on launch**

Added `#[windows_subsystem = "windows"]` (release builds only) to `nexterm.exe`,
`nexterm-server`, and `nexterm-client-gpu`. Launching `nexterm.exe` from the MSI installer
or Explorer no longer opens a stray black console window.

- Logs are written to `%LOCALAPPDATA%\nexterm\nexterm-server.log` / `nexterm-client.log`
  with daily rotation (`tracing-appender`).
- Errors are reported via `MessageBoxW` dialogs.

**macOS — binaries are ad-hoc signed + Intel Mac support**

- All macOS release binaries are now signed with `codesign --sign -` (ad-hoc).
  `xattr -dr com.apple.quarantine <file>` is all that's needed to bypass Gatekeeper.
- Built `x86_64-apple-darwin` target on the `macos-13` (Intel) runner;
  `nexterm-vX.Y.Z-macos-x86_64.tar.gz` is now included in release assets.

---

## [0.5.1] - 2026-03-31

### Fixed — Windows build & test (4 bugs)

This patch release fixes compilation and test failures that prevented the
Windows binary from being produced in the v0.5.0 release workflow.

| # | Crate / file | Root cause | Fix |
|---|---|---|---|
| 1 | `nexterm-launcher/Cargo.toml` | `windows-sys 0.59` split `CreateFileW` security descriptor handling into a separate `Win32_Security` feature; the feature was missing from the dependency declaration | Added `"Win32_Security"` to the `windows-sys` features list |
| 2 | `nexterm-launcher/src/main.rs` | `GENERIC_READ` was imported from `Win32::Storage::FileSystem`; in `windows-sys 0.59` it was moved to `Win32::Foundation` | Moved `GENERIC_READ` (and `INVALID_HANDLE_VALUE`) to the `Win32::Foundation` use statement |
| 3 | `nexterm-server/src/pane.rs` | `portable_pty` imports were guarded with `#[cfg(unix)]`, preventing `MasterPty`, `NativePtySystem`, `PtySize`, and `CommandBuilder` from being compiled on Windows even though `portable_pty` supports ConPTY on Windows | Removed the `#[cfg(unix)]` attribute from the `portable_pty` use statement |
| 4 | `nexterm-server/src/ipc.rs` | Path-validation unit tests used Unix-style absolute paths (`/home/user/…`, `/etc/passwd`, `/tmp/…`) which are **not** recognised as absolute by `std::path::Path::is_absolute()` on Windows, causing the "reject forbidden absolute paths" test to pass silently for the wrong reason | Added `#[cfg(unix)]` / `#[cfg(windows)]` guards; Windows tests use `%TEMP%\nexterm\…` and `D:\secret\…` / `C:\Windows\System32\…` style paths |

**All 93 unit tests now pass on `x86_64-pc-windows-msvc`.**

---

## [0.5.0] - 2026-03-27

### Added

**SSH & Connectivity**
- SSH multi-tab connections — SSH Host Manager (`Ctrl+Shift+H`) opens each host in a new tab
- X11 forwarding — `x11_forward = true` / `x11_trusted = true` in `[[hosts]]` (equivalent to `ssh -X` / `ssh -Y`)

**UX**
- In-app Settings GUI — `Ctrl+,` opens a Font / Colors / Window panel; changes write back to `nexterm.toml` instantly
- Settings action added to command palette (now 17 actions)

**Web Terminal**
- Embedded web terminal — `[web] enabled = true`; xterm.js served at `ws://localhost:7681`
- Token-based auth (`token = "..."` in config), disabled by default

**Package Distribution**
- Homebrew tap formula (`pkg/homebrew/nexterm.rb`)
- Scoop bucket manifest (`pkg/scoop/nexterm.json`)
- winget manifest (`pkg/winget/mizu-jun.Nexterm.yaml`)
- GitHub Pages documentation site auto-deployed via CI

---

## [0.4.0] - 2026-01-15

### Added

**SSH & Connectivity**
- SSH Host Manager — fuzzy-searchable host list (`Ctrl+Shift+H`); connects with one keystroke
- SFTP Upload / Download dialogs (`Ctrl+Shift+U` / `Ctrl+Shift+D`) with live progress bar
- Remote port forwarding (`-R`) over SSH sessions
- Serial port connections (`ConnectSerial` via command palette)

**UX & Pane Management**
- Command palette (Ctrl+Shift+P) extended with 16 actions including SFTP and host manager
- Lua Macro Picker — fuzzy-searchable macro list (`Ctrl+Shift+M`); one-key execution
- Quick Select mode (`Ctrl+Shift+Space`) — highlight URLs, paths, IPs, and hashes
- Pane zoom toggle (`Ctrl+B Z`) — focus a single pane full-screen
- Swap pane with next/previous sibling (`Ctrl+B {` / `Ctrl+B }`)
- Break pane to new window (`Ctrl+B !`)

**Automation**
- Lua event hooks: `on_session_start`, `on_attach`, `on_pane_open`
- Lua Macro engine: define `[[macros]]` in TOML, execute via picker

**Logging**
- Log filename templates (`{session}`, `{date}`, `{time}` placeholders)
- Binary PTY log mode

**Windows**
- MSI installer built with WiX Toolset v3 (CI-automated)
- Windows Service install/uninstall scripts
- Automatic code signing via `signtool.exe` when CI secrets are configured
- `nexterm-launcher` — single `nexterm.exe` auto-starts server + opens GPU client

---

## [0.3.0] - 2025-11-20

### Added

**SSH & Security**
- Known-hosts host key verification
- SSH agent authentication via `SSH_AUTH_SOCK`
- Local port forwarding through SSH tunnels
- ProxyJump multi-hop connection support
- SOCKS5 proxy support

**Terminal & Display**
- Full alternate screen buffer support (SMCUP/RMCUP)
- OSC 0/1/2 window title support
- OSC 9 desktop notifications
- CJK wide character rendering fixes

**GPU Client**
- IME input support (Japanese, Chinese, Korean)
- Keybinding customization
- Right-click context menu (Copy/Paste/Split/ClosePane)
- Pane number overlay in display-panes mode
- Mouse selection with automatic clipboard copy

---

## [0.2.0] - 2025-09-10

### Added
- GPU-accelerated renderer using wgpu + cosmic-text
- Command palette (`Ctrl+Shift+P`) with initial 8 actions
- Split pane: horizontal (`Ctrl+B %`) and vertical (`Ctrl+B "`)
- Scrollback buffer with configurable history size
- Basic session save / restore (JSON snapshots)

---

## [0.1.0] - 2025-07-01

### Added
- Initial release
- TUI client (`nexterm-client-tui`) using ratatui + crossterm
- IPC protocol between server and client (`nexterm-proto`)
- VT parser (`nexterm-vt`) with ANSI/xterm sequence support
- SSH client (`nexterm-ssh`) via `russh`
- TOML configuration (`nexterm-config`)
- i18n support for 8 languages (`nexterm-i18n`)
- `nexterm-ctl` CLI for session management
