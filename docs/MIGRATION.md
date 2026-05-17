# Migration Guide

このドキュメントは Nexterm の互換性破壊を含むバージョンへ移行する際の手順をまとめます。

---

## v1.2.0 → v1.3.0（Sprint 5-7 Phase 2: IPC バリアント追加とスナップショット拡張）

`PROTOCOL_VERSION` が `4` から `7` にバンプされ、`SNAPSHOT_VERSION` が `2` から `3` に
バンプされました。**新サーバーは旧クライアントを Hello ハンドシェイクで拒否し、
旧サーバーは新クライアントを拒否します**。クライアントとサーバーは必ず一緒にアップグレードしてください。

### 何が変わるか

#### PROTOCOL_VERSION 5（Sprint 5-7 / Phase 2-1: ワークスペース機能）

`ClientToServer` に 5 バリアント、`ServerToClient` に 2 バリアントを追加:

- `ListWorkspaces` / `CreateWorkspace { name }` / `SwitchWorkspace { name }` /
  `RenameWorkspace { from, to }` / `DeleteWorkspace { name, force }`
- `WorkspaceList { workspaces, current }` / `WorkspaceSwitched { name }`

postcard は enum バリアント追加に対して**前方互換性を持たない**ため、旧クライアント
（PROTOCOL_VERSION 4）は新サーバーから送られる `WorkspaceList` をデコードできません。

#### PROTOCOL_VERSION 6（Sprint 5-7 / Phase 2-2: Quake モード）

`ClientToServer::QuakeToggle { action }` と `ServerToClient::QuakeToggleRequest { action }`
を追加。`action` は `"toggle"` / `"show"` / `"hide"` のいずれか。

#### PROTOCOL_VERSION 7（Sprint 5-7 / Phase 2-3: タブ並べ替え）

`ClientToServer::ReorderPanes { pane_ids: Vec<u32> }` を追加。サーバーは指定された
順序でタブを並べ替え、変更があった場合のみ `LayoutChanged` を送り返します。
`LayoutChanged.panes` 配列の順序の意味が「BSP DFS 順」から「論理タブ表示順」に
変わりますが、旧クライアントは順序に依存していなかったため動作には影響しません。

#### SNAPSHOT_VERSION 3（Sprint 5-7 / Phase 2-1）

`SessionSnapshot.workspace_name: String` と `ServerSnapshot.current_workspace: String`
を追加。**v2 JSON は `serde(default = "default_workspace")` で自動マイグレーションされる**
ため、既存スナップショットを手動で書き換える必要はありません。マイグレーション後、
全セッションは `"default"` ワークスペースに所属します。

### 移行手順

1. **クライアントとサーバーを同時にアップグレード**: PROTOCOL_VERSION ミスマッチで
   ハンドシェイクが失敗します
2. **既存スナップショット**: 自動マイグレートされるため操作不要。`~/.local/state/nexterm/snapshot.json`
   を読み込み時にバージョンが 2 → 3 へ更新される
3. **設定ファイル変更不要**: 新機能はすべてオプションで、既存の `config.toml` をそのまま使えます

### Quake モード（Wayland 制限）

`global-hotkey` 0.8 crate は Wayland のセキュリティモデル上、グローバルホットキーを
登録できません。Wayland 環境では `nexterm-ctl quake toggle/show/hide` を compositor の
キーバインド（Sway の `bindsym` / Hyprland の `bind` 等）から呼び出してください:

```
# Sway 例
bindsym Mod4+grave exec nexterm-ctl quake toggle

# Hyprland 例
bind = SUPER, grave, exec, nexterm-ctl quake toggle
```

X11 環境では `config.toml` の `[quake_mode] hotkey = "ctrl+\`"` でホットキーを直接登録できます。

### 背景画像（Phase 3-1）

背景画像は起動時のみロードされます。`config.toml` を変更した場合は再起動が必要です。

```toml
[window.background_image]
path = "~/wallpaper.png"
opacity = 0.3
fit = "cover"  # cover / contain / stretch / center / tile
```

### アニメーション（Phase 3-2）

新規ペイン追加・タブ切替時に ease-out アニメーションが既定で有効になります。
アクセシビリティ（reduced motion）の観点で動きを抑えたい場合は以下を設定してください:

```toml
[animations]
enabled = false                # 全アニメーション無効
# または
intensity = "off"              # off / subtle / normal / energetic
```

---

## v1.1.0 → Unreleased（Sprint 5-1 / G3: IPC ワイヤフォーマットを postcard へ移行）

