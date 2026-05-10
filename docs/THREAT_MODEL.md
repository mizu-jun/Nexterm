# Nexterm 脅威モデル (STRIDE)

> **対象バージョン**: v1.0.2 時点
> **作成**: Sprint 4-3 (2026-05-10)
> **メソドロジー**: Microsoft STRIDE
> **関連文書**: [ARCHITECTURE.md](ARCHITECTURE.md) / [SECURITY.md](../SECURITY.md) / [SBOM.md](SBOM.md)

本書は nexterm に対する想定脅威を STRIDE 6 カテゴリで体系化し、現状の対策と残存リスクを可視化することを目的とする。

| 略号 | カテゴリ | 説明 |
|------|---------|------|
| **S** | Spoofing | なりすまし |
| **T** | Tampering | データ改ざん |
| **R** | Repudiation | 否認 |
| **I** | Information Disclosure | 情報漏洩 |
| **D** | Denial of Service | サービス拒否 |
| **E** | Elevation of Privilege | 権限昇格 |

---

## 1. システム概要と信頼境界

### 1.1 アクター

| アクター | 信頼レベル | 説明 |
|---------|----------|------|
| ローカルユーザー | Trusted | nexterm を起動した OS ユーザー（UID/SID） |
| 同一ホスト上の他ユーザー | Untrusted | 他のローカルアカウント・サービス |
| PTY 子プロセス（Shell・コマンド出力） | Semi-trusted | ユーザー権限で動くが出力は外部由来になりうる |
| SSH リモートホスト | Untrusted by default | 接続先の出力は信頼できない（ホスト鍵検証で初期信頼を確立） |
| Web ターミナル接続元 | Untrusted | ブラウザ経由の接続 |
| プラグイン (WASM) | Sandboxed | サードパーティ製コードの可能性 |
| GitHub (Update / SBOM 検証) | External | minisign / SLSA で完全性検証 |

### 1.2 信頼境界（データフロー観点）

```
                                    ┌─────────────────┐
                                    │ GitHub Releases │
                                    │ (signed by     │
                                    │  maintainer)   │
                                    └────────┬────────┘
                                             │ HTTPS + minisign
                                             ▼
┌──────────────────────────┐  IPC      ┌─────────────────┐  PTY  ┌─────────┐
│ Client (nexterm-client-  │◀════════▶│ nexterm-server  │◀═════▶│  Shell  │
│ gpu / nexterm-client-tui)│ (1)      │                 │ (2)   │  (子)   │
└──────────────────────────┘          │                 │       └─────────┘
        │                              │                 │
        │ (6) GitHub API               │                 │  SSH  ┌─────────┐
        │                              │                 │◀═════▶│ Remote  │
        ▼                              │                 │ (3)   │  Host   │
   Update                              │                 │       └─────────┘
   Checker                             │                 │
                                       │                 │  WS   ┌─────────┐
                                       │                 │◀═════▶│ Browser │
                                       │                 │ (4)   └─────────┘
                                       │                 │
                                       │                 │ WASM  ┌─────────┐
                                       │                 │◀═════▶│ Plugin  │
                                       │                 │ (5)   └─────────┘
                                       └────┬────────────┘
                                            │
                                ┌───────────┼───────────┐
                                ▼           ▼           ▼
                          config.toml  snapshot   recordings
                          + Lua (7)    .json (8)  *.log (9)
```

各境界に番号 (1)〜(9) を付与し、後続セクションで個別に分析する。

---

## 2. 境界別 STRIDE 分析

### 2.1 境界 1: クライアント ↔ サーバー (ローカル IPC)

**通信路**: Unix Domain Socket (`$XDG_RUNTIME_DIR/nexterm.sock`) または Windows 名前付きパイプ (`\\.\pipe\nexterm-<USERNAME>`)
**プロトコル**: 4 バイト LE プレフィックス + bincode (`nexterm-proto`)
**信頼方向**: 双方向（同一 UID 内）

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | 別ユーザーが他人のソケット/パイプに接続して PTY を盗聴 | Unix: `SO_PEERCRED` / `getpeereid` で UID を検証し他 UID を拒否（`nexterm-server/src/ipc/platform.rs`）。Windows: 名前付きパイプの DACL を作成ユーザー限定で設定。Hello メッセージで `client_kind` / `version` を交換 | root 権限を持つ攻撃者は OS 機能で迂回可能（OS 自体の信頼前提） |
| **T** | bincode メッセージを途中で書き換え | 同一プロセスが UDS/Pipe を介すローカル通信のみのため経路改ざんは想定しない。ただし悪意あるクライアントが不正な構造体を送る可能性は (E) で対処 | — |
| **R** | クライアント側からのコマンドが記録されない | Web 経由のアクセスは `access_log` に CSV で記録（ローテーション対応、Sprint 3-3）。ローカル IPC については操作粒度のログは未取得 | ローカル操作の監査ログは別途必要であれば追加検討 |
| **I** | PTY 出力が他プロセスに漏洩 | UID 検証で同一ユーザーのみ接続可能。tmpfs や `$XDG_RUNTIME_DIR` のパーミッションは OS が 0700 等で保護 | スワップ・コアダンプ等への漏洩は OS レイヤーの対応（mlock 等）で別途対処 |
| **D** | クライアントが巨大メッセージを送り OOM を誘発 | Hello 直後に `validate_msg_len()` で 4 バイトの長さプレフィックスを検証し、上限超を切断。受信タスクは `tokio::io::AsyncReadExt::read_exact` 単位で順次処理（Sprint 1, B1） | 上限値の妥当性は今後のテレメトリで再評価 |
| **E** | クライアントが `RecordSession` で任意パスに書き込み | `dispatch_util::validate_recording_path()` が `allowed_recording_dirs()` 配下のみ許可（Sprint 2-2 Phase A）。シンボリックリンク経由の脱出も `canonicalize` で防止 | recording ディレクトリ自体に他ユーザーが書き込めないことを前提（OS パーミッション） |

**評価**: 設計上の信頼境界としては妥当。同一 UID 配下のプロセスは 1 つの信頼ドメインとみなす（標準的な Unix モデル）。

---

### 2.2 境界 2: サーバー ↔ PTY (子プロセス)

**通信路**: portable-pty (Linux: openpty / Windows: ConPTY)
**信頼方向**: サーバー → 子プロセス（コマンド送信）/ 子プロセス → サーバー（出力受信）

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | 別ユーザーの PTY マスターを乗っ取り | PTY は `fork+exec` 直後にスレーブが子プロセスへ渡る。マスター fd はサーバー内に保持し、リーダースレッドが排他処理 | カーネル PTY 実装の信頼に依存 |
| **T** | 子プロセスが書き出した出力を VT パーサが誤解釈 | `nexterm-vt` で VT100 / OSC / DCS / APC を厳密にパース。APC オーバーフロー対策あり (`nexterm-vt/src/lib.rs`)。`cargo-fuzz` で 4 ターゲット日次実行（Sprint 3-5） | 全 VT シーケンスの仕様網羅は不可能（既知バグは fuzz で発見・修正） |
| **R** | 子プロセスが何を出力したか証跡がない | `RecordSession` IPC で recording ファイルにフレーム書き込み可能（オンデマンド）。常時ログ取得はオプション | 操作監査が必要な環境では起動時に recording 必須化を検討 |
| **I** | OSC 52 (クリップボード書き込み) でユーザーの注意を引かずに機密データを取得される | Sprint 4-1 で OSC 52 / OSC 9 / OSC 777 / URL オープンに同意ダイアログを実装（`SecurityConfig` で `prompt`/`allow`/`deny`）。OSC 52 は読み出し要求 (`?`) を拒否、書き込みも 1 MiB 上限 | 書き込み限定の透過処理を `allow` に設定すると同意なしで通過する（運用ポリシーで管理） |
| **D** | 子プロセスが巨大 OSC / DCS / Sixel 画像を送り続ける | OSC は 16 MiB で打ち切り、APC は環境変数で上限を設定可能。Sixel デコードは `image::decode_sixel()` でサイズ制限つき（fuzz ターゲット `sixel_decode` でクラッシュ耐性確認） | 画像が極めて多いセッションでメモリ使用量が増加（運用上の上限設定で対処） |
| **E** | 子プロセスがエスケープシーケンスでサーバープロセスに任意コード実行を誘発 | VT パーサは Rust 安全な `vte` クレート + `nexterm-vt` のラッパー。`unsafe` ブロックは限定的でコメント必須（`rules/rust/security.md` 準拠） | 依存クレートの未発見バグは `cargo audit` / `cargo deny` で監視 |

**評価**: 子プロセス出力は「半信頼」として扱い、表示前にサニタイズ・上限・同意 UI で防御。Sprint 4-1 での同意ダイアログ追加によりユーザー操作の透明性が大きく向上した。

---