`PROTOCOL_VERSION` が `2` から `3` にバンプされました。**新サーバーは旧クライアント
を Hello ハンドシェイクで拒否し、旧サーバーは新クライアントを拒否します**。
クライアントとサーバーは必ず一緒にアップグレードしてください。

### 何が変わるか

IPC のシリアライズフォーマットが `bincode` 1.x から `postcard` 1.x に変更されました。
両者はバイト列レベルで非互換です（postcard は varint エンコード、bincode は固定長）。

理由: `RUSTSEC-2025-0141` (bincode 1.x のメンテナンス終了) を解消し、
中長期のサプライチェーン健全性を確保するためです。

### ユーザー影響

- バイナリを揃えてアップグレードする以外、ユーザー側で必要な作業はありません。
- `nexterm-ctl` からも IPC 接続するため、CLI バイナリも同時更新が必要です。

### 副次的効果

- IPC メッセージが平均的に 10〜20% 程度小さくなる可能性があります（varint）。
- `bincode = "1"` を直接利用する third-party プラグインがある場合、
  そちらも `postcard` へ移行する必要があります（プラグイン API は WASM 経由
  なので通常は影響なし）。

---

## v1.1.0 → Unreleased（Sprint 5-1 / G1: SSH パスワード keyring 化）

`PROTOCOL_VERSION` が `1` から `2` にバンプされました。**新サーバーは旧クライアント
を Hello ハンドシェイクで拒否し、旧サーバーは新クライアントを拒否します**。
クライアントとサーバーは必ず一緒にアップグレードしてください。

### 何が変わるか

`ClientToServer::ConnectSsh` メッセージから `password: Option<String>`
フィールドが削除され、以下の 2 フィールドに置換されました:

- `password_keyring_account: Option<String>` — `<username>@<host_name>` 形式の
  キーリングアカウント識別子
- `ephemeral_password: bool` — `true` の場合、サーバーは認証完了後に
  keyring エントリを削除する

これにより、IPC 経路（Unix Domain Socket / Named Pipe）でパスワード平文が
流れなくなりました。クライアントが OS のキーリング（Service=`"nexterm-ssh"`）に
保存し、サーバーが同じユーザーの権限で取得します。

### 影響

- 旧 `nexterm-ctl` バイナリと新サーバーの混在は不可（IPC 互換性破壊）。
- パスワード認証の SSH ホストを利用する場合、サーバーホストとクライアントホストが
  **同じ OS ユーザー**で動作している必要があります（OS キーリングはユーザー単位
  でアクセス制御されるため）。
- OS キーリングサービスが利用できない環境（headless Linux で
  Secret Service 未起動など）ではパスワード認証 SSH 接続ができなくなります。
  鍵認証または `secret-tool`/`gnome-keyring`/`KWallet` 等のセットアップを推奨します。

### 移行手順

クライアント/サーバーバイナリを同時に v1.2.0+ へアップグレードするだけです。
追加設定は不要。`HostManager` のパスワードモーダルは「保存する/しない」UI を
継続利用できます。

---

## v1.0.2 → Unreleased（Sprint 4-2 プラグイン API v2）

`PLUGIN_API_VERSION` が `1` から `2` にバンプされました。**v1 プラグインは
引き続き動作しますが**、ロード時に deprecation 警告がログに出ます。
将来的に v1 サポートは削除予定（具体的な削除タイミングは未定）。

### v1 → v2 プラグイン移行手順

1. **`nexterm_api_version` エクスポートを `2` を返すように更新**:

   ```rust
   #[unsafe(no_mangle)]
   pub extern "C" fn nexterm_api_version() -> i32 {
       2
   }
   ```

2. **入力データの扱いを見直す**: v2 ではホスト側で ESC・OSC/CSI/DCS/APC・
   C0 制御文字（`\t\r\n` 除く）が事前除去されたサニタイズ済みバイト列が渡される。
   独自にエスケープシーケンスを観測・解析していたプラグインは挙動が変わる。

3. **`write_pane` の宛先 PaneId を制限**:
   - `nexterm_on_output(pane_id, ...)` 中: 渡された `pane_id` のみ書き込み可
   - `nexterm_on_command(...)` 中: どの pane にも書き込めない
   - 拒否されると warn ログが出るが、エラーにはならず処理は継続する

### v1 のまま運用する場合

`nexterm_api_version` を未エクスポート、または `1` を返すままにすれば動作します。
ただし以下の deprecation 警告が起動時にログに記録されます:

```
プラグインが API v1 で動作中（現行 v2）: <path> — サニタイズ・PaneId 検証なしの旧挙動で動作します。
将来のバージョンで v1 サポートは削除予定です。
```

---