### 2.3 境界 3: サーバー ↔ SSH リモート (`nexterm-ssh`)

**ライブラリ**: russh 0.60（GHSA-f5v4-2wr6-hqmg pre-auth DoS 修正済）
**信頼方向**: サーバー（クライアントとして）→ リモートホスト（信頼度はホスト鍵検証で確立）

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | 中間者攻撃で偽サーバーに接続 | ホスト鍵検証必須。known_hosts ファイル相当の信頼確立フローを実装。SSH agent 認証時は `request_identities()` の鍵を russh 0.60 API で検証 | 初回接続時の TOFU (Trust On First Use) は SSH 一般の問題 |
| **T** | パスワード/鍵が改ざんされる | クライアント側 `PasswordModal` で `Zeroizing<String>` ラップ（Sprint 3-2）。OS keyring 統合（Service=`nexterm-ssh`、Account=`<user>@<host>`）。host_history.json にはパスワード非保存 | keyring が利用できない環境（CLI のみのサーバー等）では平文保存リスクあり（運用回避） |
| **R** | リモート操作の証跡がない | `access_log` (Web 経由のみ)。SSH 経由の操作ログは別途リモート側で取得が必要 | nexterm 自体は SSH クライアントとしての操作記録を持たない設計 |
| **I** | 鍵フィンガープリント・パスワードがメモリダンプから取得される | `Zeroizing<String>` で Drop 時にゼロクリア。プロセスメモリへのデバッガアタッチは OS の信頼境界 | コアダンプ抑制は OS / 起動時設定で別途実施推奨 |
| **D** | russh 0.59 までの pre-auth DoS (GHSA-f5v4-2wr6-hqmg) | 0.60 にアップデート済 | russh 新規発見の脆弱性は `cargo audit` / `cargo deny advisories` で検知 |
| **E** | SSH リモートからの出力で nexterm を経由したローカル権限昇格 | リモート出力は (2.2) と同じ VT パーサに通すため OSC 同意・サイズ上限が適用 | 同意 `allow` 設定下では透過のリスクあり |

**評価**: russh 0.60 へのアップグレードと keyring 統合により実用上の安全性は大幅に向上。nexterm 側からの公開鍵指紋検証 UI は今後の改善候補。

---

### 2.4 境界 4: サーバー ↔ Web ターミナル (axum WebSocket + xterm.js)

**通信路**: WSS (TLS) over HTTP/1.1 または HTTP/2
**認証**: トークン認証 + OAuth + TOTP (Sprint 1 で OAuth Org 検証バイパスを修正)

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | 認証トークンを推測 / 盗む | トークンは CSPRNG 生成。OAuth + TOTP の二段階認証 (`web/oauth.rs` + `web/otp.rs`)。OAuth Organization 検証バイパス (Sprint 1) 修正済 | トークン期限切れポリシーは設定値による |
| **T** | WebSocket フレームが途中で改ざんされる | TLS 必須（`web/tls.rs` で証明書ロード）。`web` 設定で TLS 必須化が可能 | TLS 設定誤り（HTTP 公開）は運用責任 |
| **R** | 不正アクセスを後追いできない | `web/access_log.rs` が CSV 形式で記録。クエリパラメータ除去（Sprint 3-3 前半）+ ローテーション (10 MiB / 7 世代 / gzip 圧縮、Sprint 3-3 後半) | ログ収集サーバーへの転送は外部設定 |
| **I** | クッキー / Authorization ヘッダーがログに出る | クエリ除去・標準ロガーに機密値を流さない設計 | エラー応答メッセージのカスタマイズは運用側で確認 |
| **D** | 大量同時接続で OOM | axum 標準のコネクション制限を使用。OS レベルの ulimit と組み合わせ | 専用 DDoS 対策はリバースプロキシ層に委譲 |
| **E** | Web 経由で任意コマンド実行 | 認証必須・トークン検証後にのみ PTY バインド | TOTP/OAuth の運用ポリシー（再利用防止・管理者鍵漏洩時のローテーション）は別途整備が必要 |

**評価**: Sprint 1 / Sprint 3-3 でクリティカル課題は解消。Web 機能を有効化しない場合 (`web` セクション未設定) はこの境界自体が存在しないため、ローカル運用は影響を受けない。

---

### 2.5 境界 5: サーバー ↔ プラグイン (WASM)

**ランタイム**: wasmi (純 Rust 実装、JIT なし)
**API**: `nexterm-plugin` の `PLUGIN_API_VERSION = 1`

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | 悪意あるプラグインがメタデータを偽装 | `nexterm_meta` エクスポートで名前/バージョンを取得。ロード時に `PluginManager` がパス記録 | プラグイン作成者の信頼確認はユーザー責任（署名検証は v2 で検討候補） |
| **T** | プラグインがホストメモリを書き換え | wasmi の WASM 線形メモリは独立。ホスト関数経由でしか相互作用できない | ホスト関数引数の検証は `nexterm-server` の `plugin_dispatch.rs` で実施 |
| **R** | プラグインの動作が記録されない | プラグインロード/アンロード/リロードは IPC 経由で発火するため `tracing` ログに残る | 詳細な API コール履歴は将来の v2 で検討 |
| **I** | プラグインが他プラグインのデータを読む | wasmi インスタンスは独立。`PluginManager` が `Arc<Mutex<...>>` で保護 | プラグイン間のメッセージ仕様は v2 で API 強化予定 |
| **D** | プラグインが無限ループ / 巨大メモリ確保 | `consume_fuel` で命令数上限、メモリは 256 page (=16 MiB) 上限 (Sprint 1) | fuel 上限の妥当性はユースケースで再評価 |
| **E** | プラグインがファイル操作・ネットワークを実行 | wasmi はデフォルトで I/O ホスト関数を提供しない。明示的に host bindings を追加する必要がある | Sprint 4-2 で API v2 設計時に「サニタイズ済み入力 / PaneId 許可リスト」を追加予定 |

**評価**: WASM サンドボックスとして基本要件は満たしている。プラグインエコシステム成熟前に v2 API（後方互換 graceful 降格付き）を導入する Sprint 4-2 で更に強化予定。

---

### 2.6 境界 6: クライアント ↔ Update Checker (GitHub Releases)

**通信路**: HTTPS to api.github.com / github.com
**認証**: 公開 API（GitHub PAT 不要）
**完全性**: minisign 公開鍵検証 + SLSA Build Provenance (Sprint 3-4)

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | DNS hijack / TLS 偽装で偽 release を配信 | minisign で全アーカイブを署名検証。公開鍵 `NEXTERM_MINISIGN_PUBLIC_KEY` はビルド時にバイナリへ埋め込み (option_env!)。検証失敗時は明確なエラー（運用後鍵設定で有効化） | 鍵未設定のビルドは検証スキップ（local dev ビルド向け） |
| **T** | バイナリが配信経路で改ざんされる | minisign + SLSA Provenance による多層検証。`gh attestation verify` で外部検証可能 | minisign 秘密鍵の漏洩時は鍵ローテーションが必要（`rules/common/secret-rotation.md` 参照） |
| **R** | 攻撃者がリリースを書き換えて履歴を偽装 | GitHub Releases の immutable history + Provenance アテステーション | リポジトリ管理者アカウントが侵害された場合は別途対応 |
| **I** | Update Checker が機密情報を送信 | バージョン文字列のみを GitHub API へ問い合わせ。トークンや UID は送信しない | テレメトリ追加時は事前評価が必要 |
| **D** | GitHub API レートリミット枯渇 | ポーリング間隔は起動 5 秒後に 1 回のみ（`auto_check_update = false` で無効化可能） | バーストアクセスがあっても認証なしレートリミット (60/h) 内 |
| **E** | 偽 release で任意コード実行 | minisign + SLSA で完全性保証。検証失敗時はダウンロードを破棄 | (S) と同じく鍵管理が前提条件 |

**評価**: Sprint 3-4 で sign-and-verify が完備。運用準備（鍵生成 + GitHub Variables/Secrets 登録、`project_sprint_progress.md` 末尾参照）が完了次第、初回リリースから有効化される。

---

### 2.7 境界 7: 設定ファイル (config.toml + Lua)