## v1.0.2 → Unreleased（Sprint 1〜3 セキュリティ強化）

セキュリティ監査により、互換性破壊を含む 4 つの大きな変更があります。

### 1. プロトコル Hello メッセージ必須化（必ず影響）

**何が変わるか**

クライアント・サーバー間の IPC プロトコルにハンドシェイクメッセージが追加されました。
クライアントは接続後に `ClientToServer::Hello { proto_version, client_kind, client_version }` を最初に送信する必要があります。

**影響**

- 旧バージョン（v1.0.2 以前）のクライアントが新サーバーに接続すると、最初のメッセージが `Hello` ではないため**サーバーが接続を切断**します。
- 旧バージョンのサーバーに新クライアントを繋ぐと、サーバーが `Hello` を未知メッセージとして扱う可能性があります。

**対応**

- クライアントとサーバーを **同一バージョンで揃える**ことを必須とします。
- インストール時は GPU クライアント・TUI クライアント・`nexterm-ctl`・サーバーを一括更新してください。

```bash
# Cargo build による開発時のクリーン更新
cargo clean
cargo build --release --workspace

# パッケージマネージャ経由の場合
# Windows: msiexec で v1.0.2 をアンインストール → 新版をインストール
# Linux (Flatpak): flatpak update
```

`PROTOCOL_VERSION` は将来 `nexterm-proto/src/lib.rs` で管理されます。

---

### 2. Lua サンドボックスによる API 制限（既存 `config.lua` に影響）

**何が変わるか**

`config.lua` / Lua フック / マクロが実行する Lua インスタンスはサンドボックス化され、以下が**無効化**されました:

| 削除されたグローバル | 代替 |
|---|---|
| `os.execute` / `os.remove` / `os.rename` / `os.tmpname` 等の `os.*` | （現時点では代替なし、将来 `nexterm.*` 名前空間で限定的 API 提供予定） |
| `io.open` / `io.read` / `io.lines` 等の `io.*` | （現時点では代替なし） |
| `require` / `dofile` / `loadfile` / `load` / `loadstring` | 全 Lua コードは `~/.config/nexterm/nexterm.lua` 内にインライン記述 |
| `debug.*` | 削除 |
| `package.*` | 削除 |
| `collectgarbage` / `rawset` / `rawget` / `setfenv` / `getfenv` | 削除 |

利用可能なライブラリ: `string` / `table` / `math` / `coroutine`。

**影響**

旧 `config.lua` で以下のような記述があった場合、**ロード時にエラー**になります:

```lua
-- 旧: NG（os.execute はサンドボックスで無効）
hooks.on_pane_open = function(session, pane_id)
    os.execute("notify-send 'New pane opened'")
end

-- 旧: NG（io.write はサンドボックスで無効）
print("loaded at " .. os.date())
```

**対応**

1. **シェルコマンド呼び出し** はターミナルフック（`config.toml` の `[hooks] on_pane_open = "/path/to/script"`）を使用する。
2. **ファイル読み書き** は Lua 内で実行せず、外部スクリプト + ターミナルフック経由で行う。
3. **タイムスタンプ** は将来 `nexterm.now()` API が追加予定。それまでは UI 側のステータスバー `time` ウィジェット等を使用する。

`config.lua` を変更したくない場合の応急処置はありません。サンドボックスは無効化できない設計です（CRITICAL #4 対応のため）。

---

### 3. TLS フォールバック既定禁止（HTTPS 設定済みユーザーに影響）

**何が変わるか**

`[web] tls.enabled = true` を設定していて、証明書ファイルの読み込みに失敗した場合の挙動が変わりました。

| 旧（v1.0.2 以前） | 新（Unreleased） |
|---|---|
| 警告ログを出して **HTTP に自動降格** | **Web サーバー起動を中止** |

**影響**

証明書ファイルが存在しない / パーミッションエラー / フォーマット不正の場合、Web ターミナルが起動しなくなります（IPC は通常通り動作）。

**対応 (推奨)**

証明書ファイルパスを正しく設定するか、自己署名証明書の自動生成パスを使用する:

```toml
[web]
enabled = true
[web.tls]
enabled = true
# cert_file / key_file を省略 → ~/.config/nexterm/tls/ に自動生成
```

**対応 (テスト・開発のみ、推奨されない)**

明示的にオプトインで HTTP フォールバックを許可:

```toml
[web]
enabled = true
allow_http_fallback = true   # 警告: 平文でセッショントークンが流れる
[web.tls]
enabled = true
cert_file = "/path/to/cert.pem"   # 失敗しても起動を継続
```

---