**保存場所**: `$XDG_CONFIG_HOME/nexterm/` または `%APPDATA%\nexterm\`
**ロード順**: デフォルト値 → config.toml → config.lua（Lua によるオーバーライド可）

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | 別ユーザーが config を書き換える | OS のファイルパーミッション（ユーザーホーム配下） | OS パーミッション破損時は信頼前提崩壊 |
| **T** | 設定値の改ざんによる挙動誘導 | 起動時にスキーマ検証 (`nexterm-config/src/schema/`)。型不一致は明確なエラー | スキーマ範囲内の悪意ある値（巨大バッファサイズ等）は (D) で対処 |
| **R** | 設定変更履歴が残らない | `arc-swap::ArcSwap<RuntimeConfig>` でリロード時刻はログ記録（Sprint 2-5） | 詳細な diff 記録は将来の改善候補 |
| **I** | パスワード等が config に書かれる | nexterm 設計上 config に機密データを書かない方針。SSH パスワードは keyring（Sprint 3-2）に分離 | ユーザーが Lua フックに直書きすれば漏洩可能（運用ポリシーでカバー） |
| **D** | 不正な Lua 関数で永久ループ | `mlua` ワーカーは専用 OS スレッドで実行。チャネル経由通信のためメインを阻害しない (`StatusBarEvaluator` は毎秒評価でキャッシュ済み値を即時返答) | Lua スクリプトが意図的に CPU 100% を消費するケースは要監視 |
| **E** | Lua からシステム呼び出し | `mlua` 設定で標準ライブラリを部分制限（Sprint 1 sandbox 強化）。OS コマンド実行はホスト関数経由のみ | サンドボックス境界の継続的な検証が必要 |

**評価**: ユーザー自身が config を編集する前提なので「ユーザー自身による誤設定」が主リスク。Sprint 1 での Lua sandbox 強化により悪意ある config からの権限昇格は防御済み。

---

### 2.8 境界 8: スナップショット永続化

**保存場所**: `$XDG_STATE_HOME/nexterm/snapshot.json` (Linux/macOS) / `%LOCALAPPDATA%\nexterm\snapshot.json` (Windows)
**スキーマ**: `SNAPSHOT_VERSION = 2`（v1 自動マイグレーション対応）

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | 攻撃者がスナップショットを偽装してセッション復元時に乗っ取り | OS のファイルパーミッション + 同一 UID 起動時のみ読み込み | (1.1) と同じく OS 信頼前提 |
| **T** | スキーマ不一致を悪用した DoS | バージョン検証 (`SNAPSHOT_VERSION` < 1 は拒否、v1→v2 マイグレーション) + Atomic write (Sprint 1 で確保) | 未来バージョンのスナップショットを古い nexterm で開いた場合は明確なエラー |
| **R** | セッション履歴の改ざんが検知できない | スナップショット自体は監査ログではない（最後の状態のみ保持） | 監査が必要なら `RecordSession` を別途使用 |
| **I** | コマンド履歴・パス情報が含まれる | OS のホームディレクトリ権限で保護 | バックアップソフトウェアによる外部送信は運用責任 |
| **D** | 巨大なスナップショットでサーバー起動が遅延 | パース上限・タイムアウト | スナップショット肥大化は実運用で再評価 |
| **E** | スナップショット復元時に任意コマンド実行誘導 | スナップショットには PTY 起動時の引数のみが含まれ、自動再起動はユーザー設定依存 | ユーザーが「常に復元」設定時は同意なしで起動するため、UI で明示済み |

**評価**: Sprint 1 で atomic write 化済み（一時ファイル → rename）。シンボリックリンク経由の上書き攻撃は OS 信頼前提で軽減。

---

### 2.9 境界 9: Recording ファイル

**保存場所**: ユーザー指定パス（デフォルトは `$XDG_DATA_HOME/nexterm/recordings/`）
**形式**: 独自フレーム形式（タイムスタンプ + バイト列）

| 脅威 | シナリオ | 既存対策 | 残存リスク |
|------|---------|---------|----------|
| **S** | 別ユーザーが recording パスをすり替え | `dispatch_util::validate_recording_path()` で許可ディレクトリ配下のみ書き込み (Sprint 2-2 Phase A) | 許可ディレクトリ自体のパーミッションは OS 依存 |
| **T** | recording ファイルが書き換えられる | OS パーミッション + 書き込み中はファイルロック（プラットフォーム依存） | アーカイブ後の改ざん検知が必要なら別途ハッシュ記録を推奨 |
| **R** | 「自分は記録していない」と否認される | recording 開始/終了は IPC 経由で `tracing` ログに残る | 詳細な操作監査が必要なら別途集約ログが必要 |
| **I** | recording がパスワード入力を含む | PTY のエコー出力をそのまま記録するため、パスワード入力中の文字も入る可能性 | UI で「パスワード入力フィールドで recording 中であることを警告」する改善余地 |
| **D** | 巨大 recording で disk full | 書き込みパスは許可ディレクトリのみ。ローテーションはユーザー責任 | 自動ローテーション機能は今後の改善候補 |
| **E** | recording 形式の解析時にバグでホスト権限取得 | 現在 nexterm 自体は recording の自動再生機能を提供しない（`nexterm-ctl record` のみ生成） | サードパーティ再生ツール使用時は別途検証必要 |

**評価**: パストラバーサル防御は Sprint 2-2 で完了。情報漏洩観点では「パスワード入力時に recording 中である表示」が UX 改善候補。

---

## 3. 優先度別残存リスクと対応計画

### 3.1 直近の Sprint で対処予定

| ID | 残存リスク | 対象 Sprint |
|----|----------|----------|
| RR-1 | プラグイン API v1 の入力サニタイズが限定的 | Sprint 4-2（API v2、PaneId 許可リスト、graceful 降格） |
| RR-2 | Sixel パーサ・BSP レイアウトの property test 不足 | Sprint 4-4（proptest 導入） |

### 3.2 中長期改善候補

| ID | 残存リスク | 対応案 | 優先度 |
|----|----------|------|------|
| RR-3 | minisign 公開鍵の運用準備（鍵生成 + Variables/Secrets 登録） | リリース運用ドキュメント化済（`project_sprint_progress.md` 末尾） | 高 |
| RR-4 | 同意ダイアログ `allow` 設定時の OSC 透過リスク | 設定 UI で「常に許可」のリスク警告を強化 | 中 |
| RR-5 | recording 中のパスワード入力警告 | TUI/GPU 両方で記録中インジケータを点滅表示 | 中 |
| RR-6 | Lua スクリプトの CPU 使用率監視 | `nexterm-lua-worker` でフレーム時間計測・閾値超過時の警告 | 低 |
| RR-7 | ローカル IPC の操作監査ログ | optional な audit log（`access_log` 拡張）として実装 | 低 |
| RR-8 | プラグイン署名検証 | API v2 + plugin signature manifest（cosign 等） | 低 |

### 3.3 受容済み（OS / 外部依存に委譲）

| ID | リスク | 受容理由 |
|----|------|---------|
| RA-1 | 同一 UID 内のプロセスは同一信頼ドメイン | Unix / Windows 標準モデル |
| RA-2 | OS root / SYSTEM は全境界を迂回可能 | OS 自体の信頼前提 |
| RA-3 | コアダンプ・スワップ経由の機密漏洩 | OS 設定（`prctl(PR_SET_DUMPABLE)` / `mlock`）で対応 |
| RA-4 | TLS 中間者（証明書ストア改ざん） | OS 標準のトラストストアを信頼 |
| RA-5 | GitHub アカウント侵害 | リポジトリ運用・2FA・ブランチ保護で対処 |

---

## 4. セキュリティ運用の継続的活動

| 活動 | 頻度 | ツール | Sprint |
|-----|------|------|--------|
| 依存ライセンス・脆弱性チェック | PR / push ごと | `cargo deny` (`deny.toml`) | 4-3 |
| RustSec Advisory DB 照合 | PR / push ごと | `cargo audit` | 既存 |
| ファジングテスト | 毎日 (UTC 03:00) | `cargo-fuzz` 4 ターゲット 60 秒並列 | 3-5 |
| SBOM 生成 | リリースタグごと | `cargo-cyclonedx` (`.github/workflows/sbom.yml`) | 4-3 |
| SLSA Build Provenance | リリースタグごと | `actions/attest-build-provenance@v2` | 3-4 |
| minisign 署名 | リリースタグごと（鍵設定後） | `minisign -S` | 3-4 |
| プロパティテスト | PR / push ごと（予定） | `proptest` | 4-4（予定） |

---

## 5. 用語集

| 用語 | 説明 |
|-----|------|
| **STRIDE** | Spoofing / Tampering / Repudiation / Information Disclosure / Denial of Service / Elevation of Privilege の頭文字。Microsoft が提唱した脅威分類フレームワーク |
| **TOFU** | Trust On First Use。初回接続時の鍵を信頼し、以降は鍵変更を警告するモデル（SSH known_hosts と同じ） |
| **SLSA** | Supply-chain Levels for Software Artifacts。サプライチェーン保証レベル（L1〜L4） |
| **CycloneDX** | OWASP が標準化した SBOM 形式 |
| **TOTP** | Time-based One-Time Password (RFC 6238) |
| **minisign** | OpenBSD 由来の軽量署名ツール（Ed25519 ベース） |

---

## 6. 改訂履歴

| 日付 | バージョン | 変更内容 | 担当 Sprint |
|------|---------|---------|-----------|
| 2026-05-10 | 1.0 | 初版作成（Sprint 1〜4-1 の対策を反映） | Sprint 4-3 |