### 4. OAuth Org メンバーシップ検証が**実際に**機能するように修正

**何が変わるか**

旧実装は `allowed_orgs` 設定を**実際には検証していませんでした**（`get_current_token()` のバグで Org チェックが絶対実行されない）。
新実装で正しく GitHub API でメンバーシップを検証するようになりました。

**影響**

| 旧実装の挙動 | 新実装の挙動 |
|---|---|
| `allowed_orgs` のみ設定 → **誰もログイン不可**（機能不全） | `allowed_orgs` のみ設定 → メンバーは許可、非メンバーは拒否（仕様通り） |
| `allowed_emails` + `allowed_orgs` 併用 → メール一致のみで通過、Org チェック完全スキップ | メール一致 OR Org メンバーシップで許可 |

旧実装に依存して「Org メンバーシップで二重防御」と思っていた管理者は、その前提が誤りだったことになります。**設定を見直してください**。

---

### 5. WASM プラグイン: fuel / メモリ制限導入（プラグイン作者に影響）

**何が変わるか**

- 各 `nexterm_on_output` / `nexterm_on_command` 呼び出し前に `10,000,000` 命令分の fuel が供給されます。fuel 枯渇でトラップ。
- プラグインの線形メモリは初期 `256 ページ (16 MiB)` を超えるとロード時に拒否されます。
- プラグインに `nexterm_api_version()` をエクスポートしている場合、ホストの `PLUGIN_API_VERSION` (= 1) と一致しないとロード拒否されます。

**影響**

通常のプラグインは影響を受けません。ただし以下に注意:
- 1 呼び出しで 1,000 万命令を超える重い処理を行うプラグインはトラップされる → 処理を分割するか、ホストへ要望
- 16 MiB を超える初期メモリを要求するプラグインはロード不能 → メモリ動的拡張に変更

---

### 6. 設定ファイル `config.toml` のキー名変更（旧テンプレート使用者に影響）

**何が変わるか**

初回起動時に生成されていた `DEFAULT_CONFIG_TOML` テンプレートが、実際には**実装と一致しないキー名**を使っていたため修正されました。

| 旧テンプレート（実装と不一致） | 新テンプレート（実装と一致） |
|---|---|
| `[color_scheme] builtin = "tokyonight"` | `colors = "tokyonight"` または `[colors] scheme = "tokyonight"` |
| `[tab_bar] show = true / position = "top"` | `[tab_bar] enabled = true / height = 28` |
| `[status_bar] show = true / position = "bottom"` | `[status_bar] enabled = true` |

**影響**

- 旧テンプレートの設定は元から効いていなかったため、**ユーザー体験には変化なし**（むしろ正しく効くようになる）
- カスタマイズ済みの `config.toml` は手動で新しいキー名に変更してください

**新たに設定可能になったセクション** （旧 `TomlConfig` で無視されていたもの）

```toml
[window]
background_opacity = 0.85
padding_x = 8
padding_y = 4

[[hosts]]
name = "production"
host = "192.168.1.100"
port = 22
username = "ops"
auth_type = "key"
key_path = "~/.ssh/id_ed25519"

[[macros]]
name = "git-status"
description = "Show git status"
lua_fn = "macro_git_status"

[web]
enabled = true
[web.auth]
totp_enabled = true

cursor_style = "block"        # "block" / "beam" / "underline"
auto_check_update = true
language = "auto"             # "auto" / "ja" / "en" / "fr" / "de" / "es" / "it" / "zh-CN" / "ko"
```

---

## トラブルシューティング

### サーバーに接続できない（"プロトコルバージョン不一致"）

クライアントとサーバーのバージョンが揃っていません。両方を最新版にアップデートしてください。

### Lua スクリプトのロードに失敗

`os.execute` / `io.open` / `require` を使用していないか確認してください。サンドボックスで無効化されています（[項目 2](#2-lua-サンドボックスによる-api-制限既存-configlua-に影響)）。

### Web ターミナルが起動しない

TLS 設定失敗時のフォールバックが既定禁止になりました（[項目 3](#3-tls-フォールバック既定禁止https-設定済みユーザーに影響)）。証明書設定を見直すか `allow_http_fallback = true` を設定してください。

### OAuth で突然ログインできなくなった

`allowed_orgs` 単独設定ユーザー: 旧実装では誰もログインできない状態だったため、これまで使えていない可能性があります。改めて Org メンバーシップ設定を確認してください（[項目 4](#4-oauth-org-メンバーシップ検証が実際に機能するように修正)）。

---

## サポート

問題があれば https://github.com/mizu-jun/Nexterm/issues に報告してください。
